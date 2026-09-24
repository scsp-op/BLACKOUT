//! Shared in-memory satellite state: the cached orbital-element catalog and
//! the pure propagation/geometry helpers used to turn it into positions.
//!
//! This deliberately does **not** go through `AppState`/SQLite the way every
//! other fetched data source in this backend does. That pattern persists
//! fetched rows into the shared `Arc<Mutex<Connection>>` and reads them back
//! with plain SQL; it fits data that changes hourly and is read occasionally.
//! Satellite positions are recomputed on *every* `GET /api/satellites`
//! request (by design — see the API handlers), so routing that traffic
//! through the one mutex guarding every other endpoint's database access would
//! serialize unrelated requests behind satellite work. A separate
//! `Arc<RwLock<...>>`, mounted as an Axum `Extension` alongside (not instead
//! of) `AppState`, avoids that contention without touching any existing
//! handler.
//!
//! The `RwLock` is a *projection* of the canonical catalog, which lives in
//! SQLite (`db::satellite_catalog`, one row per NORAD ID). Membership is owned
//! by that table; this in-memory copy is rebuilt from it at boot and after
//! each refresh. Nothing in the request path can shrink it.

pub mod geodetic;
pub mod orbit;

use chrono::{DateTime, Utc};
use std::sync::{Arc, RwLock};

/// Shared handle to the cached catalog. Cloning is cheap (just the `Arc`);
/// every handler and the background refresh loop hold their own clone.
pub type SatelliteCatalog = Arc<RwLock<CatalogInner>>;

pub fn new_catalog() -> SatelliteCatalog {
    Arc::new(RwLock::new(CatalogInner::default()))
}

#[derive(Default)]
pub struct CatalogInner {
    pub objects: Vec<CachedSatellite>,
    /// When the canonical catalog was last written — i.e. the last refresh
    /// cycle that merged at least one record, or the persisted value restored
    /// at boot. `None` only when the catalog has never held anything.
    pub catalog_updated_at: Option<DateTime<Utc>>,
}

/// Where a given element set came from.
///
/// The ordering of the variants is the tie-break precedence used by
/// `merge` when two sources carry the *same* element-set epoch for one
/// object — see `Source::rank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// CelesTrak's Starlink supplemental feed (SpaceX-derived ephemeris).
    CelestrakSupplemental,
    /// CelesTrak's general GP catalog (a Space-Track republish).
    Celestrak,
    /// SatNOGS' own TLE catalogue (a smaller, independently-hosted
    /// re-publish of largely the same upstream data).
    Satnogs,
}

impl Source {
    /// Lower rank wins a tie on element-set epoch.
    ///
    /// The reasoning, in order:
    ///   0. **Supplemental** is SpaceX's own ephemeris for its own satellites,
    ///      published ahead of (and more precisely than) the general GP sweep.
    ///   1. **CelesTrak GP** is the authoritative bulk republish of
    ///      Space-Track's catalog, and carries a correct category tag because
    ///      we asked for a specific GROUP.
    ///   2. **SatNOGS** republishes largely the same upstream elements but a
    ///      few thousand objects' worth, on its own cadence, with no category
    ///      information — so at equal epoch it is the least informative copy.
    ///
    /// This only ever arbitrates *ties*. A newer epoch from any source always
    /// beats an older epoch from any other: freshness of the orbital data is
    /// the primary rule, provenance is only the tie-break.
    pub fn rank(self) -> i64 {
        match self {
            Source::CelestrakSupplemental => 0,
            Source::Celestrak => 1,
            Source::Satnogs => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Source::CelestrakSupplemental => "celestrak-supplemental",
            Source::Celestrak => "celestrak",
            Source::Satnogs => "satnogs",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "celestrak-supplemental" => Some(Source::CelestrakSupplemental),
            "celestrak" => Some(Source::Celestrak),
            "satnogs" => Some(Source::Satnogs),
            _ => None,
        }
    }
}

/// One normalized record from an upstream fetch, ready to be merged into the
/// canonical catalog. Both CelesTrak (JSON GP) and SatNOGS (3-line TLE) are
/// converted to this shape before anything touches the database, so the merge
/// logic never has to care which provider a record came from.
#[derive(Debug, Clone)]
pub struct SourceRecord {
    pub norad_id: u64,
    pub name: String,
    pub category: String,
    /// Index of the CelesTrak group this came from in the configured group
    /// list; lower wins the category tag. SatNOGS records (no category
    /// information at all) use `CATEGORY_RANK_UNKNOWN` so they can never
    /// downgrade a category a real CelesTrak group already assigned.
    pub category_rank: i64,
    pub source: Source,
    pub elements: sgp4::Elements,
}

/// Category rank for a record that carries no category information. Higher
/// than any real group index, so `merge` never lets it overwrite a
/// CelesTrak-assigned category.
pub const CATEGORY_RANK_UNKNOWN: i64 = 1_000;

impl SourceRecord {
    /// The element set's own epoch — when the orbital state it describes was
    /// determined upstream. This, not our own fetch time, is what makes one
    /// record fresher than another: re-downloading an unchanged TLE does not
    /// make it newer orbital data.
    pub fn epoch(&self) -> DateTime<Utc> {
        self.elements.datetime.and_utc()
    }
}

/// One tracked object: its orbital elements plus the propagator constants
/// derived from them once (at catalog-refresh time), so a position request
/// only ever pays for `propagate`, not for re-deriving `Constants`.
pub struct CachedSatellite {
    pub norad_id: u64,
    pub name: String,
    pub category: String,
    pub elements: sgp4::Elements,
    pub constants: sgp4::Constants,
    /// Which provider supplied the element set currently held here.
    pub source: Source,
    /// The element set's own epoch (see `SourceRecord::epoch`).
    pub epoch: DateTime<Utc>,
    /// When we last *replaced* this object's elements with a better record.
    /// Distinct from `epoch`: this is our bookkeeping, `epoch` is upstream's.
    pub last_updated: DateTime<Utc>,
}

/// Default age past which an object's orbital data is reported as stale.
///
/// Deliberately far longer than the ~2h refresh cadence: a TLE stays usable
/// for position display for days (SGP4 error grows gradually, not off a
/// cliff), and the point of this flag is to *annotate* degraded data, never to
/// hide it. An object is never removed for being stale — see
/// `db::satellite_catalog`'s module docs on freshness vs existence.
pub const DEFAULT_STALE_AFTER_HOURS: f64 = 24.0;

/// Instant before which an object's `last_updated` counts as stale.
/// `SATELLITE_STALE_AFTER_HOURS` overrides the default.
pub fn stale_cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    let hours = std::env::var("SATELLITE_STALE_AFTER_HOURS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|h| h.is_finite() && *h > 0.0)
        .unwrap_or(DEFAULT_STALE_AFTER_HOURS);
    now - chrono::Duration::milliseconds((hours * 3_600_000.0) as i64)
}

/// Default age past which an element set is too old to propagate at all.
///
/// Deliberately measured against `epoch`, not `last_updated`, and that
/// distinction is the whole point. `stale_cutoff` above asks "when did *we*
/// last accept a better record for this object" — our own bookkeeping. This
/// asks "how old is the orbital data itself", which is upstream's.
///
/// The two come apart badly. SatNOGS republishes objects whose element sets
/// are years old: in one observed catalog, 221 objects had an epoch more than
/// 30 days old — the oldest from **February 1975** — and every single one was
/// reported `fresh`, because we had accepted the record minutes earlier.
///
/// Thirty days is well past the point where SGP4 output means anything. Error
/// grows by kilometres per day, and far enough out the propagator diverges
/// outright: the observed failure is `eccentricity ... outside the range
/// [0, 1[`, which is the secular drag term having driven the orbit to
/// nonsense. Those errors are only the visible tip — an object a few months
/// stale propagates *successfully* to a position that can be thousands of
/// kilometres wrong, and is drawn on the globe with nothing to say it is
/// fiction. Refusing to propagate is the honest answer; a wrong dot is worse
/// than no dot.
///
/// This does not delete anything. The object stays in the catalog, is counted
/// in `total`, and reappears the moment any provider supplies fresher
/// elements for it — the additive-catalog invariant in
/// `db::satellite_catalog` is untouched.
pub const DEFAULT_MAX_PROPAGATION_AGE_DAYS: f64 = 30.0;

/// `SATELLITE_MAX_PROPAGATION_AGE_DAYS` overrides the default.
pub fn max_propagation_age_days() -> f64 {
    std::env::var("SATELLITE_MAX_PROPAGATION_AGE_DAYS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(DEFAULT_MAX_PROPAGATION_AGE_DAYS)
}

/// Epoch before which an element set is not propagated at all.
pub fn propagation_cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    now - chrono::Duration::milliseconds((max_propagation_age_days() * 86_400_000.0) as i64)
}

#[cfg(test)]
mod propagation_age_tests {
    use super::*;

    #[test]
    fn the_default_cutoff_is_thirty_days_back() {
        let now = Utc::now();
        let elapsed = now - propagation_cutoff(now);
        assert_eq!(elapsed.num_days(), 30);
    }

    /// The regression this cutoff exists for.
    ///
    /// SatNOGS republishes long-dead objects, so the catalog accepts a record
    /// *now* whose orbital elements are years old. `stale_cutoff` looks at
    /// `last_updated` and therefore calls it fresh — in one observed catalog
    /// that was true of 221 objects, the oldest carrying elements from
    /// February 1975. Only an `epoch`-based test catches them.
    #[test]
    fn a_just_accepted_object_with_ancient_elements_is_still_refused() {
        let now = Utc::now();

        // Exactly the observed shape: ONDOSAT-OWL-9, accepted 20 minutes ago,
        // element-set epoch 642 days old.
        let last_updated = now - chrono::Duration::minutes(20);
        let epoch = now - chrono::Duration::days(642);

        assert!(
            last_updated >= stale_cutoff(now),
            "last_updated-based staleness calls this fresh — that is the bug"
        );
        assert!(
            epoch < propagation_cutoff(now),
            "epoch-based cutoff must refuse to propagate it"
        );
    }

    #[test]
    fn a_normally_aged_element_set_is_still_propagated() {
        let now = Utc::now();
        // CelesTrak's own catalog routinely carries epochs a couple of weeks
        // old; the cutoff must not reject ordinary data.
        assert!(now - chrono::Duration::days(21) >= propagation_cutoff(now));
    }
}
