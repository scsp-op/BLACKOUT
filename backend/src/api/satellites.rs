//! Satellite position and orbit-path endpoints.
//!
//! Unlike every other handler in `api/`, these don't take `State<AppState>` —
//! there is no SQLite involved. They take an `Extension<SatelliteCatalog>`
//! instead (mounted in `main.rs` alongside, not instead of, `AppState`); see
//! `crate::satellites` for why.
//!
//! Positions are propagated to "now" and then memoised for a second (see
//! `POSITION_CACHE_TTL`), because every viewer polling within the same second
//! wants the same answer and recomputing it per viewer made CPU scale with
//! audience size rather than with catalog size.

use crate::AppState;
use crate::db::satellite_catalog as store;
use crate::models::satellite::{
    OrbitPoint, OrbitResponse, SatellitePosition, SatelliteStatus, SatellitesResponse,
};
use crate::satellites::{SatelliteCatalog, geodetic, orbit};
use axum::{
    Json,
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long one propagated position set may be reused.
///
/// Every viewer polling inside the same second gets the same answer, so
/// without this the server repeats an identical ~17,000-object SGP4 sweep and
/// JSON serialisation once per viewer — CPU scaling with audience size rather
/// than with catalog size, which is the wrong axis.
///
/// A second costs nothing in accuracy: the frontend polls every 7s, so a
/// position is already up to 7s old by design, and a LEO satellite moves
/// ~7.5 km in a second against a whole-globe pixel worth ~12 km. Overridable
/// via `SATELLITE_POSITION_CACHE_MS`; set it to 0 to disable.
fn position_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| {
        Duration::from_millis(
            std::env::var("SATELLITE_POSITION_CACHE_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
        )
    })
}

/// Serialised responses keyed by the `categories` filter.
///
/// Bounded: the filter comes from a query string, so an arbitrary client can
/// mint unlimited distinct keys. The frontend only ever sends a handful, so
/// anything beyond that is either abuse or a bug, and dropping the whole map
/// is a cheaper answer than an LRU for something that refills in one second.
const CACHE_MAX_KEYS: usize = 32;

struct CachedPositions {
    at: Instant,
    /// `Bytes` rather than `Vec<u8>` so handing either of these to a response
    /// is a refcount bump, not a 1.7 MB copy.
    raw: Bytes,
    /// Gzipped once when the entry is filled. Without this the compression
    /// layer re-compresses the identical body for every viewer, which became
    /// the dominant per-request cost once propagation was memoised — 28ms of
    /// a 40ms request. Compressing once a second instead of once a viewer is
    /// the same saving the position cache makes, one layer up.
    gzipped: Bytes,
}

fn cache() -> &'static Mutex<HashMap<String, CachedPositions>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedPositions>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Serves the pre-encoded body the client can actually accept.
///
/// `Vary` is not optional: the two variants differ only by `Accept-Encoding`,
/// and without it any shared cache between here and the browser could hand a
/// gzipped body to a client that never asked for one. `tower-http`'s
/// compression layer leaves a response alone once `Content-Encoding` is set,
/// so this does not get double-compressed.
fn encoded_response(headers: &HeaderMap, cached: &CachedPositions) -> Response {
    let accepts_gzip = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|e| e.trim().starts_with("gzip")));

    if accepts_gzip {
        (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CONTENT_ENCODING, "gzip"),
                (header::VARY, "accept-encoding"),
            ],
            cached.gzipped.clone(),
        )
            .into_response()
    } else {
        (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::VARY, "accept-encoding"),
            ],
            cached.raw.clone(),
        )
            .into_response()
    }
}

fn gzip(body: &[u8]) -> Result<Bytes, StatusCode> {
    use std::io::Write;
    let mut encoder =
        flate2::write::GzEncoder::new(Vec::with_capacity(body.len() / 3), flate2::Compression::default());
    encoder
        .write_all(body)
        .and_then(|_| encoder.finish())
        .map(Bytes::from)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Decimal places kept on the wire for a position.
///
/// `f64` serialises to ~17 significant figures, and this endpoint sends one
/// position per catalogued object — ~17,000 of them — every 7 seconds to
/// every open tab. Those digits are the single largest thing on the wire and
/// none of them are renderable: 4 decimal places of latitude is ~11 m, while
/// one pixel of a whole-globe view spans roughly 12 km.
///
/// Measured over the live catalog: 2.25 MiB -> 1.75 MiB uncompressed, and
/// 0.65 MiB -> 0.36 MiB after gzip, with 39% less compression CPU because
/// there is less input to compress. It buys egress *and* latency, which
/// compression alone does not.
const COORD_SCALE: f64 = 10_000.0; // 4 dp  ~ 11 m
const ALT_SCALE: f64 = 1_000.0; // 3 dp  ~ 1 m

fn round_to(value: f64, scale: f64) -> f64 {
    (value * scale).round() / scale
}

/// Number of points sampled across one full orbital period for an orbit path.
/// 180 gives a visually smooth curve without an oversized response.
const ORBIT_SAMPLES: usize = 180;

/// Tally of objects per category, independent of any `categories` filter —
/// computed over the *whole* catalog so every legend row (including ones not
/// currently selected/fetched) can show a live count. Pure and cheap (no
/// propagation), so it's recomputed on every request alongside the filtered
/// position list at negligible cost.
fn tally_categories<'a>(categories: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for category in categories {
        *counts.entry(category.to_string()).or_insert(0) += 1;
    }
    counts
}

#[derive(Deserialize)]
pub struct SatellitesQuery {
    /// Comma-separated category list (e.g. `starlink,gps`). Filtering
    /// happens here, before propagation, so an unrequested category is never
    /// computed or sent — bounds both payload size and CPU cost as the
    /// tracked category list grows.
    pub categories: Option<String>,
}

pub async fn list_satellites(
    Extension(catalog): Extension<SatelliteCatalog>,
    Query(params): Query<SatellitesQuery>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let wanted = params.categories.map(|raw| {
        raw.split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    });

    // Sorted so `?categories=a,b` and `?categories=b,a` share one entry.
    let cache_key = match &wanted {
        Some(list) => {
            let mut sorted = list.clone();
            sorted.sort();
            sorted.join(",")
        }
        None => String::new(),
    };
    let ttl = position_cache_ttl();

    // Looked up and released before any work: holding this across the SGP4
    // sweep would serialise every request behind one propagation, which is
    // the opposite of the point.
    if !ttl.is_zero()
        && let Ok(map) = cache().lock()
        && let Some(hit) = map.get(&cache_key)
        && hit.at.elapsed() < ttl
    {
        return Ok(encoded_response(&headers, hit));
    }

    let guard = catalog
        .read()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = Utc::now();
    // Computed once per request, not once per satellite: the thresholds are
    // the same for all ~16k objects.
    let stale_before = crate::satellites::stale_cutoff(now);
    let propagatable_from = crate::satellites::propagation_cutoff(now);

    let mut satellites = Vec::new();
    for sat in guard.objects.iter() {
        if let Some(wanted) = &wanted
            && !wanted.iter().any(|c| c == &sat.category)
        {
            continue;
        }
        // Elements too old to mean anything are skipped before propagation
        // rather than after. SGP4 diverges outright on the worst of them
        // (`eccentricity outside [0, 1[`), which used to log one line per
        // object per request — ~200 lines every 7 seconds per open browser,
        // since this endpoint is polled. The quieter half of the same problem
        // matters more: an object a few months stale propagates without
        // error to a position that can be thousands of kilometres wrong.
        // See `satellites::DEFAULT_MAX_PROPAGATION_AGE_DAYS`.
        if sat.epoch < propagatable_from {
            continue;
        }
        match geodetic::propagate_geodetic(&sat.elements, &sat.constants, now) {
            Ok((lat, lon, alt_km)) => satellites.push(SatellitePosition {
                norad_id: sat.norad_id,
                name: sat.name.clone(),
                category: sat.category.clone(),
                lat: round_to(lat, COORD_SCALE),
                lon: round_to(lon, COORD_SCALE),
                alt_km: round_to(alt_km, ALT_SCALE),
                stale: sat.last_updated < stale_before,
            }),
            Err(e) => {
                eprintln!(
                    "satellites: propagation failed for NORAD {}: {e}",
                    sat.norad_id
                )
            }
        }
    }

    let total = guard.objects.len();
    let category_counts = tally_categories(guard.objects.iter().map(|s| s.category.as_str()));
    // Catalog-wide, not filtered — same reasoning as `category_counts`: the
    // frontend needs to state overall freshness regardless of the selection.
    // A three-way partition rather than fresh/stale, because an object whose
    // elements are too old to propagate was previously counted as *fresh*
    // (we had just accepted the record), which is the most misleading answer
    // available.
    let unpropagatable_count = guard
        .objects
        .iter()
        .filter(|s| s.epoch < propagatable_from)
        .count();
    let stale_count = guard
        .objects
        .iter()
        .filter(|s| s.epoch >= propagatable_from && s.last_updated < stale_before)
        .count();

    let response = SatellitesResponse {
        generated_at: now,
        catalog_updated_at: guard.catalog_updated_at,
        total,
        fresh_count: total - stale_count - unpropagatable_count,
        stale_count,
        unpropagatable_count,
        category_counts,
        satellites,
    };
    drop(guard);

    // Serialised once here rather than per response, so a cache hit skips the
    // JSON encoding as well as the propagation.
    let body = Bytes::from(
        serde_json::to_vec(&response).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    let entry = CachedPositions {
        at: Instant::now(),
        gzipped: gzip(&body)?,
        raw: body,
    };
    let response = encoded_response(&headers, &entry);

    if !ttl.is_zero()
        && let Ok(mut map) = cache().lock()
    {
        // Two requests can miss concurrently and both compute; that is a
        // harmless duplicate once a second, and far cheaper than holding the
        // lock across the sweep to prevent it.
        if map.len() >= CACHE_MAX_KEYS && !map.contains_key(&cache_key) {
            map.clear();
        }
        map.insert(cache_key, entry);
    }

    Ok(response)
}

/// `GET /api/satellites/status` — pipeline diagnostics.
///
/// Takes both extractors because the two halves of the answer live in two
/// places on purpose: what is being *served* comes from the in-memory catalog,
/// and the refresh history comes from SQLite (where it survives restarts).
/// Reading them together is the only way to report, say, a healthy 16k catalog
/// whose CelesTrak half has not refreshed in a day.
pub async fn satellites_status(
    Extension(catalog): Extension<SatelliteCatalog>,
    State(state): State<AppState>,
) -> Result<Json<SatelliteStatus>, StatusCode> {
    let now = Utc::now();
    let stale_before = crate::satellites::stale_cutoff(now);
    let propagatable_from = crate::satellites::propagation_cutoff(now);

    let (satellite_count, stale_satellite_count, unpropagatable_satellite_count, current_data_sources) = {
        let guard = catalog
            .read()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let unpropagatable = guard
            .objects
            .iter()
            .filter(|s| s.epoch < propagatable_from)
            .count();
        let stale = guard
            .objects
            .iter()
            .filter(|s| s.epoch >= propagatable_from && s.last_updated < stale_before)
            .count();
        let sources = tally_categories(guard.objects.iter().map(|s| s.source.as_str()));
        (guard.objects.len(), stale, unpropagatable, sources)
    };

    let conn = state.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let last_successful_refresh = store::get_state_ts(&conn, store::LAST_SUCCESSFUL_REFRESH);
    let body = SatelliteStatus {
        satellite_count,
        fresh_satellite_count: satellite_count - stale_satellite_count - unpropagatable_satellite_count,
        stale_satellite_count,
        unpropagatable_satellite_count,
        last_successful_refresh,
        catalog_age_seconds: last_successful_refresh.map(|t| (now - t).num_seconds()),
        last_celestrak_success: store::get_state_ts(&conn, store::LAST_CELESTRAK_SUCCESS),
        last_satnogs_success: store::get_state_ts(&conn, store::LAST_SATNOGS_SUCCESS),
        refresh_status: store::get_state(&conn, store::LAST_REFRESH_STATUS),
        current_data_sources,
        persisted_count: store::count(&conn).unwrap_or(0),
        stale_after_hours: (now - stale_before).num_seconds() as f64 / 3600.0,
        max_propagation_age_days: crate::satellites::max_propagation_age_days(),
    };
    Ok(Json(body))
}

pub async fn satellite_orbit(
    Extension(catalog): Extension<SatelliteCatalog>,
    Path(norad_id): Path<u64>,
) -> Result<Json<OrbitResponse>, StatusCode> {
    let guard = catalog
        .read()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let sat = guard
        .objects
        .iter()
        .find(|s| s.norad_id == norad_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let now = Utc::now();
    let stale_before = crate::satellites::stale_cutoff(now);

    // Same cutoff as `list_satellites`, for the same reason: a full orbit
    // path sampled from a years-old element set is 180 points of fiction.
    // 422 rather than 404 — the object genuinely exists in the catalog, it
    // just cannot be propagated.
    if sat.epoch < crate::satellites::propagation_cutoff(now) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let period_minutes = orbit::period_minutes(sat.elements.mean_motion);
    let points = orbit::sample_orbit(
        &sat.elements,
        &sat.constants,
        now,
        period_minutes,
        ORBIT_SAMPLES,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let segments = orbit::split_at_antimeridian(points)
        .into_iter()
        .map(|segment| {
            segment
                .into_iter()
                .map(|(lat, lon, alt_km)| OrbitPoint { lat, lon, alt_km })
                .collect()
        })
        .collect();

    Ok(Json(OrbitResponse {
        norad_id: sat.norad_id,
        name: sat.name.clone(),
        period_minutes,
        generated_at: now,
        source: sat.source.as_str().to_string(),
        epoch: sat.epoch,
        last_updated: sat.last_updated,
        age_hours: (now - sat.epoch).num_seconds() as f64 / 3600.0,
        stale: sat.last_updated < stale_before,
        segments,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4 dp of latitude is ~11 m and 3 dp of altitude is ~1 m. Anything
    /// coarser starts to be visible when the camera is zoomed in on a single
    /// satellite, which the orbit view allows.
    #[test]
    fn rounding_keeps_metre_scale_precision() {
        let lat = 29.06201298981973;
        assert_eq!(round_to(lat, COORD_SCALE), 29.062);
        assert!((round_to(lat, COORD_SCALE) - lat).abs() < 0.0001);

        let alt = 996.5845878672644;
        assert_eq!(round_to(alt, ALT_SCALE), 996.585);
        assert!((round_to(alt, ALT_SCALE) - alt).abs() < 0.001);
    }

    #[test]
    fn rounding_handles_negative_and_zero() {
        assert_eq!(round_to(-128.18813120224982, COORD_SCALE), -128.1881);
        assert_eq!(round_to(0.0, COORD_SCALE), 0.0);
    }

    #[test]
    fn tally_categories_counts_each_category_independently() {
        let categories = vec!["starlink", "starlink", "navigation", "military", "starlink"];
        let counts = tally_categories(categories.into_iter());

        assert_eq!(counts.get("starlink"), Some(&3));
        assert_eq!(counts.get("navigation"), Some(&1));
        assert_eq!(counts.get("military"), Some(&1));
        assert_eq!(counts.get("earthobs"), None);
        assert_eq!(counts.values().sum::<usize>(), 5);
    }

    #[test]
    fn tally_categories_of_empty_input_is_empty() {
        assert!(tally_categories(std::iter::empty()).is_empty());
    }
}
