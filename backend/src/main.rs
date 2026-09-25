use axum::{Extension, Router, routing::get};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};

mod api;
mod db;
// Retained but unrouted. `POST /api/evaluate` is not mounted on this public
// read-only deployment, which leaves the whole assessment engine unreachable
// and therefore "dead" to the compiler. It is kept compiling — and its
// snapshot test kept running — so the feature can be restored behind a guard
// without resurrecting deleted code.
#[allow(dead_code)]
mod engine;
mod fetchers;
mod models;
mod satellites;
mod snapshot;
mod util;

pub type AppState = Arc<Mutex<Connection>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load backend/.env (cwd is `backend/`) into the process environment so
    // CLOUDFLARE_API_TOKEN and friends reach std::env::var below. A plain Rust
    // binary — unlike Vite — does not read .env on its own. `.ok()` keeps
    // startup working when no .env is present.
    dotenvy::dotenv().ok();

    // Defaults to the historical relative path so `cargo run` from `backend/`
    // keeps working untouched; in production this points at a mounted volume.
    let db_path = std::env::var("DATABASE_PATH")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "mena_ai.db".to_string());

    // The directory holding the database is not guaranteed to exist:
    // DATABASE_PATH points somewhere on a filesystem Replit rebuilds from
    // scratch on every publish. `Connection::open` does not create parent
    // directories, so without this it fails with "unable to open database
    // file" before the snapshot restore has anywhere to put anything.
    if let Some(parent) = std::path::Path::new(&db_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            anyhow::anyhow!("could not create `{}` for the database: {e}", parent.display())
        })?;
    }

    // Pulls the database back from Replit App Storage when the filesystem has
    // been rebuilt under us. Must run before the two lines below: `exists()`
    // has to see the restored file, and `Connection::open` would create an
    // empty one. `None` means no snapshots this run — see the module docs for
    // the three reasons, only one of which is a problem.
    let snapshotter = snapshot::restore_if_absent(&db_path).await;

    // Captured *before* `Connection::open`, which creates the file if absent —
    // afterwards there is no way to tell a restored database from a brand-new
    // one, and that distinction is the whole point of `report_persistence`.
    let db_existed = std::path::Path::new(&db_path).exists();

    let conn = Connection::open(&db_path)
        .map_err(|e| anyhow::anyhow!("could not open database at `{db_path}`: {e}"))?;
    println!("DB at {db_path}");
    db::init_schema(&conn)?;
    println!("DB initialized and seeded.");
    warn_missing_optional_tokens();
    util::http::report_identity();

    let state: AppState = Arc::new(Mutex::new(conn));
    report_persistence(&state, &db_path, db_existed);

    // The built SPA. Default is the repo layout relative to `backend/`, so a
    // local `npm run build` is served without configuration; in a container
    // this points wherever dist was copied.
    let static_dir = std::env::var("STATIC_DIR")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "../frontend/dist".to_string());
    if std::path::Path::new(&static_dir)
        .join("index.html")
        .exists()
    {
        println!("Serving SPA from {static_dir}");
    } else {
        eprintln!(
            "WARNING: no index.html under `{static_dir}` — the API will work but \
             the app will 404. Run `npm run build` in frontend/, or set STATIC_DIR."
        );
    }

    // Fetchers hit slow/rate-limited external APIs (OONI alone can take
    // minutes) — run them in the background instead of blocking server
    // startup on them. The server is fully usable immediately; fetched data
    // fills in as each source completes.
    let fetch_state = state.clone();
    tokio::spawn(async move {
        db::run_fetcher_loop(fetch_state).await;
    });

    // The hot in-memory catalog (`SatelliteCatalog`) is independent of
    // AppState/SQLite, and on its own (much shorter) cadence than the
    // fetchers above — see `satellites` module docs for why. It still gets
    // its own `AppState` clone, same as the two loops above, purely so its
    // refresh loop can persist/restore a durable copy of what it fetches.
    let satellite_catalog = satellites::new_catalog();
    let satellite_fetch_catalog = satellite_catalog.clone();
    let satellite_fetch_state = state.clone();
    tokio::spawn(async move {
        fetchers::satellites::run_catalog_refresh_loop(satellite_fetch_catalog, satellite_fetch_state)
            .await;
    });

    // Same AppState/SQLite state as the fetch loop above — just a much
    // shorter cadence, since HTTP/3 protocol-share and BGP prefix-visibility
    // are meant to be leading indicators, not caught up to 6 hours late.
    let precision_state = state.clone();
    tokio::spawn(async move {
        db::run_precision_fetcher_loop(precision_state).await;
    });

    // Durability, on its own cadence again — unrelated to any fetcher's, since
    // this is about surviving a redeploy rather than about any data source.
    // Absent entirely when snapshots are off, so local runs spawn nothing and
    // a failed restore cannot lead to an upload.
    if let Some(snapshotter) = snapshotter {
        let interval_state = state.clone();
        let interval_snapshotter = snapshotter.clone();
        tokio::spawn(async move {
            interval_snapshotter.run_loop(interval_state).await;
        });

        // The interval above only bounds what an unplanned crash costs; this
        // is what makes an ordinary redeploy lossless.
        let shutdown_state = state.clone();
        tokio::spawn(async move {
            snapshotter.run_shutdown_hook(shutdown_state).await;
        });
    }

    // Public and read-only. Every route below is a GET; `POST /api/evaluate`
    // is deliberately NOT mounted — it was the one write path (it persists
    // evidence rows) and nothing in the UI calls it, so on a public URL it
    // would be an anonymous write for no benefit. `api::evaluate` and the
    // whole `engine` module stay compiled and ready to re-route behind a
    // guard if the feature is ever wanted.
    //
    // CORS is absent on purpose: the browser loads the app and calls the API
    // from the same origin, so there is no cross-origin request to permit.
    let app = Router::new()
        .route("/health", get(api::health::health))
        .route("/api/countries", get(api::countries::list_countries))
        .route("/api/countries/:code", get(api::countries::get_country))
        .route("/api/geo", get(api::geo::list_geo))
        .route(
            "/api/starlink-status",
            get(api::starlink_status::list_starlink_status),
        )
        .route("/api/cables", get(api::cables::list_cables))
        .route("/api/ixp-stats", get(api::ixp::list_ixp_stats))
        .route("/api/models", get(api::models::list_models))
        .route("/api/signals", get(api::signals::list_signals))
        .route("/api/blocking", get(api::blocking::list_blocking))
        .route("/api/categories", get(api::categories::list_categories))
        .route("/api/timeline", get(api::timeline::list_timeline))
        .route("/api/tor-metrics", get(api::tor_metrics::list_tor_metrics))
        .route("/api/outages", get(api::outages::list_outages))
        .route("/api/rankings", get(api::rankings::list_rankings))
        .route(
            "/api/censorship-index",
            get(api::censorship_index::list_censorship_index),
        )
        .route(
            "/api/country-scores",
            get(api::country_scores::list_country_scores),
        )
        .route("/api/satellites", get(api::satellites::list_satellites))
        // Before the `:norad_id` route is irrelevant to matching (the two have
        // different segment counts), but it must come before `route_layer`
        // below so the catalog Extension reaches it.
        .route(
            "/api/satellites/status",
            get(api::satellites::satellites_status),
        )
        .route(
            "/api/satellites/:norad_id/orbit",
            get(api::satellites::satellite_orbit),
        )
        // Scoped to the satellite routes above (not the SPA fallback below)
        // via `route_layer`. The position/orbit handlers take this *instead*
        // of `State<AppState>`, since they have nothing to do with SQLite;
        // `satellites_status` takes both, because half of what it reports is
        // refresh history that only survives restarts by living in SQLite.
        .route_layer(Extension(satellite_catalog))
        // Placed after `route_layer` above on purpose: these handlers take
        // neither the satellite catalog nor `State`, and nothing is gained by
        // sitting inside that Extension's scope.
        .route("/api/methodology", get(api::methodology::list_methodology))
        .route(
            "/api/methodology/:slug",
            get(api::methodology::get_methodology),
        )
        .route(
            "/api/http-protocol-share",
            get(api::http_protocol_share::list_http_protocol_share),
        )
        .route(
            "/api/bgp-visibility",
            get(api::bgp_visibility::list_bgp_visibility),
        )
        // The built SPA. Anything not matching a route above falls through to
        // ServeDir, and anything ServeDir can't find falls through to
        // index.html so client-side routes and deep links resolve.
        .fallback_service(spa_service(&static_dir))
        // Applied last so it wraps every route above *and* the SPA fallback —
        // the static bundle is 13.7 MB, most of it Cesium, and was being
        // served uncompressed to every first-time visitor.
        //
        // The default predicate skips bodies under 32 bytes and already-
        // compressed content types (images, video), so the JPEG textures and
        // the like are not re-compressed for nothing.
        //
        // Measured on /api/satellites: 2.36 MB -> 0.62 MB. That endpoint is
        // polled every 7s per open tab, so it dominates egress.
        .layer(CompressionLayer::new())
        .with_state(state.clone());

    // Defaults to 3001 (what the Vite dev proxy targets). Overridable so a
    // second instance can be run alongside a dev server for verification.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Listening on 0.0.0.0:{port} (app + API, public read-only)");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Says plainly, at boot, whether the satellite catalog survived.
///
/// This exists because the single most damaging failure mode in this
/// deployment is silent: if the database was not restored, every redeploy
/// starts from an empty one and the satellite catalog has to be rebuilt from
/// scratch — which, during a CelesTrak outage, means it comes back as a
/// fraction of its real size. The plain "DB at <path>" line above looks
/// identical either way.
///
/// The boot counter in `satellite_refresh_state` is what makes the two cases
/// distinguishable: it can only be absent on a database no process has ever
/// booted against. A second boot reporting #1 means the file this process
/// opened is not the file the last one wrote.
fn report_persistence(state: &AppState, db_path: &str, db_existed: bool) {
    let boot = match db::satellite_catalog::record_boot(state) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("WARNING: could not record the boot marker: {e:#}");
            return;
        }
    };
    let satellites = state
        .lock()
        .ok()
        .and_then(|conn| db::satellite_catalog::count(&conn).ok())
        .unwrap_or(0);

    if db_existed && boot > 1 {
        println!(
            "DB restored: existing database at {db_path} (boot #{boot}), \
             {satellites} satellite(s) in the persistent catalog"
        );
        return;
    }

    eprintln!("WARNING: DB at {db_path} has no previous state (boot #{boot}).");
    eprintln!(
        "WARNING: that is expected on a first-ever deployment, and a problem on any other — \
         it means the previous deployment's data is gone."
    );
    // Only worth saying where it can actually be acted on. A local `cargo run`
    // legitimately has no snapshot and would just be told off for it.
    if std::env::var("SNAPSHOT_KEY").is_ok_and(|v| !v.trim().is_empty()) {
        eprintln!(
            "WARNING: Replit rebuilds a published app's filesystem on every publish, so \
             DATABASE_PATH only survives via the App Storage snapshot. Check the `snapshot:` \
             lines above — the restore must not have reported a WARNING. See README \
             \"Deployment (Replit)\"."
        );
    } else {
        eprintln!(
            "WARNING: SNAPSHOT_KEY is not set, so nothing is persisting this database. That is \
             correct for a local run and wrong for a deployment — see README \
             \"Deployment (Replit)\"."
        );
    }
}

/// `ServeDir` for the built SPA, falling back to `index.html` so client-side
/// routes and refreshes on a deep link return the app instead of a 404.
fn spa_service(dir: &str) -> ServeDir<ServeFile> {
    ServeDir::new(dir).fallback(ServeFile::new(format!("{dir}/index.html")))
}

/// The optional API tokens gate whole data sources, and their absence used to
/// surface only as one `eprintln!` from deep inside a background task — so a
/// deploy that forgot `PULSE_API_TOKEN` looked identical to "Pulse has no data
/// for these countries". Say it once, loudly, at boot.
fn warn_missing_optional_tokens() {
    let missing: Vec<(&str, &str)> = [
        (
            "PULSE_API_TOKEN",
            "Internet Resilience Index (all countries)",
        ),
        (
            "CLOUDFLARE_API_TOKEN",
            "Cloudflare Radar outage annotations + HTTP/3 protocol-share signal",
        ),
    ]
    .into_iter()
    .filter(|(key, _)| {
        std::env::var(key)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .is_none()
    })
    .collect();

    if missing.is_empty() {
        return;
    }
    eprintln!("WARNING: {} optional API token(s) missing —", missing.len());
    for (key, effect) in missing {
        eprintln!("WARNING:   {key} unset -> no data for: {effect}");
    }
    eprintln!(
        "WARNING: those sources will render empty, not error. Set them in .env or the host environment."
    );
}
