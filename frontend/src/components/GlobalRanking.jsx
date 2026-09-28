import { useEffect, useMemo, useState } from 'react'
import { getRankings } from '../lib/api'
import { AMBER, BORDER, CRIMSON, DIM, HIGHLIGHT, LOCAL, MONO, MUTED, TYPE, WHITE } from '../theme'

const SOURCES = [
  { key: 'V_DEM', short: 'V-DEM', label: 'V-Dem Freedom of Expression' },
  { key: 'RSF', short: 'RSF', label: 'RSF Press Freedom' },
]

// All scores are 0–100, higher = more free. In a "most censored" list the top
// rows are the lowest scores, so short crimson bars read as most repressive.
function scoreColor(score) {
  if (score >= 60) return LOCAL
  if (score >= 35) return AMBER
  return CRIMSON
}

function SourceToggle({ active, short, onClick }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        background: 'transparent',
        border: `1px solid ${active ? HIGHLIGHT : BORDER}`,
        color: active ? HIGHLIGHT : MUTED,
        fontFamily: MONO,
        fontSize: TYPE.label,
        letterSpacing: '0.05em',
        padding: '2px 5px',
        whiteSpace: 'nowrap',
        cursor: 'pointer',
      }}
    >
      {short}
    </button>
  )
}

// Content only — App renders this inside the header dock's RANKING panel
// (DockPanel), which supplies the frame, title and close control.
export default function GlobalRanking() {
  const [source, setSource] = useState('V_DEM')
  const [rows, setRows] = useState([])
  const [error, setError] = useState(false)

  useEffect(() => {
    let cancelled = false
    setError(false)

    getRankings({ source, order: 'asc', limit: 200 })
      .then((data) => {
        if (!cancelled) setRows(data)
      })
      .catch(() => {
        if (!cancelled) {
          setRows([])
          setError(true)
        }
      })

    return () => {
      cancelled = true
    }
  }, [source])

  const meta = SOURCES.find((s) => s.key === source)

  // The list runs most censored first, but each row is numbered with the
  // country's rank counted from the freest (#1 = most free, ties share a
  // rank) — the way RSF numbers its own table, and the same number the
  // country sidebar shows (FreedomRank). So the list opens at the bottom of
  // the table and counts down.
  const rankByScore = useMemo(() => {
    const ranks = new Map()
    rows.forEach((r) => {
      if (!ranks.has(r.score_overall)) {
        ranks.set(r.score_overall, 1 + rows.filter((x) => x.score_overall > r.score_overall).length)
      }
    })
    return ranks
  }, [rows])
  const year = rows[0]?.year

  return (
    <>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 4,
          padding: '6px 10px',
          borderBottom: `1px solid ${BORDER}`,
        }}
      >
        <span style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.06em', color: MUTED, marginRight: 'auto' }}>
          SOURCE
        </span>
        {SOURCES.map((s) => (
          <SourceToggle
            key={s.key}
            short={s.short}
            active={source === s.key}
            onClick={() => setSource(s.key)}
          />
        ))}
      </div>

      <div style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
        {error && rows.length === 0 && (
          <p style={{ padding: '8px 10px', fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>Rankings unavailable.</p>
        )}
        {rows.map((r) => {
          const color = scoreColor(r.score_overall)
          return (
            <div
              key={r.country_code}
              title={`${r.country_name} — ${Math.round(r.score_overall)}/100 free${r.classification ? ` · ${r.classification}` : ''}`}
              style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 10px' }}
            >
              {/* 22px: room for a three-digit rank. */}
              <span className="tabular" style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED, width: 22, flexShrink: 0, textAlign: 'right' }}>
                {rankByScore.get(r.score_overall)}
              </span>
              <span
                style={{ fontSize: TYPE.body, color: WHITE, width: 110, flexShrink: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
              >
                {r.country_name}
              </span>
              <div style={{ flex: 1, height: 6, background: DIM, position: 'relative' }}>
                <div style={{ position: 'absolute', left: 0, top: 0, bottom: 0, width: `${Math.max(2, Math.min(100, r.score_overall))}%`, background: color }} />
              </div>
              <span style={{ fontFamily: MONO, fontSize: TYPE.label, color, width: 24, flexShrink: 0, textAlign: 'right' }}>
                {Math.round(r.score_overall)}
              </span>
            </div>
          )
        })}
      </div>
      <div style={{ padding: '6px 10px', borderTop: `1px solid ${BORDER}`, fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, letterSpacing: '0.05em' }}>
        {rows.length} countries · {meta?.label}{year ? ` ${year}` : ''} · 0–100, higher = freer
      </div>
    </>
  )
}
