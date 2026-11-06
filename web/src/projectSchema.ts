import type { Edge } from '@xyflow/react'
import type { StudioNode } from './store'

export interface GraphSnapshot { nodes: StudioNode[]; edges: Edge[] }
interface SourceFile { name: string; base64: string }
interface ModelFile extends SourceFile { format: 'stl' | 'obj' }
export interface ProjectFile {
  version: 2
  graph: GraphSnapshot
  images: Record<string, SourceFile>
  models: Record<string, ModelFile>
}

const record = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)
const text = (v: unknown): v is string => typeof v === 'string' && v.length > 0
const finite = (v: unknown) => typeof v === 'number' && Number.isFinite(v)
function requireValid(ok: unknown, path: string): asserts ok {
  if (!ok) throw new Error(`Invalid project: ${path}`)
}

/** Checks graph structure, not the current node catalogue: legacy and plugin node types remain loadable. */
export function validateGraphSnapshot(value: unknown): GraphSnapshot {
  requireValid(record(value), 'graph must be an object')
  requireValid(Array.isArray(value.nodes) && Array.isArray(value.edges), 'graph needs nodes and edges arrays')
  const ids = new Set<string>()
  for (const [i, n] of value.nodes.entries()) {
    const path = `graph.nodes[${i}]`
    requireValid(record(n) && text(n.id) && !ids.has(n.id), `${path} needs a unique nonempty id`)
    ids.add(n.id)
    requireValid(n.type === undefined || n.type === 'studio', `${path}.type must be studio`)
    requireValid(record(n.position) && finite(n.position.x) && finite(n.position.y), `${path}.position must contain finite x/y`)
    requireValid(record(n.data) && text(n.data.type) && record(n.data.params), `${path}.data needs type and params`)
    requireValid(n.data.title === undefined || typeof n.data.title === 'string', `${path}.data.title must be a string`)
  }
  const edges = new Set<string>()
  for (const [i, e] of value.edges.entries()) {
    const path = `graph.edges[${i}]`
    requireValid(record(e) && text(e.id) && !edges.has(e.id), `${path} needs a unique nonempty id`)
    edges.add(e.id)
    requireValid(text(e.source) && ids.has(e.source) && text(e.target) && ids.has(e.target), `${path} references a missing node`)
    for (const key of ['sourceHandle', 'targetHandle']) requireValid(e[key] == null || typeof e[key] === 'string', `${path}.${key} must be a string or null`)
  }
  return value as unknown as GraphSnapshot
}

function sources(value: unknown, models: boolean): Record<string, SourceFile | ModelFile> {
  if (value === undefined) return {}
  requireValid(record(value), `${models ? 'models' : 'images'} must be an object`)
  for (const [id, file] of Object.entries(value)) {
    requireValid(text(id) && record(file) && typeof file.name === 'string' && typeof file.base64 === 'string', `source ${id} needs name and base64 strings`)
    // atob accepts unpadded historical base64, but rejects corrupt data before any sources are imported.
    try { atob(file.base64) } catch { throw new Error(`Invalid project: source ${id} has malformed base64`) }
    if (models) requireValid(file.format === 'stl' || file.format === 'obj', `model ${id} format must be stl or obj`)
  }
  return value as unknown as Record<string, SourceFile | ModelFile>
}

function parseVersion2(value: Record<string, unknown>): ProjectFile {
  return {
    version: 2,
    graph: validateGraphSnapshot(value.graph),
    images: sources(value.images, false),
    models: sources(value.models, true) as Record<string, ModelFile>,
  }
}

/** Versionless and v1 files used the same graph envelope, before model sources were added. */
function migrateVersion1(value: Record<string, unknown>): ProjectFile {
  return parseVersion2({ ...value, version: 2, models: value.models === undefined ? {} : value.models })
}

export function parseProject(value: unknown): ProjectFile {
  requireValid(record(value), 'file must be an object')
  switch (value.version) {
    case undefined: return migrateVersion1(value)
    case 1: return migrateVersion1(value)
    case 2: return parseVersion2(value)
    default:
      throw new Error(`Unsupported project version: ${String(value.version)}`)
  }
}
