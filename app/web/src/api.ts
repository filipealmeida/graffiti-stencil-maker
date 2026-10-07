export type Kind = 'image' | 'mask' | 'solid' | 'parts' | 'any'
export type Stage = 'trace' | 'stencil' | 'post'

export interface ParamDef {
  name: string
  label: string
  kind: 'int' | 'float' | 'bool' | 'choice' | 'text'
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
  outputs?: Record<string, { type: Kind; available: boolean }>
}
export interface GraphJson {
  nodes: { id: string; type: string; params: Record<string, unknown> }[]
  edges: { from: [string, string]; to: [string, string] }[]
}

async function j<T>(r: Response): Promise<T> {
  if (!r.ok) throw new Error((await r.json().catch(() => ({ detail: r.statusText }))).detail ?? r.statusText)
  return r.json()
}

export const api = {
  nodeTypes: () => fetch('/api/node-types').then((r) => j<NodeType[]>(r)),
  upload: (f: File) => {
    const fd = new FormData()
    fd.append('file', f)
    return fetch('/api/images', { method: 'POST', body: fd }).then((r) => j<{ id: string; width: number; height: number; name: string }>(r))
  },
  run: (graph: GraphJson, targets?: string[]) =>
    fetch('/api/run', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ graph, targets }) }).then((r) => j<{ job: string }>(r)),
  job: (id: string) => fetch(`/api/jobs/${id}`).then((r) => j<{ status: string; nodes: Record<string, NodeRun>; error?: string }>(r)),
  info: (key: string, out: string) => fetch(`/api/info/${key}/${out}`).then((r) => j<Record<string, any>>(r)),
  projects: () => fetch('/api/projects').then((r) => j<string[]>(r)),
  loadProject: (n: string) => fetch(`/api/projects/${encodeURIComponent(n)}`).then((r) => j<any>(r)),
  saveProject: (n: string, body: unknown) =>
    fetch(`/api/projects/${encodeURIComponent(n)}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).then((r) => j<unknown>(r)),
}

export const artifactUrl = (key: string, out: string, size = 0) => `/api/artifact/${key}/${out}${size ? `?size=${size}` : ''}`
