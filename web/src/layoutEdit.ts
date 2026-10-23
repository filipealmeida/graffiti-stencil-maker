/** Hand edits of a tile layout, kept as offsets from the automatic layout (mm). Mirrors `Edits` in the engine. */
export interface LayoutEdits { seams: Record<string, number>; pockets: Record<string, number> }

export interface PocketInfo {
  key: string; axis: 0 | 1; seam: number; s: number; u: number; auto_u: number
  block: [number, number, number, number]; range: [number, number]
}

export interface LayoutInfo {
  width: number; height: number
  xs: number[]; ys: number[]; auto_xs: number[]; auto_ys: number[]
  pocket_list: PocketInfo[]
  min_tile: number; max: [number, number]; gap: number; quantum: number
  edits_ignored: number
}

export type Drag =
  | { kind: 'seam'; axis: 0 | 1; i: number; v: number }
  | { kind: 'pocket'; key: string; u: number }

export const parseEdits = (text: unknown): LayoutEdits => {
  try {
    const v = JSON.parse(String(text || '{}'))
    return { seams: { ...(v.seams ?? {}) }, pockets: { ...(v.pockets ?? {}) } }
  } catch { return { seams: {}, pockets: {} } }
}

export const isEmpty = (e: LayoutEdits) => !Object.keys(e.seams).length && !Object.keys(e.pockets).length
export const stringify = (e: LayoutEdits) => (isEmpty(e) ? '' : JSON.stringify(e))

const snap = (l: LayoutInfo, v: number) => (l.quantum > 0 ? Math.round(v / l.quantum) * l.quantum : v)

/** Where a dragged seam would land, and why it can't if a tile would break the size limits. */
export function checkSeam(l: LayoutInfo, axis: 0 | 1, i: number, raw: number): { v: number; why?: string } {
  const g = axis === 0 ? l.xs : l.ys
  const v = snap(l, raw)
  const max = l.max[axis], name = axis === 0 ? 'wide' : 'high'
  const left = v - g[i - 1], right = g[i + 1] - v
  if (Math.min(left, right) < l.min_tile - 1e-6)
    return { v, why: `a tile would be only ${Math.max(0, Math.min(left, right)).toFixed(0)} mm ${name}; connectors need at least ${l.min_tile.toFixed(0)} mm` }
  if (max > 0 && Math.max(left, right) > max + 1e-6)
    return { v, why: `a tile would be ${Math.max(left, right).toFixed(0)} mm ${name}, over the ${max.toFixed(0)} mm limit` }
  return { v }
}

const shifted = (p: PocketInfo, u: number): [number, number, number, number] => {
  const d = u - p.u, b = p.block
  return p.axis === 0 ? [b[0], b[1] + d, b[2], b[3] + d] : [b[0] + d, b[1], b[2] + d, b[3]]
}

/** Where a dragged block would land: it must stay on its seam segment and clear of the other blocks. */
export function checkPocket(l: LayoutInfo, key: string, raw: number): { u: number; why?: string } {
  const p = l.pocket_list.find((x) => x.key === key)!
  const [lo, hi] = p.range
  if (raw < lo - 1e-6 || raw > hi + 1e-6) return { u: raw, why: 'a block has to stay on its seam, clear of the tile corners' }
  const a = shifted(p, raw)
  for (const o of l.pocket_list) {
    if (o.key === key) continue
    const b = o.block
    if (a[0] < b[2] + l.gap && b[0] < a[2] + l.gap && a[1] < b[3] + l.gap && b[1] < a[3] + l.gap)
      return { u: raw, why: `it would touch the block at ${o.axis === 0 ? 'x' : 'y'} ${o.s.toFixed(0)} mm (keep ${l.gap} mm apart)` }
  }
  return { u: raw }
}

export type Hit = { kind: 'seam'; axis: 0 | 1; i: number } | { kind: 'pocket'; key: string } | null

/** What lies under a plate point (mm); `tol` is the grab distance in mm. Blocks win over seams. */
export function hitTest(l: LayoutInfo, x: number, y: number, tol: number): Hit {
  for (const p of l.pocket_list) {
    const b = p.block
    if (x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]) return { kind: 'pocket', key: p.key }
  }
  let best: Hit = null, d = tol
  l.xs.forEach((s, i) => { if (i > 0 && i < l.xs.length - 1 && Math.abs(s - x) <= d) { d = Math.abs(s - x); best = { kind: 'seam', axis: 0, i } } })
  l.ys.forEach((s, i) => { if (i > 0 && i < l.ys.length - 1 && Math.abs(s - y) <= d) { d = Math.abs(s - y); best = { kind: 'seam', axis: 1, i } } })
  return best
}

/** The edits after a finished drag. */
export function applyDrag(l: LayoutInfo, e: LayoutEdits, d: Drag): LayoutEdits {
  const out = { seams: { ...e.seams }, pockets: { ...e.pockets } }
  const put = (m: Record<string, number>, k: string, off: number) => { if (Math.abs(off) < 1e-6) delete m[k]; else m[k] = +off.toFixed(4) }
  if (d.kind === 'seam') put(out.seams, `${d.axis === 0 ? 'x' : 'y'}${d.i}`, d.v - (d.axis === 0 ? l.auto_xs : l.auto_ys)[d.i])
  else put(out.pockets, d.key, d.u - l.pocket_list.find((p) => p.key === d.key)!.auto_u)
  return out
}
