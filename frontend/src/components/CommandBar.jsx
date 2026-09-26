import { useEffect, useRef, useState } from 'react'
import { BLACK, BORDER, BORDER_STRONG, CRIMSON, HIGHLIGHT, MONO, MUTED, RAISED, SANS, SIDEBAR, TYPE, WHITE } from '../theme'
import ScspLogo from './ScspLogo'
import { COMPACT_HEADER_QUERY, useMediaQuery } from '../lib/useNarrow'

const Divider = () => <span style={{ width: 1, height: 22, background: BORDER, flexShrink: 0 }} />

// Accent- and case-insensitive key, so "cote" finds "Côte d'Ivoire".
const fold = (text) => text.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase()

// Searchable country picker. Replaces a native <select>: its OS-styled list
// broke the dark UI, and scrolling ~235 entries to find one was slow. Type to
// filter by name or ISO code; ↑/↓ + Enter or click to pick; Escape or a click
// outside closes. Shows the selected country's name when not being edited.
function CountryPicker({ value, options, onChange, fluid = false }) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(0)
  const rootRef = useRef(null)
  const listRef = useRef(null)

  const selected = options.find((o) => o.value === value)
  const q = fold(query.trim())
  // Ranked: exact ISO code, then names starting with the query, then names
  // containing it — so "de" puts Germany ahead of Bangladesh.
  const matches = q
    ? options
        .map((o) => {
          const name = fold(o.label)
          const rank = o.value.toLowerCase() === q ? 0 : name.startsWith(q) ? 1 : name.includes(q) ? 2 : -1
          return { o, rank }
        })
        .filter((m) => m.rank >= 0)
        .sort((a, b) => a.rank - b.rank)
        .map((m) => m.o)
    : options

  // Close on a press anywhere outside the picker. `pointerdown`, not
  // `mousedown`: Cesium suppresses the compatibility mouse events on its
  // canvas, so a click on the globe never produced a mousedown.
  useEffect(() => {
    if (!open) return undefined
    const onDown = (e) => {
      if (rootRef.current && !rootRef.current.contains(e.target)) {
        setOpen(false)
        setQuery('')
      }
    }
    document.addEventListener('pointerdown', onDown, true)
    return () => document.removeEventListener('pointerdown', onDown, true)
  }, [open])

  // Keep the keyboard-highlighted row in view.
  useEffect(() => {
    listRef.current?.children[active]?.scrollIntoView({ block: 'nearest' })
  }, [active, open])

  const pick = (option) => {
    onChange(option.value)
    setQuery('')
    setOpen(false)
  }

  const onKeyDown = (e) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setOpen(true)
      setActive((i) => Math.min(i + 1, matches.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActive((i) => Math.max(i - 1, 0))
    } else if (e.key === 'Enter') {
      if (open && matches[active]) pick(matches[active])
    } else if (e.key === 'Escape') {
      // Handled here only: don't also close the dock panels.
      e.stopPropagation()
      setQuery('')
      setOpen(false)
      e.currentTarget.blur()
    }
  }

  return (
    <div ref={rootRef} style={{ position: 'relative', ...(fluid ? { flex: 1, minWidth: 0 } : null) }}>
      <input
        type="text"
        role="combobox"
        aria-label="Country"
        aria-expanded={open}
        aria-controls="country-picker-list"
        spellCheck={false}
        autoComplete="off"
        placeholder={selected ? selected.label : 'Search country'}
        value={open ? query : (selected?.label ?? '')}
        onFocus={() => {
          setOpen(true)
          setQuery('')
          setActive(0)
        }}
        onChange={(e) => {
          setQuery(e.target.value)
          setActive(0)
          setOpen(true)
        }}
        onKeyDown={onKeyDown}
        style={{
          height: 30,
          width: fluid ? '100%' : 200,
          background: BLACK,
          border: `1px solid ${open ? BORDER_STRONG : BORDER}`,
          borderRadius: 0,
          color: WHITE,
          fontFamily: MONO,
          fontSize: TYPE.label,
          letterSpacing: '0.05em',
          padding: '0 8px',
          outline: 'none',
        }}
      />
      {open && (
        <div
          id="country-picker-list"
          role="listbox"
          ref={listRef}
          style={{
            position: 'absolute',
            top: 'calc(100% + 4px)',
            left: 0,
            width: fluid ? '100%' : 240,
            maxHeight: 320,
            overflowY: 'auto',
            background: SIDEBAR,
            border: `1px solid ${BORDER}`,
            boxShadow: '0 8px 24px rgba(0, 0, 0, 0.45)',
            padding: '4px 0',
            // Above the globe overlays (dock panels, legends: zIndex 5).
            zIndex: 30,
          }}
        >
          {matches.length === 0 && (
            <div style={{ padding: '6px 10px', fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>No match</div>
          )}
          {matches.map((o, i) => (
            <div
              key={o.value}
              role="option"
              aria-selected={o.value === value}
              // mousedown, not click: fires before the input's blur/outside
              // handling, so the pick always lands.
              onMouseDown={(e) => {
                e.preventDefault()
                pick(o)
              }}
              onMouseEnter={() => setActive(i)}
              style={{
                display: 'flex',
                alignItems: 'baseline',
                gap: 8,
                padding: '4px 10px',
                cursor: 'pointer',
                background: i === active ? RAISED : 'transparent',
              }}
            >
              <span style={{ flex: 1, fontSize: TYPE.body, color: o.value === value ? HIGHLIGHT : WHITE }}>{o.label}</span>
              <span style={{ fontFamily: MONO, fontSize: TYPE.label, color: MUTED }}>{o.value}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

// A single labelled counter. (The outage count lives on the OUTAGES dock
// button instead, crimson when non-zero.)
function Stat({ label, value, title }) {
  return (
    <span title={title} style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.15, cursor: title ? 'help' : 'default' }}>
      <span style={{ fontFamily: MONO, fontSize: TYPE.tick, letterSpacing: '0.08em', color: MUTED }}>{label}</span>
      <span
        className="tabular"
        style={{ fontFamily: MONO, fontSize: TYPE.body, letterSpacing: '0.03em', color: WHITE }}
      >
        {value}
      </span>
    </span>
  )
}

// One dock button. The highlighted border is what ties it to the panel it
// opened at the globe's edge (App's DockColumn / DockPanel).
function DockItem({ id, label, badge, badgeColor, pulse, open, onToggle, stretch = false }) {
  return (
    <button
      type="button"
      onClick={() => onToggle(id)}
      aria-expanded={open}
      style={{
        height: 30,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 7,
        // On phones the three buttons share the row equally.
        flex: stretch ? 1 : 'none',
        background: open ? RAISED : 'transparent',
        border: `1px solid ${open ? HIGHLIGHT : BORDER}`,
        color: open ? HIGHLIGHT : WHITE,
        fontFamily: MONO,
        fontSize: TYPE.label,
        letterSpacing: '0.08em',
        padding: '0 10px',
        cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
    >
      {pulse && (
        <span
          aria-hidden="true"
          style={{
            width: 6,
            height: 6,
            borderRadius: '50%',
            background: CRIMSON,
            animation: 'outagePulse 1.4s ease-in-out infinite',
          }}
        />
      )}
      {label}
      {badge != null && <span className="tabular" style={{ color: badgeColor ?? MUTED }}>{badge}</span>}
    </button>
  )
}

// Presentational command bar. Every interactive control is a controlled input
// driven by props from App. The dock makes the panels that used to be always
// on optional: each button toggles its own panel, floating at the globe's edge
// as before (ranking and satellites on the left, outages on the right), any
// combination can be open at once, and nothing is open on load so the globe
// starts clean.
export default function CommandBar({ countries, selectedCode, onSelectCountry, counts, openPanels, onTogglePanel, onCloseAll, narrow = false }) {
  const anyOpen = Object.values(openPanels).some(Boolean)
  // Tablet widths: the full desktop row needs ~1100px, so drop the counters
  // and show the SCSP emblem instead of the full lockup.
  const compactHeader = useMediaQuery(COMPACT_HEADER_QUERY)

  // Escape closes every open panel. No click-outside close: the panels sit at
  // the globe's edges, so the globe stays usable with them open.
  useEffect(() => {
    if (!anyOpen) return undefined
    const onKey = (e) => e.key === 'Escape' && onCloseAll()
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [anyOpen, onCloseAll])

  const options = countries.map((c) => ({ value: c.country_code, label: c.country_name }))
  const dock = (
    <>
      <DockItem id="ranking" label="RANKING" open={openPanels.ranking} onToggle={onTogglePanel} stretch={narrow} />
      <DockItem
        id="outages"
        label="OUTAGES"
        badge={counts.outages}
        badgeColor={counts.outages > 0 ? CRIMSON : MUTED}
        pulse={counts.outages > 0}
        open={openPanels.outages}
        onToggle={onTogglePanel}
        stretch={narrow}
      />
      <DockItem id="satellites" label="SATELLITES" open={openPanels.satellites} onToggle={onTogglePanel} stretch={narrow} />
    </>
  )

  // Phone width: two rows — wordmark, search and the SCSP emblem, then the
  // dock buttons sharing the full width. The tagline, the counters and the
  // full SCSP lockup don't fit and are dropped.
  if (narrow) {
    return (
      <header
        style={{
          flexShrink: 0,
          display: 'flex',
          flexDirection: 'column',
          gap: 8,
          padding: '8px 12px',
          background: SIDEBAR,
          borderBottom: `1px solid ${BORDER}`,
        }}
      >
        <style>{`@keyframes outagePulse { 0%,100% { opacity: 1 } 50% { opacity: 0.25 } }`}</style>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <span style={{ fontFamily: SANS, fontWeight: 600, fontSize: TYPE.title, letterSpacing: '0.08em', color: WHITE, flexShrink: 0 }}>
            BLACKOUT
          </span>
          <CountryPicker value={selectedCode} options={options} onChange={onSelectCountry} fluid />
          <ScspLogo height={24} color={WHITE} markOnly />
        </div>
        <div style={{ display: 'flex', gap: 6 }}>{dock}</div>
      </header>
    )
  }

  return (
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
      <style>{`@keyframes outagePulse { 0%,100% { opacity: 1 } 50% { opacity: 0.25 } }`}</style>
      {/* Wordmark plus a one-line statement of what the tool measures. Without
          it "BLACKOUT" alone gives a first-time viewer nothing to anchor the
          globe and the panels to. */}
      <span style={{ display: 'flex', flexDirection: 'column', gap: 1, flexShrink: 0 }}>
        <span style={{ fontFamily: SANS, fontWeight: 600, fontSize: TYPE.title, letterSpacing: '0.08em', color: WHITE }}>
          BLACKOUT
        </span>
        <span style={{ fontFamily: MONO, fontSize: TYPE.tick, letterSpacing: '0.06em', color: MUTED, whiteSpace: 'nowrap' }}>
          INTERNET FREEDOM, MEASURED LIVE
        </span>
      </span>

      <Divider />

      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <CountryPicker value={selectedCode} options={options} onChange={onSelectCountry} />
      </div>

      <Divider />

      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>{dock}</div>

      <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 16 }}>
        {!compactHeader && (
          <>
            <Stat
              label="COUNTRIES & TERRITORIES"
              value={counts.countries}
              title="Countries and territories on the map"
            />
            <Stat
              label="COUNTRIES BLOCKING"
              value={counts.blockingCountries}
              title="Countries where OONI measurements confirm at least one tracked messaging app, AI service or circumvention tool is blocked"
            />
          </>
        )}

        {/* Reversed SCSP lockup closes the bar. Same Divider as the wordmark
            side, so the header reads as one rule of instrument groups rather
            than a logo bolted on. WHITE rather than pure #fff: the mark sits
            at the same tone as the rest of the chrome text. */}
        <Divider />
        <ScspLogo height={26} color={WHITE} markOnly={compactHeader} />
      </div>
    </header>
  )
}
