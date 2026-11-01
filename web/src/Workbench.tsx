import { STAGE_LABEL } from './ports'
import { Suspense, lazy, useEffect, useRef, useState } from 'react'
import { api, artifactUrl, partUrl, type Kind } from './api'
import Console from './Console'
import ImageView, { type LayersProps } from './ImageView'
import ParamForm from './ParamForm'
import SourceActions from './SourceActions'
import Presets from './Presets'
import { NodeActions, DeleteNode, downloadFor, runSummary } from './NodeActions'
import Findings, { useNodeFindings } from './Findings'
import ImageInfoPanel from './ImageInfo'
import { useStudio, type StudioNode } from './store'
import { parseEdits, stringify, type LayoutInfo } from './layoutEdit'
const Viewer3D = lazy(() => import('./Viewer3D'))

const STATS = ['islands_found', 'bridges_added', 'islands_remaining', 'watertight', 'triangles', 'overhang_area_mm2', 'layers', 'size_mm']

/** Full-size, live preview of one node with its parameters beside it. Double-click a node to open. */
export default function Workbench() {
  const id = useStudio((s) => s.bench)
  const node = useStudio((s) => s.nodes.find((n) => n.id === s.bench))
  const types = useStudio((s) => s.types)
  const edges = useStudio((s) => s.edges)
  const nodes = useStudio((s) => s.nodes)
  const runs = useStudio((s) => s.runs)
  const busy = useStudio((s) => s.busy)
  const open = useStudio((s) => s.openBench)
  const setParam = useStudio((s) => s.setParam)
  const [view, setView] = useState('')
  const [mode, setMode] = useState<'plate' | 'tiles'>('plate')
  const [info, setInfo] = useState<Record<string, any> | null>(null)
  const [layout, setLayout] = useState<Record<string, any> | null>(null)
  const [picked, setPicked] = useState<string>()
  const [editing, setEditing] = useState(false)
  // hand moves wait here until the user stops interacting, so the engine is not asked again for every step
  const [draft, setDraft] = useState<Record<string, [number, number]> | null>(null)
  const draftRef = useRef<Record<string, [number, number]> | null>(null)
  const timer = useRef<number>()
  const commitRef = useRef<() => void>(() => {})

  const problems = useNodeFindings(id).filter((b) => b.level !== 'info').length
  const t = node && types[node.data.type]
  const run = id ? runs[id] : undefined
  const views = (() => {
    if (!node || !t) return []
    const v: { id: string; label: string; key?: string; out: string; kind: string }[] = []
    for (const o of t.outputs)
      if (run?.key && run.outputs?.[o.name]?.available) v.push({ id: `out:${o.name}`, label: o.name, key: run.key, out: o.name, kind: o.type })
    for (const e of edges.filter((x) => x.target === node.id)) {
      const up = runs[e.source], ut = types[useStudio.getState().nodes.find((n) => n.id === e.source)?.data.type ?? '']
      const o = ut?.outputs.find((x) => x.name === e.sourceHandle)
      if (up?.key && o && up.outputs?.[o.name]?.available) v.push({ id: `in:${e.targetHandle}`, label: `in · ${e.targetHandle}`, key: up.key, out: o.name, kind: o.type })
    }
    return v
  })()
  const cur = views.find((x) => x.id === view) ?? views[0]

  useEffect(() => {
    setInfo(null)
    if (cur?.key) api.info(cur.key, cur.out).then(setInfo).catch(() => setInfo(null))
  }, [cur?.key, cur?.out])

  useEffect(() => {
    setLayout(null)
    if (t?.id === 'canvas' && run?.key) api.info(run.key, 'layout').then(setLayout).catch(() => setLayout(null))
  }, [t?.id, run?.key])

  useEffect(() => {
    if (layout) { draftRef.current = null; setDraft(null) }
  }, [layout])
  useEffect(() => () => commitRef.current(), [])

  if (!id || !node || !t) return null
  const moves = (): Record<string, [number, number]> => {
    try { const v = JSON.parse(String(node.data.params.layer_moves || '{}')); return v && typeof v === 'object' ? v : {} } catch { return {} }
  }
  const commit = () => {
    window.clearTimeout(timer.current)
    const d = draftRef.current
    if (!d) return
    const kept = Object.fromEntries(Object.entries(d).filter(([, v]) => v[0] || v[1]))
    const text = Object.keys(kept).length ? JSON.stringify(kept) : ''
    if (text === String(node.data.params.layer_moves || '')) { draftRef.current = null; setDraft(null) } else setParam(node.id, 'layer_moves', text)
  }
  commitRef.current = commit
  const moveLayer = (input: string, dx: number, dy: number, now = false) => {
    const base = draftRef.current ?? moves(), [ox, oy] = base[input] ?? [0, 0]
    const next = { ...base, [input]: [ox + dx, oy + dy] as [number, number] }
    draftRef.current = next
    setDraft(next)
    window.clearTimeout(timer.current)
    if (now) commit()
    else timer.current = window.setTimeout(() => commitRef.current(), 700)
  }
  const committed = moves()
  const shown: Record<string, any> | null = layout && {
    ...layout,
    layers: (layout.layers as any[]).map((l) => {
      const c = committed[l.input] ?? [0, 0], d = draft?.[l.input] ?? c
      const o = layerOrigin(nodes, edges, node.id, l.input)
      const key = o && runs[o.n.id]?.key
      const kind = o && types[o.n.data.type]?.outputs.find((x) => x.name === o.out)?.type
      const src = key && (kind === 'image' || kind === 'mask' || kind === 'shapes') ? artifactUrl(key, o.out) : undefined
      return { ...l, x: l.x + d[0] - c[0], y: l.y + d[1] - c[1], src }
    }),
  }
  const layerProps: LayersProps | undefined = t.id === 'canvas' && shown?.layers?.length && cur?.id.startsWith('out:')
    ? { width: shown.width, height: shown.height, boxes: shown.layers, preview: draft !== null, editing, onEditing: setEditing, selected: picked, onSelect: setPicked, onMove: moveLayer }
    : undefined
  const imageFor = (key?: string) => {
    const e = edges.find((x) => x.target === node.id && ['image', 'size'].includes(x.targetHandle ?? ''))
    const up = e && runs[e.source]
    const ut = e && types[useStudio.getState().nodes.find((n) => n.id === e.source)?.data.type ?? '']
    const o = ut?.outputs.find((x) => x.name === e!.sourceHandle)
    return key && up?.key && o?.type === 'image' ? artifactUrl(up.key, o.name) : undefined
  }

  const solidParts = () => {
    if (!cur?.key) return []
    if (mode === 'tiles' && info?.tiles?.length) return info.tiles.map((n: string) => ({ name: n, url: partUrl(cur.key!, cur.out, n) }))
    return [{ name: 'plate', url: artifactUrl(cur.key, cur.out) }]
  }
  const partsList = () => (info?.files ? Object.keys(info.files).map((n) => ({ name: n, url: partUrl(cur!.key!, cur!.out, n) })) : [])

  return (
    <div className="bench">
      <div className="bench-head">
        <button className="close" onClick={() => open(null)} aria-label="Close preview" title="Close (Esc)">✕</button>
        <h2>{node.data.title || t.label} <small className={`tag stage-${t.stage}`}>{STAGE_LABEL[t.stage]}</small>{problems > 0 && <span className="tb-badge warn" title="Findings on this node">{problems}</span>}</h2>
        <div className="tabs">
          {views.map((x) => <button key={x.id} className={x.id === cur?.id ? 'on' : ''} onClick={() => setView(x.id)}>{x.label}</button>)}
        </div>
        {cur?.kind === 'solid' && (info?.tiles?.length ?? 0) > 0 && (
          <div className="tabs">
            <button className={mode === 'plate' ? 'on' : ''} onClick={() => setMode('plate')}>whole plate</button>
            <button className={mode === 'tiles' ? 'on' : ''} onClick={() => setMode('tiles')}>tiles ({info?.tiles?.length})</button>
          </div>
        )}
        <span className="spacer" />
        <span className="mut">{runSummary(run, busy)}</span>
      </div>
      {run?.state === 'running' && <div className="bar"><i style={{ width: `${(run.progress ?? 0) * 100}%` }} /></div>}
      <div className="bench-body">
        <aside className="bench-params">
          <Findings id={node.id} />
          <Presets key={node.id} node={node} />
          {run?.state === 'error' && <div className="err">{run.error}</div>}
          <ParamForm node={node} type={t} before={<>
            <NodeActions id={node.id} download={downloadFor(cur?.kind as Kind, cur?.key, cur?.out, Object.keys(info?.files ?? {}).length)} />
            {t.id === 'source' && <SourceActions id={node.id} imageId={String(node.data.params.image_id)} title={node.data.title} />}
          </>} />
          {t.id === 'source' && <ImageInfoPanel imageId={String(node.data.params.image_id)} resolution={Number(node.data.params.resolution ?? 1000)} />}
          <div className="panel-foot"><DeleteNode id={node.id} /></div>
        </aside>
        <section className="bench-view">
          {!cur && <div className="mut center">{run?.state === 'error' ? run.error : 'nothing computed yet'}</div>}
          {cur && (cur.kind === 'image' || cur.kind === 'mask' || cur.kind === 'shapes' || cur.kind === 'tiles' || cur.kind === 'layer' || cur.kind === 'any') && cur.key && (
            <ImageView key={`${cur.id}`} src={artifactUrl(cur.key, cur.out)} kind={cur.kind === 'mask' ? 'mask' : 'image'}
              under={cur.kind === 'mask' && cur.id.startsWith('out:') ? imageFor(cur.key) : undefined}
              layout={cur.kind === 'tiles' && cur.id.startsWith('out:') && node.data.type === 'tile_layout' && info?.kind === 'tiles' && info.pocket_list
                ? { info: info as LayoutInfo, edits: parseEdits(node.data.params.edits),
                    onCommit: (e) => setParam(node.id, 'edits', stringify(e)), onReset: () => setParam(node.id, 'edits', '') }
                : undefined}
              layers={layerProps} />
          )}
          <Suspense fallback={<div className="mut center">loading 3D viewer…</div>}>
            {cur?.kind === 'solid' && <Viewer3D parts={solidParts()} />}
            {cur?.kind === 'parts' && <Viewer3D parts={partsList()} plate={info?.meta?.plate} />}
          </Suspense>
        </section>
        {t.id === 'canvas' && <LayersPanel layout={shown} picked={picked} onPick={setPicked} onMove={moveLayer} moved={Object.keys(draft ?? committed).length > 0} onReset={() => { draftRef.current = null; setDraft(null); window.clearTimeout(timer.current); setParam(node.id, 'layer_moves', '') }} />}
      </div>
      <div className="bench-foot">
        <div className="stats">
          {info?.stats && STATS.filter((k) => k in info.stats).map((k) => <span key={k}>{k.replace(/_/g, ' ')}: <b>{JSON.stringify(info.stats[k])}</b></span>)}
          {info?.kind === 'tiles' && typeof info.paint_under_blocks_mm2 === 'number' && <span>paint under blocks: <b>{info.paint_under_blocks_mm2.toFixed(0)} mm²</b>{info.paint_mm2 > 0 && <> ({((100 * info.paint_under_blocks_mm2) / info.paint_mm2).toFixed(1)}%)</>}</span>}
          {info?.coverage !== undefined && <span>{info.width}×{info.height}px · <b>{(info.coverage * 100).toFixed(1)}%</b> paint</span>}
          {info?.width !== undefined && info.coverage === undefined && <span>{info.width}×{info.height}px</span>}
        </div>
        <Console nodeId={id} height={130} />
      </div>
    </div>
  )
}

/** The layers of a Collage: where each one sits on the canvas and where it comes from. */
function LayersPanel({ layout, picked, onPick, onMove, moved, onReset }: {
  layout: Record<string, any> | null; picked?: string; onPick: (i: string) => void
  onMove: (input: string, dx: number, dy: number, now?: boolean) => void; moved: boolean; onReset: () => void
}) {
  const nodes = useStudio((s) => s.nodes)
  const edges = useStudio((s) => s.edges)
  const runs = useStudio((s) => s.runs)
  const types = useStudio((s) => s.types)
  const open = useStudio((s) => s.openBench)
  const layers: any[] = layout?.layers ?? []
  const canvasId = useStudio((s) => s.bench) ?? ''
  const origin = (input: string) => layerOrigin(nodes, edges, canvasId, input)
  return (
    <aside className="bench-layers" aria-label="Layers">
      <h4>Layers{layout && <small> · canvas {layout.width}×{layout.height}px</small>}</h4>
      {layers.length === 0 && <div className="mut">no layers yet</div>}
      {layers.length > 0 && <div className="mut">Drag a layer in the preview, or type its position.</div>}
      {moved && <button onClick={onReset} title="Forget the hand moves">Reset moves</button>}
      {layers.map((l, k) => {
        const o = origin(l.input)
        const key = o && runs[o.n.id]?.key
        const kind = o && types[o.n.data.type]?.outputs.find((x) => x.name === o.out)?.type
        return (
          <div className={`layer-row${picked === l.input ? ' on' : ''}`} key={l.input} onClick={() => onPick(l.input)}>
            <span className="layer-thumb">{key && (kind === 'image' || kind === 'mask' || kind === 'shapes') && <img src={artifactUrl(key, o.out, 48)} alt="" />}</span>
            <div>
              <b>{k + 1}. {o?.n.data.title ?? l.input}</b>
              <small>{l.kind} · {l.w}×{l.h}px{(l.scale_x !== 1 || l.scale_y !== 1) && <> · scale {Math.round(l.scale_x * 100)}% × {Math.round(l.scale_y * 100)}%</>}</small>
              <span className="layer-xy">
                <label>x <input type="number" aria-label={`${l.input} x`} value={l.x} onChange={(e) => e.target.value !== '' && onMove(l.input, +e.target.value - l.x, 0)} /></label>
                <label>y <input type="number" aria-label={`${l.input} y`} value={l.y} onChange={(e) => e.target.value !== '' && onMove(l.input, 0, +e.target.value - l.y)} /></label>
              </span>
              <span className="layer-actions">
                {o?.place && <button onClick={() => open(o.place!)}>move / scale</button>}
                {o && <button onClick={() => open(o.n.id)}>open source</button>}
              </span>
            </div>
          </div>
        )
      })}
    </aside>
  )
}

/** The node a Collage layer comes from, looking through Place nodes, and the first Place node on the way. */
function layerOrigin(nodes: StudioNode[], edges: { source: string; sourceHandle?: string | null; target: string; targetHandle?: string | null }[], canvasId: string, input: string) {
  let e = edges.find((x) => x.target === canvasId && x.targetHandle === input)
  let place: string | undefined
  while (e) {
    const n = nodes.find((x) => x.id === e!.source)
    if (!n) return undefined
    if (n.data.type !== 'place') return { n, out: e.sourceHandle ?? 'image', place }
    place ??= n.id
    e = edges.find((x) => x.target === n.id && x.targetHandle === 'item')
  }
  return undefined
}
