export type BadgeLevel = 'error' | 'warn' | 'info'
export interface Badge { level: BadgeLevel; text: string }

type Info = Record<string, any>

const n = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : 0)

/** Short findings about what a node produced, so a problem shows on the node without opening it. */
export function badgesFor(type: string, params: Record<string, unknown>, info: Info | null | undefined): Badge[] {
  const out: Badge[] = []
  const s = info?.stats
  if (s) {
    if (n(s.islands_remaining) > 0) out.push({ level: 'error', text: `${n(s.islands_remaining)} island${n(s.islands_remaining) > 1 ? 's' : ''} remain` })
    if (s.watertight === false) out.push({ level: 'error', text: 'mesh is not watertight' })
    if (n(s.overhang_area_mm2) > 0.5) out.push({ level: 'warn', text: `overhang ${n(s.overhang_area_mm2).toFixed(0)} mm²` })
    if (n(s.islands_deleted) > 0) out.push({ level: 'warn', text: `${n(s.islands_deleted)} island${n(s.islands_deleted) > 1 ? 's' : ''} deleted` })
    if (n(s.triangles) > 1_500_000) out.push({ level: 'warn', text: `${(n(s.triangles) / 1e6).toFixed(1)} M triangles: heavy to slice` })
    if (typeof s.fallback === 'string') out.push({ level: 'warn', text: s.fallback })
    if (n(s.islands_found) > 0 && n(s.islands_remaining) === 0) out.push({ level: 'info', text: `${n(s.islands_found)} islands bridged` })
  }
  if ((type === 'stencil' || type === 'vector_stencil') && n(params.bridge_mm) > 0 && n(params.bridge_mm) < 1) {
    out.push({ level: 'warn', text: `bridge ${n(params.bridge_mm)} mm is thinner than a nozzle` })
  }
  if (info?.kind === 'parts') {
    for (const w of info.meta?.pads?.warnings ?? []) out.push({ level: 'warn', text: String(w) })
    const t = info.meta?.tiling
    const bf = info.meta?.bridged_fragments
    if (n(bf?.islands_bridged) > 0) out.push({ level: 'info', text: `${n(bf.islands_bridged)} island${n(bf.islands_bridged) > 1 ? 's' : ''} bridged in ${n(bf.tiles_bridged)} tile${n(bf.tiles_bridged) > 1 ? 's' : ''}` })
    for (const w of bf?.warnings ?? []) out.push({ level: 'warn', text: String(w) })
    const fr = info.meta?.fragments
    if (n(fr?.pieces_removed) > 0) out.push({ level: 'info', text: `${n(fr.pieces_removed)} fragment${n(fr.pieces_removed) > 1 ? 's' : ''} removed from ${n(fr.tiles_cleaned)} tile${n(fr.tiles_cleaned) > 1 ? 's' : ''} (${n(fr.volume_removed_mm3).toFixed(0)} mm³)` })
    if (type === 'tiles' && n(t?.loose_pieces) > 0) out.push({ level: 'warn', text: `${n(t.loose_pieces)} ${n(t.loose_pieces) > 1 ? 'pieces' : 'piece'} loose in ${n(t.loose_tiles)} ${n(t.loose_tiles) > 1 ? 'tiles' : 'tile'}: join them with Bridges or drop them with Remove fragments` })
    if (n(t?.reinforced_seams) > 0) out.push({ level: 'info', text: `${n(t.reinforced_seams)} seams reinforced` })
    for (const f of t?.fallback_tiles ?? []) out.push({ level: 'info', text: `${f.tile}: ${f.note}` })
    if (n(t?.dropped_specks) > 0) out.push({ level: 'warn', text: `${n(t.dropped_specks)} tiny pieces dropped` })
    if (n(t?.dropped_fragments) > 0) out.push({ level: 'warn', text: `${n(t.dropped_fragments)} loose pieces dropped where tiles were cut: paint may reach through there` })
    if (n(t?.seam_step_mm) > 0) out.push({ level: 'info', text: `${n(t.seam_step_mm).toFixed(0)} mm of seam where neighbouring tiles differ by more than a layer in height` })
    if (n(t?.paint_under_blocks_mm2) > 0 && !t?.pre_tiled) out.push({ level: 'info', text: `${n(t.paint_under_blocks_mm2).toFixed(0)} mm² of paint under connector blocks` })
  }
  if (info?.kind === 'tiles' && n(info.edits_ignored) > 0) out.push({ level: 'warn', text: `${n(info.edits_ignored)} layout edit(s) no longer fit and were ignored` })
  if (info?.kind === 'tiles' && n(info.paint_under_blocks_mm2) > 0) {
    const lost = n(info.paint_under_blocks_mm2)
    const share = n(info.paint_mm2) > 0 ? ` (${((100 * lost) / n(info.paint_mm2)).toFixed(1)}% of the artwork)` : ''
    const level: BadgeLevel = n(info.paint_mm2) > 0 && lost / n(info.paint_mm2) > 0.05 ? 'warn' : 'info'
    out.push({ level, text: `${lost.toFixed(0)} mm² of paint under connector blocks${share}` })
  }
  if (info?.kind === 'mask' && typeof info.coverage === 'number') {
    if (info.coverage < 0.005) out.push({ level: 'warn', text: 'mask is almost empty' })
    else if (info.coverage > 0.995) out.push({ level: 'warn', text: 'mask is almost all paint' })
  }
  if (info?.kind === 'shapes' && n(info.loops) === 0) out.push({ level: 'warn', text: 'no outlines found' })
  return out
}
