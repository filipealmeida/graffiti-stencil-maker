import { useMemo } from 'react'
import { api } from './api'
import { badgesFor, type BadgeLevel } from './badges'
import { useStudio } from './store'

interface Finding { id: string; label: string; level: BadgeLevel; text: string }

const ICON: Record<BadgeLevel, string> = { error: '✖', warn: '▲', info: 'ℹ' }

/** Print readiness over the whole graph and where the time went. It mirrors `report.rs`, which the CLI's `--report` uses. */
export function useReport() {
  const nodes = useStudio((s) => s.nodes)
  const runs = useStudio((s) => s.runs)
  const types = useStudio((s) => s.types)
  const { findings, timing } = useMemo(() => {
    const findings: Finding[] = []
    const timing: { id: string; label: string; ms: number; cached: boolean }[] = []
    const seen = new Set<string>()
    for (const n of nodes) {
      const t = types[n.data.type]
      const run = runs[n.id]
      if (!t || !run) continue
      const label = n.data.title || t.label
      if (run.state === 'error') findings.push({ id: n.id, label, level: 'error', text: run.error ?? 'failed' })
      if (!run.key || run.stale) continue
      if (typeof run.ms === 'number') timing.push({ id: n.id, label, ms: run.ms, cached: !!run.cached })
      for (const o of t.outputs) {
        if (!run.outputs?.[o.name]?.available) continue
        for (const b of badgesFor(n.data.type, n.data.params, api.peekInfo(run.key, o.name))) {
          // parts carry the tiling description on through Pads and Screws: say it once
          if (seen.has(b.text)) continue
          seen.add(b.text)
          findings.push({ id: n.id, label, level: b.level, text: b.text })
        }
      }
    }
    timing.sort((a, b) => b.ms - a.ms)
    return { findings, timing }
  }, [nodes, runs, types])
  const has = (l: BadgeLevel) => findings.some((f) => f.level === l)
  const verdict = has('error') ? 'blocked' : has('warn') ? 'check' : 'ready'
  const computed = Object.values(runs).some((r) => r.key)
  const count = (l: BadgeLevel) => findings.filter((f) => f.level === l).length
  return { findings, timing, verdict, computed, errors: count('error'), warnings: count('warn') }
}

export default function Report() {
  const select = useStudio((s) => s.select)
  const { findings, timing, verdict, computed } = useReport()
  return (
    <div className="report" role="region" aria-label="Run report">
      <b>Print readiness: </b>
      {computed ? <span className={`verdict ${verdict}`}>{verdict}</span> : <span className="mut">run the graph first</span>}
      {findings.length > 0 && (
        <ul aria-label="Findings">
          {findings.map((f, i) => (
            <li key={i}>
              <button className={`badge ${f.level}`} onClick={() => select(f.id)} title="Select this node">
                <span aria-hidden="true">{ICON[f.level]}</span> <b>{f.label}</b>: {f.text}
              </button>
            </li>
          ))}
        </ul>
      )}
      {timing.length > 0 && (
        <>
          <h4>Time per node</h4>
          <ul aria-label="Timing">
            {timing.map((t) => (
              <li key={t.id}>
                <button className="t" onClick={() => select(t.id)}>
                  <span>{t.label}</span>
                  <span className={t.cached ? 'mut' : ''}>{t.cached ? 'cached' : `${t.ms < 1000 ? Math.round(t.ms) + ' ms' : (t.ms / 1000).toFixed(1) + ' s'}`}</span>
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  )
}
