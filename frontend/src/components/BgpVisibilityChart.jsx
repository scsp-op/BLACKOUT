import { useEffect, useState } from 'react'
import {
  ComposedChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  ReferenceLine,
  Tooltip,
  ResponsiveContainer,
} from 'recharts'
import { BORDER, CRIMSON, CYAN, MONO, MUTED, TYPE } from '../theme'
import { CURSOR_ONLY, ChartTitle, Readout, useChartHover } from './chartHover'

const MAX_TICKS = 6

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

// The ratio is computed here at render time, never stored — see
// bgp_prefix_visibility's table comment in backend/src/db/schema.rs for why.
// Based on ASN counts specifically, not prefix counts: RIPEstat's prefix
// *registration* counts (registered_v4/v6_prefixes) come back unavailable
// far more often in practice than the ASN registration count does, so a
// prefix-based ratio would be missing for most countries most of the time.
function asnVisibilityPct(row) {
  if (row.registered_asns == null || row.registered_asns === 0 || row.routed_asns == null) {
    return null
  }
  return (row.routed_asns / row.registered_asns) * 100
}

// Daily BGP ASN/prefix visibility for a country, from RIPEstat: how much of
// the country's *registered* address space is *currently routed* (visible in
// the global routing table) versus merely allocated on paper. A government
// withdrawing route announcements — a connectivity blackout at the routing
// layer, distinct from application-level blocking — shows up here as the
// visibility ratio collapsing.
export default function BgpVisibilityChart({ countryCode }) {
  const [rows, setRows] = useState(null)
  const [error, setError] = useState(false)
  const [hover, hoverHandlers] = useChartHover()

  useEffect(() => {
    let cancelled = false
    setRows(null)
    setError(false)

    fetch(`/api/bgp-visibility?country=${countryCode}`)
      .then((r) => {
        if (!r.ok) throw new Error('Failed to fetch BGP visibility')
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

  const chartData = rows
    .map((row) => ({ date: row.date, visiblePct: asnVisibilityPct(row) }))
    .filter((row) => row.visiblePct != null)
  if (chartData.length === 0) return null

  const ticks = monthlyTicks(rows)
  const hovered = hover != null ? chartData[hover] : null
  const latestWithPrefixes = [...rows].reverse().find((r) => r.routed_v4_prefixes != null)

  return (
    <section>
      <ChartTitle readout={hovered && <Readout value={`${hovered.visiblePct.toFixed(1)}%`} detail={hovered.date} />}>
        BGP PREFIX VISIBILITY
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
              domain={[0, 110]}
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
              width={38}
              unit="%"
            />
            <ReferenceLine y={100} stroke={MUTED} strokeDasharray="3 3" />
            <Tooltip {...CURSOR_ONLY} />
            <Line
              type="monotone"
              dataKey="visiblePct"
              name="ASNs visible in global BGP"
              stroke={CYAN}
              strokeWidth={1.5}
              dot={false}
            />
          </ComposedChart>
        </ResponsiveContainer>
      </div>

      <div style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em', marginTop: 4 }}>
        {chartData[chartData.length - 1].visiblePct < 50 && (
          <span style={{ color: CRIMSON }}>
            &lt;50% of registered ASNs currently visible ·{' '}
          </span>
        )}
        latest: {chartData[chartData.length - 1].visiblePct.toFixed(1)}% ASN visibility on{' '}
        {chartData[chartData.length - 1].date}
        {latestWithPrefixes &&
          ` · ${Math.round(latestWithPrefixes.routed_v4_prefixes).toLocaleString()} IPv4 prefixes routed`}
        {' '}· via RIPEstat
      </div>
    </section>
  )
}
