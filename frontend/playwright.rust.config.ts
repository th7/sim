import { defineConfig, devices } from '@playwright/test';

// E2e against the RUST backend: the existing specs, but pointed at the Vite dev
// server on :3000 (which serves the frontend and proxies /socket → the Rust
// server on :4000). No webServer here — the Rust server and Vite are expected to
// be already running (see sim/README.md). Use this config to prove the Rust
// server is a drop-in for the Elixir socket:
//   npx playwright test --config=playwright.rust.config.ts
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
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
