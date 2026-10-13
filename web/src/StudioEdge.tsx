import { BaseEdge, EdgeLabelRenderer, getBezierPath, type EdgeProps } from '@xyflow/react'
import { useStudio } from './store'

/** Bezier edge with a wide hit area and a delete button at its midpoint while selected. */
export default function StudioEdge(p: EdgeProps) {
  const [path, x, y] = getBezierPath(p)
  const remove = useStudio((s) => s.removeEdge)
  return (
    <>
      <BaseEdge id={p.id} path={path} style={{ ...p.style, stroke: p.selected ? '#ff8a1f' : p.style?.stroke, strokeWidth: p.selected ? 3 : 2 }} interactionWidth={26} />
      {p.selected && (
        <EdgeLabelRenderer>
          <button className="edge-del nodrag nopan" style={{ transform: `translate(-50%,-50%) translate(${x}px,${y}px)` }} onClick={() => remove(p.id)} title="Delete connection">×</button>
        </EdgeLabelRenderer>
      )}
    </>
  )
}
