import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import Methodology from './components/Methodology'
import { useRoute } from './lib/router'
import './index.css'

function Root() {
  const path = useRoute()
  const inMethodology = path === '/methodology' || path.startsWith('/methodology/')

  // App boots a Cesium viewer and around a dozen fetches on mount, so it is
  // mounted once and then kept — navigating to the documents and back must not
  // pay for that twice. Methodology renders as an opaque fixed overlay on top
  // of it rather than in place of it.
  //
  // The one case worth not paying for is a cold load straight to a
  // /methodology URL — a shared link, or a refresh while reading. There the
  // globe has never been wanted, so it is not mounted at all until the reader
  // asks for it, and the deep link costs a document instead of a globe.
  const [globeMounted, setGlobeMounted] = useState(!inMethodology)
  useEffect(() => {
    if (!inMethodology) setGlobeMounted(true)
  }, [inMethodology])

  return (
    <>
      {globeMounted && <App />}
      {inMethodology && <Methodology path={path} />}
    </>
  )
}

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
