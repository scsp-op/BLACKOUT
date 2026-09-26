import { useEffect, useState } from 'react'
import {
  ComposedChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceArea,
  ReferenceLine,
  ResponsiveContainer,
} from 'recharts'
import { BORDER, CRIMSON, MONO, MUTED, TYPE, US_EXPOSURE, WHITE } from '../theme'

// Most labels this axis will ever draw.
//
// One tick per month was already a big reduction from one per day, but the
// series spans ~2.5 years, so it still produced 31 labels. Recharts renders
// every tick handed to it explicitly — it does not thin them — and 31 ×
// "2026-07" cannot fit the 360px sidebar, so they collided and clipped. Five is
// what reads cleanly at that width.
const MAX_TICKS = 5

// One tick per month, then thinned to at most MAX_TICKS by taking every Nth.
// The last month is always kept: the right edge is where the eye lands to ask
// "how current is this?", and dropping it to satisfy the stride is the one
// omission a reader would actually notice.
function monthlyTicks(rows) {
  const seen = new Set()
  const months = []
  for (const row of rows) {
    const month = row.date.slice(0, 7)
    if (!seen.has(month)) {
      seen.add(month)
      months.push(row.date)
    }
  }
  if (months.length <= MAX_TICKS) return months

  const stride = Math.ceil(months.length / MAX_TICKS)
  const ticks = months.filter((_, i) => i % stride === 0)
  const last = months[months.length - 1]
  if (ticks[ticks.length - 1] !== last) {
    // Replace rather than append, so the stride never leaves two labels
    // adjacent enough to overlap again at the right edge.
    if (ticks.length >= MAX_TICKS) ticks.pop()
    ticks.push(last)
  }
  return ticks
}

// Consecutive HIGH_BLOCKING dates (within 2 days of each other) collapse
// into one event range instead of one reference line per day — a single
// blocking episode of 10 days shouldn't render as 10 separate markers.
function groupConsecutiveDates(dates, maxGapDays = 2) {
  if (dates.length === 0) return []
  const sorted = [...dates].sort()
  const groups = [[sorted[0]]]

  for (let i = 1; i < sorted.length; i++) {
    const gapDays = (new Date(sorted[i]) - new Date(sorted[i - 1])) / 86_400_000
    if (gapDays <= maxGapDays) {
      groups[groups.length - 1].push(sorted[i])
    } else {
      groups.push([sorted[i]])
    }
  }

  return groups.map((group) => ({ start: group[0], end: group[group.length - 1] }))
}

// Tor publishes per-transport figures as a low/high interval, not a count.
// The midpoint is what's shown; both bounds ride along in the API response and
// in the title tooltip, so the published uncertainty isn't lost.
const TRANSPORTS = [
  { label: 'obfs4', low: 'obfs4_low', high: 'obfs4_high' },
  { label: 'Snowflake', low: 'snowflake_low', high: 'snowflake_high' },
  { label: 'webtunnel', low: 'webtunnel_low', high: 'webtunnel_high' },
]

function midpoint(low, high) {
  if (low == null && high == null) return null
  if (low == null) return high
  if (high == null) return low
  return Math.round((low + high) / 2)
}

// The combined CSV lags the per-country one by a day or two, so the last row
// overall often has no transport split. Walk back to the most recent row that
// actually carries one rather than rendering an empty breakdown.
function latestWithTransports(rows) {
  for (let i = rows.length - 1; i >= 0; i--) {
    const row = rows[i]
    if (TRANSPORTS.some((t) => row[t.low] != null || row[t.high] != null)) return row
  }
  return null
}

// One of the two stacked series. Relay and bridge users used to share a chart
// on two independent y-axes (0–160k left, 0–60k right), where every crossing
// and gap between the lines was an artefact of the two scales — easy to read
// as meaning something. Stacked mini-charts, each on its own honest axis and
// sharing the time axis (`syncId` links their cursors), show the same data
// without inviting that comparison. Both series stay neutral slate; crimson
// is reserved for the high-blocking episodes shaded behind them.
//
// No floating tooltip: at 78–96px tall a tooltip box covered the whole plot,
// and with the charts synced both lit up at once. The hovered value is read
// out in each chart's label row instead (`hover` is the shared row index), and
// the synced cursor line marks the date on both.
function TorSeries({ data, dataKey, label, ticks, highBlockingGroups, showXAxis, height, hover, onHover }) {
  const row = hover != null ? data[hover] : null
  return (
    <>
      <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em', margin: '2px 0 2px' }}>
        <span>{label}</span>
        {row && (
          <span className="tabular" style={{ marginLeft: 'auto' }}>
            <span style={{ color: WHITE }}>{row[dataKey].toLocaleString()}</span> · {row.date}
          </span>
        )}
      </div>
      <div style={{ width: '100%', height }}>
        <ResponsiveContainer width="100%" height="100%">
          <ComposedChart
            data={data}
            syncId="tor"
            margin={{ top: 4, right: 4, bottom: 0, left: 0 }}
            onMouseMove={(state) => onHover(state?.isTooltipActive ? state.activeTooltipIndex : null)}
            onMouseLeave={() => onHover(null)}
          >
            <CartesianGrid stroke={BORDER} vertical={false} />
            <XAxis
              dataKey="date"
              ticks={ticks}
              tickFormatter={(d) => d.slice(0, 7)}
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
              hide={!showXAxis}
            />
            <YAxis
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
              width={44}
              tickCount={3}
            />
            {/* Kept only for the (synced) cursor line; renders no box. */}
            <Tooltip content={() => null} cursor={{ stroke: MUTED, strokeDasharray: '2 2' }} />
            {highBlockingGroups.map((group) => (
              <ReferenceArea
                key={`area-${group.start}`}
                x1={group.start}
                x2={group.end}
                fill={CRIMSON}
                fillOpacity={0.15}
              />
            ))}
            {highBlockingGroups.map((group) => (
              // No label: the line plus the shaded ReferenceArea mark where the
              // blocking episode falls, and the tooltip gives the exact date.
              <ReferenceLine key={`line-${group.start}`} x={group.start} stroke={CRIMSON} strokeOpacity={0.7} />
            ))}
            <Area
              type="monotone"
              dataKey={dataKey}
              name={label}
              fill={US_EXPOSURE}
              fillOpacity={0.2}
              stroke={US_EXPOSURE}
              strokeWidth={1}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>
    </>
  )
}

export default function TorChart({ countryCode }) {
  const [rows, setRows] = useState(null)
  const [error, setError] = useState(false)
  const [hover, setHover] = useState(null)

  useEffect(() => {
    let cancelled = false
    setRows(null)
    setError(false)

    fetch(`/api/tor-metrics?country=${countryCode}`)
      .then((r) => {
        if (!r.ok) throw new Error('Failed to fetch tor metrics')
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

  const chartData = rows.map((row) => ({
    date: row.date,
    relay_users: row.relay_users ?? 0,
    bridge_users: row.bridge_users ?? 0,
  }))
  const ticks = monthlyTicks(rows)
  const highBlockingGroups = groupConsecutiveDates(
    rows.filter((r) => r.blocking_signal === 'HIGH_BLOCKING').map((r) => r.date)
  )
  const transportRow = latestWithTransports(rows)

  return (
    <section style={{ width: '100%' }}>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginBottom: 8 }}>
        TOR RELAY / BRIDGE USERS
      </div>
      <TorSeries
        data={chartData}
        dataKey="relay_users"
        label="Relay users (direct)"
        ticks={ticks}
        highBlockingGroups={highBlockingGroups}
        height={78}
        hover={hover}
        onHover={setHover}
      />
      <TorSeries
        data={chartData}
        dataKey="bridge_users"
        label="Bridge users (circumvention)"
        ticks={ticks}
        highBlockingGroups={highBlockingGroups}
        showXAxis
        height={96}
        hover={hover}
        onHover={setHover}
      />

      {highBlockingGroups.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginTop: 4, fontFamily: MONO, fontSize: TYPE.tick, color: MUTED }}>
          <div style={{ width: 8, height: 8, background: CRIMSON, opacity: 0.6, flexShrink: 0 }} />
          {/* HIGH_BLOCKING = users below the lower bound of Tor Metrics'
              published anomaly-detection range (fetchers/tor_metrics.rs). */}
          Users below Tor's expected range (possible blocking)
        </div>
      )}

      {transportRow && (
        <div style={{ display: 'flex', justifyContent: 'center', gap: 16, marginTop: 6, fontFamily: MONO, fontSize: TYPE.label }}>
          {TRANSPORTS.map((t) => {
            const value = midpoint(transportRow[t.low], transportRow[t.high])
            if (value == null) return null
            const range =
              transportRow[t.low] != null && transportRow[t.high] != null
                ? `${transportRow[t.low].toLocaleString()}–${transportRow[t.high].toLocaleString()}`
                : 'single bound'
            return (
              <div
                key={t.label}
                style={{ display: 'flex', gap: 4, cursor: 'help' }}
                title={`${t.label} bridge users on ${transportRow.date} — Tor estimate range ${range}`}
              >
                <span style={{ color: MUTED }}>{t.label}</span>
                <span style={{ color: WHITE }}>{value.toLocaleString()}</span>
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}

