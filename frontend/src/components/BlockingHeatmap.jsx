import { Fragment, useEffect, useMemo, useState } from 'react'
import { AMBER, BORDER, CRIMSON, DIM, MONO, MUTED, SIDEBAR, WHITE } from '../theme'
import { BLOCKING_STATUS_COLOR } from '../lib/blockingRegistry'
import { getGeo } from '../lib/api'

// Category display, in the order the panel stacks them.
const CATEGORY_ORDER = ['AI_ACCESS', 'CIRCUMVENTION', 'MESSAGING']
const CATEGORY_LABEL = {
  AI_ACCESS: 'AI Access',
  CIRCUMVENTION: 'Circumvention',
  MESSAGING: 'Messaging',
}

// Friendlier names for the tool keys stored in technology_blocks.
const TECH_LABEL = {
  torsf: 'Snowflake',
  torproject: 'Tor Project',
  tor: 'Tor',
  i2p: 'I2P',
  psiphon: 'Psiphon',
  signal: 'Signal (web)',
  signal_messenger: 'Signal',
  whatsapp: 'WhatsApp',
  telegram: 'Telegram',
  facebook_messenger: 'Messenger',
  'openai.com': 'OpenAI',
  'claude.ai': 'Claude',
  deepseek: 'DeepSeek',
  huggingface: 'HuggingFace',
}

// Tools measured in fewer than this many countries are dropped — their
// worldwide counts aren't representative (e.g. HuggingFace, n=9).
const MIN_MEASURED = 20

function techLabel(key) {
  return TECH_LABEL[key] ?? key
}

// `rows` is every technology_blocks row (all ~196 measured countries), owned by
// App. `countries` (the 5 researched dossiers) is still accepted for prop
// compatibility but no longer drives the view — this is a worldwide summary now.
export default function BlockingHeatmap({ rows }) {
  const [collapsed, setCollapsed] = useState(false)
  const [expanded, setExpanded] = useState(null)
  const [names, setNames] = useState({})

  // Country names for the drill-down list. Self-contained fetch so this panel
  // needs nothing new from App; falls back to the code if it fails.
  useEffect(() => {
    let cancelled = false
    getGeo()
      .then((geo) => {
        if (!cancelled) setNames(Object.fromEntries(geo.map((g) => [g.country_code, g.country_name])))
      })
      .catch(() => {})
    return () => { cancelled = true }
  }, [])

  // Aggregate every (country, tool) row into one entry per tool: how many
  // countries confirmed/likely block it, how many measured it, and the list of
  // blockers for the drill-down.
  const byCategory = useMemo(() => {
    const agg = {}
    for (const r of rows ?? []) {
      const t = (agg[r.technology] ??= {
        technology: r.technology,
        category: r.category,
        confirmed: 0,
        likely: 0,
        measured: 0,
        blockers: [],
      })
      if ((r.measurement_count ?? 0) > 0) t.measured += 1
      if (r.status === 'CONFIRMED_BLOCKED') {
        t.confirmed += 1
        t.blockers.push({ code: r.country_code, status: r.status })
      } else if (r.status === 'LIKELY_BLOCKED') {
        t.likely += 1
        t.blockers.push({ code: r.country_code, status: r.status })
      }
    }

    const grouped = {}
    let max = 1
    for (const t of Object.values(agg)) {
      if (t.measured < MIN_MEASURED) continue
      t.blocked = t.confirmed + t.likely
      // confirmed before likely, then alphabetical — a stable drill-down order.
      t.blockers.sort((a, b) =>
        a.status === b.status ? a.code.localeCompare(b.code) : a.status === 'CONFIRMED_BLOCKED' ? -1 : 1,
      )
      max = Math.max(max, t.blocked)
      ;(grouped[t.category] ??= []).push(t)
    }
    for (const list of Object.values(grouped)) list.sort((a, b) => b.blocked - a.blocked)
    return { grouped, max }
  }, [rows])

  if (!rows) return null
  const activeCategories = CATEGORY_ORDER.filter((c) => byCategory.grouped[c]?.length)
  if (activeCategories.length === 0) return null

  return (
    <div
      style={{
        position: 'absolute',
        left: 24,
        top: 16,
        width: 260,
        maxHeight: 'calc(100vh - 120px)',
        display: 'flex',
        flexDirection: 'column',
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        padding: '10px 12px',
      }}
    >
      <button
        type="button"
        onClick={() => setCollapsed((c) => !c)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          background: 'transparent',
          border: 'none',
          padding: 0,
          marginBottom: collapsed ? 0 : 8,
          cursor: 'pointer',
          fontFamily: MONO,
          fontSize: 10,
          letterSpacing: '0.1em',
          color: MUTED,
        }}
      >
        <span style={{ color: WHITE, width: 10 }}>{collapsed ? '\u25B8' : '\u25BE'}</span>
        TOOL BLOCKING · WORLDWIDE
      </button>

      {!collapsed && (
        <div style={{ overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 10 }}>
          {activeCategories.map((category) => (
            <div key={category}>
              <div
                style={{
                  fontFamily: MONO,
                  fontSize: 9,
                  letterSpacing: '0.1em',
                  textTransform: 'uppercase',
                  color: MUTED,
                  marginBottom: 4,
                }}
              >
                {CATEGORY_LABEL[category] ?? category}
              </div>

              {byCategory.grouped[category].map((t) => {
                const isOpen = expanded === t.technology
                const confW = (t.confirmed / byCategory.max) * 100
                const likeW = (t.likely / byCategory.max) * 100
                return (
                  <Fragment key={t.technology}>
                    <button
                      type="button"
                      onClick={() => setExpanded(isOpen ? null : t.technology)}
                      title={`${t.blocked} of ${t.measured} measured countries block ${techLabel(t.technology)}`}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 8,
                        width: '100%',
                        background: 'transparent',
                        border: 'none',
                        padding: '2px 0',
                        cursor: 'pointer',
                        textAlign: 'left',
                      }}
                    >
                      <span style={{ fontSize: 10, color: isOpen ? WHITE : '#c8ccd4', width: 90, flexShrink: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {techLabel(t.technology)}
                      </span>
                      <div style={{ flex: 1, height: 7, background: DIM, position: 'relative', display: 'flex' }}>
                        <div style={{ width: `${confW}%`, background: CRIMSON }} />
                        <div style={{ width: `${likeW}%`, background: AMBER }} />
                      </div>
                      <span style={{ fontFamily: MONO, fontSize: 9, color: t.blocked > 0 ? CRIMSON : MUTED, width: 20, flexShrink: 0, textAlign: 'right' }}>
                        {t.blocked}
                      </span>
                    </button>

                    {isOpen && (
                      <div
                        style={{
                          maxHeight: 132,
                          overflowY: 'auto',
                          margin: '2px 0 6px 0',
                          padding: '4px 6px',
                          background: '#050505',
                          border: `1px solid ${BORDER}`,
                          display: 'flex',
                          flexWrap: 'wrap',
                          gap: '3px 8px',
                        }}
                      >
                        {t.blockers.length === 0 && (
                          <span style={{ fontFamily: MONO, fontSize: 9, color: MUTED }}>No confirmed or likely blocks.</span>
                        )}
                        {t.blockers.map((b) => (
                          <span key={b.code} style={{ display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 9.5, color: '#c8ccd4' }}>
                            <span style={{ width: 5, height: 5, borderRadius: '50%', background: BLOCKING_STATUS_COLOR[b.status] ?? BORDER, flexShrink: 0 }} />
                            {names[b.code] ?? b.code}
                          </span>
                        ))}
                      </div>
                    )}
                  </Fragment>
                )
              })}
            </div>
          ))}

          <div style={{ display: 'flex', gap: 12, fontFamily: MONO, fontSize: 8, color: MUTED, letterSpacing: '0.05em', paddingTop: 2, borderTop: `1px solid ${BORDER}` }}>
            <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 7, height: 7, background: CRIMSON }} /> confirmed
            </span>
            <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 7, height: 7, background: AMBER }} /> likely · # = countries
            </span>
          </div>
        </div>
      )}
    </div>
  )
}
