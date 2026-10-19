import { useEffect, useRef, useState } from 'react'
import { historyItems, jumpTo, useStudio } from './store'

/** The undo history as a list: pick any earlier (or later) state to go straight to it. */
export default function History() {
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLSpanElement>(null)
  useStudio((s) => s.hv)
  useEffect(() => {
    if (!open) return
    const away = (e: MouseEvent) => { if (!box.current?.contains(e.target as Node)) setOpen(false) }
    window.addEventListener('mousedown', away)
    return () => window.removeEventListener('mousedown', away)
  }, [open])
  const items = open ? historyItems() : []
  return (
    <span ref={box} style={{ position: 'relative' }}>
      <button className={open ? 'on' : ''} onClick={() => setOpen(!open)} title="List of edits: pick one to go back (or forward) to it" aria-expanded={open}>
        History
      </button>
      {open && (
        <div className="hist" role="listbox" aria-label="Edit history">
          {[...items].map((it, i) => ({ it, i })).reverse().map(({ it, i }) => (
            <button key={i} role="option" aria-selected={it.current} className={it.current ? 'now' : ''} onClick={() => { jumpTo(i); setOpen(false) }}>
              {it.current ? '● ' : ''}{it.label}
            </button>
          ))}
        </div>
      )}
    </span>
  )
}
