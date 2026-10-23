import { artifactUrl, type Kind, type NodeRun } from './api'
import { useStudio } from './store'

/** What a finished output can be downloaded as: one wording and one file name for the side panel and the preview. */
export function downloadFor(kind: Kind | undefined, key: string | undefined, out: string | undefined, files = 0) {
  if (!kind || !key || !out) return null
  const href = artifactUrl(key, out)
  if (kind === 'solid') return { href, label: 'Download STL', file: 'stencil.stl' }
  if (kind === 'shapes') return { href, label: 'Download SVG', file: 'trace.svg' }
  if (kind === 'parts') return { href, label: files > 1 ? 'Download ZIP' : 'Download STL', file: undefined }
  return null
}

/** The same row of actions for every node: preview (side panel only), download of what is on show, delete. */
export function NodeActions({ id, preview, download }: { id: string; preview?: boolean; download: ReturnType<typeof downloadFor> }) {
  const { removeNode, openBench } = useStudio.getState()
  return (
    <div className="actions">
      {preview && <button className="btn" onClick={() => openBench(id)} title="Double-click a node to open it">⤢ Open preview</button>}
      {download && <a className="btn" href={download.href} download={download.file}>{download.label}</a>}
      <button className="danger" onClick={() => removeNode(id)}>Delete node</button>
    </div>
  )
}

/** One line for the state of a node, the same in the card, the side panel and the preview. */
export function runSummary(run: NodeRun | undefined, busy = false): string {
  if (!run) return 'not run'
  if (run.state === 'running') return `running ${Math.round((run.progress ?? 0) * 100)}%${run.message ? ` · ${run.message}` : ''}`
  if (run.state === 'error') return 'failed'
  if (run.state === 'pending') return 'waiting'
  if (busy) return 'updating…'
  return run.cached ? 'done · cached' : `done · ${run.ms ?? 0} ms`
}
