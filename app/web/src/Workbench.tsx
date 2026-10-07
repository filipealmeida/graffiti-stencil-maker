import { useEffect, useState } from 'react'
import { api, artifactUrl, partUrl } from './api'
import Console from './Console'
import ImageView from './ImageView'
import ParamForm from './ParamForm'
import { useStudio } from './store'
import Viewer3D from './Viewer3D'

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
  const [view, setView] = useState('')
  const [mode, setMode] = useState<'plate' | 'tiles'>('plate')
  const [info, setInfo] = useState<Record<string, any> | null>(null)

  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === 'Escape' && open(null)
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  }, [])

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
        <h2>{node.data.title || t.label} <small className={`tag stage-${t.stage}`}>{t.stage}</small></h2>
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
        {run?.state === 'running' && <span className="pct">{Math.round((run.progress ?? 0) * 100)}% {run.message}</span>}
        {run?.state !== 'running' && busy && <span className="pct">updating…</span>}
        {run?.state === 'done' && !busy && <span className="mut">{run.cached ? 'cached' : `${run.ms ?? 0} ms`}</span>}
        <button onClick={() => open(null)}>Close ✕</button>
      </div>
      {run?.state === 'running' && <div className="bar"><i style={{ width: `${(run.progress ?? 0) * 100}%` }} /></div>}
      <div className="bench-body">
        <aside className="bench-params">
          <p className="mut">{t.doc}</p>
          <ParamForm node={node} type={t} />
          {run?.state === 'error' && <div className="err">{run.error}</div>}
        </aside>
        <section className="bench-view">
          {!cur && <div className="mut center">{run?.state === 'error' ? run.error : 'nothing computed yet'}</div>}
          {cur && (cur.kind === 'image' || cur.kind === 'mask' || cur.kind === 'any') && cur.key && (
            <ImageView key={`${cur.id}`} src={artifactUrl(cur.key, cur.out)} kind={cur.kind === 'mask' ? 'mask' : 'image'}
              under={cur.kind === 'mask' && cur.id.startsWith('out:') ? imageFor(cur.key) : undefined} />
          )}
          {cur?.kind === 'solid' && <Viewer3D parts={solidParts()} />}
          {cur?.kind === 'parts' && <Viewer3D parts={partsList()} />}
        </section>
      </div>
      <div className="bench-foot">
        <div className="stats">
          {info?.stats && STATS.filter((k) => k in info.stats).map((k) => <span key={k}>{k.replace(/_/g, ' ')}: <b>{JSON.stringify(info.stats[k])}</b></span>)}
          {info?.coverage !== undefined && <span>{info.width}×{info.height}px · <b>{(info.coverage * 100).toFixed(1)}%</b> paint</span>}
          {info?.width !== undefined && info.coverage === undefined && <span>{info.width}×{info.height}px</span>}
          {cur?.kind === 'solid' && cur.key && <a className="btn" href={artifactUrl(cur.key, cur.out)} download="stencil.stl">Download STL</a>}
          {cur?.kind === 'parts' && cur.key && <a className="btn" href={artifactUrl(cur.key, cur.out)}>Download</a>}
        </div>
        <Console nodeId={id} height={130} />
      </div>
    </div>
  )
}
