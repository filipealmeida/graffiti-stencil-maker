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
  const line = (l: (typeof shown)[number]) => `${l.ts} ${l.level.toUpperCase()} ${name(l.node, l.type)}: ${l.msg}${l.ms != null ? ` ${l.ms} ms` : ''}`
  const exportLog = () => {
    const url = URL.createObjectURL(new Blob([shown.map(line).join('\n') + '\n'], { type: 'text/plain' }))
    const a = document.createElement('a')
    a.href = url
    a.download = `stencil-console-${new Date().toISOString().replace(/[:.]/g, '-')}.log`
    a.click()
    URL.revokeObjectURL(url)
  }
  return (
    <div className="console" style={height ? { height } : undefined}>
      <div className="console-bar"><b>Console</b><span className="mut">{shown.length} lines</span><button onClick={exportLog} disabled={!shown.length}>Export</button><button onClick={clear}>Clear</button></div>
      <div className="console-lines" ref={box}>
        {shown.map((l) => (
          <div key={l.id} className={l.level}><span className="mut">{l.ts}</span> <b>{name(l.node, l.type)}</b> {l.msg}{l.ms != null && <span className="mut"> {l.ms} ms</span>}</div>
        ))}
        {!shown.length && <div className="mut">no messages yet</div>}
      </div>
    </div>
  )
}
