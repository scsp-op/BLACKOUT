import { BORDER, BORDER_STRONG, HIGHLIGHT, MONO, MUTED, RAISED, SIDEBAR, TYPE, WHITE } from '../theme'

// Static/decorative infrastructure context, not a censorship signal — a
// single binary toggle (unlike SatelliteLegend's multi-select), default off
// so it doesn't compete with the always-relevant layers on first load.
//
// Unpositioned on purpose — App.jsx renders this inside a shared, centered
// bottom row alongside IndexLegend, so the pair centers as one group
// regardless of either panel's content-driven width. Pixel-offset placements
// tried earlier (squeezed next to OutageFeed, then stacked under SPACE
// TRACKING) each broke at some combination of viewport width and sidebar
// state; a flex-centered group has no such dependency.
//
// `compact` is the phone-width variant: short title, no counts (they stay in
// the tooltip).
export default function CableLegend({ show, onToggle, routeCount, landingCount, compact = false }) {
  return (
    <div
      title={`${routeCount} cables · ${landingCount} landing points · via TeleGeography`}
      style={{
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        padding: compact ? '4px 8px' : '4px 10px',
        display: 'flex',
        alignItems: 'center',
        gap: compact ? 6 : 10,
        flexShrink: 0,
        whiteSpace: 'nowrap',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 10 }}>
        <span style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: WHITE }}>
          {compact ? 'CABLES' : 'SUBMARINE CABLES'}
        </span>
        {!compact && (
          <span style={{ fontFamily: MONO, fontSize: TYPE.tick, letterSpacing: '0.03em', color: MUTED }}>
            {routeCount} routes · {landingCount} landings
          </span>
        )}
      </div>

      <button
        type="button"
        onClick={onToggle}
        aria-pressed={show}
        style={{
          background: show ? 'rgba(214, 179, 106, 0.08)' : RAISED,
          border: `1px solid ${show ? HIGHLIGHT : BORDER_STRONG}`,
          color: show ? HIGHLIGHT : WHITE,
          fontFamily: MONO,
          fontSize: TYPE.tick,
          letterSpacing: '0.08em',
          padding: compact ? '4px 8px' : '4px 10px',
          minWidth: compact ? 44 : 52,
          marginLeft: compact ? 0 : 4,
          cursor: 'pointer',
        }}
      >
        {show ? 'HIDE' : 'SHOW'}
      </button>
    </div>
  )
}
