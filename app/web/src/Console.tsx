import { useEffect, useRef } from 'react'
import { useStudio } from './store'

export default function Console({ nodeId, height }: { nodeId?: string; height?: number }) {
  const logs = useStudio((s) => s.logs)
  const nodes = useStudio((s) => s.nodes)
  const clear = useStudio((s) => s.clearLogs)
  const box = useRef<HTMLDivElement>(null)
  const shown = nodeId ? logs.filter((l) => l.node === nodeId) : logs
  useEffect(() => { box.current?.scrollTo(0, box.current.scrollHeight) }, [shown.length])
  const name = (id: string, type: string) => nodes.find((n) => n.id === id)?.data.title || type || id
  return (
    <div className="console" style={height ? { height } : undefined}>
      <div className="console-bar"><b>Console</b><span className="mut">{shown.length} lines</span><button onClick={clear}>Clear</button></div>
      <div className="console-lines" ref={box}>
        {shown.map((l) => (
          <div key={l.id} className={l.level}><span className="mut">{l.t.toFixed(2)}s</span> <b>{name(l.node, l.type)}</b> {l.msg}</div>
        ))}
        {!shown.length && <div className="mut">no messages yet</div>}
      </div>
    </div>
  )
}
