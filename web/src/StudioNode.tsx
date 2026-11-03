import { Handle, Position, type NodeProps } from '@xyflow/react'
import { artifactUrl, type Kind } from './api'
import { KIND_INFO, canLink, visibleInputs, kindBackground, portText, portTitle } from './ports'
import { useStudio, type StudioNode } from './store'
import { useNodeFindings } from './Findings'
import { runSummary } from './NodeActions'
import { etaSeconds, formatEta, nodeStarts, useNow } from './eta'

export const KIND_COLOR = Object.fromEntries(Object.entries(KIND_INFO).map(([k, v]) => [k, v.colour])) as Record<Kind, string>

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
  const badges = useNodeFindings(id)
  const drag = useStudio((s) => s.dragFrom)
  const linked = useStudio((s) => s.edges.filter((e) => e.target === id).map((e) => e.targetHandle).join(','))
  const from = useStudio((s) => (s.dragFrom ? s.types[s.nodes.find((x) => x.id === s.dragFrom!.node)?.data.type ?? ''] : undefined))
  const fit = (dir: 'in' | 'out', kind: Kind) => {
    if (!drag) return ''
    if (drag.dir === dir || drag.node === id) return ' dim'
    const other = (drag.dir === 'out' ? from?.outputs : from?.inputs)?.find((x) => x.name === drag.port)?.type
    if (!other) return ''
    return (dir === 'in' ? canLink(other, kind) : canLink(kind, other)) ? ' fits' : ' dim'
  }
  if (!t) return <div className="node">unknown node {data.type}</div>
  const done = !!run?.key && run.state !== 'error'
  const shown = t.outputs
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
        {visibleInputs(t.inputs, linked.split(',')).map((p) => {
          const x = portText(data.type, 'in', p.name)
          return (
            <div className={`port in${fit('in', p.type)}`} key={p.name} title={portTitle(data.type, 'in', p.name, p.type, p.required)}>
              <Handle type="target" position={Position.Left} id={p.name} className={`k-${p.type}`} style={{ background: kindBackground(p.type) }} />
              <span>{x.or ? 'or ' : ''}{x.label}{p.required || x.either ? '' : '?'}</span>
            </div>
          )
        })}
        {shown.map((p) => (
          <div className={`port out${fit('out', p.type)}${done && !run.outputs?.[p.name]?.available ? ' empty' : ''}`} key={p.name} title={portTitle(data.type, 'out', p.name, p.type, true)}>
            <span>{portText(data.type, 'out', p.name).label}</span>
            {done && run.key && run.outputs?.[p.name]?.available && (p.type === 'image' || p.type === 'mask' || p.type === 'shapes' || p.type === 'tiles' || p.type === 'layer') && (
              <img className={`thumb ${p.type}${run.stale ? ' stale' : ''}`} src={artifactUrl(run.key, p.name, 96)} alt="" draggable={false} />
            )}
            <Handle type="source" position={Position.Right} id={p.name} className={`k-${p.type}`} style={{ background: kindBackground(p.type) }} />
          </div>
        ))}
        {run?.state === 'error' && <div className="err">{run.error}</div>}
      </div>
      {run?.state === 'done' && <div className="node-foot">{runSummary(run)}</div>}
    </div>
  )
}
