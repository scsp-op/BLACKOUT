import { BORDER, HIGHLIGHT, MONO, MUTED, RAISED, SIDEBAR, WHITE } from '../theme'

// Single source of truth for the satellite category taxonomy — the backend
// tags each object with one of these keys (see backend/src/fetchers/
// satellites.rs's DEFAULT_GROUPS). Anything the backend tags with a key that
// has no row here (by default 'geo' and 'other') is gathered into the derived
// "Other / Unclassified" row below, so the rows always sum to the catalog
// total. Globe.jsx and SatelliteCard.jsx import from here instead of keeping
// their own separate copies of the taxonomy.
export const SPACE_TRACKING_OPTIONS = [
  { key: 'starlink', label: 'Starlink / Comms' },
  { key: 'military', label: 'Military / Intel' },
  { key: 'navigation', label: 'GPS / Navigation' },
  { key: 'earthobs', label: 'Earth Observation' },
  { key: 'stations', label: 'Stations / Telescopes' },
]

// One distinct, thematically-grouped colour per category — deliberately more
// vivid than the app's muted dashboard palette (theme.js), which is tuned for
// text/chrome rather than for telling small dots apart at a glance. `geo`/
// `other` are background categories shown together in the derived
// "Other / Unclassified" row, so they get muted, non-competing neutrals.
export const CATEGORY_COLOR_HEX = {
  starlink: '#38bdf8', // sky blue — comms/signal
  navigation: '#fbbf24', // amber/gold — GPS/guidance
  military: '#ef4444', // red — restricted/alert
  earthobs: '#22c55e', // green — Earth/land
  stations: '#a78bfa', // violet — space science
  geo: '#94a3b8', // slate — background-only
  other: '#64748b', // darker slate — background-only
}

const NAMED_KEYS = new Set(SPACE_TRACKING_OPTIONS.map((o) => o.key))

// The backend's category set is configurable (SATELLITE_GROUPS), so the
// leftover bucket is derived from whatever `category_counts` actually returns
// rather than hardcoding 'geo,other'. That keeps the rows summing to `total`
// even if a new category is configured server-side without a row here. The
// fallback only matters on the first paint, before any counts have arrived.
const BACKGROUND_FALLBACK = ['geo', 'other']

function backgroundKeysFrom(counts) {
  const found = Object.keys(counts)
    .filter((k) => k !== 'total' && !NAMED_KEYS.has(k))
    .sort()
  return found.length ? found : BACKGROUND_FALLBACK
}

// The panel's only rule: separates the catalog-wide total from the
// per-category breakdown so the list reads as a whole and its parts rather
// than six sibling filters. The title needs no rule of its own — spacing
// already sets it apart, and a second line made the panel look striped.
const RowDivider = () => (
  <span aria-hidden="true" style={{ height: 1, background: BORDER, margin: '3px 0' }} />
)

// `hollow` marks the aggregate row: "All Satellites" is not a category, so it
// gets a ring rather than a filled swatch and stops implying that white is a
// colour in the taxonomy.
function Row({ active, color, label, count, onClick, hollow }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        width: '100%',
        // Selection reads as a highlighted row with a bar in the row's own
        // colour, which works identically on every row and says *which* layer
        // is live. The previous ring around the swatch rendered as a radio
        // target at twice the weight of the other rows and broke the rhythm.
        background: active ? RAISED : 'transparent',
        border: 'none',
        borderLeft: `2px solid ${active ? color : 'transparent'}`,
        padding: '2px 0 2px 5px',
        cursor: 'pointer',
        textAlign: 'left',
      }}
    >
      <span
        aria-hidden="true"
        style={{
          width: 6,
          height: 6,
          borderRadius: '50%',
          background: hollow ? 'transparent' : color,
          boxShadow: hollow ? `inset 0 0 0 1.5px ${color}` : 'none',
          flexShrink: 0,
        }}
      />
      <span
        style={{
          fontFamily: MONO,
          fontSize: 10,
          letterSpacing: '0.03em',
          color: active ? WHITE : MUTED,
          flex: 1,
          whiteSpace: 'nowrap',
        }}
      >
        {label}
      </span>
      {/* paddingLeft, not a bigger row `gap`: the gap also sits between the
          swatch and the label, and only this column needs the breathing room. */}
      <span style={{ fontFamily: MONO, fontSize: 9, paddingLeft: 8, color: active ? WHITE : MUTED }}>
        {count.toLocaleString()}
      </span>
    </button>
  )
}

// Single-select "space tracking" panel: one row is active at a time
// (`selection`), plus a "NONE" row that clears the layer entirely. Every
// row's count comes from the live-polled `counts` (App.jsx's
// spaceTrackingCounts, refreshed from every /api/satellites response's
// total/category_counts) regardless of which row is currently selected/
// fetched — so the whole list stays populated even while viewing one narrow
// category.
export default function SatelliteLegend({ selection, onSelect, counts }) {
  const backgroundKeys = backgroundKeysFrom(counts)
  const backgroundSelection = backgroundKeys.join(',')
  const backgroundCount = backgroundKeys.reduce((n, k) => n + (counts[k] ?? 0), 0)

  return (
    <div
      style={{
        position: 'absolute',
        top: 12,
        // GlobalRanking ("Most Censored Countries") occupies the whole left
        // edge at left:12, width:264 — sit just to its right, not on top of it.
        left: 292,
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        padding: '6px 8px',
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
        // Sized off the longest row, "Stations / Telescopes" (132px at MONO
        // 10px/0.03em), plus room for a 4-digit count. At the previous 196px
        // the widest label and a 5-digit count summed to exactly the space
        // available, so the two columns touched.
        width: 208,
        zIndex: 5,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 6,
        }}
      >
        <span style={{ fontFamily: MONO, fontSize: 10, letterSpacing: '0.1em', color: WHITE }}>
          SPACE TRACKING
        </span>
        <button
          type="button"
          onClick={() => onSelect('none')}
          aria-pressed={selection === 'none'}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 4,
            background: 'transparent',
            // Transparent until active: two boxed elements in a 196px header
            // row had the clear-layer button competing with the panel title.
            border: `1px solid ${selection === 'none' ? HIGHLIGHT : 'transparent'}`,
            color: selection === 'none' ? HIGHLIGHT : MUTED,
            fontFamily: MONO,
            fontSize: 9,
            letterSpacing: '0.08em',
            padding: '2px 5px',
            cursor: 'pointer',
          }}
        >
          NONE ✕
        </button>
      </div>

      <Row
        active={selection === 'all'}
        color={selection === 'all' ? WHITE : MUTED}
        hollow
        label="All Satellites"
        count={counts.total ?? 0}
        onClick={() => onSelect('all')}
      />
      <RowDivider />

      {SPACE_TRACKING_OPTIONS.map(({ key, label }) => (
        <Row
          key={key}
          active={selection === key}
          color={CATEGORY_COLOR_HEX[key]}
          label={label}
          count={counts[key] ?? 0}
          onClick={() => onSelect(key)}
        />
      ))}

      {/* Everything the backend tags outside the five rows above. The key is a
          comma-separated list because /api/satellites takes one (see
          SatellitesQuery.categories), so this row filters the globe like any
          other. One swatch stands for both background neutrals — they are two
          near-identical slates by design.

          Labelled "Other", not "Unclassified": the bucket includes `geo`,
          which is a real classification, and on a tool that lists "Military /
          Intel" two rows up, "unclassified" reads as a security marking rather
          than "uncategorised". */}
      <Row
        active={selection === backgroundSelection}
        color={CATEGORY_COLOR_HEX.geo}
        label="Other"
        count={backgroundCount}
        onClick={() => onSelect(backgroundSelection)}
      />
    </div>
  )
}
