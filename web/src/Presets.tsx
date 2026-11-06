import { useRef, useState } from 'react'
import { useStudio, type StudioNode } from './store'
import { BUILT_IN, mergeBook, parseBook, type Book } from './presetBook'
import Tooltip from './Tooltip'
import messages from './messages'

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
  const toast = useStudio.getState().toast
  const [draft, setDraft] = useState<string | null>(null)
  const [more, setMore] = useState(false)
  const mine = book[type] ?? {}
  const builtIn = BUILT_IN[type] ?? {}
  if (type === 'source' || type === 'mesh_import') return null
  const save = (b: Book) => { localStorage.setItem(KEY, JSON.stringify(b)); setBook(b) }
  const apply = (name: string) => {
    const p = mine[name] ?? builtIn[name]
    if (!p) return
    const { setParam, types } = useStudio.getState()
    // settings a node no longer has (saved by an older version) are skipped
    const known = new Set((types[type]?.params ?? []).map((x) => x.name))
    for (const [k, v] of Object.entries(p)) if (known.has(k)) setParam(node.id, k, v)
  }
  const commitName = () => {
    const name = draft?.trim()
    if (!name) { setDraft(null); return }
    if (builtIn[name]) { toast('That name belongs to a built-in preset.', { level: 'error' }); return }
    save({ ...book, [type]: { ...mine, [name]: { ...node.data.params } } })
    setPick(name)
    setDraft(null)
    toast(`Saved preset "${name}"`)
  }
  const remove = () => {
    if (!mine[pick]) return
    const gone = pick
    const was = mine[pick]
    const rest = { ...mine }
    delete rest[pick]
    save({ ...book, [type]: rest })
    setPick('')
    setMore(false)
    toast(`Deleted preset "${gone}"`, { undo: () => save({ ...load(), [type]: { ...(load()[type] ?? {}), [gone]: was } }) })
  }
  const exportAll = () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify({ stencilPresets: 1, presets: book }, null, 2)], { type: 'application/json' }))
    const a = document.createElement('a')
    a.href = url
    a.download = 'stencil-presets.json'
    a.click()
    URL.revokeObjectURL(url)
    toast('Presets export handed to browser')
  }
  const importFile = async (f: File) => {
    try {
      const merged = mergeBook(book, parseBook(JSON.parse(await f.text())))
      save(merged.book)
      toast(`Imported ${merged.added} preset${merged.added === 1 ? '' : 's'}.`)
    } catch (e) {
      toast(`That file is not a presets file: ${e instanceof Error ? e.message : e}`, { level: 'error' })
    }
  }
  return (
    <div className="presets" role="group" aria-label="Presets">
      {draft === null ? (
        <>
          <select aria-label="Preset" value={pick} onChange={(e) => { setPick(e.target.value); if (e.target.value) apply(e.target.value) }}>
            <option value="">Presets…</option>
            {Object.keys(builtIn).map((n) => <option key={`b${n}`} value={n}>{n}</option>)}
            {Object.keys(mine).map((n) => <option key={n} value={n}>{n}</option>)}
          </select>
          <Tooltip content={messages.actions.savePreset}><button onClick={() => setDraft('')}>Save as…</button></Tooltip>
          <span className="menu-wrap">
            <Tooltip content={messages.actions.morePresets}><button aria-label="More preset actions" aria-expanded={more} onClick={() => setMore(!more)}>⋯</button></Tooltip>
            {more && (
              <div className="menu" role="menu">
                <Tooltip content={messages.actions.deletePreset}><button role="menuitem" onClick={remove} disabled={!mine[pick]}>Delete preset</button></Tooltip>
                <Tooltip content={messages.actions.exportPresets}><button role="menuitem" onClick={() => { setMore(false); exportAll() }} disabled={!Object.keys(book).length}>Export presets…</button></Tooltip>
                <Tooltip content={messages.actions.importPresets}><button role="menuitem" onClick={() => { setMore(false); file.current?.click() }}>Import presets…</button></Tooltip>
              </div>
            )}
          </span>
        </>
      ) : (
        <form className="presetname" onSubmit={(e) => { e.preventDefault(); commitName() }}>
          <input autoFocus aria-label="Preset name" placeholder="Name this preset" value={draft} onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => { if (e.key === 'Escape') setDraft(null) }} />
          <button type="submit" className="primary">Save</button>
          <button type="button" onClick={() => setDraft(null)}>Cancel</button>
        </form>
      )}
      <input ref={file} type="file" accept="application/json,.json" hidden aria-label="Import presets file"
        onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) void importFile(f) }} />
    </div>
  )
}
