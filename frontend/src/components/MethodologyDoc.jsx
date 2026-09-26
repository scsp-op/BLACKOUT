import { useEffect, useMemo, useRef, useState } from 'react'
import { getMethodologyDoc } from '../lib/api'
import { splitNumber } from '../lib/toc'
import { BORDER, BORDER_STRONG, CYAN, MONO, MUTED, RAISED, SANS, SIDEBAR, TYPE, WHITE } from '../theme'

// Distance from the top of the scroll container at which a heading counts as
// "the section you are reading". Matching the prose's `scroll-margin-top` keeps
// the sidebar's highlight in step with where an anchor click actually lands.
const HEADING_OFFSET = 24

// How long the scroller must be quiet before a click-driven scroll counts as
// finished and the scroll-spy takes over again.
const SCROLL_SETTLE_MS = 120

// Width of the contents rail's section-number column.
const NUMBER_GUTTER = 30

// Long-form prose, so this is the one place in the app that is not sized for an
// instrument panel: 17px Inter at a 1.75 line height, in a measure capped near
// 70 characters. Everything else is derived from theme.js so the documents
// belong to the same surface as the chrome around them.
export const PROSE_CSS = `
.md-prose {
  font-family: ${SANS};
  font-size: 18px;
  line-height: 1.72;
  color: #c3cad6;
  max-width: 66ch;
}
.md-prose > *:first-child { margin-top: 0 }
.md-prose p { margin: 0 0 1.15em }
.md-prose strong { color: ${WHITE}; font-weight: 600 }
.md-prose em { color: #d3d9e3 }

.md-prose h2, .md-prose h3 {
  font-weight: 600;
  color: ${WHITE};
  letter-spacing: 0.01em;
  scroll-margin-top: ${HEADING_OFFSET}px;
}
/* No rule under a heading and none between sections. The Markdown's own
   --- separators used to render as a second line right above each numbered
   heading, so a section break drew two rules a few pixels apart. Space does
   the separating now; the hr is kept as that space rather than removed, so a
   deliberate break in the source still reads as one. */
.md-prose h2 {
  font-size: 26px;
  margin: 2.6em 0 0.7em;
}
.md-prose h3 { font-size: 19px; margin: 2em 0 0.5em; color: ${WHITE} }
.md-prose h4 { font-size: 15.5px; margin: 1.6em 0 0.4em; color: ${WHITE}; font-weight: 600 }

/* The underline carries the affordance; the colour does not need to. */
.md-prose a { color: ${WHITE}; text-decoration: none; border-bottom: 1px solid ${BORDER_STRONG} }
.md-prose a:hover { border-bottom-color: ${MUTED} }

.md-prose ul, .md-prose ol { margin: 0 0 1.15em; padding-left: 1.4em }
.md-prose li { margin: 0.3em 0 }
.md-prose li::marker { color: ${MUTED} }

.md-prose blockquote {
  margin: 1.4em 0;
  padding: 0.2em 0 0.2em 1.1em;
  border-left: 2px solid ${BORDER_STRONG};
  color: ${MUTED};
}
.md-prose blockquote p:last-child { margin-bottom: 0 }

.md-prose code {
  font-family: ${MONO};
  font-size: 0.85em;
  padding: 0.12em 0.38em;
  background: ${SIDEBAR};
  border: 1px solid ${BORDER};
  color: ${WHITE};
}
.md-prose pre {
  margin: 0 0 1.3em;
  padding: 14px 16px;
  background: ${SIDEBAR};
  border: 1px solid ${BORDER};
  overflow-x: auto;
}
.md-prose pre code { padding: 0; background: none; border: 0; color: #c3cad6; font-size: 13.5px }

/* Tables carry the source register and the index inputs — the densest, most
   citable content in the value document — so they get the instrument
   treatment: mono, tight, ruled, and horizontally scrollable rather than
   squeezed. */
.md-prose table {
  width: 100%;
  margin: 0 0 1.5em;
  border-collapse: collapse;
  font-family: ${MONO};
  font-size: 13.5px;
  line-height: 1.6;
  display: block;
  overflow-x: auto;
}
.md-prose th, .md-prose td {
  border: 1px solid ${BORDER};
  padding: 8px 11px;
  text-align: left;
  vertical-align: top;
}
.md-prose th {
  background: ${SIDEBAR};
  color: ${WHITE};
  font-weight: 600;
  letter-spacing: 0.04em;
  white-space: nowrap;
}
.md-prose tbody tr:nth-child(even) { background: rgba(13, 28, 47, 0.45) }

.md-prose hr { margin: 2.2em 0; border: 0; height: 0 }
.md-prose img { max-width: 100%; height: auto }

/* Footnote definitions land at the end of the document; set them apart from
   the body so they do not read as a final section. */
.md-prose .footnotes { margin-top: 3em; padding-top: 1.2em; border-top: 1px solid ${BORDER}; font-size: 15.5px; color: ${MUTED} }

.md-toc-link:hover, .md-toc-toggle:hover { background: ${RAISED} }
.md-toc-toggle:hover { color: ${WHITE} }

/* Below this the rail is hidden and the prose takes the full width; a 300px
   sidebar leaves nothing to read beside it. !important is kept deliberately:
   the rail is styled inline, and the first inline display property added to it would
   otherwise silently outrank this rule and strand the layout. (This block is a
   template literal: no backticks.) */
@media (max-width: 1000px) {
  .md-toc { display: none !important }
}
`

// Group the flat table of contents into sections and their subsections, so a
// section can be collapsed as a unit. Anything appearing before the first `##`
// (there is normally nothing) becomes a section of its own with no children,
// rather than being dropped.
function groupToc(toc) {
  const groups = []
  for (const entry of toc) {
    if (entry.level === 2 || groups.length === 0) groups.push({ entry, children: [] })
    else groups[groups.length - 1].children.push(entry)
  }
  return groups
}

// The contents rail, in the shape of the Rust book's: a full-height column
// flush against the left edge with a single rule down its inner side, not a
// floating box. No header strip — the list is the whole content, so labelling
// it "CONTENTS" only takes a line away from it.
//
// Subsections are folded away. At full depth this is 29 rows for the policy
// document and the numbered `4.1…4.10` runs dominate it; collapsed it is eight
// section titles you can take in at once. The open section follows the reader
// automatically — scrolling into 3.2 opens section 3 — and the +/- control
// opens one without navigating to it, which is the same control, and the same
// glyphs, that collapse GlobalRanking and the outage feed.
function TocRail({ toc, activeId, onJump }) {
  const groups = useMemo(() => groupToc(toc), [toc])

  // The section that owns whatever is currently active: the active entry
  // itself when it is a section, otherwise the section above it.
  const activeSection = useMemo(() => {
    let section = null
    for (const entry of toc) {
      if (entry.level === 2) section = entry.id
      if (entry.id === activeId) return entry.level === 2 ? entry.id : section
    }
    return null
  }, [toc, activeId])

  const [openId, setOpenId] = useState(activeSection)
  // One section open at a time. Reading is linear, so the section being read
  // is nearly always the one worth expanding, and an accordion keeps the rail
  // short enough to stay scannable.
  useEffect(() => {
    if (activeSection) setOpenId(activeSection)
  }, [activeSection])

  return (
    <nav
      className="md-toc"
      id="methodology-toc"
      style={{
        // Not sticky, and not a box. `position: sticky` releases once its
        // containing block runs out of room, which for a viewport-tall rail
        // inside the article's scroll container happened a full screen before
        // the end of a long document. A sibling column cannot come unstuck.
        width: 300,
        flexShrink: 0,
        height: '100%',
        overflowY: 'auto',
        padding: '20px 0 48px',
        background: SIDEBAR,
        borderRight: `1px solid ${BORDER}`,
      }}
    >
      {groups.map(({ entry, children }) => {
        const open = openId === entry.id
        const rows = open ? [entry, ...children] : [entry]
        return rows.map((row, i) => {
          const isSection = i === 0
          const active = row.id === activeId
          const { number, title } = splitNumber(row.text)
          return (
            <div key={row.id} style={{ display: 'flex', alignItems: 'stretch' }}>
              <a
                href={`#${row.id}`}
                className="md-toc-link"
                data-toc-id={row.id}
                aria-label={row.text}
                onClick={(e) => {
                  e.preventDefault()
                  onJump(row.id)
                }}
                style={{
                  flex: 1,
                  minWidth: 0,
                  display: 'flex',
                  gap: 10,
                  padding: isSection ? '7px 4px 7px 18px' : '5px 4px 5px 30px',
                  borderLeft: `2px solid ${active ? CYAN : 'transparent'}`,
                  textDecoration: 'none',
                  lineHeight: 1.45,
                }}
              >
                <span
                  className="tabular"
                  style={{
                    fontFamily: MONO,
                    fontSize: isSection ? 11.5 : 11,
                    // A fixed gutter whether or not this entry has a number, so
                    // every title in the list starts on the same vertical line.
                    width: NUMBER_GUTTER,
                    flexShrink: 0,
                    paddingTop: isSection ? 1.5 : 1,
                    color: active ? CYAN : MUTED,
                  }}
                >
                  {number ?? ''}
                </span>
                <span
                  style={{
                    fontFamily: SANS,
                    fontSize: isSection ? 13.5 : 13,
                    fontWeight: isSection ? 500 : 400,
                    color: active ? CYAN : isSection ? '#c3cad6' : MUTED,
                  }}
                >
                  {title}
                </span>
              </a>

              {/* Only on a section that has subsections, and deliberately a
                  separate control from the link: pressing it looks at what is
                  in a section without leaving where you are. */}
              {isSection && children.length > 0 && (
                <button
                  type="button"
                  className="md-toc-toggle"
                  aria-expanded={open}
                  aria-label={`${open ? 'Collapse' : 'Expand'} ${entry.text}`}
                  onClick={() => setOpenId(open ? null : entry.id)}
                  style={{
                    flexShrink: 0,
                    width: 26,
                    background: 'transparent',
                    border: 'none',
                    padding: 0,
                    cursor: 'pointer',
                    fontFamily: MONO,
                    fontSize: 11,
                    color: open ? WHITE : MUTED,
                  }}
                >
                  {open ? '\u2212' : '+'}
                </button>
              )}
            </div>
          )
        })
      })}
    </nav>
  )
}

export default function MethodologyDoc({ slug }) {
  const [doc, setDoc] = useState(null)
  const [error, setError] = useState('')
  const [activeId, setActiveId] = useState('')
  // Set to the id a click is scrolling towards, and held until that scroll
  // finishes. While it is set the scroll-spy stands down — see `jump`.
  const pendingJumpRef = useRef(null)
  const settleTimerRef = useRef(0)

  // The jump is finished once the scroller has been quiet for a moment. There
  // is no portable "smooth scroll ended" event (`scrollend` is still missing
  // from enough browsers to need a fallback anyway), and a fixed delay would
  // either cut a long scroll short or leave the spy off after a short one.
  const settleAfter = (ms) => {
    clearTimeout(settleTimerRef.current)
    settleTimerRef.current = setTimeout(() => {
      pendingJumpRef.current = null
    }, ms)
  }

  useEffect(() => {
    let cancelled = false
    setDoc(null)
    setError('')
    setActiveId('')
    getMethodologyDoc(slug)
      .then((d) => !cancelled && setDoc(d))
      .catch(() => !cancelled && setError(`No methodology document called "${slug}".`))
    return () => {
      cancelled = true
    }
  }, [slug])

  const ids = useMemo(() => (doc?.toc ?? []).map((t) => t.id), [doc])
  // Top-level sections, counted off the table of contents — the same
  // number the index panel shows, without a second field to keep in step.
  const sectionCount = useMemo(() => (doc?.toc ?? []).filter((t) => t.level === 2).length, [doc])

  // Which section is being read. An IntersectionObserver fires only on
  // crossings, so it cannot answer "which heading is above me right now" after
  // a jump or on first paint — hence reading positions directly on scroll,
  // rAF-throttled. The list is a few dozen headings, so the measure is cheap.
  useEffect(() => {
    if (!ids.length) return
    const scroller = document.getElementById('methodology-scroll')
    if (!scroller) return

    let frame = 0
    const measure = () => {
      frame = 0
      // A click already decided which entry is active. Recomputing from scroll
      // position mid-animation is what made the highlight crawl backwards
      // through every section on the way to the target: until the target
      // crosses the threshold, the heading above the fold genuinely IS the
      // previous one, so the spy kept overruling the click and only agreed
      // with it once the animation stopped.
      if (pendingJumpRef.current) return
      // At the foot of the document the final section can be filling the
      // screen while its heading still sits below the threshold, so the rail
      // would mark the section before it for as long as the reader is in the
      // last one. Bottoming out means the last section, whatever the headings
      // measure.
      if (scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2) {
        setActiveId(ids[ids.length - 1])
        return
      }

      const top = scroller.getBoundingClientRect().top + HEADING_OFFSET + 2
      let current = ids[0]
      for (const id of ids) {
        const el = document.getElementById(id)
        if (el && el.getBoundingClientRect().top <= top) current = id
        else break
      }
      setActiveId(current)
    }
    const onScroll = () => {
      // Every scroll event during the animation pushes the settle deadline
      // out, so the spy resumes a beat after the scrolling actually stops
      // rather than after a guessed duration.
      if (pendingJumpRef.current) {
        settleAfter(SCROLL_SETTLE_MS)
        return
      }
      if (!frame) frame = requestAnimationFrame(measure)
    }

    measure()
    scroller.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      scroller.removeEventListener('scroll', onScroll)
      if (frame) cancelAnimationFrame(frame)
    }
  }, [ids])

  // A pending jump must not outlive the document it was made in, or the spy
  // would come back to a new document still standing down.
  useEffect(() => {
    return () => {
      clearTimeout(settleTimerRef.current)
      pendingJumpRef.current = null
    }
  }, [slug])

  // A 29-entry contents list is taller than the rail, so the entry marking
  // where the reader is can sit outside it. `block: 'nearest'` scrolls only
  // when it actually has to, which keeps the rail still while the reader is
  // working through a run of sections that are already on screen.
  useEffect(() => {
    if (!activeId) return
    const rail = document.getElementById('methodology-toc')
    const link = rail?.querySelector(`[data-toc-id="${CSS.escape(activeId)}"]`)
    link?.scrollIntoView({ block: 'nearest' })
  }, [activeId])

  const jump = (id) => {
    const target = document.getElementById(id)
    if (!target) return
    // Claim the highlight before the scroll starts, so the colour moves on the
    // click rather than at the end of the animation.
    pendingJumpRef.current = id
    setActiveId(id)
    target.scrollIntoView({
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
      block: 'start',
    })
    // Covers the case where nothing scrolls at all — jumping to the section
    // already at the top fires no scroll event, and without this the spy would
    // stay switched off for good.
    settleAfter(SCROLL_SETTLE_MS * 4)
  }

  if (error) {
    return (
      <div style={{ flex: 1, padding: '64px 24px' }}>
        <p style={{ fontFamily: MONO, fontSize: 12, color: MUTED }}>{error}</p>
      </div>
    )
  }

  if (!doc) {
    return (
      <div style={{ flex: 1, padding: '64px 24px' }}>
        <p style={{ fontFamily: MONO, fontSize: 11, letterSpacing: '0.1em', color: MUTED }}>LOADING…</p>
      </div>
    )
  }

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', width: '100%' }}>
      <style>{PROSE_CSS}</style>

      <TocRail toc={doc.toc} activeId={activeId} onJump={jump} />

      {/* The article is the only thing that scrolls, and it centres in
          whatever the rail leaves rather than the pair being centred together
          — the rail belongs to the window edge, as a sidebar does. */}
      <div id="methodology-scroll" style={{ flex: 1, minWidth: 0, overflowY: 'auto' }}>
        <article style={{ maxWidth: 820, margin: '0 auto', padding: '32px 40px 120px' }}>
        {/* The tool's panel header, applied to a document: a MONO caps rule
            line with the name left and the counts right, exactly as
            GlobalRanking and OutageFeed label themselves. There is no display
            title and no restated subtitle — the index row the reader just
            clicked said both, and the document's own first paragraph is a
            better opening than a second copy of its blurb.

            No reading time. Nothing else in the tool estimates how long a
            reader will take with what it shows them. */}
        <header
          style={{
            display: 'flex',
            alignItems: 'baseline',
            gap: 12,
            paddingBottom: 8,
            borderBottom: `1px solid ${BORDER}`,
            marginBottom: 36,
          }}
        >
          <span style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.1em', color: WHITE }}>
            {doc.title.toUpperCase()}
          </span>
          <span
            className="tabular"
            style={{
              marginLeft: 'auto',
              fontFamily: MONO,
              fontSize: TYPE.label,
              letterSpacing: '0.08em',
              color: MUTED,
              flexShrink: 0,
            }}
          >
            {sectionCount} SECTIONS · {doc.word_count.toLocaleString()} WORDS
          </span>
        </header>

        {/* The markup is produced by comrak in backend/src/api/methodology.rs
            from Markdown that is compiled into the server binary, with raw
            HTML escaped rather than passed through. There is no user input
            anywhere on this path, so there is nothing here for a sanitiser to
            do. */}
        <div className="md-prose" dangerouslySetInnerHTML={{ __html: doc.html }} />
        </article>
      </div>
    </div>
  )
}
