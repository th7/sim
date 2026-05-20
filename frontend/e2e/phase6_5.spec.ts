import { test, expect, type Page } from '@playwright/test';
import './types';

function uniq(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`;
}

async function openAs(page: Page, username: string, params = ''): Promise<void> {
  const query = `u=${encodeURIComponent(username)}${params ? '&' + params : ''}`;
  await page.goto(`/?${query}`);
  await page.waitForFunction(
    (name) => !!window.__game && name in window.__game.players(),
    username,
  );
}

test('phase 6.5: ?dev=1 shows the HUD and the view count matches the rendered set', async ({
  browser,
}) => {
  const alice = uniq('alice');
  const ctx = await browser.newContext();
  const page = await ctx.newPage();

  await openAs(page, alice, 'dev=1');

  const hud = page.locator('#dev-hud');
  await expect(hud).toBeVisible();

  // The HUD's view count should equal the size of the rendered player set
  // once a stats push has populated the HUD.
  await page.waitForFunction(() => {
    const el = document.getElementById('dev-hud');
    return !!el && /view:\s*\d+/.test(el.textContent ?? '');
  });

  const rendered = await page.evaluate(
    () => Object.keys(window.__game.players()).length,
  );
  const hudText = (await hud.textContent()) ?? '';
  const match = hudText.match(/view:\s*(\d+)/);
  expect(match).not.toBeNull();
  expect(parseInt(match![1], 10)).toBe(rendered);

  await ctx.close();
});
