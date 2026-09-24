use crate::AppState;
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

// Tor Metrics exposes CSV downloads, not a REST/JSON API — there is no
// /api/1.0/ path. `events=off` on the relay endpoint matches the confirmed
// live URL format. Neither request carries a `country` param: omitting it
// returns every country in one CSV (with a `country` column), so the whole
// globe is two downloads rather than two-per-country.
const RELAY_ENDPOINT: &str = "https://metrics.torproject.org/userstats-relay-country.csv";
const BRIDGE_ENDPOINT: &str = "https://metrics.torproject.org/userstats-bridge-country.csv";
// Per-transport bridge estimates, broken down by country. This is the only
// endpoint that crosses both dimensions: userstats-bridge-transport.csv has no
// country column (its rows are global totals), and the two cross-cuts are
// unsupported — bridge-transport.csv?country= and bridge-country.csv?transport=
// both return "No data available for the given parameters".
const COMBINED_ENDPOINT: &str = "https://metrics.torproject.org/userstats-bridge-combined.csv";
const START_DATE: &str = "2024-01-01";

// The transports rendered in the sidebar. `meek` is deliberately absent: it is
// down to ~3k users globally and reports nothing at all for most countries, so
// a meek row would be permanently blank. `webtunnel` has overtaken it and, in
// several censored countries, Snowflake too. `<OR>`/`!<OR>` (unobfuscated) and
// `obfs3` (retired) are aggregates/legacy and not surfaced.
const TRANSPORT_OBFS4: &str = "obfs4";
const TRANSPORT_SNOWFLAKE: &str = "snowflake";
const TRANSPORT_WEBTUNNEL: &str = "webtunnel";

// Tor's `end` parameter is INCLUSIVE (unlike OONI's exclusive `until`), so
// today's date is the correct upper bound. This was previously a hardcoded
// literal, which silently truncated the series at that date and dropped every
// day that passed afterwards — the window has to move with the clock.
fn end_date() -> String {
    crate::util::date::today_iso()
}


// A small, polite gap between the two requests (relay + bridge).
const REQUEST_PACING: Duration = Duration::from_millis(300);

// Tor Metrics renders these CSVs on demand. The relay and bridge downloads are
// ~6 MB combined; the per-country-per-transport CSV is several times that (it
// carries a row per country *per transport* per day), so the ceiling is sized
// for the largest of the three rather than the smallest. There's no retry path
// here, so it tolerates a slow render rather than failing fast.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

struct RelayRecord {
    country: String,
    date: String,
    users: i64,
    lower: Option<i64>,
}

struct BridgeRecord {
    country: String,
    date: String,
    users: i64,
}

struct CombinedRecord {
    country: String,
    date: String,
    transport: String,
    low: Option<i64>,
    high: Option<i64>,
}

/// Tor publishes per-transport figures as a low/high interval, not a point
/// estimate — they are modelled from directory-request counts, so the width of
/// the range is the published uncertainty. Both bounds are carried through to
/// storage; the sidebar shows the midpoint.
#[derive(Default, Clone, Copy)]
struct TransportBounds {
    low: Option<i64>,
    high: Option<i64>,
}

#[derive(Default, Clone, Copy)]
struct Transports {
    obfs4: TransportBounds,
    snowflake: TransportBounds,
    webtunnel: TransportBounds,
}

struct TorRow {
    id: String,
    country: String,
    date: String,
    relay_users: Option<i64>,
    bridge_users: Option<i64>,
    ratio: f64,
    signal: &'static str,
    transports: Transports,
}

/// Fetches the relay and bridge user-count CSVs for every country in two
/// downloads, joins them by (country, date), and derives a blocking signal.
/// `lower`/`upper` on the relay CSV are Tor's own anomaly-detection bounds — a
/// user count below `lower` is a likely censorship event. When `lower` isn't
/// published for a given day, there's no basis for a HIGH_BLOCKING/LOW call, so
/// that day is marked INCONCLUSIVE rather than guessed at.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    let client = crate::util::http::client("tor-metrics")
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    // Recognised ISO codes, read once. The relay CSV emits non-country rows —
    // an empty code for the global total, plus `??`/`ap` and similar — which
    // must be dropped rather than stored as if they were countries.
    let known = {
        let conn = state
            .lock()
            .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
        crate::db::countries::known_codes(&conn)?
    };

    let relay_records = match fetch_relay_csv(&client).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("tor_metrics: failed to fetch relay CSV: {e}");
            Vec::new()
        }
    };
    tokio::time::sleep(REQUEST_PACING).await;

    let bridge_records = match fetch_bridge_csv(&client).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("tor_metrics: failed to fetch bridge CSV: {e}");
            Vec::new()
        }
    };
    tokio::time::sleep(REQUEST_PACING).await;

    // Independent of the other two: a failure here leaves the transport columns
    // null rather than losing the relay/bridge totals that already parsed.
    let combined_records = match fetch_combined_csv(&client).await {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("tor_metrics: failed to fetch combined transport CSV: {e}");
            Vec::new()
        }
    };

    // Group by uppercased country, keeping only recognised ISO codes.
    let mut relay_by_country: HashMap<String, HashMap<String, (i64, Option<i64>)>> = HashMap::new();
    for r in relay_records {
        let cc = r.country.to_uppercase();
        if !known.contains(&cc) {
            continue;
        }
        relay_by_country
            .entry(cc)
            .or_default()
            .insert(r.date, (r.users, r.lower));
    }
    let mut bridge_by_country: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for b in bridge_records {
        let cc = b.country.to_uppercase();
        if !known.contains(&cc) {
            continue;
        }
        bridge_by_country
            .entry(cc)
            .or_default()
            .insert(b.date, b.users);
    }

    // Same country normalisation as the other two CSVs — the combined endpoint
    // emits the identical lowercase alpha-2 column, plus the same `??`-style
    // pseudo-codes, so uppercasing and filtering against known_codes joins it
    // to the relay/bridge rows without a separate mapping.
    let mut transports_by_country: HashMap<String, HashMap<String, Transports>> = HashMap::new();
    for c in combined_records {
        let cc = c.country.to_uppercase();
        if !known.contains(&cc) {
            continue;
        }
        let bounds = TransportBounds {
            low: c.low,
            high: c.high,
        };
        let entry = transports_by_country
            .entry(cc)
            .or_default()
            .entry(c.date)
            .or_default();
        match c.transport.as_str() {
            TRANSPORT_OBFS4 => entry.obfs4 = bounds,
            TRANSPORT_SNOWFLAKE => entry.snowflake = bounds,
            TRANSPORT_WEBTUNNEL => entry.webtunnel = bounds,
            // Every other transport (<OR>, !<OR>, obfs3, meek) is intentionally
            // dropped — see the TRANSPORT_* consts.
            _ => {}
        }
    }

    let countries: HashSet<String> = relay_by_country
        .keys()
        .chain(bridge_by_country.keys())
        .chain(transports_by_country.keys())
        .cloned()
        .collect();

    let empty_relay: HashMap<String, (i64, Option<i64>)> = HashMap::new();
    let empty_bridge: HashMap<String, i64> = HashMap::new();
    let empty_transports: HashMap<String, Transports> = HashMap::new();

    let mut rows: Vec<TorRow> = Vec::new();
    for country in &countries {
        let relay = relay_by_country.get(country).unwrap_or(&empty_relay);
        let bridge = bridge_by_country.get(country).unwrap_or(&empty_bridge);
        let transports = transports_by_country
            .get(country)
            .unwrap_or(&empty_transports);

        let mut dates: Vec<String> = relay
            .keys()
            .chain(bridge.keys())
            .chain(transports.keys())
            .cloned()
            .collect();
        dates.sort();
        dates.dedup();

        for date in dates {
            let relay_entry = relay.get(&date);
            let relay_users = relay_entry.map(|(users, _)| *users);
            let lower = relay_entry.and_then(|(_, lower)| *lower);
            let bridge_users = bridge.get(&date).copied();
            let signal = classify_blocking(relay_users, lower);
            let ratio = bridge_users.unwrap_or(0) as f64 / (relay_users.unwrap_or(0) as f64 + 1.0);
            let transports = transports.get(&date).copied().unwrap_or_default();

            rows.push(TorRow {
                id: format!("{country}-{date}"),
                country: country.clone(),
                date,
                relay_users,
                bridge_users,
                ratio,
                signal,
                transports,
            });
        }
    }

    if let Err(e) = insert_tor_metrics(state, &rows) {
        eprintln!("tor_metrics: failed to store rows: {e}");
    }

    Ok(())
}

async fn fetch_relay_csv(client: &reqwest::Client) -> Result<Vec<RelayRecord>> {
    let url = format!(
        "{RELAY_ENDPOINT}?start={START_DATE}&end={}&events=off",
        end_date()
    );
    let body = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Columns: date,country,users,lower,upper,frac — read by header name
    // (not position), since the spec says column order may change. Comment
    // lines start with '#'.
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .comment(Some(b'#'))
        .from_reader(body.as_bytes());

    let headers = reader.headers()?.clone();
    let date_col = find_column(&headers, "date");
    let country_col = find_column(&headers, "country");
    let users_col = find_column(&headers, "users");
    let lower_col = find_column(&headers, "lower");

    let (Some(date_col), Some(country_col), Some(users_col)) = (date_col, country_col, users_col)
    else {
        bail!("relay CSV missing required date/country/users columns");
    };

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let Some(date) = record
            .get(date_col)
            .map(str::trim)
            .filter(|d| !d.is_empty())
        else {
            continue;
        };
        // Skip the global-total row (empty country) here; the `??`/`ap` style
        // pseudo-codes survive to the known_codes filter in the caller.
        let Some(country) = record
            .get(country_col)
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            continue;
        };
        let Some(users) = record.get(users_col).and_then(parse_optional_int) else {
            continue;
        };
        let lower = lower_col
            .and_then(|c| record.get(c))
            .and_then(parse_optional_int);
        rows.push(RelayRecord {
            country: country.to_string(),
            date: date.to_string(),
            users,
            lower,
        });
    }
    Ok(rows)
}

async fn fetch_bridge_csv(client: &reqwest::Client) -> Result<Vec<BridgeRecord>> {
    let url = format!("{BRIDGE_ENDPOINT}?start={START_DATE}&end={}", end_date());
    let body = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Columns: date,country,users,frac
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .comment(Some(b'#'))
        .from_reader(body.as_bytes());

    let headers = reader.headers()?.clone();
    let date_col = find_column(&headers, "date");
    let country_col = find_column(&headers, "country");
    let users_col = find_column(&headers, "users");

    let (Some(date_col), Some(country_col), Some(users_col)) = (date_col, country_col, users_col)
    else {
        bail!("bridge CSV missing required date/country/users columns");
    };

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let Some(date) = record
            .get(date_col)
            .map(str::trim)
            .filter(|d| !d.is_empty())
        else {
            continue;
        };
        let Some(country) = record
            .get(country_col)
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            continue;
        };
        let Some(users) = record.get(users_col).and_then(parse_optional_int) else {
            continue;
        };
        rows.push(BridgeRecord {
            country: country.to_string(),
            date: date.to_string(),
            users,
        });
    }
    Ok(rows)
}

/// Per-country, per-transport bridge estimates. Unlike the other two CSVs this
/// one reports `low`/`high` rather than `users`; both are kept.
async fn fetch_combined_csv(client: &reqwest::Client) -> Result<Vec<CombinedRecord>> {
    let url = format!("{COMBINED_ENDPOINT}?start={START_DATE}&end={}", end_date());
    let body = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Columns: date,country,transport,low,high,frac
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .comment(Some(b'#'))
        .from_reader(body.as_bytes());

    let headers = reader.headers()?.clone();
    let date_col = find_column(&headers, "date");
    let country_col = find_column(&headers, "country");
    let transport_col = find_column(&headers, "transport");
    let low_col = find_column(&headers, "low");
    let high_col = find_column(&headers, "high");

    let (Some(date_col), Some(country_col), Some(transport_col)) =
        (date_col, country_col, transport_col)
    else {
        bail!("combined CSV missing required date/country/transport columns");
    };
    // Without at least one bound there is nothing to store, and silently
    // writing null transports for every country would look like "no usage"
    // rather than "the feed changed shape".
    if low_col.is_none() && high_col.is_none() {
        bail!("combined CSV missing both low and high columns");
    }

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let Some(date) = record
            .get(date_col)
            .map(str::trim)
            .filter(|d| !d.is_empty())
        else {
            continue;
        };
        let Some(country) = record
            .get(country_col)
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            continue;
        };
        let Some(transport) = record
            .get(transport_col)
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            continue;
        };
        let low = low_col
            .and_then(|c| record.get(c))
            .and_then(parse_optional_int);
        let high = high_col
            .and_then(|c| record.get(c))
            .and_then(parse_optional_int);
        if low.is_none() && high.is_none() {
            continue;
        }
        rows.push(CombinedRecord {
            country: country.to_string(),
            date: date.to_string(),
            transport: transport.to_string(),
            low,
            high,
        });
    }
    Ok(rows)
}

fn find_column(headers: &csv::StringRecord, name: &str) -> Option<usize> {
    headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case(name))
}

/// `lower`/`upper` (and occasionally `users`) come through as empty strings
/// rather than absent fields when Tor has no anomaly-detection bound for
/// that day — those must read as None, not 0.
fn parse_optional_int(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok().map(|v| v.round() as i64)
}

fn classify_blocking(users: Option<i64>, lower: Option<i64>) -> &'static str {
    match (users, lower) {
        (Some(u), Some(l)) if u < l => "HIGH_BLOCKING",
        (Some(u), Some(l)) if (u as f64) < (l as f64) * 1.2 => "MODERATE",
        (Some(_), Some(_)) => "LOW",
        // No anomaly-detection bound published for this day — nothing to
        // compare against, so we can't call it blocked or not.
        _ => "INCONCLUSIVE",
    }
}

/// All rows in a single transaction. At whole-globe scale this is ~150k rows;
/// inserting them one autocommitted statement at a time (re-locking the DB each
/// time) is what made a full sweep slow, so they share one prepared statement
/// and one commit.
fn insert_tor_metrics(state: &AppState, rows: &[TorRow]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO tor_metrics
             (id, country_code, date, relay_users, bridge_users, bridge_relay_ratio, blocking_signal, source,
              obfs4_low, obfs4_high, snowflake_low, snowflake_high, webtunnel_low, webtunnel_high)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        )?;
        for r in rows {
            stmt.execute(rusqlite::params![
                r.id,
                r.country,
                r.date,
                r.relay_users,
                r.bridge_users,
                r.ratio,
                r.signal,
                "TOR_METRICS",
                r.transports.obfs4.low,
                r.transports.obfs4.high,
                r.transports.snowflake.low,
                r.transports.snowflake.high,
                r.transports.webtunnel.low,
                r.transports.webtunnel.high,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}
