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
// Text-safe crimson. CRIMSON is the brand/fill red, but as *text* on these dark
// surfaces it's ~2.6–3.1:1 — below WCAG AA even for large text, and it all but
// disappears on a projector. #ef4d6b holds 4.9–5.9:1 on BASE, SIDEBAR, RAISED
// and BLACK. Use it wherever red colours text; fills, bars, swatches and
// borders keep CRIMSON.
export const CRIMSON_TEXT = '#ef4d6b'
// Dim partner to CRIMSON (SCSP Dark Red). Currently unused: every crimson
// surface in the app is a flat fill, swatch or text run. Defined here so the
// third SCSP brand color has a home when the wordmark lands.
export const CRIMSON_DIM = '#851432'
export const HIGHLIGHT   = '#d6b36a'
export const AMBER       = '#d97706'
export const DIM         = '#3d4f6b'

// For colour maps shared by fills and text (status / score colours): the tone
// of `color` that's safe as text.
export const textTone = (color) => (color === CRIMSON ? CRIMSON_TEXT : color)

export const MONO        = '"IBM Plex Mono", "Fira Mono", monospace'
export const SANS        = '"Inter", system-ui, sans-serif'

// Type scale, in px, calibrated against the ranking list: Inter at 11 for
// names and running text, Plex Mono at 10 for figures and labels — one step
// above the original UI, whose 8–9px text read too small. Mono glyphs are
// much wider than Inter's, so mono reads visibly bigger than Inter at the same
// size — mono text therefore tops out at `label` (numeric readouts in the
// header aside), and `body` is for Inter. Pick a step by role; never
// hand-pick an off-scale size.
export const TYPE = {
  tick:     9, // chart axis ticks, footnotes, source tags
  label:   10, // mono: uppercase labels, codes, figures, controls
  body:    11, // Inter: names, list entries, running text
  title:   13, // country / satellite name, wordmark
  display: 15, // close glyph
}

// Tier colors are pinned to the stack-dependency palette rather than an
// arbitrary ramp — blue reads as "safe/trusted" which is wrong for a tier
// defined by foreign compute dependency.
export const TIER_COLORS = {
  COMPREHENSIVELY_SANCTIONED: CRIMSON,
  POST_SANCTIONS_LAG:         CN_EXPOSURE,
  US_COMPUTE_STACK:           US_EXPOSURE,
  RESOURCE_CONSTRAINED:       LOCAL,
}
