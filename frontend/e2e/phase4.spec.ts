import { test, expect, type Page } from '@playwright/test';
import './types';
import type { Coord } from './types';

function uniq(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

async function openAs(page: Page, username: string, chunk?: Coord): Promise<void> {
  const chunkParam = chunk ? `&chunk=${chunk[0]}:${chunk[1]}` : '';
  await page.goto(`/?u=${encodeURIComponent(username)}${chunkParam}`);
  await page.waitForFunction(
    (name) => !!window.__game && name in window.__game.players(),
    username,
  );
}

test('phase 4: a Player sees another Player in a neighboring chunk', async ({ browser }) => {
  const alice = uniq('alice');
  const bob = uniq('bob');

  const ctxA = await browser.newContext();
  const ctxB = await browser.newContext();
  const pageA = await ctxA.newPage();
  const pageB = await ctxB.newPage();

  // Alice's home chunk is (0,0). Bob's home chunk is (1,0). Each is hosted
  // by a separate Chunk GenServer; alice's frontend subscribes to bob's
  // chunk as an observer (it's inside her 3×3 window).
  await openAs(pageA, alice, [0, 0]);
  await openAs(pageB, bob, [1, 0]);

  try {
    // Bob should appear in alice's render — via her observer subscription
    // to chunk:1:0, not because he's in alice's home chunk.
    await pageA.waitForFunction((b) => b in window.__game.players(), bob, {
      timeout: 8_000,
    });

    // Sanity: alice's own home is (0,0).
    const aliceHome = await pageA.evaluate(() => window.__game.homeChunk);
    expect(aliceHome).toEqual([0, 0]);

    // Sanity: bob's own home is (1,0).
    const bobHome = await pageB.evaluate(() => window.__game.homeChunk);
    expect(bobHome).toEqual([1, 0]);

    // Capture bob's current x as seen from alice's tab, then drive bob east.
    // Alice must see the position update through chunk:1:0's snapshots —
    // her own home is chunk:0:0 and she never joins chunk:1:0 directly.
    const bobX0InA = await pageA.evaluate(
      (b) => window.__game.players()[b]?.x ?? 0,
      bob,
    );
    await pageB.locator('canvas').focus();
    await pageB.keyboard.down('d');
    await pageA.waitForFunction(
      ({ b, x0 }) => (window.__game.players()[b]?.x ?? 0) > x0 + 1.0,
      { b: bob, x0: bobX0InA },
      { timeout: 8_000 },
    );
    await pageB.keyboard.up('d');
  } finally {
    await ctxA.close();
    await ctxB.close();
  }
});
