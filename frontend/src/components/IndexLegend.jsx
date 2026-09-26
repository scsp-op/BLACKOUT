import { BORDER, HIGHLIGHT, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'

// Matches the globe choropleth ramp in Globe.jsx (green → amber → crimson).
const RAMP = 'linear-gradient(90deg, #6c9a5b 0%, #d97706 50%, #b31942 100%)'

// Unpositioned on purpose — App.jsx renders this inside a shared, centered
// bottom row alongside CableLegend, so the pair centers as one group
// regardless of either panel's content-driven width.
//
// `compact` is the phone-width variant: shorter title, shorter ramp, smaller
// labels and no source note, so it fits beside CableLegend on a phone.
export default function IndexLegend({ show, onToggle, compact = false }) {
  return (
    <div
      style={{
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        padding: compact ? '6px 8px' : '7px 10px',
        display: 'flex',
        alignItems: 'center',
        gap: compact ? 8 : 10,
        flexShrink: 0,
        whiteSpace: 'nowrap',
      }}
    >
      <span style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: WHITE }}>
        {compact ? 'INDEX' : 'CENSORSHIP INDEX'}
      </span>

      <div style={{ display: 'flex', alignItems: 'center', gap: 6, opacity: show ? 1 : 0.4 }}>
        <span style={{ fontFamily: MONO, fontSize: compact ? TYPE.tick : TYPE.label, color: MUTED }}>Free</span>
        <div style={{ width: compact ? 56 : 120, height: 8, background: RAMP, border: `1px solid ${BORDER}` }} />
        <span style={{ fontFamily: MONO, fontSize: compact ? TYPE.tick : TYPE.label, color: MUTED }}>Censored</span>
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
      {!compact && <span style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED }}>V-Dem · RSF blend</span>}
    </div>
  )
}
