import { useEffect, useState } from 'react'
import { getMessaging } from '../lib/api'
import { BLOCKING_STATUS_COLOR, BLOCKING_STATUS_LABEL } from '../lib/blockingRegistry'
import { BORDER, MONO, MUTED, TYPE, WHITE } from '../theme'

// technology_blocks stores messaging apps under keys; map to display names.
const APP_LABEL = {
  whatsapp: 'WhatsApp',
  telegram: 'Telegram',
  facebook_messenger: 'Messenger',
  signal_messenger: 'Signal',
}

// Order apps consistently regardless of DB row order.
const ORDER = ['whatsapp', 'telegram', 'facebook_messenger', 'signal_messenger']

export default function MessagingStatus({ countryCode }) {
  const [rows, setRows] = useState(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false
    setRows(null)
    setError(false)

    getMessaging(countryCode)
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

  // Three states that used to collapse into one blank space: still fetching,
  // the request failed, and OONI genuinely has no messaging measurements for
  // this country (47 of the drawable countries — Anguilla, Bhutan, Central
  // African Republic and similar). Rendering nothing for all three is what
  // makes a cold start look like a broken app.
  const status = error ? 'ERROR' : rows === null ? 'LOADING\u2026' : null

  const byApp = Object.fromEntries((rows ?? []).map((r) => [r.technology, r]))
  const shown = ORDER.map((key) => byApp[key]).filter((r) => r && r.measurement_count > 0)
  const label = status ?? (shown.length === 0 ? 'NO OONI COVERAGE' : null)

  if (label) {
    return (
      <section>
        <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginBottom: 8 }}>
          MESSAGING APPS
        </div>
        <p style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED }}>{label}</p>
      </section>
    )
  }

  return (
    <section>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginBottom: 8 }}>
        MESSAGING APPS
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
        {shown.map((r) => {
          const color = BLOCKING_STATUS_COLOR[r.status] ?? BORDER
          return (
            <div
              key={r.technology}
              title={`${r.measurement_count} OONI ${r.technology} measurements`}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                border: `1px solid ${color}`,
                padding: '3px 7px',
              }}
            >
              <span style={{ width: 6, height: 6, borderRadius: '50%', background: color, flexShrink: 0 }} />
              <span style={{ fontSize: TYPE.body, color: WHITE }}>{APP_LABEL[r.technology] ?? r.technology}</span>
              <span style={{ fontFamily: MONO, fontSize: TYPE.tick, color, letterSpacing: '0.05em' }}>
                {BLOCKING_STATUS_LABEL[r.status] ?? r.status}
              </span>
            </div>
          )
        })}
      </div>
    </section>
  )
}
