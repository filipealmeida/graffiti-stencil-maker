export type Book = Record<string, Record<string, Record<string, unknown>>>

/** Ready-made settings, kept apart from the user's own so they cannot be overwritten or deleted. */
export const BUILT_IN: Book = {
  tile_layout: { 'Mounting challenge 100 mm, 50 mm tiles': { width_mm: 100, margin_mm: 10, max_x: 50, max_y: 50 } },
}

/** Checks a presets file: node type -> preset name -> parameter -> value. Throws on anything else. */
export function parseBook(raw: unknown): Book {
  const src = (raw as { presets?: unknown })?.presets
  if (!src || typeof src !== 'object' || Array.isArray(src)) throw new Error('no presets in it')
  const out: Book = {}
  for (const [type, sets] of Object.entries(src)) {
    if (!sets || typeof sets !== 'object' || Array.isArray(sets)) throw new Error(`bad entry for ${type}`)
    for (const [name, params] of Object.entries(sets)) {
      if (!params || typeof params !== 'object' || Array.isArray(params)) throw new Error(`bad preset ${type}/${name}`)
      const clean: Record<string, unknown> = {}
      for (const [k, v] of Object.entries(params)) {
        if (['string', 'number', 'boolean'].includes(typeof v)) clean[k] = v
      }
      ;(out[type] ??= {})[name] = clean
    }
  }
  return out
}

/** Adds the imported presets to the existing ones; a name that is already taken keeps its saved parameters. */
export function mergeBook(have: Book, add: Book): { book: Book; added: number } {
  const book: Book = JSON.parse(JSON.stringify(have))
  let added = 0
  for (const [type, sets] of Object.entries(add)) {
    for (const [name, params] of Object.entries(sets)) {
      if (book[type]?.[name] || BUILT_IN[type]?.[name]) continue
      ;(book[type] ??= {})[name] = params
      added++
    }
  }
  return { book, added }
}
