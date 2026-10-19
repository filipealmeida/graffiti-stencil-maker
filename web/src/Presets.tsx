import { useState } from 'react'
import { useStudio, type StudioNode } from './store'

type Book = Record<string, Record<string, Record<string, unknown>>>
const KEY = 'stencil-presets'
/** Ready-made settings, kept apart from the user's own so they cannot be overwritten or deleted. */
const BUILT_IN: Book = {
  tile_layout: { 'Mounting challenge 100 mm, 50 mm tiles': { width_mm: 100, margin_mm: 10, max_x: 50, max_y: 50 } },
}

const load = (): Book => {
  try { return JSON.parse(localStorage.getItem(KEY) ?? '{}') } catch { return {} }
}

/** Named parameter sets for one node type, saved in this browser. Applying one sets every parameter it holds. */
export default function Presets({ node }: { node: StudioNode }) {
  const type = node.data.type
  const [book, setBook] = useState<Book>(load)
  const [pick, setPick] = useState('')
  const mine = book[type] ?? {}
  const builtIn = BUILT_IN[type] ?? {}
  if (type === 'source') return null
  const save = (b: Book) => { localStorage.setItem(KEY, JSON.stringify(b)); setBook(b) }
  const apply = (name: string) => {
    const p = mine[name] ?? builtIn[name]
    if (!p) return
    const { setParam } = useStudio.getState()
    for (const [k, v] of Object.entries(p)) setParam(node.id, k, v)
  }
  const add = () => {
    const name = window.prompt('Name this preset')?.trim()
    if (!name) return
    if (builtIn[name]) return window.alert('That name belongs to a built-in preset.')
    save({ ...book, [type]: { ...mine, [name]: { ...node.data.params } } })
    setPick(name)
  }
  const remove = () => {
    if (!mine[pick]) return
    const rest = { ...mine }
    delete rest[pick]
    save({ ...book, [type]: rest })
    setPick('')
  }
  return (
    <div className="presets" role="group" aria-label="Presets">
      <select aria-label="Preset" value={pick} onChange={(e) => { setPick(e.target.value); if (e.target.value) apply(e.target.value) }}>
        <option value="">Presets…</option>
        {Object.keys(builtIn).map((n) => <option key={`b${n}`} value={n}>{n}</option>)}
        {Object.keys(mine).map((n) => <option key={n} value={n}>{n}</option>)}
      </select>
      <button onClick={add} title="Save this node's parameters under a name">Save as…</button>
      <button onClick={remove} disabled={!mine[pick]} title="Delete the selected preset">Delete</button>
    </div>
  )
}
