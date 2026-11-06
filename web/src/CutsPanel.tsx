import { api, saveFile } from './api'
import { useStudio } from './store'

interface Piece { name: string; w: number; h: number; islands: number; bridges: number; lost_mm2: number }
interface Sheet { name: string; w: number; h: number; pieces: string[] }

/** The engine draws the file when the button is clicked, so a run does not pay for downloads nobody asks for. */
function Files({ keyOf, name, what }: { keyOf: string; name: string; what: string }) {
  const get = (ext: string, mime: string) => async () => {
    try {
      saveFile(`${name}.${ext}`, await api.part(keyOf, 'cuts', `${name}.${ext}`), mime)
    } catch (e) {
      useStudio.getState().toast(`Could not make ${name}.${ext}: ${e instanceof Error ? e.message : String(e)}. Run the node again`, { level: 'error' })
    }
  }
  return (
    <>
      <button className="btn" onClick={get('svg', 'image/svg+xml')} title={`Download this ${what} as SVG`}>SVG</button>
      <button className="btn" onClick={get('dxf', 'image/vnd.dxf')} title={`Download this ${what} as DXF`}>DXF</button>
    </>
  )
}

/** The parts and sheets of a Cut sheet: tick a part to see it on the preview, download any as SVG or DXF (millimetres). */
export default function CutsPanel({ info, keyOf, hidden, onHidden }: {
  info: Record<string, any>; keyOf: string; hidden: Set<string>; onHidden: (h: Set<string>) => void
}) {
  const pieces: Piece[] = info.piece_list ?? []
  const sheets: Sheet[] = info.sheet_list ?? []
  const lost = pieces.reduce((s, p) => s + p.lost_mm2, 0)
  const toggle = (name: string) => {
    const n = new Set(hidden)
    n.has(name) ? n.delete(name) : n.add(name)
    onHidden(n)
  }
  return (
    <aside className="v3parts tilepanel" aria-label="Cut parts">
      <div className="v3parts-head"><b>Sheets</b><span className="mut">{sheets.length}</span></div>
      {sheets.map((s) => (
        <div key={s.name} className="v3part">
          <label title={s.name}>{s.name}</label>
          <Files keyOf={keyOf} name={s.name} what="sheet" />
          <span className="v3size">{s.w} × {s.h} mm · {s.pieces.length} parts</span>
        </div>
      ))}
      <div className="v3parts-head"><b>Parts</b><button onClick={() => onHidden(new Set())}>all</button><button onClick={() => onHidden(new Set(pieces.map((p) => p.name)))}>none</button></div>
      <p className="mut tilepanel-note">Cut line {info.tool} mm off the material. Inner cuts are on layer CUT_1_INNER, outer cuts on CUT_2_OUTER.{lost > 0 ? ` Paint lost to the tool: ${Math.round(lost * 10) / 10} mm².` : ''}</p>
      {pieces.map((p) => (
        <div key={p.name} className={hidden.has(p.name) ? 'v3part off' : 'v3part'}>
          <label title={p.name}><input type="checkbox" checked={!hidden.has(p.name)} onChange={() => toggle(p.name)} /> {p.name}</label>
          <button title="show only this part" onClick={() => onHidden(new Set(pieces.filter((q) => q.name !== p.name).map((q) => q.name)))}>only</button>
          <Files keyOf={keyOf} name={p.name} what="part" />
          <span className="v3size">{p.w} × {p.h} mm{p.islands ? ` · ${p.bridges} bridges` : ''}</span>
        </div>
      ))}
    </aside>
  )
}
