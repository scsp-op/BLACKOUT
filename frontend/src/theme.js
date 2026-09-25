// Layered surface system — three elevation levels over a near-black base, all
// pinned to the SCSP Blue hue axis (H213). Only the saturation climbs with
// elevation; lightness is fixed per level, which is what keeps text contrast
// identical across the ramp. BORDER_STRONG is SCSP Blue #0A3161 exactly, so the
// brand color appears verbatim as the emphasis border. Keep new surfaces on
// H213 — drifting the hue is what makes a palette look accidental. BLACK is
// retained for the Cesium globe scene, which stays pure black underneath the
// chrome that floats over it.
export const BLACK         = '#000000'
export const BASE          = '#060c13'
export const SIDEBAR       = '#09121e'
export const RAISED        = '#0d1c2f'
export const BORDER        = '#0e243e'
export const BORDER_STRONG = '#0a3161'
export const WHITE         = '#e6e9ef'
export const MUTED         = '#8c95a3'

// HUD / interactive accent. Additive to the existing crimson=alert and
// gold=selection language: cyan marks live interactive surfaces (layer toggles,
// the globe reticle and graticule) and never competes with a blocking-status
// color.
export const CYAN          = '#37c0e6'
export const CYAN_DIM      = '#1c4a5a'

export const US_EXPOSURE = '#a0aec0'
export const CN_EXPOSURE = '#c9822b'
export const LOCAL       = '#6c9a5b'
export const CRIMSON     = '#b31942'
// Dim partner to CRIMSON (SCSP Dark Red). Currently unused: every crimson
// surface in the app is a flat fill, swatch or text run, and the globe's bloom
// falloff is shared with the amber layer so it can't take a red tint. Defined
// here so the third SCSP brand color has a home when the wordmark lands.
export const CRIMSON_DIM = '#851432'
export const HIGHLIGHT   = '#d6b36a'
export const AMBER       = '#d97706'
export const DIM         = '#3d4f6b'

export const MONO        = '"IBM Plex Mono", "Fira Mono", monospace'
export const SANS        = '"Inter", system-ui, sans-serif'

// Tier colors are pinned to the stack-dependency palette rather than an
// arbitrary ramp — blue reads as "safe/trusted" which is wrong for a tier
// defined by foreign compute dependency.
export const TIER_COLORS = {
  COMPREHENSIVELY_SANCTIONED: CRIMSON,
  POST_SANCTIONS_LAG:         CN_EXPOSURE,
  US_COMPUTE_STACK:           US_EXPOSURE,
  RESOURCE_CONSTRAINED:       LOCAL,
}
