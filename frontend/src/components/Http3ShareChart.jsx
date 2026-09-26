import { useEffect, useState } from 'react'
import {
  ComposedChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from 'recharts'
import { BORDER, CYAN, DIM, MONO, MUTED, TYPE, US_EXPOSURE, WHITE } from '../theme'
import { CURSOR_ONLY, ChartTitle, Readout, useChartHover } from './chartHover'

// Same thinning approach as TorChart.jsx: one tick per month, capped so
// labels don't collide in a 360px sidebar.
const MAX_TICKS = 5

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
    if (ticks.length >= MAX_TICKS) ticks.pop()
    ticks.push(last)
  }
  return ticks
}

// Legend order matches the stack, bottom to top.
const PROTOCOLS = [
  { key: 'http3', label: 'HTTP/3', color: CYAN },
  { key: 'http2', label: 'HTTP/2', color: US_EXPOSURE },
  { key: 'http1', label: 'HTTP/1.x', color: DIM },
]

// Two decimals under 1% (HTTP/3 where it's blocked sits at a few hundredths),
// one otherwise.
const formatShare = (pct) => `${pct.toFixed(pct < 1 ? 2 : 1)}%`

// HTTP/1.x vs HTTP/2 vs HTTP/3 (QUIC) daily traffic share for a country, from
// Cloudflare Radar. A leading indicator distinct from an outage: a government
// can block QUIC — and the circumvention tools that tunnel over it — while
// the network itself keeps running, well before (or without) any
// connectivity blackout. Deliberately not labeled a "filtering" or "blocking"
// chart: this is a raw share measurement, not a classification.
export default function Http3ShareChart({ countryCode }) {
  const [rows, setRows] = useState(null)
  const [error, setError] = useState(false)
  const [hover, hoverHandlers] = useChartHover()

  useEffect(() => {
    let cancelled = false
    setRows(null)
    setError(false)

    fetch(`/api/http-protocol-share?country=${countryCode}`)
      .then((r) => {
        if (!r.ok) throw new Error('Failed to fetch HTTP protocol share')
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
    http1: row.http1_pct ?? 0,
    http2: row.http2_pct ?? 0,
    http3: row.http3_pct ?? 0,
  }))
  const ticks = monthlyTicks(rows)
  const latest = rows[rows.length - 1]
  const hovered = hover != null ? chartData[hover] : null

  return (
    <section>
      <ChartTitle readout={hovered && <Readout value={hovered.date} />}>
        HTTP/3 (QUIC) TRAFFIC SHARE
      </ChartTitle>
      <div style={{ width: '100%', height: 140 }}>
        <ResponsiveContainer width="100%" height="100%">
          <ComposedChart data={chartData} margin={{ top: 6, right: 4, bottom: 0, left: 0 }} {...hoverHandlers}>
            <CartesianGrid stroke={BORDER} vertical={false} />
            <XAxis
              dataKey="date"
              ticks={ticks}
              tickFormatter={(d) => d.slice(0, 7)}
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
            />
            <YAxis
              // The three protocol shares are rounded upstream and can sum to a
              // hair over 100, which made Recharts stretch the domain to
              // 100.0001 and print that as the top tick. Pin both.
              domain={[0, 100]}
              allowDataOverflow
              ticks={[0, 25, 50, 75, 100]}
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
              width={38}
              unit="%"
            />
            <Tooltip {...CURSOR_ONLY} />
            {/* Stacked bottom-to-top: HTTP/3 (the signal of interest, in the
                app's HUD/live-signal accent) at the base, then HTTP/2, then
                HTTP/1.x on top. HTTP/3 must be the bottom band: stacked on top
                it rode along the 100% line whatever its share, so Iran's 0.06%
                read as "HTTP/3 = 100%". At the base, its top edge reads
                straight off the y-axis — and a collapse in it (QUIC blocking)
                is a band dropping to the floor. */}
            <Area
              type="monotone"
              dataKey="http3"
              name="HTTP/3 (QUIC)"
              stackId="share"
              fill={CYAN}
              fillOpacity={0.5}
              stroke={CYAN}
              strokeWidth={1.5}
            />
            <Area
              type="monotone"
              dataKey="http2"
              name="HTTP/2"
              stackId="share"
              fill={US_EXPOSURE}
              fillOpacity={0.35}
              stroke={US_EXPOSURE}
              strokeWidth={1}
            />
            <Area
              type="monotone"
              dataKey="http1"
              name="HTTP/1.x"
              stackId="share"
              fill={DIM}
              fillOpacity={0.5}
              stroke={DIM}
              strokeWidth={1}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>

      {/* The legend doubles as the hover readout: each protocol's share for
          the hovered day sits beside its swatch, so the three read against
          each other rather than HTTP/3 alone. (The date is on the title line.) */}
      <div style={{ display: 'flex', flexWrap: 'wrap', columnGap: 14, rowGap: 2, marginTop: 4, fontFamily: MONO, fontSize: TYPE.label }}>
        {PROTOCOLS.map((p) => (
          <div key={p.key} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
            <div style={{ width: 8, height: 8, background: p.color, flexShrink: 0 }} />
            <span style={{ color: MUTED }}>{p.label}</span>
            {hovered && (
              <span className="tabular" style={{ color: WHITE }}>
                {formatShare(hovered[p.key])}
              </span>
            )}
          </div>
        ))}
      </div>

      <div style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em', marginTop: 4 }}>
        latest: {(latest.http3_pct ?? 0).toFixed(2)}% HTTP/3 on {latest.date} · via Cloudflare Radar
      </div>
    </section>
  )
}
