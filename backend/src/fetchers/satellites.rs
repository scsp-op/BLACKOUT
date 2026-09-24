//! Satellite catalog refresh: fetch, normalize, validate, merge.
//!
//! The pipeline's one hard invariant lives in `db::satellite_catalog`, not
//! here: a refresh is an *update* to a persistent catalog keyed by NORAD ID,
//! never a replacement of it. This module's job is to produce the best set of
//! normalized records it can from whatever providers are reachable, and to be
//! honest in the log about what it managed.
//!
//! Consequences worth stating explicitly, because they are the whole point:
//!
//!   * A group that times out contributes nothing and costs nothing — the
//!     satellites it would have refreshed keep their previous elements.
//!   * A group that returns a fraction of its usual size still contributes
//!     its records (merging can only ever improve a row), but is flagged and
//!     does not get to lower the baseline the shrinkage guard compares
//!     against next cycle.
//!   * With every provider down, no records are produced, `merge` is never
//!     called, and the catalog is untouched.
//!
//! Startup never depends on the network: the canonical catalog is loaded from
//! SQLite first and served immediately, and the refresh only happens
//! afterwards, asynchronously, and only if the persisted catalog is actually
//! due for one.

use crate::db::satellite_catalog::{self as store, MergeOutcome};
use crate::satellites::{
    CATEGORY_RANK_UNKNOWN, CachedSatellite, SatelliteCatalog, Source, SourceRecord,
};
use serde::Deserialize;
use std::time::Duration;

// CelesTrak's modern GP data endpoint. `FORMAT` must be given explicitly —
// CSV became the default response format (2026-05-09) when it's omitted.
// Query params must be uppercase.
const GP_ENDPOINT: &str = "https://celestrak.org/NORAD/elements/gp.php";

// Starlink-specific SpaceX-derived ephemeris, refreshed roughly daily and
// fresher than the generic GP sweep for newly-deployed satellites. Uses the
// same NORAD catalog numbers as the general GP catalog, so it can override a
// general-GP record for the same object rather than being a separate object.
const SUPPLEMENTAL_ENDPOINT: &str = "https://celestrak.org/NORAD/elements/supplemental/sup-gp.php";

// SatNOGS' own TLE catalogue (a few thousand actively-tracked objects, mostly
// amateur/cubesat, re-published from Space-Track like CelesTrak's own data)
// — a second, independently-hosted source. CelesTrak has been observed
// completely unreachable (TCP connect timeouts, not its documented 403
// rate-limit) from at least one cloud network path.
//
// Fetched every cycle, not only when CelesTrak has collapsed. It used to be
// gated behind a "CelesTrak returned fewer than 500 objects" threshold, which
// meant it was skipped in exactly the degraded band that matters most (a
// 1,600-object CelesTrak sweep is badly broken but comfortably over 500) and
// made the two providers alternatives rather than complements. One request
// every two hours is a polite way to hold a second, independent opinion about
// a few thousand objects.
const SATNOGS_ENDPOINT: &str = "https://db.satnogs.org/api/tle/?format=json";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
// CelesTrak's own data only updates roughly every 2 hours and firmly
// rate-limits repeat downloads of high-traffic groups (see 403 handling
// below) — this pacing is just about being a polite, sequential client
// across the handful of group requests in one cycle, not about the update
// cadence itself (that's `run_catalog_refresh_loop`'s job).
const REQUEST_PACING: Duration = Duration::from_millis(500);

const DEFAULT_CATALOG_REFRESH_HOURS: f64 = 2.0;
const MIN_CATALOG_REFRESH_SECS: u64 = 60;

/// Attempts per source per cycle, including the first. Three is enough to ride
/// out a single dropped connection or a brief 5xx without turning a provider
/// outage into a retry storm: with the backoff below, a fully unreachable
/// provider costs ~3 timeouts and ~6s of waiting, then the cycle moves on and
/// tries again at the normal cadence.
const MAX_ATTEMPTS: u32 = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);

/// Consecutive whole-group failures after which CelesTrak is treated as
/// unreachable for the remainder of this cycle, and the outstanding group
/// requests are skipped rather than attempted.
///
/// Without this, bounded per-request retries multiply into an unbounded-
/// feeling cycle: the observed CelesTrak failure is a TCP connect timeout, so
/// each attempt costs the full `REQUEST_TIMEOUT`, and 12 groups x 3 attempts x
/// 30s is roughly 18 minutes of waiting to learn something the first three
/// failures already established. That delays the SatNOGS fetch (the one
/// provider that might actually answer) by the same margin, and on a cold
/// start it is 18 minutes of serving an empty catalog.
///
/// Three rather than one because a single group failing on its own is
/// routine — a high-traffic group being throttled says nothing about the
/// host's reachability. Three consecutive failures, each already retried, is
/// no longer plausibly per-group.
const CELESTRAK_GIVE_UP_AFTER: usize = 3;

/// How often to retry while the catalog is *completely* empty — e.g. a first
/// deployment that can't reach any provider. Much shorter than the normal
/// cadence because there is no data to serve at all, and nothing to lose by
/// asking again; once any cycle produces records the loop reverts to the
/// configured interval.
///
/// This, plus the App Storage snapshot, is deliberately the whole answer to
/// a cold start. Shipping a committed known-good catalog snapshot with the image
/// was considered and rejected: TLE accuracy decays (SGP4 position error grows
/// by kilometres per day, and a months-old element set puts a LEO satellite
/// hundreds of kilometres from where it is drawn), so a snapshot old enough to
/// have been committed would render confidently wrong positions on a globe
/// whose entire purpose is showing where things actually are. Fewer real
/// satellites beats more fictional ones. It would also need a recurring
/// refresh chore and would add ~20 MB per update to git history. A genuine
/// first deploy during a total outage therefore shows an empty panel and
/// retries every three minutes, which is honest and self-correcting.
const EMPTY_RETRY_INTERVAL: Duration = Duration::from_secs(180);

/// Fraction of a group's established size below which a *successful* response
/// is treated as partial rather than authoritative. CelesTrak's `active` group
/// carries ~11,000 objects; a response with 1,500 is not a catalog that
/// shrank by 86% in two hours, it is a truncated or partial response.
///
/// Deliberately a *ratio against what this deployment has actually seen*, not
/// an absolute floor: the real catalog size changes constantly (Starlink
/// launches and deorbits alone move it by hundreds), so hard-coding ~16,000
/// would rot. Overridable via `SATELLITE_GROUP_MIN_RETAIN_RATIO`.
const DEFAULT_MIN_RETAIN_RATIO: f64 = 0.5;

/// Default CelesTrak GP groups to fetch, each tagged with the category
/// assigned to any object first seen there. Order is precedence: earlier
/// entries win when the same NORAD ID appears in more than one group (e.g. a
/// Starlink satellite that also shows up in the generic `active` sweep).
/// `gps-ops`/`galileo`/`glo-ops`/`beidou` share one `navigation` tag (the four
/// GNSS constellations don't overlap in practice, so first-match-wins never
/// actually arbitrates between them) and `stations`/`science` share one
/// `stations` tag (space stations plus telescope/science craft — CelesTrak
/// has no combined group for that pairing, so this is two group fetches
/// feeding one category). `military`/`resource` are CelesTrak's real, public
/// "Miscellaneous Military" (~27 objects — most military/intel satellites
/// simply have no public elements at all) and "Earth Resources" (~181
/// objects) groups; both are honest counts, not padded to match any external
/// reference. Overridable via `SATELLITE_GROUPS`
/// (`group:category,group:category,...`) so the wider wishlist (OneWeb/
/// Iridium, weather, debris, ...) can be added later purely through
/// configuration.
const DEFAULT_GROUPS: &[(&str, &str)] = &[
    ("starlink", "starlink"),
    ("gps-ops", "navigation"),
    ("galileo", "navigation"),
    ("glo-ops", "navigation"),
    ("beidou", "navigation"),
    ("military", "military"),
    ("resource", "earthobs"),
    ("stations", "stations"),
    ("science", "stations"),
    ("geo", "geo"),
    ("active", "other"),
];

/// Overall outcome of one refresh cycle, as reported to the log and persisted
/// for `/api/satellites/status`.
///
/// Three states rather than a bool because the difference between them is
/// exactly what was previously impossible to read off the logs: "one group
/// failed but the catalog is fine" and "nothing was reachable at all" are very
/// different operational situations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshStatus {
    /// Every configured source answered (or had nothing new), nothing looked
    /// partial.
    Complete,
    /// At least one source failed or looked partial; the catalog was still
    /// merged from whatever did arrive, and everything else was retained.
    Degraded,
    /// No source produced a single usable record. The catalog was not touched.
    Failed,
}

impl RefreshStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RefreshStatus::Complete => "complete",
            RefreshStatus::Degraded => "degraded",
            RefreshStatus::Failed => "failed",
        }
    }
}

/// Why a single fetch attempt failed, classified by whether retrying it
/// within the same cycle could plausibly help.
#[derive(Debug)]
enum FetchError {
    /// CelesTrak's documented 403: the group has not been updated since our
    /// last successful download. Not a failure at all — there is simply
    /// nothing new to merge, which is the expected steady state when the
    /// refresh cadence and CelesTrak's update cadence are both ~2h.
    NotModified,
    /// Timeout, connection reset/refused, DNS failure, 5xx, or 429. Worth
    /// another attempt after a backoff.
    Transient(String),
    /// A 4xx we can't fix by asking again (a bad GROUP name), or a response
    /// body we can't parse. Retrying would just repeat it.
    Permanent(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NotModified => write!(f, "no new data since last download (403)"),
            FetchError::Transient(m) => write!(f, "{m}"),
            FetchError::Permanent(m) => write!(f, "{m}"),
        }
    }
}

/// Maps a reqwest failure onto the retry classification above.
///
/// The failure this whole module was hardened against — CelesTrak accepting a
/// TCP connection and then never completing it — arrives as `is_timeout` or
/// `is_connect`, which is why those are the clearest transient cases.
fn classify(e: reqwest::Error) -> FetchError {
    if e.is_timeout() || e.is_connect() || e.is_request() || e.is_body() {
        return FetchError::Transient(e.to_string());
    }
    if let Some(status) = e.status() {
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return FetchError::Transient(format!("HTTP {status}"));
        }
        return FetchError::Permanent(format!("HTTP {status}"));
    }
    FetchError::Permanent(e.to_string())
}

/// Bounded retry with exponential backoff and jitter, for one source.
///
/// Only `Transient` errors are retried; `NotModified` and `Permanent` return
/// immediately, because re-asking would either be pointless or abusive. The
/// whole thing is awaited inside the background refresh task, never on the
/// startup path, so a provider hanging for 3×30s delays nothing user-facing.
async fn with_retries<T, F, Fut>(label: &str, mut attempt_fn: F) -> Result<T, FetchError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, FetchError>>,
{
    let mut delay = RETRY_BASE_DELAY;
    for attempt in 1..=MAX_ATTEMPTS {
        match attempt_fn().await {
            Ok(value) => return Ok(value),
            Err(FetchError::Transient(msg)) if attempt < MAX_ATTEMPTS => {
                let wait = delay + jitter(delay);
                eprintln!(
                    "satellites: {label} attempt {attempt}/{MAX_ATTEMPTS} failed ({msg}), \
                     retrying in {:.1}s",
                    wait.as_secs_f64()
                );
                tokio::time::sleep(wait).await;
                delay = delay.saturating_mul(2);
            }
            Err(e) => return Err(e),
        }
    }
    // The loop either returns or retries; the last iteration cannot retry.
    unreachable!("with_retries exhausted without returning")
}

/// Up to +50% of `base`, so several sources retrying after a shared outage
/// don't line up into a synchronized burst. Derived from the clock rather than
/// adding a `rand` dependency — de-synchronising retries needs spread, not
/// cryptographic randomness.
fn jitter(base: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    Duration::from_millis(base.as_millis() as u64 / 2 * (nanos % 1000) / 1000)
}

/// `SATELLITE_GROUPS` env var, `group:category,group:category,...`, or the
/// built-in default list when unset/empty/unparseable.
fn configured_groups() -> Vec<(String, String)> {
    parse_groups_config(std::env::var("SATELLITE_GROUPS").ok().as_deref())
}

fn parse_groups_config(raw: Option<&str>) -> Vec<(String, String)> {
    let from_raw = raw.and_then(|v| {
        let parsed: Vec<(String, String)> = v
            .split(',')
            .filter_map(|entry| {
                let (group, category) = entry.split_once(':')?;
                let group = group.trim();
                let category = category.trim();
                if group.is_empty() || category.is_empty() {
                    return None;
                }
                Some((group.to_string(), category.to_string()))
            })
            .collect();
        (!parsed.is_empty()).then_some(parsed)
    });

    from_raw.unwrap_or_else(|| {
        DEFAULT_GROUPS
            .iter()
            .map(|(g, c)| (g.to_string(), c.to_string()))
            .collect()
    })
}

fn min_retain_ratio() -> f64 {
    std::env::var("SATELLITE_GROUP_MIN_RETAIN_RATIO")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|r| r.is_finite() && *r > 0.0 && *r <= 1.0)
        .unwrap_or(DEFAULT_MIN_RETAIN_RATIO)
}

/// Whether a *successful* response is too small to be believed as a complete
/// picture of its group.
///
/// `previous` is the group's established size (its high-water mark from a
/// prior healthy cycle). With no history — a first-ever fetch — nothing can be
/// judged partial, so this returns false and the response sets the baseline.
/// An empty response for a group that used to have members is always
/// suspicious, however large the ratio allows.
fn is_suspiciously_partial(received: usize, previous: usize, min_ratio: f64) -> bool {
    if previous == 0 {
        return false;
    }
    if received == 0 {
        return true;
    }
    (received as f64) < (previous as f64) * min_ratio
}

/// Turns raw element sets into catalog records, dropping any SGP4 rejects.
///
/// Validation happens *here*, before the merge, rather than at catalog-build
/// time as it used to. An element set SGP4 can't turn into `Constants` is
/// unusable, and letting it into the canonical store would mean a fresher-but-
/// broken record could displace an older working one — a corrupt upstream
/// record would then silently remove a satellite from the served catalog while
/// leaving a row in the database. Rejecting it up front keeps the previous good
/// elements in place instead.
fn normalize(
    elements: Vec<sgp4::Elements>,
    category: &str,
    category_rank: i64,
    source: Source,
) -> (Vec<SourceRecord>, usize) {
    let mut out = Vec::with_capacity(elements.len());
    let mut rejected = 0usize;
    for el in elements {
        if sgp4::Constants::from_elements(&el).is_err() {
            rejected += 1;
            continue;
        }
        let norad_id = el.norad_id;
        out.push(SourceRecord {
            norad_id,
            name: el
                .object_name
                .clone()
                .unwrap_or_else(|| format!("NORAD {norad_id}")),
            category: category.to_string(),
            category_rank,
            source,
            elements: el,
        });
    }
    (out, rejected)
}

/// Replaces the in-memory catalog with the canonical contents of the database.
///
/// Always sourced from SQLite, never assembled from whatever the last fetch
/// happened to return, so the served catalog is by construction exactly the
/// persisted one. There is no code path that can leave memory holding less
/// than the database.
fn load_into_catalog(catalog: &SatelliteCatalog, state: &crate::AppState) -> usize {
    let loaded = match state.lock() {
        Ok(conn) => {
            let rows = store::load_all(&conn);
            let updated_at = store::get_state_ts(&conn, store::LAST_SUCCESSFUL_REFRESH);
            rows.map(|(rows, skipped)| (rows, skipped, updated_at))
        }
        Err(_) => {
            eprintln!("satellites: db lock poisoned, leaving the in-memory catalog as it is");
            return 0;
        }
    };
    let (rows, skipped, updated_at) = match loaded {
        Ok(v) => v,
        Err(e) => {
            eprintln!("satellites: could not read the persistent catalog: {e:#}");
            return 0;
        }
    };
    if skipped > 0 {
        eprintln!("satellites: {skipped} stored record(s) had unreadable JSON and were skipped");
    }

    let mut objects = Vec::with_capacity(rows.len());
    let mut unusable = 0usize;
    for row in rows {
        match sgp4::Constants::from_elements(&row.elements) {
            Ok(constants) => objects.push(CachedSatellite {
                norad_id: row.norad_id,
                name: row.name,
                category: row.category,
                elements: row.elements,
                constants,
                source: row.source,
                epoch: row.epoch,
                last_updated: row.last_updated,
            }),
            Err(_) => unusable += 1,
        }
    }
    if unusable > 0 {
        eprintln!("satellites: {unusable} stored record(s) rejected by SGP4 and not served");
    }

    let count = objects.len();
    match catalog.write() {
        Ok(mut guard) => {
            guard.objects = objects;
            guard.catalog_updated_at = updated_at;
        }
        Err(_) => eprintln!("satellites: catalog lock poisoned, could not publish {count} object(s)"),
    }
    count
}

/// One refresh cycle: fetch every source, merge what arrived, log what
/// happened. Never returns an error — a refresh that achieves nothing is a
/// reportable state, not an exception, and the catalog is intact either way.
async fn refresh_cycle(catalog: &SatelliteCatalog, state: &crate::AppState) -> RefreshStatus {
    // CelesTrak and SatNOGS are both free services run by a single maintainer
    // or a small nonprofit, and this is the one fetcher that sweeps a dozen
    // endpoints per cycle — identifying honestly matters most here. The
    // identity itself lives in util::http, which explains what it is and is
    // not for.
    let client = match crate::util::http::client("satellites")
        .timeout(REQUEST_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("satellites: could not build the HTTP client: {e}");
            return RefreshStatus::Failed;
        }
    };

    let groups_config = configured_groups();
    let min_ratio = min_retain_ratio();
    let high_water = match state.lock() {
        Ok(conn) => store::group_high_water(&conn),
        Err(_) => Default::default(),
    };

    let mut records: Vec<SourceRecord> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut partials: Vec<String> = Vec::new();
    let mut celestrak_records = 0usize;
    let mut celestrak_ok_requests = 0usize;
    let mut celestrak_not_modified = 0usize;
    let mut new_high_water: Vec<(String, usize)> = Vec::new();
    let mut consecutive_failures = 0usize;
    let mut celestrak_unreachable = false;
    // Never attempted, because the breaker had already tripped. Tracked apart
    // from `failures` so the log can say "3 failed, 9 not attempted" rather
    // than the misleading "12 failed" — a request that was never sent is not
    // evidence about that group.
    let mut skipped: Vec<String> = Vec::new();

    println!("satellites: CelesTrak refresh started ({} groups)", groups_config.len());

    for (rank, (group, category)) in groups_config.iter().enumerate() {
        if celestrak_unreachable {
            skipped.push(group.clone());
            continue;
        }
        match with_retries(&format!("CelesTrak {group}"), || fetch_group(&client, group)).await {
            Ok(elements) => {
                let received = elements.len();
                let previous = high_water.get(group).copied().unwrap_or(0);
                let (normalized, rejected) =
                    normalize(elements, category, rank as i64, Source::Celestrak);

                if is_suspiciously_partial(received, previous, min_ratio) {
                    // Merged anyway: these records are individually valid and
                    // can only improve the rows they touch. What is withheld
                    // is *authority* — the group does not get to lower its own
                    // baseline, so a run of truncated responses can't ratchet
                    // the guard down until it stops guarding anything.
                    eprintln!(
                        "satellites: CelesTrak {group}: {received} object(s), previously \
                         {previous} — treating as a partial response, merging it but keeping \
                         the previous baseline"
                    );
                    partials.push(group.clone());
                } else {
                    println!("satellites: CelesTrak {group}: {received} received");
                    new_high_water.push((group.clone(), received));
                }
                if rejected > 0 {
                    eprintln!(
                        "satellites: CelesTrak {group}: {rejected} record(s) rejected by SGP4"
                    );
                }
                celestrak_ok_requests += 1;
                consecutive_failures = 0;
                celestrak_records += normalized.len();
                records.extend(normalized);
            }
            Err(FetchError::NotModified) => {
                celestrak_not_modified += 1;
                // Reachable and answering — just nothing new to merge.
                consecutive_failures = 0;
                println!("satellites: CelesTrak {group}: no new data since last download (403)");
            }
            Err(e) => {
                eprintln!("satellites: CelesTrak {group}: {e}");
                failures.push(format!("celestrak/{group}"));
                consecutive_failures += 1;
                if consecutive_failures >= CELESTRAK_GIVE_UP_AFTER {
                    celestrak_unreachable = true;
                    let remaining = groups_config.len() - (rank + 1);
                    eprintln!(
                        "satellites: CelesTrak appears unreachable ({consecutive_failures} \
                         consecutive group failures) — skipping the remaining {remaining} \
                         group(s) and the supplemental feed this cycle, and going straight to \
                         SatNOGS"
                    );
                }
            }
        }
        tokio::time::sleep(REQUEST_PACING).await;
    }

    // Skipped entirely when the breaker above tripped: the supplemental feed
    // is served by the same host as the GP endpoint, so there is no reason to
    // expect it to answer when eleven consecutive GP requests did not.
    let supplemental = if celestrak_unreachable {
        skipped.push("supplemental".to_string());
        None
    } else {
        Some(
            with_retries("CelesTrak starlink supplemental", || {
                fetch_starlink_supplemental(&client)
            })
            .await,
        )
    };

    match supplemental {
        None => {}
        Some(Ok(elements)) => {
            let received = elements.len();
            // Tagged `starlink` at a category rank just past the configured
            // groups: it must not *downgrade* a category a real GP group
            // assigned, but a supplemental-only object still needs a sensible
            // one, and Starlink is the only supplemental file fetched.
            let (normalized, rejected) = normalize(
                elements,
                "starlink",
                groups_config.len() as i64,
                Source::CelestrakSupplemental,
            );
            println!("satellites: CelesTrak starlink supplemental: {received} received");
            if rejected > 0 {
                eprintln!(
                    "satellites: CelesTrak starlink supplemental: {rejected} record(s) rejected \
                     by SGP4"
                );
            }
            celestrak_ok_requests += 1;
            celestrak_records += normalized.len();
            records.extend(normalized);
        }
        Some(Err(FetchError::NotModified)) => {
            celestrak_not_modified += 1;
            println!(
                "satellites: CelesTrak starlink supplemental: no new data since last download (403)"
            );
        }
        Some(Err(e)) => {
            eprintln!("satellites: CelesTrak starlink supplemental: {e}");
            failures.push("celestrak/supplemental".to_string());
        }
    }

    // One line that answers "is CelesTrak working" without having to read the
    // twelve above it. `no-new-data` is a success; `failed` is not.
    let celestrak_sources = groups_config.len() + 1;
    let celestrak_failed =
        celestrak_sources - celestrak_ok_requests - celestrak_not_modified - skipped.len();
    println!(
        "satellites: CelesTrak summary: {celestrak_sources} source(s) — \
         {celestrak_ok_requests} returned data, {celestrak_not_modified} had no new data, \
         {celestrak_failed} failed, {} not attempted; {celestrak_records} record(s) total",
        skipped.len()
    );
    if celestrak_records > 0 {
        let _ = store::set_state_now(state, store::LAST_CELESTRAK_SUCCESS);
    }

    // SatNOGS, every cycle, as a complement rather than a substitute — see
    // SATNOGS_ENDPOINT. Its records go through exactly the same merge as
    // CelesTrak's, so overlap deduplicates by NORAD ID and the freshest
    // element set wins whichever provider supplied it.
    let mut satnogs_ok = false;
    match with_retries("SatNOGS", || fetch_satnogs(&client)).await {
        Ok(elements) => {
            let received = elements.len();
            // SatNOGS publishes no category information, so its records must
            // never overwrite a CelesTrak-assigned category — hence the
            // deliberately-worst category rank. `other` only ever lands on an
            // object no CelesTrak group has ever supplied.
            let (normalized, rejected) =
                normalize(elements, "other", CATEGORY_RANK_UNKNOWN, Source::Satnogs);
            println!("satellites: SatNOGS: {received} received");
            if rejected > 0 {
                eprintln!("satellites: SatNOGS: {rejected} record(s) rejected by SGP4");
            }
            satnogs_ok = !normalized.is_empty();
            records.extend(normalized);
        }
        Err(e) => {
            eprintln!("satellites: SatNOGS: {e}");
            failures.push("satnogs".to_string());
        }
    }
    if satnogs_ok {
        let _ = store::set_state_now(state, store::LAST_SATNOGS_SUCCESS);
    }

    // Nothing usable arrived. `merge` is deliberately not called: there is
    // nothing to merge, and calling it with an empty batch would still rewrite
    // `last_successful_refresh`-adjacent bookkeeping for a cycle that achieved
    // nothing. The catalog keeps serving whatever it already had.
    if records.is_empty() {
        let served = catalog.read().map(|g| g.objects.len()).unwrap_or(0);
        eprintln!(
            "satellites: every source failed this cycle ({}) — catalog preserved, still serving \
             {served} satellite(s) from the persistent store",
            failures.join(", ")
        );
        let _ = store::set_state(state, store::LAST_REFRESH_STATUS, RefreshStatus::Failed.as_str());
        return RefreshStatus::Failed;
    }

    let outcome = match store::merge(state, &records) {
        Ok(o) => o,
        Err(e) => {
            // The merge is one transaction, so a failure here rolled back:
            // the previous catalog is intact, not half-updated.
            eprintln!(
                "satellites: merge failed, previous catalog preserved unchanged: {e:#}"
            );
            let _ =
                store::set_state(state, store::LAST_REFRESH_STATUS, RefreshStatus::Failed.as_str());
            return RefreshStatus::Failed;
        }
    };

    // Only now that the merge committed do the healthy groups get to move
    // their baselines. Doing it earlier would credit a group for a response
    // that never actually landed.
    for (group, received) in new_high_water {
        let _ = store::set_state(
            state,
            &format!("{}{}", store::GROUP_HIGH_WATER_PREFIX, group),
            &received.to_string(),
        );
    }

    let served = load_into_catalog(catalog, state);
    report(&outcome, served);

    let status = if failures.is_empty() && partials.is_empty() && skipped.is_empty() {
        RefreshStatus::Complete
    } else {
        RefreshStatus::Degraded
    };
    match status {
        RefreshStatus::Complete => println!("satellites: refresh complete"),
        RefreshStatus::Degraded => {
            let mut parts: Vec<String> = Vec::new();
            if !failures.is_empty() {
                parts.push(format!("failed: {}", failures.join(", ")));
            }
            if !partials.is_empty() {
                parts.push(format!("partial: {}", partials.join(", ")));
            }
            if !skipped.is_empty() {
                parts.push(format!(
                    "not attempted after giving up on the host: {}",
                    skipped.len()
                ));
            }
            println!(
                "satellites: refresh degraded but catalog preserved ({})",
                parts.join("; ")
            );
        }
        RefreshStatus::Failed => {}
    }
    let _ = store::set_state(state, store::LAST_REFRESH_STATUS, status.as_str());
    status
}

/// The per-cycle merge summary. Written as separate lines on purpose: the
/// difference between "318 new" and "1,363 retained from the previous catalog"
/// is the thing that used to be impossible to read off a single log line.
fn report(outcome: &MergeOutcome, served: usize) {
    println!(
        "satellites: {} existing record(s) updated with fresher elements",
        outcome.updated
    );
    println!("satellites: {} new NORAD ID(s) added", outcome.added);
    println!(
        "satellites: {} record(s) seen but already current",
        outcome.unchanged
    );
    println!(
        "satellites: {} record(s) retained from the previous catalog (no source listed them \
         this cycle)",
        outcome.retained
    );
    println!(
        "satellites: canonical catalog now contains {} record(s), {served} served",
        outcome.total
    );
}

/// Startup + scheduling.
///
/// Mirrors `db::run_fetcher_loop`'s spawn/loop/sleep/log shape, but runs
/// independently on its own cadence: CelesTrak's GP/SupGP data only updates
/// roughly every 2 hours — far more often than the 6-hour default the other
/// (SQLite-backed) fetchers share — so this can't just join their loop.
///
/// The ordering here is the point. The persistent catalog is loaded and
/// published *before* any network call, so `/api/satellites` serves
/// last-known-good data from the first request after boot even if every
/// provider is unreachable. Only then is the catalog's age checked, and only
/// if it is actually due does a refresh happen.
pub async fn run_catalog_refresh_loop(catalog: SatelliteCatalog, state: crate::AppState) {
    let hours = std::env::var("SATELLITE_CATALOG_REFRESH_HOURS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|h| h.is_finite() && *h > 0.0)
        .unwrap_or(DEFAULT_CATALOG_REFRESH_HOURS);
    let secs = ((hours * 3600.0) as u64).max(MIN_CATALOG_REFRESH_SECS);
    let gap = Duration::from_secs(secs);

    // One-time import of anything the previous (per-group) persistence layer
    // had stored, so an existing deployment carries its catalog forward
    // instead of rebuilding it from scratch on the first boot after this
    // change — which, during a CelesTrak outage, it might not be able to do.
    migrate_legacy_elements(&state);

    let restored = load_into_catalog(&catalog, &state);
    let age = catalog_age(&state);
    match (restored, age) {
        (0, _) => println!(
            "satellites: no persistent catalog found — this is either a first deployment or \
             the App Storage snapshot did not restore (see README: Deployment (Replit))"
        ),
        (n, Some(age)) => println!(
            "satellites: loaded {n} satellites from the persistent catalog; catalog age {}",
            humanize(age)
        ),
        (n, None) => println!(
            "satellites: loaded {n} satellites from the persistent catalog; last refresh time \
             unknown"
        ),
    }

    println!("Satellite catalog refresh loop: every {hours}h ({secs}s)");

    // Don't re-download a catalog that is still fresh. Ten redeploys in ten
    // minutes should cost zero CelesTrak requests, not ten full sweeps — which
    // is both rude and self-defeating, since hammering high-traffic groups is
    // what provokes the 403 rate-limit that then makes them look broken.
    let mut delay = match (restored, age) {
        (0, _) => Duration::ZERO,
        (_, Some(age)) if age < gap => {
            let wait = gap - age;
            println!(
                "satellites: catalog is still fresh ({} old, refresh every {}) — first refresh \
                 in {}",
                humanize(age),
                humanize(gap),
                humanize(wait)
            );
            wait
        }
        _ => Duration::ZERO,
    };

    loop {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let status = refresh_cycle(&catalog, &state).await;

        let error = match status {
            RefreshStatus::Complete => None,
            RefreshStatus::Degraded => Some("refresh degraded, catalog preserved".to_string()),
            RefreshStatus::Failed => Some("all sources failed, catalog preserved".to_string()),
        };
        if let Err(e) = crate::db::record_run(&state, "satellites", error.as_deref()) {
            eprintln!("satellites: could not record fetch outcome: {e}");
        }

        // Only a catalog that is still completely empty warrants the short
        // retry — a degraded refresh over a healthy catalog is not urgent.
        let empty = catalog.read().map(|g| g.objects.is_empty()).unwrap_or(false);
        delay = if empty { EMPTY_RETRY_INTERVAL } else { gap };
    }
}

/// How long ago the last refresh that actually merged something committed.
/// `None` when nothing has ever been persisted.
fn catalog_age(state: &crate::AppState) -> Option<Duration> {
    let conn = state.lock().ok()?;
    let last = store::get_state_ts(&conn, store::LAST_SUCCESSFUL_REFRESH)?;
    (chrono::Utc::now() - last).to_std().ok()
}

fn humanize(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Reserved `satellite_elements.source_group` value from the previous
/// persistence layer, which parked the SatNOGS fallback bucket alongside real
/// CelesTrak group names. (The Starlink supplemental sentinel needs no
/// constant here — `satellite_elements::load_all` already routes it into
/// `LoadedCache::supplemental`.)
const LEGACY_SATNOGS: &str = "__satnogs_fallback__";
const LEGACY_MIGRATED: &str = "legacy_elements_migrated";

/// Imports the old per-group `satellite_elements` table into the canonical
/// catalog, once. Idempotent via a state flag, and non-destructive: the old
/// table is left in place rather than dropped, so this change is reversible
/// and a botched migration costs nothing.
fn migrate_legacy_elements(state: &crate::AppState) {
    let (already, had_refresh_time) = match state.lock() {
        Ok(conn) => (
            store::get_state(&conn, LEGACY_MIGRATED).is_some(),
            store::get_state(&conn, store::LAST_SUCCESSFUL_REFRESH).is_some(),
        ),
        Err(_) => return,
    };
    if already {
        return;
    }

    let groups_config = configured_groups();
    let legacy = match state.lock() {
        Ok(conn) => crate::db::satellite_elements::load_all(&conn),
        Err(_) => return,
    };
    let Ok(legacy) = legacy else { return };

    let mut records: Vec<SourceRecord> = Vec::new();
    for (group, elements) in legacy.groups {
        let (category, rank, source) = if group == LEGACY_SATNOGS {
            ("other".to_string(), CATEGORY_RANK_UNKNOWN, Source::Satnogs)
        } else {
            match groups_config.iter().position(|(g, _)| *g == group) {
                Some(idx) => (groups_config[idx].1.clone(), idx as i64, Source::Celestrak),
                // A group no longer in the configured list: keep the objects
                // (membership is the valuable part) but let any configured
                // group outrank its category.
                None => (
                    "other".to_string(),
                    CATEGORY_RANK_UNKNOWN,
                    Source::Celestrak,
                ),
            }
        };
        let (normalized, _) = normalize(elements, &category, rank, source);
        records.extend(normalized);
    }
    let (supplemental, _) = normalize(
        legacy.supplemental,
        "starlink",
        groups_config.len() as i64,
        Source::CelestrakSupplemental,
    );
    records.extend(supplemental);

    if records.is_empty() {
        let _ = store::set_state(state, LEGACY_MIGRATED, "empty");
        return;
    }
    match store::merge(state, &records) {
        Ok(outcome) => {
            println!(
                "satellites: migrated {} record(s) from the previous per-group store into the \
                 canonical catalog ({} new)",
                records.len(),
                outcome.added
            );
            // `merge` stamps `last_successful_refresh` whenever it changes
            // something, which is right for a real fetch and wrong for this:
            // moving rows between two local tables is not a refresh, and
            // letting it look like one would make the upgrade boot sit out a
            // full interval before contacting CelesTrak — with a catalog whose
            // elements are as old as whenever the legacy store was last
            // written. Clearing it means the migrated catalog is served
            // immediately *and* a real refresh runs straight away.
            if !had_refresh_time {
                let _ = store::clear_state(state, store::LAST_SUCCESSFUL_REFRESH);
            }
            let _ = store::set_state(state, LEGACY_MIGRATED, "done");
        }
        Err(e) => eprintln!("satellites: could not migrate the previous element store: {e:#}"),
    }
}

// ── Providers ───────────────────────────────────────────────────────────────

async fn fetch_group(
    client: &reqwest::Client,
    group: &str,
) -> Result<Vec<sgp4::Elements>, FetchError> {
    let resp = client
        .get(GP_ENDPOINT)
        .query(&[("GROUP", group), ("FORMAT", "JSON")])
        .send()
        .await
        .map_err(classify)?;
    parse_celestrak(resp).await
}

async fn fetch_starlink_supplemental(
    client: &reqwest::Client,
) -> Result<Vec<sgp4::Elements>, FetchError> {
    let resp = client
        .get(SUPPLEMENTAL_ENDPOINT)
        .query(&[("FILE", "starlink"), ("FORMAT", "JSON")])
        .send()
        .await
        .map_err(classify)?;
    parse_celestrak(resp).await
}

/// CelesTrak answers 403 for two completely different reasons, and the body is
/// the only thing that tells them apart.
///
/// 1. Its documented rate-limit response, which says in plain text that "GP
///    data has not updated since your last successful download". That is the
///    expected steady state when our cadence matches theirs: there is simply
///    nothing new, so it maps to `NotModified` and costs the cycle nothing.
/// 2. A generic server "403 - Forbidden: Access is denied" HTML page, which
///    means this client or network is actually being refused.
///
/// Conflating them is how a *total* CelesTrak outage came to be logged as
/// `refresh complete`: twelve consecutive access-denied pages all read as
/// "nothing new, everything is fine", while the served catalog was in fact
/// whatever the one remaining provider happened to supply. Case 2 is a real
/// failure and is reported as one.
async fn parse_celestrak(resp: reqwest::Response) -> Result<Vec<sgp4::Elements>, FetchError> {
    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        let body = resp.text().await.unwrap_or_default().to_ascii_lowercase();
        if body.contains("has not updated since") || body.contains("no sooner than") {
            return Err(FetchError::NotModified);
        }
        // Transient rather than Permanent: an access-denied 403 from CelesTrak
        // has historically cleared on its own, and it is indistinguishable
        // from a throttle. Retried a bounded number of times, then reported.
        return Err(FetchError::Transient(
            "HTTP 403 access denied (not CelesTrak's no-new-data response — this client or \
             network appears to be blocked or throttled)"
                .to_string(),
        ));
    }
    let resp = resp.error_for_status().map_err(classify)?;
    resp.json::<Vec<sgp4::Elements>>().await.map_err(classify)
}

/// One row of SatNOGS' TLE API — classic 3-line TLE fields as JSON. `tle0`
/// keeps the "0 " name-line prefix verbatim, same as the raw 3-line format.
#[derive(Debug, Deserialize)]
struct SatNogsRecord {
    tle0: String,
    tle1: String,
    tle2: String,
}

/// Fetches SatNOGS' independently-hosted TLE catalogue. A record with
/// malformed TLE lines is skipped and counted rather than failing the whole
/// fetch — one bad row must not cost the other few thousand.
async fn fetch_satnogs(client: &reqwest::Client) -> Result<Vec<sgp4::Elements>, FetchError> {
    let resp = client
        .get(SATNOGS_ENDPOINT)
        .send()
        .await
        .map_err(classify)?
        .error_for_status()
        .map_err(classify)?;
    let records: Vec<SatNogsRecord> = resp.json().await.map_err(classify)?;

    let mut elements = Vec::with_capacity(records.len());
    let mut skipped = 0usize;
    for r in records {
        let name = r.tle0.trim_start_matches("0 ").trim().to_string();
        match sgp4::Elements::from_tle(
            Some(name),
            r.tle1.trim().as_bytes(),
            r.tle2.trim().as_bytes(),
        ) {
            Ok(el) => elements.push(el),
            Err(_) => skipped += 1,
        }
    }
    if skipped > 0 {
        eprintln!("satellites: SatNOGS: {skipped} record(s) with unparseable TLE lines skipped");
    }
    Ok(elements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elements_with(norad_id: u64, mean_motion: f64, eccentricity: f64) -> sgp4::Elements {
        serde_json::from_str(&format!(
            r#"{{
                "OBJECT_NAME": "TEST-{norad_id}",
                "OBJECT_ID": "2020-001A",
                "EPOCH": "2026-09-01T00:00:00.000000",
                "MEAN_MOTION": {mean_motion},
                "ECCENTRICITY": {eccentricity},
                "INCLINATION": 51.6461,
                "RA_OF_ASC_NODE": 221.2784,
                "ARG_OF_PERICENTER": 89.1723,
                "MEAN_ANOMALY": 280.4612,
                "EPHEMERIS_TYPE": 0,
                "CLASSIFICATION_TYPE": "U",
                "NORAD_CAT_ID": {norad_id},
                "ELEMENT_SET_NO": 999,
                "REV_AT_EPOCH": 23600,
                "BSTAR": -3.1515e-5,
                "MEAN_MOTION_DOT": -2.218e-5,
                "MEAN_MOTION_DDOT": 0
            }}"#
        ))
        .expect("fixture parses")
    }

    fn valid(norad_id: u64) -> sgp4::Elements {
        elements_with(norad_id, 15.49507896, 0.0001413)
    }

    #[test]
    fn parse_groups_config_falls_back_to_defaults_when_absent_or_empty() {
        for raw in [None, Some(""), Some("garbage-with-no-colon")] {
            let groups = parse_groups_config(raw);
            assert_eq!(groups.len(), DEFAULT_GROUPS.len());
            assert_eq!(groups[0].0, "starlink");
        }
    }

    #[test]
    fn parse_groups_config_parses_a_valid_override() {
        let groups = parse_groups_config(Some("oneweb:oneweb, iridium-NEXT:iridium"));
        assert_eq!(
            groups,
            vec![
                ("oneweb".to_string(), "oneweb".to_string()),
                ("iridium-NEXT".to_string(), "iridium".to_string()),
            ]
        );
    }

    #[test]
    fn a_first_ever_fetch_is_never_judged_partial() {
        // With no history there is no baseline to be short of, so whatever
        // arrives sets the baseline. Judging it partial would deadlock a
        // first deployment into never trusting any response.
        assert!(!is_suspiciously_partial(1, 0, 0.5));
        assert!(!is_suspiciously_partial(16_000, 0, 0.5));
        assert!(!is_suspiciously_partial(0, 0, 0.5));
    }

    #[test]
    fn a_response_far_below_the_established_size_is_partial() {
        // The reported incident, at group scale: `active` normally ~11,000.
        assert!(is_suspiciously_partial(1_550, 11_000, 0.5));
        assert!(is_suspiciously_partial(1_600, 16_100, 0.5));
        // An empty response for a group that has members is always suspect,
        // however permissive the ratio.
        assert!(is_suspiciously_partial(0, 27, 0.01));
    }

    #[test]
    fn ordinary_catalog_churn_is_not_partial() {
        // Starlink launches and deorbits move real counts by hundreds; none
        // of that may read as a failure.
        assert!(!is_suspiciously_partial(15_900, 16_000, 0.5));
        assert!(!is_suspiciously_partial(12_000, 16_000, 0.5));
        assert!(!is_suspiciously_partial(16_400, 16_000, 0.5));
        // Exactly at the threshold is not below it.
        assert!(!is_suspiciously_partial(8_000, 16_000, 0.5));
        assert!(is_suspiciously_partial(7_999, 16_000, 0.5));
    }

    #[test]
    fn normalize_drops_element_sets_sgp4_cannot_use() {
        // A hyperbolic eccentricity is not a propagatable orbit. Letting it
        // through would mean a fresher-but-broken record could displace an
        // older working one in the canonical store.
        let bad = elements_with(2, 15.5, 1.5);
        assert!(
            sgp4::Constants::from_elements(&bad).is_err(),
            "fixture must actually be rejected by sgp4 for this test to mean anything"
        );

        let (records, rejected) = normalize(
            vec![valid(1), bad, valid(3)],
            "starlink",
            0,
            Source::Celestrak,
        );
        assert_eq!(rejected, 1);
        assert_eq!(
            records.iter().map(|r| r.norad_id).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn normalize_carries_category_source_and_a_usable_name() {
        let (records, _) = normalize(vec![valid(7)], "navigation", 3, Source::Celestrak);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "TEST-7");
        assert_eq!(records[0].category, "navigation");
        assert_eq!(records[0].category_rank, 3);
        assert_eq!(records[0].source, Source::Celestrak);

        // A record with no OBJECT_NAME still gets an identifiable name rather
        // than an empty string.
        let mut nameless = valid(8);
        nameless.object_name = None;
        let (records, _) = normalize(vec![nameless], "other", CATEGORY_RANK_UNKNOWN, Source::Satnogs);
        assert_eq!(records[0].name, "NORAD 8");
    }

    #[test]
    fn satnogs_records_can_never_outrank_a_celestrak_category() {
        // The mechanism that keeps a SatNOGS refresh from relabelling every
        // satellite `other`: its category rank is worse than any real group's
        // index, and `merge` only upgrades on a strictly better rank.
        let groups = parse_groups_config(None);
        assert!(
            CATEGORY_RANK_UNKNOWN > groups.len() as i64,
            "the unknown-category rank must be worse than every configured group"
        );
    }

    #[test]
    fn a_total_celestrak_outage_stays_within_a_sane_time_budget() {
        // The observed CelesTrak failure is a TCP connect timeout, so every
        // attempt costs the full request timeout. This pins the worst case
        // before the cycle gives up and reaches SatNOGS, because that number
        // is how long a cold start serves an empty catalog — and it is easy to
        // blow up accidentally by raising the retry count or the threshold.
        let worst_case_secs = CELESTRAK_GIVE_UP_AFTER as u64
            * MAX_ATTEMPTS as u64
            * REQUEST_TIMEOUT.as_secs();
        assert!(
            worst_case_secs <= 300,
            "a fully unreachable CelesTrak would delay SatNOGS by {worst_case_secs}s; \
             without the circuit breaker this was ~18 minutes"
        );
        // And it must stay well inside the empty-catalog retry interval, or
        // cycles would overlap themselves on a cold start during an outage.
        assert!(worst_case_secs <= EMPTY_RETRY_INTERVAL.as_secs() * 2);
    }

    #[test]
    fn humanize_reads_as_an_age() {
        assert_eq!(humanize(Duration::from_secs(45)), "45s");
        assert_eq!(humanize(Duration::from_secs(47 * 60)), "47m");
        assert_eq!(humanize(Duration::from_secs(2 * 3600 + 5 * 60)), "2h05m");
    }

    #[test]
    fn jitter_stays_within_half_the_base_delay() {
        for _ in 0..50 {
            let j = jitter(RETRY_BASE_DELAY);
            assert!(j <= RETRY_BASE_DELAY / 2, "jitter {j:?} exceeded half the base");
        }
    }

    #[tokio::test]
    async fn transient_failures_are_retried_and_permanent_ones_are_not() {
        use std::sync::atomic::{AtomicU32, Ordering};

        // Succeeds on the third attempt, as a dropped connection would.
        let attempts = AtomicU32::new(0);
        let result: Result<u32, FetchError> = with_retries("test", || {
            let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if n < 3 {
                    Err(FetchError::Transient("connection reset".into()))
                } else {
                    Ok(n)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 3);
        assert_eq!(attempts.load(Ordering::SeqCst), MAX_ATTEMPTS);

        // A permanent error must not be retried at all.
        let attempts = AtomicU32::new(0);
        let result: Result<u32, FetchError> = with_retries("test", || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(FetchError::Permanent("HTTP 404".into())) }
        })
        .await;
        assert!(matches!(result, Err(FetchError::Permanent(_))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1, "no retry on a 4xx");

        // Neither must CelesTrak's documented 403 — it is not a failure.
        let attempts = AtomicU32::new(0);
        let result: Result<u32, FetchError> = with_retries("test", || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(FetchError::NotModified) }
        })
        .await;
        assert!(matches!(result, Err(FetchError::NotModified)));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_fully_unreachable_source_gives_up_after_max_attempts() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let attempts = AtomicU32::new(0);
        let result: Result<u32, FetchError> = with_retries("test", || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err(FetchError::Transient("timed out".into())) }
        })
        .await;
        assert!(matches!(result, Err(FetchError::Transient(_))));
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            MAX_ATTEMPTS,
            "bounded: a dead provider must not be retried indefinitely"
        );
    }
}
