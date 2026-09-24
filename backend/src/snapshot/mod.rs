//! Durable SQLite on a platform with no persistent disk.
//!
//! Replit rebuilds a published app's filesystem from the workspace file tree
//! on every publish, so `DATABASE_PATH` is gone after each deploy. That would
//! be survivable if the database were purely a cache, and it is not: three
//! fetchers pull *bounded rolling windows* and accumulate history locally that
//! upstream will never return again —
//!
//!   * `outage_events`          — IODA, 90-day window   (fetchers/ioda.rs)
//!   * `bgp_prefix_visibility`  — RIPEstat, 14-day window (fetchers/ripestat.rs)
//!   * `http_protocol_share`    — Cloudflare, 14-day window (fetchers/cloudflare_http.rs)
//!
//! — on top of `satellite_catalog`, which the README already documents as
//! non-rebuildable. Losing the file is data loss, not a cold cache.
//!
//! So the file stays exactly where it is and SQLite stays exactly as it is.
//! This module adds a layer underneath: restore the database from Replit App
//! Storage at boot, and write it back periodically and on shutdown. Nothing in
//! `db/` or `fetchers/` knows this exists.
//!
//! # The invariant
//!
//! > A failed restore must never be allowed to overwrite a good snapshot.
//!
//! The dangerous sequence is: transient network failure at boot -> empty
//! database -> fetchers repopulate a *fraction* of it -> snapshot uploaded
//! over the only good copy. It is prevented structurally rather than by
//! checking: a restore that *errors* returns no `Snapshotter` at all, so no
//! upload task is ever spawned and there is no code path that could write. A
//! clean 404 (nothing stored yet) is deliberately a different outcome from a
//! failed transfer, which is why `ObjectStore::get` distinguishes them.
//!
//! A second, softer guard sits on top for the running process: a snapshot
//! whose uncompressed size has collapsed relative to the last one is refused.
//! That is the same reasoning — and the same knob shape — as the shrinkage
//! guard in `fetchers::satellites`.

pub mod object_store;

use crate::AppState;
use anyhow::{Context, Result, anyhow, bail};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use object_store::ObjectStore;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Default gap between snapshots. Three hours rather than something tighter
/// because the planned case — a redeploy — is covered losslessly by the
/// shutdown hook, and this interval only bounds what an *unplanned* crash
/// costs. Everything written in between is either re-fetchable within a cycle
/// or inside a rolling window the next sweep re-pulls anyway. It also keeps
/// App Storage bandwidth to a few hundred MB a month rather than tens of GB.
const DEFAULT_INTERVAL_MINUTES: f64 = 180.0;

/// Hard floor regardless of configuration, for the same reason
/// `db::MIN_FETCH_INTERVAL_SECS` exists: a typo should make this slow, never
/// a tight loop uploading a 70 MB object.
const MIN_INTERVAL_SECS: u64 = 300;

/// A new snapshot smaller than this fraction of the last successful one is
/// treated as evidence that something has gone wrong locally, not as a
/// genuinely smaller database.
const DEFAULT_MIN_RETAIN_RATIO: f64 = 0.5;

/// Every SQLite database file starts with this, including one produced by
/// `VACUUM INTO`. Checked after decompression so a truncated or wrong object
/// is caught before it is put in place, rather than surfacing as "file is not
/// a database" from `Connection::open`.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Holds everything the two background tasks need. Cloneable so the interval
/// loop and the shutdown hook can each own one while sharing the guard state.
#[derive(Clone)]
pub struct Snapshotter {
    store: ObjectStore,
    key: String,
    db_path: String,
    min_retain_ratio: f64,
    /// Uncompressed size of the last snapshot known to be good — seeded from
    /// a successful restore, then updated after every upload. Shared across
    /// clones so the guard sees one history, not one per task.
    last_good_bytes: Arc<AtomicU64>,
    /// Serialises snapshot production. Without it the interval loop and the
    /// shutdown hook could both `VACUUM INTO` the same temporary path.
    gate: Arc<tokio::sync::Mutex<()>>,
}

/// Restores the database from App Storage if it is not already on disk, and
/// returns the handle the background tasks need — or `None`, meaning no
/// snapshots will be taken for the lifetime of this process.
///
/// `None` covers three very different situations, and the logging
/// distinguishes them because only the third is a problem:
///
///   1. `SNAPSHOT_KEY` unset — local development. Silent by design.
///   2. `SNAPSHOT_KEY` set but no sidecar — misconfiguration. Warned.
///   3. The restore failed — warned loudly, and uploads stay off (see the
///      module invariant).
///
/// Must be called *before* `Connection::open`, which creates the file if it
/// is absent and would therefore make every boot look like a first boot.
pub async fn restore_if_absent(db_path: &str) -> Option<Snapshotter> {
    let key = std::env::var("SNAPSHOT_KEY")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())?;

    let store = match ObjectStore::discover().await {
        Ok(Some(store)) => store,
        Ok(None) => {
            eprintln!(
                "WARNING: SNAPSHOT_KEY is set but no Replit App Storage sidecar is listening \
                 on 127.0.0.1:1106 — the database will NOT be persisted across deploys."
            );
            return None;
        }
        Err(e) => {
            eprintln!("WARNING: could not reach Replit App Storage: {e:#}");
            eprintln!("WARNING: the database will NOT be persisted across deploys.");
            return None;
        }
    };

    restore_with(store, key, db_path).await
}

/// The decision table, with the store already resolved. Split out from
/// `restore_if_absent` so the tests can drive every branch — in particular
/// the one that must return `None` — against a stub instead of the real
/// sidecar.
async fn restore_with(store: ObjectStore, key: String, db_path: &str) -> Option<Snapshotter> {
    let snapshotter = Snapshotter {
        store,
        key,
        db_path: db_path.to_string(),
        min_retain_ratio: min_retain_ratio_from_env(),
        last_good_bytes: Arc::new(AtomicU64::new(0)),
        gate: Arc::new(tokio::sync::Mutex::new(())),
    };

    // A file already here means this is a restart that kept its filesystem,
    // not a fresh deploy. The local copy is by definition at least as current
    // as the stored one, so it wins and nothing is downloaded.
    if Path::new(db_path).exists() {
        println!("snapshot: {db_path} is already present — keeping it, not restoring.");
        return Some(snapshotter);
    }

    match snapshotter.store.get(&snapshotter.key).await {
        Ok(Some(compressed)) => match install_restored(&compressed, db_path) {
            Ok(bytes) => {
                snapshotter.last_good_bytes.store(bytes, Ordering::Relaxed);
                println!(
                    "snapshot: restored {db_path} from {}/{} ({} compressed -> {}).",
                    snapshotter.store.bucket(),
                    snapshotter.key,
                    human_bytes(compressed.len() as u64),
                    human_bytes(bytes)
                );
                Some(snapshotter)
            }
            Err(e) => {
                eprintln!("WARNING: the stored snapshot could not be restored: {e:#}");
                eprintln!(
                    "WARNING: refusing to take any snapshot this run, so the stored copy is \
                     left intact. Fix the cause and redeploy; nothing has been overwritten."
                );
                None
            }
        },
        Ok(None) => {
            println!(
                "snapshot: no object `{}` in bucket {} yet — treating this as a first deployment.",
                snapshotter.key,
                snapshotter.store.bucket()
            );
            Some(snapshotter)
        }
        Err(e) => {
            eprintln!("WARNING: could not download the stored snapshot: {e:#}");
            eprintln!(
                "WARNING: refusing to take any snapshot this run, so the stored copy is left \
                 intact. This boot starts from an empty database; the next successful boot \
                 will restore the previous one."
            );
            None
        }
    }
}

impl Snapshotter {
    /// Re-snapshots forever, `SNAPSHOT_INTERVAL_MINUTES` apart. Same
    /// spawn/loop/sleep shape as the fetcher loops in `db/mod.rs`, and
    /// deliberately independent of them: this is about durability, not about
    /// any one data source's cadence.
    ///
    /// Unlike those loops it sleeps *first*. There is nothing new to save at
    /// boot — the database was either just restored or is brand new.
    pub async fn run_loop(self, state: AppState) {
        let gap = interval_from_env();
        println!(
            "Snapshot loop: every {}m -> {}/{}",
            gap.as_secs() / 60,
            self.store.bucket(),
            self.key
        );

        let mut last_fingerprint = write_fingerprint(&self.db_path);
        loop {
            tokio::time::sleep(gap).await;

            // Unchanged means the database is byte-for-byte what the last
            // snapshot already holds, so uploading again would cost bandwidth
            // to store an identical object.
            let fingerprint = write_fingerprint(&self.db_path);
            if fingerprint == last_fingerprint {
                println!("snapshot: no writes since the last snapshot — skipping.");
                continue;
            }

            match self.write(&state).await {
                Ok(Some(bytes)) => {
                    last_fingerprint = fingerprint;
                    println!("snapshot: stored {}.", human_bytes(bytes));
                }
                // Refused by the shrinkage guard: deliberately does not
                // advance the fingerprint, so the next cycle tries again.
                Ok(None) => {}
                Err(e) => eprintln!("WARNING: snapshot failed: {e:#}"),
            }
        }
    }

    /// Takes one final snapshot on SIGTERM/SIGINT, then exits.
    ///
    /// This is what makes a *planned* redeploy lossless, and it is the reason
    /// the interval above can be as long as it is. If the platform's grace
    /// period runs out mid-upload the process is killed, but GCS object
    /// writes are atomic — the previous snapshot survives whole.
    pub async fn run_shutdown_hook(self, state: AppState) {
        use tokio::signal::unix::{SignalKind, signal};

        let (mut term, mut interrupt) = match (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) {
            (Ok(t), Ok(i)) => (t, i),
            _ => {
                eprintln!(
                    "WARNING: could not install signal handlers — a redeploy will lose \
                     everything written since the last interval snapshot."
                );
                return;
            }
        };

        tokio::select! {
            _ = term.recv() => {}
            _ = interrupt.recv() => {}
        }

        println!("snapshot: shutdown signal received — taking a final snapshot.");
        match self.write(&state).await {
            Ok(Some(bytes)) => println!("snapshot: final snapshot stored ({}).", human_bytes(bytes)),
            Ok(None) => {}
            Err(e) => eprintln!("WARNING: the final snapshot failed: {e:#}"),
        }
        std::process::exit(0);
    }

    /// Produces and uploads one snapshot. `Ok(None)` means the shrinkage
    /// guard refused it — not an error, but nothing was stored.
    async fn write(&self, state: &AppState) -> Result<Option<u64>> {
        let _held = self.gate.lock().await;

        let db_path = self.db_path.clone();
        let state = state.clone();
        // `VACUUM INTO` plus gzip over tens of MB is blocking and
        // CPU-bound; running it on a worker keeps it off the runtime's
        // request threads.
        let (raw_bytes, compressed) =
            tokio::task::spawn_blocking(move || produce_snapshot(&state, &db_path))
                .await
                .map_err(|e| anyhow!("the snapshot task panicked: {e}"))??;

        let last_good = self.last_good_bytes.load(Ordering::Relaxed);
        if last_good > 0 {
            let ratio = raw_bytes as f64 / last_good as f64;
            if ratio < self.min_retain_ratio {
                eprintln!(
                    "WARNING: refusing to store a snapshot of {} — that is {:.0}% of the last \
                     good one ({}), below SNAPSHOT_MIN_RETAIN_RATIO ({:.2}).",
                    human_bytes(raw_bytes),
                    ratio * 100.0,
                    human_bytes(last_good),
                    self.min_retain_ratio
                );
                eprintln!(
                    "WARNING: the stored snapshot is left intact. If the database really did \
                     shrink, lower SNAPSHOT_MIN_RETAIN_RATIO."
                );
                return Ok(None);
            }
        }

        self.store.put(&self.key, compressed).await?;
        self.last_good_bytes.store(raw_bytes, Ordering::Relaxed);
        Ok(Some(raw_bytes))
    }
}

/// `VACUUM INTO` a temporary file next to the database, gzip it, and return
/// `(uncompressed size, compressed bytes)`.
///
/// `VACUUM INTO` rather than copying the file: it takes a read transaction,
/// so the result is a consistent image including anything still in the WAL,
/// and it is defragmented on the way out. It does hold the `AppState` mutex
/// for its duration (a second or two at 70 MB), which briefly blocks API
/// requests. If that pause ever becomes noticeable, rusqlite's incremental
/// `backup` feature replaces this function and nothing else.
fn produce_snapshot(state: &AppState, db_path: &str) -> Result<(u64, Vec<u8>)> {
    let temp = temp_path(db_path, "snapshot");

    // VACUUM INTO refuses to write to a path that already exists, so a
    // previous crashed attempt has to be cleared first.
    if temp.exists() {
        fs::remove_file(&temp)
            .with_context(|| format!("could not remove the stale {}", temp.display()))?;
    }

    {
        let conn = state
            .lock()
            .map_err(|_| anyhow!("db lock poisoned while snapshotting"))?;
        let dest = temp
            .to_str()
            .context("the snapshot path is not valid UTF-8")?;
        conn.execute("VACUUM INTO ?1", rusqlite::params![dest])
            .context("VACUUM INTO failed")?;
    }

    let raw_bytes = fs::metadata(&temp)
        .with_context(|| format!("could not stat {}", temp.display()))?
        .len();

    let mut file = fs::File::open(&temp)
        .with_context(|| format!("could not open {} for compression", temp.display()))?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    std::io::copy(&mut file, &mut encoder).context("could not compress the snapshot")?;
    let compressed = encoder.finish().context("could not finish the gzip stream")?;

    // Best-effort: leaving it behind costs disk but breaks nothing, and the
    // next run clears it anyway.
    let _ = fs::remove_file(&temp);

    Ok((raw_bytes, compressed))
}

/// Decompresses a downloaded snapshot into place and returns its uncompressed
/// size. Writes to a temporary path and renames, so a failure part-way
/// through can never leave a half-written file where the database belongs.
fn install_restored(compressed: &[u8], db_path: &str) -> Result<u64> {
    let temp = temp_path(db_path, "restore");
    if let Some(parent) = temp.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }

    let mut decoder = GzDecoder::new(compressed);
    let mut header = [0u8; SQLITE_MAGIC.len()];
    decoder
        .read_exact(&mut header)
        .context("the stored snapshot is shorter than a SQLite header")?;
    if &header != SQLITE_MAGIC {
        bail!("the stored snapshot is not a SQLite database");
    }

    let mut out = fs::File::create(&temp)
        .with_context(|| format!("could not create {}", temp.display()))?;
    out.write_all(&header)?;
    let rest = std::io::copy(&mut decoder, &mut out).context("could not decompress the snapshot")?;
    out.sync_all().context("could not flush the restored file")?;
    drop(out);

    fs::rename(&temp, db_path)
        .with_context(|| format!("could not move the restored database into {db_path}"))?;

    Ok(rest + header.len() as u64)
}

/// `<db_path>.<suffix>` — always beside the database, which is the one
/// directory guaranteed to be writable and to have room for a copy.
fn temp_path(db_path: &str, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{db_path}.{suffix}"))
}

/// A cheap "has anything been written" signal: the length and mtime of the
/// database and its write-ahead log.
///
/// Deliberately taken from the filesystem rather than from SQLite. rusqlite
/// 0.31 exposes `changes()` (rows touched by the last statement) but not a
/// cumulative counter, and `PRAGMA data_version` only moves for writes made
/// by *other* connections — useless here, where there is exactly one. In WAL
/// mode every committed write extends or rewrites `-wal`, so this moves
/// whenever the database does, needs no lock, and costs two `stat` calls.
///
/// Missing files read as zeroes, which is correct: a database that does not
/// exist has not been written to.
fn write_fingerprint(db_path: &str) -> (u64, u64, u64, u64) {
    fn stat(path: PathBuf) -> (u64, u64) {
        let Ok(meta) = fs::metadata(path) else {
            return (0, 0);
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        (meta.len(), modified)
    }

    let (main_len, main_mtime) = stat(PathBuf::from(db_path));
    let (wal_len, wal_mtime) = stat(PathBuf::from(format!("{db_path}-wal")));
    (main_len, main_mtime, wal_len, wal_mtime)
}

fn interval_from_env() -> Duration {
    let minutes = std::env::var("SNAPSHOT_INTERVAL_MINUTES")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|m| m.is_finite() && *m > 0.0)
        .unwrap_or(DEFAULT_INTERVAL_MINUTES);
    Duration::from_secs(((minutes * 60.0) as u64).max(MIN_INTERVAL_SECS))
}

fn min_retain_ratio_from_env() -> f64 {
    std::env::var("SNAPSHOT_MIN_RETAIN_RATIO")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|r| r.is_finite() && *r >= 0.0 && *r <= 1.0)
        .unwrap_or(DEFAULT_MIN_RETAIN_RATIO)
}

fn human_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes >= MIB as u64 {
        format!("{:.1} MiB", bytes as f64 / MIB)
    } else {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    }
}

/// A stand-in for the Replit sidecar and for GCS, served on loopback.
///
/// Worth the ~80 lines: the two claims this module makes that actually matter
/// — "a round trip reproduces the database" and "a failed download never
/// leads to an upload" — are both properties of the *HTTP* paths, so mocking
/// `ObjectStore` would test nothing. This drives the real client, the real
/// token exchange and the real status-code handling.
#[cfg(test)]
mod stub {
    use axum::{
        Json, Router,
        extract::{Path as AxumPath, State},
        http::StatusCode,
        routing::{get, post},
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    pub type Objects = Arc<Mutex<HashMap<String, Vec<u8>>>>;

    #[derive(Clone)]
    pub struct Stub {
        pub objects: Objects,
        /// When true, `/credential` returns 500 — the transient failure the
        /// restore path has to treat as "do not touch the stored copy".
        pub break_credentials: Arc<Mutex<bool>>,
    }

    /// Returns (base url, stub). The server runs for the rest of the test.
    pub async fn serve() -> (String, Stub) {
        let stub = Stub {
            objects: Arc::new(Mutex::new(HashMap::new())),
            break_credentials: Arc::new(Mutex::new(false)),
        };

        let app = Router::new()
            .route(
                "/object-storage/default-bucket",
                get(|| async { Json(serde_json::json!({ "bucketId": "test-bucket" })) }),
            )
            .route(
                "/credential",
                get(|State(s): State<Stub>| async move {
                    if *s.break_credentials.lock().unwrap() {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({})));
                    }
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({ "access_token": "subject-token" })),
                    )
                }),
            )
            .route(
                "/token",
                post(|| async { Json(serde_json::json!({ "access_token": "gcs-token" })) }),
            )
            .route(
                "/storage/v1/b/:bucket/o/:object",
                get(
                    |State(s): State<Stub>, AxumPath((_b, object)): AxumPath<(String, String)>| async move {
                        match s.objects.lock().unwrap().get(&object) {
                            Some(bytes) => (StatusCode::OK, bytes.clone()),
                            None => (StatusCode::NOT_FOUND, Vec::new()),
                        }
                    },
                ),
            )
            .route(
                "/upload/storage/v1/b/:bucket/o",
                post(
                    |State(s): State<Stub>,
                     axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
                     body: axum::body::Bytes| async move {
                        let name = q.get("name").cloned().unwrap_or_default();
                        s.objects.lock().unwrap().insert(name, body.to_vec());
                        StatusCode::OK
                    },
                ),
            )
            .with_state(stub.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        (format!("http://{addr}"), stub)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_trip_reproduces_the_database_byte_for_byte() {
        let dir = std::env::temp_dir().join(format!("blackout-snapshot-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("round_trip.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT NOT NULL);
             INSERT INTO t (v) VALUES ('alpha'), ('beta'), ('gamma');",
        )
        .unwrap();
        let state: AppState = Arc::new(std::sync::Mutex::new(conn));

        let (raw_bytes, compressed) = produce_snapshot(&state, &db_path_str).unwrap();
        assert!(raw_bytes > 0);

        // Drop the live database entirely, then restore over the empty slot.
        drop(state);
        fs::remove_file(&db_path).unwrap();
        let restored_bytes = install_restored(&compressed, &db_path_str).unwrap();
        assert_eq!(restored_bytes, raw_bytes);

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let rows: Vec<String> = conn
            .prepare("SELECT v FROM t ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows, vec!["alpha", "beta", "gamma"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_sqlite_payload_is_refused_before_it_is_installed() {
        let dir = std::env::temp_dir().join(format!("blackout-snapshot-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("never_written.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"this is not a database, not even close").unwrap();
        let compressed = encoder.finish().unwrap();

        assert!(install_restored(&compressed, &db_path_str).is_err());
        assert!(!db_path.exists(), "a rejected payload must not be installed");

        let _ = fs::remove_dir_all(&dir);
    }


    /// Builds a small populated database at `path`.
    fn seeded_db(path: &Path) -> AppState {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE outage_events (id INTEGER PRIMARY KEY, country TEXT NOT NULL);
             INSERT INTO outage_events (country)
             VALUES ('IR'), ('SY'), ('SD'), ('MM'), ('IQ');",
        )
        .unwrap();
        Arc::new(std::sync::Mutex::new(conn))
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "blackout-snap-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn a_snapshot_survives_the_filesystem_being_wiped() {
        let (base, _stub) = stub::serve().await;
        let dir = scratch("survives");
        let db_path = dir.join("mena_ai.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        // First "deployment": populate, then snapshot.
        let state = seeded_db(&db_path);
        let store = ObjectStore::with_endpoints("test-bucket", &base, &base);
        let first = restore_with(store, "mena_ai.db.gz".into(), &db_path_str)
            .await
            .expect("a clean 404 must still allow snapshots");
        let stored = first.write(&state).await.unwrap();
        assert!(stored.is_some(), "the first snapshot must be stored");
        drop(state);

        // Publish: the filesystem is rebuilt, so the database is gone.
        fs::remove_file(&db_path).unwrap();
        assert!(!db_path.exists());

        // Second "deployment": restore.
        let store = ObjectStore::with_endpoints("test-bucket", &base, &base);
        let second = restore_with(store, "mena_ai.db.gz".into(), &db_path_str).await;
        assert!(second.is_some());
        assert!(db_path.exists(), "the database must be back on disk");

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM outage_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 5, "every row must survive the round trip");

        let _ = fs::remove_dir_all(&dir);
    }

    /// The module's central invariant. A transient failure at boot must leave
    /// the stored copy alone — structurally, by never producing a
    /// `Snapshotter` at all.
    #[tokio::test]
    async fn a_failed_restore_can_never_overwrite_the_stored_snapshot() {
        let (base, stub) = stub::serve().await;
        let dir = scratch("failed-restore");
        let db_path = dir.join("mena_ai.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        // Store a good snapshot first.
        let state = seeded_db(&db_path);
        let store = ObjectStore::with_endpoints("test-bucket", &base, &base);
        let good = restore_with(store, "mena_ai.db.gz".into(), &db_path_str)
            .await
            .unwrap();
        good.write(&state).await.unwrap().unwrap();
        let stored_before = stub
            .objects
            .lock()
            .unwrap()
            .get("mena_ai.db.gz")
            .cloned()
            .unwrap();
        drop(state);
        fs::remove_file(&db_path).unwrap();

        // Next boot: the sidecar is broken, so the download fails.
        *stub.break_credentials.lock().unwrap() = true;
        let store = ObjectStore::with_endpoints("test-bucket", &base, &base);
        let result = restore_with(store, "mena_ai.db.gz".into(), &db_path_str).await;
        assert!(
            result.is_none(),
            "a failed restore must yield no snapshotter, so no upload task can exist"
        );

        let stored_after = stub
            .objects
            .lock()
            .unwrap()
            .get("mena_ai.db.gz")
            .cloned()
            .unwrap();
        assert_eq!(
            stored_before, stored_after,
            "the stored snapshot must be byte-for-byte untouched"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_existing_local_database_wins_over_the_stored_one() {
        let (base, stub) = stub::serve().await;
        let dir = scratch("local-wins");
        let db_path = dir.join("mena_ai.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        stub.objects
            .lock()
            .unwrap()
            .insert("mena_ai.db.gz".into(), b"would be nonsense if used".to_vec());

        // A plain restart: the filesystem kept the database.
        let state = seeded_db(&db_path);
        let store = ObjectStore::with_endpoints("test-bucket", &base, &base);
        assert!(
            restore_with(store, "mena_ai.db.gz".into(), &db_path_str)
                .await
                .is_some()
        );

        // Untouched — the bogus stored object was never downloaded.
        let rows: i64 = state
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM outage_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 5);

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_shrinkage_guard_refuses_a_collapsed_snapshot() {
        let (base, stub) = stub::serve().await;
        let dir = scratch("shrinkage");
        let db_path = dir.join("mena_ai.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, blob TEXT NOT NULL);")
            .unwrap();
        for i in 0..4000 {
            conn.execute("INSERT INTO t (blob) VALUES (?1)", [format!("{i:0>200}")])
                .unwrap();
        }
        let state: AppState = Arc::new(std::sync::Mutex::new(conn));

        let store = ObjectStore::with_endpoints("test-bucket", &base, &base);
        let snap = restore_with(store, "mena_ai.db.gz".into(), &db_path_str)
            .await
            .unwrap();
        let full = snap.write(&state).await.unwrap().expect("first snapshot");
        let good_object = stub.objects.lock().unwrap().get("mena_ai.db.gz").cloned();

        // Something local goes wrong and most of the database disappears.
        state
            .lock()
            .unwrap()
            .execute("DELETE FROM t WHERE id > 200", [])
            .unwrap();

        let refused = snap.write(&state).await.unwrap();
        assert!(
            refused.is_none(),
            "a snapshot at a fraction of {full} bytes must be refused"
        );
        assert_eq!(
            stub.objects.lock().unwrap().get("mena_ai.db.gz").cloned(),
            good_object,
            "the stored snapshot must be left intact"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_interval_floor_holds_against_a_too_small_value() {
        // Guards the same class of mistake as db::MIN_FETCH_INTERVAL_SECS.
        assert!(MIN_INTERVAL_SECS >= 60);
        assert!(DEFAULT_INTERVAL_MINUTES * 60.0 > MIN_INTERVAL_SECS as f64);
    }
}
