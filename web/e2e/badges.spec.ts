import { expect, test } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { badgesFor } from '../src/badges'

// The same cases the Rust side checks in crates/stencil-core/tests/findings_parity.rs.
const cases = JSON.parse(readFileSync(new URL('../../crates/stencil-core/tests/fixtures/findings.json', import.meta.url), 'utf8'))

for (const c of cases) {
  test(`badges match the engine findings: ${c.name}`, () => {
    expect(badgesFor(c.type, c.params, c.info)).toEqual(c.expected)
  })
}
