import { test, expect, type Page } from '@playwright/test';

type PlayerPos = { x: number; y: number };

declare global {
  interface Window {
    __game: { username: string; players(): Record<string, PlayerPos> };
  }
}

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

async function waitForPlayerX(
  page: Page,
  username: string,
  predicate: (x: number) => boolean,
): Promise<number> {
  return page.waitForFunction(
    ({ name, predSource }) => {
      const pos = window.__game.players()[name];
      if (!pos) return null;
      // eslint-disable-next-line no-new-func
      const pred = new Function('x', `return (${predSource})(x)`) as (x: number) => boolean;
      return pred(pos.x) ? pos.x : null;
    },
    { name: username, predSource: predicate.toString() },
  ).then((handle) => handle.jsonValue() as Promise<number>);
}

test('two browser tabs each see the other player\'s cube move', async ({ browser }) => {
  const alice = uniq('alice');
  const bob = uniq('bob');

  const ctxA = await browser.newContext();
  const ctxB = await browser.newContext();
  const pageA = await ctxA.newPage();
  const pageB = await ctxB.newPage();

  await openAs(pageA, alice);
  await openAs(pageB, bob);

  // Both tabs must see both players before motion begins, otherwise the
  // "moved" check is racing the initial join.
  await pageA.waitForFunction((b) => b in window.__game.players(), bob);
  await pageB.waitForFunction((a) => a in window.__game.players(), alice);

  await pageA.locator('canvas').focus();
  await pageA.keyboard.down('d');

  try {
    const aliceX_inB = await waitForPlayerX(pageB, alice, (x) => x > 0.5);
    expect(aliceX_inB).toBeGreaterThan(0.5);

    // Alice's own tab also sees her own cube move (authoritative snapshot).
    const aliceX_inA = await waitForPlayerX(pageA, alice, (x) => x > 0.5);
    expect(aliceX_inA).toBeGreaterThan(0.5);
  } finally {
    await pageA.keyboard.up('d');
    await ctxA.close();
    await ctxB.close();
  }
});
