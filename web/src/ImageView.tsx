import { useEffect, useRef, useState } from 'react'
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { applyDrag, checkPocket, checkSeam, hitTest, isEmpty, type Drag, type LayoutEdits, type LayoutInfo } from './layoutEdit'
import { logAction, useStudio } from './store'
import Tooltip from './Tooltip'
import messages from './messages'

/** Overlay preferences outlive the component: it remounts whenever a run replaces the result. */
const usePrefs = create<{ overlay: boolean; opacity: number; snap: boolean; set: (p: Partial<{ overlay: boolean; opacity: number; snap: boolean }>) => void }>()(
  persist((set) => ({ overlay: true, opacity: 0.6, snap: true, set: (p) => set(p) }), { name: 'studio.imageview' }),
)

/** Lets the tile layout be edited by hand: the plate's seams and connector blocks are drawn over the preview. */
export interface LayoutProps { info: LayoutInfo; edits: LayoutEdits; onCommit: (e: LayoutEdits) => void; onReset: () => void }
/** Lets Collage layers be dragged: their rectangles (canvas pixels, y down) are drawn over the preview. */
export interface LayerBox { input: string; x: number; y: number; w: number; h: number; src?: string }
export interface LayersProps { width: number; height: number; boxes: LayerBox[]; preview: boolean; editing: boolean; onEditing: (on: boolean) => void; selected?: string; onSelect: (input: string) => void; onMove: (input: string, dx: number, dy: number, now?: boolean) => void }
interface Props { src: string; under?: string; kind: 'image' | 'mask'; layout?: LayoutProps; layers?: LayersProps }

/** Zoomable, pannable raster view. A mask can be laid over the image it was derived from. */
export default function ImageView({ src, under, kind, layout, layers }: Props) {
  const [view, setView] = useState({ k: 1, x: 0, y: 0 })
  const loggedView = useRef(view)
  useEffect(() => {
    if (loggedView.current === view) return
    const timer = setTimeout(() => {
      loggedView.current = view
      logAction(`2D view: zoom ${view.k.toFixed(2)}, position ${view.x.toFixed(0)}, ${view.y.toFixed(0)}`, useStudio.getState().bench ?? '')
    }, 250)
    return () => clearTimeout(timer)
  }, [view])
  const { overlay, opacity, snap, set: setPrefs } = usePrefs()
  const drag = useRef<{ x: number; y: number } | null>(null)
  const host = useRef<HTMLDivElement>(null)
  const grab = useRef<Drag | null>(null)
  const [live, setLive] = useState<(Drag & { why?: string }) | null>(null)
  const lgrab = useRef<{ input: string; x: number; y: number } | null>(null)
  const [lmove, setLmove] = useState<{ input: string; dx: number; dy: number; gx?: number; gy?: number } | null>(null)
  const [limgs, setLimgs] = useState<Record<string, HTMLImageElement>>({})
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
      i.onerror = () => {
        if (alive) logAction('2D preview could not load its image', useStudio.getState().bench ?? '', 'error')
        res(undefined)
      }
      i.src = u
    })
    Promise.all([load(src), showUnder ? load(under) : Promise.resolve(undefined)]).then(([top, u]) => alive && setImgs({ top, under: u }))
    return () => { alive = false }
  }, [src, under, showUnder])

  // the layers' own pictures, so a dragged layer can be seen moving
  const srcs = (layers?.boxes ?? []).map((b) => b.src ?? '').join('|')
  useEffect(() => {
    let alive = true
    for (const u of srcs.split('|').filter(Boolean)) {
      const i = new Image()
      i.onload = () => alive && setLimgs((m) => (m[u] === i ? m : { ...m, [u]: i }))
      i.src = u
    }
    return () => { alive = false }
  }, [srcs])

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
    const moving = !!layers && (!!lmove || layers.preview) && layers.boxes.every((b) => !b.src || limgs[b.src])
    if (moving) {
      g.save()
      g.beginPath(); g.rect(-bw / 2, -bh / 2, bw, bh); g.clip()
      g.fillStyle = '#fff'
      g.fillRect(-bw / 2, -bh / 2, bw, bh)
      g.globalCompositeOperation = 'multiply'
      const k = bw / layers!.width
      for (const b of layers!.boxes) {
        const im = b.src && limgs[b.src]
        const d = lmove?.input === b.input ? lmove : null
        if (im) g.drawImage(im, (b.x + (d?.dx ?? 0)) * k - bw / 2, (b.y + (d?.dy ?? 0)) * k - bh / 2, b.w * k, b.h * k)
      }
      g.restore()
    } else g.drawImage(top, -bw / 2, -bh / 2, bw, bh)
    if (layout) drawLayout(g, layout.info, bw, bh, view.k, live)
    if (layers) drawLayers(g, layers, bw, bh, view.k, lmove)
  }, [imgs, box, view, opacity, showUnder, kind, layout, live, layers, lmove, limgs])

  /** Canvas pixels under a pointer, when layers are shown. */
  const canvasAt = (e: React.PointerEvent) => {
    const top = imgs.top
    if (!top || !layers) return null
    const r = host.current!.getBoundingClientRect()
    const bw = Math.min(box.w * 0.9, 900), bh = bw * top.naturalHeight / top.naturalWidth
    const lx = (e.clientX - r.left - box.w / 2 - view.x) / view.k, ly = (e.clientY - r.top - box.h / 2 - view.y) / view.k
    return { x: ((lx + bw / 2) / bw) * layers.width, y: ((ly + bh / 2) / bh) * layers.height }
  }
  const layerAt = (p: { x: number; y: number }) => [...(layers?.boxes ?? [])].reverse().find((b) => p.x >= b.x && p.x <= b.x + b.w && p.y >= b.y && p.y <= b.y + b.h)

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
    logAction(t, useStudio.getState().bench ?? '', 'error')
    window.clearTimeout(noteTimer.current)
    noteTimer.current = window.setTimeout(() => setNote(''), 7000)
  }
  const down = (e: React.PointerEvent) => {
    const cp = canvasAt(e), hit = cp && layerAt(cp)
    if (layers?.editing && !hit) return
    if (cp && hit && layers?.editing) {
      lgrab.current = { input: hit.input, x: cp.x, y: cp.y }
      layers!.onSelect(hit.input)
      setLmove({ input: hit.input, dx: 0, dy: 0 })
      ;(e.target as Element).setPointerCapture?.(e.pointerId)
      return
    }
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
    const lg = lgrab.current, cp = canvasAt(e)
    if (lg && cp) {
      const b = layers!.boxes.find((x) => x.input === lg.input)!
      let dx = Math.round(cp.x - lg.x), dy = Math.round(cp.y - lg.y), gx: number | undefined, gy: number | undefined
      if (snap && !e.altKey) {
        const px = Math.min(box.w * 0.9, 900) / layers!.width * view.k
        const s = snapTo(b, dx, dy, layers!, 7 / px)
        dx = s.dx; dy = s.dy; gx = s.gx; gy = s.gy
      }
      setLmove({ input: lg.input, dx, dy, gx, gy })
      return
    }
    if (cp && !drag.current) setCursor(layers?.editing ? (layerAt(cp) ? 'move' : 'default') : '')
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
  const onKey = (e: React.KeyboardEvent) => {
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key]
    if (!d || !layers?.editing || !layers.selected) return
    e.preventDefault()
    const k = e.shiftKey ? 10 : 1
    layers.onMove(layers.selected, d[0] * k, d[1] * k)
  }
  const up = () => {
    const m = lmove
    if (lgrab.current) {
      lgrab.current = null
      setLmove(null)
      if (m && layers && (m.dx || m.dy)) layers.onMove(m.input, m.dx, m.dy, true)
      return
    }
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
        tabIndex={layers?.editing ? 0 : undefined} onKeyDown={onKey}
        onPointerDown={down} onPointerMove={move} onPointerUp={up}
        onPointerCancel={() => { lgrab.current = null; setLmove(null); drag.current = null; grab.current = null; setLive(null) }}
      >
        <canvas ref={canvas} className="imgcanvas" />
      </div>
      <div className="imgbar">
        <span>{Math.round(view.k * 100)}% · wheel zoom, drag pan, double-click reset{layout && ' · drag a seam or block to edit the layout'}{layers && (layers.editing ? ' · editing: drag a layer, arrow keys nudge (Shift ×10), hold Alt to skip snapping' : '')}</span>
        {layers && (
          <>
            <Tooltip content={layers.editing ? 'Stop editing layers' : messages.actions.editLayers}><button className={`iconbtn${layers.editing ? ' on' : ''}`} aria-pressed={layers.editing} aria-label="Edit layers" onClick={() => layers.onEditing(!layers.editing)}>
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3" /></svg>
            </button></Tooltip>
            <Tooltip content={messages.actions.snapLayers}><button className={`iconbtn${snap ? ' on' : ''}`} aria-pressed={snap} aria-label="Snap" onClick={() => setPrefs({ snap: !snap })}>
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M6 3v9a6 6 0 0 0 12 0V3M6 8h4M14 8h4" /></svg>
            </button></Tooltip>
          </>
        )}
        {layout && !isEmpty(layout.edits) && <Tooltip content={messages.actions.resetLayout}><button onClick={layout.onReset}>Reset layout</button></Tooltip>}
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

/** Layer rectangles; the one being dragged is drawn where it would land. */
function drawLayers(g: CanvasRenderingContext2D, L: LayersProps, bw: number, bh: number, k: number, live: { input: string; dx: number; dy: number; gx?: number; gy?: number } | null) {
  const px = bw / L.width
  g.save()
  g.lineWidth = 1.5 / k
  for (const b of L.boxes) {
    const on = b.input === L.selected, d = live?.input === b.input ? live : null
    g.strokeStyle = on ? ACCENT : 'rgba(255,176,32,0.55)'
    g.setLineDash(on ? [] : [5 / k, 4 / k])
    g.strokeRect((b.x + (d?.dx ?? 0)) * px - bw / 2, (b.y + (d?.dy ?? 0)) * px - bh / 2, b.w * px, b.h * px)
  }
  if (live?.gx !== undefined || live?.gy !== undefined) {
    g.strokeStyle = '#ff3b30'
    g.setLineDash([3 / k, 3 / k])
    g.beginPath()
    if (live.gx !== undefined) { g.moveTo(live.gx * px - bw / 2, -bh / 2); g.lineTo(live.gx * px - bw / 2, bh / 2) }
    if (live.gy !== undefined) { g.moveTo(-bw / 2, live.gy * px - bh / 2); g.lineTo(bw / 2, live.gy * px - bh / 2) }
    g.stroke()
  }
  g.restore()
}

/** Pulls a dragged layer onto the edges and centres of the other layers and of the canvas when one is within `tol` canvas pixels. */
function snapTo(b: LayerBox, dx: number, dy: number, L: LayersProps, tol: number) {
  const others = L.boxes.filter((o) => o.input !== b.input)
  const xs = [0, L.width / 2, L.width, ...others.flatMap((o) => [o.x, o.x + o.w / 2, o.x + o.w])]
  const ys = [0, L.height / 2, L.height, ...others.flatMap((o) => [o.y, o.y + o.h / 2, o.y + o.h])]
  const pull = (lo: number, size: number, targets: number[]) => {
    let best: { d: number; at: number } | null = null
    for (const mine of [lo, lo + size / 2, lo + size])
      for (const t of targets) {
        const d = t - mine
        if (Math.abs(d) <= tol && (!best || Math.abs(d) < Math.abs(best.d))) best = { d, at: t }
      }
    return best
  }
  const sx = pull(b.x + dx, b.w, xs), sy = pull(b.y + dy, b.h, ys)
  return { dx: Math.round(dx + (sx?.d ?? 0)), dy: Math.round(dy + (sy?.d ?? 0)), gx: sx?.at, gy: sy?.at }
}
