import { useEffect, useState } from 'react'
import { AMBER, CRIMSON, DIM, LOCAL, MONO, MUTED, TYPE, WHITE } from '../theme'

// Internet Society Pulse's Internet Resilience Index. This is an
// infrastructure measure — whether a country's network holds up — not a
// freedom one, which is why it renders beside the outage history rather than
// inside GLOBAL FREEDOM INDICES. A heavily-censored country can score well
// here, and that is the point.
const PILLARS = [
  { label: 'Infrastructure', key: 'score_pulse_infrastructure' },
  { label: 'Performance', key: 'score_pulse_performance' },
  { label: 'Security', key: 'score_pulse_security' },
  { label: 'Market readiness', key: 'score_pulse_market_readiness' },
]

// Same ramp as the other 0–100 panels: higher = better.
function scoreColor(score) {
  if (score >= 60) return LOCAL
  if (score >= 35) return AMBER
  return CRIMSON
}

// Three distinct states, deliberately. An earlier panel in this project used
// `null` for both "still fetching" and "this country has no data", which made
// a real gap indistinguishable from a slow request — Pulse rates 179
// countries, so ~57 of the globe legitimately have nothing and must say so.
const LOADING = 'loading'
const MISSING = 'missing'

export default function ResilienceIndex({ countryCode }) {
  const [state, setState] = useState(LOADING)

  useEffect(() => {
    let cancelled = false
    setState(LOADING)

    fetch(`/api/country-scores?country=${countryCode}`)
      .then((r) => (r.ok ? r.json() : []))
      .then((data) => {
        if (cancelled) return
        const row = data.find((d) => d.source === 'ISOC_PULSE')
        setState(row && row.score_overall != null ? row : MISSING)
      })
      .catch(() => {
        // A failed request is not the same as "no data published", but from
        // the panel's side both mean there is nothing trustworthy to show.
        if (!cancelled) setState(MISSING)
      })

    return () => {
      cancelled = true
    }
  }, [countryCode])

  if (state === LOADING) return null

  return (
    <section>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginBottom: 8 }}>
        INTERNET RESILIENCE
      </div>

      {state === MISSING ? (
        <div style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>No IRI data</div>
      ) : (
        <>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 3 }}>
            <span style={{ fontSize: TYPE.body, color: WHITE }}>Pulse Resilience Index</span>
            <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>{state.year}</span>
          </div>
          <div style={{ height: 6, background: DIM, position: 'relative' }}>
            <div
              style={{
                position: 'absolute',
                left: 0,
                top: 0,
                bottom: 0,
                width: `${Math.max(0, Math.min(100, state.score_overall))}%`,
                background: scoreColor(state.score_overall),
              }}
            />
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 3 }}>
            <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: scoreColor(state.score_overall) }}>
              {state.classification}
            </span>
            <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>
              {Math.round(state.score_overall)}/100 resilient
            </span>
          </div>

          {PILLARS.some((p) => state[p.key] != null) && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2, marginTop: 5 }}>
              {PILLARS.map((p) => {
                const value = state[p.key]
                if (value == null) return null
                return (
                  <div key={p.key} style={{ display: 'flex', justifyContent: 'space-between' }}>
                    <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>{p.label}</span>
                    <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: scoreColor(value) }}>
                      {Math.round(value)}/100
                    </span>
                  </div>
                )
              })}
            </div>
          )}
        </>
      )}
    </section>
  )
}
