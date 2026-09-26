import { useEffect, useState } from 'react'
import { getCategories } from '../lib/api'
import { AMBER, BORDER, CRIMSON, DIM, LOCAL, MONO, MUTED, TYPE, WHITE, textTone } from '../theme'

const STATUS_COLOR = {
  HEAVILY_CENSORED: CRIMSON,
  PARTIALLY_CENSORED: AMBER,
  ACCESSIBLE: LOCAL,
  INCONCLUSIVE: DIM,
}

// Only categories with a real sample and at least a faint signal are worth a
// row — an ACCESSIBLE category at 1% anomaly is just noise in a "what's
// censored" view. Capped so the sidebar stays scannable.
const MIN_MEASUREMENTS = 100
const MIN_RATE = 0.03
const MAX_ROWS = 12

export default function CategoryBreakdown({ countryCode }) {
  const [rows, setRows] = useState(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false
    setRows(null)
    setError(false)

    getCategories(countryCode)
      .then((data) => {
        if (!cancelled) setRows(data)
      })
      .catch(() => {
        if (!cancelled) setError(true)
      })

    return () => {
      cancelled = true
    }
  }, [countryCode])

  if (error || !rows) return null

  const shown = rows
    .filter((r) => r.measurement_count >= MIN_MEASUREMENTS && r.anomaly_rate >= MIN_RATE)
    .slice(0, MAX_ROWS)

  if (shown.length === 0) return null

  return (
    <section>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginBottom: 8 }}>
        CONTENT CATEGORIES CENSORED
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        {shown.map((r) => {
          const color = STATUS_COLOR[r.status] ?? DIM
          // A true 0–100% track. Bars used to be scaled to the worst category,
          // so a 28% rate drew a full bar that read as near-total blocking;
          // category rates run low (they aggregate many URLs), and the honest
          // length still ranks them clearly.
          const width = Math.max(2, r.anomaly_rate * 100)
          return (
            <div key={r.category_code} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span
                style={{ fontSize: TYPE.body, color: WHITE, width: 140, flexShrink: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
                title={`${r.category_label} — ${r.measurement_count.toLocaleString()} measurements`}
              >
                {r.category_label}
              </span>
              <div style={{ flex: 1, height: 8, background: BORDER, position: 'relative' }}>
                <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: `${width}%`, background: color }} />
              </div>
              <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: textTone(color), width: 34, flexShrink: 0, textAlign: 'right' }}>
                {Math.round(r.anomaly_rate * 100)}%
              </span>
            </div>
          )
        })}
      </div>

      <div style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em', marginTop: 6 }}>
        % of OONI web-connectivity tests anomalous, by content category
      </div>
    </section>
  )
}
