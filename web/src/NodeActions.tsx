import { api, artifactUrl, saveFile, type Kind, type NodeRun } from './api'
import { portText } from './ports'
import { checkpoint, undo, useStudio } from './store'
import { safeName } from './project'

/** What a finished output can be downloaded as: one wording and one file name for the side panel and the preview. */
export function downloadFor(kind: Kind | undefined, key: string | undefined, out: string | undefined, files = 0) {
  const name = safeName(useStudio.getState().pname)
  if (!kind || !key || !out) return null
  const href = artifactUrl(key, out)
  if (kind === 'solid') return { href, label: 'Download STL', file: `${name}.stl` }
  if (kind === 'shapes') return { href, label: 'Download SVG', file: `${name}.svg`, dxf: { key, out, label: 'Download DXF', file: `${name}.dxf` } }
  if (kind === 'parts') return { href, label: files > 1 ? 'Download ZIP' : 'Download STL', file: files > 1 ? `${name}-parts.zip` : `${name}.stl` }
  return null
}

/** The engine draws the DXF when the button is clicked, so a run does not pay for downloads nobody asks for. */
function saveDxf(key: string, out: string, file: string) {
  api.dxf(key, out).then(
    (b) => saveFile(file, b, 'image/vnd.dxf'),
    () => useStudio.getState().toast(`Could not make ${file}: run the node again`, { level: 'error' }),
  )
}

/** The row of actions for every node: preview (side panel only) and the download of what is on show. Delete lives in `NodeFooter`. */
export function NodeActions({ id, preview, download }: { id: string; preview?: boolean; download: ReturnType<typeof downloadFor> }) {
  const { openBench } = useStudio.getState()
  const dxf = download && 'dxf' in download ? download.dxf : undefined
  return (
    <>
      <div className="actions">
        {download && <a className="btn primary" href={download.href} download={download.file}>{download.label}</a>}
        {dxf && <button className="btn primary" onClick={() => saveDxf(dxf.key, dxf.out, dxf.file)} title="Closed polylines on the layer OUTLINE, in picture pixels, for CAD and cutters">{dxf.label}</button>}
        {preview && <button className="btn" onClick={() => openBench(id)} title="Double-click a node to open it">⤢ Open preview</button>}
      </div>
      <OutlineDownloads id={id} shown={download?.href} />
    </>
  )
}

/** Every outline the node takes in or gives out, each as SVG and DXF; the one already on show has its own buttons above. */
function OutlineDownloads({ id, shown }: { id: string; shown?: string }) {
  const node = useStudio((s) => s.nodes.find((n) => n.id === id))
  const t = useStudio((s) => (node ? s.types[node.data.type] : undefined))
  const edges = useStudio((s) => s.edges)
  const runs = useStudio((s) => s.runs)
  const pname = useStudio((s) => s.pname)
  if (!node || !t) return null
  const name = safeName(pname)
  const rows: { label: string; key: string; out: string }[] = []
  for (const o of t.outputs) {
    const key = runs[id]?.key
    if (o.type === 'shapes' && key && runs[id]?.outputs?.[o.name]?.available) rows.push({ label: `${portText(t.id, 'out', o.name).label} (out)`, key, out: o.name })
  }
  for (const i of t.inputs) {
    const e = i.type === 'shapes' ? edges.find((x) => x.target === id && x.targetHandle === i.name) : undefined
    const up = e && runs[e.source]
    if (e && up?.key && up.outputs?.[e.sourceHandle!]?.available) rows.push({ label: `${portText(t.id, 'in', i.name).label} (in)`, key: up.key, out: e.sourceHandle! })
  }
  const others = rows.filter((r) => artifactUrl(r.key, r.out) !== shown)
  if (!others.length) return null
  return (
    <div className="outline-downloads">
      {others.map((r) => {
        const file = `${name}-${r.label.replace(/[^a-z0-9]+/gi, '-').replace(/^-|-$/g, '').toLowerCase()}`
        return (
          <div className="actions" key={r.label}>
            <span className="mut">{r.label}</span>
            <a className="btn" href={artifactUrl(r.key, r.out)} download={`${file}.svg`}>SVG</a>
            <button className="btn" onClick={() => saveDxf(r.key, r.out, `${file}.dxf`)}>DXF</button>
          </div>
        )
      })}
    </div>
  )
}

/** Delete sits apart from the download, and an undo toast replaces a confirmation. */
export function DeleteNode({ id }: { id: string }) {
  return (
    <button className="danger" onClick={() => {
      const s = useStudio.getState()
      const label = s.nodes.find((n) => n.id === id)?.data.title || 'node'
      checkpoint()
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
