import { AMBER, BORDER, BORDER_STRONG, CRIMSON, CYAN, LOCAL, MONO, MUTED, SIDEBAR, TYPE, WHITE } from '../theme'
import { SOURCES } from '../lib/sources'
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

const REPO_URL = 'https://github.com/moumenalaoui/globe'

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

export default function StatusBar({ status = 'ok', dataAge = null }) {
  const link = LINK[status] ?? LINK.ok
  // The backend already decided what counts as stale (HEALTH_MAX_AGE_DAYS);
  // don't duplicate the threshold here, just colour by its verdict.
  const stale = dataAge?.status === 'stale'

  return (
    <footer
      style={{
        height: 24,
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '0 16px',
        background: SIDEBAR,
        borderTop: `1px solid ${BORDER}`,
        fontFamily: MONO,
        fontSize: TYPE.tick,
        letterSpacing: '0.05em',
        whiteSpace: 'nowrap',
      }}
    >
      {/* Inline rather than in App.css because it is the only rule this
          component needs — same pattern the outage feed uses for its pulse. */}
      <style>{`.repo-link:hover { color: ${WHITE} } .source-link:hover { color: ${CYAN} }`}</style>

      <span style={{ color: MUTED }}>SOURCES</span>
      <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
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
      </span>

      <span style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: link.color }} />
        <span style={{ color: link.color }}>{link.label}</span>
      </span>
      <span style={{ width: 1, height: 12, background: BORDER }} />
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

      <span style={{ width: 1, height: 12, background: BORDER }} />

      {/* The tool's account of itself sits immediately before the byline: the
          two answer the same question a viewer has on arrival — who made this
          and on what basis — so they read as one credit rather than a menu
          item parked elsewhere. */}
      <a
        {...linkProps('/methodology')}
        className="repo-link"
        style={{ color: MUTED, textDecoration: 'none', flexShrink: 0 }}
      >
        METHODOLOGY
      </a>

      <span style={{ width: 1, height: 12, background: BORDER }} />

      {/* Byline and repo link. Sized to the bar's existing 9px/24px rhythm —
          the 11px mark sits inside the 24px height, so nothing grows. */}
      <span style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
        <span style={{ color: MUTED }}>
          BUILT BY <span style={{ color: WHITE }}>MOUMEN ALAOUI</span>
        </span>
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
