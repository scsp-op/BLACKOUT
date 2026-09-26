import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { BORDER, MONO, MUTED, TYPE, US_EXPOSURE } from '../theme'
import { CURSOR_ONLY, Readout, useChartHover } from './chartHover'

// This renders inline underneath a single technology row in the sidebar, so
// it's deliberately a sparkline: no grid, no legend, first/last tick only.
// The row above it already names the country and technology, and the sidebar
// is only 360px wide.
const HEIGHT = 56

// The share of measurements that came back anomalous. Raw anomaly_count is
// not comparable across days — OONI runs a different number of measurements
// per day per country, so 5 anomalies out of 5 and 5 out of 500 are very
// different signals.
function anomalyRate(row) {
  if (!row.measurement_count) return 0
  return row.anomaly_count / row.measurement_count
}

// Purely presentational: `rows` come from CountrySidebar, which already
// fetches every promoted (country, technology) timeline to decide which rows
// are worth showing at all. Fetching here as well re-requested a URL the parent
// already had — and because the visible technology list changes as that data
// lands, each re-render churned this component through several mounts, firing
// the same request again each time.
export default function TimelineChart({ rows }) {
  const [hover, hoverHandlers] = useChartHover()
  if (!rows || rows.length === 0) return null

  const chartData = rows.map((row) => ({
    date: row.measurement_date,
    rate: anomalyRate(row) * 100,
    measurements: row.measurement_count,
    confirmed: row.confirmed_count,
  }))

  const peak = Math.max(...chartData.map((d) => d.rate))
  const confirmedDays = rows.filter((row) => row.confirmed_count > 0).length
  const first = chartData[0].date
  const last = chartData[chartData.length - 1].date
  const hovered = hover != null ? chartData[hover] : null

  return (
    <div style={{ width: '100%' }}>
      <div style={{ width: '100%', height: HEIGHT }}>
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={chartData} margin={{ top: 2, right: 4, bottom: 0, left: 0 }} {...hoverHandlers}>
            <XAxis
              dataKey="date"
              ticks={[first, last]}
              tickFormatter={(d) => d.slice(0, 7)}
              tick={{ fill: MUTED, fontSize: TYPE.tick, fontFamily: MONO }}
              axisLine={{ stroke: BORDER }}
              tickLine={false}
              interval="preserveStartEnd"
            />
            {/* Pinned to 0–100 so the eye can compare this sparkline against
                the one on the row above it. An auto domain would rescale each
                technology to its own peak and imply equal severity. */}
            <YAxis domain={[0, 100]} hide />
            <Tooltip {...CURSOR_ONLY} />
            {/* Neutral slate, not crimson: this draws under every technology
                row whatever its status, and a red history under an ACCESSIBLE
                service read as "danger". The status chip in the row above is
                what carries the verdict colour. */}
            <Area
              type="monotone"
              dataKey="rate"
              name="rate"
              stroke={US_EXPOSURE}
              strokeWidth={1}
              fill={US_EXPOSURE}
              fillOpacity={0.18}
              dot={false}
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>

      {/* The caption doubles as the hover readout (see chartHover.jsx): the
          hovered day replaces the summary while the pointer is on the chart. */}
      <div className="tabular" style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em' }}>
        {hovered ? (
          <Readout
            value={`${hovered.rate.toFixed(1)}% anomalous`}
            detail={`${hovered.date} · ${hovered.measurements.toLocaleString()} test${hovered.measurements === 1 ? '' : 's'}${hovered.confirmed > 0 ? ` · ${hovered.confirmed} confirmed` : ''}`}
          />
        ) : (
          <>
            {rows.length} days · peak {peak.toFixed(0)}% anomalous
            {confirmedDays > 0 && ` · ${confirmedDays} confirmed-blocked`}
          </>
        )}
      </div>
    </div>
  )
}
