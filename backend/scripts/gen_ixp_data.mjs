// Generates data/seed/ixp_stats.json from PeeringDB's free, unauthenticated
// Internet Exchange Point directory — a per-country count of domestic
// interconnection points, the structural counterpart to the BGP-collapse
// signal: a country with almost no domestic IXPs has to route nearly all its
// traffic through a handful of international gateways, which is what makes a
// full national shutdown fast and cheap for a government to pull off.
//
// This is a ONE-TIME / on-demand generator, not part of the build. Its output
// is committed. Run it again only when IXP data has moved meaningfully (this
// changes on the order of months/years, not something to re-run on a
// schedule):
//
//   node --env-file=.env backend/scripts/gen_ixp_data.mjs   (from the repo root)
//   node --env-file=.env scripts/gen_ixp_data.mjs           (from backend/)
//
// Input: https://www.peeringdb.com/api/ix — returns every IXP worldwide
// (1,323 as of this writing) in a single page, no pagination to handle
// (checked with and without `limit`, same count both times). IMPORTANT:
// PeeringDB throttles UNAUTHENTICATED requests hard — a third quick request
// in the same short window came back
// `"Request was throttled. Expected available in 59 minutes."`, with no
// documented recovery-time guarantee (PeeringDB's own docs don't say whether
// that window is fixed or slides forward on every subsequent request). A
// `PEERINGDB_API_KEY` in `.env` (a free, user-level, read-only key from a
// PeeringDB account's profile page) raises this limit substantially and is
// sent below when present; the request still works without one, just subject
// to that same throttle. This is also why this is a one-time script
// committing static seed data rather than a live in-app fetcher: a periodic
// fetch loop hitting this from a shared production IP has no reason to run
// that risk when the underlying data doesn't move fast enough to justify
// polling anyway. Only ONE request is made below; do not add a per-country loop.
//
// Country resolution: PeeringDB's `country` field is already a clean ISO
// 3166-1 alpha-2 code (spot-checked live: IR, US, DE, KE, RU, CN, NG all
// resolve directly) — unlike the submarine-cable API, there is no free-text
// name to reconcile against country_reference.json. Every distinct code is
// still cross-checked against the committed country_reference.json and any
// mismatch is logged rather than assumed clean.

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { headers } from './identity.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const REPO = resolve(HERE, '../..')
const COUNTRY_REFERENCE = resolve(REPO, 'backend/data/seed/country_reference.json')
const OUT_IXP = resolve(REPO, 'backend/data/seed/ixp_stats.json')

const IX_URL = 'https://www.peeringdb.com/api/ix'

const countryReference = JSON.parse(readFileSync(COUNTRY_REFERENCE, 'utf8'))
const knownCodes = new Set(countryReference.map((r) => r.country_code))

const apiKey = process.env.PEERINGDB_API_KEY?.trim()
const requestHeaders = headers('peeringdb', apiKey ? { Authorization: `Api-Key ${apiKey}` } : {})
if (!apiKey) {
  console.warn('PEERINGDB_API_KEY not set — falling back to the unauthenticated (heavily throttled) limit.')
}
console.log(`Identifying as ${requestHeaders['User-Agent']}`)

const body = await fetch(IX_URL, { headers: requestHeaders }).then((r) => {
  if (!r.ok) throw new Error(`PeeringDB /api/ix fetch failed: ${r.status} — ${r.statusText}`)
  return r.json()
})
const rows = body.data
if (!Array.isArray(rows) || rows.length < 500) {
  throw new Error(`/api/ix looks truncated or throttled: ${JSON.stringify(body).slice(0, 300)}`)
}

// PeeringDB's own documentation fixture, permanently present in the live
// data under country AQ (Antarctica) — its `notes` field says outright "This
// is an example of a IX object ... It does not represent a real IX providing
// actual services." Confirmed by inspecting every row whose name/notes
// mention "example": this is the only genuine placeholder (id 4095); a few
// real exchanges — e.g. ERA-IX, PANDA-IX — happen to use the word "example"
// in unrelated marketing/technical copy, so this is excluded by its specific
// id rather than a text match that would wrongly catch those too.
const PEERINGDB_EXAMPLE_IX_ID = 4095

const byCountry = new Map()
const unknownCodes = new Set()
for (const ix of rows) {
  if (ix.id === PEERINGDB_EXAMPLE_IX_ID) continue
  const code = ix.country
  if (!code) continue // a handful of rows carry no country at all — skip, not a resolvable name
  if (!knownCodes.has(code)) unknownCodes.add(code)

  const entry = byCountry.get(code) ?? {
    country_code: code,
    ixp_count: 0,
    total_net_count: 0,
    largest_ixp_name: '',
    largest_ixp_net_count: -1,
  }
  entry.ixp_count += 1
  entry.total_net_count += ix.net_count ?? 0
  if ((ix.net_count ?? 0) > entry.largest_ixp_net_count) {
    entry.largest_ixp_name = ix.name
    entry.largest_ixp_net_count = ix.net_count ?? 0
  }
  byCountry.set(code, entry)
}

const today = new Date().toISOString().slice(0, 10)
const stats = [...byCountry.values()]
  .map((e) => ({ ...e, generated_at: today }))
  .sort((a, b) => a.country_code.localeCompare(b.country_code))

writeFileSync(OUT_IXP, `${JSON.stringify(stats, null, 2)}\n`)

console.log(`wrote ${OUT_IXP}`)
console.log(`  ${rows.length} IXPs across ${stats.length} countries`)
if (unknownCodes.size > 0) {
  console.warn('Country codes not found in country_reference.json (review before committing):')
  for (const code of unknownCodes) console.warn(`  - ${code}`)
} else {
  console.log('  every country code matched country_reference.json')
}
