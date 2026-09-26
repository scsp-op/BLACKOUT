import { AMBER, CRIMSON, MONO, MUTED, TYPE, WHITE } from '../theme'

// Compact age: "12m", "5h", "3d". The row tooltip spells it out.
function relativeTime(unixSecs) {
  if (!unixSecs) return ''
  const diff = Date.now() / 1000 - unixSecs
  if (diff < 0) return 'now'
  const h = diff / 3600
  if (h < 1) return `${Math.max(1, Math.round(diff / 60))}m`
  if (h < 48) return `${Math.round(h)}h`
  return `${Math.round(h / 24)}d`
}

// Compact one-line severity read from IODA's score magnitude. IODA scores are
// unbounded and datasource-relative, so this is an ordinal cue, not a unit.
// Shared with OutageTimeline so both read severity the same way.
export function severityLabel(score) {
  if (score >= 200) return 'SEVERE'
  if (score >= 60) return 'MAJOR'
  return 'MINOR'
}

export const SEVERITY_COLOR = { SEVERE: CRIMSON, MAJOR: AMBER, MINOR: MUTED }

const Dot = ({ color }) => (
  <span aria-hidden="true" style={{ width: 6, height: 6, borderRadius: '50%', background: color, flexShrink: 0 }} />
)

// Content only — App renders this inside the header dock's OUTAGES panel
// (DockPanel), which supplies the frame, title, count and close control.
// Deliberately the most compact panel, since it's the least essential: rows
// as tight as the ranking list, severity as a coloured dot rather than a word,
// and the ISO code / full severity / age spelled out in each row's tooltip.
export default function OutageFeed({ outages = [] }) {
  return (
    <>
      <div style={{ flex: 1, minHeight: 0, overflowY: 'auto', padding: '4px 0' }}>
        {!outages.length && (
          <p style={{ padding: '4px 10px', fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>No current outages.</p>
        )}
        {outages.map((o) => {
          const severity = severityLabel(o.maxScore)
          const age = relativeTime(o.latestStart)
          return (
            <div
              key={o.code}
              title={`${o.name} (${o.code}) · ${severity}${age ? ` · started ${age === 'now' ? 'just now' : `${age} ago`}` : ''}`}
              style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 10px' }}
            >
              <Dot color={SEVERITY_COLOR[severity]} />
              <span
                style={{
                  fontSize: TYPE.body,
                  color: WHITE,
                  flex: 1,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {o.name}
              </span>
              <span className="tabular" style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED, flexShrink: 0 }}>
                {age}
              </span>
            </div>
          )
        })}
      </div>
      <div style={{ padding: '6px 10px', fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 3 }}>
          {Object.entries(SEVERITY_COLOR).map(([label, color]) => (
            <span key={label} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              <Dot color={color} />
              {label.toLowerCase()}
            </span>
          ))}
        </div>
        via IODA (BGP / active probing / telescope)
      </div>
    </>
  )
}
