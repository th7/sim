import { beforeAll, afterEach, describe, expect, it } from 'vitest';
import { assertServerUp, uniqUsername } from './helpers/server.ts';
import { joinChunk, type Session } from './helpers/channel.ts';

beforeAll(assertServerUp);

// Live end-to-end verification of the collision feature. Drives intent via
// the Phoenix channel and asserts the server snapshots show the position
// clamped at the expected Footprint edge.
describe('collision (live)', () => {
  let sessions: Session[] = [];

  afterEach(async () => {
    await Promise.all(sessions.map((s) => s.disconnect()));
    sessions = [];
  });

  async function join(prefix: string): Promise<Session> {
    const s = await joinChunk(uniqUsername(prefix));
    sessions.push(s);
    return s;
  }

  it('a tree blocks westward re-entry — player stops flush against the tree Footprint', async () => {
    const me = await join('coll');

    // Spawn at chunk-(0,0) centre (8_000, 8_000), on the central tree.
    // Grandfather rule lets a spawn-overlapping player escape; walk west out
    // of the cluster.
    me.channel.push('move', { dx: -1, dy: 0 });
    await me.waitFor((s) => (s.players[me.username]?.x ?? 9999) < 4_000, 5_000);

    // Now turn around: walking east, the cluster blocks. The western trees
    // at (7_500, 7_500) and (7_500, 8_500) both constrain a player at y=8_000
    // with body r=300; first-contact x = 7_500 − ⌈√110_000⌉ = 7_168.
    me.channel.push('move', { dx: 1, dy: 0 });

    let maxX = -Infinity;
    let lastSnap = await me.waitForNext(2_000);
    const deadline = Date.now() + 3_000;
    while (Date.now() < deadline) {
      lastSnap = await me.waitForNext(1_000);
      const x = lastSnap.players[me.username]?.x ?? -Infinity;
      if (x > maxX) maxX = x;
    }
    expect(maxX).toBe(7_168);
    expect(lastSnap.players[me.username].y).toBe(8_000);
  });
});
