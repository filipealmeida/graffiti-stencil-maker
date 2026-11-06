import { expect, test } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { parseProject, validateGraphSnapshot } from '../src/projectSchema'
import { migrateProject } from '../src/migrate'

const node = (id = 'a') => ({ id, type: 'studio' as const, position: { x: 0, y: 0 }, data: { type: 'source', params: {} } })
const graph = () => ({ nodes: [node()], edges: [] })

test('version 2 and historical project envelopes remain compatible', () => {
  for (const version of [undefined, 1, 2]) {
    const parsed = parseProject({ version, graph: graph() })
    expect(parsed.version).toBe(2)
    expect(parsed.images).toEqual({})
    expect(parsed.models).toEqual({})
  }
  const fixture = JSON.parse(readFileSync('e2e/fixtures/margin-ring.json', 'utf8'))
  expect(parseProject(fixture).graph.nodes.length).toBeGreaterThan(0)
})

test('legacy bridge and complexity migrations still apply after validation', () => {
  const nodes = [
    { ...node('old'), data: { type: 'stencil', params: { bridge_mm: 2, bridge_islands: true } } },
    { ...node('mesh'), data: { type: 'complexity', params: {} } },
    { ...node('fragment'), data: { type: 'bridge_fragments', params: {} } },
  ]
  const parsed = parseProject({ version: 1, graph: { nodes, edges: [] } })
  const migrated = migrateProject(parsed.graph.nodes, parsed.graph.edges)
  expect(migrated.nodes.map((n) => n.data.type)).toEqual(['stencil', 'remesh', 'bridges', 'bridges'])
  expect(migrated.edges[0].target).toBe('old-bridges')
})

test('invalid envelopes and unsupported versions are reported', () => {
  for (const input of [null, [], {}, { version: 3, graph: graph() }, { version: '2', graph: graph() }]) {
    expect(() => parseProject(input)).toThrow(/project/i)
  }
})

test('malformed graph snapshots cannot reach migration', () => {
  const bad = [
    { nodes: null, edges: [] },
    { nodes: [node(), node()], edges: [] },
    { nodes: [{ ...node(), position: { x: Infinity, y: 0 } }], edges: [] },
    { nodes: [{ ...node(), data: { type: 'source', params: [] } }], edges: [] },
    { nodes: [node()], edges: [{ id: 'e', source: 'a', target: 'missing' }] },
    { nodes: [node()], edges: [{ id: 'e', source: 'a', target: 'a', sourceHandle: 5 }] },
  ]
  for (const snapshot of bad) expect(() => validateGraphSnapshot(snapshot)).toThrow(/Invalid project/)
  expect(() => migrateProject(null!, [])).toThrow(/Invalid project/)
})

test('all embedded sources are validated before an import can start', () => {
  for (const images of [null, [], { a: {} }, { a: { name: 'a.png', base64: '%%%' } }]) {
    expect(() => parseProject({ version: 2, graph: graph(), images })).toThrow(/Invalid project/)
  }
  expect(() => parseProject({ version: 2, graph: graph(), models: { a: { name: 'a', base64: 'YQ==', format: 'exe' } } })).toThrow(/format/)
  expect(parseProject({ version: 2, graph: graph(), images: { a: { name: 'a', base64: 'YQ' } } }).images.a.base64).toBe('YQ')
})

test('malformed autosaves are reported to the user on startup', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('studio.graph.v1', '{"nodes":null,"edges":[]}'))
  await page.goto('/')
  await expect(page.getByText(/Could not restore the browser draft/).first()).toBeVisible()
  await expect(page.locator('.react-flow__node')).toHaveCount(0)
})

test('malformed project imports report an error rather than loading', async ({ page }) => {
  await page.goto('/')
  const data = await page.evaluateHandle(() => {
    const transfer = new DataTransfer()
    transfer.items.add(new File([JSON.stringify({ version: 2, graph: { nodes: null, edges: [] } })], 'bad.stencil.json', { type: 'application/json' }))
    return transfer
  })
  await page.locator('.react-flow').dispatchEvent('drop', { dataTransfer: data })
  await expect(page.getByText(/cannot open project: Invalid project/).first()).toBeVisible()
  await expect(page.locator('.react-flow__node')).toHaveCount(0)
})
