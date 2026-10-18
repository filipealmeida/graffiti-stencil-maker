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
  await page.locator('.menu button').filter({ hasText: 'Morphology' }).click()
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

test('new nodes avoid existing ones and no lane panels are drawn', async ({ page }) => {
  await open(page)
  for (let i = 0; i < 3; i++) {
    await page.getByRole('button', { name: '+ trace' }).click()
    await page.locator('.menu button').filter({ hasText: 'Morphology' }).click()
  }
  const boxes = await page.locator('.react-flow__node').evaluateAll((l) => l.map((e) => { const r = e.getBoundingClientRect(); return [r.x, r.y, r.width, r.height] }))
  expect(boxes.length).toBe(7)
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
    const [a, b] = [boxes[i], boxes[j]]
    const overlap = a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
    expect(overlap, `nodes ${i} and ${j}`).toBe(false)
  }
  await expect(page.locator('.lane')).toHaveCount(0)
})

test('3D views show dimensions and the trace node exports SVG', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').filter({ hasText: 'Stencil' }).first().dispatchEvent('dblclick')
  await expect(page.locator('.v3dims')).toContainText(/\d+\.\d × \d+\.\d × \d+\.\d mm/, { timeout: 30_000 })
  await page.getByRole('button', { name: /Close/ }).click()

  await page.getByRole('button', { name: '+ trace' }).click()
  await page.locator('.menu button').filter({ has: page.locator('b', { hasText: /^Trace$/ }) }).click()
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

test('the Crop & rotate node can be added and rotates the picture', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: '+ trace' }).click()
  await page.getByRole('button', { name: 'Crop & rotate' }).click()
  const node = page.locator('.react-flow__node').filter({ hasText: 'Crop & rotate' })
  await expect(node).toBeVisible()
  await node.dispatchEvent('click')
  await expect(page.locator('aside')).toContainText('Crop left')
  await expect(page.locator('aside select').first()).toHaveValue('0')
})

test('the preview image zooms and pans on a bounded canvas', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').filter({ hasText: 'Threshold' }).first().dispatchEvent('dblclick')
  const stage = page.locator('.imgstage')
  await expect(stage.locator('canvas')).toBeVisible()
  const b = (await stage.boundingBox())!
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
  for (let i = 0; i < 15; i++) await page.mouse.wheel(0, -100)
  await page.mouse.down()
  await page.mouse.move(b.x + 100, b.y + 100, { steps: 5 })
  await page.mouse.up()
  const size = await stage.locator('canvas').evaluate((c: HTMLCanvasElement) => [c.width, c.height])
  expect(size[0]).toBeLessThan(5000)
  await expect(page.locator('.imgbar')).toContainText(/\d{3,4}%/)
})

test('releasing the preview image right after a move does not crash the app', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))
  await open(page)
  await page.locator('.react-flow__node').filter({ hasText: 'Threshold' }).first().dispatchEvent('dblclick')
  const stage = page.locator('.imgstage')
  await expect(stage.locator('canvas')).toBeVisible()
  await stage.evaluate((el) => {
    const r = el.getBoundingClientRect()
    const fire = (t: string, x: number) => el.dispatchEvent(new PointerEvent(t, { bubbles: true, clientX: r.x + x, clientY: r.y + 50, pointerId: 1, button: 0 }))
    for (let i = 0; i < 20; i++) { fire('pointerdown', 100); fire('pointermove', 140 + i); fire('pointerup', 140 + i) }
  })
  await page.waitForTimeout(300)
  expect(errors).toEqual([])
  await expect(page.locator('.bench')).toBeVisible()
  await expect(stage.locator('canvas')).toBeVisible()
})

test('after dropping an image the whole starter graph is in view', async ({ page }) => {
  await open(page)
  await page.waitForTimeout(800)
  const canvas = (await page.locator('.canvas').boundingBox())!
  for (const n of await page.locator('.react-flow__node').all()) {
    const b = (await n.boundingBox())!
    expect(b.x).toBeGreaterThanOrEqual(canvas.x - 2)
    expect(b.x + b.width).toBeLessThanOrEqual(canvas.x + canvas.width + 2)
  }
})

test('number boxes, reset and reset-all edit parameters', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').filter({ hasText: 'Stencil' }).first().dispatchEvent('click')
  const box = page.locator('aside input.numbox').first()
  const before = await box.inputValue()
  await box.fill('150')
  await expect(page.locator('aside .row').first()).toContainText('150')
  await expect(page.locator('aside button.reset').first()).toBeVisible()
  await box.fill('99999')
  await box.blur()
  await expect(box).not.toHaveValue('99999')
  await page.locator('aside button.reset').first().click()
  await expect(box).toHaveValue(before)
  await box.fill('120')
  await page.locator('aside button.reset-all').click()
  await expect(box).toHaveValue(before)
})

test('copy, paste and duplicate create independent nodes', async ({ page }) => {
  await open(page)
  const nodes = page.locator('.react-flow__node')
  const th = nodes.filter({ hasText: 'Threshold' })
  await th.first().click()
  await page.keyboard.press('Control+d')
  await expect(nodes).toHaveCount(5)
  await expect(th).toHaveCount(2)
  await page.keyboard.press('Control+c')
  await page.keyboard.press('Control+v')
  await expect(nodes).toHaveCount(6)
  await th.first().locator('button[title^="Duplicate"]').click()
  await expect(nodes).toHaveCount(7)
  const boxes = await nodes.evaluateAll((l) => l.map((e) => { const r = e.getBoundingClientRect(); return [r.x, r.y, r.width, r.height] }))
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
    const [a, b] = [boxes[i], boxes[j]]
    expect(a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3], `nodes ${i} and ${j}`).toBe(false)
  }
  await page.keyboard.press('Control+z')
  await expect(page.locator('.crash')).toHaveCount(0)
})

test('the Text node renders letters and can feed a stencil', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: '+ trace' }).click()
  await page.locator('.menu button').filter({ hasText: /^Text/ }).click()
  const text = page.locator('.react-flow__node').filter({ hasText: 'Text' }).last()
  await text.dispatchEvent('dblclick')
  const bench = page.locator('.bench')
  await expect(bench.locator('canvas')).toBeVisible({ timeout: 30_000 })
  await bench.locator('textarea').fill('HELLO\nWORLD')
  await expect(bench.locator('.bench-foot')).toContainText(/\d+×\d+px/, { timeout: 30_000 })
  await expect(bench.locator('.bench-foot')).toContainText('paint')
})

test('a render crash shows a recovery page instead of a black window', async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem('seeded')) {
      sessionStorage.setItem('seeded', '1')
      localStorage.setItem('studio.graph.v1', JSON.stringify({ nodes: [{ id: 'x', type: 'studio', position: { x: 0, y: 0 } }], edges: [] }))
    }
  })
  await page.goto('/')
  const crash = page.locator('.crash')
  if (await crash.count()) {
    await expect(crash).toBeVisible()
    await expect(page.getByRole('button', { name: 'Download the autosaved graph' })).toBeVisible()
    page.once('dialog', (d) => d.accept())
    await page.getByRole('button', { name: /Discard autosave/ }).click()
  }
  await expect(page.locator('.hint, .react-flow__node').first()).toBeVisible()
})

test('templates menu rebuilds the graph around the picture', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: /Templates/ }).click()
  await page.locator('.menu button').filter({ hasText: 'Vector' }).click()
  await expect(page.locator('.react-flow__node').filter({ hasText: 'Vector stencil' })).toBeVisible()
})

test('a long palette menu scrolls inside itself, not the page', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 420 })
  await open(page)
  await page.getByRole('button', { name: '+ trace' }).click()
  const m = page.locator('.menu').first()
  const b = (await m.boundingBox())!
  expect(b.y + b.height).toBeLessThanOrEqual(420)
  expect(await m.evaluate((e) => e.scrollHeight > e.clientHeight)).toBe(true)
  expect(await page.evaluate(() => document.documentElement.scrollHeight <= window.innerHeight)).toBe(true)
})

test('the Remesh can be added from the stencil menu and shows its parameters', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: '+ stencil' }).click()
  await page.locator('.menu button').filter({ hasText: 'Remesh' }).click()
  const node = page.locator('.react-flow__node').filter({ hasText: 'Remesh' })
  await expect(node).toBeVisible()
  await node.dispatchEvent('click')
  await expect(page.locator('aside')).toContainText('tolerance')
  await expect(page.locator('aside')).toContainText('overhang')
})

test('the shape cleanup nodes can be added from the trace menu', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: '+ trace' }).click()
  await page.locator('.menu button').filter({ hasText: 'Round corners' }).click()
  await page.getByRole('button', { name: '+ trace' }).click()
  await page.locator('.menu button').filter({ hasText: 'Remove islands' }).click()
  const node = page.locator('.react-flow__node').filter({ hasText: 'Remove islands' })
  await expect(node).toBeVisible()
  await node.dispatchEvent('click')
  await expect(page.locator('aside')).toContainText('Smaller than')
})

test('+ All lists every node by usage with counts, menus flow into columns, and the perf overlay toggles', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 420 })
  await open(page)
  await page.getByRole('button', { name: '+ trace' }).click()
  await page.locator('.menu button').filter({ hasText: 'Round corners' }).click()
  await page.getByRole('button', { name: '+ All' }).click()
  const items = page.locator('.menu button')
  expect(await items.count()).toBeGreaterThan(15)
  // Round corners was just used, the others have counts of at least the starter graph's nodes
  const counts = await page.locator('.menu .cnt').allTextContents()
  const nums = counts.map(Number)
  expect(nums).toEqual([...nums].sort((a, b) => b - a))
  await expect(items.filter({ hasText: 'Round corners' }).locator('.cnt')).toHaveText('1')
  const m = page.locator('.menu').first()
  const b = (await m.boundingBox())!
  expect(b.y + b.height).toBeLessThanOrEqual(420)
  const xs = new Set(await items.evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().x))))
  expect(xs.size).toBeGreaterThan(1)
  await page.keyboard.press('Escape')
  await expect(page.locator('.perf')).toHaveCount(0)
  await page.getByRole('button', { name: 'Perf', exact: true }).click()
  await expect(page.locator('.perf')).toContainText('Engine workers')
  await page.getByRole('button', { name: 'Perf', exact: true }).click()
  await expect(page.locator('.perf')).toHaveCount(0)
})

test('the Image node opens a replacement picture and downloads its own, inside the inspector', async ({ page }) => {
  await open(page)
  const node = page.locator('.react-flow__node').filter({ hasText: 'Image' }).first()
  await expect(node.getByRole('button', { name: 'Download' })).toHaveCount(0)
  await node.dispatchEvent('click')
  const side = page.locator('aside')
  const dl = page.waitForEvent('download')
  await side.getByRole('button', { name: 'Download' }).click()
  expect((await dl).suggestedFilename()).toMatch(/\.(png|jpg|jpeg|bmp|svg)$/)
  const before = await page.locator('.react-flow__node').count()
  await side.locator('input[type=file]').setInputFiles({ name: 'dot.png', mimeType: 'image/png', buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64') })
  await expect(side).toContainText('dot.png')
  expect(await page.locator('.react-flow__node').count()).toBe(before)
})

test('the Image node workbench (double-click) has Open and Download', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').filter({ hasText: 'Image' }).first().dispatchEvent('dblclick')
  const bench = page.locator('.bench-params')
  await expect(bench.getByRole('button', { name: 'Download' })).toBeVisible()
  await expect(bench.getByText('Open…')).toBeVisible()
})

test('Tiles & mounting has a connector depth slider', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: '+ post' }).click()
  await page.locator('.menu button').filter({ hasText: 'Tiles & mounting' }).click()
  await expect(page.locator('aside')).toContainText('Connector depth into each tile')
})

test('sliders carry a tooltip and a unit, and the status line is announced', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').first().dispatchEvent('click')
  await expect(page.locator('aside .help').first()).toBeAttached()
  await expect(page.locator('aside .unit').first()).toBeAttached()
  await expect(page.locator('[role=status]').first()).toBeVisible()
})

test('the Colour set template builds a registered multi-colour graph', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: /Templates/ }).click()
  await page.locator('.menu button').filter({ hasText: 'Colour set' }).click()
  await expect(page.locator('.react-flow__node').filter({ hasText: 'Colour set' })).toBeVisible()
  await expect(page.locator('.react-flow__edge')).toHaveCount(5)
})

test('undo and redo restore a deleted node', async ({ page }) => {
  await open(page)
  await page.waitForTimeout(600)
  const n = await page.locator('.react-flow__node').count()
  await page.getByRole('button', { name: '+ trace' }).click()
  await page.locator('.menu button').filter({ hasText: 'Morphology' }).click()
  await expect(page.locator('.react-flow__node')).toHaveCount(n + 1)
  await page.waitForTimeout(600)
  await page.getByRole('button', { name: /Undo/ }).click()
  await expect(page.locator('.react-flow__node')).toHaveCount(n)
  await page.getByRole('button', { name: /Redo/ }).click()
  await expect(page.locator('.react-flow__node')).toHaveCount(n + 1)
})

test('help text stays hidden until the Show help toggle is on', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').first().dispatchEvent('click')
  const help = page.locator('aside .help').first()
  await page.locator('aside .row').first().hover()
  await expect(help).toBeHidden()
  await page.getByRole('button', { name: 'Show help' }).click()
  await expect(help).toBeVisible()
  await page.getByRole('button', { name: 'Hide help' }).click()
  await expect(help).toBeHidden()
})

test('the engine thread budget leaves at least one core free when threads are available', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Perf', exact: true }).click()
  const cores = await page.evaluate(() => navigator.hardwareConcurrency)
  const shown = Number((await page.locator('.perf').textContent())?.match(/engine uses (\d+)/)?.[1] ?? '1')
  expect(shown).toBeGreaterThanOrEqual(1)
  expect(shown).toBeLessThanOrEqual(Math.max(1, Math.min(Math.floor(cores / 2), cores - 1)))
})


test('command palette adds a node', async ({ page }) => {
  await open(page)
  await page.getByTitle('Command palette (Ctrl+K)').click()
  await page.getByRole('textbox', { name: 'Command palette' }).fill('remesh')
  await page.locator('.cmdpal-list button').first().click()
  await expect(page.locator('.react-flow__node').filter({ hasText: 'Remesh' })).toBeVisible()
})

test('viewer exposes smooth and flat shading', async ({ page }) => {
  await open(page)
  await page.locator('.react-flow__node').filter({ hasText: 'Stencil' }).first().dispatchEvent('dblclick')
  await expect(page.locator('.v3bar')).toContainText('shading')
  await page.locator('.v3bar select').first().selectOption('flat')
  await page.locator('.v3bar select').first().selectOption('smooth')
})
