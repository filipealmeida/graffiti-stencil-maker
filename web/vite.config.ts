import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { execSync } from 'node:child_process'
import { readFileSync } from 'node:fs'

const git = (args: string, fallback: string) => {
  try { return execSync(`git ${args}`, { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim() || fallback } catch { return fallback }
}
const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'))
// the dirty flag marks a build made from uncommitted changes
const dirty = git('status --porcelain --untracked-files=no', '') ? '+dirty' : ''
const build = {
  version: pkg.version,
  build: Number(git('rev-list --count HEAD', '0')),
  commit: git('rev-parse --short HEAD', 'unknown') + dirty,
  time: new Date().toISOString(),
}

const isolation = { 'Cross-Origin-Opener-Policy': 'same-origin', 'Cross-Origin-Embedder-Policy': 'require-corp' }

// Relative base so the build works from any static host or sub-path.
export default defineConfig({
  base: './',
  plugins: [react()],
  define: { __BUILD__: JSON.stringify(build) },
  worker: { format: 'es' },
  // three.js is split into its own lazily loaded chunk
  build: { chunkSizeWarningLimit: 600 },
  // Shared memory (the multi-threaded engine) needs a cross-origin isolated page.
  server: { port: 5174, headers: isolation },
  preview: { headers: isolation },
})
