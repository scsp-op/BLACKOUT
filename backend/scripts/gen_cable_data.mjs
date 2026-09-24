// Generates data/seed/cable_routes.json and data/seed/cable_landing_points.json
// from TeleGeography's free, unauthenticated submarine cable map API.
//
// This is a ONE-TIME / on-demand generator, not part of the build. Its output
// is committed. Run it again only when the cable map data changes materially
// (this is physical infrastructure — it moves on the order of months/years,
// not something to re-run on a schedule):
//
//   node backend/scripts/gen_cable_data.mjs
//
// Inputs (both live, confirmed unauthenticated, no documented rate limit):
//   https://www.submarinecablemap.com/api/v3/cable/cable-geo.json
//   https://www.submarinecablemap.com/api/v3/landing-point/landing-point-geo.json
//
// Country resolution: landing-point `name` is reliably "City, Country" (or,
// less often, "City, Region, Country" — e.g. "Kalemie, Congo, Dem. Rep."),
// but TeleGeography's country strings frequently don't match this project's
// `country_reference.json` `country_name` values verbatim (that file uses
// the world-atlas basemap's abbreviated display names). Resolution tries the
// LONGEST trailing comma-segment match first (so "Congo, Dem. Rep." is tried
// before the too-generic "Dem. Rep." alone), against a combined alias table
// + the real committed country_reference.json — never a fuzzy/partial match.
// A landing point that still doesn't resolve keeps country_code: null and is
// logged, never dropped (the geometry itself isn't malformed, only the
// country join failed, and v1 has no per-country filtering that would need it).

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { headers } from './identity.mjs'

const HERE = dirname(fileURLToPath(import.meta.url))
const REPO = resolve(HERE, '../..')
const COUNTRY_REFERENCE = resolve(REPO, 'backend/data/seed/country_reference.json')
const OUT_ROUTES = resolve(REPO, 'backend/data/seed/cable_routes.json')
const OUT_LANDING = resolve(REPO, 'backend/data/seed/cable_landing_points.json')

const CABLE_URL = 'https://www.submarinecablemap.com/api/v3/cable/cable-geo.json'
const LANDING_URL = 'https://www.submarinecablemap.com/api/v3/landing-point/landing-point-geo.json'

// TeleGeography country string -> this project's ISO alpha-2 code, for every
// case where it doesn't match `country_reference.json`'s `country_name`
// verbatim. Confirmed against the live landing-point dataset and the actual
// committed country_reference.json (not assumed) — see the plan doc for the
// full cross-check. The two "Congo, ..." entries exist because
// country_reference.json's own names ("Congo", "Dem. Rep. Congo") are too
// short to appear as a trailing match once a region segment sits between the
// city and the country in a small number of landing-point names.
const ALIASES = new Map([
  ['United States', 'US'],
  ['Cape Verde', 'CV'],
  ['French Polynesia', 'PF'],
  ['Virgin Islands (U.S.)', 'VI'],
  ['Virgin Islands (U.K.)', 'VG'],
  ['Northern Mariana Islands', 'MP'],
  ['Dominican Republic', 'DO'],
  ['Solomon Islands', 'SB'],
  ['Equatorial Guinea', 'GQ'],
  ['Saint Martin', 'MF'],
  ['Saint Barthélemy', 'BL'],
  ['Antigua and Barbuda', 'AG'],
  ['Wallis and Futuna', 'WF'],
  ['Saint Pierre and Miquelon', 'PM'],
  ['Faroe Islands', 'FO'],
  ['Marshall Islands', 'MH'],
  ['Cayman Islands', 'KY'],
  ['Saint Kitts and Nevis', 'KN'],
  ['Cook Islands', 'CK'],
  ['Turks and Caicos Islands', 'TC'],
  ['British Indian Ocean Territory', 'IO'],
  ['Sao Tome and Principe', 'ST'],
  ['Ascension and Tristan da Cunha', 'SH'], // many-to-one: folds into Saint Helena
  ['Sint Eustatius and Saba', 'BQ'], // -> Bonaire, Sint Eustatius and Saba
  ['Congo, Dem. Rep.', 'CD'],
  ['Congo, Rep.', 'CG'],
  ['Saint Vincent and the Grenadines', 'VC'],
])

// Tries the longest trailing comma-segment match first (e.g. for "Kalemie,
// Congo, Dem. Rep." tries "Congo, Dem. Rep." before falling back to the
// too-generic "Dem. Rep." alone), against the alias table then the real
// country_reference.json names. Returns null (never a guess) if nothing matches.
function resolveCountryCode(name, byName) {
  const parts = name.split(',').map((s) => s.trim())
  for (let take = parts.length - 1; take >= 1; take--) {
    const candidate = parts.slice(parts.length - take).join(', ')
    const code = ALIASES.get(candidate) ?? byName.get(candidate)
    if (code) return code
  }
  return null
}

const countryReference = JSON.parse(readFileSync(COUNTRY_REFERENCE, 'utf8'))
const byName = new Map(countryReference.map((r) => [r.country_name, r.country_code]))

const cableGeo = await fetch(CABLE_URL, { headers: headers('telegeography') }).then((r) => {
  if (!r.ok) throw new Error(`cable-geo.json fetch failed: ${r.status}`)
  return r.json()
})
const landingGeo = await fetch(LANDING_URL, { headers: headers('telegeography') }).then((r) => {
  if (!r.ok) throw new Error(`landing-point-geo.json fetch failed: ${r.status}`)
  return r.json()
})

if (cableGeo.features.length < 500) {
  throw new Error(`cable-geo.json looks truncated: ${cableGeo.features.length} features`)
}
if (landingGeo.features.length < 1000) {
  throw new Error(`landing-point-geo.json looks truncated: ${landingGeo.features.length} features`)
}

// `id` (cable identifier) is NOT the same as `feature_id` (true per-feature
// primary key) — a single logical cable can legitimately span two features
// sharing one `id` (confirmed live: 20 of 707 unique ids do this). Each
// feature's own geometry is already a MultiLineString (array of linestrings);
// preserved as `segments` as-is, never flattened into one linestring — that
// would risk drawing a spurious line across the globe wherever a route (or,
// per TeleGeography's own splitting, an antimeridian crossing) is meant to be
// a break, not a continuous path. Same problem/fix as
// backend/src/satellites/orbit.rs's antimeridian handling for orbit paths.
const routes = cableGeo.features.map((f) => ({
  id: f.properties.id,
  feature_id: f.properties.feature_id,
  name: f.properties.name,
  color: f.properties.color,
  segments: f.geometry.coordinates,
}))

const unresolved = [];
const landingPoints = landingGeo.features.map((f) => {
  const countryCode = resolveCountryCode(f.properties.name, byName)
  if (!countryCode) unresolved.push(f.properties.name)
  return {
    id: f.properties.id,
    name: f.properties.name,
    country_code: countryCode,
    is_tbd: f.properties.is_tbd,
    lon: f.geometry.coordinates[0],
    lat: f.geometry.coordinates[1],
  }
})

writeFileSync(OUT_ROUTES, `${JSON.stringify(routes, null, 2)}\n`)
writeFileSync(OUT_LANDING, `${JSON.stringify(landingPoints, null, 2)}\n`)

console.log(`wrote ${OUT_ROUTES}`)
console.log(`  ${routes.length} cable features (${new Set(routes.map((r) => r.id)).size} unique cable ids)`)
console.log(`wrote ${OUT_LANDING}`)
console.log(`  ${landingPoints.length} landing points, ${unresolved.length} unresolved country names`)
if (unresolved.length > 0) {
  console.warn('Unresolved country names (kept with country_code: null, review before committing):')
  for (const name of new Set(unresolved)) console.warn(`  - ${name}`)
}
