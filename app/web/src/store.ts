import { create } from 'zustand'
import {
  addEdge, applyEdgeChanges, applyNodeChanges,
  type Connection, type Edge, type EdgeChange, type Node, type NodeChange,
} from '@xyflow/react'
import { api, type GraphJson, type NodeRun, type NodeType } from './api'

export interface StudioData extends Record<string, unknown> {
  type: string
  params: Record<string, unknown>
  title?: string
}
export type StudioNode = Node<StudioData, 'studio'>

const LANE_X = { trace: 0, stencil: 1180, post: 1740 } as const
let counter = 0
const uid = (t: string) => `${t}-${Date.now().toString(36)}${(counter++).toString(36)}`

interface State {
  types: Record<string, NodeType>
  nodes: StudioNode[]
  edges: Edge[]
  runs: Record<string, NodeRun>
  selected: string | null
  busy: boolean
  error: string | null
  setTypes: (t: NodeType[]) => void
  onNodesChange: (c: NodeChange<StudioNode>[]) => void
  onEdgesChange: (c: EdgeChange[]) => void
  connect: (c: Connection) => void
  select: (id: string | null) => void
  addNode: (type: string, params?: Record<string, unknown>, pos?: { x: number; y: number }, title?: string) => string
  setParam: (id: string, name: string, value: unknown) => void
  removeNode: (id: string) => void
  load: (nodes: StudioNode[], edges: Edge[]) => void
  starter: (imageId: string, title: string) => void
  graph: () => GraphJson
  setRuns: (r: Record<string, NodeRun>, busy: boolean, error?: string | null) => void
}

export const useStudio = create<State>((set, get) => ({
  types: {}, nodes: [], edges: [], runs: {}, selected: null, busy: false, error: null,
  setTypes: (t) => set({ types: Object.fromEntries(t.map((x) => [x.id, x])) }),
  onNodesChange: (c) => set((s) => ({ nodes: applyNodeChanges(c, s.nodes) })),
  onEdgesChange: (c) => set((s) => ({ edges: applyEdgeChanges(c, s.edges) })),
  connect: (c) => set((s) => ({
    // an input takes one connection: a new one replaces the old
    edges: addEdge({ ...c, id: `${c.source}.${c.sourceHandle}->${c.target}.${c.targetHandle}` },
      s.edges.filter((e) => !(e.target === c.target && e.targetHandle === c.targetHandle))),
  })),
  select: (id) => set({ selected: id }),
  addNode: (type, params, pos, title) => {
    const t = get().types[type]
    const id = uid(type)
    const inLane = get().nodes.filter((n) => get().types[n.data.type]?.stage === t.stage).length
    const position = pos ?? { x: LANE_X[t.stage] + 30 + (inLane % 2) * 20, y: 40 + inLane * 90 }
    const defaults = Object.fromEntries(t.params.map((p) => [p.name, p.default]))
    const node: StudioNode = { id, type: 'studio', position, data: { type, params: { ...defaults, ...params }, title } }
    set((s) => ({ nodes: [...s.nodes, node], selected: id }))
    return id
  },
  setParam: (id, name, value) => set((s) => ({
    nodes: s.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, params: { ...n.data.params, [name]: value } } } : n)),
  })),
  removeNode: (id) => set((s) => ({
    nodes: s.nodes.filter((n) => n.id !== id),
    edges: s.edges.filter((e) => e.source !== id && e.target !== id),
    selected: s.selected === id ? null : s.selected,
  })),
  load: (nodes, edges) => set({ nodes, edges, runs: {}, selected: null }),
  starter: (imageId, title) => {
    const { addNode, connect } = get()
    const src = addNode('source', { image_id: imageId }, { x: 40, y: 60 }, title)
    const thr = addNode('threshold', {}, { x: 420, y: 60 })
    const st = addNode('stencil', {}, { x: LANE_X.stencil + 30, y: 60 })
    const ex = addNode('export', {}, { x: LANE_X.post + 30, y: 60 })
    connect({ source: src, sourceHandle: 'image', target: thr, targetHandle: 'image' })
    connect({ source: thr, sourceHandle: 'mask', target: st, targetHandle: 'mask' })
    connect({ source: st, sourceHandle: 'solid', target: ex, targetHandle: 'solid' })
    set({ selected: st })
  },
  graph: () => ({
    nodes: get().nodes.map((n) => ({ id: n.id, type: n.data.type, params: n.data.params })),
    edges: get().edges.map((e) => ({ from: [e.source, e.sourceHandle ?? ''] as [string, string], to: [e.target, e.targetHandle ?? ''] as [string, string] })),
  }),
  setRuns: (runs, busy, error = null) => set({ runs, busy, error }),
}))

/** Parameters, wiring and node types decide what a run computes; positions and selection never trigger one. */
export const signature = (s: Pick<State, 'nodes' | 'edges'>) =>
  JSON.stringify([s.nodes.map((n) => [n.id, n.data.type, n.data.params]), s.edges.map((e) => [e.source, e.sourceHandle, e.target, e.targetHandle])])

let token = 0
export async function runGraph() {
  const g = useStudio.getState().graph()
  const mine = ++token
  if (!g.nodes.length) return useStudio.getState().setRuns({}, false)
  useStudio.getState().setRuns(useStudio.getState().runs, true)
  try {
    const { job } = await api.run(g)
    for (;;) {
      const st = await api.job(job)
      if (mine !== token) return
      useStudio.getState().setRuns(st.nodes, st.status !== 'done', st.error ?? null)
      if (st.status === 'done') return
      await new Promise((r) => setTimeout(r, 250))
    }
  } catch (e) {
    if (mine === token) useStudio.getState().setRuns({}, false, String(e instanceof Error ? e.message : e))
  }
}
export { LANE_X }
