import { useEffect, useRef, useState } from 'react'
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { applyDrag, checkPocket, checkSeam, hitTest, isEmpty, type Drag, type LayoutEdits, type LayoutInfo } from './layoutEdit'

/** Overlay preferences outlive the component: it remounts whenever a run replaces the result. */
const usePrefs = create<{ overlay: boolean; opacity: number; set: (p: Partial<{ overlay: boolean; opacity: number }>) => void }>()(
  persist((set) => ({ overlay: true, opacity: 0.6, set: (p) => set(p) }), { name: 'studio.imageview' }),
)

/** Lets the tile layout be edited by hand: the plate's seams and connector blocks are drawn over the preview. */
export interface LayoutProps { info: LayoutInfo; edits: LayoutEdits; onCommit: (e: LayoutEdits) => void; onReset: () => void }
interface Props { src: string; under?: string; kind: 'image' | 'mask'; layout?: LayoutProps }

/** Zoomable, pannable raster view. A mask can be laid over the image it was derived from. */
export default function ImageView({ src, under, kind, layout }: Props) {
  const [view, setView] = useState({ k: 1, x: 0, y: 0 })
  const { overlay, opacity, set: setPrefs } = usePrefs()
  const drag = useRef<{ x: number; y: number } | null>(null)
  const host = useRef<HTMLDivElement>(null)
  const grab = useRef<Drag | null>(null)
  const [live, setLive] = useState<(Drag & { why?: string }) | null>(null)
  const [note, setNote] = useState('')
  const [cursor, setCursor] = useState('')
  const noteTimer = useRef<number>()

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
    if (layout) drawLayout(g, layout.info, bw, bh, view.k, live)
  }, [imgs, box, view, opacity, showUnder, kind, layout, live])

  /** Plate millimetres under a pointer, and the scale (px per mm at zoom 1). */
  const plateAt = (e: React.PointerEvent) => {
    const top = imgs.top, L = layout?.info
    if (!top || !L) return null
    const r = host.current!.getBoundingClientRect()
    const bw = Math.min(box.w * 0.9, 900), bh = bw * top.naturalHeight / top.naturalWidth
    const lx = (e.clientX - r.left - box.w / 2 - view.x) / view.k, ly = (e.clientY - r.top - box.h / 2 - view.y) / view.k
    return { x: ((lx + bw / 2) / bw) * L.width, y: L.height - ((ly + bh / 2) / bh) * L.height, mm: bw / L.width * view.k }
  }
  const say = (t: string) => {
    setNote(t)
    window.clearTimeout(noteTimer.current)
    noteTimer.current = window.setTimeout(() => setNote(''), 7000)
  }
  const down = (e: React.PointerEvent) => {
    const p = plateAt(e)
    const h = p && layout ? hitTest(layout.info, p.x, p.y, 7 / p.mm) : null
    if (h) {
      grab.current = h.kind === 'seam'
        ? { kind: 'seam', axis: h.axis, i: h.i, v: (h.axis === 0 ? layout!.info.xs : layout!.info.ys)[h.i] }
        : { kind: 'pocket', key: h.key, u: layout!.info.pocket_list.find((q) => q.key === h.key)!.u }
      setLive(grab.current)
    } else drag.current = { x: e.clientX - view.x, y: e.clientY - view.y }
    ;(e.target as Element).setPointerCapture?.(e.pointerId)
  }
  const move = (e: React.PointerEvent) => {
    const g = grab.current, p = plateAt(e)
    if (g && p && layout) {
      if (g.kind === 'seam') {
        const c = checkSeam(layout.info, g.axis, g.i, g.axis === 0 ? p.x : p.y)
        setLive({ ...g, v: c.v, why: c.why })
      } else {
        const q = layout.info.pocket_list.find((x) => x.key === g.key)!
        const c = checkPocket(layout.info, g.key, q.axis === 0 ? p.y : p.x)
        setLive({ ...g, u: c.u, why: c.why })
      }
      return
    }
    const d = drag.current
    if (d) setView((v) => ({ ...v, x: e.clientX - d.x, y: e.clientY - d.y }))
    else if (p && layout) {
      const h = hitTest(layout.info, p.x, p.y, 7 / p.mm)
      setCursor(!h ? '' : h.kind === 'pocket' ? 'grab' : h.axis === 0 ? 'col-resize' : 'row-resize')
    }
  }
  const up = () => {
    drag.current = null
    const l = live
    grab.current = null
    setLive(null)
    if (!l || !layout) return
    if (l.why) say(`Layout edit not applied: ${l.why}.`)
    else layout.onCommit(applyDrag(layout.info, layout.edits, l))
  }
  return (
    <div className="imgview">
      <div
        ref={host} className="imgstage" onWheel={onWheel} onDoubleClick={() => setView({ k: 1, x: 0, y: 0 })}
        style={cursor ? { cursor } : undefined}
        onPointerDown={down} onPointerMove={move} onPointerUp={up}
        onPointerCancel={() => { drag.current = null; grab.current = null; setLive(null) }}
      >
        <canvas ref={canvas} className="imgcanvas" />
      </div>
      <div className="imgbar">
        <span>{Math.round(view.k * 100)}% · wheel zoom, drag pan, double-click reset{layout && ' · drag a seam or block to edit the layout'}</span>
        {layout && !isEmpty(layout.edits) && <button onClick={layout.onReset} title="Go back to the automatic layout">Reset layout</button>}
        {layout && layout.info.edits_ignored > 0 && <span className="warn">{layout.info.edits_ignored} edit(s) no longer fit and were ignored</span>}
        <span role="status" aria-live="polite" className="layout-note">{live?.why ? `Can't drop here: ${live.why}.` : note}</span>
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

const RED = '#ff3b30', ACCENT = '#ffb020'

/** Seams as grabbable lines with a grip, and the connector blocks as outlines. A drag in progress is drawn where it would land, red if it can't. */
function drawLayout(g: CanvasRenderingContext2D, L: LayoutInfo, bw: number, bh: number, k: number, live: (Drag & { why?: string }) | null) {
  const px = bw / L.width
  const X = (x: number) => x * px - bw / 2, Y = (y: number) => (L.height - y) * px - bh / 2
  const colour = live?.why ? RED : ACCENT
  g.save()
  g.lineWidth = 2 / k
  const seam = (axis: 0 | 1, v: number, c: string) => {
    g.strokeStyle = c; g.fillStyle = c
    g.setLineDash([6 / k, 4 / k])
    g.beginPath()
    if (axis === 0) { g.moveTo(X(v), -bh / 2); g.lineTo(X(v), bh / 2) } else { g.moveTo(-bw / 2, Y(v)); g.lineTo(bw / 2, Y(v)) }
    g.stroke()
    g.setLineDash([])
    const [cx, cy] = axis === 0 ? [X(v), 0] : [0, Y(v)]
    g.beginPath(); g.arc(cx, cy, 6 / k, 0, Math.PI * 2); g.fill()
  }
  const moving = live?.kind === 'seam' ? live : null
  const gridX = L.xs.map((v, i) => (moving && moving.axis === 0 && moving.i === i ? moving.v : v))
  const gridY = L.ys.map((v, i) => (moving && moving.axis === 1 && moving.i === i ? moving.v : v))
  gridX.forEach((v, i) => { if (i > 0 && i < gridX.length - 1) seam(0, v, moving?.axis === 0 && moving.i === i ? colour : ACCENT) })
  gridY.forEach((v, i) => { if (i > 0 && i < gridY.length - 1) seam(1, v, moving?.axis === 1 && moving.i === i ? colour : ACCENT) })
  for (const p of L.pocket_list) {
    let [x0, y0, x1, y1] = p.block
    let c = ACCENT
    if (moving && p.axis === moving.axis) {
      const d = moving.v - (p.axis === 0 ? L.xs : L.ys)[p.seam]
      if (p.axis === 0) { x0 += d; x1 += d } else { y0 += d; y1 += d }
      c = colour
    }
    if (live?.kind === 'pocket' && live.key === p.key) {
      const d = live.u - p.u
      if (p.axis === 0) { y0 += d; y1 += d } else { x0 += d; x1 += d }
      c = colour
    }
    g.strokeStyle = c; g.fillStyle = c + '33'
    g.beginPath(); g.rect(X(x0), Y(y1), (x1 - x0) * px, (y1 - y0) * px); g.fill(); g.stroke()
  }
  g.restore()
}
