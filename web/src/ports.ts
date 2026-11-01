import type { Kind, Stage } from './api'

export type Shape = 'circle' | 'square' | 'diamond' | 'hex' | 'oct' | 'tri'

export interface KindInfo { name: string; what: string; colour: string; shape: Shape }

/** One colour and handle shape per kind: 2D raster is round, vector is square, the plan is a diamond, 3D is a polygon. */
export const KIND_INFO: Record<Kind, KindInfo> = {
  image: { name: 'Image', what: 'a picture', colour: '#4aa3ff', shape: 'circle' },
  mask: { name: 'Paint mask', what: 'a black and white map of where the paint goes', colour: '#ffa24a', shape: 'circle' },
  shapes: { name: 'Outlines', what: 'vector outlines of the paint (curves)', colour: '#2ec4b6', shape: 'square' },
  tiles: { name: 'Tile plan', what: 'how the plate is divided into tiles, planned before it is built', colour: '#ff6fb5', shape: 'diamond' },
  solid: { name: 'Solid', what: 'one 3D model', colour: '#3fbf72', shape: 'hex' },
  parts: { name: 'Parts', what: 'a set of 3D models: tiles, slabs or hardware', colour: '#a6dd4f', shape: 'oct' },
  body: { name: 'Solid or parts', what: 'either one 3D model or a set of parts', colour: '#3fbf72', shape: 'hex' },
  layer: { name: 'Layer', what: 'a picture, paint mask or outlines with a position and size on a canvas', colour: '#9b7dff', shape: 'tri' },
  item: { name: 'Picture, paint, outlines or layer', what: 'a picture, a paint mask, outlines, or a placed layer', colour: '#cfd6df', shape: 'circle' },
  any: { name: 'Image or mask', what: 'a picture or a paint mask', colour: '#8a93a0', shape: 'circle' },
}

export const kindBackground = (k: Kind) =>
  k === 'body' ? `linear-gradient(90deg, ${KIND_INFO.solid.colour} 50%, ${KIND_INFO.parts.colour} 50%)`
  : k === 'item' ? `conic-gradient(${KIND_INFO.image.colour} 0 25%, ${KIND_INFO.mask.colour} 0 50%, ${KIND_INFO.shapes.colour} 0 75%, ${KIND_INFO.layer.colour} 0)`
  : KIND_INFO[k].colour

export const STAGE_LABEL: Record<Stage, string> = { trace: 'prepare', stencil: 'build', post: 'finish' }

interface PortText { label?: string; hint?: string; or?: boolean; either?: boolean }
type Table = Record<string, { in?: Record<string, PortText>; out?: Record<string, PortText> }>

const paint = { label: 'paint', hint: 'Where the paint goes' }
const outlines = { label: 'outlines' }
const plan = { label: 'tile plan', hint: 'From a Tile plan node' }

/** Display names and hints for ports. The port ids never change, so saved projects keep working. */
const TABLE: Table = {
  text: { out: { mask: paint } },
  threshold: { in: { image: { label: 'picture' } }, out: { mask: paint } },
  tone_patterns: { in: { image: { label: 'picture', hint: 'A reduced image' } }, out: { mask: paint, tones: { label: 'tone map', hint: 'The picture with every tone shown as a flat colour' } } },
  smooth: { in: { mask: paint }, out: { mask: paint } },
  morphology: { in: { mask: paint }, out: { mask: paint } },
  trace: { in: { mask: paint }, out: { shapes: outlines } },
  round_corners: { in: { shapes: outlines }, out: { shapes: outlines, rounded: { label: 'rounded tips', hint: 'Only the corners that were rounded, for checking' } } },
  remove_islands: { in: { shapes: outlines }, out: { shapes: outlines, removed: { label: 'removed', hint: 'Only what was removed, for checking' } } },
  shapes_to_mask: { in: { shapes: outlines }, out: { mask: paint } },
  stencil: { in: { mask: { ...paint, hint: 'Connect this, or a tile plan', either: true }, tiles: { ...plan, or: true } }, out: { solid: { label: 'solid', hint: 'The whole plate as one 3D model' }, parts: { label: 'parts', hint: 'One part per tile (with a tile plan), otherwise the plate' } } },
  vector_stencil: { in: { shapes: { ...outlines, hint: 'Connect this, or a tile plan', either: true }, tiles: { ...plan, or: true } }, out: { solid: { label: 'solid', hint: 'The whole plate as one 3D model' }, parts: { label: 'parts', hint: 'One part per tile (with a tile plan), otherwise the plate' } } },
  tile_layout: { in: { mask: { ...paint, hint: 'Connect this or outlines', either: true }, shapes: { ...outlines, hint: 'Connect this or paint', or: true } } },
  bridges: { out: { solid: { label: 'solid', hint: 'The bridged model (with a solid in)' }, parts: { label: 'parts', hint: 'The bridged tiles' } } },
  place: { in: { item: { label: 'content', hint: 'A picture, a paint mask, outlines, or another placed layer' } }, out: { layer: { label: 'layer', hint: 'The content with its position and size, for a Collage' } } },
  repeat: { in: { item: { label: 'content', hint: 'The picture, paint mask or outlines to repeat' } }, out: { image: { label: 'picture', hint: 'The grid of copies. Masks and outlines are drawn dark on white' }, mask: { label: 'paint', hint: 'Only when the content is a paint mask or outlines' }, shapes: { label: 'outlines', hint: 'Only when the content is outlines and the copies do not overlap' } } },
  canvas: { out: { image: { label: 'picture', hint: 'The combined picture. Masks and outlines are drawn dark on white' }, mask: { label: 'paint', hint: 'Only when every layer is a paint mask or outlines' }, shapes: { label: 'outlines', hint: 'Only when every layer is outlines and none overlap' } } },
  colour_set: { out: { parts: { label: 'plates', hint: 'One registered plate per colour' } } },
}

const plain = (n: string) => n.replace(/_/g, ' ')

export function portText(node: string, dir: 'in' | 'out', name: string): { label: string; hint: string; or: boolean; either: boolean } {
  const t = TABLE[node]?.[dir]?.[name]
  return { label: t?.label ?? plain(name), hint: t?.hint ?? '', or: !!t?.or, either: !!(t?.or || t?.either) }
}

export function portTitle(node: string, dir: 'in' | 'out', name: string, kind: Kind, required: boolean): string {
  const k = KIND_INFO[kind]
  const { label, hint } = portText(node, dir, name)
  const head = `${label}: ${k.name.toLowerCase()}, ${k.what}`
  const take = dir === 'in' ? (kind === 'body' ? '. Takes a solid or parts' : kind === 'item' ? '. Takes a picture, a paint mask, outlines or a placed layer' : kind === 'any' ? '. Takes an image or a mask' : '') + (required ? '' : '. Optional') : ''
  return `${head}${take}${hint ? `. ${hint}` : ''}`
}

/** Whether an output of kind `o` can feed an input of kind `i`. */
export const canLink = (o: Kind, i: Kind) => i === o || (i === 'item' && (o === 'image' || o === 'mask' || o === 'shapes' || o === 'layer')) || (i === 'any' && (o === 'image' || o === 'mask')) || (i === 'body' && (o === 'solid' || o === 'parts'))

/** Numbered inputs (layer_1, layer_2, ...) show one empty slot after the last linked one, so a node takes as many as you connect. */
export function visibleInputs<T extends { name: string }>(inputs: T[], linked: (string | null)[]): T[] {
  const num = (name: string) => /^(.+)_(\d+)$/.exec(name)
  const top: Record<string, number> = {}
  for (const l of linked) {
    const m = l && num(l)
    if (m) top[m[1]] = Math.max(top[m[1]] ?? 0, Number(m[2]))
  }
  return inputs.filter((i) => {
    const m = num(i.name)
    return !m || Number(m[2]) <= (top[m[1]] ?? 0) + 1
  })
}
