import { test, expect, type Page } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import './types';

const exec = promisify(execFile);
const __dirname = dirname(fileURLToPath(import.meta.url));

function uniq(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

async function openAs(page: Page, username: string): Promise<void> {
  await page.goto(`/?u=${encodeURIComponent(username)}`);
  await page.waitForFunction(
    (name) => !!window.__game && name in window.__game.players(),
    username,
  );
}

async function readX(page: Page, username: string): Promise<number> {
  return page.evaluate((name) => window.__game.players()[name]?.x ?? 0, username);
}

test('phase 3: a Player\'s position survives a BEAM restart', async ({ browser }) => {
  test.setTimeout(180_000);
  const username = uniq('persistent');

  // 1) Connect, drive east until x clears 1.0, then disconnect (channel
  //    terminate flushes the saved position to Postgres).
  const ctx1 = await browser.newContext();
  const page1 = await ctx1.newPage();
  await openAs(page1, username);
  await page1.locator('canvas').focus();
  await page1.keyboard.down('d');
  await page1.waitForFunction(
    (name) => (window.__game.players()[name]?.x ?? 0) > 1.0,
    username,
    { timeout: 5_000 },
  );
  await page1.keyboard.up('d');
  const savedX = await readX(page1, username);
  await ctx1.close();

  // Belt-and-suspenders: the chunk also flushes on its periodic timer and
  // on terminate; give the channel.terminate a moment to land in Postgres.
  await new Promise((r) => setTimeout(r, 500));

  // 2) Restart phx.server.
  await exec(resolve(__dirname, '../../bin/restart-e2e.sh'), [], { timeout: 90_000 });

  // 3) Reconnect; the new chunk hydrates from Postgres and our cube should
  //    appear at savedX, not at the origin.
  const ctx2 = await browser.newContext();
  const page2 = await ctx2.newPage();
  await openAs(page2, username);
  const restoredX = await readX(page2, username);
  await ctx2.close();

  expect(restoredX).toBeGreaterThan(0.5);
  expect(restoredX).toBeCloseTo(savedX, 0);
});
