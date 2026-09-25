use crate::AppState;
use crate::util::date::{days_ago_iso, days_between, is_iso_date, today_iso};
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

const MAX_RATE_LIMIT_RETRIES: u32 = 2;

// The OONI API rate-limits (HTTP 429) hard enough that firing requests
// concurrently just front-loads a wall of throttling. Every fetch is
// sequential and paced with this delay between requests — combined with
// get_with_retry's backoff as a safety net, this stays under the limit
// instead of tripping it and paying for backoff anyway.
//
// Every sweep now groups by country server-side (axis_x/axis_y = probe_cc), so
// the whole globe is a *handful* of requests per sweep rather than one per
// country — ~4 URLs + ~16 technologies + ~6 timeline techs + 1 category call.
// That is what makes covering all ~230 countries fit the startup time budget
// where a per-country loop (thousands of requests) never could.
const REQUEST_PACING: Duration = Duration::from_millis(2000);

// Generous on purpose. OONI generates aggregations server-side and a wide
// window over every country legitimately takes several seconds to come back —
// the widest (a web_connectivity timeline for all countries since 2024) is the
// most valuable response, so the ceiling is sized to tolerate it rather than
// fail fast on exactly the request that matters most.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

// Point-in-time status (technology_blocks, adoption_signals) is "is this
// blocked *now*", so it classifies from measurements in a trailing window
// rather than all of history — otherwise a tool blocked once in 2024 would read
// as blocked forever.
const POINT_IN_TIME_WINDOW_DAYS: i64 = 90;

// The per-content-category sweep is a current-state snapshot (all rows carry
// today's checked_date), like the technology blocks — so it reads a trailing
// window rather than all history. That also keeps the heavy 2-D category
// aggregation small enough that OONI's gateway doesn't time it out (504); the
// full-history version reliably 504s.
const CATEGORY_WINDOW_DAYS: i64 = 180;

// The historical timeline charts start here.
const TIMELINE_SINCE: &str = "2024-01-01";

/// Days of already-stored timeline re-fetched on an incremental sweep.
///
/// OONI measurements arrive late and get reclassified for a few days after
/// the fact, so resuming exactly at the newest stored date would freeze those
/// days at their first, least-confident value. `INSERT OR REPLACE` on
/// (country, technology, date) makes re-pulling them a correction rather than
/// a duplicate — the same self-healing overlap `cloudflare_http` relies on.
const TIMELINE_OVERLAP_DAYS: i64 = 7;

const TARGET_URLS: [&str; 4] = [
    "https://api.openai.com",
    "https://openai.com",
    "https://api.anthropic.com",
    "https://www.deepseek.com",
];

const AGGREGATION_ENDPOINT: &str = "https://api.ooni.io/api/v1/aggregation";

// Technologies to backfill a daily timeline (sparkline) for. Each is fetched
// for ALL countries in a single 2-D aggregation request (day x country), so —
// unlike the old hardcoded (country, technology) TIMELINE_TARGETS table — this
// list is technology-only and the country dimension comes from the response.
//
// This is now every technology in REGISTRY except the messaging apps, which
// have their own sidebar widget and no per-row sparkline to fill. It used to
// be six: every circumvention tool except `tor` itself, plus `openai.com`
// alone out of the four AI-access entries. That split had no reason behind it
// — `claude.ai`, `deepseek` and `huggingface` are `web_connectivity` against
// a URL exactly like `openai.com`, and `tor` is a dedicated nettest exactly
// like `psiphon` and `torsf` — so the sidebar showed a chart under some rows
// and nothing under their identical neighbours.
//
// Cost of the six additions, measured against the live API rather than
// guessed: tor 9.8s (17.9 MB, the largest response in the codebase),
// deepseek 8.4s (6.0 MB), claude.ai 8.0s (6.2 MB), tails 4.7s (258 kB),
// grapheneos 3.4s (213 kB), huggingface 3.4s (77 kB — OONI barely covers
// it). ~38s of network plus pacing, which is why the `ooni` budget in
// db/mod.rs went up alongside this.
const TIMELINE_TECHS: &[&str] = &[
    // Circumvention
    "tor",
    "torproject",
    "signal",
    "i2p",
    "psiphon",
    "torsf",
    // AI access
    "openai.com",
    "claude.ai",
    "deepseek",
    "huggingface",
    // Privacy OS
    "grapheneos",
    "tails",
];

// ── Aggregation response ───────────────────────────────────────────────────

// One flat cell shape covers every sweep. Which of `probe_cc` /
// `measurement_start_day` / `category_code` are populated depends on the
// axis_x/axis_y requested; the rest come back absent and default to None.
#[derive(Debug, Deserialize, Default)]
struct AggResponse {
    #[serde(default)]
    result: Vec<AggCell>,
}

#[derive(Debug, Deserialize, Default)]
struct AggCell {
    #[serde(default)]
    probe_cc: Option<String>,
    #[serde(default)]
    measurement_start_day: Option<String>,
    #[serde(default)]
    category_code: Option<String>,
    #[serde(default)]
    anomaly_count: i64,
    #[serde(default)]
    confirmed_count: i64,
    #[serde(default)]
    failure_count: i64,
    #[serde(default)]
    ok_count: i64,
    #[serde(default)]
    measurement_count: i64,
}

impl AggCell {
    // OONI usually sends `measurement_count`, but fall back to the component sum
    // for the rare cell that omits it, so a rate is never divided by a zero that
    // should have been a real total.
    fn total(&self) -> i64 {
        if self.measurement_count > 0 {
            self.measurement_count
        } else {
            self.anomaly_count + self.confirmed_count + self.failure_count + self.ok_count
        }
    }
}

// ── Technology registry ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TechCategory {
    AiAccess,
    Circumvention,
    PrivacyOs,
    Messaging,
}

impl TechCategory {
    fn as_db_str(self) -> &'static str {
        match self {
            TechCategory::AiAccess => "AI_ACCESS",
            TechCategory::Circumvention => "CIRCUMVENTION",
            TechCategory::PrivacyOs => "PRIVACY_OS",
            TechCategory::Messaging => "MESSAGING",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Technology {
    key: &'static str,
    category: TechCategory,
    // `None` for tools measured via a dedicated OONI test (tor/psiphon/torsf and
    // the messaging apps) rather than a `web_connectivity` probe against a URL.
    url: Option<&'static str>,
    test_name: &'static str,
}

static REGISTRY: &[Technology] = &[
    // AI access
    Technology {
        key: "openai.com",
        category: TechCategory::AiAccess,
        url: Some("https://openai.com"),
        test_name: "web_connectivity",
    },
    Technology {
        key: "claude.ai",
        category: TechCategory::AiAccess,
        url: Some("https://claude.ai"),
        test_name: "web_connectivity",
    },
    Technology {
        key: "deepseek",
        category: TechCategory::AiAccess,
        url: Some("https://www.deepseek.com"),
        test_name: "web_connectivity",
    },
    Technology {
        key: "huggingface",
        category: TechCategory::AiAccess,
        url: Some("https://huggingface.co"),
        test_name: "web_connectivity",
    },
    // Circumvention
    Technology {
        key: "tor",
        category: TechCategory::Circumvention,
        url: None,
        test_name: "tor",
    },
    Technology {
        key: "torproject",
        category: TechCategory::Circumvention,
        url: Some("https://www.torproject.org"),
        test_name: "web_connectivity",
    },
    Technology {
        key: "signal",
        category: TechCategory::Circumvention,
        url: Some("https://signal.org"),
        test_name: "web_connectivity",
    },
    Technology {
        key: "i2p",
        category: TechCategory::Circumvention,
        url: Some("https://geti2p.net"),
        test_name: "web_connectivity",
    },
    // Psiphon and Snowflake (torsf) are dedicated OONI nettests, like `tor`
    // above — they measure whether the tool itself works, not a URL.
    Technology {
        key: "psiphon",
        category: TechCategory::Circumvention,
        url: None,
        test_name: "psiphon",
    },
    Technology {
        key: "torsf",
        category: TechCategory::Circumvention,
        url: None,
        test_name: "torsf",
    },
    // Privacy-preserving OS
    Technology {
        key: "grapheneos",
        category: TechCategory::PrivacyOs,
        url: Some("https://grapheneos.org"),
        test_name: "web_connectivity",
    },
    Technology {
        key: "tails",
        category: TechCategory::PrivacyOs,
        url: Some("https://tails.boum.org"),
        test_name: "web_connectivity",
    },
    // Messaging apps, measured via their own dedicated OONI nettests (like
    // tor/psiphon above — `url: None`). These test whether the app's servers
    // are reachable, which is a stronger signal than whether its marketing
    // website loads. `signal_messenger`'s key is distinct from the `signal`
    // web_connectivity entry above so both can coexist in technology_blocks.
    Technology {
        key: "whatsapp",
        category: TechCategory::Messaging,
        url: None,
        test_name: "whatsapp",
    },
    Technology {
        key: "telegram",
        category: TechCategory::Messaging,
        url: None,
        test_name: "telegram",
    },
    Technology {
        key: "facebook_messenger",
        category: TechCategory::Messaging,
        url: None,
        test_name: "facebook_messenger",
    },
    Technology {
        key: "signal_messenger",
        category: TechCategory::Messaging,
        url: None,
        test_name: "signal",
    },
];

/// Runs the adoption-signal reachability sweep, the per-technology block
/// sweep, and the historical timeline backfill. Each phase is independent —
/// a failure in one does not prevent the others from running — but a failure
/// in any of them is still reported to the caller (rather than always
/// returning `Ok`), so `fetch_runs`/`/health` actually reflect a degraded
/// cycle instead of showing "ok" no matter what happened underneath.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    // The set of ISO codes we recognise, read once. Every sweep groups by
    // country server-side and filters its response against this, dropping the
    // junk dimensions OONI emits (e.g. `ZZ`) rather than storing them as
    // countries.
    let known = known_codes(state)?;
    let mut failures = Vec::new();

    if let Err(e) = fetch_and_store_signals(state, &known).await {
        eprintln!("ooni: adoption signal fetch failed: {e}");
        failures.push(format!("signals: {e}"));
    }
    if let Err(e) = fetch_and_store_technology_blocks(state, &known).await {
        eprintln!("ooni: technology block fetch failed: {e}");
        failures.push(format!("technology_blocks: {e}"));
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{}/2 phase(s) had failures: {}",
            failures.len(),
            failures.join(" | ")
        )
    }
}

/// The per-content-category sweep, run as its own fetcher (see run_fetchers)
/// rather than as a phase of `fetch_and_store`. It is a single request for the
/// whole globe, but chaining it behind the long technology-block + timeline
/// sweeps starved it of the shared OONI time budget — as its own concurrent
/// task with its own timeout it always completes.
pub async fn fetch_categories(state: &AppState) -> Result<()> {
    let known = known_codes(state)?;
    fetch_and_store_categories(state, &known).await
}

/// Snapshot of the recognised ISO codes, taken without holding the DB lock
/// across any `.await`.
fn known_codes(state: &AppState) -> Result<HashSet<String>> {
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    crate::db::countries::known_codes(&conn)
}

/// Issues a GET, retrying on HTTP 429 with backoff (honoring `Retry-After`
/// when the API sends one). The OONI API rate-limits hard enough that
/// without this, a burst of sweep requests mostly comes back throttled.
async fn get_with_retry(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
) -> Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        let resp = client.get(url).query(query).send().await?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            && attempt < MAX_RATE_LIMIT_RETRIES
        {
            let wait_secs = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or_else(|| 3u64 * 2u64.pow(attempt))
                .min(60);
            eprintln!(
                "ooni: rate-limited (429) on {url}, retrying in {wait_secs}s (attempt {}/{MAX_RATE_LIMIT_RETRIES})",
                attempt + 1
            );
            tokio::time::sleep(Duration::from_secs(wait_secs)).await;
            attempt += 1;
            continue;
        }

        return Ok(resp.error_for_status()?);
    }
}

/// One aggregation request → its flat list of cells.
async fn fetch_aggregation(
    client: &reqwest::Client,
    query: &[(&str, &str)],
) -> Result<Vec<AggCell>> {
    let resp = get_with_retry(client, AGGREGATION_ENDPOINT, query).await?;
    Ok(resp.json::<AggResponse>().await?.result)
}

/// Host as OONI records it in the `domain` dimension: scheme stripped, but the
/// `www.` prefix KEPT. OONI stores `www.torproject.org`, and stripping the
/// prefix collapses that domain's coverage from ~150 countries to 6.
fn host_of(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

// ── Adoption signal reachability sweep ────────────────────────────────────

struct SignalRow {
    country: String,
    status: &'static str,
    confidence: &'static str,
    count: i64,
}

/// One aggregation per AI-access URL, grouped by country, recording a
/// reachability signal per country in `adoption_signals`.
async fn fetch_and_store_signals(state: &AppState, known: &HashSet<String>) -> Result<()> {
    let client = crate::util::http::client("ooni")
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let since = days_ago_iso(POINT_IN_TIME_WINDOW_DAYS);
    let mut failures = Vec::new();

    for url in TARGET_URLS {
        let host = host_of(url);
        let query = [
            ("domain", host.as_str()),
            ("test_name", "web_connectivity"),
            ("axis_x", "probe_cc"),
            ("since", since.as_str()),
        ];
        match fetch_aggregation(&client, &query).await {
            Ok(cells) => {
                let rows: Vec<SignalRow> = cells
                    .iter()
                    .filter_map(|c| {
                        let cc = c.probe_cc.as_deref()?;
                        if !known.contains(cc) {
                            return None;
                        }
                        let total = c.total();
                        let (status, confidence) =
                            classify_signal(c.confirmed_count, c.anomaly_count, total);
                        Some(SignalRow {
                            country: cc.to_string(),
                            status,
                            confidence,
                            count: total,
                        })
                    })
                    .collect();
                if let Err(e) = insert_signals(state, url, &rows) {
                    eprintln!("ooni: failed to store signals for {url}: {e}");
                    failures.push(format!("{url} (store): {e}"));
                }
            }
            Err(e) => {
                eprintln!("ooni: failed to fetch signals for {url}: {e}");
                failures.push(format!("{url} (fetch): {e}"));
            }
        }
        tokio::time::sleep(REQUEST_PACING).await;
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{}/{} signal URL(s) failed: {}",
            failures.len(),
            TARGET_URLS.len(),
            failures.join(" | ")
        )
    }
}

/// Confirmed blocking on any measurement wins outright. Otherwise, any anomaly
/// without confirmation is treated as inconclusive rather than accessible,
/// since anomalies can stem from transient network issues.
fn classify_signal(confirmed: i64, anomaly: i64, total: i64) -> (&'static str, &'static str) {
    let status = if confirmed > 0 {
        "BLOCKED"
    } else if total == 0 {
        "INCONCLUSIVE"
    } else if anomaly == 0 {
        "ACCESSIBLE"
    } else {
        "INCONCLUSIVE"
    };

    let confidence = if total >= 10 {
        "HIGH"
    } else if total >= 3 {
        "MEDIUM"
    } else {
        "LOW"
    };

    (status, confidence)
}

fn provider_for(url: &str) -> &'static str {
    match url {
        "https://api.openai.com" | "https://openai.com" => "OpenAI",
        "https://api.anthropic.com" => "Anthropic",
        "https://www.deepseek.com" => "DeepSeek",
        _ => "Unknown",
    }
}

fn insert_signals(state: &AppState, url: &str, rows: &[SignalRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let date = today_iso();
    let provider = provider_for(url);
    let source = "OONI Aggregation API v1 (api.ooni.io/api/v1/aggregation), grouped by probe_cc";

    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO adoption_signals VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )?;
        for r in rows {
            let signal_id = format!("ooni-{}-{}-{date}", r.country, slug(url));
            let value_text = format!(
                "{}: {} OONI web_connectivity measurement{} for {url} from {}",
                r.status,
                r.count,
                if r.count == 1 { "" } else { "s" },
                r.country,
            );
            stmt.execute(rusqlite::params![
                signal_id,
                r.country,
                provider,
                url,
                "NETWORK_MEASUREMENT",
                "OONI_MEASUREMENT",
                value_text,
                date,
                r.confidence,
                source,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn slug(url: &str) -> String {
    url.replace("https://", "").replace(['/', '.'], "-")
}

// ── Per-technology block sweep ─────────────────────────────────────────────

struct TechRow {
    country: String,
    status: &'static str,
    rate: f64,
    count: i64,
}

/// One aggregation per technology, grouped by country over a trailing window,
/// inserting a current point-in-time status per country into
/// `technology_blocks`. Request count is `REGISTRY.len()`, independent of how
/// many countries come back.
async fn fetch_and_store_technology_blocks(
    state: &AppState,
    known: &HashSet<String>,
) -> Result<()> {
    let client = crate::util::http::client("ooni")
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let since = days_ago_iso(POINT_IN_TIME_WINDOW_DAYS);
    let mut failures = Vec::new();

    for tech in REGISTRY {
        let host = tech.url.map(host_of);
        let mut query: Vec<(&str, &str)> = vec![
            ("test_name", tech.test_name),
            ("axis_x", "probe_cc"),
            ("since", since.as_str()),
        ];
        if let Some(h) = host.as_deref() {
            query.push(("domain", h));
        }

        match fetch_aggregation(&client, &query).await {
            Ok(cells) => {
                let rows: Vec<TechRow> = cells
                    .iter()
                    .filter_map(|c| {
                        let cc = c.probe_cc.as_deref()?;
                        if !known.contains(cc) {
                            return None;
                        }
                        let total = c.total();
                        let (status, rate) = classify_technology(c.anomaly_count, total);
                        Some(TechRow {
                            country: cc.to_string(),
                            status,
                            rate,
                            count: total,
                        })
                    })
                    .collect();
                if let Err(e) = insert_technology_blocks(state, tech, &rows) {
                    eprintln!(
                        "ooni: failed to store technology blocks for {}: {e}",
                        tech.key
                    );
                    failures.push(format!("{} (store): {e}", tech.key));
                }
            }
            Err(e) => {
                eprintln!(
                    "ooni: failed to fetch technology blocks for {}: {e}",
                    tech.key
                );
                failures.push(format!("{} (fetch): {e}", tech.key));
            }
        }
        tokio::time::sleep(REQUEST_PACING).await;
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{}/{} technology/technologies failed: {}",
            failures.len(),
            REGISTRY.len(),
            failures.join(" | ")
        )
    }
}

/// Classifies by anomaly rate + sample size rather than the `confirmed` flag
/// alone — OONI confirms blocking for very few test types, so relying on it
/// exclusively would under-report circumvention-tool blocking. Now fed the
/// server-side aggregate counts for the whole window, which is both cheaper and
/// more representative than the old ≤50-measurement sample.
fn classify_technology(anomaly: i64, total: i64) -> (&'static str, f64) {
    if total == 0 {
        return ("INCONCLUSIVE", 0.0);
    }
    let rate = anomaly as f64 / total as f64;

    let status = if rate > 0.7 && total > 10 {
        "CONFIRMED_BLOCKED"
    } else if rate > 0.4 && total > 5 {
        "LIKELY_BLOCKED"
    } else if rate < 0.1 && total > 5 {
        "ACCESSIBLE"
    } else {
        "INCONCLUSIVE"
    };

    (status, rate)
}

fn insert_technology_blocks(state: &AppState, tech: &Technology, rows: &[TechRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let date = today_iso();
    let source = "OONI Aggregation API v1 (api.ooni.io/api/v1/aggregation), grouped by probe_cc";

    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO technology_blocks
             (country_code, category, technology, domain_or_url, test_name, status, anomaly_rate, measurement_count, checked_date, source_notes)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )?;
        for r in rows {
            stmt.execute(rusqlite::params![
                r.country,
                tech.category.as_db_str(),
                tech.key,
                tech.url.unwrap_or(""),
                tech.test_name,
                r.status,
                r.rate,
                r.count,
                date,
                source,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ── Per-content-category censorship sweep ──────────────────────────────────

struct CatRow {
    country: String,
    category_code: String,
    label: String,
    status: &'static str,
    rate: f64,
    anomaly: i64,
    confirmed: i64,
    total: i64,
}

/// A single 2-D aggregation (category_code x probe_cc) records the anomaly rate
/// per content category per country in `category_blocks` — the "what kinds of
/// content are censored" view (NEWS, HUMR, LGBT, POLR, ...) for the whole globe
/// in one request.
async fn fetch_and_store_categories(state: &AppState, known: &HashSet<String>) -> Result<()> {
    let client = crate::util::http::client("ooni")
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    let since = days_ago_iso(CATEGORY_WINDOW_DAYS);
    let query = [
        ("test_name", "web_connectivity"),
        ("axis_x", "category_code"),
        ("axis_y", "probe_cc"),
        ("since", since.as_str()),
    ];

    let cells = fetch_aggregation(&client, &query).await?;
    let rows: Vec<CatRow> = cells
        .iter()
        .filter_map(|c| {
            let cc = c.probe_cc.as_deref()?;
            if !known.contains(cc) {
                return None;
            }
            let code = c.category_code.as_deref()?;
            if code.is_empty() {
                return None;
            }
            let total = c.total();
            let rate = if total > 0 {
                c.anomaly_count as f64 / total as f64
            } else {
                0.0
            };
            Some(CatRow {
                country: cc.to_string(),
                category_code: code.to_string(),
                label: category_label(code),
                status: classify_category(rate, total),
                rate,
                anomaly: c.anomaly_count,
                confirmed: c.confirmed_count,
                total,
            })
        })
        .collect();

    insert_category_blocks(state, &rows)
}

/// Categories aggregate hundreds of URLs, most of which stay reachable even
/// where a topic is heavily targeted, so category anomaly rates run much lower
/// than a single blocked tool's — these thresholds are tuned to that scale
/// rather than reusing `classify_technology`'s (which expects one URL).
fn classify_category(anomaly_rate: f64, measurement_count: i64) -> &'static str {
    if measurement_count < 100 {
        "INCONCLUSIVE"
    } else if anomaly_rate >= 0.20 {
        "HEAVILY_CENSORED"
    } else if anomaly_rate >= 0.05 {
        "PARTIALLY_CENSORED"
    } else {
        "ACCESSIBLE"
    }
}

fn insert_category_blocks(state: &AppState, rows: &[CatRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let date = today_iso();
    let source = "OONI Aggregation API v1, grouped by Citizen Lab category_code x probe_cc";

    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO category_blocks
             (country_code, category_code, category_label, status, anomaly_rate, anomaly_count, confirmed_count, measurement_count, checked_date, source_notes)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )?;
        for r in rows {
            stmt.execute(rusqlite::params![
                r.country,
                r.category_code,
                r.label,
                r.status,
                r.rate,
                r.anomaly,
                r.confirmed,
                r.total,
                date,
                source,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Citizen Lab content-category code → human label (their canonical legend,
/// lists/00-LEGEND-new_category_codes.csv). Unknown codes fall back to the raw
/// code so a new upstream category is still shown rather than dropped.
fn category_label(code: &str) -> String {
    let label = match code {
        "ALDR" => "Alcohol & Drugs",
        "REL" => "Religion",
        "PORN" => "Pornography",
        "PROV" => "Provocative Attire",
        "POLR" => "Political Criticism",
        "HUMR" => "Human Rights",
        "ENV" => "Environment",
        "MILX" => "Terrorism & Militants",
        "HATE" => "Hate Speech",
        "NEWS" => "News Media",
        "XED" => "Sex Education",
        "PUBH" => "Public Health",
        "GMB" => "Gambling",
        "ANON" => "Anonymization & Circumvention",
        "DATE" => "Online Dating",
        "GRP" => "Social Networking",
        "LGBT" => "LGBT",
        "FILE" => "File-sharing",
        "HACK" => "Hacking Tools",
        "COMT" => "Communication Tools",
        "MMED" => "Media Sharing",
        "HOST" => "Hosting & Blogging",
        "SRCH" => "Search Engines",
        "GAME" => "Gaming",
        "CULTR" => "Culture",
        "ECON" => "Economics",
        "GOVT" => "Government",
        "COMM" => "E-commerce",
        "CTRL" => "Control Content",
        "IGO" => "Intergovernmental Orgs",
        "MISC" => "Miscellaneous",
        other => other,
    };
    label.to_string()
}

// ── Historical blocking timeline backfill ──────────────────────────────────

struct TimelineRow {
    country: String,
    day: String,
    anomaly: i64,
    confirmed: i64,
    total: i64,
    ok: i64,
}

/// One 2-D aggregation per timeline technology (day x country) backfills the
/// daily `blocking_timeline` for every country at once. Replaces the old
/// hardcoded (country, technology) sweep, so a country gets a chart the moment
/// it has measurements rather than only if it was on a hand-maintained list.
/// Where this technology's timeline sweep should start.
///
/// The full window (`TIMELINE_SINCE`, ~21 months x ~230 countries) is only
/// needed once. Re-pulling it every cycle is what broke this fetcher on
/// deployment: measured against the live API, one technology's full sweep is
/// 12.3 MiB / 9.3s versus 1.04 MiB / 2.1s for a trailing window — and twelve
/// of those blew the budget and drew `504 Gateway Time-out` from OONI itself.
///
/// The row count matters as much as the bytes. A full backfill writes ~700k
/// rows across twelve transactions, each holding the single
/// `Arc<Mutex<Connection>>` the whole time, which starves every other fetcher
/// that needs the database — `ooni_categories` is a 5.7s job that never once
/// completed inside its 180s budget while this was running.
///
/// An empty table still gets the full backfill, so a cold start is unchanged.
fn timeline_since(state: &AppState, technology: &str) -> String {
    let newest: Option<String> = state.lock().ok().and_then(|conn| {
        conn.query_row(
            "SELECT MAX(measurement_date) FROM blocking_timeline WHERE technology = ?1",
            rusqlite::params![technology],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    });

    let Some(newest) = newest else {
        return TIMELINE_SINCE.to_string();
    };
    match days_between(&newest, &today_iso()) {
        Some(age) => days_ago_iso(age.max(0) + TIMELINE_OVERLAP_DAYS),
        None => TIMELINE_SINCE.to_string(),
    }
}

/// Its own fetcher rather than a third phase of `fetch_and_store`.
///
/// Sharing one budget meant a slow timeline sweep marked the whole OONI
/// fetcher failed and took `technology_blocks` — the messaging, circumvention
/// and AI-access rows the sidebar actually renders — down with it, even
/// though that phase had already succeeded.
pub async fn fetch_timelines(state: &AppState) -> Result<()> {
    let known = known_codes(state)?;
    fetch_and_store_timeline(state, &known).await
}

async fn fetch_and_store_timeline(state: &AppState, known: &HashSet<String>) -> Result<()> {
    let client = crate::util::http::client("ooni")
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let mut failures = Vec::new();

    for &tech_key in TIMELINE_TECHS {
        let Some(tech) = REGISTRY.iter().find(|t| t.key == tech_key) else {
            continue;
        };
        let host = tech.url.map(host_of);
        let since = timeline_since(state, tech_key);
        let mut query: Vec<(&str, &str)> = vec![
            ("test_name", tech.test_name),
            ("axis_x", "measurement_start_day"),
            ("axis_y", "probe_cc"),
            ("since", since.as_str()),
        ];
        if let Some(h) = host.as_deref() {
            query.push(("domain", h));
        }

        match fetch_aggregation(&client, &query).await {
            Ok(cells) => {
                let rows: Vec<TimelineRow> = cells
                    .iter()
                    .filter_map(|c| {
                        let cc = c.probe_cc.as_deref()?;
                        if !known.contains(cc) {
                            return None;
                        }
                        let day = c.measurement_start_day.as_deref()?;
                        // OONI silently switches to hourly granularity on short
                        // windows (`2026-07-01T06:00:00Z`); that value is part of
                        // the blocking_timeline primary key, so an unvalidated
                        // hourly bucket multiplies rows instead of updating them.
                        if !is_iso_date(day) {
                            return None;
                        }
                        Some(TimelineRow {
                            country: cc.to_string(),
                            day: day.to_string(),
                            anomaly: c.anomaly_count,
                            confirmed: c.confirmed_count,
                            total: c.total(),
                            ok: c.ok_count,
                        })
                    })
                    .collect();
                if let Err(e) = insert_timeline_rows(state, tech_key, &rows) {
                    eprintln!("ooni: failed to store timeline for {tech_key}: {e}");
                    failures.push(format!("{tech_key} (store): {e}"));
                }
            }
            Err(e) => {
                eprintln!("ooni: failed to fetch timeline for {tech_key}: {e}");
                failures.push(format!("{tech_key} (fetch): {e}"));
            }
        }
        tokio::time::sleep(REQUEST_PACING).await;
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{}/{} timeline technology/technologies failed: {}",
            failures.len(),
            TIMELINE_TECHS.len(),
            failures.join(" | ")
        )
    }
}

fn insert_timeline_rows(state: &AppState, technology: &str, rows: &[TimelineRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO blocking_timeline
             (country_code, technology, measurement_date, anomaly_count, confirmed_count, measurement_count, ok_count)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )?;
        for r in rows {
            stmt.execute(rusqlite::params![
                r.country,
                technology,
                r.day,
                r.anomaly,
                r.confirmed,
                r.total,
                r.ok,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// Date helpers live in crate::util::date — see the import at the top of this
// file.

#[cfg(test)]
mod tests {
    use super::*;

    fn db_with(rows: &[(&str, &str)]) -> AppState {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE blocking_timeline (
                 country_code TEXT NOT NULL, technology TEXT NOT NULL,
                 measurement_date TEXT NOT NULL, anomaly_count INTEGER NOT NULL,
                 confirmed_count INTEGER NOT NULL, measurement_count INTEGER NOT NULL,
                 ok_count INTEGER NOT NULL,
                 PRIMARY KEY (country_code, technology, measurement_date));",
        )
        .unwrap();
        for (tech, day) in rows {
            conn.execute(
                "INSERT INTO blocking_timeline VALUES ('IR', ?1, ?2, 0,0,0,0)",
                rusqlite::params![tech, day],
            )
            .unwrap();
        }
        std::sync::Arc::new(std::sync::Mutex::new(conn))
    }

    #[test]
    fn an_empty_timeline_still_gets_the_full_backfill() {
        let state = db_with(&[]);
        assert_eq!(timeline_since(&state, "psiphon"), TIMELINE_SINCE);
    }

    #[test]
    fn a_technology_with_no_rows_of_its_own_gets_the_full_backfill() {
        // Another technology having data must not make this one resume late
        // and silently skip its own history.
        let state = db_with(&[("psiphon", "2026-09-20")]);
        assert_eq!(timeline_since(&state, "torsf"), TIMELINE_SINCE);
    }

    #[test]
    fn a_populated_timeline_resumes_with_an_overlap() {
        let newest = days_ago_iso(3);
        let state = db_with(&[("psiphon", &newest)]);
        let since = timeline_since(&state, "psiphon");

        // 3 days old + 7 days overlap = 10 days back.
        assert_eq!(since, days_ago_iso(3 + TIMELINE_OVERLAP_DAYS));
        // And far short of the full window, which is the entire point.
        assert!(since.as_str() > TIMELINE_SINCE, "incremental, not full: {since}");
    }

    /// `fetch_and_store_timeline` looks each key up in `REGISTRY` and
    /// silently `continue`s when it is absent, so a typo in `TIMELINE_TECHS`
    /// does not fail, log, or even slow anything down — the sparkline for
    /// that technology just never exists. Both lists are hand-maintained and
    /// have to agree; this is the only thing that makes them.
    #[test]
    fn every_timeline_technology_exists_in_the_registry() {
        for key in TIMELINE_TECHS {
            assert!(
                REGISTRY.iter().any(|t| t.key == *key),
                "TIMELINE_TECHS names `{key}`, which is not in REGISTRY — its \
                 timeline would be skipped without any error"
            );
        }
    }

    /// Messaging apps are rendered by their own sidebar widget, which has no
    /// per-row sparkline, so fetching a timeline for them would be several
    /// of the most expensive requests in the codebase for data nothing reads.
    #[test]
    fn timelines_cover_every_non_messaging_technology() {
        let missing: Vec<&str> = REGISTRY
            .iter()
            .filter(|t| !matches!(t.category, TechCategory::Messaging))
            .map(|t| t.key)
            .filter(|key| !TIMELINE_TECHS.contains(key))
            .collect();
        assert!(
            missing.is_empty(),
            "these technologies render a blocking row with no sparkline, while \
             their identical neighbours have one: {missing:?}"
        );
    }
}
