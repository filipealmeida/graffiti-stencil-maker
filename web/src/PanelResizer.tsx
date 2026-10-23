import { useEffect, useState } from 'react'

const KEY = 'studio.inspectorWidth'
const MIN = 300
const MAX = 700

function clamp(w: number) {
  return Math.min(MAX, Math.max(MIN, Math.round(w)))
}

/** A drag handle between the canvas and the side panel. The width is remembered. */
export default function PanelResizer() {
  const [width, setWidth] = useState(() => {
    const saved = Number(localStorage.getItem(KEY))
    return saved ? clamp(saved) : 380
  })
  useEffect(() => {
    document.documentElement.style.setProperty('--inspector-w', `${width}px`)
    try { localStorage.setItem(KEY, String(width)) } catch { /* a convenience only */ }
  }, [width])
  const drag = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    const move = (ev: PointerEvent) => setWidth(clamp(window.innerWidth - ev.clientX))
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }
  return (
    <div
      className="resizer" role="separator" aria-orientation="vertical" aria-label="Resize the side panel"
      aria-valuemin={MIN} aria-valuemax={MAX} aria-valuenow={width} tabIndex={0} onPointerDown={drag}
      onDoubleClick={() => setWidth(380)}
      onKeyDown={(e) => {
        if (e.key === 'ArrowLeft') setWidth((w) => clamp(w + 20))
        if (e.key === 'ArrowRight') setWidth((w) => clamp(w - 20))
      }}
    />
  )
}
