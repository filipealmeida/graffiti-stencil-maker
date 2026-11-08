import { expect, test } from '@playwright/test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { layoutPlateMeta, type PlateMeta } from '../src/plateMeta'

const plate: PlateMeta = {
  w: 100, h: 60,
  parts: [
    { file: 'roll_up_r0c0.stl', role: 'hinged_assembly', origin: [0, 0], size: [50, 60] },
    { file: 'roll_up_r0c1.stl', role: 'hinged_assembly', origin: [50, 0], size: [50, 60] },
  ],
}

test('hinged assembly roles preserve plate layout dimensions while dimensionless assemblies omit overlays', () => {
  expect(layoutPlateMeta(plate)).toEqual(plate)
  expect(layoutPlateMeta({ parts: [{ file: 'roll_up_stencil.stl', role: 'hinged_assembly' }] })).toBeNull()
  expect(layoutPlateMeta({ w: 100, h: 60, parts: [{ file: 'roll_up_stencil.stl', role: 'hinged_assembly' }] })).toBeNull()
  expect(layoutPlateMeta({ ...plate, parts: [...plate.parts, { file: 'rail.stl', role: 'stiffener' }] })?.parts).toEqual(plate.parts)
  expect(layoutPlateMeta({ ...plate, w: NaN })).toBeNull()
})

test('viewer lists and downloads hinged assemblies and stiffeners regardless of roles or missing dimensions', async () => {
  const originalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: { getItem: () => null } })
  try {
    const [{ default: Viewer3D }, { downloadFor }, { artifactUrl }] = await Promise.all([
      import('../src/Viewer3D'), import('../src/NodeActions'), import('../src/api'),
    ])
    const parts = [...plate.parts, { file: 'stiffener_rail.stl', role: 'stiffener' }].map((p) => ({
      name: p.file, url: `https://studio.invalid/${p.file}`,
    }))
    const markup = renderToStaticMarkup(createElement(Viewer3D, { parts, plate }))
    for (const p of parts) {
      expect(markup).toContain(`href="${p.url}"`)
      expect(markup).toContain(`download="${p.name}"`)
    }
    const single = renderToStaticMarkup(createElement(Viewer3D, {
      parts: [{ name: 'roll_up_stencil.stl', url: 'blob:assembly' }],
      plate: { parts: [{ file: 'roll_up_stencil.stl', role: 'hinged_assembly' }] },
    }))
    expect(single).toContain('class="viewer"')
    expect(single).not.toContain('> layout</label>')
    for (const output of ['parts', 'stiffeners']) {
      const stl = downloadFor('parts', 'roll-result', output, 1)
      expect(stl?.label).toBe('Download STL')
      expect(stl?.href).toBe(artifactUrl('roll-result', output))
      expect(stl?.file).toMatch(/\.stl$/)
      const zip = downloadFor('parts', 'roll-result', output, 2)
      expect(zip?.label).toBe('Download ZIP')
      expect(zip?.file).toMatch(/\.zip$/)
    }
  } finally {
    if (originalStorage) Object.defineProperty(globalThis, 'localStorage', originalStorage)
    else Reflect.deleteProperty(globalThis, 'localStorage')
  }
})
