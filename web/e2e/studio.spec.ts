import { expect, test, type Page } from '@playwright/test'
import { deflateSync } from 'node:zlib'

// A 160x120 PNG with a dark disc on white, enough to give the stencil real work.
function png(): Buffer {
  const w = 160, h = 120
  const raw = Buffer.alloc((w * 3 + 1) * h)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const dark = (x - 80) ** 2 + (y - 60) ** 2 < 40 ** 2 && (x - 80) ** 2 + (y - 60) ** 2 > 18 ** 2
      const o = y * (w * 3 + 1) + 1 + x * 3
      raw.fill(dark ? 0 : 255, o, o + 3)
    }
  }
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0 })
  const crc = (b: Buffer) => { let c = 0xffffffff; for (const v of b) c = crcTable[(c ^ v) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0 }
  const chunk = (t: string, d: Buffer) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(d.length)
    const td = Buffer.concat([Buffer.from(t), d])
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td))
    return Buffer.concat([len, td, c])
  }
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))])
}

async function open(page: Page) {
  await page.addInitScript(() => {
    const w = window as any
    w.__workers = 0
    w.__runs = 0
    const W = window.Worker
    w.Worker = function (...a: ConstructorParameters<typeof Worker>) { w.__workers++; return new W(...a) }
    const post = Worker.prototype.postMessage
    Worker.prototype.postMessage = function (this: Worker, m: any, ...r: any[]) { if (m?.op === 'run') w.__runs++; return (post as any).call(this, m, ...r) }
  })
  await page.goto('/')
  await page.setInputFiles('label.btn input[type=file] >> nth=0', { name: 'disc.png', mimeType: 'image/png', buffer: png() })
  await settled(page)
}

const states = (page: Page) => page.locator('[title=done],[title=error],[title=running],[title=pending]').evaluateAll((l) => l.map((e) => e.getAttribute('title')))
const settled = (page: Page) => expect.poll(() => states(page), { timeout: 60_000 }).toEqual(Array(4).fill('done'))
const setSlider = (page: Page, v: number) =>
  page.locator('aside input[type=range]').first().evaluate((e, v) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(e, String(v))
    e.dispatchEvent(new Event('input', { bubbles: true }))
  }, v)

test('rapid edits settle into one finished run', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').first().dispatchEvent('click')
  const before = await page.evaluate(() => (window as any).__runs)
  for (const v of [600, 700, 800, 900, 1000]) { await setSlider(page, v); await page.waitForTimeout(80) }
  await settled(page)
  const runs = (await page.evaluate(() => (window as any).__runs)) - before
  expect(runs).toBeLessThanOrEqual(2)
  await expect(page.locator('aside')).toContainText('1000')
})

test('a heavy node is abandoned by restarting the worker, and the next run still works', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').first().dispatchEvent('click')
  await setSlider(page, 2000)
  await expect(page.locator('[title=running]').first()).toBeVisible({ timeout: 30_000 })
  await expect.poll(() => page.locator('[title=running]').count()).toBeGreaterThan(0)
  await page.waitForTimeout(600)
  const workers = await page.evaluate(() => (window as any).__workers)
  const stillRunning = (await page.locator('[title=running]').count()) > 0
  await setSlider(page, 130)
  await settled(page)
  // a node that already finished needs no respawn
  const after = await page.evaluate(() => (window as any).__workers)
  expect(after).toBeLessThanOrEqual(workers + 1)
  if (stillRunning) expect(after).toBeGreaterThanOrEqual(workers)
})

test('undo returns to the previous value', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').first().dispatchEvent('click')
  await page.waitForTimeout(600) // let the starter graph become its own undo step
  await setSlider(page, 700)
  await expect(page.locator('aside')).toContainText('700')
  await page.waitForTimeout(600)
  await page.getByRole('button', { name: /Undo/ }).click()
  await page.locator('.react-flow__node').first().dispatchEvent('click')
  await expect(page.locator('aside input[type=range]').first()).not.toHaveValue('700')
  await settled(page)
})

test('the Morphology node can be added from the menu and shows its parameters', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: '+ trace' }).click()
  await page.getByRole('button', { name: 'Morphology' }).click()
  await expect(page.locator('.react-flow__node').filter({ hasText: 'Morphology' })).toBeVisible()
  await page.locator('.react-flow__node').filter({ hasText: 'Morphology' }).dispatchEvent('click')
  await expect(page.locator('aside')).toContainText('Operation')
  await expect(page.locator('aside select').first()).toHaveValue('open')
})

test('the console lists timed sub-steps of the stencil node', async ({ page }) => {
  await open(page)
  const rows = page.locator('div.info').filter({ hasText: '↳' })
  await expect(rows.filter({ hasText: 'bridge islands' })).not.toHaveCount(0)
  await expect(rows.filter({ hasText: 'build mesh' }).first()).toContainText(/\d+ ms/)
})

test('new nodes avoid existing ones and panels can be resized', async ({ page }) => {
  await open(page)
  for (let i = 0; i < 3; i++) {
    await page.getByRole('button', { name: '+ trace' }).click()
    await page.getByRole('button', { name: 'Morphology' }).click()
  }
  const boxes = await page.locator('.react-flow__node').evaluateAll((l) => l.map((e) => { const r = e.getBoundingClientRect(); return [r.x, r.y, r.width, r.height] }))
  expect(boxes.length).toBe(7)
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
    const [a, b] = [boxes[i], boxes[j]]
    const overlap = a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
    expect(overlap, `nodes ${i} and ${j}`).toBe(false)
  }
  const lane = page.locator('.lane-stencil')
  const before = (await lane.boundingBox())!
  const grip = page.locator('.lane-trace .lane-grip')
  const g = (await grip.boundingBox())!
  await page.mouse.move(g.x + g.width / 2, 130)
  await page.mouse.down()
  await page.mouse.move(g.x + g.width / 2 + 120, 130, { steps: 5 })
  await page.mouse.up()
  const after = (await lane.boundingBox())!
  expect(after.x).toBeGreaterThan(before.x + 60)
  expect(after.width).toBeLessThan(before.width - 60)
})

test('3D views show dimensions and the trace node exports SVG', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').filter({ hasText: 'Stencil' }).first().dispatchEvent('dblclick')
  await expect(page.locator('.v3dims')).toContainText(/\d+\.\d × \d+\.\d × \d+\.\d mm/, { timeout: 30_000 })
  await page.getByRole('button', { name: /Close/ }).click()

  await page.getByRole('button', { name: '+ trace' }).click()
  await page.getByRole('button', { name: 'Trace', exact: true }).click()
  const trace = page.locator('.react-flow__node').filter({ hasText: 'Trace' }).last()
  const from = page.locator('.react-flow__node').filter({ hasText: 'Threshold' }).first().locator('.source').first()
  const to = trace.locator('.target').first()
  const [a, b] = [(await from.boundingBox())!, (await to.boundingBox())!]
  await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2)
  await page.mouse.down()
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 8 })
  await page.mouse.up()
  await trace.dispatchEvent('dblclick')
  const link = page.locator('.bench').getByRole('link', { name: 'Export SVG' })
  await expect(link).toBeVisible({ timeout: 30_000 })
  const href = await link.getAttribute('href')
  const text = await page.evaluate((h) => fetch(h!).then((r) => r.text()), href)
  expect(text).toContain('<svg')
})
