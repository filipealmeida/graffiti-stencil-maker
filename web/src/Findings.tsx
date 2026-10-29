import { useState } from 'react'
import { api } from './api'
import { badgesFor, type Badge } from './badges'
import { useStudio } from './store'

const ICON = { error: '✖', warn: '▲', info: 'ℹ' } as const

/** What to try for a finding: a line of advice and, when one click can do it, that action. */
export function hintFor(text: string): { hint: string; add?: string } | null {
  if (/pieces? loose in/.test(text)) return { hint: 'Join them or drop them.', add: 'bridge_fragments' }
  if (/island.* remain/.test(text)) return { hint: 'Raise the bridge width or the ground height.' }
  if (/not watertight/.test(text)) return { hint: 'Add a Remesh node or lower the smoothing.' }
  if (/^overhang/.test(text)) return { hint: 'Raise the ground height so the paint fills below.' }
  if (/thinner than a nozzle/.test(text)) return { hint: 'Raise the bridge width to at least the nozzle size.' }
  if (/M triangles/.test(text)) return { hint: 'Add a Remesh node or lower the resolution.' }
  return null
}

/** Adds the node of `type` after the node `from` and wires the first matching ports. */
function addAfter(from: string, type: string) {
  const s = useStudio.getState()
  const src = s.nodes.find((n) => n.id === from)
  const out = src && s.types[src.data.type]?.outputs.find((o) => o.type === 'parts')
  const inp = s.types[type]?.inputs.find((i) => i.type === 'parts')
  if (!src || !out || !inp) return
  const id = s.addNode(type, {}, { x: src.position.x + 320, y: src.position.y + 40 })
  s.connect({ source: from, sourceHandle: out.name, target: id, targetHandle: inp.name })
  s.select(id)
  s.toast(`Added ${s.types[type]?.label ?? type} after ${src.data.title || s.types[src.data.type].label}`)
}

export function useNodeFindings(id: string | null | undefined): Badge[] {
  const node = useStudio((s) => s.nodes.find((n) => n.id === id))
  const run = useStudio((s) => (id ? s.runs[id] : undefined))
  const t = useStudio((s) => (node ? s.types[node.data.type] : undefined))
  if (!node || !run?.key || run.stale || !t) return []
  const seen = new Set<string>()
  const out: Badge[] = []
  for (const o of t.outputs) {
    if (!run.outputs?.[o.name]?.available) continue
    for (const b of badgesFor(node.data.type, node.data.params, api.peekInfo(run.key, o.name))) {
      if (!seen.has(b.text)) { seen.add(b.text); out.push(b) }
    }
  }
  return out
}

/** The findings of one node inside its panel. Errors and warnings stay open; notes fold away. */
export default function Findings({ id }: { id: string }) {
  const all = useNodeFindings(id)
  const [notes, setNotes] = useState(false)
  if (!all.length) return null
  const loud = all.filter((b) => b.level !== 'info')
  const quiet = all.filter((b) => b.level === 'info')
  const row = (b: Badge) => {
    const h = b.level === 'info' ? null : hintFor(b.text)
    return (
      <li key={b.text} className={`finding ${b.level}`}>
        <span aria-hidden="true">{ICON[b.level]}</span>
        <div>
          <span>{b.text}</span>
          {h && <span className="fix"> {h.hint}{h.add && <> <button onClick={() => addAfter(id, h.add!)}>Add {useStudio.getState().types[h.add]?.label ?? 'node'}</button></>}</span>}
        </div>
      </li>
    )
  }
  return (
    <section className="findings" aria-label="Findings">
      {loud.length > 0 && <ul>{loud.map(row)}</ul>}
      {quiet.length > 0 && (
        <>
          <button className="linkish" aria-expanded={notes} onClick={() => setNotes(!notes)}>{notes ? 'Hide' : 'Show'} {quiet.length} note{quiet.length > 1 ? 's' : ''}</button>
          {notes && <ul>{quiet.map(row)}</ul>}
        </>
      )}
    </section>
  )
}
