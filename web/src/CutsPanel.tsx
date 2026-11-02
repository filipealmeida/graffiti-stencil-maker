import { partUrl } from './api'

interface Piece { name: string; w: number; h: number; islands: number; bridges: number; lost_mm2: number }
interface Sheet { name: string; w: number; h: number; pieces: string[] }

function Files({ keyOf, name, what }: { keyOf: string; name: string; what: string }) {
  return (
    <>
      <a className="btn" href={partUrl(keyOf, 'cuts', `${name}.svg`)} download={`${name}.svg`} title={`Download this ${what} as SVG`}>SVG</a>
      <a className="btn" href={partUrl(keyOf, 'cuts', `${name}.dxf`)} download={`${name}.dxf`} title={`Download this ${what} as DXF`}>DXF</a>
    </>
  )
}

/** The parts and sheets of a Cut sheet: size of each, and the cut lines as SVG and DXF (millimetres). */
export default function CutsPanel({ info, keyOf }: { info: Record<string, any>; keyOf: string }) {
  const pieces: Piece[] = info.piece_list ?? []
  const sheets: Sheet[] = info.sheet_list ?? []
  const lost = pieces.reduce((s, p) => s + p.lost_mm2, 0)
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
      <div className="v3parts-head"><b>Parts</b><span className="mut">{pieces.length}</span></div>
      <p className="mut tilepanel-note">Cut line {info.tool} mm off the material. Inner cuts are on layer CUT_1_INNER, outer cuts on CUT_2_OUTER.{lost > 0 ? ` Paint lost to the tool: ${Math.round(lost * 10) / 10} mm².` : ''}</p>
      {pieces.map((p) => (
        <div key={p.name} className="v3part">
          <label title={p.name}>{p.name}</label>
          <Files keyOf={keyOf} name={p.name} what="part" />
          <span className="v3size">{p.w} × {p.h} mm{p.islands ? ` · ${p.bridges} bridges` : ''}</span>
        </div>
      ))}
    </aside>
  )
}
