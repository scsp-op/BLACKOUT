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
  - blocking status across AI access, circumvention tools, and privacy-focused operating systems
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

**This requires persistent storage**, which on Replit means the App Storage snapshot rather than a disk. See [Deployment (Replit)](#deployment-replit).

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

## Deployment (Replit)

Replit does not build Dockerfiles. The whole build/run contract is two files
at the repo root:

- `.replit` — build and run commands, `[deployment]`, port mapping, `[env]`
- `replit.nix` — Rust, Node, OpenSSL, a C toolchain (for rusqlite's bundled
  SQLite) and `cacert`

Publish as a **Reserved VM**, not Autoscale. The fetch loops (6h), precision
loops (1h), satellite refresh (2h) and snapshot loop (3h) all run on timers in
the background; an Autoscale deployment scales to zero between requests and
would simply never run them. Budget **≥ 2 vCPU / 4 GB** — linking 185 Rust
crates is the build's memory peak, and at runtime `/api/satellites` propagates
every matching element set with SGP4 on every request, polled every 7s per
connected browser.

Set `CLOUDFLARE_API_TOKEN` and `PULSE_API_TOKEN` in Replit Secrets. Both are
optional; without them those sources render empty and a `WARNING` block at
boot says so. **Replit keeps workspace secrets and deployment secrets as two
separate sets** — a key added only in the workspace is undefined in the
published app, which looks exactly like a source having no data.

### The database is not on the filesystem in any durable sense

Publishing rebuilds the app's files from the workspace tree, so
`DATABASE_PATH` is empty after every publish. That is not a cold cache — it is
data loss. Three fetchers pull **bounded rolling windows** and accumulate
history locally that upstream will never return again:

| Table | Window | Fetcher |
| --- | --- | --- |
| `outage_events` | 90 days | `fetchers/ioda.rs` |
| `bgp_prefix_visibility` | 14 days | `fetchers/ripestat.rs` |
| `http_protocol_share` | 14 days | `fetchers/cloudflare_http.rs` |

— on top of `satellite_catalog`, which cannot be rebuilt on demand at all.

So the database is snapshotted to **Replit App Storage** instead. `SNAPSHOT_KEY`
in `.replit` turns this on; unset, the layer is completely inert (which is what
you want locally). On boot, if `DATABASE_PATH` is absent, the snapshot is
downloaded, verified and put in place before SQLite opens it. It is written
back every `SNAPSHOT_INTERVAL_MINUTES` (default 180, only when something has
actually been written) and once more on `SIGTERM` — the shutdown snapshot is
what makes an ordinary redeploy lossless, and the interval only bounds what an
unplanned crash costs.

A snapshot is `VACUUM INTO` plus gzip, so it is a consistent, defragmented
image at roughly a quarter the size of the live file.

**A failed restore never overwrites a good snapshot.** If the download errors,
no snapshot task is spawned at all for that process — there is no code path
that could write — and a loud `WARNING` block says so. A clean 404 ("nothing
stored yet") is deliberately a different outcome and does allow writes. On top
of that, a snapshot whose uncompressed size has collapsed below
`SNAPSHOT_MIN_RETAIN_RATIO` (default 0.5) of the last good one is refused; same
reasoning as the satellite shrinkage guard.

### Seeding the first deploy

Upload an existing database so the first boot starts with full history instead
of a cold fetch and a 503 window:

```sh
sqlite3 mena_ai.db "VACUUM INTO '/tmp/mena_ai.db'"
gzip -9 /tmp/mena_ai.db
# upload /tmp/mena_ai.db.gz to App Storage as the object named by SNAPSHOT_KEY
```

### Reading the boot log

A healthy redeploy prints:

```text
snapshot: restored /home/runner/data/mena_ai.db from replit-objstore-.../mena_ai.db.gz (18.4 MiB compressed -> 71.2 MiB).
DB restored: existing database at /home/runner/data/mena_ai.db (boot #7), 16284 satellite(s) in the persistent catalog
satellites: loaded 16284 satellites from the persistent catalog; catalog age 47m
Snapshot loop: every 180m -> replit-objstore-.../mena_ai.db.gz
```

`boot #1` on anything but a first-ever deploy means the restore did not happen
and state is being lost. The `snapshot:` lines above it say why: no
`SNAPSHOT_KEY`, no App Storage sidecar, or a failed download.

## Running two deployments against the same upstreams

Every outbound request carries
`blackout-<fetcher>/<version> (+<contact>; id=<DEPLOYMENT_ID>)`, and RIPEstat's
`sourceapp` defaults to `blackout-<DEPLOYMENT_ID>`. Set `DEPLOYMENT_ID` and
both follow; the identity is built in one place (`backend/src/util/http.rs`)
and every HTTP client in the codebase is constructed through it. The boot log
prints the result, and warns if `DEPLOYMENT_ID` was never set.

Be precise about what that separates, because the intuitive answer is wrong.
Upstream limits here are enforced on one of two things:

| Enforced on | Sources | How to separate two deployments |
| --- | --- | --- |
| The credential | Cloudflare Radar, Internet Society Pulse, PeeringDB | **A separate token per deployment.** Nothing else works. |
| The source IP | OONI, IODA, Tor Metrics, RIPEstat, CelesTrak, SatNOGS, OWID | Already separate — two hosts, two addresses. |

`DEPLOYMENT_ID` separates neither. What it buys is that an upstream operator
who throttles or blocks someone can tell *which* instance it was, and has
somewhere to write. Several of these are free services run by volunteers or a
small nonprofit.

The three credentials, all free, all per-deployment:

| Secret | Where to get it | Scope |
| --- | --- | --- |
| `CLOUDFLARE_API_TOKEN` | Cloudflare dashboard → My Profile → API Tokens → Create Custom Token | `Account` → `Radar` → `Read`, nothing else |
| `PULSE_API_TOKEN` | `pulse.internetsociety.org` account → profile → generate key (label it per deployment) | n/a |
| `PEERINGDB_API_KEY` | PeeringDB account → profile page | read-only |

`PEERINGDB_API_KEY` is read only by `scripts/gen_ixp_data.mjs`, a one-off
generator whose output is committed. PeeringDB throttles unauthenticated
callers hard — an observed 59-minute lockout after three quick requests — so
the key is worth having before re-running it, though the script works without
one.

## Notes

- The deployment model is public and read-only. There is no auth layer.
- Data freshness is mixed by design: some datasets are periodically fetched, some are committed seed files, and Starlink status is manually maintained.
- Built by Moumen Alaoui at the FAI Hackathon 2026.
