use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;

/// One satellite's position, propagated fresh to `SatellitesResponse::generated_at`.
#[derive(Debug, Clone, Serialize)]
pub struct SatellitePosition {
    pub norad_id: u64,
    pub name: String,
    pub category: String,
    pub lat: f64,
    pub lon: f64,
    pub alt_km: f64,
    /// True when this object's orbital data has not been refreshed within
    /// `SATELLITE_STALE_AFTER_HOURS`. Skipped from the payload when false,
    /// which is the overwhelmingly common case — this endpoint is polled every
    /// few seconds for ~16k objects, so a field that would be `false` on
    /// nearly every one of them is not worth the bytes.
    ///
    /// A stale satellite is still returned. Staleness annotates data; it never
    /// hides an object.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SatellitesResponse {
    /// UTC instant every position in this response was propagated to.
    pub generated_at: DateTime<Utc>,
    /// When the orbital-element catalog itself was last successfully
    /// refreshed from CelesTrak. `None` before the first successful fetch.
    pub catalog_updated_at: Option<DateTime<Utc>>,
    /// Total objects in the catalog, independent of `categories` filtering —
    /// lets the frontend show a live "All Satellites" count.
    pub total: usize,
    /// How the catalog partitions by data age. All three are catalog-wide,
    /// independent of `categories`, and `fresh + stale + unpropagatable ==
    /// total`.
    ///
    /// `fresh`/`stale` split on `last_updated` — how recently we accepted a
    /// better record. `unpropagatable` splits on `epoch`, the element set's
    /// own age, and those objects are excluded from `satellites` entirely
    /// because SGP4 cannot produce a meaningful position from them (see
    /// `satellites::DEFAULT_MAX_PROPAGATION_AGE_DAYS`). They are deliberately
    /// counted out of `fresh` rather than left in it: an object carrying
    /// 1975 orbital data is not fresh in any sense a caller means.
    pub fresh_count: usize,
    pub stale_count: usize,
    pub unpropagatable_count: usize,
    /// Object count per category, likewise independent of `categories`
    /// filtering, so every legend row can show a live count regardless of
    /// which one is currently selected/fetched.
    pub category_counts: HashMap<String, usize>,
    pub satellites: Vec<SatellitePosition>,
}

/// Diagnostics for the satellite pipeline — `GET /api/satellites/status`.
///
/// Exists so a degraded refresh is answerable without reading container logs:
/// every field distinguishes a state that previously looked identical from
/// the outside (provider unreachable vs. one group failed vs. partial result
/// vs. stale cache being served vs. a clean refresh).
#[derive(Debug, Clone, Serialize)]
pub struct SatelliteStatus {
    /// Objects in the canonical catalog currently being served.
    pub satellite_count: usize,
    pub fresh_satellite_count: usize,
    pub stale_satellite_count: usize,
    /// Objects whose element-set epoch is too old to propagate, so they are
    /// in the catalog but never appear in `/api/satellites`. A number that
    /// climbs here while `satellite_count` holds steady is SatNOGS
    /// republishing long-dead objects, not a refresh problem.
    pub unpropagatable_satellite_count: usize,
    /// Last refresh that merged at least one record, and how long ago that is.
    pub last_successful_refresh: Option<DateTime<Utc>>,
    pub catalog_age_seconds: Option<i64>,
    /// Last time each provider returned at least one usable record. A `None`
    /// here alongside a healthy `satellite_count` is the signature of "this
    /// provider is down and we are serving its data from the store".
    pub last_celestrak_success: Option<DateTime<Utc>>,
    pub last_satnogs_success: Option<DateTime<Utc>>,
    /// `complete` | `degraded` | `failed` for the most recent cycle.
    pub refresh_status: Option<String>,
    /// Which providers the currently-served element sets actually came from,
    /// with a count each — the honest answer to "where is this data from",
    /// as opposed to which providers are configured.
    pub current_data_sources: HashMap<String, usize>,
    /// Row count in the persistent store. A mismatch against
    /// `satellite_count` means records were rejected at load, not lost.
    pub persisted_count: usize,
    pub stale_after_hours: f64,
    pub max_propagation_age_days: f64,
}

/// One point on a sampled orbit path.
#[derive(Debug, Clone, Serialize)]
pub struct OrbitPoint {
    pub lat: f64,
    pub lon: f64,
    pub alt_km: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrbitResponse {
    pub norad_id: u64,
    pub name: String,
    pub period_minutes: f64,
    pub generated_at: DateTime<Utc>,
    /// Provenance and freshness of the element set this path was propagated
    /// from. Carried here rather than on every position in
    /// `SatellitesResponse` because this endpoint returns one object, so the
    /// per-object detail costs nothing — whereas the position list is polled
    /// every few seconds for the whole ~16k catalog.
    ///
    /// `source` is which provider supplied these elements, `epoch` is the
    /// upstream element-set epoch, `last_updated` is when we last accepted
    /// better elements for it, and `age_hours` is the age of that epoch.
    pub source: String,
    pub epoch: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub age_hours: f64,
    pub stale: bool,
    /// Pre-split at the antimeridian so each inner vec can be drawn as its
    /// own polyline without a spurious wraparound line.
    pub segments: Vec<Vec<OrbitPoint>>,
}
