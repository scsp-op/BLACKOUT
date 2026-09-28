import { useLayoutEffect, useRef, useState } from 'react'
import { AMBER, BORDER, BORDER_STRONG, CRIMSON, CYAN, LOCAL, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'
import { SOURCES } from '../lib/sources'
import { REPO_URL } from '../lib/links'
import { linkProps } from '../lib/router'

// Honest link state: mirrors whether the primary country fetch is in flight,
// succeeded, or errored — not a fabricated socket. App derives `status` from
// the same load state that drives everything else.
const LINK = {
  loading: { color: AMBER, label: 'SYNCING' },
  ok: { color: LOCAL, label: 'LINK OK' },
  error: { color: CRIMSON, label: 'LINK ERR' },
}

// Age of the underlying data, from /health. `last_updated` is stored as a
// plain date, so whole days is the honest resolution — a seconds-level counter
// here would imply a precision the pipeline does not have.
function formatAge(dataAge) {
  if (!dataAge || dataAge.age_days == null) return '—'
  const days = dataAge.age_days
  if (days <= 0) return 'today'
  return days === 1 ? '1 day old' : `${days} days old`
}

// The GitHub mark, inlined rather than fetched. A remote icon would be the only
// external request the app makes, and it would fail behind exactly the kind of
// network filtering this tool measures. `currentColor` lets it inherit the
// hover colour from the anchor.
function GithubMark({ size = 11 }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
      style={{ display: 'block' }}
    >
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A7.995 7.995 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  )
}

// METHODOLOGY and PRIVACY — the pages the bar navigates to, as against the
// source credits — carry a faint resting underline so they read as links
// without a colour of their own. BORDER_STRONG, the tone of the source list's
// separators, keeps it quieter than the text.
const PAGE_LINK_UNDERLINE = {
  textDecoration: 'underline',
  textDecorationColor: BORDER_STRONG,
  textDecorationThickness: 1,
  textUnderlineOffset: 3,
}

// Fades the source list's edges while names are scrolled out past them.
const sourcesFade = ({ left, right }) =>
  `linear-gradient(to right, ${left ? 'transparent, #000 32px' : '#000'}, ${right ? '#000 calc(100% - 32px), transparent' : '#000'})`

// `narrow` (phone width) drops the sources list and the link indicator, which
// don't fit; data age, methodology and the byline stay.
export default function StatusBar({ status = 'ok', dataAge = null, narrow = false }) {
  const link = LINK[status] ?? LINK.ok
  // The backend already decided what counts as stale (HEALTH_MAX_AGE_DAYS);
  // don't duplicate the threshold here, just colour by its verdict.
  const stale = dataAge?.status === 'stale'

  // Phones: whether the byline has wrapped onto its own line. The divider
  // before it is only shown while it shares a line with PRIVACY, so it never
  // dangles at a line end. Hidden with `visibility` (it keeps its space), so
  // toggling it can't change the layout and flip the wrap back and forth.
  const footerRef = useRef(null)
  const privacyRef = useRef(null)
  const bylineRef = useRef(null)
  const [bylineWrapped, setBylineWrapped] = useState(false)
  useLayoutEffect(() => {
    if (!narrow) return undefined
    const check = () => {
      if (privacyRef.current && bylineRef.current) {
        setBylineWrapped(bylineRef.current.offsetTop > privacyRef.current.offsetTop + 2)
      }
    }
    check()
    const observer = new ResizeObserver(check)
    observer.observe(footerRef.current)
    return () => observer.disconnect()
  }, [narrow])

  // Desktop: below the ~1300px the full row needs, the source list gives way
  // first — it scrolls sideways in the room left over rather than pushing the
  // byline off-screen, fading at whichever edge has more names past it.
  const sourcesRef = useRef(null)
  const [sourcesMore, setSourcesMore] = useState({ left: false, right: false })
  useLayoutEffect(() => {
    const el = sourcesRef.current
    if (narrow || !el) return undefined
    const check = () => {
      const left = el.scrollLeft > 1
      const right = el.scrollLeft + el.clientWidth < el.scrollWidth - 1
      setSourcesMore((m) => (m.left === left && m.right === right ? m : { left, right }))
    }
    check()
    el.addEventListener('scroll', check, { passive: true })
    const observer = new ResizeObserver(check)
    observer.observe(el)
    return () => {
      el.removeEventListener('scroll', check)
      observer.disconnect()
    }
  }, [narrow])

  return (
    <footer
      ref={footerRef}
      style={{
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        // Phones: centred, and allowed to wrap onto a second centred line
        // when the items don't fit (they overflowed below ~390px wide,
        // clipping the byline). Desktop: one 24px row, as before.
        ...(narrow
          ? { minHeight: 24, flexWrap: 'wrap', justifyContent: 'center', columnGap: 6, rowGap: 2, padding: '4px 12px' }
          : { height: 24, gap: 10, padding: '0 16px' }),
        background: SIDEBAR,
        borderTop: `1px solid ${BORDER}`,
        fontFamily: MONO,
        fontSize: TYPE.status,
        letterSpacing: '0.05em',
        whiteSpace: 'nowrap',
      }}
    >
      {/* Inline rather than in App.css because it is the only rule this
          component needs — same pattern the outage feed uses for its pulse. */}
      <style>{`.repo-link:hover { color: ${WHITE} } .source-link:hover { color: ${CYAN} } .footer-sources::-webkit-scrollbar { display: none }`}</style>

      {!narrow && <span style={{ color: MUTED }}>SOURCES</span>}
      {!narrow && <span
        ref={sourcesRef}
        className="footer-sources"
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          minWidth: 0,
          overflowX: 'auto',
          scrollbarWidth: 'none',
          // Room for a keyboard focus ring inside the scroll clip, offset by
          // the negative margin so the names don't move.
          padding: 3,
          margin: '0 -3px',
          ...(sourcesMore.left || sourcesMore.right
            ? { maskImage: sourcesFade(sourcesMore), WebkitMaskImage: sourcesFade(sourcesMore) }
            : null),
        }}
      >
        {SOURCES.map((source, i) => (
          <span key={source.id} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            {i > 0 && <span style={{ color: BORDER_STRONG }}>·</span>}
            <a
              className="source-link"
              href={source.url}
              target="_blank"
              rel="noreferrer noopener"
              title={source.url}
              style={{ color: WHITE, textDecoration: 'none' }}
            >
              {source.label}
            </a>
          </span>
        ))}
      </span>}

      {!narrow && (
        <>
          {/* paddingLeft keeps the divider-width break (10 + 10) from the
              source list when it runs right up to this, as it does while
              scrolled; with room to spare the auto margin absorbs it. */}
          <span style={{ marginLeft: 'auto', paddingLeft: 10, display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ width: 6, height: 6, borderRadius: '50%', background: link.color }} />
            <span style={{ color: link.color }}>{link.label}</span>
          </span>
          <span style={{ width: 1, height: 12, background: BORDER, flexShrink: 0 }} />
        </>
      )}
      <span
        style={{ color: MUTED, cursor: dataAge?.newest_data ? 'help' : 'default' }}
        title={
          dataAge?.newest_data
            ? `Newest fetched data: ${dataAge.newest_data} · stale after ${dataAge.max_age_days} day(s)`
            : 'No fetched data yet'
        }
      >
        DATA AGE{' '}
        <span className="tabular" style={{ color: stale ? CRIMSON : WHITE }}>
          {formatAge(dataAge)}
        </span>
      </span>

      <span style={{ width: 1, height: 12, background: BORDER, flexShrink: 0 }} />

      {/* The tool's account of itself sits immediately before the byline: the
          two answer the same question a viewer has on arrival — who made this
          and on what basis — so they read as one credit rather than a menu
          item parked elsewhere. */}
      <a
        {...linkProps('/methodology')}
        className="source-link"
        style={{ color: WHITE, ...PAGE_LINK_UNDERLINE, flexShrink: 0 }}
      >
        METHODOLOGY
      </a>
      <span style={{ width: 1, height: 12, background: BORDER, flexShrink: 0 }} />
      <a
        {...linkProps('/privacy')}
        ref={privacyRef}
        className="source-link"
        style={{ color: WHITE, ...PAGE_LINK_UNDERLINE, flexShrink: 0 }}
      >
        PRIVACY
      </a>

      <span
        style={{
          width: 1,
          height: 12,
          background: BORDER,
          flexShrink: 0,
          visibility: narrow && bylineWrapped ? 'hidden' : 'visible',
        }}
      />

      {/* Byline and repo link. Sized to the bar's 24px rhythm — the 11px
          mark sits inside the 24px height, so nothing grows. One
          unit, so a phone-width wrap never splits the name from the icon. */}
      <span ref={bylineRef} style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
        <a
          className="repo-link"
          href="https://moumenalaoui.me"
          target="_blank"
          rel="noreferrer noopener"
          style={{ color: MUTED, textDecoration: 'none' }}
        >
          BUILT BY <span style={{ color: WHITE }}>MOUMEN ALAOUI</span>
        </a>
        <a
          className="repo-link"
          href={REPO_URL}
          target="_blank"
          rel="noreferrer noopener"
          aria-label="Source code on GitHub"
          title="Source code on GitHub"
          style={{ display: 'flex', alignItems: 'center', color: MUTED }}
        >
          <GithubMark />
        </a>
      </span>
    </footer>
  )
}
