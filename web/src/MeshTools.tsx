import { useEffect, useState } from 'react'
import { api, type ModelInfo } from './api'
import { checkpoint, logAction, reportError, useStudio, type StudioNode } from './store'

export function useModelInspection(node: StudioNode | undefined, preview = false) {
  const nodeId = node?.id ?? ''
  const modelId = node?.data.type === 'mesh_import' ? String(node.data.params.model_id ?? '') : ''
  const format = String(node?.data.params.format ?? 'stl')
  const [snapshot, setSnapshot] = useState<{ nodeId: string; id: string; info: ModelInfo; url: string } | null>(null)
  const [error, setError] = useState('')
  useEffect(() => {
    let live = true
    let url = ''
    setError('')
    if (!modelId) return
    Promise.all([api.modelInfo(modelId, format), preview ? api.modelPreview(modelId, format) : null]).then(([info, bytes]) => {
      if (!live) return
      url = bytes ? URL.createObjectURL(new Blob([bytes], { type: 'model/stl' })) : ''
      setSnapshot({ nodeId, id: modelId, info, url })
    }).catch((e: unknown) => {
      if (!live) return
      const message = String(e instanceof Error ? e.message : e)
      setError(message)
      logAction(`Mesh inspection failed: ${message}`, node?.id, 'error')
    })
    return () => { live = false; if (url) URL.revokeObjectURL(url) }
  }, [nodeId, modelId, format, preview])
  return {
    snapshot: snapshot?.nodeId === node?.id && snapshot?.id === modelId ? snapshot : null,
    displayInfo: snapshot?.nodeId === node?.id ? snapshot?.info : undefined,
    error,
  }
}

const REPAIRS = [
  { action: 'clean', label: 'Remove degenerate and duplicate faces', hint: 'Removes zero-area faces, repeated faces and opposing coincident face pairs. No vertices are moved.' },
  { action: 'orient', label: 'Fix winding', hint: 'Makes neighboring faces consistently oriented, then reverses the whole mesh if its signed volume is negative. Does not move vertices.' },
  { action: 'weld', label: 'Weld nearby vertices', hint: 'Changes geometry: merges vertices within the source-coordinate tolerance. May create degenerate faces; inspect and clean afterwards.' },
  { action: 'fill', label: 'Fill planar holes', hint: 'Changes geometry: closes simple, unbranched boundary loops of up to 2048 vertices. Refuses non-planar loops outside the tolerance. Can close intentional openings.' },
] as const

export default function MeshTools({ node, info, error }: { node: StudioNode; info?: ModelInfo; error: string }) {
  const [busy, setBusy] = useState(false)
  const [tolerance, setTolerance] = useState(0.01)
  const [message, setMessage] = useState<{ nodeId: string; modelId: string; text: string } | null>(null)
  const modelId = String(node.data.params.model_id ?? '')
  const repair = async (action: string, label: string) => {
    setBusy(true)
    setMessage(null)
    logAction(`${label}: started${action === 'weld' || action === 'fill' ? ` (tolerance ${tolerance} source mm)` : ''}`, node.id)
    try {
      const result = await api.repairModel(modelId, String(node.data.params.format ?? 'stl'), action, tolerance)
      const after = await api.modelInfo(result.id, result.format)
      const current = useStudio.getState().nodes.find((n) => n.id === node.id)
      if (!current || current.data.params.model_id !== modelId) {
        logAction(`${label}: result not applied because the source changed`, node.id)
        return
      }
      checkpoint()
      useStudio.setState((s) => ({ nodes: s.nodes.map((n) => n.id === node.id ? {
        ...n, data: { ...n.data, params: { ...n.data.params, model_id: result.id, format: result.format } },
      } : n) }))
      checkpoint()
      const summary = `${label}: ${info?.triangles ?? '?'} → ${after.triangles} triangles; ${info?.vertices ?? '?'} → ${after.vertices} vertices; ${after.valid ? 'solid checks passed' : 'defects remain; solid output blocked'}`
      setMessage({ nodeId: node.id, modelId: result.id, text: summary })
      logAction(summary, node.id)
    } catch (e) {
      const text = `${label} failed: ${String(e instanceof Error ? e.message : e)}`
      setMessage({ nodeId: node.id, modelId, text })
      reportError(text, node.id)
    } finally { setBusy(false) }
  }
  if (!modelId) return null
  return <section className="imageinfo mesh-tools" aria-label="Mesh repairs" aria-busy={busy}>
    <h4>Repair tools</h4>
    {error && <p className="err" role="alert">{error}</p>}
    {!info && !error && <p>Inspecting mesh…</p>}
    {info && <p className={info.valid ? 'mut' : 'err'}>{info.valid ? 'Solid checks passed.' : 'Defects found; solid output blocked. Open preview to locate defects.'}</p>}
    <fieldset disabled={busy || !info}>
      <legend>Conservative repairs</legend>
      {REPAIRS.slice(0, 2).map((r) => <button key={r.action} title={r.hint} onClick={() => void repair(r.action, r.label)}>{r.label}</button>)}
    </fieldset>
    <fieldset disabled={busy || !info}>
      <legend>Geometry-changing repairs</legend>
      <p className="mut">May change dimensions or close intentional openings.</p>
      <label>Tolerance (source mm)<input className="numbox" aria-label="Repair tolerance" type="number" min={0.000001} max={10} step={0.001} value={tolerance}
        onChange={(e) => setTolerance(e.target.valueAsNumber)} /></label>
      {REPAIRS.slice(2).map((r) => <button key={r.action} disabled={!Number.isFinite(tolerance) || tolerance < 0.000001 || tolerance > 10} title={r.hint} onClick={() => void repair(r.action, r.label)}>{r.label}</button>)}
    </fieldset>
    {message?.nodeId === node.id && message.modelId === modelId && <p role="status">{message.text}</p>}
    <p className="mut">Repairs are undoable working copies. The original disk file is unchanged; source downloads use the working copy.</p>
  </section>
}

export function MeshProperties({ node, info, error, updating = false }: { node: StudioNode; info?: ModelInfo; error: string; updating?: boolean }) {
  if (!node.data.params.model_id) return <p className="mut">Open a model to inspect it.</p>
  const scale = Number(node.data.params.scale ?? 1)
  const rows: [string, string][] = info ? [
    ['Solid output', info.valid ? 'eligible (after node size checks)' : 'blocked'],
    ['Watertight', info.watertight ? 'yes' : 'no'],
    ['Degenerate faces', String(info.degenerate)],
    ['Duplicate faces', String(info.duplicate_faces)],
    ['Boundary edges', String(info.boundary_edges)],
    ['Non-manifold edges', String(info.nonmanifold_edges)],
    ['Winding conflicts', String(info.winding_edges)],
    ['Inverted winding', info.inverted ? 'yes' : 'no'],
    ['Triangles', info.triangles.toLocaleString()],
    ['Vertices (exactly indexed)', info.vertices.toLocaleString()],
    ['Source size (mm)', info.size_mm.map((x) => +x.toFixed(4)).join(' × ')],
    ['Scaled size (mm)', info.size_mm.map((x) => +(x * scale).toFixed(4)).join(' × ')],
  ] : []
  const details: [string, string][] = info ? [
    ['Format', info.format.toUpperCase()],
    ['Source bytes', info.bytes.toLocaleString()],
    ['Signed volume (source mm³)', String(+info.signed_volume_mm3.toFixed(4))],
    ['Components', String(info.components)],
  ] : []
  const table = (values: [string, string][]) => <dl>{values.map(([k, v]) => <div key={k} style={{ display: 'contents' }}><dt>{k}</dt><dd>{v}</dd></div>)}</dl>
  return <section className="imageinfo mesh-properties" aria-label="Mesh properties" aria-busy={updating}>
    <h4>Mesh properties</h4>
    {error && <p className="err" role="alert">{error}</p>}
    {!info && !error && <p>Inspecting mesh…</p>}
    {updating && info && <p className="mut">Updating mesh information; previous statistics shown.</p>}
    {info && !updating && <p className={info.valid ? 'mut' : 'err'}>{info.valid ? 'Solid checks passed.' : 'Defects found. Solid output is blocked until repaired.'}</p>}
    {table(rows)}
    <details className="mesh-details"><summary>File and topology details</summary>{table(details)}<p className="mut">{info?.checks}</p></details>
  </section>
}
