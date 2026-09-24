// Generates data/seed/country_reference.json — the single source of truth for
// which countries exist, what their ISO codes are, and where to put a marker.
//
// This is a ONE-TIME / on-demand generator, not part of the build. Its output is
// committed. Run it again only when the ISO list or the basemap changes:
//
//   node backend/scripts/gen_country_reference.mjs
//
// Inputs:
//   1. frontend/node_modules/world-atlas/countries-50m.json — the SAME basemap
//      the globe renders, so a marker can never disagree with its own borders.
//      Provides ISO-numeric ids, short display names, and geometry.
//   2. ISO 3166-1 (fetched, see ISO_URL) — provides alpha-2/alpha-3, the
//      numeric->alpha-2 join, and UN M49 region/sub-region.
//
// The basemap has no alpha-2 codes and the ISO list has no geometry, which is
// why both are needed.

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { headers } from './identity.mjs'
import * as topojson from '../../frontend/node_modules/topojson-client/dist/topojson-client.js'

const HERE = dirname(fileURLToPath(import.meta.url))
const REPO = resolve(HERE, '../..')
const ATLAS = resolve(REPO, 'frontend/node_modules/world-atlas/countries-50m.json')
const OUT = resolve(REPO, 'backend/data/seed/country_reference.json')
const ISO_URL =
  'https://raw.githubusercontent.com/lukes/ISO-3166-Countries-with-Regional-Codes/master/all/all.json'

// Countries the project actively researches; they sort first in every fetch
// cycle so their data stays freshest.
const FOCUS = new Set(['IR', 'SY', 'AE', 'SA', 'IQ'])

// Treat Western Sahara as Morocco in this app: the standalone EH feed is too
// sparse to be useful, and the frontend aliases its atlas polygon to MA.
const EXCLUDED_CODES = new Set(['EH'])

// The basemap carries 5 features with no ISO numeric id, because they are
// territories ISO 3166-1 does not assign a code to. Only Kosovo gets a code
// here — XK is the user-assigned code in de facto standard use (and what the
// EU, IMF and World Bank publish under). The rest are intentionally dropped:
// inventing codes for disputed territories would put fabricated identifiers
// into the join key that everything else is matched on.
const NO_ID_FEATURES = {
  Kosovo: { code: 'XK', alpha3: 'XKX', region: 'Europe', subregion: 'Southern Europe' },
  Somaliland: null,
  'N. Cyprus': null,
  'Indian Ocean Ter.': null,
  'Siachen Glacier': null,
}

// ── Geometry ───────────────────────────────────────────────────────────────

// A ring that crosses the antimeridian arrives with longitudes at both ends of
// the range, so its raw bbox spans the whole planet and its centroid lands in
// the wrong hemisphere. Affects Russia, Fiji and Antarctica. Detect by
// longitude span and re-measure in a 0..360 frame.
function unwrapIfNeeded(ring) {
  const lons = ring.map(([lon]) => lon)
  if (Math.max(...lons) - Math.min(...lons) <= 180) return { ring, shifted: false }
  return { ring: ring.map(([lon, lat]) => [lon < 0 ? lon + 360 : lon, lat]), shifted: true }
}

function normalizeLon(lon) {
  return lon > 180 ? lon - 360 : lon
}

// Signed area and area-weighted centroid of a single ring, in degrees. Good
// enough for a label point — this is not an equal-area projection and makes no
// attempt to be. `area` is used only to compare rings against each other.
function ringCentroid(ring) {
  let twiceArea = 0
  let cx = 0
  let cy = 0
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [x0, y0] = ring[j]
    const [x1, y1] = ring[i]
    const f = x0 * y1 - x1 * y0
    twiceArea += f
    cx += (x0 + x1) * f
    cy += (y0 + y1) * f
  }
  if (twiceArea === 0) {
    // Degenerate ring: fall back to the mean vertex.
    const n = ring.length
    return {
      area: 0,
      lon: ring.reduce((s, p) => s + p[0], 0) / n,
      lat: ring.reduce((s, p) => s + p[1], 0) / n,
    }
  }
  return { area: Math.abs(twiceArea / 2), lon: cx / (3 * twiceArea), lat: cy / (3 * twiceArea) }
}

function outerRings(geometry) {
  if (geometry.type === 'Polygon') return [geometry.coordinates[0]]
  if (geometry.type === 'MultiPolygon') return geometry.coordinates.map((poly) => poly[0])
  return []
}

// Marker position is the centroid of the LARGEST landmass, not of all landmasses
// averaged — averaging puts the United States in the Pacific (pulled by Alaska
// and Hawaii) and Norway in the Arctic (pulled by Svalbard). The bbox, by
// contrast, spans every ring, because it is used for zoom framing.
function geometryStats(geometry) {
  const rings = outerRings(geometry)
  if (rings.length === 0) return null

  let best = null
  let minLon = Infinity
  let minLat = Infinity
  let maxLon = -Infinity
  let maxLat = -Infinity

  for (const raw of rings) {
    const { ring, shifted } = unwrapIfNeeded(raw)
    const c = ringCentroid(ring)
    if (!best || c.area > best.area) {
      best = { area: c.area, lon: shifted ? normalizeLon(c.lon) : c.lon, lat: c.lat }
    }
    for (const [lon, lat] of ring) {
      const l = shifted ? normalizeLon(lon) : lon
      if (l < minLon) minLon = l
      if (l > maxLon) maxLon = l
      if (lat < minLat) minLat = lat
      if (lat > maxLat) maxLat = lat
    }
  }

  const round = (n) => Math.round(n * 100) / 100
  return {
    centroid_lat: round(best.lat),
    centroid_lon: round(best.lon),
    bbox_min_lon: round(minLon),
    bbox_min_lat: round(minLat),
    bbox_max_lon: round(maxLon),
    bbox_max_lat: round(maxLat),
  }
}

// ── Build ──────────────────────────────────────────────────────────────────

const iso = await fetch(ISO_URL, { headers: headers('country-reference') }).then((r) => {
  if (!r.ok) throw new Error(`ISO 3166 fetch failed: ${r.status}`)
  return r.json()
})
if (iso.length < 240) throw new Error(`ISO 3166 list looks truncated: ${iso.length} entries`)

const topology = JSON.parse(readFileSync(ATLAS, 'utf8'))
const features = topojson.feature(topology, topology.objects.countries).features

// numeric id -> geometry stats. One numeric id can own more than one feature
// (036 is both Australia and Ashmore & Cartier Is.), so keep the largest.
const geoByNumeric = new Map()
const geoByName = new Map()
for (const f of features) {
  const stats = geometryStats(f.geometry)
  if (!stats) continue
  const entry = { ...stats, name: f.properties.name }
  geoByName.set(f.properties.name, entry)
  if (!f.id) continue
  const key = String(f.id).padStart(3, '0')
  const prev = geoByNumeric.get(key)
  const span = (s) => (s.bbox_max_lon - s.bbox_min_lon) * (s.bbox_max_lat - s.bbox_min_lat)
  if (!prev || span(stats) > span(prev)) geoByNumeric.set(key, entry)
}

const rows = []

for (const c of iso) {
  const code = c['alpha-2']
  if (EXCLUDED_CODES.has(code)) continue
  const numeric = String(c['country-code']).padStart(3, '0')
  const geo = geoByNumeric.get(numeric)
  rows.push({
    country_code: code,
    // Prefer the basemap's short display name ("Iran") over the ISO legal name
    // ("Iran, Islamic Republic of"): it is what the globe labels and what the
    // existing researched rows in countries.json already use.
    country_name: geo?.name ?? c.name,
    alpha3: c['alpha-3'],
    iso_numeric: numeric,
    region: c.region || null,
    subregion: c['sub-region'] || null,
    centroid_lat: geo?.centroid_lat ?? null,
    centroid_lon: geo?.centroid_lon ?? null,
    bbox_min_lon: geo?.bbox_min_lon ?? null,
    bbox_min_lat: geo?.bbox_min_lat ?? null,
    bbox_max_lon: geo?.bbox_max_lon ?? null,
    bbox_max_lat: geo?.bbox_max_lat ?? null,
    // Drawable on the globe = the basemap has geometry for it. This is
    // deliberately derived rather than a hand-maintained "is UN member" flag:
    // it is verifiable from the data, and it is the property the globe actually
    // needs. Small ISO entries with no 50m geometry (BV, HM, UM, ...) fall out.
    include_on_globe: geo ? 1 : 0,
    priority_tier: FOCUS.has(code) ? 0 : 100,
  })
}

// Kosovo and friends: present on the basemap, absent from ISO 3166-1.
for (const [name, spec] of Object.entries(NO_ID_FEATURES)) {
  if (!spec) continue
  const geo = geoByName.get(name)
  if (!geo) throw new Error(`expected basemap feature "${name}" not found`)
  rows.push({
    country_code: spec.code,
    country_name: name,
    alpha3: spec.alpha3,
    iso_numeric: null,
    region: spec.region,
    subregion: spec.subregion,
    centroid_lat: geo.centroid_lat,
    centroid_lon: geo.centroid_lon,
    bbox_min_lon: geo.bbox_min_lon,
    bbox_min_lat: geo.bbox_min_lat,
    bbox_max_lon: geo.bbox_max_lon,
    bbox_max_lat: geo.bbox_max_lat,
    include_on_globe: 1,
    priority_tier: 100,
  })
}

rows.sort((a, b) => a.country_code.localeCompare(b.country_code))

const dupes = rows.length - new Set(rows.map((r) => r.country_code)).size
if (dupes > 0) throw new Error(`${dupes} duplicate country_code rows`)
for (const code of FOCUS) {
  const r = rows.find((x) => x.country_code === code)
  if (!r) throw new Error(`focus country ${code} missing`)
  if (r.centroid_lat === null) throw new Error(`focus country ${code} has no centroid`)
}

writeFileSync(OUT, `${JSON.stringify(rows, null, 2)}\n`)

const drawable = rows.filter((r) => r.include_on_globe === 1).length
console.log(`wrote ${OUT}`)
console.log(`  ${rows.length} rows, ${drawable} drawable on the globe`)
console.log(`  ${rows.length - drawable} ISO codes with no 50m geometry`)
