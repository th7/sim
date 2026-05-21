import { defineConfig, devices } from '@playwright/test';

// E2e talks to its own isolated Phoenix on :4001 (MIX_ENV=e2e, sim_e2e DB),
// independent from dev's :4000/:3000. Override with E2E_BASE_URL if pointing
// at an already-running BEAM.
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
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
  webServer: {
    command:
      'cd .. && MIX_ENV=e2e PORT=4001 mix do assets.deploy + ecto.drop --quiet + ecto.create --quiet + ecto.migrate --quiet + phx.server',
    url: 'http://localhost:4001',
    reuseExistingServer: false,
    timeout: 120_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
