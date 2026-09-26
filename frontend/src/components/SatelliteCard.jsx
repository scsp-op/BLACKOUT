import { BORDER, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'
import { SPACE_TRACKING_OPTIONS } from './SatelliteLegend'

// Shared with the legend's row labels for the 5 headline categories; `geo`/
// `other` aren't headline rows there (no dedicated legend row, only reachable
// via "All Satellites") so they get a small local fallback here instead.
const CATEGORY_LABEL = {
  ...Object.fromEntries(SPACE_TRACKING_OPTIONS.map(({ key, label }) => [key, label])),
  geo: 'GEO',
  other: 'Other',
}

// Detail panel for the currently-selected satellite. `satellite` comes
// straight from the live-polled position list — its lat/lon/alt refresh with
// every poll for free, without a fetch of its own. `periodMinutes` comes from
// the separate orbit-path fetch (App.jsx), since only that endpoint computes it.
export default function SatelliteCard({ satellite, periodMinutes, onClose, left = 12 }) {
  if (!satellite) return null

  return (
    <div
      style={{
        position: 'absolute',
        top: 12,
        // Top-left of the globe; App shifts it right of the floating left dock
        // column while that column is open.
        left,
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        padding: '10px 12px',
        width: 220,
        zIndex: 5,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between' }}>
        <div style={{ fontSize: TYPE.title, fontWeight: 500, color: WHITE }}>{satellite.name}</div>
        <button
          type="button"
          onClick={onClose}
          style={{ background: 'transparent', border: 'none', color: MUTED, fontSize: TYPE.display, lineHeight: 1, cursor: 'pointer' }}
        >
          ×
        </button>
      </div>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED, marginTop: 2, marginBottom: 8 }}>
        NORAD {satellite.norad_id} · {CATEGORY_LABEL[satellite.category] ?? satellite.category}
      </div>
      <dl style={{ display: 'grid', gridTemplateColumns: 'auto 1fr', gap: '4px 8px', margin: 0, fontFamily: MONO, fontSize: TYPE.label }}>
        <dt style={{ color: MUTED }}>ALT</dt>
        <dd style={{ margin: 0, color: WHITE }}>{satellite.alt_km.toFixed(1)} km</dd>
        <dt style={{ color: MUTED }}>LAT</dt>
        <dd style={{ margin: 0, color: WHITE }}>{satellite.lat.toFixed(2)}°</dd>
        <dt style={{ color: MUTED }}>LON</dt>
        <dd style={{ margin: 0, color: WHITE }}>{satellite.lon.toFixed(2)}°</dd>
        {periodMinutes != null && (
          <>
            <dt style={{ color: MUTED }}>PERIOD</dt>
            <dd style={{ margin: 0, color: WHITE }}>{periodMinutes.toFixed(1)} min</dd>
          </>
        )}
      </dl>
      <a
        href={`https://www.n2yo.com/satellite/?s=${satellite.norad_id}`}
        target="_blank"
        rel="noopener noreferrer"
        style={{
          display: 'block',
          marginTop: 10,
          textAlign: 'center',
          fontFamily: MONO,
          fontSize: TYPE.label,
          letterSpacing: '0.08em',
          color: MUTED,
          border: `1px solid ${BORDER}`,
          padding: '4px 0',
          textDecoration: 'none',
        }}
      >
        TRACK ON N2YO →
      </a>
    </div>
  )
}
