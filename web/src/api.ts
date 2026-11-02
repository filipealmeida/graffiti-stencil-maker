export type Kind = 'image' | 'mask' | 'shapes' | 'tiles' | 'solid' | 'parts' | 'any' | 'body' | 'item' | 'layer'
export type Stage = 'trace' | 'stencil' | 'post'

import { plan, splitPlan } from './plan'

export interface ParamDef {
  name: string
  label: string
  kind: 'int' | 'float' | 'bool' | 'choice' | 'text' | 'string'
  default: number | boolean | string
  min?: number
  max?: number
  step?: number
  choices?: string[]
  group?: string | null
}
export interface NodeType {
  id: string
  label: string
  stage: Stage
  inputs: { name: string; type: Kind; required: boolean }[]
  outputs: { name: string; type: Kind }[]
  params: ParamDef[]
  doc: string
  legacy?: boolean
}
export interface NodeRun {
  state: 'pending' | 'running' | 'done' | 'error'
  key?: string
  cached?: boolean
  progress?: number
  message?: string
  error?: string
  ms?: number
  stale?: boolean
  outputs?: Record<string, { type: Kind; available: boolean }>
}
export interface NodeStep { node: string; state: NodeRun; n: number; total: number; log: LogLine[] }
export interface LogLine { ts: string; ms: number | null; t: number; level: 'info' | 'error'; node: string; type: string; msg: string }
export interface GraphJson {
  nodes: { id: string; type: string; params: Record<string, unknown> }[]
  edges: { from: [string, string]; to: [string, string] }[]
}

type Info = Record<string, any>

/** One engine worker. The second one exists only while a run has branches worth computing side by side. */
interface Eng {
  w: Worker
  active: { id: number; since: number | null } | null
  // after a (re)start the worker must be given the pictures and fonts again before it can run anything
  warm: Promise<unknown> | null
}
const engines: (Eng | null)[] = [null, null]
let seq = 0
const pending = new Map<number, { ok: (v: any) => void; err: (e: Error) => void; step?: (s: any) => void; eng: number }>()

/** A node that has been computing this long (ms) is worth a worker restart; shorter ones finish on their own and keep the cache. */
export const HARD_CANCEL_MS = 300

function engine(i: number): Eng {
  if (engines[i]) return engines[i]!
  const w = new Worker(new URL('./engine.worker.ts', import.meta.url), { type: 'module' })
  const e: Eng = { w, active: null, warm: null }
  engines[i] = e
  w.onmessage = (ev) => {
    const p = pending.get(ev.data.id)
    if (!p) return
    if (ev.data.step) {
      if (e.active && e.active.id === ev.data.id) e.active.since = ev.data.step.state?.state === 'running' ? Date.now() : null
      return p.step?.(ev.data.step)
    }
    if (e.active?.id === ev.data.id) e.active = null
    pending.delete(ev.data.id)
    if (ev.data.error !== undefined) p.err(new Error(ev.data.error))
    else p.ok(ev.data.ok)
    if (ev.data.fatal) {
      // the crashed module cannot be reused: replace the worker and hand it the pictures again
      dropEngine(i, new Error('The engine crashed and was restarted; run again.'))
      const n = engine(i)
      n.warm = replay(i).finally(() => { n.warm = null })
    }
  }
  if (i > 0) {
    // a fresh helper engine needs the session's pictures and fonts before it can compute anything
    e.warm = replay(i).finally(() => { e.warm = null })
  }
  return e
}

/** Terminates engine `i` and rejects whatever it was asked for. */
function dropEngine(i: number, why: Error) {
  const e = engines[i]
  if (!e) return
  e.w.terminate()
  engines[i] = null
  seenSigs[i].clear()
  for (const [id, p] of [...pending]) {
    if (p.eng !== i) continue
    pending.delete(id)
    p.err(why)
  }
}

function callOn<T>(i: number, msg: Record<string, unknown>, transfer: Transferable[] = [], step?: (s: any) => void): Promise<T> {
  const e = engine(i)
  const id = ++seq
  if (msg.op === 'run') e.active = { id, since: null }
  return new Promise<T>((ok, err) => {
    pending.set(id, { ok, err, step, eng: i })
    e.w.postMessage({ id, ...msg }, transfer)
  })
}

const call = <T,>(msg: Record<string, unknown>, transfer: Transferable[] = [], step?: (s: any) => void) => callOn<T>(0, msg, transfer, step)

/** Sends the message to every live engine (pictures and fonts must exist everywhere). */
async function broadcast<T>(msg: Record<string, unknown>, bytes?: ArrayBuffer): Promise<T> {
  const first = await callOn<T>(0, bytes ? { ...msg, bytes: bytes.slice(0) } : msg)
  for (let i = 1; i < engines.length; i++) if (engines[i]) await callOn(i, bytes ? { ...msg, bytes: bytes.slice(0) } : msg)
  return first
}

/** Hands engine `i` every stored picture and loaded font, as after a restart. */
async function replay(i: number) {
  const keys = (await idb('readonly', (s) => s.getAllKeys())) as string[]
  for (const k of keys) {
    const img = (await idb('readonly', (s) => s.get(k))) as StoredImage | undefined
    if (img) await callOn(i, { op: 'image', bytes: img.bytes.slice(0) })
  }
  for (const [name, bytes] of fontBytes) await callOn(i, { op: 'font', name, bytes: bytes.slice(0) })
}

// how long each node took when it last computed (cache hits don't count): the worker runs quick nodes first
const costs: Record<string, number> = {}

// which engine computed each artifact key, so on-demand files are asked of the engine that holds the result
const keyOwners = new Map<string, number>()
const ownerOf = (key: string) => {
  const i = keyOwners.get(key) ?? 0
  return engines[i] ? i : 0
}

// what each engine has computed, as node signatures (params and inputs), to price what a split would redo
const seenSigs: Map<string, Set<string>>[] = [new Map(), new Map()]

/** Each node's own identity in this graph: its type, parameters and where its inputs come from. */
function sigsOf(graph: GraphJson): Map<string, string> {
  const ins = new Map<string, string[]>()
  for (const e of graph.edges ?? []) {
    const l = ins.get(e.to[0]) ?? []
    l.push(`${e.to[1]}<${e.from[0]}:${e.from[1]}`)
    ins.set(e.to[0], l)
  }
  const m = new Map<string, string>()
  for (const n of graph.nodes ?? []) m.set(n.id, `${n.type}|${JSON.stringify(n.params ?? {})}|${(ins.get(n.id) ?? []).sort().join(',')}`)
  return m
}

/** Whether engine `i` most likely still caches `id`: it computed this signature before, and every input too. */
function heldBy(i: number, id: string, sigs: Map<string, string>, parents: Map<string, string[]>, memo = new Map<string, boolean>()): boolean {
  const hit = memo.get(id)
  if (hit !== undefined) return hit
  memo.set(id, false) // a cycle is never held
  const ok = (seenSigs[i].get(id)?.has(sigs.get(id) ?? '') ?? false)
    && (parents.get(id) ?? []).every((p) => heldBy(i, p, sigs, parents, memo))
  memo.set(id, ok)
  return ok
}

/**
 * Stops the current run. A node caught mid-computation can't hear a message, so its worker is replaced instead.
 * `force` replaces it whenever a node is computing, however short that has been going on.
 */
function stopRun(force = false) {
  for (let i = 0; i < engines.length; i++) {
    const e = engines[i]
    if (!e || !e.active) continue
    if (e.active.since !== null && (force || Date.now() - e.active.since > HARD_CANCEL_MS)) {
      dropEngine(i, new Error('superseded'))
      if (i === 0) {
        const n = engine(0)
        n.warm = replay(0).finally(() => { n.warm = null })
      }
    } else e.w.postMessage({ id: ++seq, op: 'cancel' })
  }
}

/** What the engine workers are doing right now, for the performance overlay. */
export const engineStatus = () => ({ workers: engines.filter(Boolean).length, busy: pending.size > 0 })

// Results live in the browser: artifacts are blobs, infos are cached JSON.
const blobs = new Map<string, string>()
const MAX_LIVE_KEYS = 48
const liveKeys: string[] = []
function retain(key: string) {
  const at = liveKeys.indexOf(key)
  if (at >= 0) liveKeys.splice(at, 1)
  liveKeys.push(key)
  while (liveKeys.length > MAX_LIVE_KEYS) {
    const old = liveKeys.shift()!
    for (const k of [...blobs.keys()]) if (k.startsWith(`${old}/`)) { URL.revokeObjectURL(blobs.get(k)!); blobs.delete(k) }
    for (const k of [...infos.keys()]) if (k.startsWith(`${old}/`)) infos.delete(k)
  }
}
const infos = new Map<string, Info>()

const IDB = 'stencil-studio'
function db(): Promise<IDBDatabase> {
  return new Promise((ok, err) => {
    const r = indexedDB.open(IDB, 1)
    r.onupgradeneeded = () => r.result.createObjectStore('images')
    r.onsuccess = () => ok(r.result)
    r.onerror = () => err(r.error)
  })
}
async function idb<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const d = await db()
  return new Promise((ok, err) => {
    const r = f(d.transaction('images', mode).objectStore('images'))
    r.onsuccess = () => ok(r.result)
    r.onerror = () => err(r.error)
  })
}

export interface ImageInfo {
  format: string; bytes: number; width: number; height: number; megapixels: number; aspect: number
  colour_model?: string; dpi?: [number, number]; print_size_mm?: [number, number]
  svg_width?: string; svg_height?: string; svg_viewbox?: string; svg_title?: string
  working: [number, number]; scale: number; mean_brightness: number; brightness_range: [number, number]
  otsu_threshold: number; dark_percent: number; colours: number
}
export interface StoredImage { name: string; bytes: ArrayBuffer }

const pad = (n: number, w = 2) => String(n).padStart(w, '0')
const stamp = (t: number) => {
  const d = new Date(t)
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`
}

const b64 = (b: ArrayBuffer) => {
  let s = ''
  const u = new Uint8Array(b)
  for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000))
  return btoa(s)
}
const unb64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0)).buffer

/** Fonts installed on this computer, through the browser's Local Font Access API (Chromium; asks permission once). */
interface LocalFont { postscriptName: string; fullName: string; family: string; style: string; blob: () => Promise<Blob> }
const fontBytes = new Map<string, ArrayBuffer>()
let localFonts: LocalFont[] | null = null
async function listFonts(): Promise<LocalFont[]> {
  const q = (window as any).queryLocalFonts as (() => Promise<LocalFont[]>) | undefined
  if (!q) throw new Error('This browser cannot list system fonts. Use a Chromium-based browser (Chrome, Edge, Brave).')
  if (!localFonts) localFonts = await q.call(window)
  return localFonts
}
async function loadFont(name: string) {
  if (!fontBytes.has(name)) {
    const f = (await listFonts()).find((x) => x.postscriptName === name)
    if (!f) throw new Error(`The font '${name}' is not installed on this computer.`)
    fontBytes.set(name, await (await f.blob()).arrayBuffer())
  }
  await broadcast({ op: 'font', name }, fontBytes.get(name)!)
}

interface RunResult { status: string; error?: string }

/** Registers a step's artifacts: blob URLs, infos, costs, and which engine holds the result. */
function absorb(eng: number, s: any) {
  if (s.state?.state === 'done' && !s.state.cached && s.state.ms > 0) costs[s.node] = s.state.ms
  for (const a of s.artifacts as any[]) {
    retain(a.key)
    const prev = blobs.get(`${a.key}/${a.out}`)
    if (prev) URL.revokeObjectURL(prev)
    if (a.bytes.length) blobs.set(`${a.key}/${a.out}`, URL.createObjectURL(new Blob([a.bytes as BlobPart], { type: a.mime })))
    for (const [name, bytes] of Object.entries<Uint8Array>(a.parts)) blobs.set(`${a.key}/${a.out}/${name}`, URL.createObjectURL(new Blob([bytes as BlobPart], { type: name.endsWith('.svg') ? 'image/svg+xml' : name.endsWith('.dxf') ? 'image/vnd.dxf' : 'model/stl' })))
    if (a.info) infos.set(`${a.key}/${a.out}`, a.info)
    keyOwners.set(a.key, eng)
  }
}

const logLine = (l: any) => ({ ts: stamp(l.at), ms: l.ms ?? null, t: l.t, level: l.level, node: l.node, type: l.type, msg: l.msg })

export const api = {
  /** The system's fonts, one entry per face. Must be called from a click the first time, to get the browser's permission. */
  async systemFonts() {
    return (await listFonts()).map((f) => ({ name: f.postscriptName, label: f.fullName || f.postscriptName, family: f.family })).sort((x, y) => x.label.localeCompare(y.label))
  },
  useFont: loadFont,
  canListFonts: () => typeof (window as any).queryLocalFonts === 'function',
  nodeTypes: () => call<NodeType[]>({ op: 'types' }),
  threads: () => call<{ n: number; why: string; version: string }>({ op: 'threads' }),
  imageInfo: (imageId: string, resolution: number) => call<ImageInfo>({ op: 'imageinfo', imageId, resolution }),
  async upload(f: File) {
    const bytes = await f.arrayBuffer()
    const info = await broadcast<{ id: string; width: number; height: number }>({ op: 'image' }, bytes)
    await idb('readwrite', (s) => s.put({ name: f.name, bytes } satisfies StoredImage, info.id))
    return { ...info, name: f.name }
  },
  async storedImage(id: string) {
    return (await idb('readonly', (s) => s.get(id))) as StoredImage | undefined
  },
  /** Gives every engine every image kept from earlier sessions. */
  async restoreImages() {
    for (let i = 0; i < engines.length; i++) if (i === 0 || engines[i]) await replay(i)
  },
  /**
   * Starts a run, which evaluates node by node and reports each one through `onStep` as it finishes.
   * Branches that cost enough on the last run go to a second engine and compute side by side.
   * A newer run (or `cancel`) stops an older one at its next node boundary and rejects it with 'superseded'.
   */
  async run(graph: GraphJson, targets: string[] | undefined, onStep: (s: NodeStep) => void) {
    stopRun()
    const e0 = engine(0)
    if (e0.warm) await e0.warm
    // a project that names a system font needs it in the engine; try quietly, a missing font shows up as the node's error
    for (const n of (graph as any).nodes ?? []) {
      const name = n.type === 'text' ? String(n.params?.system_font ?? '') : ''
      if (name && !fontBytes.has(name)) await loadFont(name).catch(() => {})
    }
    const graphText = JSON.stringify(graph)
    const gNodes = (graph.nodes ?? []).map((n) => ({ id: n.id, type: n.type }))
    const order = plan(gNodes, graph.edges ?? [], targets ?? null, costs)
    const sigs = sigsOf(graph)
    const parents = new Map<string, string[]>()
    for (const e of graph.edges ?? []) parents.set(e.to[0], [...(parents.get(e.to[0]) ?? []), e.from[0]])
    const memo = new Map<string, boolean>()
    const extra = (id: string) => (heldBy(1, id, sigs, parents, memo) ? 0 : costs[id] ?? 0)
    const record = (i: number, s: any) => {
      if (s.state?.state !== 'done') return
      const set = seenSigs[i].get(s.node) ?? new Set<string>()
      set.add(sigs.get(s.node) ?? '')
      // a draft and a full run alternate, so a node keeps its last two shapes
      while (set.size > 2) set.delete(set.values().next().value!)
      seenSigs[i].set(s.node, set)
    }
    const split = order && (window as any).__noSplit !== true ? splitPlan(graph.edges ?? [], order, costs, extra) : null
    if (!split) {
      return await callOn<RunResult>(0, { op: 'run', graph: graphText, targets: targets ? JSON.stringify(targets) : '', costs }, [], (s) => {
        absorb(0, s)
        record(0, s)
        onStep({ node: s.node, state: s.state, n: s.n, total: s.total, log: (s.log as any[]).map(logLine) })
      })
    }
    const e1 = engine(1)
    if (e1.warm) await e1.warm
    // each engine computes what its targets need in its own cache; a node shared by both is reported by its owner only
    const done = new Set<string>()
    const total = order!.length
    const stepOf = (i: number) => (s: any) => {
      absorb(i, s)
      record(i, s)
      if (split.owner.get(s.node) !== i) return
      if (s.state?.state === 'done' || s.state?.state === 'error') done.add(s.node)
      onStep({ node: s.node, state: s.state, n: done.size, total, log: (s.log as any[]).map(logLine) })
    }
    try {
      const [r0, r1] = await Promise.all([0, 1].map((i) =>
        callOn<RunResult>(i, { op: 'run', graph: graphText, targets: JSON.stringify(split.targets[i]), costs }, [], stepOf(i))))
      return r0.status === 'error' ? r0 : r1
    } catch (err) {
      stopRun()
      throw err
    }
  },
  cancel: () => stopRun(),
  /** Stops whatever is computing right now, even a node in the middle of its work. */
  stop: () => stopRun(true),
  info: async (key: string, out: string) => infos.get(`${key}/${out}`) ?? {},
  /** One file of a finished output, made by the engine that holds it, on demand (e.g. the cut sheet preview). */
  part: (key: string, out: string, name: string) => callOn<Uint8Array>(ownerOf(key), { op: 'part', key, out, name }),
  /** The DXF of an outlines output, drawn by the engine on demand. */
  dxf: (key: string, out: string) => callOn<Uint8Array>(ownerOf(key), { op: 'dxf', key, out }),
  /** The info of a finished output when it is already in memory. */
  peekInfo: (key: string, out: string) => infos.get(`${key}/${out}`),

  /** Projects are files: the graph plus the pictures it uses. */
  async exportProject(graph: unknown) {
    const used = new Set<string>()
    for (const n of (graph as any).nodes ?? []) if (n.data?.params?.image_id) used.add(String(n.data.params.image_id))
    const images: Record<string, { name: string; base64: string }> = {}
    for (const id of used) {
      const img = (await idb('readonly', (s) => s.get(id))) as StoredImage | undefined
      if (img) images[id] = { name: img.name, base64: b64(img.bytes) }
    }
    return { version: 2, graph, images }
  },
  async importProject(p: any) {
    const remap = new Map<string, string>()
    for (const [id, img] of Object.entries<{ name: string; base64: string }>(p.images ?? {})) {
      const bytes = unb64(img.base64)
      const got = await broadcast<{ id: string }>({ op: 'image' }, bytes)
      remap.set(id, got.id)
      await idb('readwrite', (s) => s.put({ name: img.name, bytes } satisfies StoredImage, got.id))
    }
    const g = p.graph as { nodes: any[]; edges: any[] }
    // older projects keep the picture ids of the engine that saved them
    for (const n of g.nodes ?? []) {
      const old = n.data?.params?.image_id
      if (old && remap.has(String(old))) n.data.params.image_id = remap.get(String(old))
    }
    return g
  },
}

const missing = ''
export const artifactUrl = (key: string, out: string, _size = 0) => blobs.get(`${key}/${out}`) ?? missing
export const partUrl = (key: string, out: string, name: string) => blobs.get(`${key}/${out}/${name}`) ?? missing

/** Hands `bytes` to the browser as a download named `name`. */
export function saveFile(name: string, bytes: Uint8Array, type: string) {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }))
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}
