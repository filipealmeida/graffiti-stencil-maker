import { useRef, useState } from 'react'
import { useStudio, type StudioNode } from './store'
import { BUILT_IN, mergeBook, parseBook, type Book } from './presetBook'

const KEY = 'stencil-presets'

const load = (): Book => {
  try { return JSON.parse(localStorage.getItem(KEY) ?? '{}') } catch { return {} }
}

/** Named parameter sets for one node type, saved in this browser. Applying one sets every parameter it holds. */
export default function Presets({ node }: { node: StudioNode }) {
  const type = node.data.type
  const [book, setBook] = useState<Book>(load)
  const [pick, setPick] = useState('')
  const file = useRef<HTMLInputElement>(null)
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
  const exportAll = () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify({ stencilPresets: 1, presets: book }, null, 2)], { type: 'application/json' }))
    const a = document.createElement('a')
    a.href = url
    a.download = 'stencil-presets.json'
    a.click()
    URL.revokeObjectURL(url)
  }
  const importFile = async (f: File) => {
    try {
      const merged = mergeBook(book, parseBook(JSON.parse(await f.text())))
      save(merged.book)
      window.alert(`Imported ${merged.added} preset${merged.added === 1 ? '' : 's'}.`)
    } catch (e) {
      window.alert(`That file is not a presets file: ${e instanceof Error ? e.message : e}`)
    }
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
      <button onClick={exportAll} disabled={!Object.keys(book).length} title="Save all your presets to a file to share or back up">Export</button>
      <button onClick={() => file.current?.click()} title="Add the presets from a file; a name that already exists is kept">Import</button>
      <input ref={file} type="file" accept="application/json,.json" hidden aria-label="Import presets file"
        onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) void importFile(f) }} />
    </div>
  )
}
