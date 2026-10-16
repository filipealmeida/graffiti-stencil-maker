import { useEffect, useRef, useState } from 'react'
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
  const canvas = useRef<HTMLCanvasElement>(null)
  const [imgs, setImgs] = useState<{ top?: HTMLImageElement; under?: HTMLImageElement }>({})
  const [box, setBox] = useState({ w: 0, h: 0 })

  // Drawn on a canvas the size of the panel: scaling a big element with CSS at high zoom makes a huge GPU layer that can blank the whole window.
  useEffect(() => {
    let alive = true
    const load = (u?: string) => new Promise<HTMLImageElement | undefined>((res) => {
      if (!u) return res(undefined)
      const i = new Image()
      i.onload = () => res(i)
      i.onerror = () => res(undefined)
      i.src = u
    })
    Promise.all([load(src), showUnder ? load(under) : Promise.resolve(undefined)]).then(([top, u]) => alive && setImgs({ top, under: u }))
    return () => { alive = false }
  }, [src, under, showUnder])

  useEffect(() => {
    const el = host.current!
    const ro = new ResizeObserver(() => setBox({ w: el.clientWidth, h: el.clientHeight }))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  useEffect(() => {
    const c = canvas.current, top = imgs.top
    if (!c || !top || !box.w || !box.h) return
    const dpr = window.devicePixelRatio || 1
    c.width = Math.round(box.w * dpr)
    c.height = Math.round(box.h * dpr)
    const g = c.getContext('2d')!
    g.imageSmoothingEnabled = false
    g.setTransform(dpr, 0, 0, dpr, 0, 0)
    const bw = Math.min(box.w * 0.9, 900), bh = bw * top.naturalHeight / top.naturalWidth
    g.translate(box.w / 2 + view.x, box.h / 2 + view.y)
    g.scale(view.k, view.k)
    g.fillStyle = '#fff'
    if (imgs.under && showUnder) {
      g.fillRect(-bw / 2, -bh / 2, bw, bh)
      g.drawImage(imgs.under, -bw / 2, -bh / 2, bw, bh)
      g.globalCompositeOperation = 'multiply'
      g.globalAlpha = opacity
    } else if (kind === 'mask') g.fillRect(-bw / 2, -bh / 2, bw, bh)
    g.drawImage(top, -bw / 2, -bh / 2, bw, bh)
  }, [imgs, box, view, opacity, showUnder, kind])
  return (
    <div className="imgview">
      <div
        ref={host} className="imgstage" onWheel={onWheel} onDoubleClick={() => setView({ k: 1, x: 0, y: 0 })}
        onPointerDown={(e) => { drag.current = { x: e.clientX - view.x, y: e.clientY - view.y }; (e.target as Element).setPointerCapture?.(e.pointerId) }}
        onPointerMove={(e) => { const d = drag.current; if (d) setView((v) => ({ ...v, x: e.clientX - d.x, y: e.clientY - d.y })) }}
        onPointerUp={() => (drag.current = null)} onPointerCancel={() => (drag.current = null)}
      >
        <canvas ref={canvas} className="imgcanvas" />
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
