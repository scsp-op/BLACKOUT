import { useState } from 'react'
import { MONO, MUTED, TYPE, WHITE } from '../theme'

export default function CollapsibleSection({ title, defaultOpen = false, children }) {
  const [open, setOpen] = useState(defaultOpen)

  return (
    <section>
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          width: '100%',
          background: 'transparent',
          border: 'none',
          padding: 0,
          marginBottom: open ? 8 : 0,
          cursor: 'pointer',
          fontFamily: MONO,
          fontSize: TYPE.label,
          letterSpacing: '0.06em',
          color: MUTED,
        }}
      >
        <span style={{ color: WHITE, width: 10, flexShrink: 0 }}>{open ? '▾' : '▸'}</span>
        {title}
      </button>

      {open && <div style={{ display: 'flex', flexDirection: 'column', gap: 18 }}>{children}</div>}
    </section>
  )
}
