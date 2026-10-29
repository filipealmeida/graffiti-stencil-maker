import type { Engine } from '../pkg/stencil_wasm.js'

type Req =
  | { id: number; op: 'types' }
  | { id: number; op: 'image'; bytes: ArrayBuffer }
  | { id: number; op: 'has'; imageId: string }
  | { id: number; op: 'imageinfo'; imageId: string; resolution: number }
  | { id: number; op: 'run'; graph: string; targets: string }
  | { id: number; op: 'cancel' }
  | { id: number; op: 'threads' }

let engine: Engine | null = null
let threads = 1
let why = 'threaded build not tried'

/** Half of the cores, and always at least one left free for the page and the rest of the machine. */
const budget = (cores: number) => Math.max(1, Math.min(Math.floor(cores / 2), cores - 1))

// The threaded build needs shared memory, which only a cross-origin isolated page may use; otherwise (or if the
// pool cannot start) the single-threaded build runs the same engine.
const ready = (async () => {
  const n = budget(navigator.hardwareConcurrency || 1)
  if (n < 2) why = 'too few cores to share with the page'
  else if (!self.crossOriginIsolated || typeof SharedArrayBuffer === 'undefined') why = 'page is not cross-origin isolated (needs COOP/COEP headers)'
  else {
    try {
      // relative to this worker (dist/assets/), so the site works from any sub-path
      const mtUrl = new URL('../pkg-mt/stencil_wasm.js', self.location.href).href
      const mt = await import(/* @vite-ignore */ mtUrl)
      await mt.default()
      await mt.initThreadPool(n)
      engine = new mt.Engine() as unknown as Engine
      threads = n
      why = 'threaded'
      return
    } catch (err) {
      why = 'threaded build missing or failed to start'
      console.warn('multi-threaded engine unavailable, using one thread', err)
    }
  }
  const st = await import('../pkg/stencil_wasm.js')
  await st.default()
  engine = new st.Engine()
})()

interface Artifact { key: string; out: string; mime: string; bytes: Uint8Array; info: unknown; parts: Record<string, Uint8Array> }

function collect(result: any): Artifact[] {
  const e = engine!
  const out: Artifact[] = []
  for (const f of result.fresh ?? []) {
    let mime = ''
    let bytes: Uint8Array<ArrayBuffer> = new Uint8Array(0)
    try { mime = e.mime(f.key, f.out); bytes = e.artifact(f.key, f.out) as Uint8Array<ArrayBuffer> } catch { /* parts with several files have no single file */ }
    const infoText = e.info(f.key, f.out)
    const info = infoText ? JSON.parse(infoText) : null
    const parts: Record<string, Uint8Array> = {}
    for (const name of Object.keys(info?.files ?? {})) parts[name] = e.part(f.key, f.out, name)
    if (info?.kind === 'solid') parts.plate = bytes
    out.push({ key: f.key, out: f.out, mime, bytes, info, parts })
  }
  return out
}

interface GNode { id: string; type: string }
interface GEdge { from: [string, string]; to: [string, string] }

/** Parents-first order of `targets` and everything they depend on, or null when the graph has a cycle. */
function plan(nodes: GNode[], edges: GEdge[], targets: string[] | null): string[] | null {
  const ids = new Set(nodes.map((n) => n.id))
  const ups = new Map<string, string[]>()
  for (const e of edges) if (ids.has(e.from[0]) && ids.has(e.to[0])) ups.set(e.to[0], [...(ups.get(e.to[0]) ?? []), e.from[0]])
  const want = new Set<string>()
  const visit = (id: string) => { if (want.has(id)) return; want.add(id); (ups.get(id) ?? []).forEach(visit) }
  ;(targets ?? nodes.map((n) => n.id)).forEach((id) => ids.has(id) && visit(id))
  const order: string[] = []
  const done = new Set<string>()
  let progressed = true
  while (progressed) {
    progressed = false
    for (const n of nodes) {
      if (!want.has(n.id) || done.has(n.id)) continue
      if ((ups.get(n.id) ?? []).every((u) => done.has(u))) { done.add(n.id); order.push(n.id); progressed = true }
    }
  }
  return order.length === want.size ? order : null
}

let runToken = 0
const turn = () => new Promise<void>((r) => setTimeout(r, 0))

function post(id: number, step: unknown, artifacts: Artifact[] = []) {
  const transfer: Transferable[] = []
  for (const a of artifacts) {
    transfer.push(a.bytes.buffer as ArrayBuffer)
    for (const p of Object.values(a.parts)) if (p.buffer !== a.bytes.buffer) transfer.push(p.buffer as ArrayBuffer)
  }
  ;(self as any).postMessage({ id, step: { ...(step as object), artifacts } }, transfer)
}

/**
 * Evaluates one node at a time and gives the message loop a turn in between, so a newer request can take over at the
 * next node boundary. Cached ancestors are free, so running a node alone costs only that node.
 */
async function runGraph(m: Extract<Req, { op: 'run' }>) {
  const e = engine!
  const mine = ++runToken
  const graph = JSON.parse(m.graph)
  const targets: string[] | null = m.targets ? JSON.parse(m.targets) : null
  const nodes: GNode[] = graph.nodes ?? []
  const edges: GEdge[] = graph.edges ?? []
  const order = plan(nodes, edges, targets)
  const t0 = Date.now()
  if (!order) {
    // let the engine report what is wrong (cycle, unknown type, ...)
    const r = JSON.parse(e.run(m.graph, m.targets))
    for (const [node, state] of Object.entries<any>(r.nodes ?? {})) post(m.id, { node, state, log: [], n: 0, total: 0 })
    return { ok: { status: r.status, error: r.error, log: r.log } }
  }
  const byId = new Map(nodes.map((n) => [n.id, n]))
  const failed = new Set<string>()
  let n = 0
  for (const id of order) {
    await turn()
    if (mine !== runToken) return { error: 'superseded' }
    const node = byId.get(id)!
    const upstream = edges.some((x) => x.to[0] === id && failed.has(x.from[0]))
    if (upstream) {
      const msg = 'an earlier node failed: fix that one first'
      failed.add(id)
      post(m.id, { node: id, state: { state: 'error', error: msg }, log: [{ at: Date.now(), t: (Date.now() - t0) / 1000, level: 'error', node: id, type: node.type, msg, ms: null }], n: ++n, total: order.length })
      continue
    }
    post(m.id, { node: id, state: { state: 'running' }, log: [], n, total: order.length })
    await turn()
    if (mine !== runToken) return { error: 'superseded' }
    let last = performance.now()
    e.set_progress((label: string, frac: number) => {
      const now = performance.now()
      const line = { at: Date.now(), t: (Date.now() - t0) / 1000, level: 'info', node: id, type: node.type, msg: `↳ ${label}`, ms: Math.round(now - last) }
      last = now
      post(m.id, { node: id, state: { state: 'running', progress: frac, message: label }, log: [line], n, total: order.length })
    })
    let r: any
    try { r = JSON.parse(e.run(m.graph, JSON.stringify([id]))) } finally { e.set_progress(undefined) }
    if (r.status === 'error') return { ok: { status: 'error', error: r.error, log: [] } }
    const state = r.nodes[id]
    if (state.state === 'error') failed.add(id)
    post(m.id, { node: id, state, log: (r.log ?? []).filter((l: any) => l.node === id), n: ++n, total: order.length }, collect(r))
  }
  return { ok: { status: 'done', log: [] } }
}

self.onmessage = async (ev: MessageEvent<Req>) => {
  const m = ev.data
  try {
    await ready
    const e = engine!
    if (m.op === 'types') return self.postMessage({ id: m.id, ok: JSON.parse(e.node_types()) })
    if (m.op === 'has') return self.postMessage({ id: m.id, ok: e.has_image(m.imageId) })
    if (m.op === 'imageinfo') return self.postMessage({ id: m.id, ok: JSON.parse(e.image_info(m.imageId, m.resolution)) })
    if (m.op === 'image') return self.postMessage({ id: m.id, ok: JSON.parse(e.add_image(new Uint8Array(m.bytes))) })
    if (m.op === 'cancel') { runToken++; return }
    if (m.op === 'threads') return self.postMessage({ id: m.id, ok: { n: threads, why, version: e.version() } })
    self.postMessage({ id: m.id, ...(await runGraph(m)) })
  } catch (err) {
    // a Rust panic surfaces as a WebAssembly.RuntimeError and leaves the module unusable
    if (err instanceof WebAssembly.RuntimeError) {
      let reason = ''
      try { reason = engine?.last_panic() ?? '' } catch { /* the module is already dead */ }
      const detail = reason || err.message
      return self.postMessage({ id: m.id, fatal: true, error: 'The engine crashed (' + detail + '). It was restarted; run again.' })
    }
    self.postMessage({ id: m.id, error: String((err as any)?.message ?? err) })
  }
}
