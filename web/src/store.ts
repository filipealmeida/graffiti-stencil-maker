import { create } from 'zustand'
import {
  addEdge, applyEdgeChanges, applyNodeChanges,
  type Connection, type Edge, type EdgeChange, type Node, type NodeChange,
} from '@xyflow/react'
import { api, type GraphJson, type LogLine, type NodeRun, type NodeType } from './api'

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
  logs: (LogLine & { id: number })[]
  consoleOpen: boolean
  bench: string | null
  progress: { done: number; total: number }
  addLogs: (l: LogLine[]) => void
  clearLogs: () => void
  setConsole: (o: boolean) => void
  openBench: (id: string | null) => void
  setTypes: (t: NodeType[]) => void
  onNodesChange: (c: NodeChange<StudioNode>[]) => void
  onEdgesChange: (c: EdgeChange[]) => void
  connect: (c: Connection) => void
  select: (id: string | null) => void
  addNode: (type: string, params?: Record<string, unknown>, pos?: { x: number; y: number }, title?: string) => string
  setParam: (id: string, name: string, value: unknown) => void
  removeNode: (id: string) => void
  removeEdge: (id: string) => void
  load: (nodes: StudioNode[], edges: Edge[]) => void
  starter: (imageId: string, title: string) => void
  colourStarter: (imageId: string, title: string) => void
  vectorStarter: (imageId: string, title: string) => void
  graph: () => GraphJson
  setRuns: (r: Record<string, NodeRun>, busy: boolean, error?: string | null) => void
  canUndo: boolean
  canRedo: boolean
}

export const useStudio = create<State>((set, get) => ({
  canUndo: false, canRedo: false,
  types: {}, nodes: [], edges: [], runs: {}, selected: null, busy: false, error: null,
  logs: [], consoleOpen: true, bench: null, progress: { done: 0, total: 0 },
  addLogs: (l) => set((s) => ({ logs: [...s.logs, ...l.map((x, i) => ({ ...x, id: s.logs.length + i + Math.random() }))].slice(-500) })),
  clearLogs: () => set({ logs: [] }),
  setConsole: (consoleOpen) => set({ consoleOpen }),
  openBench: (bench) => set({ bench, selected: bench ?? get().selected }),
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
  removeEdge: (id) => set((s) => ({ edges: s.edges.filter((e) => e.id !== id) })),
  removeNode: (id) => set((s) => ({
    nodes: s.nodes.filter((n) => n.id !== id),
    edges: s.edges.filter((e) => e.source !== id && e.target !== id),
    selected: s.selected === id ? null : s.selected,
    bench: s.bench === id ? null : s.bench,
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
  colourStarter: (imageId, title) => {
    const { addNode, connect } = get()
    set({ nodes: [], edges: [], runs: {}, selected: null })
    const src = addNode('source', { image_id: imageId, resolution: 600 }, { x: 40, y: 60 }, title)
    const red = addNode('reduce', { colours: 3 }, { x: 400, y: 60 })
    const tp = addNode('tone_patterns', {}, { x: 760, y: 60 })
    const st = addNode('stencil', { width_mm: 100, margin_mm: 6, raised_bridges: true, z_bridging: 'easy', layer_height_mm: 0.2 }, { x: LANE_X.stencil + 30, y: 60 })
    const ex = addNode('export', {}, { x: LANE_X.post + 30, y: 60 })
    connect({ source: src, sourceHandle: 'image', target: red, targetHandle: 'image' })
    connect({ source: red, sourceHandle: 'image', target: tp, targetHandle: 'image' })
    connect({ source: tp, sourceHandle: 'mask', target: st, targetHandle: 'mask' })
    connect({ source: st, sourceHandle: 'solid', target: ex, targetHandle: 'solid' })
    set({ selected: tp })
  },
  vectorStarter: (imageId, title) => {
    const { addNode, connect } = get()
    set({ nodes: [], edges: [], runs: {}, selected: null })
    const src = addNode('source', { image_id: imageId, resolution: 600 }, { x: 40, y: 60 }, title)
    const thr = addNode('threshold', {}, { x: 400, y: 60 })
    const tr = addNode('trace', {}, { x: 760, y: 60 })
    const st = addNode('vector_stencil', {}, { x: LANE_X.stencil + 30, y: 60 })
    const ex = addNode('export', {}, { x: LANE_X.post + 30, y: 60 })
    connect({ source: src, sourceHandle: 'image', target: thr, targetHandle: 'image' })
    connect({ source: thr, sourceHandle: 'mask', target: tr, targetHandle: 'mask' })
    connect({ source: tr, sourceHandle: 'shapes', target: st, targetHandle: 'shapes' })
    connect({ source: st, sourceHandle: 'solid', target: ex, targetHandle: 'solid' })
    set({ selected: tr })
  },
  graph: () => ({
    nodes: get().nodes.map((n) => ({ id: n.id, type: n.data.type, params: n.data.params })),
    edges: get().edges.map((e) => ({ from: [e.source, e.sourceHandle ?? ''] as [string, string], to: [e.target, e.targetHandle ?? ''] as [string, string] })),
  }),
  setRuns: (runs, busy, error = null) => set({ runs, busy, error }),
}))

interface Snap { nodes: StudioNode[]; edges: Edge[]; key: string }
const snapKey = (nodes: StudioNode[], edges: Edge[]) =>
  JSON.stringify([nodes.map((n) => [n.id, n.data.type, n.data.params, n.data.title, Math.round(n.position.x), Math.round(n.position.y)]),
    edges.map((e) => [e.source, e.sourceHandle, e.target, e.targetHandle])])
const snap = (): Snap => {
  const { nodes, edges } = useStudio.getState()
  return { nodes, edges, key: snapKey(nodes, edges) }
}
const HISTORY_LIMIT = 100
const history = { past: [] as Snap[], future: [] as Snap[], base: null as Snap | null, timer: 0 as ReturnType<typeof setTimeout> | 0 }
const flags = () => useStudio.setState({ canUndo: history.past.length > 0 || pending(), canRedo: history.future.length > 0 })
const pending = () => !!history.base && snap().key !== history.base.key

/** Edits that settle within 400 ms (a slider drag, a node drag) become one undo step. */
function commit() {
  if (history.timer) clearTimeout(history.timer)
  history.timer = 0
  const cur = snap()
  if (!history.base) history.base = cur
  else if (cur.key !== history.base.key) {
    history.past.push(history.base)
    if (history.past.length > HISTORY_LIMIT) history.past.shift()
    history.base = cur
    history.future = []
  }
  flags()
}
export function resetHistory() {
  if (history.timer) clearTimeout(history.timer)
  history.timer = 0
  history.past = []; history.future = []; history.base = snap()
  flags()
}
function restore(to: Snap) {
  history.base = to
  useStudio.setState({ nodes: to.nodes, edges: to.edges, selected: null })
  flags()
}
export function undo() {
  commit()
  const prev = history.past.pop()
  if (!prev || !history.base) return flags()
  history.future.push(history.base)
  restore(prev)
}
export function redo() {
  commit()
  const next = history.future.pop()
  if (!next || !history.base) return flags()
  history.past.push(history.base)
  restore(next)
}
let seen: [StudioNode[], Edge[]] = [[], []]
useStudio.subscribe((s) => {
  if (s.nodes === seen[0] && s.edges === seen[1]) return
  seen = [s.nodes, s.edges]
  if (!history.base) history.base = snap()
  if (history.timer) clearTimeout(history.timer)
  history.timer = setTimeout(commit, 400)
  useStudio.setState((p) => (p.canUndo === true ? p : { canUndo: true }))
})

/** Parameters, wiring and node types decide what a run computes; positions and selection never trigger one. */
export const signature = (s: Pick<State, 'nodes' | 'edges'>) =>
  JSON.stringify([s.nodes.map((n) => [n.id, n.data.type, n.data.params]), s.edges.map((e) => [e.source, e.sourceHandle, e.target, e.targetHandle])])

let token = 0
let runningSig: string | null = null
let doneSig: string | null = null

/** Evaluates the whole graph. A run for a graph that is already running or already evaluated is skipped. */
export async function runGraph() {
  const st = useStudio.getState()
  const g = st.graph()
  if (!g.nodes.length) { token++; runningSig = doneSig = null; api.cancel(); return st.setRuns({}, false) }
  const sig = signature(st)
  if (sig === runningSig || (sig === doneSig && !st.error)) return
  const mine = ++token
  runningSig = sig
  doneSig = null
  // results of the previous run stay on screen, marked stale, until each node has its new one
  const merged: Record<string, NodeRun> = {}
  for (const n of g.nodes) {
    const prev = st.runs[n.id]
    merged[n.id] = prev ? { ...prev, stale: true } : { state: 'pending' }
  }
  useStudio.setState({ runs: merged, busy: true, error: null, progress: { done: 0, total: g.nodes.length } })
  try {
    const r = await api.run(g, undefined, (step) => {
      if (mine !== token) return
      const prev = merged[step.node]
      merged[step.node] = step.state.state === 'running' && prev?.key ? { ...step.state, key: prev.key, outputs: prev.outputs, stale: true } : step.state
      if (step.log.length) useStudio.getState().addLogs(step.log)
      useStudio.setState({ runs: { ...merged }, progress: { done: step.n, total: step.total || g.nodes.length } })
    })
    if (mine !== token) return
    runningSig = null
    if (r.status === 'done') doneSig = sig
    useStudio.setState({ busy: false, error: r.error ?? null })
  } catch (e) {
    if (mine !== token) return
    runningSig = null
    if (!(e instanceof Error && e.message === 'superseded')) useStudio.setState({ busy: false, error: String(e instanceof Error ? e.message : e) })
  }
}
export { LANE_X }
