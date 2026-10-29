import ImageInfoPanel from './ImageInfo'
import { useEffect, useState } from 'react'
import { api, artifactUrl } from './api'
import { useStudio } from './store'
import ParamForm from './ParamForm'
import SourceActions from './SourceActions'
import Presets from './Presets'
import { NodeActions, DeleteNode, downloadFor, runSummary } from './NodeActions'
import Findings from './Findings'

export default function Inspector() {
  const id = useStudio((s) => s.selected)
  const node = useStudio((s) => s.nodes.find((n) => n.id === s.selected))
  const t = useStudio((s) => (node ? s.types[node.data.type] : undefined))
  const run = useStudio((s) => (id ? s.runs[id] : undefined))
  const busy = useStudio((s) => s.busy)
  const [info, setInfo] = useState<Record<string, any> | null>(null)
  const [out, setOut] = useState<string>('')
  const key = run?.key
  const upKey = useStudio((s) => {
    const e = s.edges.find((x) => x.target === s.selected && x.targetHandle === 'solid')
    return e ? s.runs[e.source]?.key : undefined
  })
  const upOut = useStudio((s) => s.edges.find((x) => x.target === s.selected && x.targetHandle === 'solid')?.sourceHandle ?? 'solid')
  const [layers, setLayers] = useState<number | undefined>()
  useEffect(() => {
    setLayers(undefined)
    if (node?.data.type === 'remesh' && upKey)
      api.info(upKey, upOut).then((i) => setLayers(i?.stats?.layers)).catch(() => {})
  }, [upKey, upOut, node?.data.type])
  const avail = t?.outputs.filter((o) => run?.outputs?.[o.name]?.available) ?? []
  const cur = avail.find((o) => o.name === out) ?? avail[0]

  useEffect(() => {
    setInfo(null)
    if (key && cur) api.info(key, cur.name).then(setInfo).catch(() => setInfo(null))
  }, [key, cur?.name])

  if (!node || !t) return <aside className="inspector none"><p className="mut">Select a node to edit its parameters and see its result. Drop a picture on the canvas to start.</p></aside>
  const fileInput = t.id === 'source'
  return (
    <aside className="inspector">
      <h2>{t.label} <small className={`tag stage-${t.stage}`}>{t.stage}</small></h2>
      <p className="mut status-line">{runSummary(run, busy)}</p>
      <Findings id={node.id} />
      <Presets key={node.id} node={node} />
      {run?.state === 'error' && <div className="err">{run.error}</div>}
      <ParamForm node={node} type={t} layers={layers} before={<>
      <NodeActions id={node.id} preview download={downloadFor(cur?.type, key, cur?.name, Object.keys(info?.files ?? {}).length)} />
      {fileInput && <><p className="mut">Image: {String(node.data.title ?? node.data.params.image_id)}</p><SourceActions id={node.id} imageId={String(node.data.params.image_id)} title={node.data.title} /></>}
      </>} />
      {fileInput && <ImageInfoPanel imageId={String(node.data.params.image_id)} resolution={Number(node.data.params.resolution ?? 1000)} />}
      {key && cur && (
        <div className="result">
          {avail.length > 1 && (
            <div className="tabs">{avail.map((o) => <button key={o.name} className={o.name === cur.name ? 'on' : ''} onClick={() => setOut(o.name)}>{o.name}</button>)}</div>
          )}
          {(cur.type === 'image' || cur.type === 'mask' || cur.type === 'shapes') && <img className={`big ${cur.type}`} src={artifactUrl(key, cur.name, 700)} alt="" />}
          {cur.type === 'parts' && info?.files && (
            <>
              <ul>{Object.entries(info.files as Record<string, number>).map(([n, b]) => <li key={n}>{n} <em>{(b / 1024).toFixed(0)} kB</em></li>)}</ul>
            </>
          )}
          {info?.stats && (
            <table><tbody>
              {['islands_found', 'bridges_added', 'islands_remaining', 'watertight', 'triangles', 'overhang_area_mm2', 'layers', 'size_mm', 'remesh'].filter((k) => k in info.stats).map((k) => (
                <tr key={k}><td>{k.replace(/_/g, ' ')}</td><td>{JSON.stringify(info.stats[k])}</td></tr>
              ))}
            </tbody></table>
          )}
          {info?.coverage !== undefined && <p className="mut">{info.width}×{info.height}px · {(info.coverage * 100).toFixed(1)}% paint</p>}
        </div>
      )}
      <div className="panel-foot"><DeleteNode id={node.id} /></div>
    </aside>
  )
}
