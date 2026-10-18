import { create } from 'zustand'
import {
  addEdge, applyEdgeChanges, applyNodeChanges,
  type Connection, type Edge, type EdgeChange, type Node, type NodeChange,
} from '@xyflow/react'
import { api, type GraphJson, type LogLine, type NodeRun, type NodeType } from './api'
import { nodeStarts } from './eta'

export interface StudioData extends Record<string, unknown> {
  type: string
  params: Record<string, unknown>
  title?: string
}
export type StudioNode = Node<StudioData, 'studio'>

export const STORE_KEY = 'studio.graph.v1'
const LANES_KEY = 'studio.lanes.v1'
const MIN_LANE = 320
const GAP = 30
export interface Lanes { stencil: number; post: number; end: number }
const DEFAULT_LANES: Lanes = { stencil: 1180, post: 1740, end: 2140 }
function loadLanes(): Lanes {
  try {
    const l = JSON.parse(localStorage.getItem(LANES_KEY) ?? 'null')
    if (l && [l.stencil, l.post, l.end].every(Number.isFinite) && l.stencil >= MIN_LANE + GAP && l.post - l.stencil >= MIN_LANE + GAP && l.end - l.post >= MIN_LANE) return l
  } catch { /* fall back to the default layout */ }
  return DEFAULT_LANES
}
/** Left edge and width of a lane's usable area (nodes are placed inside it). */
export function laneSpan(l: Lanes, stage: 'trace' | 'stencil' | 'post') {
  if (stage === 'trace') return { x: 0, w: l.stencil - GAP }
  if (stage === 'stencil') return { x: l.stencil, w: l.post - GAP - l.stencil }
  return { x: l.post, w: l.end - l.post }
}
/** Moves one lane boundary, keeping every lane at least MIN_LANE wide. */
export function resizeLane(l: Lanes, edge: keyof Lanes, x: number): Lanes {
  const n = { ...l }
  if (edge === 'stencil') n.stencil = Math.min(Math.max(x, MIN_LANE + GAP), l.post - GAP - MIN_LANE)
  else if (edge === 'post') n.post = Math.min(Math.max(x, l.stencil + GAP + MIN_LANE), l.end - MIN_LANE)
  else n.end = Math.max(x, l.post + MIN_LANE)
  return n
}
const NODE_W = 240
const NODE_H = 170
/** First spot at or below/right of `want` inside the lane that overlaps no existing node. */
export function freeSpot(nodes: StudioNode[], lane: { x: number; w: number }, want: { x: number; y: number }) {
  const box = (n: StudioNode) => ({ x: n.position.x, y: n.position.y, w: n.measured?.width ?? NODE_W, h: n.measured?.height ?? NODE_H })
  const boxes = nodes.map(box)
  const hits = (x: number, y: number) => boxes.some((b) => x < b.x + b.w + 16 && b.x < x + NODE_W + 16 && y < b.y + b.h + 16 && b.y < y + NODE_H + 16)
  const x0 = Math.min(Math.max(want.x, lane.x + 10), Math.max(lane.x + 10, lane.x + lane.w - NODE_W))
  const columns = Math.max(1, Math.floor((lane.x + lane.w - x0) / (NODE_W + 24)))
  for (let row = 0; row < 60; row++) {
    for (let col = 0; col < columns; col++) {
      const x = x0 + col * (NODE_W + 24)
      const y = want.y + row * 40
      if (!hits(x, y)) return { x, y }
    }
  }
  return want
}
let counter = 0
const USAGE_KEY = 'stencil-usage'
/** How often each node type was added on this browser, kept to sort the "+ All" menu by popularity. */
function loadUsage(): Record<string, number> {
  try { return JSON.parse(localStorage.getItem(USAGE_KEY) ?? '{}') } catch { return {} }
}

let clip: { nodes: StudioNode[]; edges: Edge[] } | null = null
let pasteCount = 0
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
  /** The results on screen come from a quick coarse pass, made while a slider is being dragged. */
  draft: boolean
  addLogs: (l: LogLine[]) => void
  clearLogs: () => void
  setConsole: (o: boolean) => void
  openBench: (id: string | null) => void
  setTypes: (t: NodeType[]) => void
  onNodesChange: (c: NodeChange<StudioNode>[]) => void
  onEdgesChange: (c: EdgeChange[]) => void
  connect: (c: Connection) => void
  select: (id: string | null) => void
  usage: Record<string, number>
  perfOpen: boolean
  setPerf: (o: boolean) => void
  addNode: (type: string, params?: Record<string, unknown>, pos?: { x: number; y: number }, title?: string) => string
  setParam: (id: string, name: string, value: unknown) => void
  removeNode: (id: string) => void
  copy: () => boolean
  paste: () => void
  duplicate: (id?: string) => void
  removeEdge: (id: string) => void
  load: (nodes: StudioNode[], edges: Edge[]) => void
  starter: (imageId: string, title: string) => void
  colourStarter: (imageId: string, title: string) => void
  vectorStarter: (imageId: string, title: string) => void
  colourSetStarter: (imageId: string, title: string) => void
  graph: () => GraphJson
  setRuns: (r: Record<string, NodeRun>, busy: boolean, error?: string | null) => void
  canUndo: boolean
  canRedo: boolean
  lanes: Lanes
  setLanes: (l: Lanes) => void
}

export const useStudio = create<State>((set, get) => ({
  canUndo: false, canRedo: false,
  lanes: loadLanes(),
  setLanes: (lanes) => { localStorage.setItem(LANES_KEY, JSON.stringify(lanes)); set({ lanes }) },
  types: {}, nodes: [], edges: [], runs: {}, selected: null, busy: false, error: null,
  logs: [], usage: loadUsage(), perfOpen: localStorage.getItem('stencil-perf') === '1', consoleOpen: true, bench: null, progress: { done: 0, total: 0 }, draft: false,
  addLogs: (l) => set((s) => ({ logs: [...s.logs, ...l.map((x, i) => ({ ...x, id: s.logs.length + i + Math.random() }))].slice(-500) })),
  clearLogs: () => set({ logs: [] }),
  setConsole: (consoleOpen) => set({ consoleOpen }),
  setPerf: (perfOpen) => { localStorage.setItem('stencil-perf', perfOpen ? '1' : '0'); set({ perfOpen }) },
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
    const lane = laneSpan(get().lanes, t.stage)
    const position = freeSpot(get().nodes, lane, pos ?? { x: lane.x + 30, y: 40 })
    const defaults = Object.fromEntries(t.params.map((p) => [p.name, p.default]))
    const node: StudioNode = { id, type: 'studio', position, data: { type, params: { ...defaults, ...params }, title } }
    const usage = { ...get().usage, [type]: (get().usage[type] ?? 0) + 1 }
    try { localStorage.setItem(USAGE_KEY, JSON.stringify(usage)) } catch { /* the count is a convenience */ }
    set((s) => ({ nodes: [...s.nodes, node], selected: id, usage }))
    return id
  },
  setParam: (id, name, value) => set((s) => ({
    nodes: s.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, params: { ...n.data.params, [name]: value } } } : n)),
  })),
  copy: () => {
    const { nodes, edges, selected } = get()
    let picked = nodes.filter((n) => n.selected)
    if (!picked.length && selected) picked = nodes.filter((n) => n.id === selected)
    if (!picked.length) return false
    const ids = new Set(picked.map((n) => n.id))
    clip = {
      nodes: picked.map((n) => ({ ...n, data: { ...n.data, params: structuredClone(n.data.params) } })),
      edges: edges.filter((e) => ids.has(e.source) && ids.has(e.target)),
    }
    pasteCount = 0
    return true
  },
  paste: () => {
    if (!clip) return
    const { types, nodes, lanes } = get()
    pasteCount++
    const map = new Map<string, string>()
    clip.nodes.forEach((n) => map.set(n.id, uid(n.data.type)))
    const fresh: StudioNode[] = []
    for (const n of clip.nodes) {
      const t = types[n.data.type]
      if (!t) continue
      // a single node goes to the nearest free spot of its lane; a group keeps its shape and shifts as a block
      const want = { x: n.position.x + 40 * pasteCount, y: n.position.y + 40 * pasteCount }
      const position = clip.nodes.length === 1 ? freeSpot(nodes, laneSpan(lanes, t.stage), want) : want
      fresh.push({ ...n, id: map.get(n.id)!, position, selected: true, measured: undefined, data: { ...n.data, params: structuredClone(n.data.params) } })
    }
    if (!fresh.length) return
    const kept = new Set(fresh.map((n) => n.id))
    const edges = clip.edges
      .filter((e) => kept.has(map.get(e.source)!) && kept.has(map.get(e.target)!))
      .map((e) => {
        const [source, target] = [map.get(e.source)!, map.get(e.target)!]
        return { ...e, id: `${source}.${e.sourceHandle}->${target}.${e.targetHandle}`, source, target, selected: false }
      })
    set((s) => ({
      nodes: [...s.nodes.map((n) => (n.selected ? { ...n, selected: false } : n)), ...fresh],
      edges: [...s.edges, ...edges],
      selected: fresh[fresh.length - 1].id,
    }))
  },
  duplicate: (id) => {
    const { nodes, selected } = get()
    if (id) set({ nodes: nodes.map((n) => ({ ...n, selected: n.id === id })), selected: id })
    else if (!nodes.some((n) => n.selected) && selected) set({ nodes: nodes.map((n) => ({ ...n, selected: n.id === selected })) })
    if (get().copy()) get().paste()
  },
  removeEdge: (id) => set((s) => ({ edges: s.edges.filter((e) => e.id !== id) })),
  removeNode: (id) => set((s) => ({
    nodes: s.nodes.filter((n) => n.id !== id),
    edges: s.edges.filter((e) => e.source !== id && e.target !== id),
    selected: s.selected === id ? null : s.selected,
    bench: s.bench === id ? null : s.bench,
  })),
  // "complexity" is the old name of the Remesh node
  load: (nodes, edges) =>
    set({ nodes: nodes.map((n) => (n.data.type === 'complexity' ? { ...n, data: { ...n.data, type: 'remesh' } } : n)), edges, runs: {}, selected: null }),
  starter: (imageId, title) => {
    const { addNode, connect } = get()
    set({ nodes: [], edges: [], runs: {}, selected: null })
    const src = addNode('source', { image_id: imageId }, { x: 40, y: 60 }, title)
    const thr = addNode('threshold', {}, { x: 420, y: 60 })
    const st = addNode('stencil', {}, { x: laneSpan(get().lanes, 'stencil').x + 30, y: 60 })
    const ex = addNode('export', {}, { x: laneSpan(get().lanes, 'post').x + 30, y: 60 })
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
    const st = addNode('stencil', { width_mm: 100, margin_mm: 6, raised_bridges: true, z_bridging: 'easy', layer_height_mm: 0.2 }, { x: laneSpan(get().lanes, 'stencil').x + 30, y: 60 })
    const ex = addNode('export', {}, { x: laneSpan(get().lanes, 'post').x + 30, y: 60 })
    connect({ source: src, sourceHandle: 'image', target: red, targetHandle: 'image' })
    connect({ source: red, sourceHandle: 'image', target: tp, targetHandle: 'image' })
    connect({ source: tp, sourceHandle: 'mask', target: st, targetHandle: 'mask' })
    connect({ source: st, sourceHandle: 'solid', target: ex, targetHandle: 'solid' })
    set({ selected: tp })
  },
  colourSetStarter: (imageId, title) => {
    const { addNode, connect } = get()
    set({ nodes: [], edges: [], runs: {}, selected: null })
    const src = addNode('source', { image_id: imageId, resolution: 600 }, { x: 40, y: 60 }, title)
    const split = addNode('split_colours', { colours: 2 }, { x: 400, y: 60 })
    const sp = laneSpan(get().lanes, 'stencil')
    const plate = { width_mm: 100, margin_mm: 10, raised_bridges: true, z_bridging: 'easy' }
    const a = addNode('stencil', plate, { x: sp.x + 30, y: 60 }, 'Colour 1')
    const b = addNode('stencil', plate, { x: sp.x + 30, y: 300 }, 'Colour 2')
    const set_ = addNode('colour_set', {}, { x: laneSpan(get().lanes, 'post').x + 30, y: 60 })
    connect({ source: src, sourceHandle: 'image', target: split, targetHandle: 'image' })
    connect({ source: split, sourceHandle: 'colour_1', target: a, targetHandle: 'mask' })
    connect({ source: split, sourceHandle: 'colour_2', target: b, targetHandle: 'mask' })
    connect({ source: a, sourceHandle: 'solid', target: set_, targetHandle: 'colour_1' })
    connect({ source: b, sourceHandle: 'solid', target: set_, targetHandle: 'colour_2' })
    set({ selected: set_ })
  },
  vectorStarter: (imageId, title) => {
    const { addNode, connect } = get()
    set({ nodes: [], edges: [], runs: {}, selected: null })
    const src = addNode('source', { image_id: imageId, resolution: 600 }, { x: 40, y: 60 }, title)
    const thr = addNode('threshold', {}, { x: 400, y: 60 })
    const tr = addNode('trace', {}, { x: 760, y: 60 })
    const st = addNode('vector_stencil', {}, { x: laneSpan(get().lanes, 'stencil').x + 30, y: 60 })
    const ex = addNode('export', {}, { x: laneSpan(get().lanes, 'post').x + 30, y: 60 })
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

/** Settings that make a run cheaper while a slider is dragged: the same graph at low resolution and without stepped bridges. */
const DRAFT_RESOLUTION = 250
export function coarsen(g: GraphJson): GraphJson {
  return {
    ...g,
    nodes: g.nodes.map((n) => {
      const p = { ...n.params }
      if (n.type === 'source' && Number(p.resolution) > DRAFT_RESOLUTION) p.resolution = DRAFT_RESOLUTION
      if ((n.type === 'stencil' || n.type === 'vector_stencil') && p.z_bridging === 'steps') p.z_bridging = 'easy'
      if (n.type === 'vector_stencil' && Number(p.detail) > 2) p.detail = 2
      return { ...n, params: p }
    }),
  }
}

/**
 * Evaluates the whole graph. A run for a graph that is already running or already evaluated is skipped.
 * A draft run uses coarser settings (see `coarsen`) and is skipped when that would change nothing.
 */
export async function runGraph(draft = false) {
  const st = useStudio.getState()
  const full = st.graph()
  if (!full.nodes.length) { token++; runningSig = doneSig = null; api.cancel(); return st.setRuns({}, false) }
  const g = draft ? coarsen(full) : full
  if (draft && JSON.stringify(g) === JSON.stringify(full)) return
  const sig = draft ? `draft:${JSON.stringify(g)}` : signature(st)
  if (sig === runningSig || (sig === doneSig && !st.error)) return
  const mine = ++token
  runningSig = sig
  doneSig = null
  for (const k of Object.keys(nodeStarts)) delete nodeStarts[k]
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
      if (step.state.state === 'running' && nodeStarts[step.node] === undefined) nodeStarts[step.node] = performance.now()
      merged[step.node] = step.state.state === 'running' && prev?.key ? { ...step.state, key: prev.key, outputs: prev.outputs, stale: true, ms: prev.ms } : step.state
      if (step.log.length) useStudio.getState().addLogs(step.log)
      useStudio.setState({ runs: { ...merged }, progress: { done: step.n + (step.state.state === 'running' ? step.state.progress ?? 0 : 0), total: step.total || g.nodes.length } })
    })
    if (mine !== token) return
    runningSig = null
    // a draft is never final: the full run must still happen
    if (r.status === 'done' && !draft) doneSig = sig
    useStudio.setState({ busy: false, error: r.error ?? null, draft: draft && r.status === 'done' })
  } catch (e) {
    if (mine !== token) return
    runningSig = null
    if (!(e instanceof Error && e.message === 'superseded')) useStudio.setState({ busy: false, error: String(e instanceof Error ? e.message : e) })
  }
}
