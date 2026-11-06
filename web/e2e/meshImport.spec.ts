import { expect, test, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'

const vertices = Array.from({ length: 8 }, (_, i) => [i & 1 ? 30 : -10, i & 2 ? 15 : -5, i & 4 ? 0 : -2])
const faces = [[0, 2, 3], [0, 3, 1], [4, 5, 7], [4, 7, 6], [0, 1, 5], [0, 5, 4], [2, 6, 7], [2, 7, 3], [0, 4, 6], [0, 6, 2], [1, 3, 7], [1, 7, 5]]
const ascii = Buffer.from(`solid plate\n${faces.map((t) => `facet normal 0 0 0\nouter loop\n${t.map((i) => `vertex ${vertices[i].join(' ')}`).join('\n')}\nendloop\nendfacet`).join('\n')}\nendsolid plate`)
const obj = Buffer.from(`${vertices.map((p) => `v ${p.join(' ')}`).join('\n')}\n${faces.map((t) => `f ${t.map((i) => i + 1).join(' ')}`).join('\n')}`)

const done = (page: Page) => expect(page.locator('.react-flow__node [title=done]')).toHaveCount(1)

async function addImport(page: Page, buffer = ascii) {
  await page.goto('/')
  await page.getByRole('button', { name: '+ build', exact: true }).click()
  await page.locator('.menu button').filter({ hasText: '3D import' }).click()
  await page.locator('.react-flow__node').dispatchEvent('click')
  await expect(page.locator('aside')).toContainText('no 3D model loaded')
  await page.locator('aside .node-actions input[type=file]').setInputFiles({ name: 'plate.stl', mimeType: 'model/stl', buffer })
  await done(page)
}

test('3D import is in Build, previews a solid, and replaces and undoes the source', async ({ page }) => {
  await addImport(page)
  await expect(page.locator('aside')).toContainText('size mm')
  await expect(page.locator('aside')).toContainText('[40,20,2]')
  await page.locator('.react-flow__node').dispatchEvent('dblclick')
  await expect(page.locator('.bench-view canvas')).toBeVisible()
  await expect(page.locator('.bench-params .node-actions input[type=file]')).toHaveAttribute('accept', '.stl,.obj')
  await page.waitForTimeout(600)
  await page.locator('.bench-params .node-actions input[type=file]').setInputFiles({ name: 'replacement.obj', mimeType: 'text/plain', buffer: obj })
  await expect(page.locator('.bench-head h2')).toContainText('replacement.obj')
  await done(page)
  await page.waitForTimeout(600)
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: /Undo/ }).click()
  await expect(page.locator('.react-flow__node')).toContainText('plate.stl')
  await done(page)
})

test('imported models survive reload and saved projects reopen in a fresh browser', async ({ page, browser }) => {
  await addImport(page)
  await page.reload()
  await done(page)
  const [download] = await Promise.all([page.waitForEvent('download'), page.keyboard.press('Control+s')])
  const saved = readFileSync((await download.path())!)
  const project = JSON.parse(saved.toString())
  expect(Object.values(project.models)).toEqual([{ name: 'plate.stl', format: 'stl', base64: ascii.toString('base64') }])
  expect(project.images).toEqual({})
  const context = await browser.newContext()
  try {
    const fresh = await context.newPage()
    await fresh.goto('/')
    await fresh.locator('#open-project').setInputFiles({ name: 'plate.stencil.json', mimeType: 'application/json', buffer: saved })
    await done(fresh)
    await fresh.locator('.react-flow__node').dispatchEvent('click')
    await expect(fresh.locator('aside')).toContainText('[40,20,2]')
  } finally {
    await context.close()
  }
})

test('dropping a model creates an import node and invalid replacement preserves the source', async ({ page }) => {
  await page.goto('/')
  const data = await page.evaluateHandle((contents) => {
    const transfer = new DataTransfer()
    transfer.items.add(new File([contents], 'plate.OBJ', { type: 'text/plain' }))
    return transfer
  }, obj.toString())
  await page.locator('.react-flow').dispatchEvent('drop', { dataTransfer: data })
  await done(page)
  await expect(page.locator('.react-flow__node')).toContainText('plate.OBJ')
  await page.locator('.react-flow__node').dispatchEvent('click')
  await page.locator('aside .node-actions input[type=file]').setInputFiles({ name: 'broken.stl', mimeType: 'model/stl', buffer: Buffer.from('not a mesh') })
  await expect(page.getByText('not a valid STL: expected binary STL or an ASCII solid', { exact: false }).first()).toBeVisible()
  await expect(page.locator('.react-flow__node')).toContainText('plate.OBJ')
  await done(page)
})

test('single-thread fallback imports binary STL with a solid header', async ({ page }) => {
  await page.route('http://localhost:5199/', async (route) => {
    const response = await route.fetch()
    const headers = response.headers()
    delete headers['cross-origin-opener-policy']
    await route.fulfill({ response, headers })
  })
  const buffer = Buffer.alloc(84 + faces.length * 50)
  buffer.write('solid binary model')
  buffer.writeUInt32LE(faces.length, 80)
  faces.forEach((face, i) => face.forEach((index, j) => vertices[index].forEach((value, k) => {
    buffer.writeFloatLE(value, 84 + i * 50 + 12 + j * 12 + k * 4)
  })))
  await addImport(page, buffer)
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(false)
  await expect(page.locator('aside')).toContainText('[40,20,2]')
})
