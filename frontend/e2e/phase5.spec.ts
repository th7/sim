import { test, expect, type Page } from '@playwright/test';
import './types';

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

test("phase 5: a Player walks across multiple chunk boundaries without glitches", async ({
  browser,
}) => {
  const alice = uniq('alice');
  const ctx = await browser.newContext();
  const page = await ctx.newPage();

  await openAs(page, alice);

  // Install a tap that records every x ever seen for THIS alice. Used
  // after the walk to verify monotonic progression with no rewinds and
  // no holes (a "hiccup" would be alice disappearing from her own view
  // mid-migration).
  await page.evaluate((name) => {
    const samples: Array<{ t: number; x: number; present: boolean }> = [];
    (window as unknown as { __samples: typeof samples }).__samples = samples;
    const start = performance.now();
    setInterval(() => {
      const me = window.__game.players()[name];
      samples.push({
        t: performance.now() - start,
        x: me?.x ?? -1,
        present: !!me,
      });
    }, 50);
  }, alice);

  await page.locator('canvas').focus();
  await page.keyboard.down('d');

  // Alice spawns at chunk (0,0) center, world (8, 8). Crosses into (1,0)
  // at x=16, into (2,0) at x=32. Wait until she's safely inside (2,0).
  await page.waitForFunction(
    (name) => (window.__game.players()[name]?.x ?? 0) > 33,
    alice,
    { timeout: 20_000 },
  );
  await page.keyboard.up('d');

  // Briefly let snapshots settle.
  await page.waitForTimeout(300);

  const samples = await page.evaluate(
    () =>
      (window as unknown as {
        __samples: Array<{ t: number; x: number; present: boolean }>;
      }).__samples,
  );

  // Skip warm-up samples taken before alice's join landed.
  const firstSeen = samples.findIndex((s) => s.present);
  expect(firstSeen).toBeGreaterThanOrEqual(0);
  const motion = samples.slice(firstSeen);

  // No hiccup: once alice is visible to her own tab, she stays visible —
  // a botched migration would show as a `present: false` gap between
  // chunks while source has already removed her and dest hasn't broadcast.
  for (const s of motion) {
    expect(s.present).toBe(true);
  }

  // No rewind: alice's x is non-decreasing within tiny FP slack. A
  // migration that resets her position would show as a sharp drop.
  let prev = motion[0].x;
  for (const s of motion) {
    expect(s.x).toBeGreaterThanOrEqual(prev - 0.01);
    prev = s.x;
  }
  expect(motion[motion.length - 1].x).toBeGreaterThan(33);

  await ctx.close();
});
