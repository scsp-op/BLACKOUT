# BLACKOUT — Technical Methodology

**What this document is.** An account of how BLACKOUT is built: its architecture, the
algorithms it implements, the data structures that hold its state, the mechanisms that
keep it correct, and the reasoning behind each choice. Every non-obvious claim is cited
to `file:line`, and the parts a reader would want to check for themselves are quoted
verbatim rather than paraphrased.

It is written for someone who will read the source. It assumes you would rather see the
arithmetic than be told it is sound.

**Its companion.** The policy methodology covers what BLACKOUT measures, where each
number comes from as a *source*, and what the tool can and cannot be used to claim. It
is written for analysts. The two documents deliberately do not overlap: where this one
gives the code for the censorship index, that one gives its interpretation and its
limits. Where this one describes the snapshot layer's failure modes, that one states
the integrity principle the layer exists to serve.

If you want to know whether a number can be cited, read that document. If you want to
know whether the machine producing it is built correctly, read this one.

---

## 1. System overview

BLACKOUT is a single Rust binary that serves a JSON API and a static React bundle from
the same port, backed by one local SQLite file, with four background timer loops that
keep that file populated from ten external sources.

```text
                         ┌─────────────────────────────────────────┐
                         │  one process, one port (default 3001)   │
   browser ──────────────▶                                         │
                         │  axum Router                            │
                         │    GET routes  ──────┐                  │
                         │    SPA fallback      │                  │
                         │    CompressionLayer  │                  │
                         └──────────────────────┼──────────────────┘
                                                │
                    ┌───────────────────────────┴───────────┐
                    │                                       │
            Arc<Mutex<Connection>>                 Arc<RwLock<Catalog>>
            (SQLite, WAL)                          (satellite elements,
                    │                               in memory)
                    │                                       ▲
     ┌──────────────┼───────────────┬─────────────┐         │
     │              │               │             │         │
  fetch loop   precision loop   snapshot loop  shutdown   catalog
    (6 h)          (1 h)           (3 h)        hook     refresh (2 h)
     │              │               │                        │
     ▼              ▼               ▼                        ▼
  OONI, IODA,   Cloudflare     Replit App             CelesTrak,
  Tor, OWID,    Radar HTTP,    Storage                SatNOGS
  Pulse, CF     RIPEstat       (gzipped
  Radar                         VACUUM image)
```

There is no separate API server, no worker process, no message queue, no cache tier and
no external database. The deployment is one binary and one file.

### 1.1 Scale

| Artifact | Files | Lines |
| --- | ---: | ---: |
| Rust (`backend/src/**/*.rs`) | 72 | 13,620 |
| JS / JSX (`frontend/src`) | 32 | 4,812 |
| Node generator scripts (`backend/scripts/*.mjs`) | 4 | 558 |
| CSS | 2 | 78 |
| `insta` snapshot fixture | 1 | 3,978 |
| Committed seed data (JSON + CSV) | 9 | 1.79 MB |

Rust, by module:

| Module | Lines | What lives there |
| --- | ---: | --- |
| `fetchers/` | 4,261 | the ten upstream clients |
| `db/` | 3,192 | schema, migrations, seed loaders, query helpers, the fetch loops |
| `api/` | 1,557 | the route handlers |
| `engine/` | 1,422 | the unmounted assessment engine (§12) |
| `snapshot/` | 1,166 | durability |
| `models/` | 739 | serde types |
| `satellites/` | 529 | in-memory catalogue, SGP4 driving, frame conversion |
| `util/` | 419 | outbound identity, date arithmetic |
| `main.rs` | 335 | boot sequence and router |

Counts exclude `api/methodology.rs`, which serves these documents themselves (§7).

### 1.2 Rationale in the source

2,063 of those 13,620 Rust lines are comments, 1,108 of them doc comments — about 15%.
More usefully, 130 comment lines contain an explicit justification (`deliberately`,
`rather than`, `on purpose`), and many record the incident that produced the decision.
The module header of `snapshot/object_store.rs` contains a post-mortem of a lost
deployment; `satellites/mod.rs` names the 1975-epoch objects that motivated a threshold.

Much of the reasoning below therefore comes from the source rather than from
reconstruction, and where a comment states a rationale better than a paraphrase would,
it is quoted directly.

---

## 2. Why this architecture

Four constraints produced nearly every structural decision. Most of what follows is
downstream of one of them.

**1. The build had about eight weeks.** 95 commits between 2026-07-31 and 2026-09-25.
That budget bought a single-binary design with no infrastructure to operate: SQLite
instead of Postgres, an in-process scheduler instead of a job runner, `println!` instead
of a tracing pipeline, and static file serving from the same process as the API. Each is
defensible at this scale and a liability at ten times it. The seams are catalogued in
§13.

**2. The runtime is two vCPUs.** A Replit Reserved VM. This is why `/api/satellites`
memoises propagated positions, why it caches a *pre-gzipped* copy of the body, why
coordinates are rounded before serialisation, and why `tower-http` is compiled with gzip
and not brotli. Each of those is annotated in the source with the measurement that
justified it — `Cargo.toml:12-16`, `api/satellites.rs:127-144`.

**3. Ten upstreams, each with a different failure mode.** Some rate-limit on a
credential, some on source IP. Some return 403 to mean "you already have the current
version." Some republish fifty-year-old data as if it were fresh. Some are free services
run by volunteers. The ingestion layer is shaped almost entirely by the need to contain
these independently: per-source timeout budgets, per-source pacing, per-source retry,
and a bookkeeping table that records degradation instead of hiding it (§4).

**4. Some of the data cannot be re-fetched.** Three sources publish only bounded rolling
windows — IODA 90 days, RIPEstat 14 days, Cloudflare HTTP share 14 days. Anything
accumulated past those windows exists in exactly one place. The satellite catalogue is
worse: CelesTrak rate-limits repeat downloads and has been observed unreachable at the
TCP level for extended periods, so it cannot be rebuilt on demand at all.

That fourth constraint most distorts the design away from a conventional web service.
Losing the database is not a cold cache, it is data loss. The consequences run through
the satellite catalogue's additive-only merge (§6), the snapshot layer's structural
no-overwrite invariant (§8), shrinkage guards in two separate subsystems, and a
boot-time check that exists for no reason other than to make a silent restore failure
loud (§8.6).

---

## 3. The data model

One SQLite file, 29 tables, opened once at boot and held behind a single mutex for the
life of the process.

### 3.1 Connection model

```rust
// backend/src/main.rs:22
pub type AppState = Arc<Mutex<Connection>>;
```

A `std::sync::Mutex`, not a tokio one, wrapping a single `rusqlite::Connection`. There
is no pool. Every handler that touches SQLite takes the lock, runs its query, drops it.

Pragmas are set once in the schema batch (`db/schema.rs:7-15`):

```sql
PRAGMA journal_mode=WAL;
PRAGMA busy_timeout=5000;
PRAGMA synchronous=NORMAL;
```

`journal_mode` persists in the file; the other two are per-connection and must be
re-applied on every open. WAL turns out to be load-bearing for durability in a way
unrelated to concurrency — the snapshot layer's dirty-tracking depends on the `-wal`
file moving whenever the database does (§8.5).

This is the design's clearest concurrency compromise, and a real one: a slow query
blocks every other SQLite-backed route, and the snapshot's `VACUUM INTO` holds the lock
for one to two seconds at 70 MB; §13 returns to it. Two mitigations exist today.
The satellite endpoints — by far the most frequently polled — deliberately do not use
`AppState` at all (§6.6). And the expensive snapshot work runs under `spawn_blocking`,
so it never occupies a runtime worker (§8.3).

### 3.2 Table inventory

Grouped by what writes them.

**Reference data — written only by the seed loader at boot.**

| Table | Key | Contents |
| --- | --- | --- |
| `country_reference` | `country_code` | 249 countries: ISO codes, centroid, bbox, `include_on_globe`, `priority_tier` |
| `starlink_status` | `country_code` | 25 hand-curated restriction rows |
| `cable_routes` | `feature_id` | 728 TeleGeography cables, `geometry` as JSON MultiLineString |
| `cable_landing_points` | `id` | 1,925 landing points, `country_code` nullable |
| `ixp_stats` | `country_code` | 170 countries with PeeringDB IXP counts |
| `countries` | `country_code` | 5 hand-researched policy dossiers |
| `model_releases` | `model_id` | 8 rows |

**Measured data — written by the fetch loops.**

| Table | Key | Fetcher |
| --- | --- | --- |
| `technology_blocks` | `(country_code, technology)` | OONI phase B |
| `category_blocks` | `(country_code, category_code)` | OONI phase C |
| `blocking_timeline` | `(country_code, technology, measurement_date)` | OONI phase D |
| `adoption_signals` | `signal_id` | OONI phase A + Cloudflare Radar annotations |
| `outage_events` | `id` | IODA |
| `tor_metrics` | `id` = `{cc}-{date}` | Tor Metrics |
| `country_scores` | `id` = `{cc}-{source}` | V-Dem, RSF, ISOC Pulse |
| `http_protocol_share` | `id` = `{cc}-{date}` | Cloudflare Radar |
| `bgp_prefix_visibility` | `id` = `{cc}-{date}` | RIPEstat |
| `satellite_catalog` | `norad_id` | CelesTrak + SatNOGS |
| `satellite_refresh_state` | `key` | refresh bookkeeping, high-water marks, boot counter |
| `satellite_elements` | `(source_group, norad_id)` | legacy, migrated away from, read-only |
| `fetch_runs` | `fetcher` | per-fetcher run outcomes |

The remaining eight — `services`, `service_channels`, `path_templates`,
`path_dependencies`, `country_constraints`, `evidence_items`, `constraint_evidence`,
`path_evidence` — are seeded from hardcoded Rust arrays in `db/seed.rs` and feed the
unmounted assessment engine (§12). `model_usage` (`schema.rs:145`) has no loader and no
writer at all.

### 3.3 Why the keys are shaped as they are

The primary keys carry most of the deduplication logic in the system, because almost
every fetcher writes with `INSERT OR REPLACE` and relies on the key to decide whether a
row is new or a refresh. Three patterns:

**Composite natural keys, where the grain is the identity.** `technology_blocks` is
keyed `(country_code, technology)` (`schema.rs:176`), so there is exactly one current
row per pair and a re-fetch overwrites in place — the table holds current state, not
history. `blocking_timeline` is keyed
`(country_code, technology, measurement_date)` (`schema.rs:187`), so it *is* history,
one row per day. That distinction is the entire difference between the two tables.

The timeline key has a consequence the fetcher must respect: OONI's aggregation API
returns hourly buckets under some conditions, and an hourly timestamp in a column that
is part of the primary key would silently create duplicate logical days. The fetcher
filters them out with `util::date::is_iso_date` before writing (`ooni.rs:922`).

**Synthetic keys that deliberately omit a dimension.** `country_scores` uses
`format!("{country_code}-{}", source.db_name())` — for example `IR-V_DEM`
(`indices.rs:341-342`). The year is deliberately *not* in the key, so a re-run refreshes
in place rather than accumulating one row per year. Pulse does the same, keyed
`{country}-ISOC_PULSE` with no quarter (`pulse.rs:205-214`). These tables answer "what
is the current score"; the year rides along as a column so a consumer can see how
current it is.

**Synthetic keys that deliberately include one.** `outage_events` uses
`format!("ioda-{country}-{datasource}-{}", row.start)` (`ioda.rs:111`). The start
timestamp is in the key because IODA re-serves the same event across overlapping 90-day
windows and re-fetching must update rather than duplicate it. The `datasource` is in the
key because IODA reports the same outage independently from different measurement
backends, and those are genuinely different observations that should not collapse into
one another.

`adoption_signals` keys on `ooni-{country}-{slug(url)}-{date}` with `date = today_iso()`
(`ooni.rs:490-494`) — one row per country/URL/day, so re-running today overwrites and
running tomorrow appends.

One index decision goes the other way from the rest and is worth quoting:

> `blocking_timeline` deliberately gets nothing: its PRIMARY KEY autoindex already
> serves every query, and an extra index is paid on every row of a ~600k-row sweep.
> — `db/schema.rs:532-535`

### 3.4 Migrations

The migration system is deliberately minimal and does exactly one thing:

```rust
// backend/src/db/migrations.rs:26-56
const COLUMNS: &[(&str, &str, &str)] = &[ /* (table, column, decl) */ ];
```

Nineteen entries, applied in order at every boot, each an `ALTER TABLE ... ADD COLUMN`
of a nullable column. Three groups: six Tor per-transport bridge bounds, nine V-Dem
sub-score columns with their credible intervals, four ISOC Pulse resilience pillars.

There is **no version table and no down-migration.** Anything structural — dropping a
column, changing a type, altering a constraint — needs SQLite's twelve-step table
rebuild and is explicitly out of scope (`migrations.rs:10-14`).

Two details are engineering rather than description. The existence check uses
`PRAGMA table_info` rather than attempting the `ADD COLUMN` and catching the error:

> SQLite reports a duplicate column as a generic `SqliteFailure`, which is
> indistinguishable from a real failure.
> — `migrations.rs:74-78`

And a missing *target table* is a hard `bail!` rather than a silent skip
(`migrations.rs:80-85`), so a typo in the migration table fails the boot instead of
quietly doing nothing.

Entries are never pruned — "they are the record of what existing databases need, not a
changelog to prune" (`migrations.rs:24-25`). The cost is that adding a column is a
two-line change in two places, `schema.rs` for fresh databases and `COLUMNS` for
existing ones, and both are required. That coupling is stated in the module docs
(`migrations.rs:16-18`) precisely because it is easy to half-do.

### 3.5 Seeding

`db/seed.rs` runs sixteen loaders inside one explicit transaction, with `ROLLBACK` on
any error (`seed.rs:42-73`). A partial seed can never be committed.

Eight loaders read committed files from `SEED_DIR` (default `data/seed`, relative to the
working directory, normally `backend/`); the other eight are hardcoded Rust arrays
backing the dormant engine.

The conflict policy splits along a meaningful line. Generated data uses
`INSERT OR REPLACE`, so regenerating a script and redeploying actually takes effect.
`country_reference` goes further and does a wholesale `DELETE` before reinserting
(`seed.rs:78-81`) — because a row *removed* from the seed file must also disappear from
SQLite, which `INSERT OR REPLACE` alone would never accomplish. Tables a live process
may also write, or where database state should win over the seed, use
`INSERT OR IGNORE`.

| File | Rows | Policy |
| --- | ---: | --- |
| `country_reference.json` | 249 | `DELETE` then `INSERT OR REPLACE` |
| `cable_landing_points.json` | 1,925 | `INSERT OR REPLACE` |
| `cable_routes.json` | 728 | `INSERT OR REPLACE` |
| `ixp_stats.json` | 170 | `INSERT OR REPLACE` |
| `starlink_status.json` | 25 | `INSERT OR REPLACE` |
| `models.json` | 8 | `INSERT OR IGNORE` |
| `signals.json` | 6 | `INSERT OR IGNORE` |
| `countries.json` | 5 | `INSERT OR IGNORE` |
| `vdem_dsp_v16.csv` | 179 | read by `indices.rs`, not by `seed.rs` |

Of the 249 countries in `country_reference`, 235 carry `include_on_globe` and form the
sweep list every per-country fetcher iterates (`db/countries.rs:61-71`); five sit at
`priority_tier = 0` and sort first.

### 3.6 A boot-time invariant

Schema initialisation ends with an assertion rather than a query:

```rust
// backend/src/db/mod.rs:19-28
pub fn init_schema(conn: &Connection) -> Result<()> {
    schema::create_tables(conn)?;
    migrations::run(conn)?;
    seed::load_all(conn)?;
    assert_reference_covers_researched(conn)?;
```

If any row in `countries` has no matching row in `country_reference`, the process aborts
**before the listener binds** (`db/mod.rs:34-54`). A researched dossier with no reference
row would be a country the API knows about but the globe cannot draw or locate. Failing
the boot is the right response, and it is far cheaper to detect here than to debug from
a blank sidebar.
---

## 4. Ingestion

Ten upstream sources, none of which agree on anything: scale, cadence, vocabulary,
error conventions, or what a missing value looks like. The ingestion layer's job is to
normalise all of that without ever letting one source's bad day become another's.

### 4.1 The loops

`fetchers/mod.rs` is fifteen lines of module declarations. The loops themselves live in
`db/mod.rs` and are spawned from `main.rs` after the router is configured:

| Loop | Default interval | Env override | Spawned at |
| --- | --- | --- | --- |
| `run_fetcher_loop` | 6 h | `FETCH_INTERVAL_HOURS` | `main.rs:101-103` |
| `run_catalog_refresh_loop` | 2 h | `SATELLITE_CATALOG_REFRESH_HOURS` | `main.rs:113-116` |
| `run_precision_fetcher_loop` | 1 h | `PRECISION_FETCH_INTERVAL_HOURS` | `main.rs:122-124` |
| snapshot loop + shutdown hook | 3 h | `SNAPSHOT_INTERVAL_MINUTES` | `main.rs:130-143` |

The first and third are floored at 60 seconds — `((hours * 3600.0) as u64).max(60)`
(`db/mod.rs:144`, `:286`, constant at `:63`). The floor exists so that a mistyped
environment variable produces a slow loop rather than a request flood. The snapshot loop
has a much higher floor of 300 s for the same reason, scaled to what it uploads (§8.6).

The split into a 6-hour general loop and a 1-hour precision loop is not arbitrary.
HTTP/3 protocol share and BGP prefix visibility are the two signals meant to *lead* an
event rather than confirm it, and a six-hour lag would defeat that
(`main.rs:118-120`). Everything else tolerates six hours comfortably, and
`/health` only turns red at two days (`api/health.rs`, `HEALTH_MAX_AGE_DAYS` default 2),
so the 6-hour cycle sits well inside its own freshness SLO.

The general cycle runs eight sources concurrently through a single `tokio::join!`
(`db/mod.rs:206-268`), each wrapped in its own timeout budget:

| `fetch_runs` name | Timeout |
| --- | ---: |
| `ooni` | 300 s |
| `ooni_timeline` | 900 s |
| `ooni_categories` | 180 s |
| `tor_metrics` | 600 s |
| `ioda` | 180 s |
| `pulse` | 90 s |
| `indices` | 60 s |
| `cloudflare` | 30 s |

The precision cycle runs two (`db/mod.rs:301-328`): `cloudflare_http_protocol` at 600 s
and `ripestat_bgp_visibility` at 300 s. Those names are deliberately distinct from
`cloudflare` so that two different Cloudflare endpoints keep separate failure histories.

**Within** a fetcher there is no concurrency at all. Every per-country sweep is
sequential and paced: OONI 2,000 ms between requests, IODA 300 ms, Cloudflare HTTP
300 ms, Tor 300 ms, RIPEstat 200 ms, satellites 500 ms. This is a deliberate trade of
wall-clock time for being a good citizen — several of these are free services, and the
pacing is what keeps a 235-country sweep from looking like an attack.

### 4.2 Containment

Nothing a fetcher does can escape its loop iteration:

```rust
// backend/src/db/mod.rs:72-91 — run_with_timeout
//   Ok(Ok(()))  -> success
//   Ok(Err(e))  -> logged error
//   Err(_)      -> "timed out after {seconds}s"
```

Every outcome, including success, is written to `fetch_runs`. The conflict resolution
there encodes a subtle and important decision (`db/mod.rs:98-126`):

```sql
-- success
ON CONFLICT(fetcher) DO UPDATE SET
  last_attempt_at = ?2, last_success_at = ?2,
  last_outcome = 'ok', last_error = NULL, consecutive_failures = 0

-- failure: last_success_at deliberately untouched
ON CONFLICT(fetcher) DO UPDATE SET
  last_attempt_at = ?2, last_outcome = 'error', last_error = ?3,
  consecutive_failures = consecutive_failures + 1
```

Attempt and success are separate facts. A failing fetcher keeps updating
`last_attempt_at` while `last_success_at` stays frozen at the last time it actually
worked, so the age of the data and the liveness of the process are independently
readable. Collapsing these into one column is the ordinary mistake here, and it makes a
source that has been broken for a week indistinguishable from one that just ran.

`report_cycle` (`db/mod.rs:157-198`) prints one summary line per cycle naming any
fetcher whose `last_outcome != 'ok'`, with its consecutive failure count and its last
success.

**There is no global backoff.** `consecutive_failures` is recorded but never consulted
for scheduling; a failing source simply retries at the next interval. Per-source retry
exists only where the upstream's behaviour demands it: OONI's 429 handling
(`ooni.rs:334-347`) and the satellite fetcher's `with_retries` (`satellites.rs:236-260`).
IODA, RIPEstat, Cloudflare, Pulse, Tor and OWID have no retry at all — a failed item is
logged and the sweep continues.

### 4.3 Outbound identity

Every HTTP client in the codebase is constructed through one function:

```rust
// backend/src/util/http.rs:96-103
pub fn user_agent(component: &str) -> String {
    format!(
        "blackout-{component}/{} (+{}; id={})",
        env!("CARGO_PKG_VERSION"),
        contact(),
        deployment_id()
    )
}
```

Producing, for example,
`blackout-ioda/0.1.0 (+https://github.com/moumenalaoui/BLACKOUT; id=local)`.
The `component` vocabulary matches the `fetch_runs` names, so a log line upstream and a
row in the local bookkeeping table name the same thing.

The module exists because of a measured problem, recorded in its own header: nine of
twelve clients previously sent no `User-Agent` at all and the other three disagreed
with each other (`util/http.rs:1-29`).

The `DEPLOYMENT_ID` is sanitised rather than trusted (`util/http.rs:153-158`): only
`[A-Za-z0-9._-]` survives, truncated at 48 characters. The reason is concrete — the
value reaches both an HTTP `HeaderValue` and a RIPEstat query parameter, and a stray
newline in it would fail client construction and take *every* fetcher down at once.
The function warns both when sanitisation produced nothing and when it changed the
value. `ripestat_sourceapp()` is derived from the same id rather than defaulting
independently (`util/http.rs:125-134`), so setting one variable separates two
deployments everywhere at once.

Two non-goals are stated explicitly in the source. This is not rate-limit management —
credential-based limits need separate credentials and IP-based limits are already
separated by host (`:11-21`). And UA rotation or forged `X-Forwarded-For` headers are
rejected as misrepresentation, with the practical note that they would not help anyway,
since the observed failure is a TCP connect timeout that happens before any header is
sent (`:108-113`).

`client()` is the entire shared configuration:

```rust
// backend/src/util/http.rs:114-116
pub fn client(component: &str) -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent(user_agent(component))
}
```

No shared timeout, no shared retry, no pool tuning. Each caller adds its own `.timeout()`
because the right value genuinely differs by two orders of magnitude across these
sources — 15 seconds for a Cloudflare bulk call, 300 for a Tor CSV.

### 4.4 OONI

Four separate fetchers against one endpoint (`ooni.rs:62`):

```rust
const AGGREGATION_ENDPOINT: &str = "https://api.ooni.io/api/v1/aggregation";
```

On HTTP 429 the client waits `Retry-After` if it parses as a number, else
`3 * 2^attempt` capped at 60 seconds, for at most two retries (`ooni.rs:322-352`).

**Phase A — adoption signals.** One request per URL in a four-entry target list
(`api.openai.com`, `openai.com`, `api.anthropic.com`, `www.deepseek.com`), over a
90-day window, aggregated by `probe_cc`:

```rust
// backend/src/fetchers/ooni.rs:383-441
let query = [
    ("domain", host.as_str()),        // scheme stripped, `www.` KEPT
    ("test_name", "web_connectivity"),
    ("axis_x", "probe_cc"),
    ("since", since.as_str()),        // days_ago_iso(90)
];
```

Classification (`ooni.rs:446-466`) is conservative — a confirmed measurement is
`BLOCKED`, zero anomalies is `ACCESSIBLE`, and anything in between is `INCONCLUSIVE`
rather than a guess. Confidence is a function of sample size: `HIGH` at 10+
measurements, `MEDIUM` at 3+, else `LOW`.

**Phase B — technology blocking.** One request per entry in a 14-entry registry
(`ooni.rs:170-264`): four AI-access domains, six circumvention tools, four messaging
nettests. Circumvention tools that OONI runs as dedicated nettests (`tor`, `psiphon`,
`torsf`) carry no `domain` parameter; the rest are `web_connectivity` with a host.

This is the classification that drives the globe's blooms, quoted in full because it is
the single most consequential threshold set in the tool:

```rust
// backend/src/fetchers/ooni.rs:613-630
if total == 0 { return ("INCONCLUSIVE", 0.0); }
let rate = anomaly as f64 / total as f64;
let status = if rate > 0.7 && total > 10 { "CONFIRMED_BLOCKED" }
    else if rate > 0.4 && total > 5  { "LIKELY_BLOCKED" }
    else if rate < 0.1 && total > 5  { "ACCESSIBLE" }
    else { "INCONCLUSIVE" };
```

Note that sample size is part of the rule at every level, not a footnote attached
afterwards. A 100% anomaly rate over eight measurements is `INCONCLUSIVE`, not
`CONFIRMED_BLOCKED`. And the gaps between the bands are intentional: a rate of 0.3 over
20 measurements matches none of the first three conditions and falls through to
`INCONCLUSIVE`. The rule is built so that the uncertain case is the *default*, reached
by falling through, rather than a branch someone had to remember to write.

`total()` itself is defensive (`ooni.rs:132-138`): it uses `measurement_count` when
positive, and otherwise reconstructs it as `anomaly + confirmed + failure + ok`,
because OONI does not always populate the former.

**Phase C — content categories.** A single two-dimensional request for the entire
globe, over 180 days, with `axis_x = category_code` and `axis_y = probe_cc`. The
thresholds are deliberately different from phase B (`ooni.rs:736-746`):

```rust
if measurement_count < 100 { "INCONCLUSIVE" }
else if anomaly_rate >= 0.20 { "HEAVILY_CENSORED" }
else if anomaly_rate >= 0.05 { "PARTIALLY_CENSORED" }
else { "ACCESSIBLE" }
```

The minimum sample is 100 rather than 5 or 10, and the anomaly thresholds are far
lower. Both differences follow from what is being measured: a *category* aggregates
many sites, so the sample is much larger and a 20% anomaly rate across an entire
category is a much stronger signal than 20% against one domain.

**Phase D — timelines.** One two-dimensional request per technology
(`axis_x = measurement_start_day`, `axis_y = probe_cc`), for ten technologies. The
`since` parameter is computed incrementally:

```rust
// backend/src/fetchers/ooni.rs:855-873 — timeline_since
let newest = SELECT MAX(measurement_date) FROM blocking_timeline WHERE technology = ?1;
match days_between(&newest, &today_iso()) {
    Some(age) => days_ago_iso(age.max(0) + TIMELINE_OVERLAP_DAYS),  // +7d overlap
    None      => TIMELINE_SINCE.to_string(),                        // "2024-01-01"
}
```

An empty table cold-starts from 2024-01-01; a populated one re-fetches from seven days
before its newest row. The overlap exists because OONI's recent days are provisional and
get revised as late measurements arrive — re-fetching a week of them and letting
`INSERT OR REPLACE` overwrite is how those revisions land. This is the slowest operation
in the application; the frontend has explicit handling for it (§9.6).

Each phase collects its per-item failures and ends with an `anyhow::bail!` summarising
them (`ooni.rs:289-297`, `:431-440`, `:596-605`, `:948-957`), so a partially degraded
sweep is recorded as an error in `fetch_runs` rather than reported as success.

### 4.5 IODA

```rust
// backend/src/fetchers/ioda.rs:12-24
const OUTAGES_ENDPOINT: &str = "https://api.ioda.inetintel.cc.gatech.edu/v2/outages/events";
const WINDOW_SECS: i64 = 90 * 24 * 60 * 60;   // 90 days
```

One request per country over the ~235-country sweep list, 300 ms apart, with `from` and
`until` as Unix seconds (`ioda.rs:92-99`). Rows are mapped with
`end_ts = row.start + row.duration`, a `score` defaulting to 0.0 and a `datasource`
defaulting to `"unknown"` (`ioda.rs:106-134`), and each country's rows are written
immediately after its own fetch, so a failure partway through the sweep keeps everything
already retrieved.

### 4.6 Tor Metrics

Three CSV downloads for the whole world — no per-country requests at all
(`tor_metrics.rs:11-19`), each with `start=2024-01-01&end={today}`. Tor's `end` is
inclusive, unlike OONI's `since`, and the source notes the discrepancy at `:30-33`.

Columns are located by name, case-insensitively (`find_column`, `:460`), rather than by
position — the one place in the codebase that does this, and the right call for a
published CSV whose column order is not a stable contract. Empty cells become `None`,
never `0` (`parse_optional_int`, `:469-475`). Only `obfs4`, `snowflake` and `webtunnel`
transports are retained; `meek`, `obfs3` and the `<OR>` pseudo-transports are dropped.

Two derived values:

```rust
// :234
let ratio = bridge_users.unwrap_or(0) as f64 / (relay_users.unwrap_or(0) as f64 + 1.0);
```

```rust
// :477-486 — classify_blocking
(Some(u), Some(l)) if u < l                       => "HIGH_BLOCKING",
(Some(u), Some(l)) if (u as f64) < (l as f64)*1.2 => "MODERATE",
(Some(_), Some(_))                                => "LOW",
_                                                 => "INCONCLUSIVE",
```

The `+ 1.0` in the ratio is a divide-by-zero guard, and it means the ratio is
approximate at very low relay counts — acceptable for a display metric, worth knowing
before citing one. `classify_blocking` compares observed users against `lower`, which
is Tor's *own published anomaly-detection bound*, not a threshold invented here. When
Tor does not publish a bound the answer is `INCONCLUSIVE`, never a guess.

Roughly 150,000 rows are written in a single transaction through one prepared statement
(`:488-527`).

### 4.7 Cloudflare Radar

Two endpoints on different loops.

**Outage annotations** (`cloudflare.rs`) — one bulk request for the globe,
`limit=200&dateRange=52w&format=json`. The `dateRange` parameter is mandatory; without
it the endpoint returns HTTP 400 code 2001, which is recorded at `:8-9`. Each annotation
carries a list of locations and fans out to one `adoption_signals` row per recognised
country. `endDate` is `Option<String>` because ongoing outages send JSON `null`
(`:28-31`) — a small thing, but the type is honest about it rather than defaulting to
"now".

**HTTP version share** (`cloudflare_http.rs`) — a per-country sweep with
`dateRange=14d&aggInterval=1d&normalization=PERCENTAGE`. Series are the literal strings
`"HTTP/1.x"`, `"HTTP/2"`, `"HTTP/3"`, and values arrive as numeric *strings*. A missing
series becomes `None` rather than zero, because Cloudflare omits near-zero versions
rather than reporting them — so "no HTTP/3 series" means "not reported", not "zero
HTTP/3" (`:43-53`).

The rolling 14-day re-fetch is deliberate self-correction (`:17-20`): Cloudflare's most
recent days are provisional, and re-fetching the window each hour lets the revisions
overwrite the provisional values in place.

Both skip cleanly — returning `Ok(())`, not an error — when `CLOUDFLARE_API_TOKEN` is
unset, which is what makes an absent credential render as an empty panel rather than a
failed fetch. The absence is instead reported loudly once, at boot (§8.1).

### 4.8 RIPEstat

```rust
// backend/src/fetchers/ripestat.rs:17-28
const ENDPOINT: &str = "https://stat.ripe.net/data/country-resource-stats/data.json";
const WINDOW_DAYS: i64 = 14;
```

Per-country, `resolution=1d`, no credential, with `sourceapp` derived from the
deployment identity (`:109-115`).

Two details matter here. All six counts are `f64`, not `i64`, because
RIPEstat averages across RIS collector snapshots and genuinely returns fractional
values — the source cites a live example of `v6_prefixes_ris: 49559.5` (`:42-58`).
Typing these as integers would silently truncate every one of them.

And the sentinel handling:

```rust
// :127-129
fn normalize(v) { if v < 0.0 { None } else { Some(v) } }
```

RIPEstat uses `-1` for "not available". Storing that as a count would produce negative
prefix counts on a chart. The `_ris` columns (currently routed, from RIS collectors) and
the `_stats` columns (registered, from RIR delegation) are stored as `routed_*` and
`registered_*` respectively — two genuinely different questions that are easy to
conflate. No ratio between them is precomputed, deliberately (`schema.rs:375-380`).

### 4.9 Internet Society Pulse

One bulk request for all 179 rated countries at the latest quarter, roughly 425 KB
(`pulse.rs:16-19`). The host matters and is documented: `pulse-api.internetsociety.org`,
**not** `pulse.internetsociety.org/api`, because the latter sits behind a Cloudflare
JavaScript interstitial that returns 403 to every non-browser request (`:9-14`).

Pulse publishes 0–1; the fetcher rescales and rounds (`:164-167`):

```rust
scale(raw) = ((raw * 100.0).clamp(0,100) * 10.0).round() / 10.0
```

Bands at 70 / 50 / 35 (`:183-193`). The key is `{country}-ISOC_PULSE` with no quarter,
so re-runs refresh in place.

### 4.10 V-Dem and RSF

Both come from Our World in Data grapher CSVs (`indices.rs:14-15`):

```rust
const VDEM_URL: &str = "https://ourworldindata.org/grapher/freedom-of-expression-index.csv?csvType=full&useColumnShortNames=true";
const RSF_URL:  &str = "https://ourworldindata.org/grapher/press-freedom-index-rsf.csv?csvType=full&useColumnShortNames=true";
```

`csvType=full` returns every country-year. Parsing is positional — `record.get(1)` is
the ISO3 code, `get(2)` the year, `get(3)` the value (`:286-298`) — with rows skipped
for an empty ISO3 (OWID's regional and World aggregates), an unmapped code (its
`OWID_*` pseudo-codes), an unparseable year, or a blank value.

Year selection keeps the **maximum year per country**, not a fixed year
(`indices.rs:300-307`). This is the right choice for a tool that wants each country's
most recent assessment, and it has a consequence worth stating plainly: two countries'
scores may be from different years. The year is carried through to the API so a consumer
can see that.

Normalisation is where the two sources diverge (`indices.rs:118-140`):

```rust
// V-Dem freedom-of-expression estimate is already 0–1, higher = freer
Source::Vdem => { let score = (raw * 100.0).clamp(0.0, 100.0); ... }

// OWID's RSF series is the pre-2022 index: higher = LESS free -> invert
Source::Rsf  => { let inverted = (100.0 - raw).clamp(0.0, 100.0);
                  classification: rsf_band(raw) }   // band from the RAW value
```

The RSF inversion is the single most error-prone step in the ingestion layer, and the
code does something subtle and correct with it: the *score* is inverted so it matches
the tool's "higher = freer" convention, but the *band label* is computed from the raw
value using RSF's own published thresholds (`rsf_band`, `:158-171`: <15 Good,
<25 Satisfactory, <35 Problematic, <55 Difficult, else Very serious). Inverting the
score and then re-deriving the band from the inverted number would have produced labels
that disagree with RSF's own.

**The V-Dem censorship sub-scores do not come from OWID.** They are read from a
committed extract of V-Dem Country-Year Full+Others v16 (`indices.rs:19-38`):

```rust
const DSP_SEED_FILE: &str = "vdem_dsp_v16.csv";
const DSP_VARS: [(&str, f64); 3] = [
    ("v2smgovfilprc", 4.0),   // government internet filtering in practice   (0–4)
    ("v2smgovshut",   4.0),   // government internet shutdown in practice    (0–4)
    ("v2mecenefi",    3.0),   // internet censorship effort (V-Dem core Media) (0–3)
];
```

The three different denominators are load-bearing: `v2mecenefi` is a 0–3 variable, and
normalising it against 4 would understate every country's score by a quarter. None of
the three is inverted, because all three are already coded higher-is-freer, matching the
tool's convention (`:28-33`).

For each variable the fetcher reads three columns by name — `{var}_osp`,
`{var}_osp_codelow`, `{var}_osp_codehigh` (`:201-208`) — carrying V-Dem's credible
interval alongside the point estimate, all rescaled to 0–100 identically. Countries in
the V-Dem file with no ISO 3166-1 landing spot (Somaliland, Zanzibar, the Republic of
Vietnam) are counted and printed, not silently dropped (`:241-250`).

The write is one `INSERT OR REPLACE` covering all nineteen columns, and the reason is
recorded at `:350-352`: `INSERT OR REPLACE` deletes and re-inserts the row, so the
natural-looking pattern of inserting the headline score and then `UPDATE`-ing the
sub-scores would null those sub-scores on every cycle.

One failure-handling note, flagged here because it is the layer's one real
inconsistency: V-Dem and RSF each fetch inside their own `if let Err` and
`fetch_and_store` still returns `Ok(())` (`:80-87`). So `indices` can report `ok` to
`fetch_runs` while one of its two sources failed. Every other fetcher bails on partial
failure. See §13.

---

## 5. The censorship index, as implemented

This is the one number in the tool that BLACKOUT constructs rather than reports. It is
computed on the fly per request from `country_scores`, and nothing about it is cached
or materialised.

The inputs are two columns and nothing else (`api/censorship_index.rs:43-48`):

```sql
SELECT country_code, source, score_overall FROM country_scores
WHERE source IN ('V_DEM','RSF') AND score_overall IS NOT NULL
```

Note what is *not* an input: the ISOC Pulse rows, and all nine `score_vdem_*`
sub-scores. The V-Dem internet-filtering and shutdown sub-scores are displayed in the
sidebar but do not feed the composite.

The weights:

```rust
// backend/src/api/censorship_index.rs:12-13
const W_VDEM: f64 = 0.5;
const W_RSF: f64 = 0.3;
```

And the computation in full (`censorship_index.rs:81-107`):

```rust
let mut weighted = 0.0; let mut weight = 0.0; let mut count = 0i64;
for (value, w) in [(c.vdem, W_VDEM), (c.rsf, W_RSF)] {
    if let Some(v) = value { weighted += v * w; weight += w; count += 1; }
}
if weight == 0.0 { return None; }

let freedom = (weighted / weight).clamp(0.0, 100.0);
let round1 = |x: f64| (x * 10.0).round() / 10.0;

censorship_score: round1(100.0 - freedom),
freedom_score:    round1(freedom),
```

### 5.1 What the arithmetic actually does

Because the accumulated weight is divided out, **the weights renormalise over whichever
components are present**. That produces three cases:

| Components | Result |
| --- | --- |
| Both | `freedom = (0.5·V + 0.3·R) / 0.8` = `0.625·V + 0.375·R` |
| V-Dem only | `freedom = 0.5·V / 0.5 = V` exactly |
| RSF only | `freedom = 0.3·R / 0.3 = R` exactly |
| Neither | `None` — the country is omitted from the response entirely |

Two consequences follow that are easy to miss from the constants alone.

**Only the 5:3 ratio is meaningful.** That `W_VDEM + W_RSF = 0.8` rather than 1.0 is
inert; writing `0.625` and `0.375`, or `5.0` and `3.0`, would produce identical output
for every country. The absolute values carry no information.

**A single-component country scores exactly its own input.** A country with V-Dem but no
RSF gets its V-Dem score unchanged, not a score discounted by the missing component.
This is the right behaviour — the alternative would systematically rank
sparsely-covered countries as freer — but it means two scores are not directly
comparable unless they have the same `component_count`. The response returns that count
precisely so a consumer can check (`censorship_index.rs:30`), alongside the raw,
*unrounded* `vdem` and `rsf` values, so the composite can always be recomputed from
what the API itself returned.

**A country with neither component is dropped, never zeroed.** This is the "absence is
not zero" invariant appearing in code: `compute` returns `Option`, and `filter_map`
discards the `None`s (`:71-74`). A country with no freedom data does not appear in the
choropleth at all, rather than appearing as maximally free or maximally censored.

The clamp is applied to `freedom` *before* the inversion, so
`censorship_score ∈ [0, 100]` by construction rather than by luck. Results sort
most-censored first (`:76`), which is a stable and useful default for any consumer.

### 5.2 Orientation

Every score in the tool is normalised to "higher = more free" at ingest, and the
inversion to censorship happens once, here, at the last moment before display. The RSF
series from OWID is the pre-2022 index where higher means *less* free, so it is inverted
during ingestion (§4.10) — and, as noted there, its band label is computed from the raw
value so the labels still agree with RSF's own published thresholds.

### 5.3 What `/api/rankings` is not

`api/rankings.rs` looks adjacent but computes no composite. It is a single-source
leaderboard (`rankings.rs:53-61`) selecting `score_overall` for one `source` —
defaulting to `V_DEM` — joined to `country_reference` for the name and region.

One note on SQL construction: `order` is string-
interpolated into the SQL rather than bound, and is therefore whitelisted to exactly
`DESC` or `ASC` before use (`:47-50`), defaulting to `ASC` so the least-free country
appears first. `limit` is bound normally and clamped to `1..=500` (`:51`). Every other
parameter in the codebase is bound.

---

## 6. The satellite subsystem

The most technically involved part of the system, and the one with the most non-obvious
correctness requirements. It has three distinct jobs: acquire orbital element sets from
two providers that disagree; hold them as durable state that can only ever improve; and
propagate them to positions on demand, cheaply enough for a 7-second poll on two vCPUs.

### 6.1 Propagation

BLACKOUT does not implement SGP4. It uses the `sgp4` crate, version 2.4.0, and the
source is explicit that the boundary matters:

> Neither of these is orbital propagation — the propagation itself is entirely
> `sgp4::Constants::propagate`, called once per sample here.
> — `backend/src/satellites/orbit.rs:2-8`

Two properties of the crate's configuration deserve attention, because
they are chosen by which constructor is called rather than by any local constant.
`Constants::from_elements` selects the **WGS84 geopotential and the IAU sidereal-time
expression**, not the AFSPC/WGS72 compatibility path that
`from_elements_afspc_compatibility_mode` would give. And near-earth SGP4 versus
deep-space SDP4 is chosen by the crate itself on mean motion, so geostationary and
Molniya objects are handled by the resonance integrator without any special-casing here.

Element sets arrive by two different routes into the same type. CelesTrak is requested
as OMM JSON and deserialised straight into `Vec<sgp4::Elements>`
(`fetchers/satellites.rs:986`). SatNOGS publishes classic three-line TLEs as JSON
fields, parsed with `Elements::from_tle` after stripping the `"0 "` name-line prefix
(`:1014-1019`); a malformed row is skipped and counted, never fatal.

Critically, **every element set is validated before it is allowed to compete**:

```rust
// backend/src/fetchers/satellites.rs:349
if sgp4::Constants::from_elements(&el).is_err() { /* rejected */ }
```

A fresher-but-unusable record therefore cannot displace an older working one — which
matters because the merge rule in §6.3 is otherwise driven purely by epoch, and would
happily accept newer garbage.

### 6.2 Frames and the geodetic conversion

`sgp4::Constants::propagate` returns a position in the **TEME** frame (True Equator,
Mean Equinox) in kilometres, and stops there. Turning that into a latitude, longitude
and altitude takes two further steps, both implemented locally in
`backend/src/satellites/geodetic.rs`.

The time argument is signed minutes from the element set's own epoch
(`geodetic.rs:33-35`), so propagation backwards is as valid as forwards:

```rust
pub fn minutes_since_epoch(elements: &sgp4::Elements, at: DateTime<Utc>) -> f64 {
    (at.naive_utc() - elements.datetime).num_milliseconds() as f64 / 60_000.0
}
```

**TEME → ECEF** is a pure rotation about Z by Greenwich Mean Sidereal Time
(`geodetic.rs:43-54`):

```rust
pub fn teme_to_geodetic(position_teme_km: [f64; 3], at: DateTime<Utc>) -> (f64, f64, f64) {
    let years_since_j2000 = sgp4::julian_years_since_j2000(&at.naive_utc());
    let gmst_rad = sgp4::iau_epoch_to_sidereal_time(years_since_j2000);
    let (sin_gmst, cos_gmst) = gmst_rad.sin_cos();

    let [x, y, z] = position_teme_km;
    let x_ecef =  x * cos_gmst + y * sin_gmst;
    let y_ecef = -x * sin_gmst + y * cos_gmst;
    let z_ecef = z;

    ecef_to_geodetic(x_ecef, y_ecef, z_ecef)
}
```

The GMST expression is the crate's, not a local reimplementation — the IAU-82 /
Vallado polynomial in seconds of time, converted to radians. Using the crate's function
rather than writing one is the correct choice here for a reason beyond convenience: it
is the *same* sidereal-time function the propagator was constructed with, so the frame
and the rotation cannot drift apart.

What this conversion deliberately omits is stated in the source:

> this ignores polar motion and nutation/precession corrections beyond what TEME already
> bakes in, which is standard practice for SGP4-based tracking (SGP4 itself is only
> accurate to within about a kilometer, so sub-arcsecond frame corrections wouldn't
> survive it anyway).
> — `geodetic.rs:37-42`

That is the right call, and stating the error budget is what makes it checkable.

**ECEF → geodetic** is a Bowring-style fixed-point iteration on the WGS-84 ellipsoid
(`geodetic.rs:56-74`), with the flattening and semi-major axis as the only hardcoded
constants and the eccentricity derived rather than duplicated:

```rust
const WGS84_A_KM: f64 = 6378.137;
const WGS84_F: f64 = 1.0 / 298.257223563;

fn ecef_to_geodetic(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let p = (x * x + y * y).sqrt();
    let lon = y.atan2(x);

    let mut lat = z.atan2(p * (1.0 - e2));
    let mut alt = 0.0;
    for _ in 0..6 {
        let sin_lat = lat.sin();
        let n = WGS84_A_KM / (1.0 - e2 * sin_lat * sin_lat).sqrt();
        alt = p / lat.cos() - n;
        lat = z.atan2(p * (1.0 - e2 * n / (n + alt)));
    }

    (lat.to_degrees(), lon.to_degrees(), alt)
}
```

Six iterations, no convergence test — justified on the same error-budget grounds, and
cheaper than testing. Units are kilometres throughout. The `alt = p / cos(lat) − N`
form is the classic Bowring altitude expression and is ill-conditioned near the poles,
where `p → 0` and `cos(lat) → 0`; there is no polar special case. See §13.

Both steps are tested: the ISS fixture from the `sgp4` crate's own examples propagated
at epoch into a plausible LEO band, and an analytic round-trip of a point on the equator
at 500 km agreeing to within 1e-6 (`geodetic.rs:107-146`).

### 6.3 Acquisition and the merge rule

Three endpoints (`fetchers/satellites.rs:35-56`):

| Constant | URL | Query |
| --- | --- | --- |
| `GP_ENDPOINT` | `https://celestrak.org/NORAD/elements/gp.php` | `GROUP=<group>&FORMAT=JSON` |
| `SUPPLEMENTAL_ENDPOINT` | `https://celestrak.org/NORAD/elements/supplemental/sup-gp.php` | `FILE=starlink&FORMAT=JSON` |
| `SATNOGS_ENDPOINT` | `https://db.satnogs.org/api/tle/?format=json` | — |

`FORMAT` is passed explicitly because CSV became CelesTrak's default on 2026-05-09
(`:32-34`) — the kind of upstream change that silently breaks a parser, recorded in the
code so the next reader knows the parameter is load-bearing.

The canonical store is `satellite_catalog`, keyed on **`norad_id` alone**
(`schema.rs:461-473`). Membership belongs to that table, not to any upstream response,
and there is **no `DELETE` anywhere in the merge path** (`satellite_catalog.rs:112-113`).
That single fact is the anti-shrinkage guarantee: no truncated, partial, or failed
response can remove a satellite, because no code exists that could.

A refresh is an upsert, and whether an incoming record wins is decided by one function:

```rust
// backend/src/db/satellite_catalog.rs:77-102
fn is_improvement(incoming_epoch, incoming_rank, current: &ExistingMeta) -> bool {
    match incoming_epoch.cmp(&current.epoch) {
        Ordering::Greater => true,
        Ordering::Equal   => incoming_rank < current.source_rank,
        Ordering::Less    => false,
    }
}
```

The primary criterion is the **element set's own epoch** — upstream's determination
time — not the time it was fetched. Ties break on source rank:
`CelestrakSupplemental (0) < Celestrak (1) < Satnogs (2)` (`satellites/mod.rs:76-82`).
The two providers are complementary sources merged into one catalogue, not alternatives.

Category is upgraded on a separate axis from elements, in both update paths:

```sql
category      = CASE WHEN ?8 < category_rank THEN ?9 ELSE category END,
category_rank = MIN(category_rank, ?8)
```

so a SatNOGS record can supply fresher orbital elements without demoting a satellite's
CelesTrak-derived category. SatNOGS records carry `CATEGORY_RANK_UNKNOWN = 1000`
(`satellites/mod.rs:123`), guaranteed worse than any group index, so they can never win
that comparison.

The whole merge is one transaction, and `last_successful_refresh` is stamped only if
something actually changed (`satellite_catalog.rs:275-278`). Afterwards the in-memory
catalogue is reloaded **from SQLite**, never assembled from the fetch response
(`fetchers/satellites.rs:369-428`) — so memory can never hold less than disk.

Categories come from the group list, whose **order is the precedence**
(`fetchers/satellites.rs:142-154`):

```rust
("starlink","starlink"), ("gps-ops","navigation"), ("galileo","navigation"),
("glo-ops","navigation"), ("beidou","navigation"), ("military","military"),
("resource","earthobs"), ("stations","stations"), ("science","stations"),
("geo","geo"), ("active","other"),
```

Overridable wholesale via `SATELLITE_GROUPS`, falling back to these defaults if unset or
unparseable. Note there is no OneWeb or Iridium group by default — the source names them
as a configuration-only possibility (`:138-141`).

### 6.4 Reliability machinery

A single CelesTrak outage should not cost the catalogue, so this fetcher carries more
defensive machinery than any other:

- **Retry**: `MAX_ATTEMPTS = 3`, base delay 2 s, exponential with up to +50% jitter
  (`:74-75`, `:236-272`). Only `Transient` errors retry.
- **403 disambiguation** (`:970-987`), the correctness-critical piece. A 403 whose body
  contains `"has not updated since"` or `"no sooner than"` is CelesTrak saying *you
  already have this* — a success, not retried. Any other 403 is a genuine transient
  failure. Conflating the two is not hypothetical: it once logged a total outage as
  "refresh complete" (`:956-969`).
- **Circuit breaker**: three consecutive group failures abandon the remaining groups
  *and* the supplemental feed for that cycle (`:93`, `:522-550`), bounding a full
  outage's cost. A test pins the worst case at ≤300 s.
- **Pacing**: 500 ms between group requests.
- **Empty-catalogue retry**: while the catalogue is completely empty the loop retries
  every 180 s instead of waiting the full 2 hours (`:112`, `:816-817`).
- **Deferred first refresh**: on boot the persisted catalogue is loaded and served
  *before* any network call, and a refresh only runs if the catalogue is actually due —
  so repeated redeploys cost zero upstream requests (`:745-818`).

The shrinkage guard is quoted in full, because its behaviour is subtler than
"reject small responses":

```rust
// backend/src/fetchers/satellites.rs:305-329
fn is_suspiciously_partial(received: usize, previous: usize, min_ratio: f64) -> bool {
    if previous == 0 { return false; }   // first-ever fetch sets the baseline
    if received == 0 { return true; }    // empty for a group with members is always suspect
    (received as f64) < (previous as f64) * min_ratio
}
```

`previous` is a persisted per-group high-water mark. When a response is judged partial
it **is still merged** — records can only ever improve rows, so there is no reason to
discard good data — but it is *denied authority*: its size does not become the new
baseline (`:486-501`), and baselines are only committed after a successful merge
(`:666-672`). The distinction between "use this data" and "trust this data as a measure
of how much data there should be" is the whole idea.

### 6.5 Two clocks

The subsystem tracks freshness and usability as separate questions, and conflating them
is the mistake the design exists to avoid.

| Concept | Default | Measured against | Effect |
| --- | --- | --- | --- |
| `stale_after_hours` | 24 h | `last_updated` — when a better record was last accepted | annotates `stale: true`; still rendered |
| `max_propagation_age_days` | 30 d | `epoch` — the element set's own age | **excluded from propagation entirely** |

These come apart because SatNOGS republishes long-dead objects. The motivating evidence
is recorded at `satellites/mod.rs:173-198`: 221 objects with epochs older than 30 days,
the oldest from **February 1975**, every one of them reporting `fresh` because
`last_updated` was minutes old. SGP4 given such an element set either diverges outright
or — far worse — succeeds and returns a position thousands of kilometres wrong.

The regression test uses the real observed case: ONDOSAT-OWL-9, accepted twenty minutes
ago, carrying an epoch 642 days old, still refused
(`mod.rs:233-250`). Nothing is ever deleted for staleness; an unpropagatable object stays
in the catalogue, is counted in `total`, and is simply left out of the position response.

### 6.6 Serving positions

`GET /api/satellites` takes `Extension<SatelliteCatalog>` — an `Arc<RwLock<…>>` — and
deliberately **not** `State<AppState>`. The hot, 7-second-polled endpoint is kept
entirely outside the single SQLite mutex so that polling cannot serialise unrelated
routes (`satellites/mod.rs:1-19`, `main.rs:193-198`).

Per request, the handler propagates every matching object in a plain serial loop
(`api/satellites.rs:233`). There is no parallelism and no per-object caching. Two
filters run *before* propagation so that skipped objects cost nothing: the category
filter (`:217-221`) and the epoch cutoff (`:230-232`). `sgp4::Constants` are derived once
at catalogue-load time, so a request pays only for `propagate`.

Three optimisations make that affordable, each with its measurement in the source:

**Response cache.** A 1000 ms TTL (`SATELLITE_POSITION_CACHE_MS`, `0` disables), keyed on
the category filter *sorted and comma-joined* so `a,b` and `b,a` share an entry
(`:184-192`). The entry holds the raw JSON **and a pre-gzipped copy**, because once
propagation was memoised, compression was 28 ms of a 40 ms request (`:63-74`).
`encoded_response` picks the variant from `Accept-Encoding` and always sets
`Vary: accept-encoding`; `tower-http`'s `CompressionLayer` then leaves the body alone
because `Content-Encoding` is already set. The map is bounded at 32 keys and cleared
wholesale on overflow. The lock is explicitly not held across the sweep, so two
concurrent misses may both compute — accepted deliberately (`:195-197`, `:299-301`).

**Coordinate rounding** (`:127-144`): 4 decimal places on latitude and longitude
(≈11 m), 3 on altitude (≈1 m). Measured effect, cited at `:135-137`: 2.25 → 1.75 MiB
raw, 0.65 → 0.36 MiB gzipped, and 39% less compression CPU.

**Cheap absence.** `stale` is `#[serde(skip_serializing_if = "std::ops::Not::not")]`
(`models/satellite.rs:22`), so the common `false` costs no bytes at all across thousands
of objects.

The response reports `total`, `fresh_count`, `stale_count` and `unpropagatable_count`,
with the documented invariant that the last three sum to the first
(`models/satellite.rs:36-39`) — so a consumer can always tell how much of the catalogue
is being withheld and why. `category_counts` is computed catalogue-wide and deliberately
unaffected by the request's filter (`:155-161`), so the legend does not change as you
toggle layers.

`GET /api/satellites/:norad_id/orbit` samples one full period centred on *now*, 180
points (`orbit.rs:28-46`), splitting the path wherever consecutive longitudes jump more
than 180° (`orbit.rs:52-68`). It is uncached, so it costs 180 propagations. Its error
handling is the principle in miniature: a NORAD ID absent from the catalogue is `404`,
but one present with an epoch past the cutoff is **`422 Unprocessable Entity`**
(`:384-386`) rather than 180 points of fiction.

`GET /api/satellites/status` is the only satellite handler taking both extractors,
because half of what it reports — refresh history — survives restarts only by living in
SQLite, while the other half describes what is being served from memory (`:311-317`).

---

## 7. The HTTP surface

One `axum::Router`, built in `main.rs:154-222`. **Every mounted route is a GET.** The
deployment is public, unauthenticated and read-only, and that is a structural property
rather than a policy: there is no mounted handler that writes.

| Path | Handler | Cost |
| --- | --- | --- |
| `/health` | `health::health` | one aggregate query |
| `/api/countries` | `countries::list_countries` | 5 rows |
| `/api/countries/:code` | `countries::get_country` | 1 row, 404 meaningful |
| `/api/geo` | `geo::list_geo` | 235 rows (249 with `?all`) |
| `/api/starlink-status` | `starlink_status::list_starlink_status` | 25 rows |
| `/api/cables` | `cables::list_cables` | 728 routes + 1,925 points |
| `/api/ixp-stats` | `ixp::list_ixp_stats` | 170 rows |
| `/api/models` | `models::list_models` | 8 rows, unused by the UI |
| `/api/signals` | `signals::list_signals` | 6 rows, unused by the UI |
| `/api/blocking` | `blocking::list_blocking` | filtered by country and layer |
| `/api/categories` | `categories::list_categories` | per country |
| `/api/timeline` | `timeline::list_timeline` | per country and technology |
| `/api/tor-metrics` | `tor_metrics::list_tor_metrics` | per country |
| `/api/outages` | `outages::list_outages` | per country, or active globally |
| `/api/rankings` | `rankings::list_rankings` | single-source leaderboard |
| `/api/censorship-index` | `censorship_index::list_censorship_index` | computed per request |
| `/api/country-scores` | `country_scores::list_country_scores` | per country |
| `/api/http-protocol-share` | `http_protocol_share::…` | 14-day series |
| `/api/bgp-visibility` | `bgp_visibility::…` | 14-day series |
| `/api/satellites` | `satellites::list_satellites` | **propagation, cached 1 s** |
| `/api/satellites/status` | `satellites::satellites_status` | catalogue tallies |
| `/api/satellites/:norad_id/orbit` | `satellites::satellite_orbit` | **180 propagations, uncached** |
| `/api/methodology` | `methodology::list_methodology` | document chooser metadata |
| `/api/methodology/:slug` | `methodology::get_methodology` | one rendered document |

The last two serve the methodology documents. The mechanism is unusual enough to note:
the Markdown is compiled into the binary with `include_str!` rather than read from disk
at runtime (`api/methodology.rs:7-12`), so a renamed or missing file is a build error
naming the path rather than a 404 in production. `comrak` renders it to HTML, and each
document's table of contents and reading time are derived from its own `##` and `###`
headings.

Two routing decisions are deliberate and documented.

`/api/geo` is mounted at its own path rather than under `/api/countries/...`
specifically so it cannot be shadowed by the dynamic `/api/countries/:code` route
(`api/geo.rs:21-22`). The two endpoints answer genuinely different questions — the
whole-world drawable list versus the five researched dossiers — and colliding them
behind one prefix would have made the collision a routing accident rather than a
design.

`route_layer(Extension(satellite_catalog))` at `main.rs:198` scopes the in-memory
catalogue to the three satellite routes above it, and specifically **not** to the SPA
fallback below.

### 7.1 Middleware

The stack is deliberately thin.

`CompressionLayer::new()` is applied **last**, so it wraps every route *and* the static
fallback (`main.rs:211-221`). The reason is in the comment: the bundle is 13.7 MB, most
of it Cesium, and was being served uncompressed to every first-time visitor. The
measured effect on the hot endpoint is recorded inline — `/api/satellites` 2.36 MB →
0.62 MB, polled every 7 s per open tab, which dominates egress. The default predicate
skips bodies under 32 bytes and already-compressed content types, so JPEG textures are
not recompressed for nothing.

The choice of gzip over brotli is argued in `Cargo.toml:12-16`: brotli compresses
better, but its cost on a 2.3 MB body is far less predictable, and this server has two
vCPUs shared with the SGP4 propagation that produces that body. gzip measured 3.5× and
is universally supported.

**There is no CORS layer**, on purpose: the browser loads the app and calls the API from
the same origin, so there is no cross-origin request to permit (`main.rs:152-153`).

**There is no tracing layer.** No `TraceLayer`, and the `tracing` crate is not a
dependency. All observability is `println!` and `eprintln!` with a `WARNING:` prefix
convention. This is a real limitation at scale and an adequate fit for the current one, and
§13 returns to it.

The SPA fallback is `ServeDir` with an `index.html` fallback (`main.rs:297-299`), so a
refresh on a deep link returns the app rather than a 404.

---

## 8. Durability

This section is the clearest example of the fourth constraint from §2 driving design.
The module header names exactly what is at stake (`snapshot/mod.rs:1-14`): three tables
accumulating history inside rolling windows that upstream will never re-serve —
`outage_events` (IODA, 90 days), `bgp_prefix_visibility` (RIPEstat, 14 days),
`http_protocol_share` (Cloudflare, 14 days) — plus a satellite catalogue that cannot be
rebuilt on demand at all.

Replit rebuilds a published app's filesystem on every publish. Without this layer, every
deploy is data loss.

### 8.1 Boot sequence

The order in `main.rs` is load-bearing at three separate points.

| # | Step | Line |
| ---: | --- | --- |
| 1 | `dotenvy::dotenv().ok()` — absence non-fatal | 30 |
| 2 | Resolve `DATABASE_PATH`, default `mena_ai.db` | 34-38 |
| 3 | `create_dir_all` on the parent directory | 45-51 |
| 4 | **Snapshot restore** → `Option<Snapshotter>` | 58 |
| 5 | `db_existed = Path::exists()` — captured before open | 63 |
| 6 | `Connection::open` | 65-67 |
| 7 | `init_schema`: tables → migrations → seed → invariant | 68 |
| 8 | `warn_missing_optional_tokens()` | 70 |
| 9 | `report_identity()` | 71 |
| 10 | Build `AppState`; `report_persistence` | 73-74 |
| 11 | Resolve `STATIC_DIR`, warn if no `index.html` | 79-94 |
| 12 | Spawn fetch, satellite, precision loops | 100-124 |
| 13 | Spawn snapshot loop + shutdown hook, **only if `Some`** | 130-143 |
| 14 | Build router, bind, serve | 154-233 |

Step 3 exists because `DATABASE_PATH` points at a filesystem Replit rebuilds from
scratch, and `Connection::open` does not create parent directories — without it the
process fails with "unable to open database file" *before* the restore has anywhere to
put anything (`main.rs:40-44`).

Step 4 must precede steps 5 and 6, and the comment says why: `exists()` has to see the
restored file, and `Connection::open` would otherwise create an empty one (`:53-57`).

Step 5 must precede step 6 for a related reason — after `open`, a restored database and
a brand-new one are indistinguishable, and that distinction is the entire point of
`report_persistence` (`:60-62`).

Step 8 exists because a missing optional token used to surface only as one `eprintln!`
from deep inside a background task, making a forgotten `PULSE_API_TOKEN` look identical
to "Pulse has no data for these countries" (`main.rs:302-304`). It now names each
missing key and the data it gates, once, loudly, at boot.

### 8.2 The central invariant

> A failed restore must never be allowed to overwrite a good snapshot. It is prevented
> **structurally rather than by checking**: a restore that *errors* returns no
> `Snapshotter` at all, so no upload task is ever spawned and there is no code path that
> could write.
> — `snapshot/mod.rs:21-31`

The invariant is enforced by
the type: `restore_if_absent` returns `Option<Snapshotter>`, and `main.rs:130` spawns
the upload tasks only inside `if let Some(...)`. There is no flag to check, no branch to
forget, and no way to add a write path later without first obtaining a `Snapshotter`
that a failed restore never produces.

The outcomes are a four-way decision, not a boolean (`mod.rs:137-195`):

| Situation | Result | Uploads |
| --- | --- | --- |
| A database is already on disk | keep local, no download | enabled |
| Download succeeded | install, seed shrinkage baseline | enabled |
| Install failed | warn loudly | **disabled** |
| Clean 404 — nothing stored yet | treat as first deployment | enabled |
| Transfer errored | warn loudly | **disabled** |

The 404-versus-error distinction is the one that makes this usable. A clean 404 is the
expected answer on a first-ever deployment and is deliberately *not* treated as a
failure (`object_store.rs:204`, `mod.rs:177-184`), while any other non-success status
is (`object_store.rs:207-236`). Collapsing them would either block the first deploy from
ever saving anything, or allow a transport failure to overwrite good data — the two
failure modes this design exists to separate.

### 8.3 Producing a snapshot

```rust
// backend/src/snapshot/mod.rs:344
conn.execute("VACUUM INTO ?1", ...)
```

`VACUUM INTO` is chosen over a file copy because it takes a read transaction, so the
result is a consistent image including anything still in the WAL, and it is defragmented
on the way out (`mod.rs:320-326`). The output is gzipped with `flate2` at default
compression, giving roughly a quarter of the live file's size.

The cost is documented rather than glossed: it holds the `AppState` mutex for one to two
seconds at 70 MB, with rusqlite's incremental `backup` named as the successor. The whole
operation runs under `tokio::task::spawn_blocking` (`mod.rs:288`) so it never occupies a
runtime worker, and a `tokio::sync::Mutex` gate (`mod.rs:91`, `:280`) prevents the
interval loop and the shutdown hook from both vacuuming to the same path.

A stale temp file is removed first, because `VACUUM INTO` refuses to write to a path
that already exists (`mod.rs:330-335`) — a small detail that turns a recurring failure
into a non-event.

### 8.4 Installing a restored snapshot

```rust
// backend/src/snapshot/mod.rs:75
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
```

Verified after decompression and **before** install (`mod.rs:376-382`), so a truncated
or wrong object is caught here rather than surfacing later as "file is not a database"
from `Connection::open`. The file is then written to a `.restore` temp path, `sync_all`'d,
and `rename`d into place (`:384-392`), so a failure part-way through can never leave a
half-written file where the database belongs.

### 8.5 Two guards and a fingerprint

**Shrinkage.** A snapshot whose uncompressed size has collapsed below
`SNAPSHOT_MIN_RETAIN_RATIO` (default 0.5, clamped to `[0,1]`) of the last good one is
refused (`mod.rs:292-310`). It returns `Ok(None)` — not an error — and logs both the
percentage and the remediation. The baseline is an `AtomicU64` shared across clones "so
the guard sees one history, not one per task" (`mod.rs:85-88`). This is the same
reasoning as the satellite catalogue's guard in §6.4, applied to a different asset.

**Dirty tracking.** Every interval iteration first compares a four-part fingerprint —
main file length and mtime, WAL length and mtime (`mod.rs:415-432`) — and skips the
snapshot entirely if nothing has changed. The choice of the filesystem over SQLite is
explained precisely:

> rusqlite 0.31 exposes `changes()` (rows touched by the last statement) but not a
> cumulative counter, and `PRAGMA data_version` only moves for writes made by *other*
> connections — useless here, where there is exactly one. In WAL mode every committed
> write extends or rewrites `-wal`, so this moves whenever the database does, needs no
> lock, and costs two `stat` calls.
> — `mod.rs:406-414`

And one detail that is easy to get wrong: a shrinkage-guard refusal deliberately does
**not** advance the stored fingerprint, so the next cycle retries rather than concluding
nothing has changed (`mod.rs:232-234`).

### 8.6 Cadence and shutdown

The interval defaults to 180 minutes with a hard floor of 300 seconds — "a typo should
make this slow, never a tight loop uploading a 70 MB object" (`mod.rs:64`). The loop
sleeps *first*, since there is nothing new to save at boot.

`run_shutdown_hook` (`mod.rs:246-275`) selects over SIGTERM and SIGINT, takes one final
snapshot, and exits. The division of labour is stated at `main.rs:137-138`: the interval
bounds only what an unplanned crash costs, while the shutdown snapshot is what makes an
ordinary redeploy lossless. If the platform's grace period expires mid-upload the
process is killed, but GCS object writes are atomic, so the previous snapshot survives
whole (`mod.rs:242-245`).

### 8.7 Sidecar discovery, and the incident that shaped it

Replit exposes object storage through a localhost sidecar. Discovering it used to be a
single probe, and the post-mortem is in the source:

> A single attempt was wrong, and it cost a full deployment. The app probed 2.8s after
> the container started, got a connection refused, and concluded 'not on Replit' for the
> life of the process — so it cold-started and never restored, while the very same
> endpoint answered fine from the workspace shell. Nothing orders the sidecar's startup
> against the app's.
> — `snapshot/object_store.rs:39-52`

The fix retries for up to 10 seconds with exponential backoff from 400 ms capped at 3 s,
bounded tightly "because this runs before the HTTP listener binds and every second here
is a second of failing platform health checks" (`:47-49`). On giving up it now names the
error, because "'Connection refused' (nothing is listening) and 'operation timed out'
(something is, but wedged) call for completely different responses, and they used to be
logged identically — as nothing at all" (`:131-141`).

A sidecar that answers but reports no bucket is a hard error rather than `None`
(`:99-103`) — that is a configuration mistake worth failing loudly on, not an absence.

### 8.8 Making a silent failure loud

The most damaging failure mode in this deployment is silent: if the database was not
restored, every redeploy starts empty, and the plain `DB at <path>` line looks identical
either way.

`report_persistence` (`main.rs:250-293`) solves this with a monotonic boot counter stored
in `satellite_refresh_state`:

> it can only be absent on a database no process has ever booted against. A second boot
> reporting #1 means the file this process opened is not the file the last one wrote.
> — `main.rs:246-249`

A healthy redeploy prints `DB restored: ... (boot #N)` with `N > 1`. Anything else emits
a `WARNING` block whose remediation text differs depending on whether `SNAPSHOT_KEY` is
set — because a local `cargo run` legitimately has no snapshot and should not be told
off for it (`:277-292`).

---

## 9. The frontend

React 18 and CesiumJS, built by Vite, served as static files by the same Rust binary.
4,812 lines across 32 files. `cesium ^1.121`, `react ^18.3`, `recharts ^2.12`,
`topojson-client ^3.1`, `world-atlas ^2.0`.

`vite-plugin-cesium` is not optional: it copies Cesium's workers, `Assets/` and
`Widgets/` into the bundle and sets `CESIUM_BASE_URL`. Without it the globe renders
blank even though the CSS import resolves (`vite.config.js:6-9`). The dev server proxies
both `/api` and `/health` to port 3001 — `/health` separately, because the footer's
data-age readout reads it.

### 9.1 Viewer construction

The viewer is created with every widget disabled (`Globe.jsx:252-274`). Two settings
are correctness-relevant rather than cosmetic, and both are annotated:

`baseLayer: false` — **not** the pre-1.107 `imageryProvider: false`, which Cesium
silently ignores while falling back to Ion World Imagery (`:253-256`). The older spelling
would appear to work and quietly introduce a network dependency on Ion.

`skyBox.show = false` is set *after* construction rather than as a constructor option,
because the constructor's `skyBox: false` also leaves `scene.skyAtmosphere` undefined
(`:302-307`) — and the atmosphere is wanted.

An Ion token is read from `VITE_CESIUM_ION_TOKEN` if present and is harmless when unset,
since no Ion asset is ever requested.

### 9.2 Geometry and the join

The basemap is `world-atlas/countries-50m.json`, imported dynamically and converted with
`topojson.feature` (`Globe.jsx:319-321`).

The join between that geometry and BLACKOUT's data is the part worth checking. TopoJSON
features carry an ISO 3166-1 **numeric** id and no alpha-2 code, while every table in
the database keys on alpha-2. `country_reference` therefore carries `iso_numeric`
explicitly as the join key, and the model documents exactly that
(`models/country_reference.rs`), including that it is `None` for codes outside
ISO 3166-1 — currently only Kosovo.

The same file records two more decisions that matter for correctness. `centroid_lat/lon`
is "the centroid of the country's largest landmass, not of all its landmasses averaged"
— which is the difference between a label sitting on a country and a label sitting in
the ocean between two of its islands. And the bounding box, which *does* span every
landmass, exists so a client can derive a fly-to altitude without a geometry index.

`include_on_globe` is a column rather than an array in the code, and the comment says
why: it "replaces every hardcoded country-code array in the codebase."

Borders are drawn from `topojson.mesh` with a filter that suppresses the internal border
between aliased regions — Morocco (504) and Western Sahara (732) — so the map does not
draw a line the data model does not recognise (`Globe.jsx:9-14`, `:345-355`).

### 9.3 Rendering strategy

Everything drawn in bulk is a batched Cesium primitive rather than an entity, and the
layers are separated by explicit heights so they cannot z-fight:

| Layer | Height | Construct |
| --- | ---: | --- |
| Land underlay | 250 m | one `Primitive`, `PerInstanceColorAppearance`, `asynchronous: true` |
| Choropleth | 600 m | one `Primitive`, translucent, `allowPicking: false` |
| Borders | 2,000 m | one `PolylineCollection` |
| Cable routes | 2,500 m | one `PolylineCollection` |
| Blooms | 3,000 m | entities (bounded count) |
| Satellites | compressed | one `PointPrimitiveCollection` |

Three of those choices are load-bearing.

The land underlay is `asynchronous: true` (`:590`) so whole-world tessellation does not
block first paint. The choropleth is `allowPicking: false` (`:660-662`) specifically so
clicks fall *through* it to the land polygons beneath, which carry the alpha-2 country
code as their instance `id` and are the sole pick target for country selection. And
picking disambiguates by JavaScript type (`:396-403`): land polygons carry a **string**
id, satellite points carry a **number**, so the two pick targets cannot collide no
matter what is under the cursor.

Only outer rings are rendered (`g.coordinates[0]`, `:582-583`) — holes are not drawn.

The choropleth ramp is a two-stop interpolation, green → amber → crimson at alpha 0.55
(`:94-105`), driven by `censorship_score` from `/api/censorship-index`, which is the one
place in the tool where the orientation flips from "higher = freer" to "higher = worse".

### 9.4 Animation and blooms

Blooms are radial-gradient canvas textures on surface ellipses, cached by
`(kind, hex)` so the texture set stays bounded (`:107-139`). The gradient carries a hot
near-white core at stop 0 specifically so the mark stays legible over crimson choropleth
land (`:125-127`).

Only `CONFIRMED_BLOCKED` and `LIKELY_BLOCKED` get a bloom; everything else is carried by
the choropleth alone. Severity reads through size as well as hue — radius scales with
the composite index from 130 km to 390 km (`:38-41`), and likely-blocked blooms are
scaled to 0.62 of that (`:76`). Outage bloom radius steps with IODA severity: 340 km at
score ≥200, 240 km at ≥60, else 160 km.

The **only** animation in the scene is the outage pulse, and it animates alpha only:

```js
// frontend/src/components/Globe.jsx:529-532
color: new Cesium.CallbackProperty(
  () => Cesium.Color.WHITE.withAlpha(0.3 + 0.55 * pulse01(OUTAGE_PULSE_PERIOD_MS)),
  false,
)
```

Geometry never changes, so nothing re-tessellates per frame (`:26-27`, `:512-517`), and
there is no `requestAnimationFrame` loop anywhere in the component.

### 9.5 Satellites on the globe

One `PointPrimitiveCollection`, created once, giving a single draw call regardless of
count. The rationale is explicit (`:357-361`): the per-entity bloom pattern "is fine for
a few hundred countries, not for thousands of moving points."

The update path clears and re-adds the whole collection on every poll (`:673-693`), which
is acknowledged in the source as a simplification over incremental diffing. At the
observed catalogue size and a 7-second cadence it holds frame rate; it is the first
thing to change if the catalogue grows substantially.

Altitude is **display-compressed, not true-scale** — a square-root curve mapping 0 to
42,000 km onto 150 km to 3,000 km of rendered height (`:151-166`). Without it, LEO and
GEO cannot both be legible on one globe. The comment is explicit that this changes only
the rendered position: the `alt_km` shown in the satellite card and returned by the API
is the true value. On-screen altitude is therefore monotone but not proportional; the
numeric readout is unaffected.

Orbit paths are fetched only when a satellite is selected, on an effect keyed to the
selection so it does not re-fire on every poll tick. They use the backend's pre-split
segments directly rather than reimplementing antimeridian logic. They are skipped
entirely for geostationary objects (`:714`), because a geosynchronous orbit samples to
an analemma that the longitude-jump split never breaks up.

Polling is a fixed 7,000 ms (`App.jsx:34`, `:346`). Selecting "none" in the space-tracking
control short-circuits to an empty array without fetching, while deliberately leaving the
category counts intact so the legend does not blank out.

### 9.6 Sidebar, charts, and two display rules

The API layer (`lib/api.js`) is plain `fetch` with no client library, no retry and no
caching. Every function throws on a non-OK response except `getCountry`, where **404 is
a meaningful null** — most countries simply have no researched dossier, and that is not
an error (`:15-26`).

Charts are Recharts. The country sidebar issues two fetches per country change: the
blocking rows, and a `Promise.all` over ten timeline technologies.

Two display rules in `CountrySidebar.jsx` are methodological rather than cosmetic:

**A seven-day minimum for timeline evidence** (`:77`). A technology needs at least seven
days of timeline to count as a signal on its own. The comment documents the real false
positive that produced the rule: Iran had exactly one isolated OONI day for a technology,
against 400–990 days for the technologies actually tracked there (`:64-76`).

**Loading is distinguished from genuinely empty** (`:93-108`). A row is only treated as
settled once *both* fetches have landed, because either can promote it. The reason is
that OONI's timeline backfill is the slowest thing in the application — ten
two-dimensional aggregations since 2024-01-01, up to ~18 MB each — so without this,
"empty" and "cold start" would be indistinguishable for minutes at a time.

Both rules are the same principle as the backend's: make the uncertain state visible
rather than letting it render as a confident zero.

---

## 10. Correctness mechanisms

### 10.1 The test suite

75 tests across 14 files. They are not evenly spread, and the distribution is itself
informative — they cluster exactly where a silent wrong answer would be most expensive.

| File | Tests | What they protect |
| --- | ---: | --- |
| `db/satellite_catalog.rs` | 13 | epoch/source merge precedence, persistence, partial-refresh resistance |
| `fetchers/satellites.rs` | 13 | group config parsing, partial-response detection, SGP4 usability, outage time budget |
| `util/date.rs` | 8 | civil-date arithmetic |
| `snapshot/mod.rs` | 7 | round-trip, restore invariant, shrinkage guard, interval floor |
| `fetchers/ooni.rs` | 5 | timeline backfill and resume, registry coverage |
| `fetchers/ripestat.rs` | 5 | sentinel normalisation, deserialisation |
| `util/http.rs` | 4 | identity sanitisation, header validity |
| `satellites/orbit.rs` | 4 | orbital period, antimeridian splitting |
| `api/satellites.rs` | 4 | propagation-age cutoff, staleness policy |
| `fetchers/cloudflare_http.rs` | 3 | percentage parsing, timestamp zipping |
| `satellites/geodetic.rs` | 3 | SGP4 propagation, ECEF→geodetic round-trip |
| `satellites/mod.rs` | 3 | coordinate rounding, category tallies |
| `snapshot/object_store.rs` | 2 | object-name percent-encoding |
| `engine/rules.rs` | 1 | 80-scenario `insta` snapshot |

### 10.2 Tests that test the right thing

Three of them matter more than their count suggests, because in each case the obvious
test would have passed while the real bug shipped.

**The snapshot tests drive a real HTTP server.** `snapshot/mod.rs:460-549` contains an
~80-line axum stub mimicking the Replit sidecar and GCS, with a `break_credentials`
switch. The justification:

> the two claims this module makes that actually matter — 'a round trip reproduces the
> database' and 'a failed download never leads to an upload' — are both properties of
> the *HTTP* paths, so mocking `ObjectStore` would test nothing. This drives the real
> client, the real token exchange and the real status-code handling.
> — `mod.rs:462-466`

`a_failed_restore_can_never_overwrite_the_stored_snapshot` asserts the stored bytes are
identical before and after a broken boot. `the_shrinkage_guard_refuses_a_collapsed_snapshot`
builds 4,000 rows, deletes all but 200, and asserts both the refusal and that the stored
object is untouched. These test the invariant, not the code that implements it.

**The identity test asserts on the wire.** `util/http.rs:183-228` spins up a server and
checks that the `User-Agent` header actually arrives — because asserting on
`user_agent()`'s return value would pass even if `client()` forgot to apply it. That is
precisely the bug the module was written to fix.

**The staleness test uses a real observed object.**
`a_just_accepted_object_with_ancient_elements_is_still_refused` encodes ONDOSAT-OWL-9:
accepted twenty minutes ago, epoch 642 days old. A synthetic fixture would have tested
the threshold; this tests the *scenario* that made the threshold necessary.

### 10.3 The snapshot fixture

`engine/rules.rs` carries one `insta` test covering the full cross-product of 5
countries × 4 organisation types × 4 sensitivity levels — 80 scenarios serialised to a
236 KB `.snap` file. This is what keeps the dormant engine (§12) honest: the code is
unreachable at runtime, so without a pinned output there would be nothing to notice if a
refactor changed its behaviour.

### 10.4 Invariants enforced in code

The mechanisms described throughout this document, collected in one place:

| Invariant | Mechanism |
| --- | --- |
| A failed restore cannot overwrite a good snapshot | `Option<Snapshotter>` — structural, no branch to forget (§8.2) |
| A truncated response cannot shrink the catalogue | no `DELETE` exists in the merge path (§6.3) |
| A truncated snapshot cannot replace a good one | min-retain-ratio guard (§8.5) |
| A partial group response cannot lower expectations | merged, but denied high-water authority (§6.4) |
| A broken element set cannot displace a working one | `Constants::from_elements` validated pre-merge (§6.1) |
| A corrupt snapshot cannot reach `Connection::open` | SQLite magic verified pre-install (§8.4) |
| A half-written file cannot land on the database path | temp + `sync_all` + `rename` (§8.4) |
| A partial seed cannot be committed | single transaction with `ROLLBACK` (§3.5) |
| A dossier without geometry cannot boot | `assert_reference_covers_researched` (§3.6) |
| A failed fetch cannot fake freshness | `last_success_at` untouched on failure (§4.2) |
| A missing credential cannot look like an empty source | boot-time `WARNING` block (§8.1) |
| A lost database cannot look like a fresh one | monotonic boot counter (§8.8) |
| Absence cannot render as zero | `Option` returns, `NO DATA` states, 422 over fiction (§5.1, §6.6) |

The last is the one that recurs most: `compute` returns `None` for a country with no
index components; `/orbit` returns 422 rather than a fabricated path; unpropagatable
satellites are counted but not drawn; Tor's classifier returns `INCONCLUSIVE` when the
published bound is absent; Cloudflare's missing series is `None`, not zero.

---

## 11. Build and deployment

There is no Dockerfile. The build and run contract is two files at the repository root.

### 11.1 `.replit`

The deployment build command, in order, with its assertions:

```sh
rustup toolchain install 1.91.0 --profile minimal --no-self-update
  && rustup default 1.91.0 && cargo --version
  && cd frontend && npm ci && npm run build
  && cd ../backend && cargo build --release --locked
  && cd .. && mkdir -p bin && cp backend/target/release/backend bin/blackout
  && test -x bin/blackout && test -f frontend/dist/index.html
```

Four decisions here are deliberate.

**The toolchain is pinned through `rustup`, not taken from the Nix channel.** The
channel's Cargo is 1.77.1, and this crate is `edition = "2024"`, which needs 1.85 or
later. The code uses let-chains (`main.rs:45-47`), so this is not a nominal setting.
1.91.0 is pinned because it is the version the crate was verified against.

**The toolchain installs first**, so a toolchain failure surfaces in about 30 seconds
rather than after the two-minute frontend build.

**`cargo --version` is echoed** because, in the maintainer's note, the last failure was
a Cargo version mismatch that no other log line would have revealed.

**The binary is copied into `bin/`** because `backend/target/` is excluded from what the
build stage hands to the run stage — several gigabytes. `.replitignore` carries a
hard-won comment about this: excluding `frontend/dist/` as well once produced a build
that succeeded and then failed at runtime with
`fork/exec backend/target/release/backend: no such file or directory`. It also excludes
`*.db`, because a shipped local database would be found by `restore_if_absent`'s
"already present" branch and silently beat the real snapshot.

`deploymentTarget = "vm"` — a Reserved VM, not Autoscale — and the reasoning is stated
in the file: the fetch, precision, satellite and snapshot loops are all timer-driven,
and an Autoscale deployment scales to zero between requests and would simply never run
them.

Port 3001 maps to external 80. `PORT` is deliberately left unset so the platform can
inject it.

### 11.2 `replit.nix`

Five dependencies, each with a cited reason: `rustup` (the channel Cargo predates
edition 2024), `nodejs_22` (Vite 5 needs 18+), `pkg-config` and `openssl` (reqwest's
default features pull in native-tls), `stdenv.cc` (rusqlite's `bundled` feature compiles
SQLite from C source), and `cacert` (every fetcher and the snapshot layer is HTTPS).

A `rust-toolchain.toml` was explicitly rejected, because it would also retarget local
`cargo build`, and this is a Replit packaging concern rather than a project-wide one.

### 11.3 Dependency choices

Thirteen runtime crates. Three carry their justification in `Cargo.toml` itself:

`tower-http` with `compression-gzip` and deliberately not brotli, for the two-vCPU
reason in §7.1. `flate2` with default features, because that selects the pure-Rust
miniz_oxide backend and therefore adds no system library for `replit.nix` to provide.
And `comrak` with `default-features = false`, because the default set drags in clap and
a syntax highlighter that links the C Oniguruma regex library — a system dependency, for
highlighting on documents that contain no code.

`chrono` is declared directly despite arriving transitively through `sgp4`, because a
transitive dependency is not `use`-able on its own, and it is enabled with `serde` so
satellite timestamps derive `Serialize`.

`backend/Cargo.lock` is committed (2,005 lines), so `cargo build --release --locked` is
a genuine reproducibility claim rather than a gesture. `frontend/package-lock.json` is
committed for the same reason and `npm ci` depends on it.

### 11.4 Configuration

Every variable, with its default and what it gates:

| Variable | Default | Effect |
| --- | --- | --- |
| `DEPLOYMENT_ID` | `local` | User-Agent and RIPEstat `sourceapp`; sanitised to `[A-Za-z0-9._-]`, ≤48 chars |
| `DEPLOYMENT_CONTACT` | project URL | contact in the User-Agent; rejected if it holds control characters |
| `CLOUDFLARE_API_TOKEN` | unset | Radar annotations + HTTP-version share; boot WARNING if absent |
| `PULSE_API_TOKEN` | unset | Internet Resilience Index; boot WARNING if absent |
| `PEERINGDB_API_KEY` | unset | **script only**, never read by the server |
| `DATABASE_PATH` | `mena_ai.db` | SQLite location |
| `SEED_DIR` | `data/seed` | a wrong value is fatal — crash loop |
| `STATIC_DIR` | `../frontend/dist` | SPA root |
| `PORT` | `3001` | listener |
| `HEALTH_MAX_AGE_DAYS` | `2` | `/health` turns 503 past this |
| `FETCH_INTERVAL_HOURS` | `6` | general loop, floored at 60 s |
| `PRECISION_FETCH_INTERVAL_HOURS` | `1` | precision loop, floored at 60 s |
| `SNAPSHOT_KEY` | unset | unset makes the whole snapshot layer inert |
| `SNAPSHOT_INTERVAL_MINUTES` | `180` | floored at 300 s |
| `SNAPSHOT_MIN_RETAIN_RATIO` | `0.5` | clamped to `[0,1]` |
| `SNAPSHOT_SIDECAR_WAIT_SECS` | `10` | `0` means a single attempt |
| `SATELLITE_CATALOG_REFRESH_HOURS` | `2` | matches CelesTrak's own cycle |
| `SATELLITE_STALE_AFTER_HOURS` | `24` | annotation only |
| `SATELLITE_MAX_PROPAGATION_AGE_DAYS` | `30` | exclusion from propagation |
| `SATELLITE_GROUP_MIN_RETAIN_RATIO` | `0.5` | per-group shrinkage guard |
| `SATELLITE_GROUPS` | 11-group list | group → category, order is precedence |
| `RIPESTAT_SOURCEAPP` | derived | `blackout-<DEPLOYMENT_ID>` |
| `SSL_CERT_FILE` | set in `.replit` | without it every fetcher fails while the process looks healthy |

All three credentials are free and all three are optional in the sense that the app
starts without them. Two of them gate visible data and are named individually at boot;
the third is never read by the server at all.

### 11.5 Generator scripts

Four Node scripts in `backend/scripts/`, none part of the build, each producing
committed output. They exist because their upstreams should be called rarely and by
hand.

`gen_ixp_data.mjs` records why: "a third quick request in the same short window came
back `Request was throttled. Expected available in 59 minutes.`" — hence the file's
instruction to make exactly one request and never add a per-country loop.

`gen_country_reference.mjs` generates centroids and bounding boxes from
`world-atlas/countries-50m.json` — **the same basemap the globe renders** — "so a marker
can never disagree with its own borders." That is the correct way to eliminate a whole
class of join bug, and it is a design choice rather than an implementation detail.

`gen_cable_data.mjs` reconciles TeleGeography's country names against the committed
reference by longest-trailing-segment matching and an alias table, **never a fuzzy
match**; unresolved landing points keep a null country code and are logged rather than
dropped.

---

## 12. Unshipped surface

`engine/` is 1,422 lines — `contract.rs` (1,060) and `rules.rs` — implementing a
deployment-feasibility assessment: given a country, an organisation type and a data
sensitivity level, which deployment paths are open, what blocks the others, and what the
residual risks are. `contract.rs` builds the assessment twice, once for the caller's
request and once against a hardcoded baseline, and diffs them.

It is reachable only through `POST /api/evaluate`, which **is not mounted**:

> Every route below is a GET; `POST /api/evaluate` is deliberately NOT mounted — it was
> the one write path (it persists evidence rows) and nothing in the UI calls it, so on a
> public URL it would be an anonymous write for no benefit.
> — `main.rs:145-150`

The module is kept compiling under `#[allow(dead_code)]` rather than deleted, with the
policy stated at `main.rs:9-13`: it stays compiled, and its snapshot test stays running,
so the feature can be restored behind a guard without resurrecting deleted code.

The deployed security posture, in summary: the assessment engine is dormant, its only
write path is unrouted, and every mounted route is a GET.
The eight hardcoded seed tables in `db/seed.rs` exist to feed it and are otherwise inert.

`/api/models` and `/api/signals` are mounted and functional but unused by the shipped
frontend. `engine/brief.rs` is an empty file exported as a module.

---

## 13. Known limitations and sharp edges

Stated plainly, and in the same detail as everything above.

**No CI.** There is no `.github/` directory and no pipeline of any kind. The 75 tests
run when someone types `cargo test`. The only automated gate is `.replit`'s
`test -x bin/blackout && test -f frontend/dist/index.html`, which checks that artifacts
exist, not that they are correct. This is the single largest gap between the quality of
the code and the quality of the process around it — the invariants in §10.4 are well
tested and nothing enforces that they stay that way.

**No frontend tests.** 4,812 lines of JSX and JS with no test runner configured.

**The censorship index is untested.** The one formula BLACKOUT constructs itself — the
weights, the renormalisation, the inversion, and the drop-on-no-components behaviour in
§5 — has no unit test. Each of those behaviours is a plausible regression and each would
be a two-line test.

**No HTTP graceful shutdown.** `axum::serve` is called without
`with_graceful_shutdown`. SIGTERM is handled only inside the snapshot hook, which takes
its final snapshot and then calls `std::process::exit(0)`, so in-flight requests are
dropped rather than drained. And when `SNAPSHOT_KEY` is unset, no signal handler is
installed at all — the process dies on default disposition. For a read-only API the
practical cost is low, but it is a real gap and not an intentional one.

**`indices` can report success on partial failure.** V-Dem and RSF each fetch inside
their own error handler and `fetch_and_store` returns `Ok(())` regardless (§4.10), so
`fetch_runs` can record `ok` for a cycle in which one of the two sources failed. Every
other fetcher bails on partial failure. This is an inconsistency, not a design.

**The global connection mutex.** One `Mutex<Connection>` serialises every
SQLite-backed route (§3.1), and `VACUUM INTO` holds it for one to two seconds at 70 MB.
The hot path is deliberately kept outside it and the snapshot runs on a blocking thread,
but under real concurrency this is the first thing that would need to become a pool.

**Bowring near the poles.** `alt = p / cos(lat) − N` (§6.2) is ill-conditioned as
`cos(lat) → 0`, and there is no polar special case. Whether this matters in practice
depends on how close to ±90° the catalogue's high-inclination objects actually get; the
iteration is not wrong, it is numerically fragile in a narrow band.

**Satellite rendering clears and re-adds** the entire point collection every 7 seconds
rather than diffing. Acknowledged in the source. It holds at the current catalogue size.

**Three stale comments in the frontend** assert there is no server-side position cache —
`lib/api.js:132-135`, `App.jsx:29-33`, `Globe.jsx:668-669` — which the 1-second cache at
`api/satellites.rs:32-53` contradicts. `Globe.jsx` also describes the poll as "every
5-10s" against a fixed 7,000 ms. Prose only, no behavioural impact, but these are
exactly the comments a future maintainer would trust.

**SatNOGS is unattributed in the UI.** It is a live upstream
(`fetchers/satellites.rs:56`) but does not appear in the `SOURCES` list that drives the
footer (`lib/sources.js:24-36`). Given that the same file goes out of its way to
attribute V-Dem and RSF to the specific Our World in Data grapher pages actually used,
and to *exclude* hand-curated Starlink data rather than claim a provenance it does not
have, this omission reads as an oversight rather than a decision.

**A hand-maintained coupling.** `TIMELINE_TECHNOLOGIES` in
`frontend/src/lib/blockingRegistry.js:75-86` must be kept in sync by hand with
`TIMELINE_TECHS` in `backend/src/fetchers/ooni.rs`. The comment says so, which is better
than nothing, but nothing enforces it.

**Undocumented variable.** `SNAPSHOT_SIDECAR_WAIT_SECS` exists in code but not in
`.env.example`.

**Data freshness is mixed by design.** Some datasets are fetched continuously, some are
committed reference files regenerated by hand, and Starlink status is manually curated.
The UI distinguishes them; anyone reading the database directly should know the seeded
files age silently.

---

## 14. Provenance

95 commits between 2026-07-31 and 2026-09-25 — 56 days. 79 commits by `moumenalaoui`
and 17 by `scsp-op`, the latter forming the tail of the history: the snapshot sidecar
retry, SCSP branding, and the space-tracking widget.

Roughly 19,000 lines of hand-written code and configuration, plus 1.79 MB of committed
seed data produced by four generator scripts that are themselves committed — so the
reference corpus can be regenerated rather than merely trusted.

The dependency surface is small and deliberate: thirteen runtime crates and six frontend
packages, with both lockfiles committed and the build running `--locked`.
