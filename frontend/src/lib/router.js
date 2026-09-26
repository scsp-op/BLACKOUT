// A small router, hand-rolled.
//
// Routes: the globe `/`, a selected country `/country/XX` (see App.jsx), and
// the methodology documents — `/methodology`, `/methodology/value` and
// `/methodology/technical`.
// That does not pay for react-router: the hard part of client-side routing is
// making a deep link survive a refresh, and the backend already does it.
// `fallback_service(spa_service(..))` in main.rs serves index.html for any path
// ServeDir cannot find, so /methodology/technical returns the app rather than a
// 404, and Vite's dev server does the same thing in development.
//
// What is left is reading the path and re-rendering when it changes, which is
// this file.
import { useEffect, useState } from 'react'

// Trailing slashes are normalised away so `/methodology/` and `/methodology`
// are the same route, and `/` survives as `/` rather than collapsing to ''.
function currentPath() {
  return window.location.pathname.replace(/\/+$/, '') || '/'
}

// The last path outside /methodology — the globe, or a /country/XX link. The
// documents' "back to the globe" links go here, so returning from them keeps
// the country that was open rather than resetting to the bare globe.
const isDocs = (path) => path === '/methodology' || path.startsWith('/methodology/')
let lastGlobePath = isDocs(currentPath()) ? '/' : currentPath()
window.addEventListener('popstate', () => {
  const path = currentPath()
  if (!isDocs(path)) lastGlobePath = path
})

/** Where "back to the globe" should go (see `lastGlobePath`). */
export function globePath() {
  return lastGlobePath
}

/** The current path, kept in sync with Back/Forward and with `navigate`. */
export function useRoute() {
  const [path, setPath] = useState(currentPath)

  useEffect(() => {
    const onPop = () => setPath(currentPath())
    window.addEventListener('popstate', onPop)
    return () => window.removeEventListener('popstate', onPop)
  }, [])

  return path
}

/**
 * Push a new path and re-render.
 *
 * `pushState` deliberately does not fire `popstate` — that event means "the
 * user moved through history", not "the URL changed". Dispatching one by hand
 * is what lets `useRoute` have a single subscription that covers both our own
 * navigations and the browser's back button.
 */
export function navigate(path, { replace = false } = {}) {
  if (path === currentPath()) return
  if (replace) window.history.replaceState({}, '', path)
  else window.history.pushState({}, '', path)
  window.dispatchEvent(new PopStateEvent('popstate'))
}

/** An <a> that routes in-app but still behaves like a real link. */
export function linkProps(path) {
  return {
    href: path,
    onClick: (e) => {
      // Let the browser handle anything that means "open this somewhere else"
      // — middle-click, cmd/ctrl-click, shift-click — so the documents can be
      // opened in a new tab like any other page.
      if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
      e.preventDefault()
      navigate(path)
    },
  }
}
