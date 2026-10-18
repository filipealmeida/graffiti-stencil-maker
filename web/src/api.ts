export type Kind = 'image' | 'mask' | 'shapes' | 'solid' | 'parts' | 'any'
export type Stage = 'trace' | 'stencil' | 'post'

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

let worker: Worker | null = null
let seq = 0
const pending = new Map<number, { ok: (v: any) => void; err: (e: Error) => void; step?: (s: any) => void }>()

/** A node that has been computing this long (ms) is worth a worker restart; shorter ones finish on their own and keep the cache. */
export const HARD_CANCEL_MS = 300
let active: { id: number; since: number | null } | null = null
// after a restart the new worker must be given the pictures again before it can run anything
let warm: Promise<unknown> | null = null

function call<T>(msg: Record<string, unknown>, transfer: Transferable[] = [], step?: (s: any) => void): Promise<T> {
  if (!worker) {
    worker = new Worker(new URL('./engine.worker.ts', import.meta.url), { type: 'module' })
    worker.onmessage = (ev) => {
      const p = pending.get(ev.data.id)
      if (!p) return
      if (ev.data.step) {
        const a = active as { id: number; since: number | null } | null
        if (a && a.id === ev.data.id) a.since = ev.data.step.state?.state === 'running' ? Date.now() : null
        return p.step?.(ev.data.step)
      }
      if (active?.id === ev.data.id) active = null
      pending.delete(ev.data.id)
      if (ev.data.error !== undefined) p.err(new Error(ev.data.error))
      else p.ok(ev.data.ok)
    }
  }
  const id = ++seq
  if (msg.op === 'run') active = { id, since: null }
  return new Promise<T>((ok, err) => {
    pending.set(id, { ok, err, step })
    worker!.postMessage({ id, ...msg }, transfer)
  })
}

/** Stops the current run. A node caught mid-computation can't hear a message, so the worker is replaced instead. */
function stopRun() {
  if (!worker || !active) return
  if (active.since !== null && Date.now() - active.since > HARD_CANCEL_MS) {
    worker.terminate()
    worker = null
    active = null
    const dead = [...pending.values()]
    pending.clear()
    for (const p of dead) p.err(new Error('superseded'))
    warm = api.restoreImages().finally(() => { warm = null })
  } else worker.postMessage({ id: ++seq, op: 'cancel' })
}

/** What the engine worker is doing right now, for the performance overlay. */
export const engineStatus = () => ({ workers: worker ? 1 : 0, busy: pending.size > 0 })

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

export const api = {
  nodeTypes: () => call<NodeType[]>({ op: 'types' }),
  threads: () => call<number>({ op: 'threads' }),
  async upload(f: File) {
    const bytes = await f.arrayBuffer()
    const info = await call<{ id: string; width: number; height: number }>({ op: 'image', bytes: bytes.slice(0) }, [])
    await idb('readwrite', (s) => s.put({ name: f.name, bytes } satisfies StoredImage, info.id))
    return { ...info, name: f.name }
  },
  async storedImage(id: string) {
    return (await idb('readonly', (s) => s.get(id))) as StoredImage | undefined
  },
  /** Gives the engine every image kept from earlier sessions. */
  async restoreImages() {
    const keys = (await idb('readonly', (s) => s.getAllKeys())) as string[]
    for (const k of keys) {
      const img = (await idb('readonly', (s) => s.get(k))) as StoredImage | undefined
      if (img) await call({ op: 'image', bytes: img.bytes.slice(0) })
    }
  },
  /**
   * Starts a run in the worker, which evaluates node by node and reports each one through `onStep` as it finishes.
   * A newer run (or `cancel`) stops an older one at its next node boundary and rejects it with 'superseded'.
   */
  async run(graph: GraphJson, targets: string[] | undefined, onStep: (s: NodeStep) => void) {
    stopRun()
    if (warm) await warm
    const r = await call<{ status: string; error?: string }>(
      { op: 'run', graph: JSON.stringify(graph), targets: targets ? JSON.stringify(targets) : '' }, [],
      (s) => {
        for (const a of s.artifacts as any[]) {
          retain(a.key)
          const prev = blobs.get(`${a.key}/${a.out}`)
          if (prev) URL.revokeObjectURL(prev)
          if (a.bytes.length) blobs.set(`${a.key}/${a.out}`, URL.createObjectURL(new Blob([a.bytes as BlobPart], { type: a.mime })))
          for (const [name, bytes] of Object.entries<Uint8Array>(a.parts)) blobs.set(`${a.key}/${a.out}/${name}`, URL.createObjectURL(new Blob([bytes as BlobPart], { type: 'model/stl' })))
          if (a.info) infos.set(`${a.key}/${a.out}`, a.info)
        }
        onStep({
          node: s.node, state: s.state, n: s.n, total: s.total,
          log: (s.log as any[]).map((l) => ({ ts: stamp(l.at), ms: l.ms ?? null, t: l.t, level: l.level, node: l.node, type: l.type, msg: l.msg })),
        })
      })
    return r
  },
  cancel: stopRun,
  info: async (key: string, out: string) => infos.get(`${key}/${out}`) ?? {},
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
      const got = await call<{ id: string }>({ op: 'image', bytes: bytes.slice(0) })
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
