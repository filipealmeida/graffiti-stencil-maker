import { useEffect, useId, useRef } from 'react'
import { logAction, useStudio } from './store'

export default function Console({ nodeId, height }: { nodeId?: string; height?: number }) {
  const logs = useStudio((s) => s.logs)
  const nodes = useStudio((s) => s.nodes)
  const clear = useStudio((s) => s.clearLogs)
  const open = useStudio((s) => s.consoleOpen)
  const setOpen = useStudio((s) => s.setConsole)
  const logId = useId()
  const box = useRef<HTMLDivElement>(null)
  const shown = nodeId ? logs.filter((l) => l.node === nodeId || (!l.node && l.type === 'Action')) : logs
  useEffect(() => { if (open) box.current?.scrollTo(0, box.current.scrollHeight) }, [shown.length, open])
  const name = (id: string, type: string) => nodes.find((n) => n.id === id)?.data.title || type || id
  const line = (l: (typeof shown)[number]) => `${l.ts} ${l.level.toUpperCase()} ${name(l.node, l.type)}: ${l.msg}${l.ms != null ? ` ${l.ms} ms` : ''}`
  const exportLog = () => {
    const url = URL.createObjectURL(new Blob([shown.map(line).join('\n') + '\n'], { type: 'text/plain' }))
    const a = document.createElement('a')
    a.href = url
    a.download = `stencil-console-${new Date().toISOString().replace(/[:.]/g, '-')}.log`
    a.click()
    logAction(`Console export handed to browser: ${a.download}`, nodeId)
    URL.revokeObjectURL(url)
  }
  return (
    <section className="console" style={open && height ? { height } : undefined} aria-label="Console">
      <div className="console-bar"><button aria-expanded={open} aria-controls={logId} onClick={() => setOpen(!open)}>{open ? 'Hide console' : 'Show console'}</button><span className="mut">{shown.length} line{shown.length === 1 ? '' : 's'}{shown.some((l) => l.level === 'error') ? ' · errors' : ''}</span>{open && <><button onClick={exportLog} disabled={!shown.length}>Export</button><button onClick={clear}>Clear</button></>}</div>
      <div id={logId} hidden={!open} className="console-lines" ref={box} tabIndex={0} role="log" aria-label="Engine log">
        {shown.map((l) => (
          <div key={l.id} className={l.level}><span className="mut">{l.ts}</span> <b>{name(l.node, l.type)}</b> {l.msg}{l.ms != null && <span className="mut"> {l.ms} ms</span>}</div>
        ))}
        {!shown.length && <div className="mut">no messages yet</div>}
      </div>
    </section>
  )
}
