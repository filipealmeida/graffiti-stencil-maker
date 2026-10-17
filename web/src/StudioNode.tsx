import { Handle, Position, type NodeProps } from '@xyflow/react'
import { api, artifactUrl, type Kind } from './api'
import { useStudio, type StudioNode } from './store'
import { badgesFor } from './badges'
import { etaSeconds, formatEta, nodeStarts, useNow } from './eta'

export const KIND_COLOR: Record<Kind, string> = { image: '#4aa3ff', mask: '#ffa24a', shapes: '#f0d24a', solid: '#5fd38d', parts: '#c28bff', any: '#8a93a0' }

/** Open another picture as a new Image node, or save the one loaded here. */
function SourceActions({ id, imageId, title }: { id: string; imageId: string; title?: string }) {
  const open = async (f: File) => {
    try {
      const img = await api.upload(f)
      const st = useStudio.getState()
      const me = st.nodes.find((n) => n.id === id)
      const at = me ? { x: me.position.x, y: me.position.y + 220 } : undefined
      st.addNode('source', { image_id: img.id }, at, img.name)
    } catch (e) {
      useStudio.setState({ error: String(e instanceof Error ? e.message : e) })
    }
  }
  const download = async () => {
    const img = await api.storedImage(imageId)
    if (!img) return useStudio.setState({ error: 'the picture is no longer stored in this browser' })
    const url = URL.createObjectURL(new Blob([img.bytes]))
    const a = document.createElement('a')
    a.href = url
    a.download = img.name || title || 'image'
    a.click()
    setTimeout(() => URL.revokeObjectURL(url), 10_000)
  }
  return (
    <div className="node-actions nodrag">
      <label className="btn" title="Open another picture as a new Image node">Open new…
        <input type="file" accept=".png,.jpg,.jpeg,.bmp,.svg" hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) open(f) }} />
      </label>
      <button title="Download the original picture loaded in this node" onClick={(e) => { e.stopPropagation(); download() }}>Download</button>
    </div>
  )
}

/** What a running node is doing and how long it has left. */
function RunNote({ id, message, progress, ms }: { id: string; message?: string; progress?: number; ms?: number }) {
  const now = useNow(true)
  const started = nodeStarts[id] ?? now
  const left = etaSeconds({ [id]: { state: 'running', progress, ms } }, now)
  return (
    <div className="runnote" aria-live="off">
      {message && <span>{message}</span>}
      <span>{Math.max(0, Math.round((now - started) / 1000))} s{left !== null && left >= 3 ? ` · ${formatEta(left)}` : ''}</span>
    </div>
  )
}

export default function StudioNodeView({ id, data, selected }: NodeProps<StudioNode>) {
  const t = useStudio((s) => s.types[data.type])
  const run = useStudio((s) => s.runs[id])
  const edges = useStudio((s) => s.edges)
  if (!t) return <div className="node">unknown node {data.type}</div>
  const done = !!run?.key && run.state !== 'error'
  const first = t.outputs.find((o) => run?.outputs?.[o.name]?.available)
  const badges = done && !run.stale && run.key && first ? badgesFor(data.type, data.params, api.peekInfo(run.key, first.name)) : []
  const shown = t.outputs.filter((o) => !done || run?.outputs?.[o.name]?.available || edges.some((e) => e.source === id && e.sourceHandle === o.name))
  return (
    <div className={`node stage-${t.stage}${selected ? ' sel' : ''}`} aria-label={`${data.title || t.label} node, ${run?.state ?? 'not run'}`}>
      <div className="node-head">
        <span className={`dot ${run?.state ?? 'idle'}`} role="img" aria-label={run?.error ?? run?.state ?? 'not run'} title={run?.error ?? run?.state ?? 'not run'} />
        <b>{data.title || t.label}</b>
        <button className="node-del nodrag" title="Duplicate node (Ctrl+D)" aria-label="Duplicate node" onClick={(e) => { e.stopPropagation(); useStudio.getState().duplicate(id) }}>⧉</button>
        <button className="node-del nodrag" title="Delete node" aria-label="Delete node" onClick={(e) => { e.stopPropagation(); useStudio.getState().removeNode(id) }}>×</button>
        {run?.state === 'running' && <span className="pct">{Math.round((run.progress ?? 0) * 100)}%</span>}
      </div>
      {run?.state === 'running' && <div className="bar" role="progressbar" aria-label={`${data.title || t.label} progress`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((run.progress ?? 0) * 100)}><i style={{ width: `${(run.progress ?? 0) * 100}%` }} /></div>}
      {run?.state === 'running' && <RunNote id={id} message={run.message} progress={run.progress} ms={run.ms} />}
      {badges.length > 0 && (
        <ul className="badges" aria-label="Findings">
          {badges.map((b) => <li key={b.text} className={`badge ${b.level}`}><span aria-hidden="true">{b.level === 'error' ? '✖' : b.level === 'warn' ? '▲' : 'ℹ'}</span> {b.text}</li>)}
        </ul>
      )}
      <div className="node-body">
        {t.inputs.map((p) => (
          <div className="port in" key={p.name}>
            <Handle type="target" position={Position.Left} id={p.name} style={{ background: KIND_COLOR[p.type] }} />
            <span>{p.name}{p.required ? '' : '?'}</span>
          </div>
        ))}
        {shown.map((p) => (
          <div className="port out" key={p.name}>
            <span>{p.name}</span>
            {done && run.key && run.outputs?.[p.name]?.available && (p.type === 'image' || p.type === 'mask' || p.type === 'shapes') && (
              <img className={`thumb ${p.type}${run.stale ? ' stale' : ''}`} src={artifactUrl(run.key, p.name, 96)} alt="" draggable={false} />
            )}
            <Handle type="source" position={Position.Right} id={p.name} style={{ background: KIND_COLOR[p.type] }} />
          </div>
        ))}
        {t.id === 'source' && <SourceActions id={id} imageId={String(data.params.image_id)} title={data.title} />}
        {run?.state === 'error' && <div className="err">{run.error}</div>}
      </div>
    </div>
  )
}
