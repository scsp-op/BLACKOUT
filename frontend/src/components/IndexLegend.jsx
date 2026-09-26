import { BORDER, HIGHLIGHT, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'

// The globe choropleth ramp (Globe.jsx CHORO_STOPS) as it renders on the map —
// each stop's colour at its opacity over the dark land fill — so the key
// matches what the eye sees rather than the brighter raw colours.
const RAMP = 'linear-gradient(90deg, #273d4b 0%, #735224 50%, #c74057 100%)'

// Unpositioned on purpose — App.jsx renders this inside a shared, centered
// bottom row alongside CableLegend, so the pair centers as one group
// regardless of either panel's content-driven width.
export default function IndexLegend({ show, onToggle }) {
  return (
    <div
      style={{
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        padding: '7px 10px',
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        flexShrink: 0,
        whiteSpace: 'nowrap',
      }}
    >
      <span style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: WHITE }}>
        CENSORSHIP INDEX
      </span>

      <div style={{ display: 'flex', alignItems: 'center', gap: 6, opacity: show ? 1 : 0.4 }}>
        <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>Free</span>
        <div style={{ width: 120, height: 8, background: RAMP, border: `1px solid ${BORDER}` }} />
        <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>Censored</span>
      </div>

      <button
        type="button"
        onClick={onToggle}
        aria-pressed={show}
        style={{
          background: 'transparent',
          border: `1px solid ${show ? HIGHLIGHT : BORDER}`,
          color: show ? HIGHLIGHT : MUTED,
          fontFamily: MONO,
          fontSize: TYPE.label,
          letterSpacing: '0.08em',
          padding: '2px 8px',
          cursor: 'pointer',
        }}
      >
        {show ? 'HIDE' : 'SHOW'}
      </button>

      {/* The composite is V-Dem + RSF only — see backend/src/api/censorship_index.rs.
          No Freedom House data is fetched, stored or weighted anywhere in the system. */}
      <span style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED }}>V-Dem · RSF blend</span>
    </div>
  )
}
