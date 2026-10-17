import { useEffect, useState } from 'react'
import { engineStatus } from './api'

const TICK_MS = 100
const WINDOW = 100

/** Browsers do not expose real CPU use, so this shows what can be measured: how much of the time the engine worker was working and how late the page's own timer fires. */
export default function Perf() {
  const [m, setM] = useState({ busy: [] as number[], lag: 0, maxLag: 0, workers: 0, now: false })
  useEffect(() => {
    const busy: number[] = []
    const lags: number[] = []
    let last = performance.now()
    const id = setInterval(() => {
      const now = performance.now()
      lags.push(Math.max(0, now - last - TICK_MS))
      last = now
      const st = engineStatus()
      busy.push(st.busy ? 1 : 0)
      if (busy.length > WINDOW) { busy.shift(); lags.shift() }
      if (busy.length % 5 === 0)
        setM({ busy: [...busy], lag: lags.reduce((a, b) => a + b, 0) / lags.length, maxLag: Math.max(...lags), workers: st.workers, now: st.busy })
    }, TICK_MS)
    return () => clearInterval(id)
  }, [])
  const load = m.busy.length ? (m.busy.reduce((a, b) => a + b, 0) / m.busy.length) * 100 : 0
  const bars = m.busy.filter((_, i) => i % 2 === 0)
  const mem = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory
  return (
    <div className="perf" role="status" aria-label="Performance overlay" title="Browsers do not report CPU use. Engine load is the share of the last 10 s that the engine worker was computing; UI lag is how late the page's own timer fires.">
      <b>Performance</b>
      <div>Engine workers: <b>{m.workers}</b> <span className={m.now ? 'busy' : 'mut'}>{m.workers ? (m.now ? 'busy' : 'idle') : 'not started'}</span></div>
      <div>CPU cores: <b>{navigator.hardwareConcurrency ?? '?'}</b> <span className="mut">(engine uses 1)</span></div>
      <div>Engine load (10 s): <b>{load.toFixed(0)}%</b></div>
      <svg viewBox={`0 0 ${WINDOW / 2} 10`} preserveAspectRatio="none" className="spark">
        {bars.map((b, i) => (b ? <rect key={i} x={i} y={0} width={1} height={10} /> : null))}
      </svg>
      <div>UI lag: <b>{m.lag.toFixed(0)} ms</b> <span className="mut">max {m.maxLag.toFixed(0)}</span></div>
      {mem && <div>JS memory: <b>{(mem.usedJSHeapSize / 1048576).toFixed(0)} MB</b></div>}
    </div>
  )
}
