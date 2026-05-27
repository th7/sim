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

  // Chunk-centre and walk target. Post-collision the wall's AABB cannot
  // overlap any tree (live or depleted) or the placing player's body, so
  // alice walks east of the harvested cluster before placing. The exact
  // wall position is derived from alice's actual stopping spot (computed
  // after she settles), since the margin between "no body overlap" and
  // "within 1u damage range" is only 200 sub-units.
  const centreSubX = cx * CHUNK_SUB + 8_000;
  const centreSubY = cy * CHUNK_SUB + 8_000;
  const aliceTargetSubX = centreSubX + 2_500; // 2.5u east — well outside cluster

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
  // Alice is grandfathered through the cluster on spawn (her body overlaps
  // the centre tree); harvest just needs interact range, not navigation.
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

  // Walk east out of the (depleted-but-still-collidable) tree cluster.
  await page1.locator('canvas').focus();
  await page1.keyboard.down('d');
  await page1.waitForFunction(
    (target) => {
      const me = window.__game.players()[window.__game.username];
      return !!me && me.x * 1000 >= target;
    },
    aliceTargetSubX,
  );
  await page1.keyboard.up('d');

  // Wait for alice to actually stop — poll until position is stable across
  // a few snapshot intervals (server is 10Hz, mesh interpolates between).
  await page1.waitForFunction(
    () => {
      const me = window.__game.players()[window.__game.username];
      if (!me) return false;
      const k = `__phase8_stop_${me.x.toFixed(3)}`;
      const w = window as unknown as Record<string, number>;
      w[k] = (w[k] ?? 0) + 1;
      return w[k] >= 5;
    },
    null,
    { polling: 100, timeout: 5_000 },
  );

  const aliceStop = await page1.evaluate(() => {
    const me = window.__game.players()[window.__game.username];
    return { x: Math.round(me.x * 1000), y: Math.round(me.y * 1000) };
  });
  // Place wall exactly 1u east of alice's centre — wall AABB west edge sits
  // 200 sub-units past alice's body (no overlap), distance from alice to
  // wall centre is 1000 sub-units (boundary of damage interact_range_sq).
  const wallSubX = aliceStop.x + 1_000;
  const wallSubY = aliceStop.y;

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
  // Datastore default flush interval is 1s; give it a clear window so the
  // wall + alice's walked position are flushed before SIGTERM.
  await new Promise((r) => setTimeout(r, 1_500));

  // Restart Phoenix; the wall row is in Postgres and inventory has flushed.
  await exec(resolve(__dirname, '../../bin/restart-e2e'), [], { timeout: 90_000 });

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
