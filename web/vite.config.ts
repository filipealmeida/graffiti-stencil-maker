import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Relative base so the build works from any static host or sub-path.
export default defineConfig({
  base: './',
  plugins: [react()],
  worker: { format: 'es' },
  // three.js is split into its own lazily loaded chunk
  build: { chunkSizeWarningLimit: 600 },
  server: { port: 5174 },
})
