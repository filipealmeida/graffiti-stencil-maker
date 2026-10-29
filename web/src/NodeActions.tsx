import { artifactUrl, type Kind, type NodeRun } from './api'
import { undo, useStudio } from './store'
import { safeName } from './project'

/** What a finished output can be downloaded as: one wording and one file name for the side panel and the preview. */
export function downloadFor(kind: Kind | undefined, key: string | undefined, out: string | undefined, files = 0) {
  const name = safeName(useStudio.getState().pname)
  if (!kind || !key || !out) return null
  const href = artifactUrl(key, out)
  if (kind === 'solid') return { href, label: 'Download STL', file: `${name}.stl` }
  if (kind === 'shapes') return { href, label: 'Download SVG', file: `${name}.svg` }
  if (kind === 'parts') return { href, label: files > 1 ? 'Download ZIP' : 'Download STL', file: files > 1 ? `${name}-parts.zip` : `${name}.stl` }
  return null
}

/** The row of actions for every node: preview (side panel only) and the download of what is on show. Delete lives in `NodeFooter`. */
export function NodeActions({ id, preview, download }: { id: string; preview?: boolean; download: ReturnType<typeof downloadFor> }) {
  const { openBench } = useStudio.getState()
  return (
    <div className="actions">
      {download && <a className="btn primary" href={download.href} download={download.file}>{download.label}</a>}
      {preview && <button className="btn" onClick={() => openBench(id)} title="Double-click a node to open it">⤢ Open preview</button>}
    </div>
  )
}

/** Delete sits apart from the download, and an undo toast replaces a confirmation. */
export function DeleteNode({ id }: { id: string }) {
  return (
    <button className="danger" onClick={() => {
      const s = useStudio.getState()
      const label = s.nodes.find((n) => n.id === id)?.data.title || 'node'
      s.removeNode(id)
      s.toast(`Deleted ${label}`, { undo })
    }}>Delete node</button>
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
