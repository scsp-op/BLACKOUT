//! The canonical, persistent satellite catalog — one row per NORAD ID.
//!
//! This module owns the single most important invariant in the satellite
//! pipeline:
//!
//! > Once a healthy catalog exists, a temporary upstream failure can never
//! > replace it with a smaller one.
//!
//! It holds by construction rather than by checking. There is no code path
//! here that deletes a satellite: `merge` only ever inserts new NORAD IDs and
//! updates existing ones. A provider that returns 1,500 objects instead of
//! 16,000 therefore updates 1,500 rows and leaves the other ~14,500 exactly
//! as they were — the "partial response wipes the catalog" failure mode is not
//! defended against, it is unrepresentable.
//!
//! Membership (which satellites exist) and freshness (how current each one's
//! orbital data is) are deliberately separate columns, so degraded upstream
//! data makes objects *stale*, never *absent*.

use crate::AppState;
use crate::satellites::{Source, SourceRecord};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::collections::HashMap;

/// One row of the canonical catalog, as read back at startup.
pub struct StoredSatellite {
    pub norad_id: u64,
    pub name: String,
    pub category: String,
    pub elements: sgp4::Elements,
    pub source: Source,
    pub epoch: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
}

/// What one `merge` call actually changed. Every field is a real count taken
/// inside the transaction, so the refresh log can distinguish "nothing came
/// back" from "everything came back but nothing had moved on".
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MergeOutcome {
    /// NORAD IDs that were not in the catalog before this merge.
    pub added: usize,
    /// Existing rows whose elements this merge replaced with a better record.
    pub updated: usize,
    /// Existing rows a source listed, but with elements no better than what we
    /// already had (same epoch re-download, or an older element set). Only
    /// `last_seen` was touched.
    pub unchanged: usize,
    /// Rows no source mentioned at all this cycle, kept verbatim.
    pub retained: usize,
    /// Catalog size after the merge committed.
    pub total: usize,
}

/// Timestamp format for the stored `epoch` / `last_seen` / `last_updated`
/// columns: fixed-width UTC, so the strings sort in the same order as the
/// instants they represent and a plain `MAX()`/`<` in SQL is meaningful.
fn iso(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

fn parse_iso(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Per-NORAD-ID state needed to decide whether an incoming record is an
/// improvement, without loading 16k element-set blobs to find out.
struct ExistingMeta {
    epoch: DateTime<Utc>,
    source_rank: i64,
}

/// True when `incoming` is better orbital data than what we already hold.
///
/// The rules, in order, and the *only* place they are decided:
///
/// 1. **Newer element-set epoch wins, whatever the source.** The epoch is
///    when upstream determined the orbital state. It is the only
///    provider-independent measure of freshness, and re-downloading an
///    unchanged TLE does not make it newer data.
/// 2. **At equal epoch, the better-ranked source wins** — supplemental,
///    then CelesTrak GP, then SatNOGS (see `satellites::Source::rank` for
///    why that order). This is how the two providers complement rather than
///    fight each other: whoever has genuinely moved on supplies the elements,
///    and when they agree we keep the more informative copy.
/// 3. **Otherwise keep what we have.** An older epoch never overwrites a
///    newer one, so a provider serving a stale cache cannot roll us back.
fn is_improvement(
    incoming_epoch: DateTime<Utc>,
    incoming_rank: i64,
    current: &ExistingMeta,
) -> bool {
    match incoming_epoch.cmp(&current.epoch) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Equal => incoming_rank < current.source_rank,
        std::cmp::Ordering::Less => false,
    }
}

/// Merges normalized upstream records into the canonical catalog in one
/// transaction, and returns exactly what changed.
///
/// Transactional on purpose (never leave the catalog half-written): either
/// every record in this batch lands or none does, so a crash or a SQLite error
/// mid-merge leaves the previous known-good catalog intact rather than a
/// mixture of two refreshes.
///
/// Note what is absent: any `DELETE`. Rows not mentioned by `records` are
/// simply not touched, which is the whole anti-shrinkage guarantee.
pub fn merge(state: &AppState, records: &[SourceRecord]) -> Result<MergeOutcome> {
    let mut conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let now_iso = iso(Utc::now());
    let tx = conn.transaction()?;

    // Read the metadata for every known object up front. One scan of three
    // small columns is far cheaper than a correlated subquery per record, and
    // keeping it updated as we go (rather than re-reading) is what makes the
    // comparison correct when the same NORAD ID appears twice in one batch —
    // which it routinely does, since an object can be in two CelesTrak groups
    // and in SatNOGS as well.
    let mut existing: HashMap<u64, ExistingMeta> = {
        let mut stmt =
            tx.prepare("SELECT norad_id, epoch, source_rank FROM satellite_catalog")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (norad_id, epoch, source_rank) = row?;
            map.insert(
                norad_id,
                ExistingMeta {
                    // An unparseable stored epoch would otherwise make every
                    // incoming record look like a non-improvement forever.
                    // Treat it as maximally old so the next fetch repairs it.
                    epoch: parse_iso(&epoch).unwrap_or(DateTime::<Utc>::MIN_UTC),
                    source_rank,
                },
            );
        }
        map
    };
    let before = existing.len();

    // Tracked as sets rather than counters so the tallies stay exact when one
    // NORAD ID is merged more than once in a single batch.
    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut added: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut updated: std::collections::HashSet<u64> = std::collections::HashSet::new();

    {
        let mut insert = tx.prepare(
            "INSERT INTO satellite_catalog
               (norad_id, name, category, category_rank, elements_json,
                source, source_rank, epoch, first_seen, last_updated, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?9)",
        )?;
        // Replaces the orbital data *and* refreshes `last_updated`. The
        // category is upgraded only when this record's group ranks better, so
        // a SatNOGS record (rank CATEGORY_RANK_UNKNOWN) can supply fresher
        // elements for an object without demoting its CelesTrak category.
        let mut update_elements = tx.prepare(
            "UPDATE satellite_catalog SET
               name          = ?2,
               elements_json = ?3,
               source        = ?4,
               source_rank   = ?5,
               epoch         = ?6,
               last_updated  = ?7,
               last_seen     = ?7,
               category      = CASE WHEN ?8 < category_rank THEN ?9 ELSE category END,
               category_rank = MIN(category_rank, ?8)
             WHERE norad_id = ?1",
        )?;
        // The record was no improvement: record that a source still lists the
        // object (so it is provably alive, not merely un-deleted) and let a
        // better category through, but leave the orbital data and
        // `last_updated` alone.
        let mut touch_seen = tx.prepare(
            "UPDATE satellite_catalog SET
               last_seen     = ?2,
               category      = CASE WHEN ?3 < category_rank THEN ?4 ELSE category END,
               category_rank = MIN(category_rank, ?3)
             WHERE norad_id = ?1",
        )?;

        for rec in records {
            let epoch = rec.epoch();
            let rank = rec.source.rank();
            let elements_json = serde_json::to_string(&rec.elements)?;
            seen.insert(rec.norad_id);

            match existing.get(&rec.norad_id) {
                None => {
                    insert.execute(rusqlite::params![
                        rec.norad_id as i64,
                        rec.name,
                        rec.category,
                        rec.category_rank,
                        elements_json,
                        rec.source.as_str(),
                        rank,
                        iso(epoch),
                        now_iso,
                    ])?;
                    added.insert(rec.norad_id);
                    existing.insert(
                        rec.norad_id,
                        ExistingMeta {
                            epoch,
                            source_rank: rank,
                        },
                    );
                }
                Some(current) => {
                    if is_improvement(epoch, rank, current) {
                        update_elements.execute(rusqlite::params![
                            rec.norad_id as i64,
                            rec.name,
                            elements_json,
                            rec.source.as_str(),
                            rank,
                            iso(epoch),
                            now_iso,
                            rec.category_rank,
                            rec.category,
                        ])?;
                        // A row we inserted earlier in this same batch counts
                        // as `added`, not `updated` — hence the guard.
                        if !added.contains(&rec.norad_id) {
                            updated.insert(rec.norad_id);
                        }
                        existing.insert(
                            rec.norad_id,
                            ExistingMeta {
                                epoch,
                                source_rank: rank,
                            },
                        );
                    } else {
                        touch_seen.execute(rusqlite::params![
                            rec.norad_id as i64,
                            now_iso,
                            rec.category_rank,
                            rec.category,
                        ])?;
                    }
                }
            }
        }
    }

    let outcome = MergeOutcome {
        added: added.len(),
        updated: updated.len(),
        // Mentioned by a source, but nothing better than what we already had.
        unchanged: seen.len() - added.len() - updated.len(),
        // Everything we knew about that no source mentioned at all this cycle,
        // kept verbatim. These are precisely the rows the old per-group design
        // used to lose whenever a group came back short.
        retained: before - (seen.len() - added.len()),
        total: before + added.len(),
    };

    if outcome.added > 0 || outcome.updated > 0 {
        set_state_in(&tx, LAST_SUCCESSFUL_REFRESH, &now_iso)?;
    }
    tx.commit()?;
    Ok(outcome)
}

/// Reads the whole canonical catalog back, e.g. at process startup.
///
/// A row with unparseable stored JSON is skipped and counted rather than
/// failing the load: one corrupt blob must not cost us the other 16,000
/// satellites, which is the same reasoning applied to a single bad element set
/// during a fetch.
pub fn load_all(conn: &Connection) -> Result<(Vec<StoredSatellite>, usize)> {
    let mut stmt = conn.prepare(
        "SELECT norad_id, name, category, elements_json, source, epoch, last_updated
         FROM satellite_catalog",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)? as u64,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut out = Vec::with_capacity(rows.len());
    let mut skipped = 0usize;
    for (norad_id, name, category, elements_json, source, epoch, last_updated) in rows {
        let Ok(elements) = serde_json::from_str::<sgp4::Elements>(&elements_json) else {
            skipped += 1;
            continue;
        };
        out.push(StoredSatellite {
            norad_id,
            name,
            category,
            elements,
            source: Source::from_str(&source).unwrap_or(Source::Celestrak),
            epoch: parse_iso(&epoch).unwrap_or(DateTime::<Utc>::MIN_UTC),
            last_updated: parse_iso(&last_updated).unwrap_or(DateTime::<Utc>::MIN_UTC),
        });
    }
    Ok((out, skipped))
}

pub fn count(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM satellite_catalog", [], |r| r.get(0))?;
    Ok(n as usize)
}

// ── Refresh-state bookkeeping ───────────────────────────────────────────────
//
// Keys are constants rather than ad-hoc strings so a typo is a compile error
// instead of a silently-always-absent value.

pub const LAST_SUCCESSFUL_REFRESH: &str = "last_successful_refresh";
pub const LAST_CELESTRAK_SUCCESS: &str = "last_celestrak_success";
pub const LAST_SATNOGS_SUCCESS: &str = "last_satnogs_success";
pub const LAST_REFRESH_STATUS: &str = "last_refresh_status";
pub const BOOT_MARKER: &str = "boot_count";
/// Prefix for the per-group high-water member count that the shrinkage guard
/// compares a new response against (`group_high_water:starlink`).
pub const GROUP_HIGH_WATER_PREFIX: &str = "group_high_water:";

pub fn get_state(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM satellite_refresh_state WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

pub fn get_state_ts(conn: &Connection, key: &str) -> Option<DateTime<Utc>> {
    get_state(conn, key).as_deref().and_then(parse_iso)
}

fn set_state_in(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO satellite_refresh_state (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
        rusqlite::params![key, value, iso(Utc::now())],
    )?;
    Ok(())
}

pub fn set_state(state: &AppState, key: &str, value: &str) -> Result<()> {
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    set_state_in(&conn, key, value)
}

pub fn set_state_now(state: &AppState, key: &str) -> Result<()> {
    set_state(state, key, &iso(Utc::now()))
}

pub fn clear_state(state: &AppState, key: &str) -> Result<()> {
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    conn.execute(
        "DELETE FROM satellite_refresh_state WHERE key = ?1",
        rusqlite::params![key],
    )?;
    Ok(())
}

/// Reads every `group_high_water:*` entry as a map of group -> member count.
pub fn group_high_water(conn: &Connection) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT key, value FROM satellite_refresh_state WHERE key LIKE 'group_high_water:%'",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) else {
        return out;
    };
    for (key, value) in rows.flatten() {
        if let (Some(group), Ok(n)) = (
            key.strip_prefix(GROUP_HIGH_WATER_PREFIX),
            value.parse::<usize>(),
        ) {
            out.insert(group.to_string(), n);
        }
    }
    out
}

/// Increments and returns the boot counter. A returned `1` means this process
/// opened a database that had never been booted against before — either a
/// genuine first deployment, or (the case worth shouting about) a container
/// whose App Storage snapshot did not restore.
pub fn record_boot(state: &AppState) -> Result<u64> {
    let conn = state
        .lock()
        .map_err(|_| anyhow::anyhow!("db lock poisoned"))?;
    let previous = get_state(&conn, BOOT_MARKER)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let next = previous + 1;
    set_state_in(&conn, BOOT_MARKER, &next.to_string())?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_epoch_wins_regardless_of_source() {
        let older = ExistingMeta {
            epoch: "2026-09-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            source_rank: Source::CelestrakSupplemental.rank(),
        };
        let newer = "2026-09-02T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        // SatNOGS is the lowest-ranked source, but its epoch is newer, so the
        // orbital data is genuinely better and must win.
        assert!(is_improvement(newer, Source::Satnogs.rank(), &older));
    }

    #[test]
    fn older_epoch_never_overwrites_newer() {
        let current = ExistingMeta {
            epoch: "2026-09-05T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            source_rank: Source::Satnogs.rank(),
        };
        let stale = "2026-09-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        // Even the best-ranked source cannot roll us back to older elements.
        assert!(!is_improvement(
            stale,
            Source::CelestrakSupplemental.rank(),
            &current
        ));
    }

    #[test]
    fn equal_epoch_is_broken_by_source_rank() {
        let epoch = "2026-09-05T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let held_by_satnogs = ExistingMeta {
            epoch,
            source_rank: Source::Satnogs.rank(),
        };
        assert!(is_improvement(
            epoch,
            Source::Celestrak.rank(),
            &held_by_satnogs
        ));

        let held_by_celestrak = ExistingMeta {
            epoch,
            source_rank: Source::Celestrak.rank(),
        };
        // Same epoch, worse source: not an improvement, so no pointless write.
        assert!(!is_improvement(
            epoch,
            Source::Satnogs.rank(),
            &held_by_celestrak
        ));
        // Same epoch, same source: also not an improvement.
        assert!(!is_improvement(
            epoch,
            Source::Celestrak.rank(),
            &held_by_celestrak
        ));
    }
}

#[cfg(test)]
mod pipeline_tests {
    //! The failure modes this whole module exists to prevent, expressed as
    //! the scenarios that actually happen in production.
    //!
    //! Every test here runs against a real SQLite database (in memory, or a
    //! temp file where a restart has to be simulated) and goes through the
    //! real `merge`, because the invariant being checked is a property of the
    //! SQL, not of a mock.

    use super::*;
    use crate::satellites::CATEGORY_RANK_UNKNOWN;
    use std::sync::{Arc, Mutex};

    /// The catalog sizes these tests use. Chosen to mirror the real
    /// deployment (~16k objects, and a ~1.6k partial refresh) because the
    /// reported bug was specifically a 16k catalog collapsing to 1.6k — but
    /// nothing in the code under test knows or cares about these numbers.
    const HEALTHY: u64 = 16_000;
    const PARTIAL: u64 = 1_600;

    fn state_in_memory() -> AppState {
        let conn = Connection::open_in_memory().expect("in-memory db");
        crate::db::schema::create_tables(&conn).expect("schema");
        Arc::new(Mutex::new(conn))
    }

    fn state_at(path: &std::path::Path) -> AppState {
        let conn = Connection::open(path).expect("file db");
        crate::db::schema::create_tables(&conn).expect("schema");
        Arc::new(Mutex::new(conn))
    }

    /// One valid element set, parsed once; callers clone and retag it. Parsing
    /// 16,000 JSON fixtures individually would dominate the test runtime.
    fn base_elements() -> sgp4::Elements {
        serde_json::from_str(
            r#"{
                "OBJECT_NAME": "TEST",
                "OBJECT_ID": "2020-001A",
                "EPOCH": "2026-09-01T00:00:00.000000",
                "MEAN_MOTION": 15.49507896,
                "ECCENTRICITY": 0.0001413,
                "INCLINATION": 51.6461,
                "RA_OF_ASC_NODE": 221.2784,
                "ARG_OF_PERICENTER": 89.1723,
                "MEAN_ANOMALY": 280.4612,
                "EPHEMERIS_TYPE": 0,
                "CLASSIFICATION_TYPE": "U",
                "NORAD_CAT_ID": 1,
                "ELEMENT_SET_NO": 999,
                "REV_AT_EPOCH": 23600,
                "BSTAR": -3.1515e-5,
                "MEAN_MOTION_DOT": -2.218e-5,
                "MEAN_MOTION_DDOT": 0
            }"#,
        )
        .expect("fixture parses")
    }

    /// `count` records with NORAD IDs `first..first+count`, all carrying the
    /// same element-set epoch (offset by `epoch_days` from the fixture's).
    fn records(
        first: u64,
        count: u64,
        epoch_days: i64,
        category: &str,
        source: Source,
    ) -> Vec<SourceRecord> {
        let base = base_elements();
        (first..first + count)
            .map(|norad_id| {
                let mut elements = base.clone();
                elements.norad_id = norad_id;
                elements.datetime += chrono::Duration::days(epoch_days);
                elements.object_name = Some(format!("SAT-{norad_id}"));
                SourceRecord {
                    norad_id,
                    name: format!("SAT-{norad_id}"),
                    category: category.to_string(),
                    category_rank: if source == Source::Satnogs {
                        CATEGORY_RANK_UNKNOWN
                    } else {
                        0
                    },
                    source,
                    elements,
                }
            })
            .collect()
    }

    fn celestrak(first: u64, count: u64, epoch_days: i64) -> Vec<SourceRecord> {
        records(first, count, epoch_days, "starlink", Source::Celestrak)
    }

    fn total(state: &AppState) -> usize {
        let conn = state.lock().unwrap();
        count(&conn).unwrap()
    }

    /// Establishes a healthy 16,000-satellite catalog, as a deployment that
    /// has been running normally would have.
    fn seeded_healthy_catalog() -> AppState {
        let state = state_in_memory();
        let outcome = merge(&state, &celestrak(1, HEALTHY, 0)).unwrap();
        assert_eq!(outcome.added, HEALTHY as usize);
        assert_eq!(total(&state), HEALTHY as usize);
        state
    }

    #[test]
    fn normal_refresh_updates_everything_and_adds_the_new_objects() {
        let state = seeded_healthy_catalog();

        // A complete, healthy refresh a day later: the same 16,000 objects
        // with newer elements, plus 100 newly-catalogued ones.
        let outcome = merge(&state, &celestrak(1, HEALTHY + 100, 1)).unwrap();

        assert_eq!(outcome.updated, HEALTHY as usize, "all existing refreshed");
        assert_eq!(outcome.added, 100, "newly-catalogued objects added");
        assert_eq!(outcome.retained, 0, "nothing was left behind");
        assert_eq!(total(&state), (HEALTHY + 100) as usize);
    }

    #[test]
    fn partial_celestrak_refresh_does_not_shrink_the_catalog() {
        // The exact reported failure: a 16,000-satellite catalog, and a
        // refresh in which only one small group (1,600 objects) came back.
        let state = seeded_healthy_catalog();

        let outcome = merge(&state, &celestrak(1, PARTIAL, 1)).unwrap();

        assert_eq!(
            total(&state),
            HEALTHY as usize,
            "a 1,600-object refresh must NOT become a 1,600-object catalog"
        );
        assert_eq!(outcome.updated, PARTIAL as usize, "1,600 got fresh data");
        assert_eq!(
            outcome.retained,
            (HEALTHY - PARTIAL) as usize,
            "the other 14,400 keep their previous orbital data"
        );
        assert_eq!(outcome.added, 0);
    }

    #[test]
    fn celestrak_unavailable_leaves_a_healthy_catalog_refreshed_by_satnogs() {
        let state = seeded_healthy_catalog();

        // CelesTrak contributes nothing at all; SatNOGS covers 4,000 of the
        // objects we already know about, with newer elements.
        let satnogs = records(1, 4_000, 1, "other", Source::Satnogs);
        let outcome = merge(&state, &satnogs).unwrap();

        assert_eq!(total(&state), HEALTHY as usize, "catalog intact");
        assert_eq!(outcome.updated, 4_000, "SatNOGS refreshed what it could");
        assert_eq!(outcome.retained, (HEALTHY - 4_000) as usize);

        // And SatNOGS, which publishes no categories, must not have
        // overwritten the CelesTrak category on any of them.
        let conn = state.lock().unwrap();
        let (rows, _) = load_all(&conn).unwrap();
        assert!(
            rows.iter().all(|r| r.category == "starlink"),
            "a SatNOGS refresh must not downgrade a CelesTrak category"
        );
        // The elements, however, did come from SatNOGS for those 4,000.
        assert_eq!(
            rows.iter().filter(|r| r.source == Source::Satnogs).count(),
            4_000
        );
    }

    #[test]
    fn both_providers_unavailable_retains_the_whole_catalog() {
        let state = seeded_healthy_catalog();

        // Nothing was fetched. `refresh_cycle` does not call `merge` at all in
        // this case; this asserts the stronger property that even if it did,
        // an empty batch is a no-op rather than a truncation.
        let outcome = merge(&state, &[]).unwrap();

        assert_eq!(total(&state), HEALTHY as usize);
        assert_eq!(outcome.retained, HEALTHY as usize);
        assert_eq!(outcome.added, 0);
        assert_eq!(outcome.updated, 0);
    }

    #[test]
    fn catalog_survives_a_restart_with_no_network() {
        // Run 1: fetch 16,000 and persist them to a real file.
        let dir = std::env::temp_dir().join(format!("sat-restart-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("catalog.db");
        let _ = std::fs::remove_file(&path);

        {
            let state = state_at(&path);
            merge(&state, &celestrak(1, HEALTHY, 0)).unwrap();
            assert_eq!(total(&state), HEALTHY as usize);
        } // connection dropped — the process is gone.

        // Run 2: a new process opens the same database and fetches nothing,
        // because every provider is unreachable.
        {
            let state = state_at(&path);
            let conn = state.lock().unwrap();
            let (rows, skipped) = load_all(&conn).unwrap();
            assert_eq!(skipped, 0);
            assert_eq!(
                rows.len(),
                HEALTHY as usize,
                "a redeploy during an outage must still serve the last-known-good catalog"
            );
            // And the refresh bookkeeping survived too, which is what stops
            // the new process from immediately re-downloading everything.
            assert!(
                get_state_ts(&conn, LAST_SUCCESSFUL_REFRESH).is_some(),
                "last_successful_refresh must survive a restart"
            );
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_suspiciously_small_provider_response_cannot_wipe_the_catalog() {
        let state = seeded_healthy_catalog();

        // A provider returns 12 objects. Whatever else happens, the other
        // 15,988 must still be there.
        merge(&state, &celestrak(1, 12, 2)).unwrap();
        assert_eq!(total(&state), HEALTHY as usize);

        // Even repeatedly — there is no accumulating erosion.
        for _ in 0..5 {
            merge(&state, &celestrak(1, 12, 3)).unwrap();
        }
        assert_eq!(total(&state), HEALTHY as usize);
    }

    #[test]
    fn the_same_norad_id_from_both_providers_is_one_satellite() {
        let state = state_in_memory();

        // Deliberately overlapping: CelesTrak has 1..=1000, SatNOGS has
        // 500..=1500, and SatNOGS' elements are a day newer.
        let mut batch = celestrak(1, 1_000, 0);
        batch.extend(records(500, 1_001, 1, "other", Source::Satnogs));

        let outcome = merge(&state, &batch).unwrap();

        assert_eq!(
            total(&state),
            1_500,
            "1,000 + 1,001 records with 501 overlapping is 1,500 satellites"
        );
        assert_eq!(outcome.added, 1_500);

        let conn = state.lock().unwrap();
        let (rows, _) = load_all(&conn).unwrap();
        let mut ids: Vec<u64> = rows.iter().map(|r| r.norad_id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 1_500, "no NORAD ID appears twice");

        // On the overlap, SatNOGS' newer epoch won the elements (freshness is
        // the primary rule) while the CelesTrak category was kept.
        let overlap: Vec<_> = rows
            .iter()
            .filter(|r| (500..=1_000).contains(&r.norad_id))
            .collect();
        assert_eq!(overlap.len(), 501);
        assert!(overlap.iter().all(|r| r.source == Source::Satnogs));
        assert!(overlap.iter().all(|r| r.category == "starlink"));
    }

    #[test]
    fn a_stale_provider_response_cannot_roll_back_fresher_elements() {
        let state = state_in_memory();
        merge(&state, &celestrak(1, 100, 5)).unwrap();

        // A provider serving a week-old cache must not replace newer data.
        let outcome = merge(&state, &celestrak(1, 100, 0)).unwrap();
        assert_eq!(outcome.updated, 0, "no row accepted older elements");
        assert_eq!(outcome.unchanged, 100);

        let conn = state.lock().unwrap();
        let (rows, _) = load_all(&conn).unwrap();
        let expected = base_elements().datetime.and_utc() + chrono::Duration::days(5);
        assert!(rows.iter().all(|r| r.epoch == expected));
    }

    #[test]
    fn a_merge_that_fails_midway_leaves_the_previous_catalog_untouched() {
        let state = seeded_healthy_catalog();

        // Force a genuine mid-transaction failure: a trigger that aborts on
        // one specific NORAD ID. The batch below inserts 50 good new records
        // before reaching it, so if the merge were not transactional those 50
        // would survive the error.
        {
            let conn = state.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER abort_on_sentinel BEFORE INSERT ON satellite_catalog
                 WHEN NEW.norad_id = 999999
                 BEGIN SELECT RAISE(ABORT, 'simulated mid-merge failure'); END;",
            )
            .unwrap();
        }

        let mut batch = celestrak(20_000, 50, 1);
        batch.extend(celestrak(999_999, 1, 1));

        let result = merge(&state, &batch);
        assert!(result.is_err(), "the merge must surface the failure");

        // Nothing from the failed batch landed, and the pre-existing catalog
        // is exactly as it was.
        assert_eq!(
            total(&state),
            HEALTHY as usize,
            "a failed merge must roll back completely, not partially apply"
        );
        let conn = state.lock().unwrap();
        let (rows, _) = load_all(&conn).unwrap();
        assert!(
            !rows.iter().any(|r| r.norad_id >= 20_000),
            "no record from the rolled-back batch may remain"
        );
    }

    #[test]
    fn last_successful_refresh_only_moves_when_something_actually_merged() {
        let state = state_in_memory();
        {
            let conn = state.lock().unwrap();
            assert!(get_state_ts(&conn, LAST_SUCCESSFUL_REFRESH).is_none());
        }

        merge(&state, &celestrak(1, 10, 0)).unwrap();
        let first = {
            let conn = state.lock().unwrap();
            get_state_ts(&conn, LAST_SUCCESSFUL_REFRESH).expect("set after a real merge")
        };

        // A cycle in which every record was already current changed nothing,
        // so it must not advertise itself as a successful refresh.
        let outcome = merge(&state, &celestrak(1, 10, 0)).unwrap();
        assert_eq!(outcome.added, 0);
        assert_eq!(outcome.updated, 0);
        let conn = state.lock().unwrap();
        assert_eq!(
            get_state_ts(&conn, LAST_SUCCESSFUL_REFRESH),
            Some(first),
            "an all-unchanged cycle must not move last_successful_refresh"
        );
    }
}
