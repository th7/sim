import { test, expect, type Page } from '@playwright/test';
import './types';

function uniq(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

async function openAtHome(page: Page, username: string): Promise<void> {
  // Phase 9 specifically wants the Player at chunk {0,0}, where the only
  // Worldgen-placed Portal lives (at sub-units (4000, 4000) — quarter offset
  // from the spawn at chunk-center 8000, 8000).
  await page.goto(`/?u=${encodeURIComponent(username)}&chunk=0:0`);
  await page.waitForFunction(
    (name) => !!window.__game && name in window.__game.players(),
    username,
  );
}

// Golden path: the full Instance round-trip through a real browser — realm
// transition, camera follow, and return. The server-side semantics (entry/exit
// migration, inventory survival, instance isolation, no Resource nodes,
// disconnect teardown) are pinned in the Rust backend (`sim/tests/verbs.rs`,
// `sim/tests/persistence.rs`); this spec is the one browser-observable path the
// backend tests can't reach (rendered realm switch + camera).
test('phase 9: walk into the Portal, around the Instance, and back out — camera follows', async ({
  browser,
}) => {
  test.setTimeout(60_000);
  const alice = uniq('alice');
  const ctx = await browser.newContext();
  const page = await ctx.newPage();

  await openAtHome(page, alice);

  // Sanity: the Overworld Portal is visible in the snapshot at world (4, 4).
  const overworldPortal = await page.evaluate(() => {
    const ps = window.__game.portals();
    return Object.values(ps).find((p) => p.direction === 'into_instance') ?? null;
  });
  expect(overworldPortal).toMatchObject({
    type: 'dungeon',
    direction: 'into_instance',
    x: 4,
    y: 4,
  });

  // Start in the Overworld realm.
  expect(await page.evaluate(() => window.__game.realm().kind)).toBe('overworld');

  // Walk northwest toward the Portal. Spawn is at (8, 8); Portal at (4, 4).
  await page.locator('canvas').focus();
  await page.keyboard.down('a'); // west
  await page.keyboard.down('w'); // north

  // The realm flips to "instance" once the Player's position overlaps the
  // Portal (within 0.5 world units). We poll for the realm change.
  await page.waitForFunction(
    () => window.__game.realm().kind === 'instance',
    null,
    { timeout: 15_000 },
  );
  await page.keyboard.up('a');
  await page.keyboard.up('w');

  // Inside the Instance: the visible Portal is the return-Portal (out_of_instance).
  await page.waitForFunction(
    (name) => {
      const portals = Object.values(window.__game.portals());
      const me = window.__game.players()[name];
      return (
        !!me &&
        portals.some((p) => p.direction === 'out_of_instance') &&
        // Player's Instance-local position is roughly the spawn offset
        // (one world unit west of the return-Portal at (24, 24)).
        Math.abs(me.x - 23) < 1 &&
        Math.abs(me.y - 24) < 1
      );
    },
    alice,
    { timeout: 5_000 },
  );

  // The camera follows the Player into the Instance — it must frame the
  // Instance-local position, not stay stuck on the Overworld home chunk.
  await page.waitForFunction(
    (name) => {
      const me = window.__game.players()[name];
      const cam = window.__game.cameraPos();
      if (!me) return false;
      const dx = cam.x - me.x;
      const dz = cam.z - me.y;
      return Math.sqrt(dx * dx + dz * dz) < 20;
    },
    alice,
    { timeout: 5_000 },
  );

  // Walk east into the return-Portal at (24, 24).
  await page.keyboard.down('d');
  await page.waitForFunction(
    () => window.__game.realm().kind === 'overworld',
    null,
    { timeout: 15_000 },
  );
  await page.keyboard.up('d');

  // Re-emerged in the Overworld near the entry Portal (one world unit west
  // of (4, 4) — so around (3, 4)).
  await page.waitForFunction(
    (name) => {
      const me = window.__game.players()[name];
      return !!me && Math.abs(me.x - 3) < 1.5 && Math.abs(me.y - 4) < 1.5;
    },
    alice,
    { timeout: 5_000 },
  );

  // The into_instance Portal is back in view (we're back in the Overworld).
  const overworldPortalAgain = await page.evaluate(() => {
    const ps = window.__game.portals();
    return Object.values(ps).find((p) => p.direction === 'into_instance') ?? null;
  });
  expect(overworldPortalAgain).toMatchObject({ direction: 'into_instance' });

  await ctx.close();
});
