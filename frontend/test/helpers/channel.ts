import { Socket, type Channel } from 'phoenix';
import { PHX_WS } from './server.ts';

export type PlayerPos = { x: number; y: number };
export type Snapshot = { players: Record<string, PlayerPos> };

export interface Session {
  socket: Socket;
  channel: Channel;
  username: string;
  /** Snapshots received since join, in arrival order. */
  snapshots: Snapshot[];
  /** Resolves with the first snapshot — already-received or future — matching `pred`. */
  waitFor(pred: (s: Snapshot) => boolean, timeoutMs?: number): Promise<Snapshot>;
  /** Resolves with the next snapshot to arrive after this call. Ignores already-buffered snapshots. */
  waitForNext(timeoutMs?: number): Promise<Snapshot>;
  disconnect(): Promise<void>;
}

export async function joinChunk(
  username: string,
  topic = 'chunk:0:0',
): Promise<Session> {
  const socket = new Socket(PHX_WS);
  socket.connect();
  const snapshotChannel = socket.channel(topic, { username });
  const initialChunk = parseChunkTopic(topic);
  const playerChannel = socket.channel(`player:${username}`, {
    username,
    initial_chunk: initialChunk,
  });

  const snapshots: Snapshot[] = [];
  const waiters: {
    pred: (s: Snapshot) => boolean;
    resolve: (s: Snapshot) => void;
  }[] = [];

  snapshotChannel.on('snapshot', (snap: Snapshot) => {
    snapshots.push(snap);
    for (let i = waiters.length - 1; i >= 0; i--) {
      if (waiters[i].pred(snap)) {
        waiters[i].resolve(snap);
        waiters.splice(i, 1);
      }
    }
  });

  await joinAndWait(playerChannel);
  await joinAndWait(snapshotChannel);

  function waitFor(
    pred: (s: Snapshot) => boolean,
    timeoutMs = 2000,
  ): Promise<Snapshot> {
    const already = snapshots.find(pred);
    if (already) return Promise.resolve(already);
    return new Promise<Snapshot>((resolve, reject) => {
      const entry = { pred, resolve };
      waiters.push(entry);
      setTimeout(() => {
        const idx = waiters.indexOf(entry);
        if (idx !== -1) {
          waiters.splice(idx, 1);
          reject(new Error(`timeout waiting for snapshot after ${timeoutMs}ms`));
        }
      }, timeoutMs);
    });
  }

  function waitForNext(timeoutMs = 2000): Promise<Snapshot> {
    const seen = snapshots.length;
    return new Promise<Snapshot>((resolve, reject) => {
      const pred = () => snapshots.length > seen;
      const entry = { pred, resolve };
      waiters.push(entry);
      setTimeout(() => {
        const idx = waiters.indexOf(entry);
        if (idx !== -1) {
          waiters.splice(idx, 1);
          reject(new Error(`timeout waiting for next snapshot after ${timeoutMs}ms`));
        }
      }, timeoutMs);
    });
  }

  async function disconnect(): Promise<void> {
    // Leaving the PlayerChannel terminates the Session, which removes the
    // Player's entity from whichever Chunk currently owns it.
    await leaveChannel(playerChannel);
    await leaveChannel(snapshotChannel);
    socket.disconnect();
  }

  return {
    socket,
    channel: playerChannel,
    username,
    snapshots,
    waitFor,
    waitForNext,
    disconnect,
  };
}

function parseChunkTopic(topic: string): [number, number] {
  const m = topic.match(/^chunk:(-?\d+):(-?\d+)$/);
  if (!m) return [0, 0];
  return [parseInt(m[1], 10), parseInt(m[2], 10)];
}

function joinAndWait(channel: Channel): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    channel
      .join()
      .receive('ok', () => resolve())
      .receive('error', (e: unknown) =>
        reject(new Error(`join failed: ${JSON.stringify(e)}`)),
      )
      .receive('timeout', () => reject(new Error('join timeout')));
  });
}

function leaveChannel(channel: Channel): Promise<void> {
  return new Promise<void>((resolve) => {
    channel
      .leave()
      .receive('ok', () => resolve())
      .receive('timeout', () => resolve());
  });
}
