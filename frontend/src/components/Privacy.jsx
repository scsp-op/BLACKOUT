import { useEffect } from 'react'
import { PROSE_CSS } from './MethodologyDoc'
import { ISSUES_URL, REPO_URL } from '../lib/links'
import { globePath, linkProps } from '../lib/router'
import { BASE, BORDER, CYAN, HIGHLIGHT, MONO, MUTED, RAISED, SANS, SIDEBAR, TYPE, WHITE } from '../theme'

const LAST_UPDATED = '26 September 2026'

// Same overlay layer as the methodology documents (see Methodology.jsx): the
// page covers the globe rather than replacing it, so returning is instant.
const OVERLAY_Z = 60

// The tool's own privacy notice. It deliberately doesn't defer to SCSP's
// organisation-wide policy: BLACKOUT makes a narrower promise — no tracking of
// any kind — and every statement below is checkable against the code:
//   - no cookies, localStorage/sessionStorage, analytics or tracking scripts
//     anywhere in the frontend;
//   - no third-party requests on load (fonts are self-hosted, see main.jsx);
//   - no request logging in the backend.
// If any of that changes, this page has to change with it.
export default function Privacy() {
  useEffect(() => {
    document.getElementById('privacy-scroll')?.scrollTo({ top: 0 })
  }, [])

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: OVERLAY_Z,
        background: BASE,
        display: 'flex',
        flexDirection: 'column',
      }}
    >
      {/* The methodology overlay's header, so the two documents share chrome. */}
      <header
        style={{
          height: 50,
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 16,
          padding: '0 16px',
          background: SIDEBAR,
          borderBottom: `1px solid ${BORDER}`,
        }}
      >
        <style>{`.privacy-nav:hover { color: ${CYAN} !important }`}</style>
        <a
          {...linkProps(globePath())}
          style={{ fontFamily: SANS, fontWeight: 600, fontSize: TYPE.title, letterSpacing: '0.08em', color: WHITE, textDecoration: 'none', flexShrink: 0 }}
        >
          BLACKOUT
        </a>
        <span style={{ width: 1, height: 18, background: BORDER, flexShrink: 0 }} />
        {/* Styled like the main header's dock buttons, in their "open" state. */}
        <span
          style={{
            height: 30,
            display: 'flex',
            alignItems: 'center',
            fontFamily: MONO,
            fontSize: TYPE.label,
            letterSpacing: '0.08em',
            padding: '0 10px',
            background: RAISED,
            border: `1px solid ${HIGHLIGHT}`,
            color: HIGHLIGHT,
            flexShrink: 0,
          }}
        >
          PRIVACY
        </span>
        <a
          {...linkProps(globePath())}
          className="privacy-nav"
          style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.12em', color: MUTED, textDecoration: 'none' }}
        >
          BACK TO GLOBE
        </a>
      </header>

      <div id="privacy-scroll" style={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
        <style>{PROSE_CSS}</style>
        <article style={{ maxWidth: 820, margin: '0 auto', padding: '32px clamp(16px, 5vw, 40px) 120px' }}>
          {/* The panel-style rule line the methodology documents open with. */}
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
            <span style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.1em', color: WHITE }}>PRIVACY</span>
            <span className="tabular" style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>
              LAST UPDATED {LAST_UPDATED.toUpperCase()}
            </span>
          </header>

          <div className="md-prose">
            <p>
              <strong>BLACKOUT does not track you.</strong> It has no accounts, sets no cookies, stores nothing in
              your browser, and runs no analytics, advertising or tracking scripts.
            </p>

            <h3>What the tool collects</h3>
            <p>
              Nothing about you. BLACKOUT is a read-only map of public data: there are no sign-ups, forms or
              accounts, and nothing you do on the page — the countries you open, the panels you use, what you
              search for — is recorded or sent anywhere.
            </p>

            <h3>What your browser loads</h3>
            <p>
              Everything the page needs, including its fonts, is served by BLACKOUT itself. Opening the tool makes
              no requests to any third party — no font services, analytics providers or ad networks.
            </p>

            <h3>Links to other sites</h3>
            <p>
              The source names in the status bar, the tracking link on a satellite card, the GitHub link and the
              SCSP logo open other websites in a new tab. Those sites have their own privacy practices. BLACKOUT
              shares nothing with them; they only see your visit if you click through.
            </p>

            <h3>Where the data comes from</h3>
            <p>
              The map shows public, aggregated, country-level data from the sources listed in the status bar and
              described in the <a {...linkProps('/methodology')}>methodology</a>. BLACKOUT&apos;s server fetches
              that data itself — your browser never contacts those sources — and none of it identifies
              individuals.
            </p>

            <h3>Server logs</h3>
            <p>
              Like any website, BLACKOUT runs on a hosting provider whose infrastructure may keep standard technical
              logs of requests, such as IP address, time and the page requested, for security and operations.
              BLACKOUT itself does not record or analyse visitors, and holds no personal data about them — so there
              is nothing for it to access, correct or delete.
            </p>

            <h3>Open source</h3>
            <p>
              BLACKOUT is open source, so these statements can be checked against{' '}
              <a href={REPO_URL} target="_blank" rel="noreferrer noopener">its code</a>. If what the tool does
              changes, this page will be updated and the date above will change with it.
            </p>

            <h3>Questions</h3>
            <p>
              Questions about this page or the tool can be raised by{' '}
              <a href={ISSUES_URL} target="_blank" rel="noreferrer noopener">opening an issue on GitHub</a>.
            </p>
          </div>
        </article>
      </div>
    </div>
  )
}
