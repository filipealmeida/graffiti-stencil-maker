import { expect, test } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { badgesFor, badgesForNode } from '../src/badges'
import { api } from '../src/api'

// The same cases the Rust side checks in crates/stencil-core/tests/findings_parity.rs.
const cases = JSON.parse(readFileSync(new URL('../../crates/stencil-core/tests/fixtures/findings.json', import.meta.url), 'utf8'))

for (const c of cases) {
  test(`badges match the engine findings: ${c.name}`, () => {
    expect(badgesFor(c.type, c.params, c.info)).toEqual(c.expected)
  })
}

type Graph = Parameters<typeof badgesForNode>[0]

const peekInfo = api.peekInfo
test.afterEach(() => { api.peekInfo = peekInfo })

function graph(): Graph {
  api.peekInfo = () => ({ stats: { islands_found: 1, islands_remaining: 0 } })
  return {
    nodes: [
      { id: 'stencil', position: { x: 0, y: 0 }, data: { type: 'vector_stencil', params: {} } },
      { id: 'bridges', position: { x: 300, y: 0 }, data: { type: 'bridges', params: {} } },
    ],
    edges: [{ id: 'link', source: 'stencil', sourceHandle: 'solid', target: 'bridges', targetHandle: 'body' }],
    runs: { bridges: { state: 'done', key: 'bridged', outputs: { solid: { available: true, type: 'solid' } } } },
    types: { bridges: { id: 'bridges', label: 'Bridges', stage: 'post', inputs: [], outputs: [{ name: 'solid', type: 'solid' }], params: [], doc: '' } },
  }
}

const findings = (g: Graph) => badgesForNode(g, 'stencil', 'solid', { stats: { islands_remaining: 1 } })

test('upstream islands become a note only with confirmed downstream bridging', () => {
  const g = graph()
  expect(findings(g)).toEqual([{ level: 'info', text: '1 island bridged downstream' }])
  expect(badgesForNode(g, 'stencil', 'solid', { stats: { islands_remaining: 1, watertight: false } }))
    .toContainEqual({ level: 'error', text: 'mesh is not watertight' })
  g.edges = []
  expect(findings(g)[0].level).toBe('error')
})

test('failed, stale, pending and running bridges never resolve upstream findings', () => {
  const g = graph()
  for (const state of ['error', 'pending', 'running'] as const) {
    g.runs.bridges.state = state
    expect(findings(g)[0].level).toBe('error')
  }
  g.runs.bridges.state = 'done'
  g.runs.bridges.stale = true
  expect(findings(g)[0].level).toBe('error')
})

test('unresolved, missing and unavailable downstream stats keep the error', () => {
  const g = graph()
  api.peekInfo = () => ({ stats: { islands_found: 1, islands_remaining: 1 } })
  expect(findings(g)[0].level).toBe('error')
  api.peekInfo = () => ({})
  expect(findings(g)[0].level).toBe('error')
  api.peekInfo = () => ({ stats: { islands_found: 1, islands_remaining: 0 } })
  g.runs.bridges.outputs!.solid.available = false
  expect(findings(g)[0].level).toBe('error')
})

test('an unresolved fanout branch keeps upstream islands actionable', () => {
  const g = graph()
  g.nodes.push({ id: 'tiles', position: { x: 300, y: 200 }, data: { type: 'tiles', params: {} } })
  g.edges.push({ id: 'other', source: 'stencil', sourceHandle: 'solid', target: 'tiles', targetHandle: 'solid' })
  expect(findings(g)[0].level).toBe('error')
})
