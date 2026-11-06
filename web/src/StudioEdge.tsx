import { BaseEdge, EdgeLabelRenderer, getBezierPath, type EdgeProps } from '@xyflow/react'
import { useStudio } from './store'
import { KIND_INFO } from './ports'
import { deleteEdgeWithUndo } from './NodeActions'

/** Bezier edge with a wide hit area and a delete button at its midpoint while selected. */
export default function StudioEdge(p: EdgeProps) {
  const [path, x, y] = getBezierPath(p)
  const kind = useStudio((s) => s.types[s.nodes.find((n) => n.id === p.source)?.data.type ?? '']?.outputs.find((o) => o.name === p.sourceHandleId)?.type)
  const colour = kind ? KIND_INFO[kind].colour : p.style?.stroke
  return (
    <>
      <BaseEdge id={p.id} path={path} style={{ ...p.style, stroke: p.selected ? '#ff8a1f' : colour, strokeWidth: p.selected ? 3 : 2 }} interactionWidth={26} />
      {p.selected && (
        <EdgeLabelRenderer>
          <button className="edge-del nodrag nopan" style={{ transform: `translate(-50%,-50%) translate(${x}px,${y}px)` }} onClick={() => deleteEdgeWithUndo(p.id)} title="Delete connection" aria-label="Delete connection">×</button>
        </EdgeLabelRenderer>
      )}
    </>
  )
}
