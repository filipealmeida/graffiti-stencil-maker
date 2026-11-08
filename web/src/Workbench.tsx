import { STAGE_LABEL } from './ports'
import { Suspense, lazy, useEffect, useId, useRef, useState } from 'react'
import { api, artifactUrl, partUrl, type Kind } from './api'
import Console from './Console'
import ImageView, { type LayersProps } from './ImageView'
import ParamForm from './ParamForm'
import SourceActions from './SourceActions'
import Presets from './Presets'
import TilesPanel from './TilesPanel'
import CutsPanel from './CutsPanel'
import { NodeActions, DeleteNode, downloadFor, runSummary } from './NodeActions'
import Findings, { useNodeFindings } from './Findings'
import ImageInfoPanel from './ImageInfo'
import MeshTools, { MeshProperties, useModelInspection } from './MeshTools'
import { useStudio, type StudioNode } from './store'
import { parseEdits, stringify, type LayoutInfo } from './layoutEdit'
import useDialogFocus from './useDialogFocus'
import CatalogueLabel from './CatalogueLabel'
const Viewer3D = lazy(() => import('./Viewer3D'))

const STATS = ['islands_found', 'bridges_added', 'islands_remaining', 'watertight', 'triangles', 'overhang_area_mm2', 'layers', 'size_mm']

/** Full-size, live preview of one node with its parameters beside it. Double-click a node to open. */
export default function Workbench() {
  const id = useStudio((s) => s.bench)
  const node = useStudio((s) => s.nodes.find((n) => n.id === s.bench))
  const model = useModelInspection(node, true)
  const types = useStudio((s) => s.types)
  const edges = useStudio((s) => s.edges)
  const nodes = useStudio((s) => s.nodes)
  const runs = useStudio((s) => s.runs)
  const busy = useStudio((s) => s.busy)
  const open = useStudio((s) => s.openBench)
  const setParam = useStudio((s) => s.setParam)
  const [view, setView] = useState('')
  const [mode, setMode] = useState<'plate' | 'tiles'>('plate')
  const [meshPanel, setMeshPanel] = useState<'controls' | 'information'>('controls')
  const [showMeshInfo, setShowMeshInfo] = useState(true)
  const meshInfoId = useId()
  const meshControlsId = useId()
  const [info, setInfo] = useState<Record<string, any> | null>(null)
  const [layout, setLayout] = useState<Record<string, any> | null>(null)
  const [cuts, setCuts] = useState<Record<string, any> | null>(null)
  const [hideCut, setHideCut] = useState<Set<string>>(new Set())
  const [cutsUrl, setCutsUrl] = useState('')
  const cutsUrlRef = useRef('')
  const [picked, setPicked] = useState<string>()
  const [editing, setEditing] = useState(false)
  // hand moves wait here until the user stops interacting, so the engine is not asked again for every step
  const [draft, setDraft] = useState<Record<string, [number, number]> | null>(null)
  const dialog = useRef<HTMLDivElement>(null)
  useDialogFocus(dialog, !!id)
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
    // the previous layout stays until the new one arrives, so the layers do not jump while the engine works
    if (t?.id !== 'canvas') setLayout(null)
    else if (run?.key) api.info(run.key, 'layout').then(setLayout).catch(() => setLayout(null))
  }, [t?.id, run?.key])

  useEffect(() => {
    if (t?.id !== 'cut_sheet') setCuts(null)
    else if (run?.key) api.info(run.key, 'cuts').then(setCuts).catch(() => setCuts(null))
  }, [t?.id, run?.key])

  useEffect(() => { setHideCut(new Set()) }, [id])

  // the cut sheet preview is drawn by the engine on demand, with only the ticked parts, so the view stays light
  useEffect(() => {
    const drop = () => { if (cutsUrlRef.current) { URL.revokeObjectURL(cutsUrlRef.current); cutsUrlRef.current = '' } }
    if (t?.id !== 'cut_sheet' || !run?.key || !cuts?.piece_list) { drop(); setCutsUrl(''); return }
    let alive = true
    const shown = (cuts.piece_list as any[]).map((p) => String(p.name)).filter((n) => !hideCut.has(n))
    api.part(run.key, 'cuts', `preview:${shown.join(',')}`).then((b) => {
      if (!alive) return
      drop()
      cutsUrlRef.current = URL.createObjectURL(new Blob([b as BlobPart], { type: 'image/svg+xml' }))
      setCutsUrl(cutsUrlRef.current)
    }).catch(() => { if (alive) setCutsUrl('') })
    return () => { alive = false }
  }, [t?.id, run?.key, cuts, hideCut])

  useEffect(() => () => { if (cutsUrlRef.current) URL.revokeObjectURL(cutsUrlRef.current) }, [])

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
    if (text !== String(node.data.params.layer_moves || '')) setParam(node.id, 'layer_moves', text)
    draftRef.current = null
    setDraft(null)
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
  // each layer is drawn where the user put it: the position the engine used, plus the hand move that has not been applied yet
  let outdated = false
  const shown: Record<string, any> | null = layout && {
    ...layout,
    layers: (layout.layers as any[]).map((l) => {
      const want = (draft ?? committed)[l.input] ?? [0, 0]
      if (want[0] !== (l.move_x ?? 0) || want[1] !== (l.move_y ?? 0)) outdated = true
      const o = layerOrigin(nodes, edges, node.id, l.input)
      const key = o && runs[o.n.id]?.key
      const kind = o && types[o.n.data.type]?.outputs.find((x) => x.name === o.out)?.type
      const src = key && (kind === 'image' || kind === 'mask' || kind === 'shapes') ? artifactUrl(key, o.out) : undefined
      return { ...l, x: l.x - (l.move_x ?? 0) + want[0], y: l.y - (l.move_y ?? 0) + want[1], src }
    }),
  }
  const updating = outdated || run?.state === 'running'
  const layerProps: LayersProps | undefined = t.id === 'canvas' && shown?.layers?.length && cur?.id.startsWith('out:')
    ? { width: shown.width, height: shown.height, boxes: shown.layers, preview: outdated, editing, onEditing: setEditing, selected: picked, onSelect: setPicked, onMove: moveLayer }
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
  // the Cut sheet's own output is shown as the light preview, never as the full drawing
  const cutView = t.id === 'cut_sheet' && cur?.id === 'out:sheet'
  const isModel = t.id === 'mesh_import'
  const actions = <>
    <NodeActions id={node.id} download={isModel && model.snapshot?.info.valid !== true ? null : downloadFor(cur?.kind as Kind, cur?.key, cur?.out, Object.keys(info?.files ?? {}).length)} />
    {t.id === 'source' && <SourceActions id={node.id} imageId={String(node.data.params.image_id)} title={node.data.title} />}
    {isModel && <SourceActions id={node.id} imageId={String(node.data.params.model_id ?? '')} title={node.data.title} model />}
  </>

  return (
    <div ref={dialog} className={`bench${isModel ? ' bench-mesh' : ''}`} data-mesh-panel={meshPanel} data-mesh-info={showMeshInfo} role="dialog" aria-modal="true" aria-label={`${node.data.title || t.label} preview`} tabIndex={-1}>
      <div className="bench-head">
        <button className="close" onClick={() => open(null)} aria-label="Close preview" title="Close (Esc)">✕</button>
        <h2><CatalogueLabel type={t} title={node.data.title} /> <small className={`tag stage-${t.stage}`}>{STAGE_LABEL[t.stage]}</small>{problems > 0 && <span className="tb-badge warn" title="Findings on this node">{problems}</span>}</h2>
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
        {isModel && <button className="mesh-info-toggle" aria-expanded={showMeshInfo} aria-controls={meshInfoId}
          onClick={() => setShowMeshInfo(!showMeshInfo)}>{showMeshInfo ? 'Hide information' : 'Show information'}</button>}
        <span className="mut">{runSummary(run, busy)}</span>
      </div>
      {run?.state === 'running' && <div className="bar"><i style={{ width: `${(run.progress ?? 0) * 100}%` }} /></div>}
      {isModel && <div className="mesh-panel-tabs tabs" role="group" aria-label="Mesh side panel">
        <button aria-pressed={meshPanel === 'controls'} aria-controls={meshControlsId} onClick={() => setMeshPanel('controls')}>Controls</button>
        <button aria-pressed={meshPanel === 'information'} aria-controls={meshInfoId} onClick={() => setMeshPanel('information')}>Information</button>
      </div>}
      <div className="bench-body">
        <aside className="bench-params" id={meshControlsId} aria-label={isModel ? 'Model controls' : undefined}>
          <div className="panel-scroll">
          {isModel && <>{actions}<MeshTools node={node} info={model.snapshot?.info} error={model.error} /></>}
          <Findings id={node.id} />
          <Presets key={node.id} node={node} />
          {run?.state === 'error' && !isModel && <div className="err">{run.error}</div>}
          <ParamForm node={node} type={t} before={isModel ? undefined : actions} />
          {t.id === 'source' && <ImageInfoPanel imageId={String(node.data.params.image_id)} resolution={Number(node.data.params.resolution ?? 1000)} />}
          </div>
          <div className="panel-foot"><DeleteNode id={node.id} /></div>
        </aside>
        <section className="bench-view">
          {!cur && t.id !== 'mesh_import' && <div className="mut center">{run?.state === 'error' ? run.error : 'nothing computed yet'}</div>}
          {cutView && (cutsUrl
            ? <ImageView key="cuts" src={cutsUrl} kind="image" />
            : <div className="mut center">drawing the sheets…</div>)}
          {cur && !cutView && (cur.kind === 'image' || cur.kind === 'mask' || cur.kind === 'shapes' || cur.kind === 'tiles' || cur.kind === 'layer' || cur.kind === 'any') && cur.key && (
            <ImageView key={`${cur.id}`} src={artifactUrl(cur.key, cur.out)} kind={cur.kind === 'mask' ? 'mask' : 'image'}
              under={cur.kind === 'mask' && cur.id.startsWith('out:') ? imageFor(cur.key) : undefined}
              layout={cur.kind === 'tiles' && cur.id.startsWith('out:') && node.data.type === 'tile_layout' && info?.kind === 'tiles' && info.pocket_list
                ? { info: info as LayoutInfo, edits: parseEdits(node.data.params.edits),
                    onCommit: (e) => setParam(node.id, 'edits', stringify(e)), onReset: () => setParam(node.id, 'edits', '') }
                : undefined}
              layers={layerProps} />
          )}
          <Suspense fallback={<div className="mut center">loading 3D viewer…</div>}>
            {t.id === 'mesh_import' && (model.snapshot?.url
              ? <Viewer3D parts={[{ name: 'source mesh', url: model.snapshot.url }]} defects={model.snapshot.info.markers} />
              : <p className="mut center">{model.error || 'Open a model to inspect it.'}</p>)}
            {cur?.kind === 'solid' && t.id !== 'mesh_import' && <Viewer3D parts={solidParts()} />}
            {cur?.kind === 'parts' && <Viewer3D parts={partsList()} plate={info?.meta?.plate} />}
          </Suspense>
        </section>
        {isModel && <aside className="bench-object-info" id={meshInfoId} aria-label="Object information">
          <h3>Object information</h3>
          <MeshProperties node={node} info={model.displayInfo} error={model.error} updating={!model.snapshot} />
          {run?.state === 'error' && <details className="mesh-details"><summary>Node error</summary><p className="err">{run.error}</p></details>}
        </aside>}
        {t.id === 'tile_layout' && info?.kind === 'tiles' && info.tile_list && cur?.key && <TilesPanel info={info} keyOf={cur.key} out={cur.out} />}
        {t.id === 'cut_sheet' && cuts?.piece_list && run?.key && <CutsPanel info={cuts} keyOf={run.key} hidden={hideCut} onHidden={setHideCut} />}
        {t.id === 'canvas' && <LayersPanel updating={updating} layout={shown} picked={picked} onPick={setPicked} onMove={moveLayer} moved={Object.keys(draft ?? committed).length > 0} onReset={() => { draftRef.current = null; setDraft(null); window.clearTimeout(timer.current); setParam(node.id, 'layer_moves', '') }} />}
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
function LayersPanel({ updating, layout, picked, onPick, onMove, moved, onReset }: {
  updating: boolean; layout: Record<string, any> | null; picked?: string; onPick: (i: string) => void
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
    <aside className={`bench-layers${updating ? ' updating' : ''}`} aria-label="Layers" aria-busy={updating}>
      <h4>Layers{updating && <em className="updating-tag"> updating…</em>}{layout && <small> · canvas {layout.width}×{layout.height}px</small>}</h4>
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
