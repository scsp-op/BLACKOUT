import { useState } from 'react'
import { MONO, MUTED, TYPE, WHITE } from '../theme'

// Hover readouts for the sidebar charts. A floating tooltip box covered most
// of these small charts (and, on synced charts, every one of them at once), so
// the hovered value is read out in a text line beside the chart instead, with
// a dotted cursor marking the position. Every chart uses this one pattern.

// The hovered row index, plus the chart props that track it. Spread `handlers`
// onto a Recharts chart; synced charts can share one hover by all taking the
// same handlers.
export function useChartHover() {
  const [index, setIndex] = useState(null)
  const handlers = {
    onMouseMove: (state) => setIndex(state?.isTooltipActive ? state.activeTooltipIndex : null),
    onMouseLeave: () => setIndex(null),
  }
  return [index, handlers]
}

// Props for a Recharts <Tooltip> that draws only the cursor line, never a box.
export const CURSOR_ONLY = {
  content: () => null,
  cursor: { stroke: MUTED, strokeDasharray: '2 2' },
}

// A `value · detail` readout, with the value emphasised.
export function Readout({ value, detail }) {
  return (
    <>
      <span style={{ color: WHITE }}>{value}</span>
      {detail && <> · {detail}</>}
    </>
  )
}

// A chart's title line, with the hovered readout right-aligned on it.
export function ChartTitle({ children, readout }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'baseline',
        gap: 8,
        fontFamily: MONO,
        fontSize: TYPE.label,
        letterSpacing: '0.06em',
        color: MUTED,
        marginBottom: 8,
      }}
    >
      <span>{children}</span>
      {readout && (
        <span className="tabular" style={{ marginLeft: 'auto', fontSize: TYPE.tick, letterSpacing: '0.03em', whiteSpace: 'nowrap' }}>
          {readout}
        </span>
      )}
    </div>
  )
}
