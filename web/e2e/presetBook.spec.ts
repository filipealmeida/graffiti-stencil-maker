import { expect, test } from '@playwright/test'
import { mergeBook, parseBook } from '../src/presetBook'

test('a presets file is checked and merged without overwriting', () => {
  const file = { stencilPresets: 1, presets: { stencil: { A: { bridge_mm: 2, junk: { x: 1 } }, B: { invert: true } }, tile_layout: { 'Mounting challenge 100 mm, 50 mm tiles': { width_mm: 1 } } } }
  const got = parseBook(file)
  expect(got.stencil.A).toEqual({ bridge_mm: 2 })
  const merged = mergeBook({ stencil: { A: { bridge_mm: 9 } } }, got)
  expect(merged.added).toBe(1)
  expect(merged.book.stencil.A).toEqual({ bridge_mm: 9 })
  expect(merged.book.stencil.B).toEqual({ invert: true })
  expect(merged.book.tile_layout).toBeUndefined()
})

test('files that are not presets are refused', () => {
  expect(() => parseBook({})).toThrow()
  expect(() => parseBook({ presets: [] })).toThrow()
  expect(() => parseBook({ presets: { stencil: { A: 3 } } })).toThrow()
})
