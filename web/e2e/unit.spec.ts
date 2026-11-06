import { expect, test } from '@playwright/test'
import type { Edge } from '@xyflow/react'
import type { StudioNode } from '../src/store'
import { badgesFor, badgesForNode } from '../src/badges'
import { api } from '../src/api'
import { etaSeconds, formatEta, nodeStarts } from '../src/eta'
import { applyDrag, checkPocket, checkSeam, hitTest, isEmpty, parseEdits, stringify } from '../src/layoutEdit'
import type { LayoutInfo } from '../src/layoutEdit'
import { migrateProject } from '../src/migrate'
import { ancestorsOf, plan, splitPlan } from '../src/plan'

const studioNode = (id: string, type: string, params: Record<string, unknown> = {}, x = 0, y = 0): StudioNode => ({
  id, type: 'studio', position: { x, y }, data: { type, params },
})

const edge = (source: string, sourceHandle: string, target: string, targetHandle: string): Edge => ({
  id: `${source}.${sourceHandle}->${target}.${targetHandle}`, source, sourceHandle, target, targetHandle,
})

test('migration renames legacy nodes, remaps their input, and extracts bridges', () => {
  const nodes = [
    studioNode('source', 'source'),
    studioNode('old', 'bridge_fragments', { bridge_mm: 0.8 }, 10, 20),
    studioNode('stencil', 'stencil', { bridge_mm: 0.6, bridges_per_island: 2, bridge_islands: true, width_mm: 50 }),
    studioNode('end', 'mesh_export'),
  ]
  const edges = [
    edge('source', 'solid', 'old', 'parts'),
    edge('stencil', 'solid', 'end', 'body'),
  ]
  const result = migrateProject(nodes, edges)

  expect(result.nodes.find((n) => n.id === 'old')?.data.type).toBe('bridges')
  expect(result.edges[0]).toEqual(edge('source', 'solid', 'old', 'body'))
  expect(result.nodes.find((n) => n.id === 'stencil')?.data.params).toEqual({ width_mm: 50 })
  expect(result.nodes.find((n) => n.id === 'stencil-bridges')?.data.params).toEqual({ bridge_mm: 0.6, bridges_per_island: 2 })
  expect(result.edges).toContainEqual(edge('stencil', 'solid', 'stencil-bridges', 'body'))
  expect(result.edges).toContainEqual(edge('stencil-bridges', 'solid', 'end', 'body'))
})

test('migration honors disabled bridging and avoids generated node ID collisions', () => {
  const result = migrateProject([
    studioNode('a', 'vector_stencil', { bridge_islands: true }),
    studioNode('a-bridges', 'bridges'),
    studioNode('b', 'tiles', { bridge_islands: false, bridge_passes: 3, width_mm: 90 }),
  ], [])
  expect(result.nodes.map((n) => n.id)).toContain('a-bridges_')
  expect(result.nodes.find((n) => n.id === 'b')?.data.params).toEqual({ width_mm: 90 })
  expect(result.nodes.some((n) => n.id === 'b-bridges')).toBe(false)
})

test('planning includes dependencies of requested targets and orders ready work by cost', () => {
  const nodes = ['source', 'slow', 'quick', 'target', 'unused'].map((id) => ({ id, type: id }))
  const edges = [
    { from: ['source', 'out'], to: ['slow', 'in'] },
    { from: ['source', 'out'], to: ['quick', 'in'] },
    { from: ['slow', 'out'], to: ['target', 'left'] },
    { from: ['quick', 'out'], to: ['target', 'right'] },
  ] as const
  expect(plan(nodes, edges as never, ['target'], { slow: 90, quick: 10 })).toEqual(['source', 'quick', 'slow', 'target'])
  expect(ancestorsOf('target', edges as never)).toEqual(new Set(['target', 'slow', 'quick', 'source']))
  expect(plan(nodes, [...edges, { from: ['target', 'out'], to: ['slow', 'cycle'] }] as never, ['target'])).toBeNull()
})

test('split planning balances exclusive branches and avoids splitting an unpriced first run', () => {
  const edges = [
    { from: ['root', 'out'], to: ['left', 'in'] },
    { from: ['root', 'out'], to: ['right', 'in'] },
  ]
  const split = splitPlan(edges as never, ['root', 'left', 'right'], { root: 100, left: 1000, right: 900 }, () => 0)
  expect(split).not.toBeNull()
  expect(split!.targets.flat().sort()).toEqual(['left', 'right'])
  expect(split!.owner.get('root')).toBeDefined()
  expect(splitPlan(edges as never, ['root', 'left', 'right'], {}, () => 0)).toBeNull()
})

const layout: LayoutInfo = {
  width: 100, height: 100, xs: [0, 50, 100], ys: [0, 50, 100], auto_xs: [0, 50, 100], auto_ys: [0, 50, 100],
  pocket_list: [
    { key: 'p1', axis: 0, seam: 1, s: 30, u: 20, auto_u: 18, block: [45, 10, 55, 30], range: [10, 40] },
    { key: 'p2', axis: 0, seam: 1, s: 40, u: 40, auto_u: 40, block: [45, 35, 55, 45], range: [30, 45] },
  ],
  min_tile: 20, max: [60, 60], gap: 2, quantum: 5, edits_ignored: 0,
}

test('layout edits parse safely, omit empty edits, and snap valid seams', () => {
  expect(parseEdits('{bad')).toEqual({ seams: {}, pockets: {} })
  expect(isEmpty(parseEdits('{}'))).toBe(true)
  expect(stringify({ seams: {}, pockets: {} })).toBe('')
  expect(parseEdits(stringify({ seams: { x1: 5 }, pockets: {} }))).toEqual({ seams: { x1: 5 }, pockets: {} })
  expect(checkSeam(layout, 0, 1, 53)).toEqual({ v: 55 })
  expect(checkSeam(layout, 0, 1, 13).why).toContain('connectors need at least')
})

test('layout hit testing prioritizes blocks and rejects colliding pocket drags', () => {
  expect(hitTest(layout, 50, 20, 2)).toEqual({ kind: 'pocket', key: 'p1' })
  expect(hitTest(layout, 51, 70, 2)).toEqual({ kind: 'seam', axis: 0, i: 1 })
  expect(checkPocket(layout, 'p1', 42).why).toContain('stay on its seam')
  expect(checkPocket(layout, 'p1', 30).why).toContain('touch the block')
  expect(applyDrag(layout, { seams: { x1: 3 }, pockets: { p1: 0.5 } }, { kind: 'seam', axis: 0, i: 1, v: 50 }))
    .toEqual({ seams: {}, pockets: { p1: 0.5 } })
})

test('ETA estimates active progress and unfinished stale nodes but ignores cached results', () => {
  nodeStarts.work = 1_000
  expect(etaSeconds({ work: { state: 'running', progress: 0.5 }, cached: { state: 'done', stale: true, cached: true, ms: 500 } }, 5_000))
    .toBe(4)
  expect(etaSeconds({ next: { state: 'done', stale: true, cached: false, ms: 3_000 } }, 0)).toBe(3)
  expect(etaSeconds({ done: { state: 'done', cached: true, ms: 3_000 } }, 0)).toBeNull()
  delete nodeStarts.work
})

test('ETA formatting keeps short, seconds, and minutes labels readable', () => {
  expect(formatEta(0.9)).toBe('almost done')
  expect(formatEta(12)).toBe('~12 s left')
  expect(formatEta(90)).toBe('~2 min left')
})

test('badges report threshold findings and distinguish serious from minor paint loss', () => {
  expect(badgesFor('mesh', {}, { stats: { islands_remaining: 2, watertight: false, triangles: 1_600_000 } })).toEqual([
    { level: 'error', text: '2 islands remain' },
    { level: 'error', text: 'mesh is not watertight' },
    { level: 'warn', text: '1.6 M triangles: heavy to slice' },
  ])
  expect(badgesFor('tiles', {}, { kind: 'tiles', paint_under_blocks_mm2: 6, paint_mm2: 100 })).toContainEqual({
    level: 'warn', text: '6 mm² of paint under connector blocks (6.0% of the artwork)',
  })
  expect(badgesFor('mask', {}, { kind: 'mask', coverage: 0.999 })).toContainEqual({ level: 'warn', text: 'mask is almost all paint' })
})

test('upstream island findings soften only when current downstream bridges resolve them', () => {
  const originalPeekInfo = api.peekInfo
  api.peekInfo = () => ({ stats: { islands_found: 1, islands_remaining: 0 } })
  const graph = {
    nodes: [studioNode('stencil', 'stencil'), studioNode('bridges', 'bridges')],
    edges: [edge('stencil', 'solid', 'bridges', 'body')],
    runs: { bridges: { state: 'done' as const, key: 'fresh', outputs: { solid: { available: true, type: 'solid' as const } } } },
    types: { bridges: { id: 'bridges', label: 'Bridges', stage: 'post' as const, inputs: [], outputs: [{ name: 'solid', type: 'solid' as const }], params: [], doc: '' } },
  }
  try {
    expect(badgesForNode(graph, 'stencil', 'solid', { stats: { islands_remaining: 1 } })).toEqual([
      { level: 'info', text: '1 island bridged downstream' },
    ])
    graph.runs.bridges.stale = true
    expect(badgesForNode(graph, 'stencil', 'solid', { stats: { islands_remaining: 1 } })[0].level).toBe('error')
    expect(badgesForNode(graph, 'missing', 'solid', null)).toEqual([])
  } finally {
    api.peekInfo = originalPeekInfo
  }
})

test('store loading migrates and fills defaults while clearing transient run and selection state', async () => {
  const values = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value) },
  })
  const { useStudio } = await import('../src/store')
  const state = useStudio.getState()
  state.setTypes([{
    id: 'remesh', label: 'Remesh', stage: 'post', inputs: [], outputs: [], doc: '',
    params: [{ name: 'quality', label: 'Quality', kind: 'int', default: 4 }],
  }])
  useStudio.setState({
    selected: 'old', runs: { old: { state: 'done', key: 'cached' } },
  })
  state.load([studioNode('old', 'complexity', { detail: 2 })], [])
  expect(useStudio.getState().nodes[0].data).toMatchObject({ type: 'remesh', params: { detail: 2, quality: 4 } })
  expect(useStudio.getState().runs).toEqual({})
  expect(useStudio.getState().selected).toBeNull()
})

test('store removal prunes connected edges and clears selected node', async () => {
  const { useStudio } = await import('../src/store')
  const source = studioNode('source', 'source')
  const target = studioNode('target', 'stencil')
  useStudio.setState({ nodes: [source, target], edges: [edge('source', 'image', 'target', 'mask')], selected: 'target' })
  useStudio.getState().removeNode('target')
  expect(useStudio.getState().nodes.map((n) => n.id)).toEqual(['source'])
  expect(useStudio.getState().edges).toEqual([])
  expect(useStudio.getState().selected).toBeNull()
})
