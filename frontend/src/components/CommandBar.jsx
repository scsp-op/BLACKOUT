import { useEffect } from 'react'
import { BLACK, BORDER, CRIMSON, CRIMSON_TEXT, HIGHLIGHT, MONO, MUTED, RAISED, SANS, SIDEBAR, TYPE, WHITE } from '../theme'
import ScspLogo from './ScspLogo'

const Divider = () => <span style={{ width: 1, height: 22, background: BORDER, flexShrink: 0 }} />

function CountrySelect({ value, options, onChange }) {
  return (
    <select
      aria-label="Country"
      value={value}
      onChange={(event) => onChange(event.target.value)}
      style={{
        height: 30,
        background: BLACK,
        border: `1px solid ${BORDER}`,
        borderRadius: 0,
        color: WHITE,
        fontFamily: MONO,
        fontSize: TYPE.label,
        letterSpacing: '0.05em',
        padding: '0 8px',
        outline: 'none',
        minWidth: 170,
      }}
    >
      <option value="" disabled style={{ background: BLACK, color: MUTED }}>
        Select country
      </option>
      {options.map((option) => (
        <option key={option.value} value={option.value} style={{ background: BLACK, color: WHITE }}>
          {option.label}
        </option>
      ))}
    </select>
  )
}

// A single labelled counter. (The outage count lives on the OUTAGES dock
// button instead, crimson when non-zero.)
function Stat({ label, value }) {
  return (
    <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.15 }}>
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
function DockItem({ id, label, badge, badgeColor, pulse, open, onToggle }) {
  return (
    <button
      type="button"
      onClick={() => onToggle(id)}
      aria-expanded={open}
      style={{
        height: 30,
        display: 'flex',
        alignItems: 'center',
        gap: 7,
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
export default function CommandBar({ countries, selectedCode, onSelectCountry, counts, openPanels, onTogglePanel, onCloseAll }) {
  const anyOpen = Object.values(openPanels).some(Boolean)

  // Escape closes every open panel. No click-outside close: the panels sit at
  // the globe's edges, so the globe stays usable with them open.
  useEffect(() => {
    if (!anyOpen) return undefined
    const onKey = (e) => e.key === 'Escape' && onCloseAll()
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [anyOpen, onCloseAll])

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
        <CountrySelect
          value={selectedCode}
          options={countries.map((c) => ({ value: c.country_code, label: c.country_name }))}
          onChange={onSelectCountry}
        />
      </div>

      <Divider />

      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        <DockItem id="ranking" label="RANKING" open={openPanels.ranking} onToggle={onTogglePanel} />
        <DockItem
          id="outages"
          label="OUTAGES"
          badge={counts.outages}
          badgeColor={counts.outages > 0 ? CRIMSON_TEXT : MUTED}
          pulse={counts.outages > 0}
          open={openPanels.outages}
          onToggle={onTogglePanel}
        />
        <DockItem id="satellites" label="SATELLITES" open={openPanels.satellites} onToggle={onTogglePanel} />
      </div>

      <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 16 }}>
        <Stat label="COUNTRIES" value={counts.countries} />
        <Stat label="SIGNALS" value={counts.signals} />

        {/* Reversed SCSP lockup closes the bar. Same Divider as the wordmark
            side, so the header reads as one rule of instrument groups rather
            than a logo bolted on. WHITE rather than pure #fff: the mark sits
            at the same tone as the rest of the chrome text. */}
        <Divider />
        <ScspLogo height={26} color={WHITE} />
      </div>
    </header>
  )
}
