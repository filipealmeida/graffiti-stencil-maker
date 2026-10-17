import { useEffect, useState } from 'react'
import type { NodeRun } from './api'

/** When each node of the current run began computing (performance.now), for time-left estimates. */
export const nodeStarts: Record<string, number> = {}

/**
 * Seconds left in the current run. The running node is extrapolated from its progress; nodes that have not started
 * count with the time they took last time (nodes the engine can serve from its cache will be quicker).
 */
export function etaSeconds(runs: Record<string, NodeRun>, now: number): number | null {
  let ms = 0
  let known = false
  for (const [id, r] of Object.entries(runs)) {
    if (r.state === 'running') {
      const elapsed = now - (nodeStarts[id] ?? now)
      const f = r.progress ?? 0
      ms += f > 0.08 ? (elapsed * (1 - f)) / f : Math.max((r.ms ?? 0) - elapsed, 0)
      known = true
    } else if (r.stale && r.state === 'done' && !r.cached && (r.ms ?? 0) > 0) {
      ms += r.ms ?? 0
      known = true
    }
  }
  return known ? ms / 1000 : null
}

export function formatEta(s: number): string {
  if (s < 1) return 'almost done'
  if (s < 90) return `~${Math.round(s)} s left`
  return `~${Math.round(s / 60)} min left`
}

/** Re-renders its component twice a second while `on`, so elapsed times and estimates stay current. */
export function useNow(on: boolean): number {
  const [now, setNow] = useState(performance.now())
  useEffect(() => {
    if (!on) return
    setNow(performance.now())
    const t = setInterval(() => setNow(performance.now()), 500)
    return () => clearInterval(t)
  }, [on])
  return now
}
