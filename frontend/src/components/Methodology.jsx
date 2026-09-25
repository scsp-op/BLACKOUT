import { useEffect, useState } from 'react'
import MethodologyDoc from './MethodologyDoc'
import { getMethodologyIndex } from '../lib/api'
import { linkProps } from '../lib/router'
import { splitNumber } from '../lib/toc'
import { BASE, BORDER, BORDER_STRONG, CYAN, DIM, HIGHLIGHT, MONO, MUTED, RAISED, SANS, SIDEBAR, WHITE } from '../theme'

// The documents render over the globe rather than in place of it: App owns a
// Cesium viewer and roughly a dozen mount-time fetches, and tearing that down
// to read a document would mean paying for the whole boot again on the way
// back. `position: fixed` + an opaque background covers it completely; main.jsx
// decides whether the globe is mounted underneath at all.
const OVERLAY_Z = 60

/** Slug from `/methodology/<slug>`, or null on the chooser route itself. */
export function docSlugFromPath(path) {
  const rest = path.replace(/^\/methodology\/?/, '')
  return rest === '' ? null : rest
}

function Divider() {
  return <span style={{ width: 1, height: 18, background: BORDER, flexShrink: 0 }} />
}

// The two documents, as a pair of panels.
//
// A panel per document, in the tool's chrome: MONO caps header strip with a
// right-aligned count, then the title and what it is for, then — and this is
// the part that earns the space — the document's own top-level contents. A
// chooser that only names two documents makes a reader guess; one that lists
// what is inside each answers the question they actually have. The section
// titles come from the index endpoint, which already had them.
function DocPanel({ doc }) {
  return (
    <a
      {...linkProps(`/methodology/${doc.slug}`)}
      className="doc-panel"
      style={{
        display: 'flex',
        flexDirection: 'column',
        background: SIDEBAR,
        border: `1px solid ${BORDER}`,
        textDecoration: 'none',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '8px 12px',
          borderBottom: `1px solid ${BORDER}`,
        }}
      >
        <span style={{ fontFamily: MONO, fontSize: 10, letterSpacing: '0.1em', color: WHITE }}>
          {doc.kind}
        </span>
        <span className="tabular" style={{ marginLeft: 'auto', fontFamily: MONO, fontSize: 9, letterSpacing: '0.08em', color: MUTED }}>
          {doc.sections.length} SECTIONS · {doc.word_count.toLocaleString()} WORDS
        </span>
      </div>

      <div style={{ padding: '16px 12px 18px' }}>
        <div style={{ fontFamily: SANS, fontSize: 17, fontWeight: 500, letterSpacing: '0.01em', color: WHITE }}>
          {doc.title}
        </div>
        <div style={{ fontFamily: SANS, fontSize: 13.5, lineHeight: 1.6, color: MUTED, marginTop: 7 }}>
          {doc.subtitle}
        </div>
      </div>

      {/* The contents, in the same numbered-gutter shape as the rail beside an
          open document, so the two read as the same list. */}
      <div style={{ borderTop: `1px solid ${BORDER}`, padding: '8px 0 10px' }}>
        {doc.sections.map((section, i) => {
          const { number, title } = splitNumber(section)
          return (
            <div key={i} style={{ display: 'flex', gap: 10, padding: '3px 12px' }}>
              <span
                className="tabular"
                style={{ fontFamily: MONO, fontSize: 10.5, width: 18, flexShrink: 0, color: DIM, paddingTop: 1 }}
              >
                {number ?? ''}
              </span>
              <span style={{ fontFamily: SANS, fontSize: 12.5, lineHeight: 1.45, color: '#aab3c0' }}>{title}</span>
            </div>
          )
        })}
      </div>
    </a>
  )
}

function Chooser() {
  const [docs, setDocs] = useState([])
  const [error, setError] = useState('')

  useEffect(() => {
    let cancelled = false
    getMethodologyIndex()
      .then((d) => !cancelled && setDocs(d))
      .catch(() => !cancelled && setError('Could not load the methodology index.'))
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <div style={{ flex: 1, overflowY: 'auto' }}>
      {/* A flat background change on the whole panel — the tool's own RAISED
          elevation, not a transform or a shadow. */}
      <style>{`.doc-panel:hover { background: ${RAISED}; border-color: ${BORDER_STRONG} }`}</style>

      <div
        style={{
          minHeight: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          padding: '40px 24px 64px',
        }}
      >
        <div style={{ width: '100%', maxWidth: 940 }}>
          {error ? (
            <p style={{ fontFamily: MONO, fontSize: 11, color: MUTED }}>{error}</p>
          ) : (
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(330px, 1fr))',
                gap: 16,
                alignItems: 'start',
              }}
            >
              {docs.map((doc) => (
                <DocPanel key={doc.slug} doc={doc} />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

export default function Methodology({ path }) {
  const slug = docSlugFromPath(path)

  // A document and the chooser are separate scroll positions; without this,
  // opening a document from halfway down the chooser starts it halfway down.
  useEffect(() => {
    document.getElementById('methodology-scroll')?.scrollTo({ top: 0 })
  }, [slug])

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
      {/* Mirrors CommandBar's 50px bar so the chrome does not jump height
          when the overlay opens over it. */}
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
        <style>{`
          .methodology-nav:hover { color: ${CYAN} !important }
          .methodology-tab:hover { color: ${HIGHLIGHT} !important; border-color: ${HIGHLIGHT} !important }
        `}</style>

        <a
          {...linkProps('/')}
          style={{
            fontFamily: SANS,
            fontWeight: 600,
            fontSize: 13,
            letterSpacing: '0.14em',
            color: WHITE,
            textDecoration: 'none',
            flexShrink: 0,
          }}
        >
          BLACKOUT
        </a>

        <Divider />

        {/* A real button rather than a 9px caption. It is the only way back
            to the index from inside a document, and at chrome sizing it read
            as a label you were not meant to press. Bordered, padded and set
            at 11.5px, it borrows GlobalRanking's V-DEM/RSF toggle exactly —
            including the HIGHLIGHT border that marks the one you are on. */}
        <a
          {...linkProps('/methodology')}
          className="methodology-tab"
          style={{
            fontFamily: MONO,
            fontSize: 11.5,
            letterSpacing: '0.14em',
            padding: '5px 12px',
            border: `1px solid ${slug ? BORDER : HIGHLIGHT}`,
            color: slug ? MUTED : HIGHLIGHT,
            textDecoration: 'none',
            flexShrink: 0,
          }}
        >
          METHODOLOGY
        </a>

        <a
          {...linkProps('/')}
          className="methodology-nav"
          style={{
            marginLeft: 'auto',
            fontFamily: MONO,
            fontSize: 10.5,
            letterSpacing: '0.12em',
            color: MUTED,
            textDecoration: 'none',
          }}
        >
          BACK TO GLOBE
        </a>
      </header>

      {/* A row, not a scroller. The document view puts the scrollbar on the
          article alone so its contents rail can hold still; the chooser brings
          its own. */}
      <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
        {slug ? <MethodologyDoc slug={slug} /> : <Chooser />}
      </div>
    </div>
  )
}
