import { useState } from 'react'
import { nodeFindings, type Badge } from './badges'
import { useStudio } from './store'

const ICON = { error: '✖', warn: '▲', info: 'ℹ' } as const
export const LEVEL_ORDER = { error: 0, warn: 1, info: 2 } as const

/** What to try for a finding: a line of advice and, when one click can do it, that action. */
export function hintFor(text: string): { hint: string; add?: string } | null {
  if (/pieces? loose in/.test(text)) return { hint: 'Join them or drop them.', add: 'bridges' }
  if (/island.* remain/.test(text)) return { hint: 'Join them with a Bridges node, or raise its bridge width.', add: 'bridges' }
  if (/not watertight/.test(text)) return { hint: 'Add a Simplify mesh node or lower the smoothing.' }
  if (/^overhang/.test(text)) return { hint: 'Raise the ground height so the paint fills below.' }
  if (/thinner than a nozzle/.test(text)) return { hint: 'Raise the bridge width to at least the nozzle size.' }
  if (/M triangles/.test(text)) return { hint: 'Add a Simplify mesh node or lower the resolution.' }
  return null
}

/** Adds the node of `type` after the node `from` and wires the first matching ports. */
function addAfter(from: string, type: string) {
  const s = useStudio.getState()
  const src = s.nodes.find((n) => n.id === from)
  const outs = src ? s.types[src.data.type]?.outputs ?? [] : []
  const made = (o: { name: string }) => s.runs[from]?.outputs?.[o.name]?.available
  const out = outs.find((o) => o.type === 'parts' && made(o)) ?? outs.find((o) => o.type === 'solid' && made(o)) ?? outs.find((o) => o.type === 'parts')
  const inp = s.types[type]?.inputs.find((i) => i.type === 'body' || i.type === out?.type)
  if (!src || !out || !inp) return
  const id = s.addNode(type, {}, { x: src.position.x + 320, y: src.position.y + 40 })
  s.connect({ source: from, sourceHandle: out.name, target: id, targetHandle: inp.name })
  s.select(id)
  s.toast(`Added ${s.types[type]?.label ?? type} after ${src.data.title || s.types[src.data.type].label}`)
}

export function useNodeFindings(id: string | null | undefined): Badge[] {
  const nodes = useStudio((s) => s.nodes)
  const edges = useStudio((s) => s.edges)
  const runs = useStudio((s) => s.runs)
  const types = useStudio((s) => s.types)
  return id ? nodeFindings({ nodes, edges, runs, types }, id) : []
}

/** The findings of one node inside its panel: a one-line summary that keeps the parameters
 * below it in the same place however many findings there are. The list opens on demand
 * and scrolls within a capped height. */
export default function Findings({ id }: { id: string }) {
  const all = useNodeFindings(id)
  const [open, setOpen] = useState(false)
  if (!all.length) return null
  const noun = { error: 'error', warn: 'warning', info: 'note' } as const
  const counts = (['error', 'warn', 'info'] as const)
    .map((level) => ({ level, n: all.filter((b) => b.level === level).length }))
    .filter((c) => c.n > 0)
  const sorted = [...all].sort((a, b) => LEVEL_ORDER[a.level] - LEVEL_ORDER[b.level])
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
      <button className="findings-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="mut">{open ? 'Hide' : 'Show'}</span>
        {counts.map((c) => (
          <span key={c.level} className={`finding-count ${c.level}`}>
            <span aria-hidden="true">{ICON[c.level]}</span> {c.n} {noun[c.level]}{c.n > 1 ? 's' : ''}
          </span>
        ))}
      </button>
      <ul className="findings-list" hidden={!open}>{sorted.map(row)}</ul>
    </section>
  )
}
