import { useEffect, useState } from 'react'
import {
  CartesianGrid,
  ResponsiveContainer,
  Scatter,
  ScatterChart,
  Tooltip,
  XAxis,
  YAxis,
  ZAxis,
} from 'recharts'
import { BORDER, CRIMSON, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'

const HEIGHT = 150

function formatDay(ms) {
  const d = new Date(ms)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
}

function formatDuration(secs) {
  if (secs >= 3600) return `${(secs / 3600).toFixed(1)}h`
  return `${Math.round(secs / 60)}m`
}

function OutageTooltip({ active, payload }) {
  if (!active || !payload || !payload.length) return null
  const p = payload[0].payload
  return (
    <div style={{ background: SIDEBAR, border: `1px solid ${BORDER}`, padding: '6px 8px', fontFamily: MONO, fontSize: TYPE.label }}>
      <div style={{ color: WHITE }}>{new Date(p.t).toLocaleString('en-US', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}</div>
      <div style={{ color: MUTED }}>severity {Math.round(p.score)} · {formatDuration(p.durationSecs)}</div>
      <div style={{ color: MUTED }}>source: {p.datasource}</div>
    </div>
  )
}

// Discrete internet-outage events over the trailing 90-day window. Each point
// is one IODA-detected disruption; y-position is severity (score), so a
// vertical run of high dots reads as a period of serious disruption.
export default function OutageTimeline({ countryCode }) {
  const [rows, setRows] = useState(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false
    setRows(null)
    setError(false)

    fetch(`/api/outages?country=${countryCode}`)
      .then((r) => {
        if (!r.ok) throw new Error('Failed to fetch outages')
        return r.json()
      })
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

  if (error || !rows || rows.length === 0) return null

  const points = rows.map((e) => ({
    t: e.start_ts * 1000,
    score: e.score,
    durationSecs: e.duration_secs,
    datasource: e.datasource,
  }))

  const times = points.map((p) => p.t)
  const min = Math.min(...times)
  const max = Math.max(...times)
  const severe = points.filter((p) => p.score >= 200).length

  return (
    <section>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginBottom: 8 }}>
        INTERNET OUTAGES (90D)
      </div>
      <div style={{ width: '100%', height: HEIGHT }}>
        <ResponsiveContainer width="100%" height="100%">
          <ScatterChart margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
            <CartesianGrid stroke={BORDER} vertical={false} />
            <XAxis
              type="number"
              dataKey="t"
              domain={[min, max]}
              // One label when every outage falls on the same day — two
              // identical dates would otherwise draw on top of each other.
              ticks={formatDay(min) === formatDay(max) ? [min] : [min, max]}
              tickFormatter={formatDay}
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
            />
            <YAxis
              type="number"
              dataKey="score"
              name="severity"
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
              width={38}
            />
            <ZAxis type="number" dataKey="durationSecs" range={[24, 180]} />
            <Tooltip content={<OutageTooltip />} cursor={{ stroke: BORDER }} />
            <Scatter data={points} fill={CRIMSON} fillOpacity={0.7} />
          </ScatterChart>
        </ResponsiveContainer>
      </div>
      <div style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em', marginTop: 2 }}>
        {points.length} outage{points.length === 1 ? '' : 's'} detected
        {severe > 0 && ` · ${severe} severe`} · via IODA
      </div>
    </section>
  )
}
