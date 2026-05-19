import { beforeAll, afterEach, describe, expect, it } from 'vitest';
import { assertServerUp, uniqUsername } from './helpers/server.ts';
import { joinChunk, type Session } from './helpers/channel.ts';

beforeAll(assertServerUp);

describe('chunk:0:0 channel', () => {
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

  it('a joined player appears in a snapshot at the origin', async () => {
    const me = await join('origin');
    const snap = await me.waitFor((s) => me.username in s.players);
    expect(snap.players[me.username]).toEqual({ x: 0, y: 0 });
  });

  it('intent moves the player; zero intent halts it', async () => {
    const me = await join('mover');
    await me.waitFor((s) => me.username in s.players);

    me.channel.push('move', { dx: 1, dy: 0 });
    const moved = await me.waitFor((s) => (s.players[me.username]?.x ?? 0) > 0.1);
    expect(moved.players[me.username].x).toBeGreaterThan(0.1);
    expect(moved.players[me.username].y).toBeCloseTo(0, 5);

    me.channel.push('move', { dx: 0, dy: 0 });
    // Let the halt take effect (the snapshot still in flight may reflect motion).
    await me.waitForNext();
    await me.waitForNext();
    const baseline = (await me.waitForNext()).players[me.username].x;
    const later = (await me.waitForNext()).players[me.username].x;
    expect(later).toBeCloseTo(baseline, 5);
  });

  it('two players each see the other in the same snapshots', async () => {
    const a = await join('alice');
    const b = await join('bob');

    const snap = await a.waitFor(
      (s) => a.username in s.players && b.username in s.players,
    );
    expect(snap.players[a.username]).toBeDefined();
    expect(snap.players[b.username]).toBeDefined();
  });

  it('leaving the channel removes the player from subsequent snapshots', async () => {
    const watcher = await join('watcher');
    const transient = await joinChunk(uniqUsername('transient'));

    await watcher.waitFor((s) => transient.username in s.players);
    await transient.disconnect();

    await watcher.waitFor((s) => !(transient.username in s.players));
  });
});
