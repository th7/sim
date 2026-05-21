import { test, expect, type Page } from '@playwright/test';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import './types';

const exec = promisify(execFile);
const __dirname = dirname(fileURLToPath(import.meta.url));

const CHUNK_SUB = 16_000;

function uniq(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

async function openAs(page: Page, username: string, chunk: [number, number]): Promise<void> {
  const q = `u=${encodeURIComponent(username)}&chunk=${chunk[0]}:${chunk[1]}`;
  await page.goto(`/?${q}`);
  await page.waitForFunction(
    (name) => !!window.__game && name in window.__game.players(),
    username,
  );
}

// Each Phase 8 test run uses a fresh, far-away chunk so other clients in
// the dev server can't leave Resource nodes or Structures in a stale
// state. Choose a random coord well outside any other test's neighbourhood.
function pickChunk(): [number, number] {
  const c = 5_000 + Math.floor(Math.random() * 1_000);
  return [c, c];
}

test('phase 8: gather → build → persistence round-trip', async ({ browser }) => {
  test.setTimeout(180_000);
  const alice = uniq('alice');
  const [cx, cy] = pickChunk();

  // Chunk-centre and an adjacent cell anchor for the wall.
  const centreSubX = cx * CHUNK_SUB + 8_000;
  const centreSubY = cy * CHUNK_SUB + 8_000;
  const wallSubX = centreSubX - 500; // 0.5u west of centre
  const wallSubY = centreSubY - 500;

  // Worldgen tree positions in this chunk (sub-units):
  const treeOffsets = [
    [500, 500],
    [500, -500],
    [-500, 500],
    [-500, -500],
    [0, 0],
  ];

  const ctx1 = await browser.newContext();
  const page1 = await ctx1.newPage();
  await openAs(page1, alice, [cx, cy]);

  // HUD mounted, inventory starts empty.
  await expect(page1.locator('#inv-hud')).toBeVisible();

  // Chop every Worldgen tree in the chunk (5 total → 5 wood for one wall).
  // Trees and walls can share a cell at the persistence layer; cell-occupied
  // only rejects when a *Structure* already sits there.
  for (let i = 0; i < 5; i++) {
    const [dx, dy] = treeOffsets[i];
    await page1.evaluate(
      ([x, y]) => window.__game.harvest(x, y),
      [centreSubX + dx, centreSubY + dy],
    );
    await page1.waitForFunction(
      (want) => (window.__game.inventory().wood ?? 0) >= want,
      i + 1,
    );
  }

  expect(await page1.evaluate(() => window.__game.inventory().wood)).toBe(5);

  // Build a wall on the wall's cell anchor.
  await page1.evaluate(
    ([x, y]) => window.__game.build('wall', x, y),
    [wallSubX, wallSubY],
  );
  await page1.waitForFunction(() => Object.keys(window.__game.structures()).length === 1);
  expect(await page1.evaluate(() => window.__game.inventory().wood ?? 0)).toBe(0);

  const built = await page1.evaluate(() => Object.values(window.__game.structures())[0]);
  expect(built.hp).toBe(100);
  expect(built.owner).toBe(alice);

  await ctx1.close();
  await new Promise((r) => setTimeout(r, 500));

  // Restart Phoenix; the wall row is in Postgres and inventory has flushed.
  await exec(resolve(__dirname, '../../bin/restart-e2e.sh'), [], { timeout: 90_000 });

  // Reconnect as the same user and verify the wall + inventory hydrate.
  const ctx2 = await browser.newContext();
  const page2 = await ctx2.newPage();
  await openAs(page2, alice, [cx, cy]);

  await page2.waitForFunction(() => Object.keys(window.__game.structures()).length === 1);
  const restored = await page2.evaluate(() => Object.values(window.__game.structures())[0]);
  expect(restored.x).toBe(built.x);
  expect(restored.y).toBe(built.y);
  expect(restored.hp).toBe(built.hp);
  expect(restored.owner).toBe(alice);
  expect(await page2.evaluate(() => window.__game.inventory().wood ?? 0)).toBe(0);

  // Now damage it to destruction: 4 clicks × 25 HP = 100.
  for (let i = 0; i < 4; i++) {
    await page2.evaluate(
      ([x, y]) => window.__game.damage(x, y),
      [wallSubX, wallSubY],
    );
  }
  await page2.waitForFunction(() => Object.keys(window.__game.structures()).length === 0);

  await ctx2.close();
});
