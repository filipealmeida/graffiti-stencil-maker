import { partUrl } from './api'
import Tooltip from './Tooltip'
import messages from './messages'

interface Tile { name: string; row: number; col: number; w: number; h: number }

/** The tiles of a Tile plan: size of each, and its cut outlines as SVG and DXF (millimetres, at its own origin). */
export default function TilesPanel({ info, keyOf, out }: { info: Record<string, any>; keyOf: string; out: string }) {
  const tiles: Tile[] = info.tile_list ?? []
  return (
    <aside className="v3parts tilepanel" aria-label="Tiles">
      <div className="v3parts-head"><b>Tiles</b><span className="mut">{tiles.length} · plate {Math.round(info.width)} × {Math.round(info.height)} mm</span></div>
      <p className="mut tilepanel-note">{info.vector ? 'Cut outlines from the vector outlines' : 'Cut outlines traced from the mask'}, with the tile border and the connector blocks, in millimetres.</p>
      {tiles.map((t) => (
        <div key={t.name} className="v3part">
          <label title={t.name}>{t.name}</label>
          <Tooltip content={messages.actions.downloadSvgTile}><a className="btn" href={partUrl(keyOf, out, `${t.name}.svg`)} download={`${t.name}.svg`}>SVG</a></Tooltip>
          <Tooltip content={messages.actions.downloadDxfTile}><a className="btn" href={partUrl(keyOf, out, `${t.name}.dxf`)} download={`${t.name}.dxf`}>DXF</a></Tooltip>
          <span className="v3size">{t.w} × {t.h} mm</span>
        </div>
      ))}
    </aside>
  )
}
