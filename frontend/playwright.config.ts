import { defineConfig, devices } from '@playwright/test';

// Default to talking to Vite (which proxies /socket to Phoenix on :4000),
// but allow E2E_BASE_URL=http://localhost:4000 to skip Vite — useful when
// debugging WebSocket flakes that originate in the proxy.
const BASE_URL = process.env.E2E_BASE_URL ?? 'http://localhost:3000';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list']],
  timeout: 60_000,
  expect: { timeout: 5_000 },
  use: {
    baseURL: BASE_URL,
    headless: true,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
});
