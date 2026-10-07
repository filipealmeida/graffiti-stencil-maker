import { useRef, useState } from 'react'
import { create } from 'zustand'
import { persist } from 'zustand/middleware'

/** Overlay preferences outlive the component: it remounts whenever a run replaces the result. */
const usePrefs = create<{ overlay: boolean; opacity: number; set: (p: Partial<{ overlay: boolean; opacity: number }>) => void }>()(
  persist((set) => ({ overlay: true, opacity: 0.6, set: (p) => set(p) }), { name: 'studio.imageview' }),
)

interface Props { src: string; under?: string; kind: 'image' | 'mask' }

/** Zoomable, pannable raster view. A mask can be laid over the image it was derived from. */
export default function ImageView({ src, under, kind }: Props) {
  const [view, setView] = useState({ k: 1, x: 0, y: 0 })
  const { overlay, opacity, set: setPrefs } = usePrefs()
  const drag = useRef<{ x: number; y: number } | null>(null)
  const host = useRef<HTMLDivElement>(null)

  const onWheel = (e: React.WheelEvent) => {
    const r = host.current!.getBoundingClientRect()
    const px = e.clientX - r.left - r.width / 2, py = e.clientY - r.top - r.height / 2
    const f = e.deltaY < 0 ? 1.25 : 0.8
    setView((v) => {
      const k = Math.min(40, Math.max(0.2, v.k * f))
      const g = k / v.k
      return { k, x: px - (px - v.x) * g, y: py - (py - v.y) * g }
    })
  }
  const showUnder = !!under && kind === 'mask' && overlay
  return (
    <div className="imgview">
      <div
        ref={host} className="imgstage" onWheel={onWheel} onDoubleClick={() => setView({ k: 1, x: 0, y: 0 })}
        onPointerDown={(e) => { drag.current = { x: e.clientX - view.x, y: e.clientY - view.y }; (e.target as Element).setPointerCapture?.(e.pointerId) }}
        onPointerMove={(e) => drag.current && setView((v) => ({ ...v, x: e.clientX - drag.current!.x, y: e.clientY - drag.current!.y }))}
        onPointerUp={() => (drag.current = null)}
      >
        <div className="imgstack" style={{ transform: `translate(${view.x}px,${view.y}px) scale(${view.k})` }}>
          {showUnder && <img src={under} alt="" draggable={false} className="under" />}
          <img src={src} alt="" draggable={false} className={`top ${kind}`} style={showUnder ? { mixBlendMode: 'multiply', opacity } : undefined} />
        </div>
      </div>
      <div className="imgbar">
        <span>{Math.round(view.k * 100)}% · wheel zoom, drag pan, double-click reset</span>
        {under && kind === 'mask' && (
          <>
            <label><input type="checkbox" checked={overlay} onChange={(e) => setPrefs({ overlay: e.target.checked })} /> over source</label>
            {overlay && <input type="range" min={0.1} max={1} step={0.05} value={opacity} onChange={(e) => setPrefs({ opacity: +e.target.value })} />}
          </>
        )}
      </div>
    </div>
  )
}
