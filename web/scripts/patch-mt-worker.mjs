import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

function walk(dir) {
  const out = []
  for (const n of readdirSync(dir)) {
    const p = join(dir, n)
    const st = statSync(p)
    if (st.isDirectory()) out.push(...walk(p))
    else out.push(p)
  }
  return out
}

const root = new URL('../public/pkg-mt', import.meta.url)
let n = 0
try {
  for (const f of walk(root.pathname)) {
    if (!f.endsWith('workerHelpers.js')) continue
    const src = readFileSync(f, 'utf8')
    const out = src.replace("import('../../..')", "import('../../../stencil_wasm.js')")
    if (out !== src) {
      writeFileSync(f, out)
      n++
    }
  }
} catch {
  // threaded package absent; keep single-thread fallback
}
console.log(`patched ${n} threaded worker helper file(s)`)
