use crate::AppState;
use crate::util::date::today_iso;
use anyhow::Result;
use serde::Deserialize;
use std::time::Duration;

// IODA (Internet Outage Detection and Analysis, Georgia Tech / CAIDA) exposes
// a free, token-less JSON API. It detects country-level internet outages by
// fusing BGP, active probing (ping-slash24), the Merit network telescope
// (merit-nt), and Google Transparency Report (gtr) signals. Each detected
// outage becomes an `outage_events` row.
const OUTAGES_ENDPOINT: &str = "https://api.ioda.inetintel.cc.gatech.edu/v2/outages/events";

// How far back to pull. 90 days gives the per-country timeline enough history
// to be a chart while still surfacing anything currently ongoing.
const WINDOW_SECS: i64 = 90 * 24 * 60 * 60;

// IODA is far more forgiving than OONI, but we still pace requests: one sweep
// now covers every drawable country (~230 requests), and hammering a public
// research API concurrently is rude and invites throttling. Each result is
// inserted immediately after its own fetch so a timeout only loses countries
// not yet reached.
const REQUEST_PACING: Duration = Duration::from_millis(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct OutagesResponse {
    #[serde(default)]
    data: Vec<OutageRow>,
}

#[derive(Debug, Deserialize)]
struct OutageRow {
    #[serde(default)]
    start: i64,
    #[serde(default)]
    duration: i64,
    #[serde(default)]
    datasource: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    score: Option<f64>,
}

/// Sweeps every drawable country (focus tier first) for internet-outage
/// events over the trailing window and records each in `outage_events`.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    let codes = fetch_codes(state)?;

    let now = now_unix();
    let from = now - WINDOW_SECS;

    let client = crate::util::http::client("ioda")
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    for country in &codes {
        match fetch_country_outages(&client, country, from, now).await {
            Ok(rows) => {
                for row in &rows {
                    if let Err(e) = insert_outage(state, country, row) {
                        eprintln!("ioda: failed to store outage for {country}: {e}");
                    }
                }
            }
            Err(e) => eprintln!("ioda: failed to fetch outages for {country}: {e}"),
        }
        tokio::time::sleep(REQUEST_PACING).await;
    }
    Ok(())
}

/// Drawable countries, focus tier first. Read here (rather than reusing the
/// OONI-oriented `super::fetch_codes`, which is focus-only) so the outage map
/// covers the whole globe as the country set expands.
fn fetch_codes(state: &AppState) -> Result<Vec<String>> {
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    crate::db::countries::codes_by_priority(&conn)
}

async fn fetch_country_outages(
    client: &reqwest::Client,
    country: &str,
    from: i64,
    until: i64,
) -> Result<Vec<OutageRow>> {
    let from = from.to_string();
    let until = until.to_string();
    let resp = client
        .get(OUTAGES_ENDPOINT)
        .query(&[
            ("from", from.as_str()),
            ("until", until.as_str()),
            ("entityType", "country"),
            ("entityCode", country),
        ])
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<OutagesResponse>().await?.data)
}

fn insert_outage(state: &AppState, country: &str, row: &OutageRow) -> Result<()> {
    let datasource = row.datasource.as_deref().unwrap_or("unknown");
    let end_ts = row.start + row.duration;
    // start + datasource uniquely identify an outage window for a country, so
    // re-running the sweep refreshes rather than duplicates.
    let id = format!("ioda-{country}-{datasource}-{}", row.start);

    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    conn.execute(
        "INSERT OR REPLACE INTO outage_events
         (id, country_code, start_ts, duration_secs, end_ts, datasource, method, score, source, last_updated)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        rusqlite::params![
            id,
            country,
            row.start,
            row.duration,
            end_ts,
            datasource,
            row.method,
            row.score.unwrap_or(0.0),
            "IODA (api.ioda.inetintel.cc.gatech.edu/v2)",
            today_iso(),
        ],
    )?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
