import { Handle, Position, type NodeProps } from '@xyflow/react'
import { artifactUrl, type Kind } from './api'
import { useStudio, type StudioNode } from './store'

export const KIND_COLOR: Record<Kind, string> = { image: '#4aa3ff', mask: '#ffa24a', solid: '#5fd38d', parts: '#c28bff', any: '#8a93a0' }

export default function StudioNodeView({ id, data, selected }: NodeProps<StudioNode>) {
  const t = useStudio((s) => s.types[data.type])
  const run = useStudio((s) => s.runs[id])
  const edges = useStudio((s) => s.edges)
  if (!t) return <div className="node">unknown node {data.type}</div>
  const done = run?.state === 'done'
  const shown = t.outputs.filter((o) => !done || run?.outputs?.[o.name]?.available || edges.some((e) => e.source === id && e.sourceHandle === o.name))
  return (
    <div className={`node stage-${t.stage}${selected ? ' sel' : ''}`}>
      <div className="node-head">
        <span className={`dot ${run?.state ?? 'idle'}`} title={run?.error ?? run?.state ?? 'not run'} />
        <b>{data.title || t.label}</b>
        {run?.state === 'running' && <span className="pct">{Math.round((run.progress ?? 0) * 100)}%</span>}
      </div>
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
            {done && run.key && run.outputs?.[p.name]?.available && (p.type === 'image' || p.type === 'mask') && (
              <img className={`thumb ${p.type}`} src={artifactUrl(run.key, p.name, 96)} alt="" draggable={false} />
            )}
            <Handle type="source" position={Position.Right} id={p.name} style={{ background: KIND_COLOR[p.type] }} />
          </div>
        ))}
        {run?.state === 'error' && <div className="err">{run.error}</div>}
      </div>
    </div>
  )
}
