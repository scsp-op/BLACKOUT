import { MONO, MUTED, TYPE, WHITE } from '../theme'

// Structural chokepoint signal, not a live measurement (see backend/data/
// seed/ixp_stats.json, generated once by gen_ixp_data.mjs) — a country with
// very few domestic exchange points routes nearly all its traffic through a
// handful of international gateways, making a full shutdown fast and cheap.
// Unlike StarlinkBadge, `entry` absent still renders: 0 known exchange
// points is itself the strongest reading this signal can give, not a "no
// data" case to hide. Lives in the NETWORK & PROTOCOL theme in
// CountrySidebar.jsx (alongside BGP visibility) rather than the fixed header
// — it's an analytical infrastructure signal like its section-mates, not an
// at-a-glance flag like StarlinkBadge.
export default function IxpBadge({ entry }) {
  const ixpCount = entry?.ixp_count ?? 0
  const netCount = entry?.total_net_count ?? 0
  const largestName = entry?.largest_ixp_name
  const largestNet = entry?.largest_ixp_net_count

  return (
    <div style={{ border: `1px solid ${MUTED}33`, padding: '4px 8px' }}>
      <div style={{ fontFamily: MONO, fontSize: TYPE.label, letterSpacing: '0.05em', color: WHITE }}>
        INTERNET EXCHANGE POINTS: {ixpCount}
      </div>
      <div style={{ fontFamily: MONO, fontSize: TYPE.tick, color: MUTED, marginTop: 2 }}>
        {netCount} network{netCount === 1 ? '' : 's'} connected
        {largestName ? ` · largest: ${largestName} (${largestNet})` : ''}
        {' · via PeeringDB, manually refreshed'}
      </div>
    </div>
  )
}
