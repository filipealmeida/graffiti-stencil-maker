import type { Edge } from '@xyflow/react'
import type { StudioNode } from './store'

/** Bridge settings the stencil and Split volume nodes used to have. */
const LEGACY = ['bridge_mm', 'bridges_per_island', 'bridge_spacing', 'bridge_min_angle_deg', 'bridge_max_length', 'bridge_passes', 'raised_bridges', 'z_bridging', 'bridge_islands']
const KEPT = ['bridge_mm', 'bridges_per_island', 'bridge_spacing', 'bridge_min_angle_deg', 'bridge_max_length', 'raised_bridges', 'z_bridging']
const OWNERS = ['stencil', 'vector_stencil', 'tiles']

const edgeId = (e: { source: string; sourceHandle?: string | null; target: string; targetHandle?: string | null }) =>
  `${e.source}.${e.sourceHandle}->${e.target}.${e.targetHandle}`

/** Brings a project saved by an older version up to the current nodes (the engine does the same, see graph/migrate.rs). */
export function migrateProject(nodes: StudioNode[], edges: Edge[]): { nodes: StudioNode[]; edges: Edge[] } {
  const renamed = new Set(nodes.filter((n) => n.data.type === 'bridge_fragments').map((n) => n.id))
  let ns = nodes.map((n) => {
    const t = n.data.type === 'complexity' ? 'remesh' : n.data.type === 'bridge_fragments' ? 'bridges' : n.data.type
    return t === n.data.type ? n : { ...n, data: { ...n.data, type: t } }
  })
  let es = edges.map((e) => (e.targetHandle === 'parts' && renamed.has(e.target) ? { ...e, targetHandle: 'body', id: edgeId({ ...e, targetHandle: 'body' }) } : e))
  for (const n of ns.filter((x) => OWNERS.includes(x.data.type) && LEGACY.some((k) => k in x.data.params))) {
    const params: Record<string, unknown> = {}
    const rest: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(n.data.params)) {
      if (!LEGACY.includes(k)) rest[k] = v
      else if (KEPT.includes(k)) params[k] = v
    }
    const wanted = n.data.params.bridge_islands !== false
    ns = ns.map((x) => (x.id === n.id ? { ...x, data: { ...x.data, params: rest } } : x))
    if (!wanted) continue
    let id = `${n.id}-bridges`
    while (ns.some((x) => x.id === id)) id += '_'
    const tiled = es.some((e) => e.target === n.id && e.targetHandle === 'tiles')
    const port = n.data.type === 'tiles' || tiled ? 'parts' : 'solid'
    es = es.map((e) => {
      if (e.source !== n.id || (e.sourceHandle !== 'solid' && e.sourceHandle !== 'parts')) return e
      if (n.data.type === 'tiles' && e.sourceHandle === 'solid') return e
      const moved = { ...e, source: id }
      return { ...moved, id: edgeId(moved) }
    })
    const link = { source: n.id, sourceHandle: port, target: id, targetHandle: 'body' }
    es.push({ ...link, id: edgeId(link) })
    ns.push({ id, type: 'studio', position: { x: n.position.x + 300, y: n.position.y + 110 }, data: { type: 'bridges', params, title: undefined } })
  }
  return { nodes: ns, edges: es }
}
