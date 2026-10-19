import { Handle, Position, type NodeProps } from '@xyflow/react'
import { api, artifactUrl, type Kind } from './api'
import { useStudio, type StudioNode } from './store'
import { badgesFor } from './badges'
import { etaSeconds, formatEta, nodeStarts, useNow } from './eta'

export const KIND_COLOR: Record<Kind, string> = { image: '#4aa3ff', mask: '#ffa24a', shapes: '#f0d24a', tiles: '#ff7a9c', solid: '#5fd38d', parts: '#c28bff', any: '#8a93a0' }

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
  const problem = badges.some((b) => b.level === 'error') || run?.state === 'error' ? ' problem-error' : badges.some((b) => b.level === 'warn') ? ' problem-warn' : ''
  const stale = run?.stale && run.state !== 'running' ? ' stale-node' : ''
  return (
    <div className={`node stage-${t.stage}${selected ? ' sel' : ''}${problem}${stale}`} aria-label={`${data.title || t.label} node, ${run?.state ?? 'not run'}`}>
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
            {done && run.key && run.outputs?.[p.name]?.available && (p.type === 'image' || p.type === 'mask' || p.type === 'shapes' || p.type === 'tiles') && (
              <img className={`thumb ${p.type}${run.stale ? ' stale' : ''}`} src={artifactUrl(run.key, p.name, 96)} alt="" draggable={false} />
            )}
            <Handle type="source" position={Position.Right} id={p.name} style={{ background: KIND_COLOR[p.type] }} />
          </div>
        ))}
        {run?.state === 'error' && <div className="err">{run.error}</div>}
      </div>
    </div>
  )
}
