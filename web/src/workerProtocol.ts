import type { ImageInfo, ModelInfo, NodeRun, NodeType } from './api'

export interface WorkerRequests {
  types: { op: 'types' }
  image: { op: 'image'; bytes: ArrayBuffer }
  model: { op: 'model'; bytes: ArrayBuffer; format: string }
  modelinfo: { op: 'modelinfo'; modelId: string; format: string }
  modelpreview: { op: 'modelpreview'; modelId: string; format: string }
  modelrepair: { op: 'modelrepair'; modelId: string; format: string; action: string; tolerance: number }
  has: { op: 'has'; imageId: string }
  imageinfo: { op: 'imageinfo'; imageId: string; resolution: number }
  run: { op: 'run'; graph: string; targets: string; costs?: Record<string, number> }
  part: { op: 'part'; key: string; out: string; name: string }
  dxf: { op: 'dxf'; key: string; out: string }
  cancel: { op: 'cancel' }
  threads: { op: 'threads' }
  font: { op: 'font'; name: string; bytes: ArrayBuffer }
  hasfont: { op: 'hasfont'; name: string }
}

export interface WorkerResults {
  types: NodeType[]
  image: { id: string; width: number; height: number }
  model: { id: string }
  modelinfo: ModelInfo
  modelpreview: Uint8Array<ArrayBuffer>
  modelrepair: Uint8Array<ArrayBuffer>
  has: boolean
  imageinfo: ImageInfo
  run: { status: string; error?: string; log?: WorkerLog[] }
  part: Uint8Array<ArrayBuffer>
  dxf: Uint8Array<ArrayBuffer>
  cancel: undefined
  threads: { n: number; why: string; version: string }
  font: boolean
  hasfont: boolean
}

export type WorkerOp = keyof WorkerRequests
export type WorkerCommand = WorkerRequests[WorkerOp]
export type WorkerRequest = WorkerCommand & { id: number }
export interface WorkerLog {
  at: number; ms: number | null; t: number; level: 'info' | 'error'; node: string; type: string; msg: string
}
export interface ArtifactInfo extends Record<string, unknown> {
  files?: Record<string, unknown>
  tile_files?: string[]
  kind?: string
}
export interface Artifact {
  key: string; out: string; mime: string; bytes: Uint8Array<ArrayBuffer>
  info: ArtifactInfo | null; parts: Record<string, Uint8Array<ArrayBuffer>>
}
export interface WorkerStep {
  node: string; state: NodeRun; n: number; total: number; log: WorkerLog[]; artifacts: Artifact[]
}
export type WorkerSuccess = {
  [K in WorkerOp]: { kind: 'result'; id: number; op: K; ok: WorkerResults[K] }
}[WorkerOp]
export type WorkerResponse =
  | WorkerSuccess
  | { kind: 'progress'; id: number; step: WorkerStep }
  | { kind: 'error'; id: number; error: string; fatal?: boolean }

/** JSON emitted by the engine before artifact bytes are collected. */
export interface EngineRunResult {
  status: string
  error?: string
  nodes: Record<string, NodeRun>
  log?: WorkerLog[]
  fresh?: { key: string; out: string }[]
}
