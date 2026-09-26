import { useEffect, useState } from 'react'
import { AMBER, CRIMSON, DIM, LOCAL, MONO, MUTED, TYPE, WHITE } from '../theme'

// Which sources this component renders, and how to label them.
const SOURCES = [
  { key: 'V_DEM', label: 'V-Dem — Freedom of Expression' },
  { key: 'RSF', label: 'RSF — Press Freedom' },
]

// V-Dem's internet-censorship indicators, shown as sub-scores under the V-Dem
// row. All three are coded higher = less censorship, so they read the same
// direction as every other score here despite names ("censorship effort")
// that suggest the opposite. V-Dem publishes a credible interval rather than a
// point estimate; the point is shown and the interval kept on hover.
const VDEM_SUBSCORES = [
  { label: 'Filtering', key: 'score_vdem_filtering' },
  { label: 'Shutdown-free', key: 'score_vdem_shutdown' },
  { label: 'Censorship effort', key: 'score_vdem_censorship' },
]

// All scores are normalised to 0–100, higher = more free, so one colour ramp
// reads consistently across indices.
function scoreColor(score) {
  if (score >= 60) return LOCAL
  if (score >= 35) return AMBER
  return CRIMSON
}

export default function GlobalIndices({ countryCode }) {
  const [rows, setRows] = useState(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false
    setRows(null)
    setError(false)

    fetch(`/api/country-scores?country=${countryCode}`)
      .then((r) => (r.ok ? r.json() : []))
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

  const bySource = Object.fromEntries(rows.map((r) => [r.source, r]))
  const present = SOURCES.map((s) => ({ ...s, row: bySource[s.key] })).filter(
    (s) => s.row && s.row.score_overall != null,
  )

  if (present.length === 0) return null

  return (
    <section>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginBottom: 8 }}>
        GLOBAL FREEDOM INDICES
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {present.map(({ key, label, row }) => {
          const score = row.score_overall
          const color = scoreColor(score)
          return (
            <div key={key}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 3 }}>
                <span style={{ fontSize: TYPE.body, color: WHITE }}>{label}</span>
                <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>{row.year}</span>
              </div>
              <div style={{ height: 6, background: DIM, position: 'relative' }}>
                <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: `${Math.max(0, Math.min(100, score))}%`, background: color }} />
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 3 }}>
                <span style={{ fontFamily: MONO, fontSize: TYPE.label, color }}>{row.classification}</span>
                <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>{Math.round(score)}/100 free</span>
              </div>

              {key === 'V_DEM' && VDEM_SUBSCORES.some((s) => row[s.key] != null) && (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2, marginTop: 5 }}>
                  {VDEM_SUBSCORES.map((s) => {
                    const value = row[s.key]
                    if (value == null) return null
                    const low = row[`${s.key}_low`]
                    const high = row[`${s.key}_high`]
                    const interval =
                      low != null && high != null
                        ? `V-Dem credible interval ${low}–${high} of 100`
                        : 'no published interval'
                    return (
                      <div
                        key={s.key}
                        style={{ display: 'flex', justifyContent: 'space-between', cursor: 'help' }}
                        title={`${s.label} — ${value}/100 free · ${interval}`}
                      >
                        <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>{s.label}</span>
                        <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: scoreColor(value) }}>
                          {Math.round(value)}/100
                        </span>
                      </div>
                    )
                  })}
                </div>
              )}
            </div>
          )
        })}
      </div>

      <div style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em', marginTop: 6 }}>
        Normalized so higher = more free · RSF shown is the pre-2022 index
      </div>
    </section>
  )
}
