import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const isolation = { 'Cross-Origin-Opener-Policy': 'same-origin', 'Cross-Origin-Embedder-Policy': 'require-corp' }

// Relative base so the build works from any static host or sub-path.
export default defineConfig({
  base: './',
  plugins: [react()],
  worker: { format: 'es' },
  // three.js is split into its own lazily loaded chunk
  build: { chunkSizeWarningLimit: 600 },
  // Shared memory (the multi-threaded engine) needs a cross-origin isolated page.
  server: { port: 5174, headers: isolation },
  preview: { headers: isolation },
})
