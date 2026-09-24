use crate::AppState;
use crate::util::date::today_iso;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::time::Duration;

// Two global freedom indices, sourced from Our World in Data's clean grapher
// CSVs (which republish V-Dem and RSF per country-year). Both cover ~180
// countries, giving the whole world a baseline score. Stored in the shared
// `country_scores` table.
//
// OWID serves these behind a 301 to a canonical slug; reqwest follows
// redirects by default. `csvType=full` returns every country-year.
const VDEM_URL: &str = "https://ourworldindata.org/grapher/freedom-of-expression-index.csv?csvType=full&useColumnShortNames=true";
const RSF_URL: &str = "https://ourworldindata.org/grapher/press-freedom-index-rsf.csv?csvType=full&useColumnShortNames=true";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// V-Dem's internet-censorship indicators, extracted from the Country-Year
// Full+Others v16 dataset and committed as a seed. Two of the three are
// Digital Society Project variables carried inside V-Dem; the third
// (v2mecenefi) is a V-Dem core Media variable. OWID's mirror publishes one
// indicator per grapher CSV and carries none of them, which is why this is a
// committed file rather than another download.
const DSP_SEED_FILE: &str = "vdem_dsp_v16.csv";

// Each variable is stored on its own ordinal scale, and the denominators are
// NOT the same: v2mecenefi has four categories (0-3), the other two have five
// (0-4). Normalising v2mecenefi against 4 would understate every country by a
// quarter. All three are coded higher = LESS censorship, which already matches
// this tool's "higher = more free" convention, so none of them is inverted —
// despite names ("censorship effort", "filtering in practice") that read the
// other way.
const DSP_VARS: [(&str, f64); 3] = [
    ("v2smgovfilprc", 4.0),
    ("v2smgovshut", 4.0),
    ("v2mecenefi", 3.0),
];

/// A point estimate with V-Dem's credible interval, all normalised 0-100.
#[derive(Clone, Copy, Default)]
struct Bounded {
    point: Option<f64>,
    low: Option<f64>,
    high: Option<f64>,
}

#[derive(Clone, Copy, Default)]
struct DspScores {
    filtering: Bounded,
    shutdown: Bounded,
    censorship: Bounded,
}

/// Fetches both indices and upserts the latest year per country into
/// `country_scores`. Each source is independent — one failing does not stop
/// the other.
pub async fn fetch_and_store(state: &AppState) -> Result<()> {
    let alpha3 = {
        let conn = state
            .lock()
            .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
        crate::db::countries::alpha3_to_code(&conn)?
    };

    let client = crate::util::http::client("owid")
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    // Local read, so a failure here shouldn't cost us the OWID scores — the
    // V_DEM rows just carry no sub-scores.
    let dsp = match load_dsp_seed(&alpha3) {
        Ok(map) => map,
        Err(e) => {
            eprintln!("indices: V-Dem censorship seed unavailable: {e}");
            HashMap::new()
        }
    };

    if let Err(e) = fetch_source(state, &client, &alpha3, Source::Vdem, &dsp).await {
        eprintln!("indices: V-Dem fetch failed: {e}");
    }
    // RSF rows carry no sub-scores; the V-Dem indicators belong to V_DEM only.
    if let Err(e) = fetch_source(state, &client, &alpha3, Source::Rsf, &HashMap::new()).await {
        eprintln!("indices: RSF fetch failed: {e}");
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Source {
    Vdem,
    Rsf,
}

impl Source {
    fn url(self) -> &'static str {
        match self {
            Source::Vdem => VDEM_URL,
            Source::Rsf => RSF_URL,
        }
    }
    fn db_name(self) -> &'static str {
        match self {
            Source::Vdem => "V_DEM",
            Source::Rsf => "RSF",
        }
    }
}

/// The value each source stores, normalised to a 0–100 "higher = more free"
/// scale so all three indices in the sidebar point the same way.
struct Normalised {
    score_overall: f64,
    classification: String,
}

fn normalise(source: Source, raw: f64) -> Normalised {
    match source {
        // V-Dem's freedom-of-expression estimate is already 0–1, higher = freer.
        Source::Vdem => {
            let score = (raw * 100.0).clamp(0.0, 100.0);
            Normalised {
                score_overall: (score * 10.0).round() / 10.0,
                classification: vdem_band(score),
            }
        }
        // OWID's RSF series is the pre-2022 index, where the score is an abuse
        // measure (higher = LESS free). Invert to a 0–100 freedom score so it
        // reads the same direction as the others; the classification is derived
        // from the original value using RSF's old bands.
        Source::Rsf => {
            let inverted = (100.0 - raw).clamp(0.0, 100.0);
            Normalised {
                score_overall: (inverted * 10.0).round() / 10.0,
                classification: rsf_band(raw),
            }
        }
    }
}

fn vdem_band(score_0_100: f64) -> String {
    let label = if score_0_100 >= 80.0 {
        "Very free"
    } else if score_0_100 >= 60.0 {
        "Free"
    } else if score_0_100 >= 40.0 {
        "Partly free"
    } else if score_0_100 >= 20.0 {
        "Repressed"
    } else {
        "Highly repressed"
    };
    label.to_string()
}

// RSF's pre-2022 bands, defined on the raw (higher = worse) score.
fn rsf_band(raw: f64) -> String {
    let label = if raw < 15.0 {
        "Good"
    } else if raw < 25.0 {
        "Satisfactory"
    } else if raw < 35.0 {
        "Problematic"
    } else if raw < 55.0 {
        "Difficult"
    } else {
        "Very serious"
    };
    label.to_string()
}

/// Reads the committed V-Dem seed and keys it by our alpha-2 code, using the
/// same `alpha3_to_code` map the OWID indices already join through.
///
/// Unmatched rows are reported rather than dropped in silence: V-Dem carries
/// historical and sub-state entities (Somaliland, Zanzibar, Republic of
/// Vietnam) whose `country_text_id` is not ISO 3166-1, and those legitimately
/// have nowhere to land — but a *large* miss count would mean the join broke,
/// and that has to be visible.
fn load_dsp_seed(alpha3: &HashMap<String, String>) -> Result<HashMap<String, DspScores>> {
    // Same SEED_DIR resolution as the JSON seeds.
    let path = crate::db::seed::seed_path(DSP_SEED_FILE);
    let body = std::fs::read_to_string(&path).with_context(|| {
        format!("could not read `{path}` — set SEED_DIR, or run from `backend/`")
    })?;

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(body.as_bytes());
    let headers = reader.headers()?.clone();

    let col = |name: &str| headers.iter().position(|h| h.trim() == name);
    let Some(code_col) = col("country_text_id") else {
        anyhow::bail!("{path} missing country_text_id column");
    };

    // (point, low, high) column indices per variable, in DSP_VARS order.
    let mut var_cols = Vec::new();
    for (var, max) in DSP_VARS {
        let point = col(&format!("{var}_osp"));
        let low = col(&format!("{var}_osp_codelow"));
        let high = col(&format!("{var}_osp_codehigh"));
        let Some(point) = point else {
            anyhow::bail!("{path} missing {var}_osp column");
        };
        var_cols.push((point, low, high, max));
    }

    let mut out: HashMap<String, DspScores> = HashMap::new();
    let mut unmatched: Vec<String> = Vec::new();

    for record in reader.records() {
        let record = record?;
        let iso3 = record.get(code_col).unwrap_or("").trim().to_uppercase();
        if iso3.is_empty() {
            continue;
        }
        let Some(country_code) = alpha3.get(&iso3) else {
            unmatched.push(iso3);
            continue;
        };

        // Normalised onto 0-100 against each variable's own maximum.
        let read = |point: usize, low: Option<usize>, high: Option<usize>, max: f64| Bounded {
            point: parse_normalised(record.get(point), max),
            low: low.and_then(|c| parse_normalised(record.get(c), max)),
            high: high.and_then(|c| parse_normalised(record.get(c), max)),
        };

        out.insert(
            country_code.clone(),
            DspScores {
                filtering: read(var_cols[0].0, var_cols[0].1, var_cols[0].2, var_cols[0].3),
                shutdown: read(var_cols[1].0, var_cols[1].1, var_cols[1].2, var_cols[1].3),
                censorship: read(var_cols[2].0, var_cols[2].1, var_cols[2].2, var_cols[2].3),
            },
        );
    }

    if !unmatched.is_empty() {
        unmatched.sort();
        unmatched.dedup();
        println!(
            "indices: V-Dem seed matched {} countries; {} unmatched country_text_id (not ISO 3166-1): {}",
            out.len(),
            unmatched.len(),
            unmatched.join(", ")
        );
    }
    Ok(out)
}

/// `(osp / max) * 100`, rounded to 1dp and clamped. No inversion — see DSP_VARS.
fn parse_normalised(raw: Option<&str>, max: f64) -> Option<f64> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    let osp = value.parse::<f64>().ok()?;
    let score = (osp / max * 100.0).clamp(0.0, 100.0);
    Some((score * 10.0).round() / 10.0)
}

async fn fetch_source(
    state: &AppState,
    client: &reqwest::Client,
    alpha3: &HashMap<String, String>,
    source: Source,
    dsp: &HashMap<String, DspScores>,
) -> Result<()> {
    let body = client
        .get(source.url())
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    // Keep only the latest year per country. OWID rows are one country-year
    // each: entity, code (ISO3), year, value[, region].
    let mut latest: HashMap<String, (i64, f64)> = HashMap::new();
    let mut reader = csv::Reader::from_reader(body.as_bytes());
    for record in reader.records() {
        let record = record?;
        let code_iso3 = record.get(1).unwrap_or("").trim().to_uppercase();
        if code_iso3.is_empty() {
            continue; // regional/World aggregates carry no ISO3
        }
        let Some(country_code) = alpha3.get(&code_iso3) else {
            continue; // OWID_* pseudo-codes and non-countries
        };
        let Some(year) = record.get(2).and_then(|y| y.trim().parse::<i64>().ok()) else {
            continue;
        };
        let Some(value) = record.get(3).and_then(|v| v.trim().parse::<f64>().ok()) else {
            continue; // blank cells
        };

        latest
            .entry(country_code.clone())
            .and_modify(|cur| {
                if year > cur.0 {
                    *cur = (year, value);
                }
            })
            .or_insert((year, value));
    }

    let mut stored = 0usize;
    for (country_code, (year, raw)) in &latest {
        if let Err(e) = insert_score(
            state,
            source,
            country_code,
            *year,
            *raw,
            dsp.get(country_code),
        ) {
            eprintln!(
                "indices: failed to store {} for {country_code}: {e}",
                source.db_name()
            );
        } else {
            stored += 1;
        }
    }
    println!("indices: stored {stored} {} scores", source.db_name());
    Ok(())
}

fn insert_score(
    state: &AppState,
    source: Source,
    country_code: &str,
    year: i64,
    raw: f64,
    dsp: Option<&DspScores>,
) -> Result<()> {
    let n = normalise(source, raw);
    // Keyed without the year so a re-run refreshes in place rather than
    // accumulating a row per year; the latest year is what we keep.
    let id = format!("{country_code}-{}", source.db_name());
    let d = dsp.copied().unwrap_or_default();

    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    // The DSP sub-scores are written in this same statement rather than by a
    // follow-up UPDATE: this is INSERT OR REPLACE, which deletes and re-inserts
    // the row, so anything not listed here would be nulled on the next refresh.
    conn.execute(
        "INSERT OR REPLACE INTO country_scores
         (id, country_code, source, year, score_overall, score_access, score_content, score_rights, classification, last_updated,
          score_vdem_filtering, score_vdem_filtering_low, score_vdem_filtering_high,
          score_vdem_shutdown, score_vdem_shutdown_low, score_vdem_shutdown_high,
          score_vdem_censorship, score_vdem_censorship_low, score_vdem_censorship_high)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        rusqlite::params![
            id,
            country_code,
            source.db_name(),
            year,
            n.score_overall,
            Option::<f64>::None,
            Option::<f64>::None,
            Option::<f64>::None,
            n.classification,
            today_iso(),
            d.filtering.point,
            d.filtering.low,
            d.filtering.high,
            d.shutdown.point,
            d.shutdown.low,
            d.shutdown.high,
            d.censorship.point,
            d.censorship.low,
            d.censorship.high,
        ],
    )?;
    Ok(())
}
