import { useEffect, useState } from 'react'
import { api, artifactUrl, type ParamDef } from './api'
import { useStudio } from './store'
import Viewer3D from './Viewer3D'

function Field({ p, value, onChange }: { p: ParamDef; value: unknown; onChange: (v: unknown) => void }) {
  if (p.kind === 'bool')
    return <label className="row chk"><input type="checkbox" checked={!!value} onChange={(e) => onChange(e.target.checked)} />{p.label}</label>
  if (p.kind === 'choice')
    return <label className="row"><span>{p.label}</span><select value={String(value)} onChange={(e) => onChange(e.target.value)}>{p.choices!.map((c) => <option key={c}>{c}</option>)}</select></label>
  if (p.kind === 'text') return null
  return (
    <label className="row">
      <span>{p.label} <em>{String(value)}</em></span>
      <input type="range" min={p.min} max={p.max} step={p.step} value={Number(value)} onChange={(e) => onChange(+e.target.value)} />
    </label>
  )
}

export default function Inspector() {
  const id = useStudio((s) => s.selected)
  const node = useStudio((s) => s.nodes.find((n) => n.id === s.selected))
  const t = useStudio((s) => (node ? s.types[node.data.type] : undefined))
  const run = useStudio((s) => (id ? s.runs[id] : undefined))
  const { setParam, removeNode } = useStudio.getState()
  const [info, setInfo] = useState<Record<string, any> | null>(null)
  const [out, setOut] = useState<string>('')
  const key = run?.state === 'done' ? run.key : undefined
  const avail = t?.outputs.filter((o) => run?.outputs?.[o.name]?.available) ?? []
  const cur = avail.find((o) => o.name === out) ?? avail[0]

  useEffect(() => {
    setInfo(null)
    if (key && cur) api.info(key, cur.name).then(setInfo).catch(() => setInfo(null))
  }, [key, cur?.name])

  if (!node || !t) return <aside className="inspector"><p className="mut">Select a node to edit its parameters and see its result. Drop a picture on the canvas to start.</p></aside>
  const groups = [...new Set(t.params.map((p) => p.group ?? ''))]
  const fileInput = t.id === 'source'
  return (
    <aside className="inspector">
      <h2>{t.label} <small className={`tag stage-${t.stage}`}>{t.stage}</small></h2>
      <p className="mut">{t.doc}</p>
      {fileInput && <p className="mut">Image: {String(node.data.title ?? node.data.params.image_id)}</p>}
      {groups.map((g) => (
        <fieldset key={g}>
          {g && <legend>{g}</legend>}
          {t.params.filter((p) => (p.group ?? '') === g).map((p) => (
            <Field key={p.name} p={p} value={node.data.params[p.name]} onChange={(v) => setParam(node.id, p.name, v)} />
          ))}
        </fieldset>
      ))}
      {run?.state === 'error' && <div className="err">{run.error}</div>}
      {key && cur && (
        <div className="result">
          {avail.length > 1 && (
            <div className="tabs">{avail.map((o) => <button key={o.name} className={o.name === cur.name ? 'on' : ''} onClick={() => setOut(o.name)}>{o.name}</button>)}</div>
          )}
          {(cur.type === 'image' || cur.type === 'mask') && <img className={`big ${cur.type}`} src={artifactUrl(key, cur.name, 700)} alt="" />}
          {cur.type === 'solid' && (
            <>
              <Viewer3D url={artifactUrl(key, cur.name)} />
              <a className="btn" href={artifactUrl(key, cur.name)} download="stencil.stl">Download STL</a>
            </>
          )}
          {cur.type === 'parts' && info?.files && (
            <>
              <ul>{Object.entries(info.files as Record<string, number>).map(([n, b]) => <li key={n}>{n} <em>{(b / 1024).toFixed(0)} kB</em></li>)}</ul>
              <a className="btn" href={artifactUrl(key, cur.name)}>Download {Object.keys(info.files).length > 1 ? 'ZIP' : 'STL'}</a>
            </>
          )}
          {info?.stats && (
            <table><tbody>
              {['islands_found', 'bridges_added', 'islands_remaining', 'watertight', 'triangles', 'overhang_area_mm2', 'layers', 'size_mm'].filter((k) => k in info.stats).map((k) => (
                <tr key={k}><td>{k.replace(/_/g, ' ')}</td><td>{JSON.stringify(info.stats[k])}</td></tr>
              ))}
            </tbody></table>
          )}
          {info?.coverage !== undefined && <p className="mut">{info.width}×{info.height}px · {(info.coverage * 100).toFixed(1)}% paint</p>}
        </div>
      )}
      <button className="danger" onClick={() => removeNode(node.id)}>Delete node</button>
    </aside>
  )
}
