import { Suspense, lazy, useEffect, useState } from 'react'
import { api, artifactUrl, partUrl, type Kind } from './api'
import Console from './Console'
import ImageView from './ImageView'
import ParamForm from './ParamForm'
import SourceActions from './SourceActions'
import Presets from './Presets'
import { NodeActions, DeleteNode, downloadFor, runSummary } from './NodeActions'
import Findings, { useNodeFindings } from './Findings'
import ImageInfoPanel from './ImageInfo'
import { useStudio } from './store'
import { parseEdits, stringify, type LayoutInfo } from './layoutEdit'
const Viewer3D = lazy(() => import('./Viewer3D'))

const STATS = ['islands_found', 'bridges_added', 'islands_remaining', 'watertight', 'triangles', 'overhang_area_mm2', 'layers', 'size_mm']

/** Full-size, live preview of one node with its parameters beside it. Double-click a node to open. */
export default function Workbench() {
  const id = useStudio((s) => s.bench)
  const node = useStudio((s) => s.nodes.find((n) => n.id === s.bench))
  const types = useStudio((s) => s.types)
  const edges = useStudio((s) => s.edges)
  const runs = useStudio((s) => s.runs)
  const busy = useStudio((s) => s.busy)
  const open = useStudio((s) => s.openBench)
  const setParam = useStudio((s) => s.setParam)
  const [view, setView] = useState('')
  const [mode, setMode] = useState<'plate' | 'tiles'>('plate')
  const [info, setInfo] = useState<Record<string, any> | null>(null)

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

  if (!id || !node || !t) return null
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
        <h2>{node.data.title || t.label} <small className={`tag stage-${t.stage}`}>{t.stage}</small>{problems > 0 && <span className="tb-badge warn" title="Findings on this node">{problems}</span>}</h2>
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
          {cur && (cur.kind === 'image' || cur.kind === 'mask' || cur.kind === 'shapes' || cur.kind === 'tiles' || cur.kind === 'any') && cur.key && (
            <ImageView key={`${cur.id}`} src={artifactUrl(cur.key, cur.out)} kind={cur.kind === 'mask' ? 'mask' : 'image'}
              under={cur.kind === 'mask' && cur.id.startsWith('out:') ? imageFor(cur.key) : undefined}
              layout={cur.kind === 'tiles' && cur.id.startsWith('out:') && node.data.type === 'tile_layout' && info?.kind === 'tiles' && info.pocket_list
                ? { info: info as LayoutInfo, edits: parseEdits(node.data.params.edits),
                    onCommit: (e) => setParam(node.id, 'edits', stringify(e)), onReset: () => setParam(node.id, 'edits', '') }
                : undefined} />
          )}
          <Suspense fallback={<div className="mut center">loading 3D viewer…</div>}>
            {cur?.kind === 'solid' && <Viewer3D parts={solidParts()} />}
            {cur?.kind === 'parts' && <Viewer3D parts={partsList()} plate={info?.meta?.plate} />}
          </Suspense>
        </section>
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
