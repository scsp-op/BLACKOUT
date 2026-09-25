const BASE = '/api'

export async function getCountries() {
  const r = await fetch(`${BASE}/countries`)
  if (!r.ok) throw new Error('Failed to fetch countries')
  return r.json()
}

export async function getBlocking() {
  const r = await fetch(`${BASE}/blocking`)
  if (!r.ok) throw new Error('Failed to fetch blocking status')
  return r.json()
}

// 404 here means "no dossier row for this country" (only a handful of
// countries have one — see backend/src/api/countries.rs's `get_country`),
// not a failure: App.jsx's caller falls back to the lighter country_reference
// stub for every other country, so a 404 is an expected, silent null rather
// than a thrown error that would surface a misleading "failed to fetch" toast
// on nearly every country click. A genuine failure (network/5xx) still throws.
export async function getCountry(code) {
  const r = await fetch(`${BASE}/countries/${code}`)
  if (r.status === 404) return null
  if (!r.ok) throw new Error(`Failed to fetch country ${code}`)
  return r.json()
}

export async function evaluate(countryCode, sensitivity, orgType) {
  const r = await fetch(`${BASE}/evaluate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      country_code: countryCode,
      sensitivity,
      org_type: orgType,
    }),
  })
  if (!r.ok) throw new Error('Evaluate failed')
  return r.json()
}

export async function getSignals(countryCode) {
  const r = await fetch(`${BASE}/signals?country=${countryCode}`)
  if (!r.ok) throw new Error('Failed to fetch signals')
  return r.json()
}

// Country identity + map geometry (centroids) for every drawable country.
// Used to place the global internet-outage overlay, which can light up any
// country, not just the researched five.
export async function getGeo() {
  const r = await fetch(`${BASE}/geo`)
  if (!r.ok) throw new Error('Failed to fetch geo')
  return r.json()
}

// Hand-curated Starlink legal/regulatory status per country (~25 rows, not a
// live feed — see backend/data/seed/starlink_status.json). Fetched once, like
// getGeo(); absence of a country in the list means no known restriction.
export async function getStarlinkStatus() {
  const r = await fetch(`${BASE}/starlink-status`)
  if (!r.ok) throw new Error('Failed to fetch Starlink status')
  return r.json()
}

// Submarine cable routes + landing points from TeleGeography, generated once
// by backend/scripts/gen_cable_data.mjs — static infrastructure data, not a
// live feed. Fetched once regardless of the layer's toggle state, matching
// getGeo()'s eager-fetch convention.
export async function getCables() {
  const r = await fetch(`${BASE}/cables`)
  if (!r.ok) throw new Error('Failed to fetch cables')
  return r.json()
}

// Per-country Internet Exchange Point density from PeeringDB, generated once
// by backend/scripts/gen_ixp_data.mjs — static infrastructure data, not a
// live feed. Fetched once on mount, like getStarlinkStatus()/getCables().
export async function getIxpStats() {
  const r = await fetch(`${BASE}/ixp-stats`)
  if (!r.ok) throw new Error('Failed to fetch IXP stats')
  return r.json()
}

// IODA internet-outage events. `active` restricts to outages that ended
// within the backend's recent-activity grace window (the "live" set);
// `country` restricts to one country's history.
export async function getOutages({ country, active } = {}) {
  const params = new URLSearchParams()
  if (country) params.set('country', country)
  if (active) params.set('active', 'true')
  const qs = params.toString()
  const r = await fetch(`${BASE}/outages${qs ? `?${qs}` : ''}`)
  if (!r.ok) throw new Error('Failed to fetch outages')
  return r.json()
}

// Composite censorship index (0–100, higher = more censored) per country,
// blended from V-Dem / RSF. Drives the globe choropleth.
export async function getCensorshipIndex() {
  const r = await fetch(`${BASE}/censorship-index`)
  if (!r.ok) throw new Error('Failed to fetch censorship index')
  return r.json()
}

// Whole-world freedom-index ranking (V-Dem / RSF), names joined in.
// `order: 'asc'` = least-free / most-censored first.
export async function getRankings({ source = 'V_DEM', order = 'asc', limit = 200 } = {}) {
  const params = new URLSearchParams({ source, order, limit: String(limit) })
  const r = await fetch(`${BASE}/rankings?${params.toString()}`)
  if (!r.ok) throw new Error('Failed to fetch rankings')
  return r.json()
}

// Per-content-category censorship for a country (OONI web_connectivity
// aggregated by Citizen Lab category_code), ordered most-censored first.
export async function getCategories(countryCode) {
  const r = await fetch(`${BASE}/categories?country=${countryCode}`)
  if (!r.ok) throw new Error('Failed to fetch categories')
  return r.json()
}

// Messaging-app blocking (whatsapp/telegram/facebook_messenger/signal),
// stored as MESSAGING-category rows in technology_blocks and served by the
// existing /api/blocking endpoint filtered by layer=MESSAGING.
export async function getMessaging(countryCode) {
  const r = await fetch(`${BASE}/blocking?country=${countryCode}&layer=MESSAGING`)
  if (!r.ok) throw new Error('Failed to fetch messaging status')
  return r.json()
}

// Satellite positions, SGP4-propagated server-side from cached CelesTrak
// orbital elements. Computed fresh on every call — there is no server-side
// position cache — so the caller is expected to poll this every 5-10s for
// satellites that visibly move, rather than fetching it once. `category`
// omitted/falsy fetches every tracked object ("All Satellites"). The legend is
// single-select, but a row may stand for several backend categories (the
// derived "Other / Unclassified" bucket), so this takes either one category or
// a comma-separated list — the backend splits on commas either way. The response also carries `total`/`category_counts` computed over
// the whole catalog regardless of this filter, so a caller can show a live
// count on every legend row, not just the one currently selected.
export async function getSatellites(category) {
  const params = new URLSearchParams()
  if (category) params.set('categories', category)
  const qs = params.toString()
  const r = await fetch(`${BASE}/satellites${qs ? `?${qs}` : ''}`)
  if (!r.ok) throw new Error('Failed to fetch satellites')
  return r.json()
}

// One satellite's orbit path (pre-split at the antimeridian into drawable
// segments), for the currently-selected satellite only.
export async function getSatelliteOrbit(noradId) {
  const r = await fetch(`${BASE}/satellites/${noradId}/orbit`)
  if (!r.ok) throw new Error(`Failed to fetch orbit for ${noradId}`)
  return r.json()
}
