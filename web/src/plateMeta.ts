export interface PlateMeta {
  w?: number
  h?: number
  parts: { file: string; role?: string; origin?: [number, number]; size?: [number, number] }[]
  connectors?: number[][]
  pads?: { x: number; y: number; r: number }[]
  holes?: { x: number; y: number; r: number }[]
  joins?: { a: [number, number]; b: [number, number] }[]
}

type LayoutPart = PlateMeta['parts'][number] & { origin: [number, number]; size: [number, number] }
export type LayoutPlateMeta = Omit<PlateMeta, 'w' | 'h' | 'parts'> & { w: number; h: number; parts: LayoutPart[] }

/** Missing layout dimensions disable overlays, never the meshes or file downloads. */
export function layoutPlateMeta(plate: PlateMeta | null | undefined): LayoutPlateMeta | null {
  if (!plate || !Number.isFinite(plate.w) || !Number.isFinite(plate.h)) return null
  const pair = (v: number[] | undefined) => Array.isArray(v) && v.length === 2 && v.every(Number.isFinite)
  const parts = plate.parts.filter((p): p is LayoutPart => pair(p.origin) && pair(p.size))
  return parts.length ? { ...plate, w: plate.w!, h: plate.h!, parts } : null
}
