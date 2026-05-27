import { defineConfig, devices } from '@playwright/test';

// E2e runs against the Rust sim server, which serves both the built frontend
// bundle and the Phoenix-Channels socket on one port (default :4001). The
// server is started by bin/e2e (`npm run test:e2e`), which also exports the
// env phase3/phase8 need to restart it via bin/restart-e2e. Override the
// target with E2E_BASE_URL.
const BASE_URL = process.env.E2E_BASE_URL ?? 'http://localhost:4001';

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
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
