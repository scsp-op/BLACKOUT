import { AMBER, CRIMSON, MONO, MUTED, TYPE, WHITE } from '../theme'

// BANNED = Starlink's own GPS-geofence; JAMMED/RESTRICTED share a color
// (differentiated by label, not a new theme color) since both mean "not a
// clean path" without Starlink itself having blocked the country outright.
const STATUS_COLOR = {
  BANNED: CRIMSON,
  JAMMED: AMBER,
  RESTRICTED: AMBER,
}

// Hand-curated, not a live measurement (see backend/data/seed/
// starlink_status.json) — the citation in the title/footer text is
// deliberate, so this never reads as a refreshed feed the way every other
// badge/chart in this sidebar is. Renders nothing if `entry` is null/
// undefined: absence means no known restriction, not "unknown". Lives in the
// NETWORK & PROTOCOL theme in CountrySidebar.jsx (alongside IXP density) —
// an alternative-connectivity/infrastructure signal like its section-mates,
// not an at-a-glance flag that belongs ahead of everything else.
export default function StarlinkBadge({ entry }) {
  if (!entry) return null
  const color = STATUS_COLOR[entry.status] ?? MUTED

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'flex-start',
        gap: 6,
        border: `1px solid ${color}`,
        padding: '4px 8px',
      }}
    >
      <span style={{ width: 6, height: 6, borderRadius: '50%', background: color, flexShrink: 0, marginTop: 4 }} />
      <div>
        <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.05em', color: WHITE }}>
          STARLINK: {entry.status}
        </div>
        <div style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, marginTop: 2 }}>
          {entry.note} · {entry.source_note}
        </div>
      </div>
    </div>
  )
}
