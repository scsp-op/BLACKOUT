// Section numbers, lifted out of the heading text so they can sit in their own
// gutter — "3.1  The inputs" rather than a wrapped run of "3.1 The inputs".
// This is the move that makes a long contents list readable at a glance, and
// both the rail beside a document and the panels on the index need it.
//
// Headings that carry no number ("Why this matters now") come back with
// `number: null` and are simply indented under the one above.
export function splitNumber(text) {
  const m = /^(\d+(?:\.\d+)*)\.?\s+(.+)$/.exec(text)
  // Guard against a heading that merely opens with a year or a large figure;
  // a real section number is short.
  return m && m[1].length <= 6 ? { number: m[1], title: m[2] } : { number: null, title: text }
}
