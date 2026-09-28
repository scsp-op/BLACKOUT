# BLACKOUT — Technical Methodology

*Prepared September 2026. This document describes how BLACKOUT acquires, stores, computes
and serves its data: the system architecture, the algorithms it implements, and the
mechanisms that keep its output correct. It is written for engineers who want to
understand the implementation or test it. The companion policy methodology covers what
the tool measures and how its findings should be interpreted.*

*File references point into the source. Rust paths are relative to `backend/src/`,
frontend paths to `frontend/src/`.*

---

## 1. Architecture

BLACKOUT is a single Rust binary built on axum. It serves a read-only JSON API and the
static React/CesiumJS frontend from one port, backed by one local SQLite file. Three timer
loops keep that file current from ten external sources (nine services: V-Dem and RSF both
arrive through Our World in Data), a fourth backs it up, and an in-memory catalogue holds
the satellite element sets used for orbit propagation.

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

There is no separate worker process, queue, cache tier or external database. The
deployment runs on a two-vCPU Replit Reserved VM, chosen over Autoscale because the timer
loops must keep running between requests. The repository also contains a
deployment-assessment engine (`engine/`) that is compiled but not mounted; it is outside
the scope of this document.

### 1.1 Design principles

Four rules recur throughout the implementation. Most of the mechanisms below apply one
of them, and they are the properties most worth testing.

1. **Absence is not zero.** Missing data is represented as missing — `None`,
   `INCONCLUSIVE`, or omitted from a response — never as a zero that reads as a
   measurement.
2. **Uncertainty is the default.** Classifiers are written so that a case matching no
   rule falls through to `INCONCLUSIVE`, and sample size is part of every threshold.
3. **Durable state only improves.** Some of the data cannot be fetched again (§6). The
   satellite catalogue has no delete path, a partial response cannot lower expectations,
   and a failed restore cannot overwrite a good snapshot.
4. **Each upstream fails alone.** Every source has its own timeout, pacing and retry
   policy, and its outcome is recorded separately.

---

## 2. Data model

One SQLite file with 29 tables, opened once at boot. A single `rusqlite::Connection`
sits behind a `std::sync::Mutex` (`AppState`, `main.rs`), with no connection pool. The
schema sets `journal_mode=WAL`, `busy_timeout=5000` and `synchronous=NORMAL`. The most
frequently polled endpoint, satellite positions, does not touch the database (§5.6).

### 2.1 Tables

| Group | Tables | Written by |
| --- | --- | --- |
| Reference | `country_reference` (249), `starlink_status` (25), `cable_routes` (728), `cable_landing_points` (1,925), `ixp_stats` (170), `countries` (5 researched dossiers), `model_releases` (8) | seed loader, at boot |
| Censorship | `technology_blocks`, `category_blocks`, `blocking_timeline`, `adoption_signals` | OONI, Cloudflare Radar |
| Disruption and routing | `outage_events`, `http_protocol_share`, `bgp_prefix_visibility`, `tor_metrics` | IODA, Cloudflare Radar, RIPEstat, Tor Metrics |
| Indices | `country_scores` | V-Dem, RSF, ISOC Pulse |
| Satellites | `satellite_catalog`, `satellite_refresh_state`, `satellite_elements` (legacy, read once for migration) | CelesTrak, SatNOGS |
| Bookkeeping | `fetch_runs` | every fetcher |

The remaining nine tables serve the unmounted assessment engine or are unused.

### 2.2 Keys carry the deduplication

Almost every fetcher writes with `INSERT OR REPLACE`, so the primary key decides whether
a row is new or a refresh.

- **Current state versus history.** `technology_blocks` is keyed
  `(country_code, technology)`: one current row per pair, overwritten on each fetch.
  `blocking_timeline` adds `measurement_date` to the key and is therefore history, one
  row per day. Because the date is part of the key, the fetcher discards OONI's hourly
  buckets (`util::date::is_iso_date`) so they cannot create duplicate logical days.
- **Keys that omit a dimension.** `country_scores` is keyed `{country}-{source}` (for
  example `IR-V_DEM`), with the year as an ordinary column. A re-run refreshes the
  current score in place rather than accumulating a row per year.
- **Keys that include one.** `outage_events` is keyed
  `ioda-{country}-{datasource}-{start}`. IODA re-serves an event across overlapping
  windows, which must update rather than duplicate it, while the same outage seen by
  different IODA datasources stays as separate observations. `adoption_signals` is keyed
  per country, URL and day, so re-running today overwrites and tomorrow appends.

Because `INSERT OR REPLACE` deletes and re-inserts the row, each write lists every column
it owns. The V-Dem write, for instance, sets the headline score and all nine sub-score
columns in one statement, so no sub-score is ever nulled by a later refresh.

### 2.3 Schema evolution and seeding

Migrations are additive only (`db/migrations.rs`): an ordered list of nullable
`ADD COLUMN`s, each applied at boot if `PRAGMA table_info` shows the column missing. A
migration naming a non-existent table fails the boot. There is no version table, and
structural changes are out of scope.

Seeding runs sixteen loaders in one transaction that rolls back on any error, so a
partial seed is never committed (`db/seed.rs`). Eight read committed files from
`data/seed`; eight populate the dormant engine. Generated and curated reference files use
`INSERT OR REPLACE`, so an updated file takes effect on the next deploy, and
`country_reference` is deleted and reloaded wholesale so that removed rows disappear too.
Tables that a live process may also write, or where database state should win over the
seed, use `INSERT OR IGNORE`. The generated files come from three Node scripts in
`backend/scripts/`, run by hand: country centroids and bounding boxes from the same
`world-atlas` basemap the globe renders, cables from TeleGeography with explicit alias
matching rather than fuzzy matching, and IXP counts from PeeringDB.

Of the 249 reference countries, 235 are flagged `include_on_globe`, and that list is the
sweep every per-country fetcher iterates. Boot ends with an invariant: every researched
dossier in `countries` must have a `country_reference` row, or the process exits before
the listener binds.

---

## 3. Ingestion

### 3.1 Scheduling

| Loop | Default | Override | Runs |
| --- | --- | --- | --- |
| General | 6 h | `FETCH_INTERVAL_HOURS` | OONI (three fetchers), Tor, IODA, Pulse, V-Dem/RSF, Cloudflare annotations |
| Precision | 1 h | `PRECISION_FETCH_INTERVAL_HOURS` | Cloudflare HTTP version share, RIPEstat |
| Satellite catalogue | 2 h | `SATELLITE_CATALOG_REFRESH_HOURS` | CelesTrak, SatNOGS |
| Snapshot | 3 h | `SNAPSHOT_INTERVAL_MINUTES` | database backup (§6) |

The fetch loops are floored at 60 s and the snapshot loop at 300 s, so a mistyped value
produces a slow loop rather than a request flood. HTTP/3 share and BGP visibility run
hourly because they are the two signals intended to lead an event rather than confirm it.

Within a loop, fetchers run concurrently (`tokio::join!`), each under its own timeout,
from 30 s for Cloudflare's single bulk request to 900 s for the OONI timeline sweep
(`db/mod.rs`). Within a fetcher, requests are sequential and paced — per country for
IODA (300 ms), Cloudflare HTTP (300 ms) and RIPEstat (200 ms), and per request for OONI
(2 s), Tor (300 ms) and CelesTrak groups (500 ms) — to stay within what free, often
volunteer-run services can absorb.

### 3.2 Recording outcomes

Every run is recorded in `fetch_runs` (`db/mod.rs`):

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

Attempt and success are separate columns, so the age of the data and the liveness of the
fetcher can be read independently. A run is recorded as `error` when the fetcher times
out or returns an error. OONI and the satellite refresh return an error if any item in
the sweep failed; the other fetchers log per-item failures, keep what succeeded, and
complete. Retries are per source, where the upstream calls for them (OONI's rate limiting,
CelesTrak's transient errors); there is no global backoff.

`/health` measures freshness from the data itself — the newest `last_updated` across
`country_scores` and `outage_events` — and returns 503 once that is older than
`HEALTH_MAX_AGE_DAYS` (default 2).

### 3.3 Outbound identity

Every fetcher builds its HTTP client through `util/http.rs`, which sets a User-Agent of
the form `blackout-{component}/{version} (+{contact}; id={deployment})`, for example
`blackout-ioda/0.1.0 (+https://github.com/moumenalaoui/BLACKOUT; id=local)`.
`DEPLOYMENT_ID` is sanitised to `[A-Za-z0-9._-]` and 48 characters, because it also
reaches RIPEstat's `sourceapp` parameter. BLACKOUT does not rotate User-Agents or forge
headers to work around rate limits.

### 3.4 OONI

All four phases query the aggregation API (`https://api.ooni.io/api/v1/aggregation`) and
run as three fetchers: `ooni` (phases A and B), `ooni_categories` and `ooni_timeline`
(`fetchers/ooni.rs`). On HTTP 429 the client honours `Retry-After`, otherwise waits
`3·2^attempt` seconds, capped at 60 s, for at most two retries.

**Phase A — AI service reachability.** One request per target (`api.openai.com`,
`openai.com`, `api.anthropic.com`, `www.deepseek.com`): the `web_connectivity` test over
90 days, aggregated by country. Any confirmed measurement gives `BLOCKED`; zero anomalies
gives `ACCESSIBLE`; anything else, including no measurements, is `INCONCLUSIVE`.
Confidence is `HIGH` at 10 or more measurements, `MEDIUM` at 3 or more, else `LOW`.

**Phase B — technology blocking.** A 14-entry registry: four AI-access domains, six
circumvention tools and four messaging apps. Seven entries are `web_connectivity` against
a host; `tor`, `psiphon`, `torsf` and the four messaging apps use OONI's dedicated
nettests. Each (country, technology) pair is classified from its anomaly count and total:

```rust
// fetchers/ooni.rs — classify_technology
if total == 0 { return ("INCONCLUSIVE", 0.0); }
let rate = anomaly as f64 / total as f64;
let status = if rate > 0.7 && total > 10 { "CONFIRMED_BLOCKED" }
    else if rate > 0.4 && total > 5  { "LIKELY_BLOCKED" }
    else if rate < 0.1 && total > 5  { "ACCESSIBLE" }
    else { "INCONCLUSIVE" };
```

This is the classification that drives the blooms on the globe. Sample size is part of
every band: a 100% anomaly rate is `LIKELY_BLOCKED` over eight measurements and
`INCONCLUSIVE` over five. The gaps between bands fall through deliberately, so 30% over
20 measurements is `INCONCLUSIVE`. Where OONI leaves `measurement_count` empty, the total
is reconstructed as `anomaly + confirmed + failure + ok`.

**Phase C — content categories.** One two-dimensional request for the whole world
(`axis_x = category_code`, `axis_y = probe_cc`, 180 days):

```rust
if measurement_count < 100 { "INCONCLUSIVE" }
else if anomaly_rate >= 0.20 { "HEAVILY_CENSORED" }
else if anomaly_rate >= 0.05 { "PARTIALLY_CENSORED" }
else { "ACCESSIBLE" }
```

The minimum sample is higher and the thresholds lower than in phase B because a category
aggregates many sites: the sample is much larger, and a 20% anomaly rate across a whole
category is a stronger signal than 20% against one domain.

**Phase D — blocking timelines.** One two-dimensional request per technology, for ten
technologies (`axis_x = measurement_start_day`, `axis_y = probe_cc`). The window is
incremental: each request starts seven days before the newest stored date for that
technology, or at 2024-01-01 on a cold start. The overlap exists because OONI's recent
days are provisional; re-fetching them lets `INSERT OR REPLACE` apply the revisions.

### 3.5 IODA

One request per country to `https://api.ioda.inetintel.cc.gatech.edu/v2/outages/events`
over a 90-day window, with `from` and `until` as Unix seconds (`fetchers/ioda.rs`). Each
event is stored with `end = start + duration`; a missing score defaults to 0.0 and a
missing datasource to `"unknown"`. Each country's rows are written as soon as its request
returns, so a failure part-way through keeps everything already retrieved.

### 3.6 Tor Metrics

Three whole-world CSV downloads — relay users by country, bridge users by country, and
bridge users by country and transport — each from 2024-01-01 to today, where Tor's `end`
is inclusive (`fetchers/tor_metrics.rs`). Columns are located by name, case-insensitively,
because the column order of a published CSV is not a stable contract. Empty cells are
stored as NULL. Only the `obfs4`, `snowflake` and `webtunnel` transports are kept.

Two values are derived per country-day:

```rust
let ratio = bridge_users.unwrap_or(0) as f64 / (relay_users.unwrap_or(0) as f64 + 1.0);

fn classify_blocking(users: Option<i64>, lower: Option<i64>) -> &'static str {
    match (users, lower) {
        (Some(u), Some(l)) if u < l => "HIGH_BLOCKING",
        (Some(u), Some(l)) if (u as f64) < (l as f64) * 1.2 => "MODERATE",
        (Some(_), Some(_)) => "LOW",
        _ => "INCONCLUSIVE",
    }
}
```

The `+ 1.0` guards against division by zero, which makes the ratio approximate at very
low relay counts. The classifier compares relay users against `lower`, Tor's own
published anomaly-detection bound, rather than a threshold set by BLACKOUT; when Tor
publishes no bound, the result is `INCONCLUSIVE`. Roughly 150,000 rows are written in one
transaction.

### 3.7 Cloudflare Radar

**Outage annotations** (`fetchers/cloudflare.rs`): one bulk request,
`limit=200&dateRange=52w&format=json`. Each annotation lists affected locations and fans
out to one `adoption_signals` row per recognised country. Ongoing outages keep a null end
date rather than defaulting to "now".

**HTTP version share** (`fetchers/cloudflare_http.rs`): one request per country,
`dateRange=14d&aggInterval=1d&normalization=PERCENTAGE`, run hourly. The whole 14-day
window is re-fetched each time so that Cloudflare's provisional recent days are
overwritten as they settle. Cloudflare omits near-zero series rather than reporting them,
so a missing HTTP/3 series is stored as NULL ("not reported"), not 0.

Both fetchers skip, with a warning at boot and on each cycle, when
`CLOUDFLARE_API_TOKEN` is unset, so a missing credential renders as an empty panel
rather than a failed fetch.

### 3.8 RIPEstat

One request per country to `https://stat.ripe.net/data/country-resource-stats/data.json`
over 14 days at one-day resolution, with no credential and a `sourceapp` derived from the
deployment identity (`fetchers/ripestat.rs`). Counts are stored as floating point because
RIPEstat averages across route-collector snapshots and returns fractional values (the US
has returned `v6_prefixes_ris: 49559.5`). RIPEstat's `-1` "not available" sentinel is
stored as NULL. Routed counts (`_ris`, from RIS collectors) and registered counts
(`_stats`, from RIR delegations) answer different questions and are stored separately as
`routed_*` and `registered_*`; no ratio between them is precomputed.

### 3.9 Internet Society Pulse

One bulk request to `pulse-api.internetsociety.org` returns the latest quarter for all
179 rated countries (`fetchers/pulse.rs`). The host is the API host, not
`pulse.internetsociety.org/api`, which sits behind a browser challenge. Scores are
rescaled from 0–1 to 0–100 at one decimal place and banded at 70 (highly resilient),
50 (moderately resilient) and 35 (fragile), with anything lower highly fragile.

### 3.10 V-Dem and RSF

Headline scores come from two Our World in Data grapher CSVs
(`freedom-of-expression-index` and `press-freedom-index-rsf`, both `csvType=full`), read
positionally as ISO3 code, year and value (`fetchers/indices.rs`). Rows without an ISO3
code, with OWID's aggregate pseudo-codes, or with a blank value are skipped. The latest
year per country is kept, so two countries' scores may come from different years; the
year is returned alongside each score.

Every score in the tool is stored as "higher = more free":

```rust
// V-Dem freedom-of-expression estimate is already 0–1, higher = freer
Source::Vdem => score = (raw * 100.0).clamp(0.0, 100.0)

// OWID's RSF series is the pre-2022 index, higher = LESS free
Source::Rsf  => score = (100.0 - raw).clamp(0.0, 100.0), band = rsf_band(raw)
```

The RSF score is inverted, but its band label is computed from the raw value using RSF's
own published thresholds (below 15 Good, 25 Satisfactory, 35 Problematic, 55 Difficult,
otherwise Very serious), so the labels agree with RSF's.

V-Dem's internet-censorship sub-scores are not on OWID. They come from a committed
extract of V-Dem Country-Year Full+Others v16 (`data/seed/vdem_dsp_v16.csv`):

| Variable | Measures | Scale |
| --- | --- | --- |
| `v2smgovfilprc` | government internet filtering in practice | 0–4 |
| `v2smgovshut` | government internet shutdown in practice | 0–4 |
| `v2mecenefi` | internet censorship effort | 0–3 |

Each is normalised to 0–100 against its own scale, so `v2mecenefi` is divided by 3, not 4.
None is inverted, because V-Dem already codes all three as higher = less censorship. Each
is stored with its credible interval (`_osp_codelow`, `_osp_codehigh`), rescaled the same
way. V-Dem entities with no ISO 3166-1 code (in this extract, Somaliland, Zanzibar and
Palestine/Gaza) are counted and logged. The sub-scores are displayed in the country
sidebar but do not enter the Censorship Index.

---

## 4. The Censorship Index

The index is the one number BLACKOUT constructs rather than reports. It is computed on
each request from `country_scores` and never stored (`api/censorship_index.rs`).

Its inputs are two columns:

```sql
SELECT country_code, source, score_overall FROM country_scores
WHERE source IN ('V_DEM','RSF') AND score_overall IS NOT NULL
```

And the computation, in full:

```rust
const W_VDEM: f64 = 0.5;
const W_RSF: f64 = 0.3;

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

Because the accumulated weight is divided out, the weights renormalise over whichever
components are present:

| Components | Result |
| --- | --- |
| Both | `freedom = (0.5·V + 0.3·R) / 0.8 = 0.625·V + 0.375·R` |
| V-Dem only | `freedom = V` |
| RSF only | `freedom = R` |
| Neither | the country is omitted from the response |

Several properties follow:

- **Only the 5:3 ratio matters.** The weights summing to 0.8 rather than 1 has no effect
  on any output.
- **A single-component country scores exactly its input**, rather than being discounted
  for the missing component, which would systematically rank sparsely covered countries
  as freer. Scores are therefore directly comparable only between countries with the same
  `component_count`, which each entry includes.
- **No components means no entry**, not a zero; such a country is absent from the
  choropleth rather than drawn as maximally free or maximally censored.
- **The clamp precedes the inversion**, so `censorship_score` lies in [0, 100] by
  construction.
- Each entry carries the `vdem` and `rsf` inputs as stored, so the composite can be
  recomputed from the response itself. Entries are sorted most-censored first.

The inversion from "higher = more free" to "higher = more censored" happens here, once,
immediately before display. `/api/rankings` is a separate single-source leaderboard over
`country_scores` and computes no composite.

---

## 5. Satellite tracking

The satellite subsystem acquires orbital element sets from two providers, holds them as
durable state that can only improve, and propagates them to positions on demand — within
a 7-second poll on two vCPUs.

### 5.1 Element sets

| Source | Endpoint | Format |
| --- | --- | --- |
| CelesTrak GP | `celestrak.org/NORAD/elements/gp.php?GROUP=<group>&FORMAT=JSON` | OMM JSON |
| CelesTrak supplemental | `celestrak.org/NORAD/elements/supplemental/sup-gp.php?FILE=starlink&FORMAT=JSON` | OMM JSON |
| SatNOGS | `db.satnogs.org/api/tle/?format=json` | three-line TLEs in JSON |

`FORMAT` is passed explicitly because CelesTrak's default changed to CSV on 2026-05-09.
OMM records deserialise directly into `sgp4::Elements`; SatNOGS TLEs are parsed with
`Elements::from_tle`, and a malformed row is skipped and counted.

Every element set is validated with `sgp4::Constants::from_elements` before it may
compete in the merge, so a newer but unusable record can never displace an older working
one.

Categories come from the CelesTrak group list, whose order is the precedence
(`fetchers/satellites.rs`, overridable with `SATELLITE_GROUPS`):

```rust
("starlink","starlink"), ("gps-ops","navigation"), ("galileo","navigation"),
("glo-ops","navigation"), ("beidou","navigation"), ("military","military"),
("resource","earthobs"), ("stations","stations"), ("science","stations"),
("geo","geo"), ("active","other"),
```

### 5.2 The catalogue and its merge rule

The canonical store is `satellite_catalog`, keyed on `norad_id` alone. Membership belongs
to that table, not to any upstream response, and the merge path contains no `DELETE`: a
truncated, partial or failed response cannot remove a satellite.

A refresh is an upsert, and whether an incoming record replaces the stored one is decided
by a single comparison (`db/satellite_catalog.rs`):

```rust
fn is_improvement(incoming_epoch, incoming_rank, current: &ExistingMeta) -> bool {
    match incoming_epoch.cmp(&current.epoch) {
        Ordering::Greater => true,
        Ordering::Equal   => incoming_rank < current.source_rank,
        Ordering::Less    => false,
    }
}
```

The primary criterion is the element set's own epoch — when upstream determined the
orbit — not when it was fetched. Ties break on source rank: CelesTrak supplemental (0),
then CelesTrak (1), then SatNOGS (2). The providers are complementary sources merged into
one catalogue, not alternatives.

Category is upgraded on a separate axis:

```sql
category      = CASE WHEN ?8 < category_rank THEN ?9 ELSE category END,
category_rank = MIN(category_rank, ?8)
```

so SatNOGS can supply fresher elements without demoting a CelesTrak-derived category;
SatNOGS records carry category rank 1000, below every group. The merge runs in one
transaction, after which the in-memory catalogue is reloaded from SQLite, re-validating
each row, rather than assembled from the response.

### 5.3 Refreshing against unreliable upstreams

CelesTrak rate-limits repeat downloads and has been unreachable for extended periods, so
the catalogue refresh carries more defensive machinery than any other fetcher:

- **Retry:** three attempts, 2 s base delay, exponential with up to 50% jitter, for
  transient errors only.
- **403 disambiguation:** a 403 whose body contains `"has not updated since"` or
  `"no sooner than"` is CelesTrak saying the data is unchanged, and counts as success.
  Any other 403 is a transient failure.
- **Circuit breaker:** three consecutive group failures abandon the remaining groups and
  the supplemental feed for that cycle, bounding a full outage at 300 s.
- **Pacing:** 500 ms between group requests.
- **Empty-catalogue retry:** while the catalogue is empty the loop retries every 180 s
  instead of waiting two hours.
- **Deferred first refresh:** at boot the persisted catalogue is loaded and served before
  any network call, and a refresh runs only if one is due, so a redeploy costs no
  upstream requests.

Each group also has a shrinkage guard:

```rust
fn is_suspiciously_partial(received: usize, previous: usize, min_ratio: f64) -> bool {
    if previous == 0 { return false; }   // first fetch sets the baseline
    if received == 0 { return true; }    // an empty response for a known group is suspect
    (received as f64) < (previous as f64) * min_ratio
}
```

`previous` is the group's last accepted size, persisted in `satellite_refresh_state`, and
`min_ratio` defaults to 0.5 (`SATELLITE_GROUP_MIN_RETAIN_RATIO`). A response judged
partial is still merged — records can only improve rows — but its size does not become
the new baseline, and baselines are committed only after a successful merge. The data is
used; it is not trusted as a measure of how much data there should be.

### 5.4 Propagation and coordinate frames

Propagation uses the `sgp4` crate (2.4). `Constants::from_elements` selects the WGS-84
geopotential and IAU sidereal-time expression rather than the AFSPC/WGS-72 compatibility
mode, and the crate chooses near-earth SGP4 or deep-space SDP4 by orbital period, so
geostationary and Molniya orbits need no special handling. Constants are derived once,
when the catalogue loads.

The time argument is signed minutes from the element set's own epoch, so propagation
backwards is as valid as forwards. `propagate` returns a position in the TEME frame, in
kilometres. Converting it to latitude, longitude and altitude takes two steps, both in
`satellites/geodetic.rs`.

**TEME → ECEF** is a rotation about Z by Greenwich Mean Sidereal Time, using the crate's
IAU-82 GMST function — the same one it hands the propagator for its deep-space terms:

```rust
let gmst_rad = sgp4::iau_epoch_to_sidereal_time(sgp4::julian_years_since_j2000(&at.naive_utc()));
let (sin_gmst, cos_gmst) = gmst_rad.sin_cos();
let x_ecef =  x * cos_gmst + y * sin_gmst;
let y_ecef = -x * sin_gmst + y * cos_gmst;
let z_ecef = z;
```

Polar motion and nutation beyond what TEME already includes are ignored. This is standard
for SGP4-based tracking: SGP4 itself is accurate only to about a kilometre, so
sub-arcsecond frame corrections would not survive it.

**ECEF → geodetic** is a fixed-point iteration on the WGS-84 ellipsoid (the classical
Heiskanen–Moritz scheme):

```rust
const WGS84_A_KM: f64 = 6378.137;
const WGS84_F: f64 = 1.0 / 298.257223563;

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
```

Six fixed iterations replace a convergence test, on the same error-budget grounds. The
`p / cos(lat)` altitude term loses precision only very close to the polar axis: the
altitude error is about 5 mm at 1 m from the axis.

### 5.5 Freshness versus usability

The subsystem treats "how recently was this updated" and "can this still be propagated"
as separate questions:

| Setting | Default | Measured against | Effect |
| --- | --- | --- | --- |
| `SATELLITE_STALE_AFTER_HOURS` | 24 h | `last_updated`, when a better record was last accepted | annotated `stale: true` in the API; still served |
| `SATELLITE_MAX_PROPAGATION_AGE_DAYS` | 30 d | `epoch`, the element set's own age | excluded from propagation |

The two come apart because SatNOGS republishes long-dead objects. At one point 221 objects
had epochs older than 30 days — the oldest from February 1975 — while reporting fresh,
because `last_updated` was minutes old. SGP4 given such an element set either diverges or,
worse, returns a position thousands of kilometres wrong. Nothing is deleted for age: an
unpropagatable object stays in the catalogue, is counted, and is left out of the
positions.

### 5.6 Serving positions

`GET /api/satellites` reads the in-memory catalogue (`Arc<RwLock<…>>`) and never the
SQLite connection, so polling cannot contend with other routes (`api/satellites.rs`). Per
request it propagates every matching object in a serial loop, after two filters that
cost nothing — the category filter and the epoch cutoff. Three measures keep that
affordable:

- **Response cache.** A 1,000 ms TTL (`SATELLITE_POSITION_CACHE_MS`, 0 disables), keyed
  on the sorted, comma-joined category filter and bounded at 32 keys. Each entry holds
  the JSON and a pre-gzipped copy, served according to `Accept-Encoding` with
  `Vary: accept-encoding`; once propagation was cached, compression had been 28 ms of a
  40 ms request. Two concurrent misses may both compute; the lock is not held across the
  sweep.
- **Coordinate rounding.** Four decimal places on latitude and longitude (about 11 m),
  three on altitude (about 1 m): 2.25 → 1.75 MiB raw, 0.65 → 0.36 MiB gzipped, and 39%
  less compression CPU.
- **Omitted defaults.** `stale` is serialised only when true.

The response reports `total`, `fresh_count`, `stale_count` and `unpropagatable_count`,
where the last three sum to the first. `category_counts` covers the whole catalogue
regardless of the filter, so the legend does not change as layers are toggled.

`GET /api/satellites/:norad_id/orbit` samples one full period centred on now, at 180
points, and splits the path wherever consecutive longitudes jump by more than 180°
(`satellites/orbit.rs`). An unknown NORAD ID returns 404; a known one whose epoch is past
the cutoff returns 422 rather than a fabricated path. `GET /api/satellites/status` reads
both the catalogue and SQLite, because its refresh history survives restarts only in the
database.

---

## 6. Durability

Replit rebuilds a published app's filesystem on every publish, and some of BLACKOUT's
data cannot be fetched again. Three fetchers pull bounded rolling windows — IODA 90 days,
RIPEstat 14 days, Cloudflare HTTP share 14 days — and never re-request older history, so
what has accumulated beyond those windows exists only in the database. The satellite
catalogue cannot be rebuilt on demand while CelesTrak is rate-limiting or unreachable.
The snapshot layer (`snapshot/`) makes the database survive redeploys. It is inert unless
`SNAPSHOT_KEY` is set and a Replit object-storage sidecar is found.

### 6.1 Taking a snapshot

A snapshot is `VACUUM INTO` a temporary file — a consistent, defragmented image that
includes anything still in the WAL — gzipped with `flate2` to roughly a quarter of the
live size and uploaded to Replit App Storage, all under `spawn_blocking`. `VACUUM INTO`
holds the connection for one to two seconds at 70 MB. Snapshots run every 180 minutes
(floored at 300 s) and once more on SIGTERM or SIGINT: the interval bounds what a crash
can lose, and the shutdown snapshot makes an ordinary redeploy lossless. Object writes are
atomic, so an interrupted upload leaves the previous snapshot intact.

Two checks precede each upload:

- **Dirty tracking.** The snapshot is skipped if the length and mtime of both the main
  file and the WAL are unchanged since the last upload. In WAL mode every committed write
  touches the `-wal` file, so this detects change without taking the lock.
- **Shrinkage guard.** A snapshot smaller than `SNAPSHOT_MIN_RETAIN_RATIO` (default 0.5)
  of the last good one is refused and logged, and retried on the next cycle.

### 6.2 Restoring at boot

Restore runs before the database is opened, because `Connection::open` would otherwise
create an empty file in its place. The rule it enforces is that **a failed restore must
never overwrite a good snapshot**: the failure sequence it guards against is a transient
network error at boot, an empty database, fetchers repopulating a fraction of it, and that
fraction being uploaded over the only good copy.

The rule is enforced structurally. `restore_if_absent` returns `Option<Snapshotter>`, and
the upload tasks are spawned only when it returns `Some`. A restore that errors returns
`None`, so no code path exists that could upload.

| Situation | Result | Uploads |
| --- | --- | --- |
| A database is already on disk | keep it, no download | enabled |
| Download succeeded | install it, seed the shrinkage baseline | enabled |
| Install failed | warn | disabled |
| Clean 404: nothing stored yet | treat as a first deployment | enabled |
| Any other transfer error | warn | disabled |

Separating a clean 404 from other errors is what makes the rule workable: a 404 treated
as failure would stop a first deployment from ever saving, and a transport error treated
as "nothing stored" would overwrite good data. A downloaded snapshot must carry the SQLite
header (`SQLite format 3\0`) after decompression, and is written to a temporary path,
`fsync`ed and renamed into place, so neither a truncated object nor an interrupted write
can land on the database path.

The storage sidecar is discovered with exponential backoff (400 ms doubling to 3 s) for
up to 10 s (`SNAPSHOT_SIDECAR_WAIT_SECS`), because its startup is not ordered against the
app's; a single early probe once missed it and that deployment started without restoring.

### 6.3 Verifying persistence

A restored database and a fresh one look identical once opened, so each boot increments a
counter stored in `satellite_refresh_state`. The counter can only be absent on a database
no process has booted against, so a redeploy that logs boot #1 has not restored. A healthy
redeploy logs `DB restored: … (boot #N)` with N > 1; anything else logs a `WARNING` with
remediation.

---

## 7. HTTP interface

Every mounted route is a GET, and no mounted handler writes; the deployment is public,
unauthenticated and read-only (`main.rs`).

| Path | Returns |
| --- | --- |
| `/health` | data freshness; 503 when stale (§3.2) |
| `/api/countries`, `/api/countries/:code` | the five researched dossiers; 404 for a country without one |
| `/api/geo` | reference rows for the globe (235; 249 with `?all`) |
| `/api/starlink-status`, `/api/cables`, `/api/ixp-stats` | infrastructure reference data |
| `/api/blocking`, `/api/categories`, `/api/timeline` | OONI results, by country and technology |
| `/api/tor-metrics`, `/api/outages` | Tor series; IODA outages by country, or active globally |
| `/api/http-protocol-share`, `/api/bgp-visibility` | per-country daily series, full accumulated history |
| `/api/country-scores`, `/api/rankings` | index scores by country; single-source leaderboard |
| `/api/censorship-index` | the composite, computed per request (§4) |
| `/api/signals`, `/api/models` | adoption signals and model reference data (not used by the UI) |
| `/api/satellites`, `/api/satellites/status`, `/api/satellites/:norad_id/orbit` | positions, catalogue status, orbit path (§5.6) |
| `/api/methodology`, `/api/methodology/:slug` | these documents, rendered to HTML |

Request parameters are bound into SQL, with one exception: `order` in `/api/rankings`
is whitelisted to `ASC` or `DESC` before interpolation. `limit` there is clamped to
1–500. The methodology documents are compiled into the binary with `include_str!` and
rendered once with `comrak`; each table of contents comes from the document's headings.

A `CompressionLayer` (gzip, not brotli, whose cost is less predictable on two vCPUs) wraps
every route and the static bundle, which is 13.7 MB, mostly Cesium. It skips bodies under
32 bytes and images other than SVG, and passes through the satellite response, which is
already compressed. The static frontend is served by `ServeDir` with an `index.html`
fallback, so deep links resolve to the app. There is no CORS layer, because the app and
API share an origin. Logging is `println!`/`eprintln!` to standard output, with a
`WARNING:` prefix for conditions that need attention.

---

## 8. Frontend

React 18 and CesiumJS (`cesium ^1.121`, `react ^18.3`, `recharts ^2.12`,
`topojson-client ^3.1`, `world-atlas ^2.0`), built by Vite and served by the same binary.
`vite-plugin-cesium` copies Cesium's workers and assets into the bundle.

### 8.1 Globe construction and geometry

The Cesium viewer is created with every widget disabled and `baseLayer: false`, so no
imagery is requested from Cesium Ion. The basemap is `world-atlas/countries-50m.json`.
Its features carry ISO 3166-1 numeric ids while the database keys on alpha-2, so
`country_reference` carries `iso_numeric` as the join key (null only for Kosovo, which
has no ISO code). Centroids are those of each country's largest landmass, and bounding
boxes, which span every landmass, give a fly-to altitude without a geometry index; both
are generated from the same basemap the globe draws. Borders come from `topojson.mesh`,
omitting the internal border between Morocco (504) and Western Sahara (732) to match the
data model.

### 8.2 Rendering

Bulk layers are batched Cesium primitives rather than entities, separated by height so
they cannot z-fight:

| Layer | Height | Construct |
| --- | ---: | --- |
| Land underlay | 250 m | one `Primitive`, built asynchronously so tessellation does not block first paint |
| Choropleth | 600 m | one translucent `Primitive`, `allowPicking: false` |
| Borders | 2,000 m | one `PolylineCollection` |
| Cable routes | 2,500 m | one `PolylineCollection` |
| Blooms | 3,000 m | entities (bounded count) |
| Satellites | compressed altitude | one `PointPrimitiveCollection` |

Clicks fall through the choropleth to the land polygons, whose instance id is the country's
alpha-2 code; blooms carry the same code in their properties. Picking tells targets apart
by type — countries have string ids, satellites numeric NORAD ids — so the two cannot
collide. Only outer polygon rings are drawn.

The choropleth colour is driven by `censorship_score` from `/api/censorship-index`: a
two-segment interpolation from green through amber to crimson, at alpha 0.55. Countries
without a score are not filled.

Blooms mark only `CONFIRMED_BLOCKED` and `LIKELY_BLOCKED` technologies; everything else
is carried by the choropleth. Their radius scales with the country's index from 130 to
390 km, with likely-blocked blooms at 0.62 of that. Outage blooms step with IODA severity:
340 km at a score of at least 200, 240 km at 60 or more, otherwise 160 km. Textures are
radial-gradient canvases cached by kind and colour. The only animation is the outage
pulse, which varies alpha alone (`0.3 + 0.55 · pulse`), so nothing is re-tessellated per
frame.

### 8.3 Satellites

All satellites share one `PointPrimitiveCollection` — a single draw call — rebuilt on
each 7-second poll. Rendered altitude is compressed: a square-root curve maps 0–42,000 km
onto 150–3,000 km so that low-earth and geostationary orbits are legible on one globe.
The altitude shown in the satellite card, like the API's `alt_km`, is the true value.
Orbit paths are fetched only for the selected satellite, drawn from the backend's
pre-split segments, and skipped for the `geo` category, whose paths sample to an analemma
rather than a track.

### 8.4 Display rules

The API client is plain `fetch`; `getCountry` treats 404 as a meaningful null, since most
countries have no researched dossier. Two rules in the country sidebar
(`components/CountrySidebar.jsx`) apply the same principles as the backend:

- **A seven-day minimum for timeline evidence.** A technology needs at least seven days
  of timeline data to count as a signal on its own; a single isolated OONI day once
  produced a false positive for Iran.
- **Loading is distinct from empty.** A row is settled only once both the blocking and
  timeline requests have returned, since either can change it.

---

## 9. Verification

`cargo test` runs 80 backend tests. Half cover the satellite subsystem (merge
precedence, persistence, partial-response detection, SGP4 validation, the
propagation-age cutoff, orbit sampling, rounding); the rest cover the snapshot layer,
the OONI, RIPEstat and Cloudflare fetchers, date arithmetic, outbound identity, the
methodology renderer and the dormant engine. Tests target the properties rather than
the implementation:

- **Snapshot tests drive a real HTTP server** mimicking the Replit sidecar and object
  storage, because "a round trip reproduces the database" and "a failed download never
  leads to an upload" are properties of the HTTP paths. One asserts the stored bytes are
  unchanged after a broken boot; another deletes 3,800 of 4,000 rows and asserts the
  upload is refused and the stored object untouched.
- **The identity test asserts on the wire**, checking that the `User-Agent` arrives at a
  live server rather than that a function returns the right string.
- **The staleness test uses a real observed object**, ONDOSAT-OWL-9: accepted twenty
  minutes earlier, with an epoch 642 days old, and still refused.

The guarantees described in this document, and where each is enforced:

| Guarantee | Mechanism |
| --- | --- |
| A failed restore cannot overwrite a good snapshot | `Option<Snapshotter>`: no uploader exists without a successful restore (§6.2) |
| A truncated response cannot shrink the satellite catalogue | no `DELETE` in the merge path (§5.2) |
| A partial group response cannot lower expectations | merged, but not accepted as a baseline (§5.3) |
| A broken element set cannot displace a working one | `Constants::from_elements` checked before merge (§5.1) |
| A collapsed snapshot cannot replace a good one | retain-ratio guard (§6.1) |
| A corrupt snapshot cannot reach `Connection::open` | SQLite header checked before install (§6.2) |
| A half-written file cannot land on the database path | temp file, `fsync`, `rename` (§6.2) |
| A partial seed cannot be committed | one transaction with rollback (§2.3) |
| A dossier cannot exist without geometry | boot-time reference check (§2.3) |
| A failed fetch cannot advance `last_success_at` | separate attempt and success columns (§3.2) |
| A lost database cannot pass as a restored one | monotonic boot counter (§6.3) |
| Absence cannot render as zero | `None` and `INCONCLUSIVE` states, omitted entries, 422 rather than a fabricated orbit (§3, §4, §5) |

---

## 10. Build and deployment

The build is defined in `.replit`:

```sh
rustup toolchain install 1.91.0 --profile minimal --no-self-update
  && rustup default 1.91.0 && cargo --version
  && cd frontend && npm ci && npm run build
  && cd ../backend && cargo build --release --locked
  && cd .. && mkdir -p bin && cp backend/target/release/backend bin/blackout
  && test -x bin/blackout && test -f frontend/dist/index.html
```

The toolchain is pinned through `rustup` because the Nix channel's Cargo (1.77.1)
predates the crate's `edition = "2024"`, and it installs first so a toolchain failure
surfaces before the frontend build. The binary is copied into `bin/` because
`backend/target/` is excluded from what the build stage hands to the run stage.
`.replitignore` also excludes `*.db`, so a local database can never ship and take
precedence over the stored snapshot. Both lockfiles are committed, and the backend builds
with `--locked`.

`replit.nix` provides `rustup`, `nodejs_22`, `pkg-config` and `openssl` (for reqwest's
native TLS), `stdenv.cc` (rusqlite compiles SQLite from source) and `cacert`. The backend
has fourteen runtime crates; `flate2` and `comrak` are built without system libraries, so
nothing further needs installing.

Configuration is by environment variable:

| Variable | Default | Effect |
| --- | --- | --- |
| `DEPLOYMENT_ID` | `local` | User-Agent and RIPEstat `sourceapp` |
| `DEPLOYMENT_CONTACT` | project URL | contact in the User-Agent |
| `CLOUDFLARE_API_TOKEN` | unset | Radar annotations and HTTP version share |
| `PULSE_API_TOKEN` | unset | Internet Society Pulse resilience scores |
| `PEERINGDB_API_KEY` | unset | generator script only |
| `DATABASE_PATH` | `mena_ai.db` | SQLite location |
| `SEED_DIR` | `data/seed` | seed files |
| `STATIC_DIR` | `../frontend/dist` | frontend bundle |
| `PORT` | `3001` | listener |
| `HEALTH_MAX_AGE_DAYS` | `2` | `/health` staleness threshold |
| `FETCH_INTERVAL_HOURS` | `6` | general loop |
| `PRECISION_FETCH_INTERVAL_HOURS` | `1` | precision loop |
| `SATELLITE_CATALOG_REFRESH_HOURS` | `2` | catalogue loop |
| `SATELLITE_GROUPS` | 11-group list | CelesTrak groups and categories, in precedence order |
| `SATELLITE_GROUP_MIN_RETAIN_RATIO` | `0.5` | per-group shrinkage guard |
| `SATELLITE_STALE_AFTER_HOURS` | `24` | staleness annotation |
| `SATELLITE_MAX_PROPAGATION_AGE_DAYS` | `30` | propagation cutoff |
| `SATELLITE_POSITION_CACHE_MS` | `1000` | position cache TTL; `0` disables |
| `SNAPSHOT_KEY` | unset | enables the snapshot layer |
| `SNAPSHOT_INTERVAL_MINUTES` | `180` | snapshot interval, floored at 300 s |
| `SNAPSHOT_MIN_RETAIN_RATIO` | `0.5` | snapshot shrinkage guard; values outside [0, 1] use the default |
| `SNAPSHOT_SIDECAR_WAIT_SECS` | `10` | sidecar discovery budget; `0` means one attempt |
| `RIPESTAT_SOURCEAPP` | derived | `blackout-<DEPLOYMENT_ID>` |
| `VITE_CESIUM_ION_TOKEN` | unset | frontend, build time; optional |

The app starts without any of the credentials. Missing `CLOUDFLARE_API_TOKEN` or
`PULSE_API_TOKEN` is reported at boot, naming the data each one gates.

---

## 11. Limitations

These follow from the design choices above and bound what the implementation should be
expected to do.

- **Throughput.** One SQLite connection serialises every database-backed request, and
  handlers wait on it synchronously, so a `VACUUM INTO` of one to two seconds delays them
  and can occupy both runtime workers. This suits a single read-mostly deployment. Higher
  concurrency would need a connection pool.
- **Orbital accuracy.** Positions carry SGP4's error of about a kilometre and are only as
  current as each element set's epoch, within the 30-day cutoff. Rendered altitude is
  compressed for legibility and is not to scale.
- **Index comparability.** Composite scores are comparable only between countries with
  the same `component_count`, and component years can differ between countries (§4,
  §3.10).
- **Reference data.** Seeded files (country geometry, cables, IXP counts, Starlink status)
  change only when regenerated or edited by hand; Starlink status is curated manually.
