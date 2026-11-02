export interface GNode { id: string; type: string }
export interface GEdge { from: [string, string]; to: [string, string] }

/**
 * Parents-first order of `targets` and everything they depend on, or null when the graph has a cycle.
 * Of the nodes that are ready, the one that took least last time goes first, so a slow node holds up only what
 * depends on it and not the quick branches beside it.
 */
export function plan(nodes: GNode[], edges: GEdge[], targets: string[] | null, costs: Record<string, number> = {}): string[] | null {
  const ids = new Set(nodes.map((n) => n.id))
  const ups = new Map<string, string[]>()
  for (const e of edges) if (ids.has(e.from[0]) && ids.has(e.to[0])) ups.set(e.to[0], [...(ups.get(e.to[0]) ?? []), e.from[0]])
  const want = new Set<string>()
  const visit = (id: string) => { if (want.has(id)) return; want.add(id); (ups.get(id) ?? []).forEach(visit) }
  ;(targets ?? nodes.map((n) => n.id)).forEach((id) => ids.has(id) && visit(id))
  const order: string[] = []
  const done = new Set<string>()
  for (;;) {
    let pick: string | null = null
    for (const n of nodes) {
      if (!want.has(n.id) || done.has(n.id)) continue
      if (!(ups.get(n.id) ?? []).every((u) => done.has(u))) continue
      if (pick === null || (costs[n.id] ?? 0) < (costs[pick] ?? 0)) pick = n.id
    }
    if (pick === null) break
    done.add(pick)
    order.push(pick)
  }
  return order.length === want.size ? order : null
}

/** The nodes `id` depends on, itself included. */
export function ancestorsOf(id: string, edges: GEdge[]): Set<string> {
  const ups = new Map<string, string[]>()
  for (const e of edges) ups.set(e.to[0], [...(ups.get(e.to[0]) ?? []), e.from[0]])
  const seen = new Set<string>()
  const visit = (n: string) => { if (seen.has(n)) return; seen.add(n); (ups.get(n) ?? []).forEach(visit) }
  visit(id)
  return seen
}

/**
 * Splits the needed sinks over two engines when the branches are worth it. A branch is the work only one sink
 * needs, priced with the costs of the previous run (an unknown node counts as free, so a first run never splits).
 * The second engine re-derives in its own cache whatever the groups share that it does not already hold; `extraMs`
 * prices that per node. The split happens when the time saved on branches clearly outweighs the duplicated work, or
 * when it roughly breaks even and warms the second engine up for the runs that follow. Returns the two target
 * groups and who owns each node, or null to run everything on one engine.
 */
export function splitPlan(edges: GEdge[], order: string[], costs: Record<string, number>, extraMs: (id: string) => number, minMs = 250):
  { targets: [string[], string[]]; owner: Map<string, number> } | null {
  const need = new Set(order)
  const sinks = order.filter((id) => !edges.some((e) => e.from[0] === id && need.has(e.to[0])))
  if (sinks.length < 2) return null
  const anc = new Map(sinks.map((s) => [s, ancestorsOf(s, edges)]))
  // the cost of what only this sink needs
  const exclusive = (s: string) =>
    [...anc.get(s)!].filter((n) => sinks.every((o) => o === s || !anc.get(o)!.has(n))).reduce((sum, n) => sum + (costs[n] ?? 0), 0)
  const priced = sinks.map((s) => ({ s, ms: exclusive(s) })).sort((a, b) => b.ms - a.ms)
  // longest processing time first into the lighter bin
  const bins: { ids: string[]; ms: number }[] = [{ ids: [], ms: 0 }, { ids: [], ms: 0 }]
  for (const p of priced) {
    const b = bins[0].ms <= bins[1].ms ? bins[0] : bins[1]
    b.ids.push(p.s)
    b.ms += p.ms
  }
  if (!bins[0].ids.length || !bins[1].ids.length) return null
  const owned0 = new Set<string>()
  for (const s of bins[0].ids) for (const n of anc.get(s)!) owned0.add(n)
  const owned1 = new Set<string>()
  for (const s of bins[1].ids) for (const n of anc.get(s)!) owned1.add(n)
  // what the run saves against what the second engine would compute twice
  const gain = bins[0].ms + bins[1].ms - Math.max(bins[0].ms, bins[1].ms)
  const dupe = [...owned1].filter((n) => owned0.has(n)).reduce((sum, n) => sum + extraMs(n), 0)
  if (gain < dupe + minMs && !(gain >= 400 && gain >= dupe / 2)) return null
  // everything the second group needs and the first does not is owned by engine 1; the rest by engine 0
  const owner = new Map<string, number>()
  for (const n of order) owner.set(n, owned0.has(n) ? 0 : 1)
  return { targets: [bins[0].ids, bins[1].ids], owner }
}
