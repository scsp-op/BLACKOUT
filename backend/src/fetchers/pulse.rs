use crate::AppState;
use crate::util::date::today_iso;
use anyhow::Result;
use serde::Deserialize;
use std::time::Duration;

// The Internet Resilience Index, from Internet Society Pulse.
//
// Note the host: this is `pulse-api.internetsociety.org`, NOT the
// `pulse.internetsociety.org/api` path that Pulse's own docs and third-party
// connectors reference. That one sits behind a Cloudflare JS interstitial and
// answers every non-browser request with a 403 "Just a moment..." page —
// identically with a valid token, an invalid one, and none at all. A reqwest
// client can never get past it. The host below is unchallenged.
//
// Called with no `country` parameter it returns every rated country in one
// response (179 of them, ~425 KB), each already at the latest quarter — so
// this is a single bulk fetch rather than a per-country sweep.
const RESILIENCE_ENDPOINT: &str = "https://pulse-api.internetsociety.org/resilience";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

// Pulse reports every score as a 0-1 fraction, while the site presents the
// index as "out of 100" and every other score in this tool is 0-100. Without
// this the whole panel renders at a hundredth of its true value.
const PULSE_SCALE: f64 = 100.0;

#[derive(Debug, Deserialize)]
struct ResilienceResponse {
    #[serde(default)]
    data: Vec<ResilienceRow>,
}

#[derive(Debug, Deserialize)]
struct ResilienceRow {
    #[serde(default)]
    country: String,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    quarter: Option<i64>,
    /// Headline index, 0-1 as published.
    #[serde(default)]
    value: Option<f64>,
    #[serde(default)]
    pillars: Pillars,
}

/// Each pillar also carries a `dimensions` -> `indicators` sub-tree (the full
/// 30-metric breakdown). That is deliberately not modelled: nothing renders it,
/// and serde simply ignores the extra fields.
#[derive(Debug, Deserialize, Default)]
struct Pillars {
    #[serde(default)]
    infrastructure: Pillar,
    #[serde(default)]
    performance: Pillar,
    #[serde(default)]
    security: Pillar,
    #[serde(default)]
    market_readiness: Pillar,
}

#[derive(Debug, Deserialize, Default)]
struct Pillar {
    #[serde(default)]
    value: Option<f64>,
}

struct PulseScore {
    country: String,
    year: i64,
    overall: f64,
    infrastructure: Option<f64>,
    performance: Option<f64>,
    security: Option<f64>,
    market_readiness: Option<f64>,
}

/// Fetches the Internet Resilience Index for every rated country in one
/// request and stores it in `country_scores` under source `ISOC_PULSE`.
/// Skips entirely (not an error) when no API token is configured, matching how
/// the Cloudflare integration handles its optional token.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    let token = match std::env::var("PULSE_API_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            eprintln!(
                "WARNING: pulse: PULSE_API_TOKEN not set — skipping. The resilience \
                 panel will read 'No IRI data' for EVERY country, which is \
                 indistinguishable from Pulse genuinely having no coverage."
            );
            return Ok(());
        }
    };

    let client = crate::util::http::client("pulse")
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    // Recognised ISO codes. Pulse keys on uppercase alpha-2, the same basis as
    // country_reference, so this is a validation filter rather than a mapping.
    let known = {
        let conn = state
            .lock()
            .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
        crate::db::countries::known_codes(&conn)?
    };

    let rows = match fetch_resilience(&client, &token).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("pulse: failed to fetch resilience index: {e}");
            return Ok(());
        }
    };

    let mut scores = Vec::new();
    let mut unknown = Vec::new();
    for row in rows {
        let code = row.country.trim().to_uppercase();
        if code.is_empty() {
            continue;
        }
        if !known.contains(&code) {
            unknown.push(code);
            continue;
        }
        // A row with no headline value is not a country we can score.
        let Some(overall) = row.value else { continue };

        scores.push(PulseScore {
            country: code,
            year: row.year.unwrap_or(0),
            overall: scale(overall),
            infrastructure: row.pillars.infrastructure.value.map(scale),
            performance: row.pillars.performance.value.map(scale),
            security: row.pillars.security.value.map(scale),
            market_readiness: row.pillars.market_readiness.value.map(scale),
        });
        let _ = row.quarter; // present in the feed; not stored
    }

    if !unknown.is_empty() {
        unknown.sort();
        unknown.dedup();
        println!(
            "pulse: {} country code(s) not in country_reference: {}",
            unknown.len(),
            unknown.join(", ")
        );
    }

    let stored = scores.len();
    if let Err(e) = insert_scores(state, &scores) {
        eprintln!("pulse: failed to store scores: {e}");
        return Ok(());
    }
    println!("pulse: stored {stored} resilience scores");
    Ok(())
}

/// 0-1 -> 0-100, rounded to 1dp like the other normalised indices.
fn scale(raw: f64) -> f64 {
    let v = (raw * PULSE_SCALE).clamp(0.0, 100.0);
    (v * 10.0).round() / 10.0
}

async fn fetch_resilience(client: &reqwest::Client, token: &str) -> Result<Vec<ResilienceRow>> {
    let body = client
        .get(RESILIENCE_ENDPOINT)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?
        .error_for_status()?
        .json::<ResilienceResponse>()
        .await?;
    Ok(body.data)
}

/// Banding mirrors the other 0-100 indices so the panel reads consistently.
fn resilience_band(score: f64) -> &'static str {
    if score >= 70.0 {
        "Highly resilient"
    } else if score >= 50.0 {
        "Moderately resilient"
    } else if score >= 35.0 {
        "Fragile"
    } else {
        "Highly fragile"
    }
}

fn insert_scores(state: &AppState, scores: &[PulseScore]) -> Result<()> {
    if scores.is_empty() {
        return Ok(());
    }
    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = conn.transaction()?;
    {
        // Keyed without the quarter so a re-run refreshes in place rather than
        // accumulating a row per period; the latest is what we keep.
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO country_scores
             (id, country_code, source, year, score_overall, score_access, score_content, score_rights, classification, last_updated,
              score_pulse_infrastructure, score_pulse_performance, score_pulse_security, score_pulse_market_readiness)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        )?;
        for s in scores {
            stmt.execute(rusqlite::params![
                format!("{}-ISOC_PULSE", s.country),
                s.country,
                "ISOC_PULSE",
                s.year,
                s.overall,
                Option::<f64>::None,
                Option::<f64>::None,
                Option::<f64>::None,
                resilience_band(s.overall),
                today_iso(),
                s.infrastructure,
                s.performance,
                s.security,
                s.market_readiness,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}
