use crate::AppState;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Duration;

// The `/outages` sub-resource is the one that returns network-outage
// annotations in the shape parsed below. The bare `/radar/annotations` path
// rejects the request with HTTP 400 (code 2001) unless a date range is given.
const ANNOTATIONS_ENDPOINT: &str = "https://api.cloudflare.com/client/v4/radar/annotations/outages";

#[derive(Debug, Deserialize, Default)]
struct RadarResponse {
    #[serde(default)]
    result: RadarResult,
}

#[derive(Debug, Deserialize, Default)]
struct RadarResult {
    #[serde(default)]
    annotations: Vec<RadarEvent>,
}

#[derive(Debug, Deserialize, Default)]
struct RadarEvent {
    #[serde(default, rename = "startDate")]
    start_date: String,
    // Ongoing outages have no end yet — `endDate` comes back JSON `null`, so this
    // must be optional; a plain String fails to decode the whole response.
    #[serde(default, rename = "endDate")]
    end_date: Option<String>,
    #[serde(default, rename = "eventType")]
    event_type: String,
    #[serde(default)]
    description: String,
    // ISO 3166-1 alpha-2 codes this annotation applies to. A single outage can
    // span several countries, so each becomes its own adoption_signals row.
    #[serde(default)]
    locations: Vec<String>,
}

/// Fetches Cloudflare Radar network-outage annotations for the whole globe in a
/// SINGLE request — each annotation already carries the countries it affects in
/// `locations` — and records one adoption signal per (country, annotation).
/// Skips entirely (not an error) when no API token is configured, since this
/// integration is optional.
///
/// This one bulk request replaces the old per-country sweep: walking ~230
/// countries one request at a time couldn't finish inside the fetcher's time
/// budget and timed out on every startup.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    let token = match std::env::var("CLOUDFLARE_API_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            eprintln!(
                "WARNING: cloudflare: CLOUDFLARE_API_TOKEN not set — skipping. \
                 No Radar outage annotations will be recorded."
            );
            return Ok(());
        }
    };

    let client = crate::util::http::client("cloudflare-radar")
        .timeout(Duration::from_secs(15))
        .build()?;

    // Recognised ISO codes, read once. Annotation `locations` are filtered
    // against this so a stray/global code is never stored as a country.
    let known = {
        let conn = state
            .lock()
            .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
        crate::db::countries::known_codes(&conn)?
    };

    let events = match fetch_annotations(&client, &token).await {
        Ok(events) => events,
        Err(e) => {
            eprintln!("cloudflare: failed to fetch annotations: {e}");
            return Ok(());
        }
    };

    if let Err(e) = insert_events(state, &known, &events) {
        eprintln!("cloudflare: failed to store annotations: {e}");
    }

    Ok(())
}

async fn fetch_annotations(client: &reqwest::Client, token: &str) -> Result<Vec<RadarEvent>> {
    let resp = client
        .get(ANNOTATIONS_ENDPOINT)
        .query(&[
            // No `location` filter: one request returns every country's
            // annotations, each tagged with its own `locations` array.
            ("limit", "200"),
            // Required: Cloudflare returns HTTP 400 (code 2001, "must send
            // either range or start & end dates") without a time bound. A
            // one-year window captures recent outage annotations.
            ("dateRange", "52w"),
            ("format", "json"),
        ])
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json::<RadarResponse>()
        .await?;
    Ok(resp.result.annotations)
}

/// Writes the whole batch in one transaction. Each annotation is stored once
/// per recognised country in its `locations`; unrecognised codes and empty
/// location lists (global annotations with no attributable country) are dropped.
fn insert_events(state: &AppState, known: &HashSet<String>, events: &[RadarEvent]) -> Result<()> {
    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO adoption_signals VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )?;
        for event in events {
            if event.start_date.is_empty() {
                continue;
            }
            let value_text = format!("{}: {}", event.event_type, event.description);
            let source = format!(
                "Cloudflare Radar annotations API (api.cloudflare.com/client/v4/radar/annotations/outages); ends {}",
                event.end_date.as_deref().unwrap_or("ongoing")
            );
            for loc in &event.locations {
                let country = loc.to_uppercase();
                if !known.contains(&country) {
                    continue;
                }
                let signal_id = format!("cloudflare-{country}-{}", event.start_date);
                stmt.execute(rusqlite::params![
                    signal_id,
                    country,
                    "Cloudflare Radar",
                    event.event_type,
                    "NETWORK_INFRASTRUCTURE",
                    "CLOUDFLARE_OUTAGE_EVENT",
                    value_text,
                    event.start_date,
                    "HIGH",
                    source,
                ])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}
