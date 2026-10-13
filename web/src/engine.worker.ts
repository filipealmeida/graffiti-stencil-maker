import init, { Engine } from '../pkg/stencil_wasm.js'

type Req =
  | { id: number; op: 'types' }
  | { id: number; op: 'image'; bytes: ArrayBuffer }
  | { id: number; op: 'has'; imageId: string }
  | { id: number; op: 'run'; graph: string; targets: string }

let engine: Engine | null = null
const ready = init().then(() => { engine = new Engine() })

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

self.onmessage = async (ev: MessageEvent<Req>) => {
  const m = ev.data
  try {
    await ready
    const e = engine!
    if (m.op === 'types') return self.postMessage({ id: m.id, ok: JSON.parse(e.node_types()) })
    if (m.op === 'has') return self.postMessage({ id: m.id, ok: e.has_image(m.imageId) })
    if (m.op === 'image') return self.postMessage({ id: m.id, ok: JSON.parse(e.add_image(new Uint8Array(m.bytes))) })
    const result = JSON.parse(e.run(m.graph, m.targets))
    const artifacts = collect(result)
    const transfer: Transferable[] = []
    for (const a of artifacts) {
      transfer.push(a.bytes.buffer as ArrayBuffer)
      for (const p of Object.values(a.parts)) if (p.buffer !== a.bytes.buffer) transfer.push(p.buffer as ArrayBuffer)
    }
    (self as any).postMessage({ id: m.id, ok: { result, artifacts } }, transfer)
  } catch (err) {
    self.postMessage({ id: m.id, error: String((err as any)?.message ?? err) })
  }
}
