import { BORDER, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'

// A column of header-dock panels floating at one side of the globe, as the
// panels did before the dock: an absolute overlay inside <main>, so opening a
// panel never resizes the globe or moves the censorship-index / cable legend
// pair centred at the bottom of <main>. At default zoom the columns sit in the
// empty space beside the globe. `maxHeight` stops each column above that
// legend row (12 top + 12 bottom + ~36 row + 12 gap = 72) so it never covers
// it; `cap` tightens it further for a panel that shouldn't run the full height.
// Panels in the same column stack vertically.
export function DockColumn({ side, width, cap, narrow = false, children }) {
  const clearLegends = 'calc(100% - 72px)'
  return (
    <aside
      style={{
        position: 'absolute',
        top: narrow ? 8 : 12,
        // Phone width: full-width (one panel at a time — see App).
        ...(narrow ? { left: 8, right: 8 } : { [side]: 12, width }),
        maxHeight: cap ? `min(${clearLegends}, ${cap}px)` : clearLegends,
        display: 'flex',
        flexDirection: 'column',
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        overflow: 'hidden',
        zIndex: 5,
      }}
    >
      {children}
    </aside>
  )
}

// One panel in a DockColumn: title row (optional `accessory`, then a close
// control), then the panel's content. `grow` panels fill the column up to its
// max height and scroll inside it; the others size to their content.
export function DockPanel({ title, accessory, onClose, grow, children }) {
  return (
    // .dock-panel (index.css) draws the rule between stacked panels only, so a
    // panel alone in its column doesn't double up with the column's border.
    <section
      aria-label={title}
      className="dock-panel"
      style={{
        display: 'flex',
        flexDirection: 'column',
        flex: grow ? '1 1 auto' : 'none',
        minHeight: 0,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '8px 10px',
          borderBottom: `1px solid ${BORDER}`,
          flexShrink: 0,
        }}
      >
        <span style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: WHITE, whiteSpace: 'nowrap' }}>
          {title}
        </span>
        <span style={{ marginLeft: 'auto' }}>{accessory}</span>
        <button
          type="button"
          onClick={onClose}
          aria-label={`Close ${title}`}
          style={{ background: 'transparent', border: 'none', color: MUTED, fontFamily: MONO, fontSize: TYPE.label, cursor: 'pointer', padding: 0 }}
        >
          ✕
        </button>
      </div>
      {children}
    </section>
  )
}
