import { useEffect, useMemo, useState } from 'react'
import Globe from './components/Globe'
import CountrySidebar from './components/CountrySidebar'
import OutageFeed from './components/OutageFeed'
import GlobalRanking from './components/GlobalRanking'
import IndexLegend from './components/IndexLegend'
import CableLegend from './components/CableLegend'
import SatelliteLegend from './components/SatelliteLegend'
import SatelliteCard from './components/SatelliteCard'
import CommandBar from './components/CommandBar'
import StatusBar from './components/StatusBar'
import { buildBlockingMap } from './lib/blockingRegistry'
import {
  getBlocking,
  getCensorshipIndex,
  getCountries,
  getCountry,
  getCables,
  getGeo,
  getIxpStats,
  getOutages,
  getSatellites,
  getSatelliteOrbit,
  getStarlinkStatus,
} from './lib/api'
import { BASE, BORDER, MONO, MUTED, SIDEBAR } from './theme'
import './App.css'

// How often the client re-fetches satellite positions. The backend computes
// them fresh on every request (no server-side position cache), so this
// interval alone is what keeps satellites visibly moving — the fix for the
// bug where a comparable reference implementation (OSIRIS) fetched positions
// once per session and never re-polled.
const SATELLITE_POLL_MS = 7000

// Outages are the globe's only live layer now that the static blocking blooms
// are gone, so they have to keep arriving rather than being a snapshot of
// whenever the tab was opened. Slower than the satellite poll because these are
// detected events, not positions: IODA takes minutes to confirm one, so polling
// faster would just re-fetch the same list.
const OUTAGE_POLL_MS = 60_000

export default function App() {
  // `countries` and `blocking` are owned here and passed down, rather than
  // fetched independently by the components that derive from them (the globe
  // markers and the sidebar), which had made the app request /api/countries
  // and /api/blocking multiple times on every load.
  const [countries, setCountries] = useState([])
  const [blocking, setBlocking] = useState(null)
  const [countriesError, setCountriesError] = useState('')
  const [selectedCode, setSelectedCode] = useState('')
  const [selectedCountry, setSelectedCountry] = useState(null)
  const [selectionError, setSelectionError] = useState('')
  const [globeError, setGlobeError] = useState('')
  const [isLoadingCountries, setIsLoadingCountries] = useState(true)
  const [isLoadingSelection, setIsLoadingSelection] = useState(false)
  // Layer is pinned to ALL — the UI shows every category now, so the old
  // AI-access/circumvention toggle was removed. Kept as a constant the globe
  // and sidebar still read.
  const [layer] = useState('ALL')
  const [geo, setGeo] = useState([])
  const [outages, setOutages] = useState([])
  // Hand-curated Starlink legal/regulatory status per country (not a live
  // feed) — fetched once, indexed by country_code below.
  const [starlinkStatus, setStarlinkStatus] = useState([])
  // Submarine cable routes + landing points (static, fetched once regardless
  // of toggle state — see getCables()'s doc comment) and its own binary
  // show/hide, default off since it's decorative infrastructure context, not
  // an always-relevant signal.
  const [cables, setCables] = useState({ routes: [], landing_points: [] })
  const [showCables, setShowCables] = useState(false)
  // Per-country Internet Exchange Point density (not a live feed — see
  // getIxpStats()'s doc comment). Fetched once, same shape as starlinkStatus.
  const [ixpStats, setIxpStats] = useState([])
  // Composite censorship index (code -> 0–100) driving the globe choropleth,
  // plus its on/off toggle (default on).
  const [indexByCode, setIndexByCode] = useState({})
  const [showIndex, setShowIndex] = useState(true)
  // Satellite tracking layer: live-polled positions, a single-select "space
  // tracking" choice ('none' | 'all' | a category key — see SatelliteLegend's
  // SPACE_TRACKING_OPTIONS), live per-category counts, and the currently-
  // selected satellite's orbit path. Defaults to 'all' so the layer shows
  // everything tracked on first load.
  const [satellites, setSatellites] = useState([])
  const [spaceTrackingSelection, setSpaceTrackingSelection] = useState('all')
  const [spaceTrackingCounts, setSpaceTrackingCounts] = useState({})
  const [selectedSatelliteId, setSelectedSatelliteId] = useState(null)
  const [satelliteOrbit, setSatelliteOrbit] = useState(null)
  // Freshness of the DATA, not of the last network call. This used to be
  // `Date.now()` stamped whenever a fetch returned, which meant the status bar
  // read "3s ago" over a database that had not been refreshed in weeks — the
  // readout was measuring the browser, not the pipeline. /health reports the
  // newest `last_updated` across the fetched tables; that is the honest number.
  const [dataAge, setDataAge] = useState(null)

  useEffect(() => {
    let cancelled = false
    // /health is deliberately outside the auth layer and returns 503 when the
    // data is stale — the body is what we want either way, so status is not
    // checked here beyond parsing.
    fetch('/health')
      .then((r) => r.json())
      .then((body) => {
        if (!cancelled) setDataAge(body)
      })
      .catch(() => {
        if (!cancelled) setDataAge(null)
      })
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    let cancelled = false

    async function loadCountries() {
      setIsLoadingCountries(true)
      setCountriesError('')

      try {
        const result = await getCountries()
        if (cancelled) return

        setCountries(result)
      } catch (error) {
        if (cancelled) return
        setCountriesError(error instanceof Error ? error.message : 'Failed to load countries')
      } finally {
        if (!cancelled) {
          setIsLoadingCountries(false)
        }
      }
    }

    loadCountries()

    return () => {
      cancelled = true
    }
  }, [])

  // Blocking status for every country/technology. Non-fatal if it fails: the
  // globe falls back to uncoloured markers and the heatmap hides itself, which
  // is why this doesn't feed `statusMessage` the way the country load does.
  useEffect(() => {
    let cancelled = false

    getBlocking()
      .then((rows) => {
        if (!cancelled) {
          setBlocking(rows)
        }
      })
      .catch(() => {
        if (!cancelled) setBlocking([])
      })

    return () => {
      cancelled = true
    }
  }, [])

  // Derived once here and shared by the globe markers and the heatmap, so
  // neither has to re-scan the row set.
  const blockingByCode = useMemo(() => buildBlockingMap(blocking ?? []), [blocking])

  // Country identity + centroid + bbox for every drawable country. The globe
  // needs this to place markers and to derive a fly-to altitude, replacing the
  // hardcoded 5-entry centroid and altitude tables it used to carry.
  useEffect(() => {
    let cancelled = false

    getGeo()
      .then((rows) => {
        if (!cancelled) {
          setGeo(rows)
        }
      })
      .catch(() => {
        if (!cancelled) setGeo([])
      })

    return () => {
      cancelled = true
    }
  }, [])

  const geoByCode = useMemo(
    () => Object.fromEntries(geo.map((g) => [g.country_code, g])),
    [geo],
  )

  // Starlink status: fetched once, same shape as getGeo() above. Non-fatal —
  // on failure the sidebar simply shows no badge for any country.
  useEffect(() => {
    let cancelled = false

    getStarlinkStatus()
      .then((rows) => {
        if (!cancelled) setStarlinkStatus(rows)
      })
      .catch(() => {
        if (!cancelled) setStarlinkStatus([])
      })

    return () => {
      cancelled = true
    }
  }, [])

  const starlinkByCode = useMemo(
    () => Object.fromEntries(starlinkStatus.map((s) => [s.country_code, s])),
    [starlinkStatus],
  )

  // Submarine cables: fetched once, same non-fatal shape as getGeo() above.
  useEffect(() => {
    let cancelled = false

    getCables()
      .then((data) => {
        if (!cancelled) setCables(data)
      })
      .catch(() => {
        if (!cancelled) setCables({ routes: [], landing_points: [] })
      })

    return () => {
      cancelled = true
    }
  }, [])

  // IXP density: fetched once, same non-fatal shape as getStarlinkStatus().
  useEffect(() => {
    let cancelled = false

    getIxpStats()
      .then((rows) => {
        if (!cancelled) setIxpStats(rows)
      })
      .catch(() => {
        if (!cancelled) setIxpStats([])
      })

    return () => {
      cancelled = true
    }
  }, [])

  const ixpByCode = useMemo(
    () => Object.fromEntries(ixpStats.map((s) => [s.country_code, s])),
    [ixpStats],
  )

  // Composite censorship index for the choropleth. Non-fatal: on failure the
  // globe simply renders without fills.
  useEffect(() => {
    let cancelled = false

    getCensorshipIndex()
      .then((rows) => {
        if (cancelled) return
        setIndexByCode(Object.fromEntries(rows.map((r) => [r.country_code, r.censorship_score])))
      })
      .catch(() => {
        if (!cancelled) setIndexByCode({})
      })

    return () => {
      cancelled = true
    }
  }, [])

  // Live internet-outage overlay, re-polled on an interval. Fetches
  // currently-active IODA events plus every country's centroid, then
  // aggregates events to one entry per country (severity = worst score,
  // recency = latest start) for the globe blooms, the feed panel and the
  // command-bar counter — all three read this one piece of state, so they
  // stay in step. Non-fatal: see the failure handling inside.
  useEffect(() => {
    let cancelled = false
    // Country centroids are static reference data — fetched once and reused by
    // every poll. Held in a local rather than a ref so a failed first fetch is
    // simply retried on the next tick instead of poisoning the layer forever.
    let centroid = null
    // A poll that fails after the layer is populated keeps the previous
    // outages on screen. Blanking the globe on one dropped request would read
    // as "everything recovered", which is the opposite of the truth.
    let loaded = false

    async function poll() {
      try {
        if (!centroid) {
          const geo = await getGeo()
          if (cancelled) return
          centroid = new Map(
            geo.map((g) => [g.country_code, { name: g.country_name, lat: g.centroid_lat, lon: g.centroid_lon }]),
          )
        }

        const events = await getOutages({ active: true })
        if (cancelled) return

        const byCountry = new Map()
        for (const e of events) {
          const geoInfo = centroid.get(e.country_code)
          if (!geoInfo || geoInfo.lat == null || geoInfo.lon == null) continue
          const existing = byCountry.get(e.country_code)
          if (existing) {
            existing.count += 1
            existing.maxScore = Math.max(existing.maxScore, e.score)
            existing.latestStart = Math.max(existing.latestStart, e.start_ts)
          } else {
            byCountry.set(e.country_code, {
              code: e.country_code,
              name: geoInfo.name,
              lat: geoInfo.lat,
              lon: geoInfo.lon,
              count: 1,
              maxScore: e.score,
              latestStart: e.start_ts,
            })
          }
        }

        const aggregated = [...byCountry.values()].sort((a, b) => b.latestStart - a.latestStart)
        setOutages(aggregated)
        loaded = true
      } catch {
        if (!cancelled && !loaded) setOutages([])
      }
    }

    poll()
    const id = setInterval(poll, OUTAGE_POLL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [])

  // Satellite positions. Unlike every other data effect above, this one polls
  // — the backend computes positions fresh per request rather than caching
  // them, so a one-shot fetch would freeze satellites at load time exactly
  // like the reference implementation this feature was built to improve on.
  // Skips the fetch entirely (and clears the rendered layer) when "NONE" is
  // selected, so a hidden layer costs nothing — but leaves
  // `spaceTrackingCounts` alone so the legend's per-row counts stay visible
  // rather than flickering to zero while hidden.
  useEffect(() => {
    if (spaceTrackingSelection === 'none') {
      setSatellites([])
      return
    }

    const category = spaceTrackingSelection === 'all' ? null : spaceTrackingSelection
    let cancelled = false
    async function poll() {
      try {
        const data = await getSatellites(category)
        if (!cancelled) {
          setSatellites(data.satellites)
          setSpaceTrackingCounts({ total: data.total, ...data.category_counts })
        }
      } catch {
        if (!cancelled) setSatellites([])
      }
    }

    poll()
    const id = setInterval(poll, SATELLITE_POLL_MS)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [spaceTrackingSelection])

  // Selected satellite's orbit path. Independent of the position poll above —
  // fetched once per selection, not on every poll tick.
  useEffect(() => {
    if (!selectedSatelliteId) {
      setSatelliteOrbit(null)
      return
    }

    let cancelled = false
    getSatelliteOrbit(selectedSatelliteId)
      .then((data) => {
        if (!cancelled) setSatelliteOrbit(data)
      })
      .catch(() => {
        if (!cancelled) setSatelliteOrbit(null)
      })

    return () => {
      cancelled = true
    }
  }, [selectedSatelliteId])

  // A selected satellite can drop out of the live poll — its category got
  // unchecked, or it decayed out of the catalog — while its orbit path stays
  // fetched (that effect above is keyed only on `selectedSatelliteId`, not on
  // `satellites`), leaving an orbit line on the globe with no selected point
  // or card to justify it. Clearing the selection here removes the card (via
  // `selectedSatellite` resolving to null) and the orbit (via the effect
  // above re-running with `selectedSatelliteId` cleared). Guarded on a
  // non-empty `satellites` so a transient empty poll (e.g. right after
  // toggling the layer back on) doesn't spuriously clear a valid selection.
  useEffect(() => {
    if (
      selectedSatelliteId != null &&
      satellites.length > 0 &&
      !satellites.some((s) => s.norad_id === selectedSatelliteId)
    ) {
      setSelectedSatelliteId(null)
    }
  }, [satellites, selectedSatelliteId])

  // No auto-selection on load — the globe's default state is intentionally
  // sparse (outlines + pulsing markers) until the user picks a country via
  // the globe or the dropdown.
  useEffect(() => {
    if (!selectedCode) return

    let cancelled = false

    async function loadSelection() {
      setIsLoadingSelection(true)
      setSelectionError('')

      try {
        const country = await getCountry(selectedCode)
        if (cancelled) return

        setSelectedCountry(country)
      } catch (error) {
        if (cancelled) return
        setSelectedCountry(null)
        setSelectionError(error instanceof Error ? error.message : 'Failed to load country')
      } finally {
        if (!cancelled) {
          setIsLoadingSelection(false)
        }
      }
    }

    loadSelection()

    return () => {
      cancelled = true
    }
  }, [selectedCode])

  const statusMessage = countriesError || selectionError || globeError
    || (isLoadingCountries && 'Loading country data...')
    || (isLoadingSelection && 'Refreshing country...')

  // Command-bar counters — plain reflections of the datasets already loaded
  // above, computed here so the chrome adds no fetches of its own.
  const counts = {
    // The globe/dropdown cover every drawable country, so the counter reflects
    // that (geo), not the small researched-dossier set (`countries`).
    countries: geo.length,
    signals: blocking?.length ?? 0,
    outages: outages.length,
  }

  // Status-bar link state, derived from the same country-load flags that gate
  // the rest of the UI — not a separate health check.
  const linkStatus = countriesError ? 'error' : isLoadingCountries ? 'loading' : 'ok'

  // Sidebar country: the full researched dossier when we have it; otherwise,
  // once the dossier fetch has settled, the country_reference stub (code + name)
  // from geoByCode — so a click on ANY globe marker or dropdown entry resolves,
  // not just the handful of countries that have a /api/countries dossier row.
  // While the dossier fetch is still in flight we intentionally hold at the
  // loading state rather than flashing the stub first.
  const sidebarCountry = selectedCountry || (!isLoadingSelection ? geoByCode[selectedCode] : null)

  // Refreshes with every poll for free, since it's just a lookup into the
  // already-live `satellites` state rather than a fetch of its own.
  const selectedSatellite = useMemo(
    () => satellites.find((s) => s.norad_id === selectedSatelliteId) ?? null,
    [satellites, selectedSatelliteId],
  )

  return (
    <div style={{ background: BASE, height: '100vh', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <CommandBar
        countries={geo}
        selectedCode={selectedCode}
        onSelectCountry={setSelectedCode}
        counts={counts}
      />

      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        <main style={{ position: 'relative', flex: 1, overflow: 'hidden' }}>
          <Globe
            geoByCode={geoByCode}
            outages={outages}
            indexByCode={indexByCode}
            showIndex={showIndex}
            onCountrySelect={setSelectedCode}
            onLoadError={setGlobeError}
            selectedCode={selectedCode}
            satellites={satellites}
            onSatelliteSelect={setSelectedSatelliteId}
            selectedSatelliteId={selectedSatelliteId}
            selectedSatelliteCategory={selectedSatellite?.category ?? null}
            satelliteOrbit={satelliteOrbit}
            cables={cables}
            showCables={showCables}
          />

          {/* Vignette: darkens the globe-area corners to focus the eye and add
              depth, without touching the docked chrome — it lives inside <main>,
              beneath the zIndex-5 overlay panels, and is click-through. */}
          <div
            aria-hidden="true"
            style={{
              position: 'absolute',
              inset: 0,
              pointerEvents: 'none',
              background: 'radial-gradient(ellipse at center, rgba(0,0,0,0) 55%, rgba(0,0,0,0.4) 100%)',
            }}
          />

          <OutageFeed outages={outages} />

          <GlobalRanking />

          {/* Centered as one group so the pair's combined width — not
              either panel's individually — is what centers at the bottom.
              Bounded to the region right of GlobalRanking (left:288 = its
              own left:12 + 264px width + a 12px gap), not the full viewport
              width — a plain 50% center ignored that GlobalRanking occupies
              a fixed column on the left, so on narrower windows this group
              centered enough to overlap the bottom of that list. Bounding +
              centering within the remaining space fixes that at any width,
              rather than a fixed pixel nudge that would only hold at one
              specific window size.

              `position: fixed` (viewport-relative) rather than `absolute`
              (relative to <main>) is deliberate: <main> is a flex sibling of
              the country sidebar and shrinks by the sidebar's width whenever
              one is open, which shifted this group visibly left every time a
              country was selected. Fixed positioning is anchored to the
              window instead, so it holds the same spot on screen regardless
              of sidebar state — `bottom: 36` reproduces the same visual
              offset as the old `bottom: 12` inside <main> once the 24px
              StatusBar footer below <main> is accounted for.

              `right: 392` permanently reserves the country sidebar's width
              (380px + a 12px gap), the same way `left: 288` permanently
              reserves GlobalRanking's — even though the sidebar, unlike
              GlobalRanking, isn't always mounted. Since this group no longer
              re-centers when the sidebar opens (see above), splitting the
              difference between "centered when closed" and "clear of the
              sidebar when open" isn't possible with one static position;
              always reserving the space is the option that never overlaps,
              at the cost of sitting slightly left of true-center while the
              sidebar is closed. */}
          <div
            style={{
              position: 'fixed',
              bottom: 36,
              left: 288,
              right: 392,
              display: 'flex',
              justifyContent: 'center',
              gap: 12,
              zIndex: 5,
            }}
          >
            <IndexLegend show={showIndex} onToggle={() => setShowIndex((v) => !v)} />
            <CableLegend
              show={showCables}
              onToggle={() => setShowCables((v) => !v)}
              routeCount={cables.routes.length}
              landingCount={cables.landing_points.length}
            />
          </div>

          <SatelliteLegend
            selection={spaceTrackingSelection}
            onSelect={setSpaceTrackingSelection}
            counts={spaceTrackingCounts}
          />

          <SatelliteCard
            satellite={selectedSatellite}
            periodMinutes={satelliteOrbit?.period_minutes}
            onClose={() => setSelectedSatelliteId(null)}
          />

          {statusMessage && (
            <div
              style={{
                position: 'absolute',
                bottom: 16,
                right: 16,
                background: SIDEBAR,
                border: `1px solid ${BORDER}`,
                padding: '6px 10px',
                fontFamily: MONO,
                fontSize: 10,
                letterSpacing: '0.05em',
                color: MUTED,
              }}
            >
              {statusMessage}
            </div>
          )}
        </main>

        {/* CountrySidebar deliberately carries no `key={selectedCode}`: every
            effect inside it and its charts already depends on the country code,
            so remounting the subtree only forced avoidable teardown — and
            refetched country-independent data like /api/models on every
            selection. */}
        {selectedCode && (
          <aside style={{ width: 380, flexShrink: 0, height: '100%', overflow: 'hidden' }}>
            {sidebarCountry ? (
              <CountrySidebar
                country={sidebarCountry}
                layer={layer}
                starlinkStatus={starlinkByCode[sidebarCountry?.country_code]}
                ixpStats={ixpByCode[sidebarCountry?.country_code]}
                onClose={() => {
                  setSelectedCode('')
                  setSelectedCountry(null)
                }}
              />
            ) : (
              <div
                style={{
                  height: '100%',
                  width: '100%',
                  background: SIDEBAR,
                  borderLeft: `1px solid ${BORDER}`,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  textAlign: 'center',
                  padding: '0 24px',
                  fontSize: 13,
                  color: MUTED,
                }}
              >
                {isLoadingSelection ? 'Loading country...' : (selectionError || 'Country data unavailable')}
              </div>
            )}
          </aside>
        )}
      </div>

      <StatusBar status={linkStatus} dataAge={dataAge} />
    </div>
  )
}
