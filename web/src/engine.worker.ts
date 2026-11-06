import type { Engine } from '../pkg/stencil_wasm.js'
import { plan, type GEdge, type GNode } from './plan'

import type { Artifact, ArtifactInfo, EngineRunResult, WorkerLog, WorkerRequest, WorkerResponse, WorkerResults, WorkerStep, WorkerSuccess } from './workerProtocol'

// Use the worker overload, not the Window.postMessage overload from the shared DOM compilation.
const send = (message: WorkerResponse, transfer: Transferable[] = []) =>
  (self as unknown as { postMessage(message: WorkerResponse, transfer: Transferable[]): void }).postMessage(message, transfer)
const result = <K extends WorkerRequest['op']>(m: { id: number; op: K }, ok: WorkerResults[K], transfer: Transferable[] = []) =>
  send({ kind: 'result', id: m.id, op: m.op, ok } as WorkerSuccess, transfer)

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

function collect(result: EngineRunResult): Artifact[] {
  const e = engine!
  const out: Artifact[] = []
  for (const f of result.fresh ?? []) {
    let mime = ''
    let bytes: Uint8Array<ArrayBuffer> = new Uint8Array(0)
    try { mime = e.mime(f.key, f.out); bytes = e.artifact(f.key, f.out) as Uint8Array<ArrayBuffer> } catch { /* parts with several files have no single file */ }
    const infoText = e.info(f.key, f.out)
    const info: ArtifactInfo | null = infoText ? JSON.parse(infoText) : null
    const parts: Record<string, Uint8Array<ArrayBuffer>> = {}
    // cut sheet files (info.cut_files) are not extracted here: they are made on demand with the 'part' op
    for (const name of [...Object.keys(info?.files ?? {}), ...(info?.tile_files ?? [])]) parts[name] = e.part(f.key, f.out, name) as Uint8Array<ArrayBuffer>
    if (info?.kind === 'solid') parts.plate = bytes
    out.push({ key: f.key, out: f.out, mime, bytes, info, parts })
  }
  return out
}

let runToken = 0
const turn = () => new Promise<void>((r) => setTimeout(r, 0))

function post(id: number, step: Omit<WorkerStep, 'artifacts'>, artifacts: Artifact[] = []) {
  const transfer = new Set<Transferable>()
  for (const a of artifacts) {
    transfer.add(a.bytes.buffer)
    for (const p of Object.values(a.parts)) transfer.add(p.buffer)
  }
  send({ kind: 'progress', id, step: { ...step, artifacts } }, [...transfer])
}

/**
 * Evaluates one node at a time and gives the message loop a turn in between, so a newer request can take over at the
 * next node boundary. Cached ancestors are free, so running a node alone costs only that node.
 */
async function runGraph(m: Extract<WorkerRequest, { op: 'run' }>): Promise<WorkerResults['run']> {
  const e = engine!
  const mine = ++runToken
  const graph = JSON.parse(m.graph)
  const targets: string[] | null = m.targets ? JSON.parse(m.targets) : null
  const nodes: GNode[] = graph.nodes ?? []
  const edges: GEdge[] = graph.edges ?? []
  const order = plan(nodes, edges, targets, m.costs)
  const t0 = Date.now()
  if (!order) {
    // let the engine report what is wrong (cycle, unknown type, ...)
    const r: EngineRunResult = JSON.parse(e.run(m.graph, m.targets))
    for (const [node, state] of Object.entries(r.nodes ?? {})) post(m.id, { node, state, log: [], n: 0, total: 0 })
    return { status: r.status, error: r.error, log: r.log }
  }
  const byId = new Map(nodes.map((n) => [n.id, n]))
  const failed = new Set<string>()
  let n = 0
  for (const id of order) {
    await turn()
    if (mine !== runToken) throw new Error('superseded')
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
    if (mine !== runToken) throw new Error('superseded')
    let last = performance.now()
    e.set_progress((label: string, frac: number) => {
      const now = performance.now()
      const line: WorkerLog = { at: Date.now(), t: (Date.now() - t0) / 1000, level: 'info', node: id, type: node.type, msg: `↳ ${label}`, ms: Math.round(now - last) }
      last = now
      post(m.id, { node: id, state: { state: 'running', progress: frac, message: label }, log: [line], n, total: order.length })
    })
    let r: EngineRunResult
    try { r = JSON.parse(e.run(m.graph, JSON.stringify([id]))) } finally { e.set_progress(undefined) }
    if (r.status === 'error') return { status: 'error', error: r.error, log: [] }
    const state = r.nodes[id]
    if (state.state === 'error') failed.add(id)
    post(m.id, { node: id, state, log: (r.log ?? []).filter((l) => l.node === id), n: ++n, total: order.length }, collect(r))
  }
  return { status: 'done', log: [] }
}

self.onmessage = async (ev: MessageEvent<WorkerRequest>) => {
  const m = ev.data
  try {
    await ready
    const e = engine!
    if (m.op === 'types') return result(m, JSON.parse(e.node_types()) as WorkerResults['types'])
    if (m.op === 'has') return result(m, e.has_image(m.imageId))
    if (m.op === 'imageinfo') return result(m, JSON.parse(e.image_info(m.imageId, m.resolution)) as WorkerResults['imageinfo'])
    if (m.op === 'image') return result(m, JSON.parse(e.add_image(new Uint8Array(m.bytes))) as WorkerResults['image'])
    if (m.op === 'model') return result(m, JSON.parse(e.add_model(new Uint8Array(m.bytes), m.format)) as WorkerResults['model'])
    if (m.op === 'modelinfo') return result(m, JSON.parse(e.model_info(m.modelId, m.format)) as WorkerResults['modelinfo'])
    if (m.op === 'modelpreview' || m.op === 'modelrepair') {
      const b = (m.op === 'modelpreview' ? e.model_preview(m.modelId, m.format) : e.repair_model(m.modelId, m.format, m.action, m.tolerance)) as Uint8Array<ArrayBuffer>
      return result(m, b, [b.buffer])
    }
    if (m.op === 'font') { e.add_font(m.name, new Uint8Array(m.bytes)); return result(m, true) }
    if (m.op === 'hasfont') return result(m, e.has_font(m.name))
    if (m.op === 'part') {
      const b = e.part(m.key, m.out, m.name) as Uint8Array<ArrayBuffer>
      return result(m, b, [b.buffer])
    }
    if (m.op === 'dxf') {
      const b = e.dxf(m.key, m.out) as Uint8Array<ArrayBuffer>
      return result(m, b, [b.buffer])
    }
    if (m.op === 'cancel') { runToken++; return }
    if (m.op === 'threads') return result(m, { n: threads, why, version: e.version() })
    result(m, await runGraph(m))
  } catch (err) {
    // a Rust panic surfaces as a WebAssembly.RuntimeError and leaves the module unusable
    if (err instanceof WebAssembly.RuntimeError) {
      let reason = ''
      try { reason = engine?.last_panic() ?? '' } catch { /* the module is already dead */ }
      const detail = reason || err.message
      return send({ kind: 'error', id: m.id, fatal: true, error: 'The engine crashed (' + detail + '). It was restarted; run again.' })
    }
    send({ kind: 'error', id: m.id, error: err instanceof Error ? err.message : String(err) })
  }
}
