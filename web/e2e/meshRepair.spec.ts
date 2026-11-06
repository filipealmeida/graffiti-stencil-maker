import { expect, test, type Page } from '@playwright/test'
import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const vertices = Array.from({ length: 8 }, (_, i) => [i & 1 ? 10 : 0, i & 2 ? 8 : 0, i & 4 ? 2 : 0])
const faces = [[0, 2, 3], [0, 3, 1], [4, 5, 7], [4, 7, 6], [0, 1, 5], [0, 5, 4], [2, 6, 7], [2, 7, 3], [0, 4, 6], [0, 6, 2], [1, 3, 7], [1, 7, 5]]
function stl(triangles = faces, points = vertices) {
  const buffer = Buffer.alloc(84 + triangles.length * 50)
  buffer.writeUInt32LE(triangles.length, 80)
  triangles.forEach((face, i) => face.forEach((index, j) => points[index].forEach((value, k) => {
    buffer.writeFloatLE(value, 84 + i * 50 + 12 + j * 12 + k * 4)
  })))
  return buffer
}
async function open(page: Page, buffer: Buffer, name = 'defective.stl') {
  await page.goto('/')
  await page.getByRole('button', { name: '+ build', exact: true }).click()
  await page.locator('.menu button').filter({ hasText: '3D import' }).click()
  await page.locator('.react-flow__node').dispatchEvent('click')
  await page.locator('.inspector .node-actions input[type=file]').setInputFiles({ name, mimeType: 'model/stl', buffer })
  await expect(page.getByRole('region', { name: 'Mesh properties' })).toContainText('Triangles')
}
const done = (page: Page) => expect(page.locator('.react-flow__node [title=done]')).toHaveCount(1)
const tools = (page: Page) => page.getByRole('region', { name: 'Mesh properties' }).last()
const property = (page: Page, name: string) => tools(page).getByText(name, { exact: true }).locator('+ dd')

test('invalid meshes retain preview and diagnostics; cleaning updates stats, gates solid output, and supports undo', async ({ page }) => {
  await open(page, stl([...faces, [0, 0, 1], faces[0]]))
  await expect(property(page, 'Degenerate faces')).toHaveText('1')
  await expect(property(page, 'Duplicate faces')).toHaveText('1')
  await expect(property(page, 'Solid output')).toHaveText('blocked')
  await expect(page.locator('.inspector').getByRole('link', { name: 'Download STL', exact: true })).toHaveCount(0)
  await expect(page.locator('.console .error').filter({ hasText: 'solid output blocked' }).first()).toBeVisible()
  await page.locator('.react-flow__node').dispatchEvent('dblclick')
  await expect(page.locator('.bench-view canvas')).toBeVisible()
  await expect(page.getByLabel(/show defects/)).toBeChecked()
  await tools(page).getByRole('button', { name: 'Remove degenerate and duplicate faces', exact: true }).click()
  await done(page)
  await expect(property(page, 'Degenerate faces')).toHaveText('0')
  await expect(property(page, 'Triangles')).toHaveText('12')
  await expect(page.locator('.bench .console')).toContainText('14 → 12 triangles')
  await expect(tools(page).getByRole('status')).toContainText('14 → 12 triangles')
  await expect(page.locator('.bench-params').getByRole('link', { name: 'Download STL', exact: true })).toBeVisible()
  await page.waitForTimeout(500)
  await page.keyboard.press('Control+z')
  await expect(property(page, 'Triangles')).toHaveText('14')
  await expect(property(page, 'Solid output')).toHaveText('blocked')
  await expect(page.locator('.bench .console')).toContainText('Undo: restored previous graph')
  await expect(tools(page).getByRole('status')).toHaveCount(0)
  await page.keyboard.press('Control+Shift+z')
  await done(page)
  await expect(property(page, 'Triangles')).toHaveText('12')
  await page.reload()
  await done(page)
})

test('hole filling and winding are separate repairs with refreshed statistics', async ({ page }) => {
  await open(page, stl(faces.filter((_, i) => i !== 2 && i !== 3)))
  await expect(property(page, 'Boundary edges')).toHaveText('4')
  await page.getByRole('button', { name: 'Fill planar holes', exact: true }).click()
  await done(page)
  await expect(property(page, 'Boundary edges')).toHaveText('0')
  await expect(property(page, 'Triangles')).toHaveText('12')
  const reversed = faces.map((face, i) => i === 3 ? [face[0], face[2], face[1]] : face)
  await page.locator('.inspector .node-actions input[type=file]').setInputFiles({ name: 'winding.stl', mimeType: 'model/stl', buffer: stl(reversed) })
  await expect(property(page, 'Winding conflicts')).not.toHaveText('0')
  await expect(tools(page).getByRole('status')).toHaveCount(0)
  await page.getByRole('button', { name: 'Fix winding', exact: true }).click()
  await done(page)
  await expect(property(page, 'Winding conflicts')).toHaveText('0')
})

test('tolerance welding fixes a crack and unsafe hole filling logs a visible failure', async ({ page }) => {
  const points = [...vertices, [0.001, 0, 0]]
  const cracked = faces.map((face, i) => i === 0 ? [8, face[1], face[2]] : face)
  await open(page, stl(cracked, points))
  await expect(property(page, 'Boundary edges')).not.toHaveText('0')
  await page.getByRole('button', { name: 'Weld nearby vertices', exact: true }).click()
  await done(page)
  await expect(property(page, 'Boundary edges')).toHaveText('0')
  const warped = vertices.map((p, i) => i === 7 ? [p[0], p[1], 3] : p)
  await page.locator('.inspector .node-actions input[type=file]').setInputFiles({ name: 'nonplanar.stl', mimeType: 'model/stl', buffer: stl(faces.filter((_, i) => i !== 2 && i !== 3), warped) })
  await expect(property(page, 'Boundary edges')).toHaveText('4')
  await page.getByRole('button', { name: 'Fill planar holes', exact: true }).click()
  await expect(page.locator('.console .error').filter({ hasText: 'hole is not planar' }).last()).toBeVisible()
  await expect(property(page, 'Triangles')).toHaveText('10')
})

test('malformed uploads, repeated failures, edits, undo and downloads log results', async ({ page }) => {
  await open(page, stl())
  await done(page)
  await expect(page.locator('.console')).toContainText('Added defective.stl')
  for (let i = 0; i < 2; i++) {
    await page.locator('.inspector .node-actions input[type=file]').setInputFiles({ name: 'broken.stl', mimeType: 'model/stl', buffer: Buffer.from('not a mesh') })
    await expect(page.locator('.console .error').filter({ hasText: 'not a valid STL' })).toHaveCount(i + 1)
  }
  await page.locator('.inspector input.numbox').first().fill('2')
  await page.locator('.inspector input.numbox').first().blur()
  await done(page)
  await expect(page.locator('.console')).toContainText('Edit applied: scale=2')
  await page.locator('.inspector h2').click()
  await page.keyboard.press('Control+z')
  await expect(page.locator('.console')).toContainText('Undo: restored previous graph')
  await page.locator('.react-flow__node').dispatchEvent('click')
  await Promise.all([page.waitForEvent('download'), page.locator('.inspector .node-actions button').click()])
  await expect(page.locator('.console')).toContainText('Source download handed to browser')
  await Promise.all([page.waitForEvent('download'), page.keyboard.press('Control+s')])
  await expect(page.locator('.console')).toContainText('Save project: started')
  await expect(page.locator('.console')).toContainText('Saved ')
})

test('saved projects preserve defective sources and repaired working copies in a fresh browser', async ({ page, browser }) => {
  await open(page, stl([...faces, [0, 0, 1]]))
  for (const triangles of ['13', '12']) {
    const [download] = await Promise.all([page.waitForEvent('download'), page.keyboard.press('Control+s')])
    const saved = readFileSync((await download.path())!)
    const context = await browser.newContext()
    try {
      const fresh = await context.newPage()
      await fresh.goto('/')
      await fresh.locator('#open-project').setInputFiles({ name: 'mesh.stencil.json', mimeType: 'application/json', buffer: saved })
      await fresh.locator('.react-flow__node').dispatchEvent('click')
      await expect(property(fresh, 'Triangles')).toHaveText(triangles)
      await expect(property(fresh, 'Solid output')).toHaveText(triangles === '13' ? 'blocked' : 'eligible (after node size checks)')
      if (triangles === '12') await done(fresh)
    } finally { await context.close() }
    if (triangles === '13') {
      await page.getByRole('button', { name: 'Remove degenerate and duplicate faces', exact: true }).click()
      await done(page)
      await expect(property(page, 'Triangles')).toHaveText('12')
    }
  }
})

test('the sandbox IKEA mesh can be inspected and explicitly cleaned', async ({ page }) => {
  const path = fileURLToPath(new URL('../../sandbox/IkeaSKADIShexagon.stl', import.meta.url))
  test.skip(!existsSync(path), 'Optional real-world sandbox model is not committed.')
  await open(page, readFileSync(path), 'IkeaSKADIShexagon.stl')
  await expect(property(page, 'Degenerate faces')).toHaveText('3468')
  await expect(property(page, 'Triangles')).toHaveText('107,392')
  await page.getByRole('button', { name: 'Remove degenerate and duplicate faces', exact: true }).click()
  await done(page)
  await expect(property(page, 'Degenerate faces')).toHaveText('0')
  await expect(property(page, 'Triangles')).toHaveText('103,924')
})
