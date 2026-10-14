import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: 'e2e',
  timeout: 90_000,
  workers: 1,
  use: { baseURL: 'http://localhost:5199' },
  webServer: { command: 'npx vite preview --port 5199 --strictPort', url: 'http://localhost:5199', reuseExistingServer: true },
})
