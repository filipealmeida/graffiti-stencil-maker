import { zipSync, strToU8 } from 'fflate'
import { api, artifactUrl, partUrl } from './api'
import { badgesFor } from './badges'
import { useStudio } from './store'

export const safeName = (n: string) => (n.trim().replace(/[\\/:*?"<>|]+/g, '-').replace(/\s+/g, '_') || 'project')

export function downloadBlob(blob: Blob, file: string) {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = file
  a.hidden = true
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

const RECENT = 'studio.recent'
export const recentProjects = (): string[] => {
  try { return JSON.parse(localStorage.getItem(RECENT) ?? '[]') } catch { return [] }
}
const remember = (name: string) => {
  try { localStorage.setItem(RECENT, JSON.stringify([name, ...recentProjects().filter((n) => n !== name)].slice(0, 5))) } catch { /* a convenience only */ }
}

/** The project as a file named after it; the unsaved mark clears. */
export async function saveProject() {
  const s = useStudio.getState()
  const p = await api.exportProject({ nodes: s.nodes, edges: s.edges })
  const name = safeName(s.pname)
  downloadBlob(new Blob([JSON.stringify(p)], { type: 'application/json' }), `${name}.stencil.json`)
  remember(name)
  s.markSaved()
  s.toast(`Saved ${name}.stencil.json to your downloads`)
}

const fetchBytes = async (url: string) => new Uint8Array(await (await fetch(url)).arrayBuffer())

/** One zip with the project file, every final model, a findings list and a README. */
export async function exportEverything() {
  const s = useStudio.getState()
  const name = safeName(s.pname)
  const files: Record<string, Uint8Array> = {}
  const notes: string[] = []
  const lines: string[] = []
  const used = new Set<string>()
  const uniq = (base: string, ext: string) => {
    let f = `${base}.${ext}`
    for (let i = 2; used.has(f); i++) f = `${base}-${i}.${ext}`
    used.add(f)
    return f
  }
  for (const n of s.nodes) {
    const t = s.types[n.data.type]
    const run = s.runs[n.id]
    if (!t || !run?.key || run.stale) continue
    const label = n.data.title || t.label
    for (const o of t.outputs) {
      if (!run.outputs?.[o.name]?.available) continue
      const info = api.peekInfo(run.key, o.name)
      for (const b of badgesFor(n.data.type, n.data.params, info)) lines.push(`[${b.level}] ${label}: ${b.text}`)
      if (!['solid', 'parts', 'shapes'].includes(o.type) || s.edges.some((e) => e.source === n.id && e.sourceHandle === o.name)) continue
      const base = safeName(label)
      if (o.type === 'solid') files[`models/${uniq(base, 'stl')}`] = await fetchBytes(artifactUrl(run.key, o.name))
      else if (o.type === 'shapes') files[`models/${uniq(base, 'svg')}`] = await fetchBytes(artifactUrl(run.key, o.name))
      else for (const part of Object.keys(info?.files ?? {})) {
        const url = partUrl(run.key, o.name, part)
        if (url) files[`models/${base}/${part}`] = await fetchBytes(url)
      }
      notes.push(`${label} (${o.type})`)
    }
  }
  if (!notes.length) { s.toast('Nothing to export yet: run the graph first', { level: 'error' }); return }
  const project = await api.exportProject({ nodes: s.nodes, edges: s.edges })
  files[`${name}.stencil.json`] = strToU8(JSON.stringify(project))
  files['findings.txt'] = strToU8(lines.length ? lines.join('\n') + '\n' : 'No findings.\n')
  files['README.txt'] = strToU8(
    `${name}\nExported ${new Date().toISOString()}\n\nmodels/ holds the final outputs: ${notes.join(', ')}.\n` +
    `${name}.stencil.json opens in Stencil Studio (drop it on the canvas).\nfindings.txt lists what the checks found.\n`)
  downloadBlob(new Blob([zipSync(files, { level: 0 }) as BlobPart], { type: 'application/zip' }), `${name}-everything.zip`)
  s.toast(`Exported ${name}-everything.zip`)
}
