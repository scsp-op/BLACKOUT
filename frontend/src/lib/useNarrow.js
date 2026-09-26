import { useEffect, useState } from 'react'

// The app's layout breakpoints. At phone widths (NARROW) the header stacks
// into two rows, dock panels open full-width one at a time, the country
// sidebar becomes a bottom sheet and the legends and status bar compact.
// Between that and full desktop (COMPACT_HEADER) only the header trims — its
// counters and the full SCSP lockup need ~1100px. Above, desktop is untouched.
// NARROW ends at 820px because the trimmed one-row header still needs ~795px;
// portrait tablets up to that width get the stacked layout, which suits them.
export const NARROW_QUERY = '(max-width: 820px)'
export const COMPACT_HEADER_QUERY = '(max-width: 1100px)'

export function useMediaQuery(query) {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches)
  useEffect(() => {
    const mq = window.matchMedia(query)
    const onChange = () => setMatches(mq.matches)
    onChange()
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [query])
  return matches
}

export const useNarrow = () => useMediaQuery(NARROW_QUERY)
