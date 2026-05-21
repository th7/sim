import { test, expect, type Page } from '@playwright/test';
import './types';
import type { Coord } from './types';

// Regression for the "click any tree after a chunk boundary crossing
// resets the game" bug:
//   - the home chunk crashed with a WithClauseError because the harvest
//     handler did not tolerate a player whose Position had migrated out
//   - harvest/build/damage were routed to the *home* chunk by the owner
//     channel, but the player's entity had migrated to a neighbour
// Symptoms the user saw, all of which we assert against here:
//   1) clicking a tree after migration succeeds (inventory gains wood)
//   2) the clicker's position is not reset back to the home chunk
//   3) a second player in the destination chunk stays connected & visible

const CHUNK_SUB = 16_000;
const CHUNK_WU = 16;

function uniq(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

async function openAs(page: Page, username: string, chunk: Coord): Promise<void> {
  await page.goto(`/?u=${encodeURIComponent(username)}&chunk=${chunk[0]}:${chunk[1]}`);
  await page.waitForFunction(
    (name) => !!window.__game && name in window.__game.players(),
    username,
  );
}

// Each run picks a fresh, far-away chunk so prior tests can't leave trees
// in a depleted state in our path. Same strategy as phase8.spec.ts.
function pickChunk(): Coord {
  const c = 8_000 + Math.floor(Math.random() * 1_000);
  return [c, c];
}

test('post-migration harvest: clicker keeps position, bystander stays connected', async ({
  browser,
}) => {
  test.setTimeout(60_000);

  const alice = uniq('alice');
  const bob = uniq('bob');
  const [cx, cy] = pickChunk();
  const eastCx = cx + 1;

  // Tree at the centre of the destination chunk — alice walks straight
  // through this point so she's guaranteed to be within interact range
  // by the time we stop her.
  const treeSubX = eastCx * CHUNK_SUB + 8_000;
  const treeSubY = cy * CHUNK_SUB + 8_000;

  // World-coord landmarks in Three.js units (= sub-units / 1000).
  const eastBoundaryX = eastCx * CHUNK_WU; // x where alice migrates in
  const treeWorldX = treeSubX / 1000;
  const treeWorldY = treeSubY / 1000;
  const homeCentreX = cx * CHUNK_WU + CHUNK_WU / 2; // where she spawned

  const ctxA = await browser.newContext();
  const ctxB = await browser.newContext();
  const pageA = await ctxA.newPage();
  const pageB = await ctxB.newPage();

  try {
    // Bob lives in the chunk alice will migrate INTO. He must remain
    // visible to her, and present in his own view, throughout.
    await openAs(pageB, bob, [eastCx, cy]);
    await openAs(pageA, alice, [cx, cy]);

    // Alice's 3×3 view window includes bob's chunk — she should see him.
    await pageA.waitForFunction((b) => b in window.__game.players(), bob, {
      timeout: 10_000,
    });

    // Walk east until alice is within 0.5u of the tree at the destination
    // chunk's centre. That guarantees she's already migrated AND is in
    // interact range. (Server-side range check is 1.0u.)
    await pageA.locator('canvas').focus();
    await pageA.keyboard.down('d');
    await pageA.waitForFunction(
      ({ name, tx, ty }) => {
        const p = window.__game.players()[name];
        if (!p) return false;
        const dx = p.x - tx;
        const dy = p.y - ty;
        return dx * dx + dy * dy < 0.5 * 0.5;
      },
      { name: alice, tx: treeWorldX, ty: treeWorldY },
      { timeout: 20_000 },
    );
    await pageA.keyboard.up('d');

    // Sanity: she crossed the boundary.
    const xAfterWalk = (await pageA.evaluate(
      (n) => window.__game.players()[n]?.x ?? 0,
      alice,
    )) as number;
    expect(xAfterWalk).toBeGreaterThan(eastBoundaryX);

    // Click the tree. Pre-fix: the home chunk's harvest handler crashed
    // here, the channel exited, alice rejoined and rehydrated at
    // homeCentre with empty inventory.
    await pageA.evaluate(
      ([x, y]) => window.__game.harvest(x, y),
      [treeSubX, treeSubY],
    );

    // (1) Inventory gained wood.
    await pageA.waitForFunction(
      () => (window.__game.inventory().wood ?? 0) >= 1,
      undefined,
      { timeout: 10_000 },
    );
    expect(await pageA.evaluate(() => window.__game.inventory().wood)).toBe(1);

    // (2) Alice has NOT been rehydrated back to her home chunk centre.
    // A reset would put her at (homeCentreX, ...). We assert she's still
    // well east of the home chunk.
    const aliceAfter = (await pageA.evaluate(
      (n) => window.__game.players()[n],
      alice,
    )) as { x: number; y: number };
    expect(aliceAfter.x).toBeGreaterThan(eastBoundaryX);
    // Defensive: the failure mode literally drops her at homeCentreX.
    expect(Math.abs(aliceAfter.x - homeCentreX)).toBeGreaterThan(1);

    // (3) Bob is still visible from alice's tab AND from his own tab
    // (cascading supervisor restart would have wiped him out of his
    // own chunk's snapshot too).
    expect(
      await pageA.evaluate((b) => b in window.__game.players(), bob),
    ).toBe(true);
    expect(
      await pageB.evaluate((b) => b in window.__game.players(), bob),
    ).toBe(true);
  } finally {
    await ctxA.close();
    await ctxB.close();
  }
});
