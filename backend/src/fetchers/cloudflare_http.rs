use crate::AppState;
use crate::util::date::today_iso;
use anyhow::Result;
use serde::Deserialize;
use std::time::Duration;

// Cloudflare Radar's HTTP-version-share timeseries, grouped by the
// `HTTP_VERSION` dimension. NOT the same as `HTTP_PROTOCOL` (that's HTTP vs
// HTTPS, cleartext vs TLS) — `HTTP_VERSION` is HTTP/1.x vs HTTP/2 vs HTTP/3
// (QUIC), which is the signal the leading-indicator case for this fetcher is
// built on: a government can block QUIC — and the circumvention tools that
// tunnel over it — while the network itself keeps running, well before (or
// without) any full connectivity blackout.
const HTTP_VERSION_ENDPOINT: &str =
    "https://api.cloudflare.com/client/v4/radar/http/timeseries_groups/HTTP_VERSION";

// A rolling window, refetched every cycle (INSERT OR REPLACE is idempotent):
// Cloudflare's own recent-day figures are provisional and firm up over the
// following days, so re-pulling the trailing window lets already-stored rows
// self-correct rather than freezing at their first, least-confident estimate.
const DATE_RANGE: &str = "14d";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
// Same politeness convention as ioda.rs's whole-globe sweep — one request per
// country, paced, rather than hammering a third-party API concurrently.
const REQUEST_PACING: Duration = Duration::from_millis(300);

#[derive(Debug, Deserialize, Default)]
struct RadarResponse {
    #[serde(default)]
    result: RadarResult,
}

#[derive(Debug, Deserialize, Default)]
struct RadarResult {
    #[serde(default, rename = "serie_0")]
    serie: Serie,
}

// Field names are exactly what Cloudflare returns (confirmed via a live
// call), including the literal slash in "HTTP/1.x" — not the `HTTPv1`-style
// names some Radar docs examples use elsewhere.
#[derive(Debug, Deserialize, Default)]
struct Serie {
    #[serde(default)]
    timestamps: Vec<String>,
    #[serde(default, rename = "HTTP/1.x")]
    http1: Vec<String>,
    #[serde(default, rename = "HTTP/2")]
    http2: Vec<String>,
    #[serde(default, rename = "HTTP/3")]
    http3: Vec<String>,
}

/// One day's HTTP-version share for one country, ready to insert.
struct DailyShare {
    date: String,
    http1_pct: Option<f64>,
    http2_pct: Option<f64>,
    http3_pct: Option<f64>,
}

/// Sweeps every drawable country for daily HTTP/1.x-2-3 traffic share and
/// records it in `http_protocol_share`. Skips entirely (not an error) when no
/// API token is configured, matching `cloudflare.rs`'s existing convention —
/// this reuses the same `CLOUDFLARE_API_TOKEN`, just a different Radar
/// sub-API.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    let token = match std::env::var("CLOUDFLARE_API_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            eprintln!(
                "WARNING: cloudflare_http: CLOUDFLARE_API_TOKEN not set — skipping. \
                 No HTTP/3 protocol-share signal will be recorded."
            );
            return Ok(());
        }
    };

    let codes = fetch_codes(state)?;
    let client = crate::util::http::client("cloudflare-radar")
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    for country in &codes {
        match fetch_country_share(&client, &token, country).await {
            Ok(days) => {
                for day in &days {
                    if let Err(e) = insert_share(state, country, day) {
                        eprintln!("cloudflare_http: failed to store share for {country}: {e}");
                    }
                }
            }
            Err(e) => eprintln!("cloudflare_http: failed to fetch share for {country}: {e}"),
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

async fn fetch_country_share(
    client: &reqwest::Client,
    token: &str,
    country: &str,
) -> Result<Vec<DailyShare>> {
    let resp = client
        .get(HTTP_VERSION_ENDPOINT)
        .query(&[
            ("location", country),
            ("dateRange", DATE_RANGE),
            ("aggInterval", "1d"),
            ("normalization", "PERCENTAGE"),
            ("format", "json"),
        ])
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json::<RadarResponse>()
        .await?;

    let serie = resp.result.serie;
    let days = serie
        .timestamps
        .iter()
        .enumerate()
        .map(|(i, ts)| DailyShare {
            // Timestamps are `YYYY-MM-DDT00:00:00Z`; the date prefix matches
            // this app's ISO-date convention used everywhere else.
            date: ts.chars().take(10).collect(),
            http1_pct: parse_pct(serie.http1.get(i)),
            http2_pct: parse_pct(serie.http2.get(i)),
            http3_pct: parse_pct(serie.http3.get(i)),
        })
        .collect();
    Ok(days)
}

/// Cloudflare sends percentages as numeric strings (`"0.061217"`), not JSON
/// numbers, and can omit a version's series entirely for a country/day with
/// ~0% share rather than sending an explicit zero — both cases fall through
/// to `None` rather than failing the whole row.
fn parse_pct(raw: Option<&String>) -> Option<f64> {
    raw.and_then(|s| s.parse::<f64>().ok())
}

fn insert_share(state: &AppState, country: &str, day: &DailyShare) -> Result<()> {
    let id = format!("{country}-{}", day.date);
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    conn.execute(
        "INSERT OR REPLACE INTO http_protocol_share
         (id, country_code, date, http1_pct, http2_pct, http3_pct, source, last_updated)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![
            id,
            country,
            day.date,
            day.http1_pct,
            day.http2_pct,
            day.http3_pct,
            "Cloudflare Radar (api.cloudflare.com/client/v4/radar/http/timeseries_groups/HTTP_VERSION)",
            today_iso(),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pct_reads_a_numeric_string() {
        let value = "0.061217".to_string();
        assert_eq!(parse_pct(Some(&value)), Some(0.061217));
    }

    #[test]
    fn parse_pct_is_none_for_missing_or_unparseable_input() {
        assert_eq!(parse_pct(None), None);
        let garbage = "n/a".to_string();
        assert_eq!(parse_pct(Some(&garbage)), None);
    }

    #[test]
    fn fetch_country_share_zips_timestamps_with_each_version_by_position() {
        // Mirrors a real (trimmed) response: three days, HTTP/3 share present
        // every day, confirming the `.get(i)` indexing lines up correctly
        // rather than silently truncating to the shortest series.
        let serie = Serie {
            timestamps: vec![
                "2026-08-25T00:00:00Z".to_string(),
                "2026-08-26T00:00:00Z".to_string(),
            ],
            http1: vec!["54.4".to_string(), "53.0".to_string()],
            http2: vec!["45.5".to_string(), "46.9".to_string()],
            http3: vec!["0.026".to_string(), "0.024".to_string()],
        };
        let days: Vec<DailyShare> = serie
            .timestamps
            .iter()
            .enumerate()
            .map(|(i, ts)| DailyShare {
                date: ts.chars().take(10).collect(),
                http1_pct: parse_pct(serie.http1.get(i)),
                http2_pct: parse_pct(serie.http2.get(i)),
                http3_pct: parse_pct(serie.http3.get(i)),
            })
            .collect();

        assert_eq!(days.len(), 2);
        assert_eq!(days[0].date, "2026-08-25");
        assert_eq!(days[0].http3_pct, Some(0.026));
        assert_eq!(days[1].date, "2026-08-26");
        assert_eq!(days[1].http1_pct, Some(53.0));
    }
}
