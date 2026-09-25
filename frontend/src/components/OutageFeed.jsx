import { useState } from 'react'
import { BORDER, CRIMSON, MONO, MUTED, SIDEBAR, WHITE } from '../theme'

function relativeTime(unixSecs) {
  if (!unixSecs) return ''
  const diff = Date.now() / 1000 - unixSecs
  if (diff < 0) return 'now'
  const h = diff / 3600
  if (h < 1) return `${Math.max(1, Math.round(diff / 60))}m ago`
  if (h < 48) return `${Math.round(h)}h ago`
  return `${Math.round(h / 24)}d ago`
}

// Compact one-line severity read from IODA's score magnitude. IODA scores are
// unbounded and datasource-relative, so this is an ordinal cue, not a unit.
function severityLabel(score) {
  if (score >= 200) return 'SEVERE'
  if (score >= 60) return 'MAJOR'
  return 'MINOR'
}

export default function OutageFeed({ outages = [] }) {
  const [collapsed, setCollapsed] = useState(false)

  if (!outages.length) return null

  return (
    <div
      style={{
        position: 'absolute',
        top: 12,
        right: 12,
        width: 260,
        // 24px of margin, plus 45px to clear the index/cable legend row that
        // sits at the bottom of the same column.
        maxHeight: 'calc(100% - 69px)',
        display: 'flex',
        flexDirection: 'column',
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        zIndex: 5,
      }}
    >
      <style>{`@keyframes outagePulse { 0%,100% { opacity: 1 } 50% { opacity: 0.25 } }`}</style>

      <button
        type="button"
        onClick={() => setCollapsed((c) => !c)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          width: '100%',
          background: 'transparent',
          border: 'none',
          borderBottom: collapsed ? 'none' : `1px solid ${BORDER}`,
          padding: '8px 10px',
          cursor: 'pointer',
          textAlign: 'left',
        }}
      >
        <span
          style={{
            width: 7,
            height: 7,
            borderRadius: '50%',
            background: CRIMSON,
            flexShrink: 0,
            animation: 'outagePulse 1.4s ease-in-out infinite',
          }}
        />
        <span style={{ fontFamily: MONO, fontSize: 10, letterSpacing: '0.1em', color: WHITE }}>
          INTERNET OUTAGES
        </span>
        <span style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: 10, color: CRIMSON }}>
          {outages.length}
        </span>
        <span style={{ fontFamily: MONO, fontSize: 10, color: MUTED }}>{collapsed ? '+' : '\u2212'}</span>
      </button>

      {!collapsed && (
        <>
          <div style={{ overflowY: 'auto' }}>
            {outages.map((o) => (
              <div
                key={o.code}
                style={{
                  display: 'flex',
                  alignItems: 'baseline',
                  gap: 8,
                  padding: '6px 10px',
                  borderBottom: `1px solid ${BORDER}`,
                }}
              >
                <span style={{ fontFamily: MONO, fontSize: 10, color: MUTED, width: 22, flexShrink: 0 }}>
                  {o.code}
                </span>
                <span
                  style={{
                    fontSize: 11,
                    color: WHITE,
                    flex: 1,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                  title={o.name}
                >
                  {o.name}
                </span>
                <span style={{ fontFamily: MONO, fontSize: 9, color: CRIMSON, flexShrink: 0 }}>
                  {severityLabel(o.maxScore)}
                </span>
                <span style={{ fontFamily: MONO, fontSize: 9, color: MUTED, flexShrink: 0, width: 48, textAlign: 'right' }}>
                  {relativeTime(o.latestStart)}
                </span>
              </div>
            ))}
          </div>
          <div style={{ padding: '6px 10px', fontFamily: MONO, fontSize: 9, color: MUTED }}>
            Detected via IODA (BGP / active probing / telescope)
          </div>
        </>
      )}
    </div>
  )
}
