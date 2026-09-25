# BLACKOUT

BLACKOUT is a read-only web app for exploring internet freedom, censorship, and network disruption by country. It combines a React/Cesium globe frontend with a Rust/Axum API, a local SQLite store, background fetchers, and a small set of seeded reference data.

## What the app does

- Renders a whole-world 3D globe with:
  - a composite censorship-index choropleth derived from V-Dem and RSF
  - OONI-based country blocking blooms
  - recent IODA internet-outage blooms
  - an optional submarine-cable overlay
  - live-polled satellite positions, with selectable categories and orbit paths
- Opens a country sidebar with per-country metrics for:
  - blocking status across AI access and circumvention tools
  - messaging app reachability
  - censored content categories
  - 90-day outage history
  - Tor relay and bridge usage
  - HTTP/1.x, HTTP/2, and HTTP/3 traffic share
  - BGP visibility
  - Internet Exchange Point density
  - Starlink restriction status
  - Internet Society Pulse resilience scores
  - V-Dem and RSF freedom scores
- Shows a global "most censored countries" ranking panel and a footer with source links, link state, and data age.

Any drawable country on the globe can be selected. Separately, the backend still contains 5 hand-researched country dossier rows (`IR`, `SY`, `AE`, `SA`, `IQ`), but the shipped UI is primarily driven by the live and seeded measurement datasets above.

## Architecture

- `frontend/` — React 18 + Vite + CesiumJS + Recharts.
- `backend/` — Rust (edition 2024) + Axum + Tokio + `rusqlite` with bundled SQLite.

On startup, the backend creates and seeds the database, starts background fetch loops, and serves the built SPA from `frontend/dist` when present. Satellite orbital elements are cached in memory, while rendered satellite positions are computed fresh on each `/api/satellites` request.

### Satellite catalog persistence

The satellite catalog is the one dataset that cannot be rebuilt on demand: CelesTrak rate-limits repeat downloads and has been observed unreachable at the TCP level for extended periods. It is therefore treated as durable state rather than a cache.

- The canonical catalog is the `satellite_catalog` table, **one row per NORAD catalog ID**. Membership belongs to that table, not to any upstream response.
- A refresh is an *upsert*, never a replacement. Records missing from a response are left untouched, so a truncated or failed fetch cannot shrink the catalog. There is no code path that deletes a satellite.
- Per-object freshness (`epoch`, `last_updated`, `last_seen`) is tracked separately from existence. Degraded data makes an object **stale**, never absent.
- On boot the catalog is loaded from SQLite and served *before* any network call, and a refresh only runs if the persisted catalog is actually due for one — so redeploying repeatedly costs zero upstream requests.
- Merge precedence: the newest element-set **epoch** wins regardless of provider; ties break toward CelesTrak supplemental, then CelesTrak GP, then SatNOGS. CelesTrak and SatNOGS are complementary sources merged into the same catalog, not alternatives.

`GET /api/satellites/status` reports catalog size, the fresh/stale/unpropagatable split, per-provider last success, and the last refresh outcome (`complete` / `degraded` / `failed`).

**Freshness and element age are different questions.** `stale` tracks
`last_updated` — when *we* last accepted a better record. `unpropagatable`
tracks the element set's own `epoch`. They come apart because SatNOGS
republishes long-dead objects: a record accepted a minute ago can carry
orbital data from 1975. Objects whose epoch is older than
`SATELLITE_MAX_PROPAGATION_AGE_DAYS` (default 30) stay in the catalog and are
counted in `total`, but are left out of `/api/satellites` and their orbit
endpoint returns 422 — SGP4 cannot produce a meaningful position from them,
and a confidently-wrong dot is worse than no dot.

This requires persistent storage; see [Deployment](#deployment).

## Data sources

| Source | Used for |
| --- | --- |
| OONI | Technology blocking, messaging reachability, content-category censorship, blocking timelines |
| IODA | Country-level internet outage events |
| Tor Metrics | Relay/bridge users and bridge transport estimates |
| Cloudflare Radar | HTTP-version share and outage annotations |
| RIPEstat | BGP ASN/prefix visibility |
| Internet Society Pulse | Internet Resilience Index |
| V-Dem via Our World in Data | Freedom of expression score and internet-censorship subscores |
| RSF via Our World in Data | Press freedom score |
| PeeringDB | Seeded IXP density lookup |
| TeleGeography | Seeded submarine cable routes and landing points |
| CelesTrak | Satellite catalog and orbital elements |
| Manual seed data | Starlink country restrictions |

## Running locally

Prerequisites: Rust stable and Node 18+.

### 1. Backend (`:3001`)

```sh
cd backend
cargo run
```

This creates `mena_ai.db`, loads the seed data, and starts the API immediately. External fetchers then populate and refresh the live datasets in the background.

Useful optional env vars are documented in `backend/.env.example`, including:

- `DEPLOYMENT_ID`
- `CLOUDFLARE_API_TOKEN`
- `PULSE_API_TOKEN`
- `PEERINGDB_API_KEY` (used by `scripts/gen_ixp_data.mjs`, not by the server)
- `DATABASE_PATH`
- `SEED_DIR`
- `STATIC_DIR`
- `FETCH_INTERVAL_HOURS`
- `PRECISION_FETCH_INTERVAL_HOURS`
- `SATELLITE_CATALOG_REFRESH_HOURS`
- `SATELLITE_STALE_AFTER_HOURS`
- `SATELLITE_GROUP_MIN_RETAIN_RATIO`
- `PORT`

### 2. Frontend (`:5173`)

```sh
cd frontend
npm install
npm run dev
```

Open <http://localhost:5173>. Vite proxies both `/api` and `/health` to `http://localhost:3001` in development. `VITE_CESIUM_ION_TOKEN` is optional; the current globe setup does not require it.

### 3. Production build

```sh
cd frontend
npm run build
```

By default, the backend serves `../frontend/dist` when that build output exists.

## API surface

The shipped app mounts these read-only routes:

- `GET /health`
- `GET /api/geo`
- `GET /api/countries`
- `GET /api/countries/:code`
- `GET /api/blocking`
- `GET /api/categories`
- `GET /api/timeline`
- `GET /api/tor-metrics`
- `GET /api/outages`
- `GET /api/censorship-index`
- `GET /api/rankings`
- `GET /api/country-scores`
- `GET /api/starlink-status`
- `GET /api/cables`
- `GET /api/ixp-stats`
- `GET /api/satellites`
- `GET /api/satellites/status`
- `GET /api/satellites/:norad_id/orbit`
- `GET /api/models`
- `GET /api/signals`
- `GET /api/http-protocol-share`
- `GET /api/bgp-visibility`

Notes:

- `/api/geo` is the whole-world drawable country list used by the globe.
- `/api/countries` is only the small researched-country metadata set.
- `POST /api/evaluate` exists in code but is not mounted in the public read-only app.
- `/api/models` and `/api/signals` are exposed by the backend but are not currently used by the shipped frontend.

## Deployment

The app runs on Replit as a Reserved VM. There is no Dockerfile; the build and
run contract lives in two files at the repo root:

- `.replit` — build and run commands, `[deployment]`, port mapping, `[env]`
- `replit.nix` — `rustup`, Node, OpenSSL, a C toolchain (for `rusqlite`'s
  bundled SQLite) and `cacert`

The Rust toolchain is pinned in `.replit`'s build command through `rustup`
rather than taken from the Nix channel, whose Cargo is older than
`edition = "2024"` requires. The build stage copies the release binary to
`bin/` because `backend/target/` is excluded from what is handed to the run
stage.

Reserved VM rather than Autoscale: the fetch loops (6h), precision loops (1h),
satellite refresh (2h) and snapshot loop (3h) are timer-driven, and an
Autoscale deployment scales to zero between requests and never runs them. The
build links ~200 crates and compiles SQLite from C source, so it wants at
least 2 vCPU / 4 GB. At runtime `/api/satellites` propagates every matching
element set with SGP4 on each request, polled every 7s per connected browser.

Replit keeps workspace secrets and deployment secrets as separate sets. A key
present only in the workspace is undefined in the published app, which is
indistinguishable from a source genuinely having no data.

### Persistence

Publishing rebuilds the app's files from the workspace tree, so
`DATABASE_PATH` does not survive a deploy. That is data loss rather than a
cold cache: three fetchers pull bounded rolling windows and accumulate history
locally that upstream will not return again.

| Table | Window | Fetcher |
| --- | --- | --- |
| `outage_events` | 90 days | `fetchers/ioda.rs` |
| `bgp_prefix_visibility` | 14 days | `fetchers/ripestat.rs` |
| `http_protocol_share` | 14 days | `fetchers/cloudflare_http.rs` |

That is on top of `satellite_catalog`, which cannot be rebuilt on demand at
all.

The database is therefore snapshotted to Replit App Storage. `SNAPSHOT_KEY` in
`.replit` enables the layer; with it unset the layer is inert, which is the
local-development case. On boot, when `DATABASE_PATH` is absent, the stored
snapshot is downloaded, verified and put in place before SQLite opens it. It
is written back every `SNAPSHOT_INTERVAL_MINUTES` (default 180, and only when
something has been written since the last one) and once more on `SIGTERM`. The
shutdown snapshot is what makes an ordinary redeploy lossless; the interval
bounds only what an unplanned crash costs.

A snapshot is `VACUUM INTO` plus gzip — a consistent, defragmented image at
roughly a quarter the size of the live file.

**This does not currently work on the deployment.** The App Storage sidecar
answers on `127.0.0.1:1106` from the workspace but nothing listens there in
the published container — observed over 7 attempts across 11.8s. Until that is
resolved the app cold-starts on every publish: it is fully serving in under a
second and fully current in about two and a half minutes, but history that
upstream will not re-serve (outage events beyond 90 days, BGP and HTTP/3
beyond 14) resets each time. A seeded snapshot is kept outside the repo ready
to restore once it does work.

A failed restore never overwrites a good snapshot. When the download errors no
snapshot task is spawned for that process, so no code path can write, and a
`WARNING` block says so. A clean 404 — nothing stored yet — is deliberately a
different outcome and does allow writes. A snapshot whose uncompressed size
has collapsed below `SNAPSHOT_MIN_RETAIN_RATIO` (default 0.5) of the last good
one is refused, on the same reasoning as the satellite shrinkage guard.

### Outbound identity

Every outbound request carries
`blackout-<fetcher>/<version> (+<contact>; id=<DEPLOYMENT_ID>)`, and RIPEstat's
`sourceapp` derives from the same value. The identity is built in
`backend/src/util/http.rs`, every HTTP client in the codebase is constructed
through it, and the boot log prints the result.

This is identification, not rate-limit management. Upstream limits are
enforced either on the credential — Cloudflare Radar, Internet Society Pulse,
PeeringDB — or on the source IP — OONI, IODA, Tor Metrics, RIPEstat, CelesTrak,
SatNOGS and Our World in Data. Several of those are free services run by
volunteers or a small nonprofit, and identifying honestly costs nothing.

### Credentials

All three are free, and all are optional in the sense that the app starts
without them; a `WARNING` block at boot names any that are missing.

| Secret | Gates | Scope |
| --- | --- | --- |
| `CLOUDFLARE_API_TOKEN` | Radar outage annotations and HTTP-version share | `Account` → `Radar` → `Read` |
| `PULSE_API_TOKEN` | Internet Resilience Index | n/a |
| `PEERINGDB_API_KEY` | `scripts/gen_ixp_data.mjs` only, never the server | read-only |

Without the first two, those sources render empty rather than erroring.
`PEERINGDB_API_KEY` matters only when regenerating the committed IXP seed
data: PeeringDB throttles unauthenticated callers hard — an observed
59-minute lockout after three quick requests — though the script works
without one.

### Boot log

A healthy redeploy prints:

```text
snapshot: restored /home/runner/data/mena_ai.db from replit-objstore-.../mena_ai.db.gz (19.1 MiB compressed -> 84.0 MiB).
DB restored: existing database at /home/runner/data/mena_ai.db (boot #3), 13587 satellite(s) in the persistent catalog
satellites: loaded 13587 satellites from the persistent catalog; catalog age 47m
Snapshot loop: every 180m -> replit-objstore-.../mena_ai.db.gz
```

`boot #1` on anything other than a first-ever deploy means the restore did not
happen and state is being lost. The `snapshot:` lines above it say why: no
`SNAPSHOT_KEY`, no App Storage sidecar, or a failed download.

## Notes

- The deployment model is public and read-only. There is no auth layer.
- Data freshness is mixed by design: some datasets are periodically fetched, some are committed seed files, and Starlink status is manually maintained.
- Built by Moumen Alaoui at the FAI Hackathon 2026, from [moumenalaoui/BLACKOUT](https://github.com/moumenalaoui/BLACKOUT).
