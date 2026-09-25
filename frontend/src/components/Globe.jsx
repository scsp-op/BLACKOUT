import { useEffect, useRef, useState } from 'react'
import * as Cesium from 'cesium'
import * as topojson from 'topojson-client'
import 'cesium/Build/Cesium/Widgets/widgets.css'
import { CRIMSON } from '../theme'
import { CATEGORY_COLOR_HEX } from './SatelliteLegend'

const NUMERIC_CODE_ALIASES = new Map([[732, 'MA']])

// Fallback for atlas features that carry no ISO numeric id at all, matched on
// the atlas's own `properties.name`. Kosovo is the case that matters: the API
// tracks it (XK, include_on_globe = 1) but its `iso_numeric` is null — it has
// no ISO 3166-1 numeric code — and the atlas feature has no id either, so the
// numeric join can never reach it and the country renders as land that cannot
// be clicked.
//
// The atlas's four other id-less features are left unmapped on purpose:
// Somaliland and N. Cyprus are not tracked by the API, and Siachen Glacier /
// Indian Ocean Ter. are disputed or dependent territories with no country of
// their own to select. Mapping those onto a neighbour would be a territorial
// claim, not a bug fix.
const NAME_CODE_ALIASES = new Map([['Kosovo', 'XK']])

function borderGroupForNumeric(numeric) {
  if (numeric === 504 || numeric === 732) return 'MA'
  return numeric
}

// Read from the environment rather than inlined here: anything in this file
// ships to the browser *and* to git. The Viewer below runs with
// `imageryProvider: false` and no terrain provider, so no Ion asset is
// actually requested and an unset token is harmless — it's wired up only so
// enabling Ion imagery later doesn't require hardcoding a credential again.
const ION_TOKEN = import.meta.env.VITE_CESIUM_ION_TOKEN
if (ION_TOKEN) {
  Cesium.Ion.defaultAccessToken = ION_TOKEN
}

// The acute layer's only motion: outage blooms breathe in brightness. Alpha
// only — never geometry — so nothing re-tessellates per frame. Smooth 0→1→0.
const OUTAGE_PULSE_PERIOD_MS = 1600

function pulse01(periodMs) {
  const now = performance.now()
  return 0.5 - 0.5 * Math.cos(((now % periodMs) / periodMs) * Math.PI * 2)
}

function outageRadius(score) {
  if (score >= 200) return 340_000
  if (score >= 60) return 240_000
  return 160_000
}

// Empty-space colour behind/around the globe (skybox is off — see init —
// so this, not a starfield texture, is what fills it).
const SPACE_BG = '#03060a'

// Whole-globe framing, centred on ~20°E/15°N rather than 0/0 so the front
// hemisphere on load holds Europe, Africa, the Middle East and South Asia — the
// densest censorship geography — instead of the mid-Atlantic.
const HOME_VIEW = { lon: 20.0, lat: 15.0, height: 20_000_000 }

// Just above the 2000m border outlines so cable routes/landing points draw on
// top of them rather than z-fighting at the surface.
const CABLE_HEIGHT = 2500

// How far the cursor may sit from a satellite's projected centre and still
// count as a hit, in pixels. Points are drawn at 2-3px, so this is deliberately
// about the size of the dot itself: Cesium's own pick tolerance is wider, and
// letting it decide is what made the cloud swallow country clicks.
// Land tint under the cursor. Same hue as LAND_COLOR (H213), lifted from L10
// to L20 — clearly a response without becoming a second data colour. This is
// the only thing that tells you bare land is clickable; before it, the cursor
// change was the sole affordance.
const LAND_HOVER_HEX = '#17304f'

// Hover fires on every mouse move, and drillPick costs several render passes,
// so it is rate-limited. 50ms is well under the threshold where a highlight
// feels laggy but caps the picking work at 20/s instead of ~60/s.
const HOVER_THROTTLE_MS = 50

const SAT_PICK_SLOP_PX = 3

// Reused across picks so hover (which fires on every mouse move) doesn't
// allocate a Cartesian2 per event.
const scratchWindowPosition = new Cesium.Cartesian2()

// A css rgba() string from a theme hex + alpha, for the radial-gradient canvas
// textures below.
function rgbaFrom(hex, alpha) {
  const c = Cesium.Color.fromCssColorString(hex)
  return `rgba(${Math.round(c.red * 255)}, ${Math.round(c.green * 255)}, ${Math.round(c.blue * 255)}, ${alpha})`
}

// Dark-slate land fill so every country reads as land over black ocean, even
// with no index score. Opaque (the black globe sits beneath it) and a hair
// below the SIDEBAR chrome tone so land stays subordinate to the panels.
const LAND_COLOR = Cesium.Color.fromCssColorString('#0c1928')
const LAND_HOVER_COLOR = Cesium.Color.fromCssColorString(LAND_HOVER_HEX)

// Choropleth ramp for the composite censorship index (0 = free → 100 = most
// censored): green → amber → crimson. Local constants so this doesn't depend on
// the shared theme import line. The fill is translucent so borders and markers
// still read on top.
const CHORO_LOW = Cesium.Color.fromCssColorString('#6c9a5b')
const CHORO_MID = Cesium.Color.fromCssColorString('#d97706')
const CHORO_HIGH = Cesium.Color.fromCssColorString('#b31942')
const CHORO_ALPHA = 0.55

function choroplethColor(censorship) {
  const t = Math.max(0, Math.min(100, censorship)) / 100
  const c = t <= 0.5
    ? Cesium.Color.lerp(CHORO_LOW, CHORO_MID, t / 0.5, new Cesium.Color())
    : Cesium.Color.lerp(CHORO_MID, CHORO_HIGH, (t - 0.5) / 0.5, new Cesium.Color())
  return c.withAlpha(CHORO_ALPHA)
}

// Canvases are cached per (kind, hex) instead of created per country. There are
// only a handful of distinct statuses, so this is a bounded set no matter how
// many countries are drawn.
const canvasCache = new Map()

// Soft radial glow (bright core → transparent edge) — the "thermal bloom" that
// replaces the old dot+ring marker. Painted onto a surface ellipse so it reads
// as signal radiating off the map rather than a symbol standing on top of it.
function bloomCanvas(hex) {
  const key = `bloom:${hex}`
  if (!canvasCache.has(key)) {
    const size = 128
    const canvas = document.createElement('canvas')
    canvas.width = size
    canvas.height = size
    const ctx = canvas.getContext('2d')
    const r = size / 2
    const gradient = ctx.createRadialGradient(r, r, 0, r, r, r)
    // Hot near-white core → saturated mid → transparent edge. The bright core
    // keeps the mark legible even over crimson choropleth land, where a plain
    // crimson glow blended into the globe.
    gradient.addColorStop(0, 'rgba(255, 241, 224, 0.95)')
    gradient.addColorStop(0.14, rgbaFrom(hex, 0.92))
    gradient.addColorStop(0.4, rgbaFrom(hex, 0.4))
    gradient.addColorStop(1, rgbaFrom(hex, 0))
    ctx.fillStyle = gradient
    ctx.beginPath()
    ctx.arc(r, r, r, 0, Math.PI * 2)
    ctx.fill()
    canvasCache.set(key, canvas)
  }
  return canvasCache.get(key)
}

// Fly-to altitude derived from the country's bounding box rather than a
// hand-authored per-country table, so it frames Vatican City and Russia
// sensibly without anyone maintaining a list. Clamped because a bbox that spans
// the antimeridian or a whole hemisphere would otherwise zoom to orbit.
function altitudeFor(geo) {
  if (!geo || geo.bbox_min_lon == null || geo.bbox_max_lon == null) return 4_000_000
  const span = Math.max(geo.bbox_max_lon - geo.bbox_min_lon, geo.bbox_max_lat - geo.bbox_min_lat)
  return Math.min(Math.max(span * 180_000, 700_000), 8_000_000)
}

// Real physical altitude spans ~160 km (very low LEO) to ~36,000 km (GEO) —
// against Earth's ~6,371 km radius, plotting it true-scale would bury LEO
// satellites against the surface and make GEO ones a barely-distinguishable
// speck. This is a *display-only* transform: it only changes the Cartesian3
// height passed to the renderer, never `alt_km` itself (still shown verbatim
// in SatelliteCard and the API response). sqrt compression keeps LEO/MEO/GEO
// visually separated without linearly exaggerating GEO into orbit-breaking
// distances.
const SAT_DISPLAY_MIN_M = 150_000
const SAT_DISPLAY_MAX_M = 3_000_000
const SAT_DISPLAY_ALT_CEILING_KM = 42_000 // just past GEO (~35,786 km)

function satelliteDisplayHeight(altKm) {
  const t = Math.sqrt(Math.max(0, Math.min(altKm, SAT_DISPLAY_ALT_CEILING_KM)) / SAT_DISPLAY_ALT_CEILING_KM)
  return SAT_DISPLAY_MIN_M + t * (SAT_DISPLAY_MAX_M - SAT_DISPLAY_MIN_M)
}

// Colour palette lives in SatelliteLegend.jsx (the legend's swatches must
// match the globe's points, so one shared source avoids the two drifting).
const satelliteColorCache = new Map()
function satelliteColor(category) {
  if (!satelliteColorCache.has(category)) {
    const hex = CATEGORY_COLOR_HEX[category] ?? CATEGORY_COLOR_HEX.other
    satelliteColorCache.set(category, Cesium.Color.fromCssColorString(hex))
  }
  return satelliteColorCache.get(category)
}

// `geoByCode` supplies centroids and bounding boxes for every country the
// basemap can draw (from /api/geo). Blocking status is deliberately not a prop:
// the globe shows censorship intensity through the choropleth and live events
// through the outage blooms, and the per-country blocking detail belongs to the
// sidebar. The researched policy dossiers are likewise not a prop.
export default function Globe({
  geoByCode = {},
  outages = [],
  indexByCode = {},
  showIndex = true,
  onCountrySelect,
  onLoadError,
  selectedCode = '',
  satellites = [],
  onSatelliteSelect,
  selectedSatelliteId = null,
  selectedSatelliteCategory = null,
  satelliteOrbit = null,
  cables = null,
  showCables = false,
}) {
  const containerRef = useRef(null)
  const viewerRef = useRef(null)
  // Per-country marker state: a crimson bloom entity for each confirmed-blocked
  // country (the only visible status marks). Picking is handled by the land
  // fill, not markers, so there are no invisible pick billboards anymore.
  const outageStateRef = useRef({})
  // Choropleth: the loaded basemap geojson is stashed here in init so the
  // fill effect (which reacts to index data arriving later) can reuse it, and
  // the current fill Primitive is tracked so it can be swapped/removed.
  const geojsonRef = useRef(null)
  const choroplethRef = useRef(null)
  // Static dark-slate land fill (built once from the basemap geometry); tracked
  // so it can be torn down with the viewer.
  const landRef = useRef(null)
  // code → the ids of every ring drawn for that country, so a multi-part
  // country (USA, Indonesia) lights all of its pieces on hover rather than
  // just whichever ring happened to be built first.
  const landRingIdsRef = useRef(null)
  const hoveredCodeRef = useRef(null)
  // GPU-batched point cloud for satellite markers (one collection, positions
  // updated per poll) and a polyline collection for the selected satellite's
  // orbit path — both created once in init, alongside outlineCollection.
  const satPointsRef = useRef(null)
  const satOrbitRef = useRef(null)
  // Submarine cable routes + landing points: static reference data (fetched
  // once in App.jsx, never changes for the life of the session), so — unlike
  // the satellite collections above — these are populated exactly once, the
  // first time the `cables` prop arrives non-empty, and never rebuilt.
  // `cablesBuiltRef` guards that one-time population across re-renders.
  const cableRouteRef = useRef(null)
  const cableLandingRef = useRef(null)
  const cablesBuiltRef = useRef(false)
  const [ready, setReady] = useState(false)

  // Keep the latest callbacks in refs so the init effect (which only runs
  // once) always calls the current prop without needing to re-run.
  const onCountrySelectRef = useRef(onCountrySelect)
  const onLoadErrorRef     = useRef(onLoadError)
  const onSatelliteSelectRef = useRef(onSatelliteSelect)
  useEffect(() => { onCountrySelectRef.current = onCountrySelect }, [onCountrySelect])
  useEffect(() => { onLoadErrorRef.current = onLoadError }, [onLoadError])
  useEffect(() => { onSatelliteSelectRef.current = onSatelliteSelect }, [onSatelliteSelect])

  // `prevSelectedRef` lets the framing effect tell a real deselect (return to
  // the whole-globe view) apart from the empty selection on first load (which
  // must stay instant, since init already frames HOME_VIEW).
  const prevSelectedRef = useRef(selectedCode)

  useEffect(() => {
    if (!containerRef.current) return

    let cancelled = false
    let viewer

    const init = async () => {
      viewer = new Cesium.Viewer(containerRef.current, {
        // `baseLayer: false` (not the pre-1.107 `imageryProvider: false`) is
        // what suppresses the base imagery now. Cesium silently ignores the
        // old key and falls back to Ion World Imagery, which both requires an
        // Ion token and paints over the intended black globe.
        baseLayer:            false,
        baseLayerPicker:      false,
        geocoder:             false,
        homeButton:           false,
        sceneModePicker:      false,
        navigationHelpButton: false,
        animation:            false,
        timeline:             false,
        fullscreenButton:     false,
        infoBox:              false,
        selectionIndicator:   false,
        creditContainer:      Object.assign(document.createElement('div'), { style: 'display:none' }),
        // Render at the display's native devicePixelRatio instead of Cesium's
        // default 1x CSS-pixel resolution — the globe rendered soft/low-res on
        // HiDPI (Retina) screens, most visibly when zoomed out. MSAA is already
        // 4x by default, so this is purely a pixel-density fix.
        useBrowserRecommendedResolution: false,
      })

      // Bail out if this effect was cleaned up (e.g. React StrictMode
      // double-invoke) while the Viewer was being constructed.
      if (cancelled) {
        viewer.destroy()
        return
      }

      viewer.scene.globe.enableLighting = false
      // Ocean = the globe base colour: lifted off pure black so it doesn't read
      // as a dead void, and sat on the SCSP Blue hue axis (H213) like the rest
      // of the chrome. Kept very dark (L7) — the blue is a tone, not a tint.
      // What this must NOT come back to is the bright ground-atmosphere wash
      // that used to tint the ocean; that's disabled just below.
      viewer.scene.globe.baseColor = Cesium.Color.fromCssColorString('#0a111a')
      // Keep only the sky-atmosphere limb (a thin rim just outside the globe) so
      // the sphere still reads against space — but drop the ground atmosphere,
      // which is what tinted the ocean blue. The rim is dimmed and heavily
      // desaturated toward neutral so it doesn't reintroduce a blue cast.
      // brightnessShift/saturationShift ∈ [-1,1]; atmosphereLightIntensity
      // default is 50.
      viewer.scene.globe.showGroundAtmosphere = false
      viewer.scene.skyAtmosphere.show = true
      viewer.scene.skyAtmosphere.brightnessShift = -0.5
      viewer.scene.skyAtmosphere.saturationShift = -0.7
      viewer.scene.skyAtmosphere.atmosphereLightIntensity = 5
      viewer.scene.backgroundColor = Cesium.Color.fromCssColorString(SPACE_BG)
      // Cesium's default starfield skybox — off so empty space is the flat
      // SPACE_BG colour above, not a field of stars. Disabled here (not via
      // the constructor's `skyBox: false`, which — in this globe config —
      // also leaves `scene.skyAtmosphere` undefined) so the atmosphere rim
      // configured below is unaffected.
      viewer.scene.skyBox.show = false
      viewer.scene.sun.show  = false
      viewer.scene.moon.show = false

      viewerRef.current = viewer

      viewer.camera.flyTo({
        destination: Cesium.Cartesian3.fromDegrees(HOME_VIEW.lon, HOME_VIEW.lat, HOME_VIEW.height),
        duration: 0,
      })

      // Load accurate, locally-bundled world borders from world-atlas
      const worldData = await import('world-atlas/countries-50m.json')
      if (cancelled) return
      const geojson = topojson.feature(worldData.default, worldData.default.objects.countries)
      // Stashed for the choropleth fill effect, which needs this geometry but
      // runs separately so it can react to index data that arrives after init.
      geojsonRef.current = geojson

      // Thin outline-only borders for every country. Build these from the atlas
      // topology rather than each country's own polygon rings so borders between
      // aliased regions (Morocco / Western Sahara) can be suppressed.
      const outlineCollection = new Cesium.PolylineCollection()
      viewer.scene.primitives.add(outlineCollection)

      const addOutlineRing = (coords) => {
        const positions = coords.map(([lng, lat]) => Cesium.Cartesian3.fromDegrees(lng, lat, 2000))
        outlineCollection.add({
          positions,
          width: 1.2,
          material: Cesium.Material.fromType('Color', {
            // Cool slate-white at ~2.3:1 on black — legible without the harsh
            // full-white hairline. Was #ffffff @ 0.15 (~1.3:1), sub-pixel.
            color: Cesium.Color.fromCssColorString('#9db4c9').withAlpha(0.3),
          }),
        })
      }

      const borderMesh = topojson.mesh(
        worldData.default,
        worldData.default.objects.countries,
        (a, b) => {
          const aNumeric = parseInt(a?.id, 10)
          const bNumeric = parseInt(b?.id, 10)
          if (Number.isNaN(aNumeric) || Number.isNaN(bNumeric)) return true
          return a === b || borderGroupForNumeric(aNumeric) !== borderGroupForNumeric(bNumeric)
        },
      )
      borderMesh.coordinates.forEach(addOutlineRing)

      // Satellite markers: a single PointPrimitiveCollection (GPU-batched, one
      // draw call regardless of count) rather than one Entity per satellite —
      // the existing per-Entity bloom pattern above is fine for a few hundred
      // countries, not for thousands of moving points. Populated/updated by
      // its own effect below, reacting to the `satellites` prop.
      satPointsRef.current = new Cesium.PointPrimitiveCollection()
      viewer.scene.primitives.add(satPointsRef.current)

      // Selected satellite's orbit path. A PolylineCollection like the border
      // outlines above, but rebuilt per selection rather than built once.
      satOrbitRef.current = new Cesium.PolylineCollection()
      viewer.scene.primitives.add(satOrbitRef.current)

      // Submarine cables: a PolylineCollection for routes and a
      // PointPrimitiveCollection for landing points, both created empty here
      // and populated once by their own effect below when the `cables` prop
      // first arrives. `.show` starts false and is driven entirely by the
      // `showCables` toggle effect, so the layer is invisible until switched
      // on even though the collections exist from init.
      cableRouteRef.current = new Cesium.PolylineCollection()
      cableRouteRef.current.show = false
      viewer.scene.primitives.add(cableRouteRef.current)

      cableLandingRef.current = new Cesium.PointPrimitiveCollection()
      cableLandingRef.current.show = false
      viewer.scene.primitives.add(cableLandingRef.current)

      // Markers and blooms are built by their own effects, from props — see
      // below. Init owns only what the scene needs once: the viewer, the borders
      // and the input handlers. There is deliberately no per-frame animation
      // loop — the only moving part (outage bloom alpha) animates itself through
      // CallbackProperty, so there's nothing to tear down.
      const handler = new Cesium.ScreenSpaceEventHandler(viewer.scene.canvas)

      // Resolve a country code from a pick: the land-fill polygons carry the code
      // directly as their GeometryInstance id (a string), and the crimson blooms
      // carry it in entity properties. Clicking the ocean resolves to nothing.
      // This is what lets any country — not just signalled ones — be selected
      // from the globe, matching the header dropdown.
      const codeFromPick = (picked) => {
        const id = picked?.id
        // Land rings are `<code>#<ring>` so each ring carries a unique id and
        // can be recoloured on its own; everything else that resolves to a
        // country (the blooms, the selection marker) uses the bare code.
        if (typeof id === 'string') {
          const hash = id.indexOf('#')
          return hash === -1 ? id : id.slice(0, hash)
        }
        return id?.properties?.code?.getValue()
      }

      // Satellite points carry their NORAD ID (a number) directly as `.id`,
      // set when each PointPrimitive is added — same convention as the land
      // polygons carrying a country code, just a different id type so the two
      // pick targets can't collide.
      const satelliteIdFromPick = (picked) => (typeof picked?.id === 'number' ? picked.id : null)

      // Satellites are drawn above everything else, so a plain `scene.pick` —
      // which returns only the topmost hit — hands back a satellite for most
      // clicks and the country underneath becomes unreachable. With the full
      // catalogue on, ~8.5k points are on the near side at once and each has a
      // pick footprint a little larger than its 2-3px dot, so they cover
      // roughly half the visible globe.
      //
      // Drilling through the stack does not solve this: in the dense shells
      // well over a dozen points can overlap one cursor position, so any fixed
      // drill depth is exhausted by satellites before it reaches land. Instead
      // the layer is hidden for a single extra pick, which is bounded at two
      // picks no matter how deep the cloud is. `show` is restored before the
      // next frame, so nothing flickers — pick renders to its own framebuffer.
      // Pass 2: the cursor is over the satellite layer but not on a dot. Hide
      // that layer for one pick so the country beneath is reachable. A shallow
      // drill rather than a single pick because the border polylines are drawn
      // above the fills and carry no id — a plain pick can land on a hairline
      // and resolve to nothing.
      const pickCountryBehindSatellites = (windowPosition) => {
        const scene = viewer.scene
        const satellitePoints = satPointsRef.current
        if (satellitePoints) satellitePoints.show = false
        try {
          for (const picked of scene.drillPick(windowPosition, 4)) {
            const code = codeFromPick(picked)
            if (code) return code
          }
          return undefined
        } finally {
          if (satellitePoints) satellitePoints.show = true
        }
      }

      const resolvePick = (windowPosition) => {
        const scene = viewer.scene
        const top = scene.pick(windowPosition)

        const noradId = satelliteIdFromPick(top)
        if (noradId == null) {
          const code = codeFromPick(top)
          // No satellite on top, but possibly a border hairline with no id —
          // the same shallow drill resolves that too.
          return { code: code ?? pickCountryBehindSatellites(windowPosition) }
        }

        // A satellite is on top. It only wins if the cursor is genuinely on the
        // dot, rather than merely inside Cesium's more forgiving pick rectangle
        // — that check is what shrinks the target back to what the user sees.
        const worldPosition = top.primitive?.position
        if (worldPosition) {
          const dotCentre = Cesium.SceneTransforms.worldToWindowCoordinates(
            scene,
            worldPosition,
            scratchWindowPosition,
          )
          if (
            dotCentre &&
            Cesium.Cartesian2.distance(dotCentre, windowPosition) <= SAT_PICK_SLOP_PX
          ) {
            return { noradId }
          }
        }

        return { code: pickCountryBehindSatellites(windowPosition) }
      }

      // Recolour every ring of one country in place. The land layer is a single
      // Primitive with per-instance colour, so this needs no new geometry and
      // no rebuild — just a write to the instance attribute.
      const tintLand = (code, color) => {
        const primitive = landRef.current
        if (!primitive || !primitive.ready) return
        const ids = landRingIdsRef.current?.get(code)
        if (!ids) return
        const value = Cesium.ColorGeometryInstanceAttribute.toValue(color)
        for (const id of ids) {
          const attributes = primitive.getGeometryInstanceAttributes(id)
          if (attributes) attributes.color = value
        }
      }

      let lastHoverAt = 0
      handler.setInputAction(({ endPosition }) => {
        const now = performance.now()
        if (now - lastHoverAt < HOVER_THROTTLE_MS) return
        lastHoverAt = now

        const { noradId, code } = resolvePick(endPosition)
        viewer.scene.canvas.style.cursor = noradId != null || code ? 'pointer' : 'default'

        const previous = hoveredCodeRef.current
        const next = code ?? null
        if (next === previous) return
        if (previous) tintLand(previous, LAND_COLOR)
        if (next) tintLand(next, LAND_HOVER_COLOR)
        hoveredCodeRef.current = next
      }, Cesium.ScreenSpaceEventType.MOUSE_MOVE)

      // Click → report selection up. The camera fly-to lives in the selection-
      // framing effect so click and dropdown share one framing path.
      handler.setInputAction(({ position }) => {
        const { noradId, code } = resolvePick(position)

        if (noradId != null) {
          onSatelliteSelectRef.current?.(noradId)
          return
        }

        if (!code) return
        onCountrySelectRef.current?.(code)
      }, Cesium.ScreenSpaceEventType.LEFT_CLICK)

      setReady(true)
    }

    init().catch(error => {
      console.error(error)
      onLoadErrorRef.current?.(error instanceof Error ? error.message : 'Failed to initialize globe')
    })

    return () => {
      cancelled = true
      if (viewer && !viewer.isDestroyed()) {
        viewer.destroy()
      }
      if (viewerRef.current === viewer) {
        viewerRef.current = null
      }
    }
  }, [])

  // Global internet-outage overlay, rebuilt whenever the active-outage set
  // changes. Independent of the blocking markers so it can light up any country
  // in the world; each entry carries its own centroid from /api/geo.
  useEffect(() => {
    if (!ready) return
    const viewer = viewerRef.current
    if (!viewer || viewer.isDestroyed()) return

    const outageState = outageStateRef.current
    for (const [code, o] of Object.entries(outageState)) {
      viewer.entities.remove(o.bloomEntity)
      delete outageState[code]
    }

    outages.forEach((o) => {
      if (o.lat == null || o.lon == null) return
      // A crimson surface bloom whose brightness breathes — the only motion on
      // the map, marking a LIVE disruption. Only the material alpha animates
      // (via CallbackProperty), never the ellipse size, so the geometry is
      // never re-tessellated. Surface-hugging + depth-tested, so the far side
      // is occluded by the globe like everything else.
      const radius = outageRadius(o.maxScore)
      const bloomEntity = viewer.entities.add({
        position: Cesium.Cartesian3.fromDegrees(o.lon, o.lat),
        properties: { code: o.code },
        ellipse: {
          semiMajorAxis: radius,
          semiMinorAxis: radius,
          height: 3000,
          material: new Cesium.ImageMaterialProperty({
            image: bloomCanvas(CRIMSON),
            transparent: true,
            color: new Cesium.CallbackProperty(
              () => Cesium.Color.WHITE.withAlpha(0.3 + 0.55 * pulse01(OUTAGE_PULSE_PERIOD_MS)),
              false,
            ),
          }),
        },
      })
      outageState[o.code] = { bloomEntity }
    })
  }, [ready, outages])

  // Dark-slate land underlay: one flat Primitive filling every country polygon
  // so land reads as land over the dark ocean, even where we have no index
  // score. Built at height 250 (beneath the choropleth at 600 and borders at
  // 2000, which read on top). It doubles as the click target — each polygon
  // carries its country code as a pick id — so it rebuilds when geoByCode
  // arrives. `asynchronous` so the whole-world tessellation never blocks first
  // paint (a brief land pop-in on load is the tradeoff).
  useEffect(() => {
    if (!ready) return
    const viewer = viewerRef.current
    if (!viewer || viewer.isDestroyed()) return
    const geojson = geojsonRef.current
    if (!geojson) return

    // basemap features are keyed by ISO numeric; map to our alpha-2 codes so
    // each land polygon can carry its country code as a pick id — clicking the
    // land is what selects a country now (no invisible marker billboards).
    const numericToCode = new Map()
    for (const [code, geo] of Object.entries(geoByCode)) {
      if (geo && geo.iso_numeric != null) numericToCode.set(parseInt(geo.iso_numeric, 10), code)
    }

    const instances = []
    const ringIdsByCode = new Map()
    const addRing = (ring, code) => {
      if (!ring || ring.length < 3) return
      const flat = []
      for (const [lng, lat] of ring) flat.push(lng, lat)

      let id
      if (code) {
        const ids = ringIdsByCode.get(code) ?? []
        id = `${code}#${ids.length}`
        ids.push(id)
        ringIdsByCode.set(code, ids)
      }

      instances.push(new Cesium.GeometryInstance({
        // `<code>#<ring>`: unique per ring so each can be recoloured on hover,
        // and parsed back to the country by codeFromPick. Left undefined for a
        // feature that resolves to no tracked country, which makes the ring
        // explicitly unpickable instead of carrying `id: undefined` by
        // accident — the polygon still draws, so the map keeps its coastlines.
        ...(id ? { id } : {}),
        geometry: new Cesium.PolygonGeometry({
          polygonHierarchy: new Cesium.PolygonHierarchy(Cesium.Cartesian3.fromDegreesArray(flat)),
          height: 250, // below choropleth (600) and borders (2000)
          vertexFormat: Cesium.PerInstanceColorAppearance.VERTEX_FORMAT,
        }),
        attributes: { color: Cesium.ColorGeometryInstanceAttribute.fromColor(LAND_COLOR) },
      }))
    }

    // The atlas ships ~241 features against the ~196 countries the API tracks,
    // so some land legitimately belongs to no selectable country (dependencies,
    // disputed areas, Antarctica). The warning exists to tell those apart from
    // a real country that is silently unclickable because its ISO numeric never
    // resolved — add an entry to NUMERIC_CODE_ALIASES when one shows up here.
    const unresolved = []

    for (const feature of geojson.features) {
      const numeric = parseInt(feature.id, 10)
      const code =
        numericToCode.get(numeric) ??
        NUMERIC_CODE_ALIASES.get(numeric) ??
        NAME_CODE_ALIASES.get(feature.properties?.name)
      if (!code) unresolved.push(`${feature.properties?.name ?? '?'} (${feature.id})`)
      const g = feature.geometry
      if (g.type === 'Polygon') addRing(g.coordinates[0], code)
      else if (g.type === 'MultiPolygon') g.coordinates.forEach((poly) => addRing(poly[0], code))
    }

    if (import.meta.env.DEV && unresolved.length) {
      console.warn(
        `globe: ${unresolved.length} land features map to no tracked country and are not clickable:\n  ` +
          unresolved.join('\n  '),
      )
    }

    if (instances.length === 0) return
    const primitive = new Cesium.Primitive({
      geometryInstances: instances,
      appearance: new Cesium.PerInstanceColorAppearance({ flat: true, translucent: false }),
      asynchronous: true,
    })
    viewer.scene.primitives.add(primitive)
    landRef.current = primitive
    landRingIdsRef.current = ringIdsByCode
    // Any tint from the previous primitive died with it, so drop the record of
    // it too — otherwise the next hover would try to restore a stale ring id.
    hoveredCodeRef.current = null

    return () => {
      if (landRef.current && !viewer.isDestroyed()) {
        viewer.scene.primitives.remove(landRef.current)
      }
      landRef.current = null
      landRingIdsRef.current = null
      hoveredCodeRef.current = null
    }
  }, [ready, geoByCode])

  // Composite-index choropleth: fill every country whose score we have, from
  // green (free) to crimson (most censored). Built as a single translucent
  // Primitive (one draw, like the border collection) rather than hundreds of
  // entities, and kept below the borders/markers by a small height offset.
  // Rebuilt when the index data, geometry, or toggle changes.
  useEffect(() => {
    if (!ready) return
    const viewer = viewerRef.current
    if (!viewer || viewer.isDestroyed()) return
    const geojson = geojsonRef.current
    if (!geojson) return

    if (choroplethRef.current) {
      viewer.scene.primitives.remove(choroplethRef.current)
      choroplethRef.current = null
    }
    if (!showIndex) return

    // basemap features are keyed by ISO numeric; map those to our alpha-2 codes.
    const numericToCode = new Map()
    for (const [code, geo] of Object.entries(geoByCode)) {
      if (geo && geo.iso_numeric != null) numericToCode.set(parseInt(geo.iso_numeric, 10), code)
    }

    const instances = []
    const addRing = (ring, color, code) => {
      if (!ring || ring.length < 3) return
      const flat = []
      for (const [lng, lat] of ring) flat.push(lng, lat)
      instances.push(new Cesium.GeometryInstance({
        // The choropleth sits above the land fill, so it — not the land — is
        // what a click over a scored country reaches. It therefore has to
        // carry the country code itself. See the note on allowPicking below.
        id: code,
        geometry: new Cesium.PolygonGeometry({
          polygonHierarchy: new Cesium.PolygonHierarchy(Cesium.Cartesian3.fromDegreesArray(flat)),
          height: 600, // above the black globe (avoids z-fight), below borders (2000)
          vertexFormat: Cesium.PerInstanceColorAppearance.VERTEX_FORMAT,
        }),
        attributes: { color: Cesium.ColorGeometryInstanceAttribute.fromColor(color) },
      }))
    }

    for (const feature of geojson.features) {
      const numeric = parseInt(feature.id, 10)
      if (Number.isNaN(numeric)) continue
      const code = numericToCode.get(numeric) ?? NUMERIC_CODE_ALIASES.get(numeric)
      if (!code) continue
      const score = indexByCode[code]
      if (score == null) continue
      const color = choroplethColor(score)
      const g = feature.geometry
      if (g.type === 'Polygon') addRing(g.coordinates[0], color, code)
      else if (g.type === 'MultiPolygon') g.coordinates.forEach((poly) => addRing(poly[0], color, code))
    }

    if (instances.length === 0) return
    const primitive = new Cesium.Primitive({
      geometryInstances: instances,
      appearance: new Cesium.PerInstanceColorAppearance({ flat: true, translucent: true }),
      asynchronous: false,
      // Deliberately pickable. This layer used to set `allowPicking: false` in
      // the belief that clicks would fall through to the land fill beneath —
      // they do not. `allowPicking: false` only stops a primitive from
      // producing a pick ID; its geometry still renders in the pick pass and
      // still writes depth, so it hid the land behind it. The effect was that
      // every country with an index score was unclickable on its landmass and
      // only the blooms above the choropleth could be hit. The fix is for the
      // topmost layer to carry the code, not to try to be invisible to picks.
    })
    viewer.scene.primitives.add(primitive)
    choroplethRef.current = primitive
  }, [ready, indexByCode, geoByCode, showIndex])

  // Satellite markers, rebuilt on every poll (App.jsx re-fetches every 5-10s —
  // this is what keeps them visibly moving instead of frozen at first load).
  // Full clear-and-readd rather than incremental per-point diffing: simpler,
  // and cheap enough at v1 object counts; worth revisiting if profiling at the
  // high end of the tracked-object-count target shows it as a bottleneck.
  useEffect(() => {
    if (!ready) return
    const viewer = viewerRef.current
    if (!viewer || viewer.isDestroyed()) return
    const points = satPointsRef.current
    if (!points) return

    points.removeAll()
    for (const sat of satellites) {
      if (sat.lat == null || sat.lon == null) continue
      const selected = sat.norad_id === selectedSatelliteId
      points.add({
        id: sat.norad_id,
        position: Cesium.Cartesian3.fromDegrees(sat.lon, sat.lat, satelliteDisplayHeight(sat.alt_km)),
        pixelSize: selected ? 5 : sat.category === 'starlink' ? 2 : 3,
        color: satelliteColor(sat.category),
        outlineColor: Cesium.Color.WHITE,
        outlineWidth: selected ? 2 : 0,
      })
    }
  }, [ready, satellites, selectedSatelliteId])

  // Selected satellite's orbit path: one full period, pre-split at the
  // antimeridian by the backend so each segment can be drawn as its own
  // polyline without a spurious wraparound line.
  //
  // Skipped for 'geo' category satellites: a circular LEO/MEO orbit draws as
  // one clean loop, but geosynchronous-family orbits (true GEO, plus inclined
  // ones like QZS) sample to a tight analemma/figure-8 near the satellite's
  // fixed longitude that reads as a confusing tangle rather than a path,
  // especially with the antimeridian-split logic never triggering to break it
  // up (its longitude barely moves). The period/alt/lat/lon in SatelliteCard
  // stay accurate either way — only this line is skipped.
  useEffect(() => {
    if (!ready) return
    const viewer = viewerRef.current
    if (!viewer || viewer.isDestroyed()) return
    const collection = satOrbitRef.current
    if (!collection) return

    collection.removeAll()
    if (!satelliteOrbit || selectedSatelliteCategory === 'geo') return

    // Plain white, not a per-category colour: the same track colour regardless
    // of which category is selected reads clearly as "this is the selection",
    // and stays legible against every category colour above.
    const orbitColor = Cesium.Color.WHITE.withAlpha(0.9)
    for (const segment of satelliteOrbit.segments) {
      if (segment.length < 2) continue
      const positions = segment.map((p) =>
        Cesium.Cartesian3.fromDegrees(p.lon, p.lat, satelliteDisplayHeight(p.alt_km)),
      )
      collection.add({
        positions,
        width: 1.5,
        material: Cesium.Material.fromType('Color', { color: orbitColor }),
      })
    }
  }, [ready, satelliteOrbit, selectedSatelliteCategory])

  // Submarine cable geometry: built exactly once, the first time `cables`
  // arrives with data, guarded by `cablesBuiltRef` since this is static
  // reference data with no reason to ever be rebuilt for the session. Each
  // route's `segments` array is drawn as one polyline per segment (never
  // flattened) — TeleGeography's own MultiLineString splitting already
  // avoids spurious antimeridian-crossing lines for most routes (verified:
  // 306/728 routes carry more than one segment), so no extra client-side
  // splitting is done here.
  useEffect(() => {
    if (!ready) return
    const viewer = viewerRef.current
    if (!viewer || viewer.isDestroyed()) return
    if (cablesBuiltRef.current) return
    if (!cables || !cables.routes?.length) return
    const routeCollection = cableRouteRef.current
    const landingCollection = cableLandingRef.current
    if (!routeCollection || !landingCollection) return

    for (const route of cables.routes) {
      const color = Cesium.Color.fromCssColorString(route.color || '#5a6472').withAlpha(0.55)
      for (const segment of route.segments) {
        if (segment.length < 2) continue
        const positions = segment.map(([lon, lat]) =>
          Cesium.Cartesian3.fromDegrees(lon, lat, CABLE_HEIGHT),
        )
        routeCollection.add({
          positions,
          width: 1,
          material: Cesium.Material.fromType('Color', { color }),
        })
      }
    }

    for (const point of cables.landing_points) {
      if (point.lon == null || point.lat == null) continue
      landingCollection.add({
        position: Cesium.Cartesian3.fromDegrees(point.lon, point.lat, CABLE_HEIGHT),
        pixelSize: 2,
        color: Cesium.Color.fromCssColorString('#c9cfd6').withAlpha(0.75),
      })
    }

    cablesBuiltRef.current = true
  }, [ready, cables])

  // Cable layer visibility: a plain `.show` flip on both collections, not
  // add/remove — the geometry above is only ever built once.
  useEffect(() => {
    if (cableRouteRef.current) cableRouteRef.current.show = showCables
    if (cableLandingRef.current) cableLandingRef.current.show = showCables
  }, [showCables])

  // Reactive camera framing: the globe follows the app-wide selection, whatever
  // set it — a marker click here or the country dropdown in the header. A
  // selected country is framed from its bbox-derived altitude; clearing the
  // selection flies back to the whole-globe HOME_VIEW. Keeping framing here
  // rather than in the click handler means both selection paths share one code
  // path (no double-fly) and a dropdown pick moves the camera too.
  useEffect(() => {
    if (!ready) return
    const viewer = viewerRef.current
    if (!viewer || viewer.isDestroyed()) return

    const prev = prevSelectedRef.current
    prevSelectedRef.current = selectedCode

    if (selectedCode) {
      const geo = geoByCode[selectedCode]
      if (!geo || geo.centroid_lon == null || geo.centroid_lat == null) return
      viewer.camera.flyTo({
        destination: Cesium.Cartesian3.fromDegrees(geo.centroid_lon, geo.centroid_lat, altitudeFor(geo)),
        duration: 1.2,
      })
    } else if (prev) {
      // A real deselect (not the empty selection on first load) → return to the
      // whole-globe framing. init already placed the camera at HOME_VIEW at
      // duration 0, so skipping the `!prev` case keeps startup instant.
      viewer.camera.flyTo({
        destination: Cesium.Cartesian3.fromDegrees(HOME_VIEW.lon, HOME_VIEW.lat, HOME_VIEW.height),
        duration: 1.5,
      })
    }
  }, [ready, selectedCode, geoByCode])

  return <div ref={containerRef} className="w-full h-full" />
}
