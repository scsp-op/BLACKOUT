import { useEffect, useState } from 'react'
import { getRankings } from '../lib/api'
import { MONO, MUTED, TYPE, WHITE } from '../theme'

// The RANKING panel's sources, in its order, so the line reads like its
// SOURCE toggle.
const SOURCES = [
  { key: 'V_DEM', short: 'V-DEM', label: 'V-Dem Freedom of Expression' },
  { key: 'RSF', short: 'RSF', label: 'RSF Press Freedom' },
]

// Both whole-world rankings, fetched once and shared by every country the
// sidebar opens — they only change when the scores are re-fetched server-side.
// Same query as GlobalRanking, so the totals match its country counts.
let rankingsPromise = null
function loadRankings() {
  rankingsPromise ??= Promise.all(
    SOURCES.map((s) => getRankings({ source: s.key, order: 'asc', limit: 200 })),
  ).catch((e) => {
    // Let the next country retry rather than caching the failure.
    rankingsPromise = null
    throw e
  })
  return rankingsPromise
}

// One line under the country name in the sidebar: the country's rank on each
// index, so reading it doesn't mean opening the RANKING panel and scanning for
// it. Counted freest-first (#1 = most free), the way RSF publishes its own
// ranking and the way a bare rank is read; the panel numbers its rows with the
// same rank, even though its list runs from most censored. Ties share a rank.
// The source year stays on hover — the two indices aren't always the same
// vintage.
export default function FreedomRank({ countryCode }) {
  const [ranks, setRanks] = useState([])

  useEffect(() => {
    let cancelled = false
    setRanks([])
    loadRankings()
      .then((lists) => {
        if (cancelled) return
        const found = SOURCES.map((s, i) => {
          const rows = lists[i]
          const me = rows.find((r) => r.country_code === countryCode)
          if (!me) return null
          const rank = 1 + rows.filter((r) => r.score_overall > me.score_overall).length
          return { ...s, rank, total: rows.length, year: me.year }
        })
        setRanks(found.filter(Boolean))
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [countryCode])

  if (ranks.length === 0) return null

  return (
    <div style={{ marginTop: 6, fontFamily: MONO, fontSize: TYPE.tick, letterSpacing: '0.05em', color: MUTED }}>
      {ranks.map((r, i) => (
        <span key={r.key} title={`${r.label} ${r.year} — #${r.rank} of ${r.total}, 1 = freest`}>
          {i > 0 && ' · '}
          {r.short}{' '}
          <span className="tabular" style={{ color: WHITE }}>
            #{r.rank}/{r.total}
          </span>
        </span>
      ))}
    </div>
  )
}
