use crate::AppState;
use crate::util::date::{days_ago_iso, today_iso};
use anyhow::Result;
use serde::Deserialize;
use std::time::Duration;

// RIPEstat's per-country BGP visibility endpoint: for a given country, how
// many of its registered ASNs/prefixes are *currently visible in the global
// routing table* (the `_ris` fields, sourced from RIPE NCC's RIS BGP route
// collectors) versus merely *allocated on paper* (the `_stats` fields, from
// RIR delegation records). A government withdrawing route announcements — a
// connectivity blackout at the routing layer — shows up here as `_ris` counts
// collapsing while `_stats` counts stay flat. Field direction confirmed via a
// live call against Iran's Jan 2026 shutdown window, not assumed from docs
// alone: `v6_prefixes_ris` dropped from 433 (Dec 31) to 43 (Jan 8) while
// `*_stats` stayed flat across the same window.
const ENDPOINT: &str = "https://stat.ripe.net/data/country-resource-stats/data.json";

// Bounded window, refetched every cycle (INSERT OR REPLACE is idempotent) —
// RIPEstat's own documented default `starttime` reaches back to 2004, so an
// unbounded request would return far more history than one cycle needs.
const WINDOW_DAYS: i64 = 14;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
// RIPEstat documents no hard rate limit (an 8-concurrent-requests-per-IP
// ceiling, not a request-rate one), but a small pace is still the polite-
// client convention this codebase uses for every whole-globe sweep.
const REQUEST_PACING: Duration = Duration::from_millis(200);

#[derive(Debug, Deserialize, Default)]
struct RipestatResponse {
    #[serde(default)]
    data: RipestatData,
}

#[derive(Debug, Deserialize, Default)]
struct RipestatData {
    #[serde(default)]
    stats: Vec<StatEntry>,
}

// All six numeric fields are `f64`, not integers, even though they're
// conceptually counts: RIPEstat averages across multiple RIS route-collector
// snapshots within the `resolution` window, so a busy country's count is
// frequently fractional (confirmed live — the US returned `v6_prefixes_ris:
// 49559.5`). Deserializing these as `i64` fails outright for any country
// whose count isn't a whole number that day, which is common for
// higher-traffic countries.
#[derive(Debug, Deserialize)]
struct StatEntry {
    stats_date: String,
    v4_prefixes_ris: f64,
    v6_prefixes_ris: f64,
    asns_ris: f64,
    v4_prefixes_stats: f64,
    v6_prefixes_stats: f64,
    asns_stats: f64,
}

/// Sweeps every drawable country for daily BGP prefix/ASN visibility and
/// records it in `bgp_prefix_visibility`. No API token needed — RIPEstat's
/// data API is open.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    let codes = fetch_codes(state)?;
    let client = crate::util::http::client("ripestat")
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    let starttime = days_ago_iso(WINDOW_DAYS);
    let endtime = today_iso();

    for country in &codes {
        match fetch_country_stats(&client, country, &starttime, &endtime).await {
            Ok(entries) => {
                for entry in &entries {
                    if let Err(e) = insert_stats(state, country, entry) {
                        eprintln!("ripestat: failed to store visibility for {country}: {e}");
                    }
                }
            }
            Err(e) => eprintln!("ripestat: failed to fetch visibility for {country}: {e}"),
        }
        tokio::time::sleep(REQUEST_PACING).await;
    }
    Ok(())
}

fn fetch_codes(state: &AppState) -> Result<Vec<String>> {
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    crate::db::countries::codes_by_priority(&conn)
}

async fn fetch_country_stats(
    client: &reqwest::Client,
    country: &str,
    starttime: &str,
    endtime: &str,
) -> Result<Vec<StatEntry>> {
    // RIPEstat's own documented convention for high-volume/registered
    // callers — identifies this tool in their logs, not an auth credential.
    // Derived from DEPLOYMENT_ID unless overridden, so one variable is enough
    // to tell two deployments apart everywhere they are visible.
    let sourceapp = crate::util::http::ripestat_sourceapp();

    let resp = client
        .get(ENDPOINT)
        .query(&[
            ("resource", country.to_lowercase().as_str()),
            ("starttime", starttime),
            ("endtime", endtime),
            ("resolution", "1d"),
            ("sourceapp", sourceapp),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<RipestatResponse>()
        .await?;
    Ok(resp.data.stats)
}

/// RIPEstat uses `-1` as a "not available" sentinel (observed live: prefix
/// registration counts are frequently unavailable even when the ASN count
/// isn't) — treated as `None`, not a literal negative count.
fn normalize(value: f64) -> Option<f64> {
    if value < 0.0 { None } else { Some(value) }
}

fn insert_stats(state: &AppState, country: &str, entry: &StatEntry) -> Result<()> {
    // `stats_date` is `YYYY-MM-DDTHH:MM:SS`; the date prefix matches this
    // app's ISO-date convention used everywhere else.
    let date: String = entry.stats_date.chars().take(10).collect();
    let id = format!("{country}-{date}");
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    conn.execute(
        "INSERT OR REPLACE INTO bgp_prefix_visibility
         (id, country_code, date, registered_asns, routed_asns,
          registered_v4_prefixes, routed_v4_prefixes,
          registered_v6_prefixes, routed_v6_prefixes, source, last_updated)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        rusqlite::params![
            id,
            country,
            date,
            normalize(entry.asns_stats),
            normalize(entry.asns_ris),
            normalize(entry.v4_prefixes_stats),
            normalize(entry.v4_prefixes_ris),
            normalize(entry.v6_prefixes_stats),
            normalize(entry.v6_prefixes_ris),
            "RIPEstat (stat.ripe.net/data/country-resource-stats)",
            today_iso(),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_passes_through_non_negative_counts() {
        assert_eq!(normalize(852.0), Some(852.0));
        assert_eq!(normalize(0.0), Some(0.0));
    }

    #[test]
    fn normalize_treats_negative_sentinel_as_unavailable() {
        // RIPEstat's documented "not available" sentinel — observed live:
        // v4/v6 prefix registration counts are frequently -1 even when the
        // ASN count for the same country/day is a real, positive number.
        assert_eq!(normalize(-1.0), None);
    }

    #[test]
    fn normalize_passes_through_a_fractional_ris_average() {
        // Confirmed live for the US: RIS values are averaged across multiple
        // route-collector snapshots within the resolution window, so a busy
        // country's count is frequently not a whole number.
        assert_eq!(normalize(49_559.5), Some(49_559.5));
    }

    #[test]
    fn stat_entry_deserializes_the_real_response_shape() {
        // A trimmed real response body (Iran, around the Jan 2026 BGP
        // withdrawal), confirming field names/types match what RIPEstat
        // actually sends rather than an assumed schema.
        let body = r#"{
            "data": {
                "stats": [
                    {
                        "timeline": [{"starttime": "2026-01-07T00:00:00", "endtime": "2026-01-07T00:00:00"}],
                        "v4_prefixes_ris": 8209,
                        "v6_prefixes_ris": 433,
                        "asns_ris": 556,
                        "v4_prefixes_stats": -1,
                        "v6_prefixes_stats": -1,
                        "asns_stats": 852,
                        "stats_date": "2026-01-07T00:00:00"
                    }
                ]
            }
        }"#;
        let parsed: RipestatResponse = serde_json::from_str(body).expect("parses");
        assert_eq!(parsed.data.stats.len(), 1);
        let entry = &parsed.data.stats[0];
        assert_eq!(entry.stats_date, "2026-01-07T00:00:00");
        assert_eq!(entry.v6_prefixes_ris, 433.0);
        assert_eq!(normalize(entry.v4_prefixes_stats), None);
        assert_eq!(normalize(entry.asns_stats), Some(852.0));
    }

    #[test]
    fn stat_entry_deserializes_a_fractional_ris_value() {
        // The real response shape that broke i64 deserialization for
        // higher-traffic countries — confirmed live against the US.
        let body = r#"{
            "data": {
                "stats": [
                    {
                        "timeline": [{"starttime": "2026-09-01T00:00:00", "endtime": "2026-09-01T00:00:00"}],
                        "v4_prefixes_ris": 299184,
                        "v6_prefixes_ris": 49559.5,
                        "asns_ris": 18394,
                        "v4_prefixes_stats": -1,
                        "v6_prefixes_stats": -1,
                        "asns_stats": 32170,
                        "stats_date": "2026-09-01T00:00:00"
                    }
                ]
            }
        }"#;
        let parsed: RipestatResponse = serde_json::from_str(body).expect("parses");
        assert_eq!(parsed.data.stats[0].v6_prefixes_ris, 49_559.5);
    }
}
