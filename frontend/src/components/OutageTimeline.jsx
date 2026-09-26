import { useEffect, useState } from 'react'
import {
  CartesianGrid,
  Cell,
  ResponsiveContainer,
  Scatter,
  ScatterChart,
  Tooltip,
  XAxis,
  YAxis,
  ZAxis,
} from 'recharts'
import { BORDER, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'
import { SEVERITY_COLOR, severityLabel } from './OutageFeed'

// Severity bands, bottom to top, as y positions 0/1/2.
const BANDS = ['MINOR', 'MAJOR', 'SEVERE']

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
      <div style={{ color: SEVERITY_COLOR[p.severity] }}>{p.severity.toLowerCase()} · {formatDuration(p.durationSecs)}</div>
      <div style={{ color: MUTED }}>IODA score {Math.round(p.score)}</div>
      <div style={{ color: MUTED }}>source: {p.datasource}</div>
    </div>
  )
}

// Discrete internet-outage events over the trailing 90-day window. Each point
// is one IODA-detected disruption, placed in its severity band (minor / major
// / severe — the same thresholds and colours as the Outages panel) and sized
// by duration. The y-axis used to be the raw IODA score (0–8000, no unit),
// which is unbounded and datasource-relative — meaningless to a lay reader,
// and it let one extreme event flatten everything else. The score is still in
// the tooltip.
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

  const points = rows.map((e, i) => {
    const severity = severityLabel(e.score)
    return {
      t: e.start_ts * 1000,
      // Band centre plus a small deterministic offset, so same-day events in
      // one band don't draw exactly on top of each other.
      y: BANDS.indexOf(severity) + (((i * 37) % 11) / 11 - 0.5) * 0.44,
      severity,
      score: e.score,
      durationSecs: e.duration_secs,
      datasource: e.datasource,
    }
  })

  const times = points.map((p) => p.t)
  const min = Math.min(...times)
  const max = Math.max(...times)
  const severe = points.filter((p) => p.severity === 'SEVERE').length

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
              dataKey="y"
              name="severity"
              domain={[-0.5, 2.5]}
              ticks={[0, 1, 2]}
              tickFormatter={(i) => BANDS[i]?.toLowerCase() ?? ''}
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
              width={44}
            />
            <ZAxis type="number" dataKey="durationSecs" range={[24, 180]} />
            <Tooltip content={<OutageTooltip />} cursor={{ stroke: BORDER }} />
            <Scatter data={points} fillOpacity={0.8}>
              {points.map((p, i) => (
                <Cell key={i} fill={SEVERITY_COLOR[p.severity]} />
              ))}
            </Scatter>
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
