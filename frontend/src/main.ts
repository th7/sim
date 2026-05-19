import * as THREE from 'three';
import { Socket, type Channel } from 'phoenix';

type PlayerPos = { x: number; y: number };
type Snapshot = { players: Record<string, PlayerPos> };
type Coord = readonly [number, number];

const CHUNK_SIZE = 16;

const app = document.querySelector<HTMLDivElement>('#app')!;
const urlParams = new URLSearchParams(window.location.search);

const username = urlParams.get('u') ?? `player-${Math.floor(Math.random() * 10000)}`;
const homeChunk = parseChunkParam(urlParams.get('chunk')) ?? ([0, 0] as const);

function parseChunkParam(raw: string | null): Coord | null {
  if (!raw) return null;
  const m = raw.match(/^(-?\d+):(-?\d+)$/);
  if (!m) return null;
  return [parseInt(m[1], 10), parseInt(m[2], 10)] as const;
}

function chunkKey([cx, cy]: Coord): string {
  return `${cx}:${cy}`;
}

function windowCoords([cx, cy]: Coord): Coord[] {
  const out: Coord[] = [];
  for (let dx = -1; dx <= 1; dx++) {
    for (let dy = -1; dy <= 1; dy++) {
      out.push([cx + dx, cy + dy] as const);
    }
  }
  return out;
}

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x101010);

const camera = new THREE.PerspectiveCamera(
  50,
  window.innerWidth / window.innerHeight,
  0.1,
  500,
);
const camLookAt = new THREE.Vector3(homeChunk[0] * CHUNK_SIZE, 0, homeChunk[1] * CHUNK_SIZE);
camera.position.set(camLookAt.x + 12, 12, camLookAt.z + 12);
camera.lookAt(camLookAt);

const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setPixelRatio(window.devicePixelRatio);
renderer.setSize(window.innerWidth, window.innerHeight);
app.appendChild(renderer.domElement);

scene.add(new THREE.GridHelper(CHUNK_SIZE * 5, CHUNK_SIZE * 5, 0x404040, 0x202020));

const playerMeshes = new Map<string, THREE.Mesh>();
const palette = [0x4caf50, 0x2196f3, 0xff9800, 0xe91e63, 0x9c27b0, 0xffeb3b];

function colorFor(name: string): number {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  return palette[Math.abs(h) % palette.length];
}

// One snapshot map per subscribed chunk. The rendered set is the union.
const channelSnapshots = new Map<string, Map<string, PlayerPos>>();

function updateRenderedFromMerge(): void {
  const union = new Map<string, PlayerPos>();
  for (const m of channelSnapshots.values()) {
    for (const [name, pos] of m) union.set(name, pos);
  }
  for (const [name, pos] of union) {
    let mesh = playerMeshes.get(name);
    if (!mesh) {
      mesh = new THREE.Mesh(
        new THREE.BoxGeometry(1, 1, 1),
        new THREE.MeshBasicMaterial({ color: colorFor(name) }),
      );
      scene.add(mesh);
      playerMeshes.set(name, mesh);
    }
    mesh.position.set(pos.x, 0.5, pos.y);
  }
  for (const [name, mesh] of playerMeshes) {
    if (!union.has(name)) {
      scene.remove(mesh);
      playerMeshes.delete(name);
    }
  }
}

function ingestChunkSnapshot(key: string, snap: Snapshot): void {
  channelSnapshots.set(key, new Map(Object.entries(snap.players)));

  if (key === chunkKey(homeChunk)) {
    const mine = snap.players[username];
    if (mine) maybeShiftWindow(mine);
  }

  updateRenderedFromMerge();
}

let windowCenter: Coord = homeChunk;
const homeKey0 = chunkKey(homeChunk);

function maybeShiftWindow({ x, y }: PlayerPos): void {
  const [cx, cy] = [Math.floor(x / CHUNK_SIZE), Math.floor(y / CHUNK_SIZE)];
  if (cx === windowCenter[0] && cy === windowCenter[1]) return;

  const newCenter: Coord = [cx, cy];
  const oldKeys = new Set(windowCoords(windowCenter).map(chunkKey));
  const newKeys = new Set(windowCoords(newCenter).map(chunkKey));

  // Drop stale observer subscriptions, but never leave the home chunk —
  // it's the channel that owns the local Player.
  for (const k of oldKeys) {
    if (newKeys.has(k)) continue;
    if (k === homeKey0) continue;
    const ch = channels.get(k);
    if (ch) {
      ch.leave();
      channels.delete(k);
      channelSnapshots.delete(k);
    }
  }

  // Subscribe to newly-in-window chunks as observers.
  for (const k of newKeys) {
    if (channels.has(k)) continue;
    const [ncx, ncy] = k.split(':').map((s) => parseInt(s, 10));
    subscribeChunk([ncx, ncy], 'observer');
  }

  windowCenter = newCenter;
}

(window as unknown as { __game: unknown }).__game = {
  username,
  homeChunk,
  players(): Record<string, PlayerPos> {
    const out: Record<string, PlayerPos> = {};
    for (const [name, mesh] of playerMeshes) {
      out[name] = { x: mesh.position.x, y: mesh.position.z };
    }
    return out;
  },
};

const socket = new Socket('/socket');
socket.connect();

const channels = new Map<string, Channel>();

function subscribeChunk(coord: Coord, role: 'owner' | 'observer'): Channel {
  const key = chunkKey(coord);
  const topic = `chunk:${coord[0]}:${coord[1]}`;
  const channel = socket.channel(topic, { username, role });
  channel.on('snapshot', (snap: Snapshot) => ingestChunkSnapshot(key, snap));
  channel
    .join()
    .receive('error', (e: unknown) => console.error(`join ${topic} failed`, e));
  channels.set(key, channel);
  return channel;
}

const homeKey = chunkKey(homeChunk);
const ownerChannel = subscribeChunk(homeChunk, 'owner');

for (const coord of windowCoords(homeChunk)) {
  if (chunkKey(coord) === homeKey) continue;
  subscribeChunk(coord, 'observer');
}

const keys = { w: false, a: false, s: false, d: false };
let lastIntent = { dx: 0, dy: 0 };

function currentIntent(): { dx: number; dy: number } {
  const dx = (keys.d ? 1 : 0) - (keys.a ? 1 : 0);
  const dy = (keys.s ? 1 : 0) - (keys.w ? 1 : 0);
  const len = Math.hypot(dx, dy);
  return len === 0 ? { dx: 0, dy: 0 } : { dx: dx / len, dy: dy / len };
}

function maybePushIntent(): void {
  const intent = currentIntent();
  if (intent.dx !== lastIntent.dx || intent.dy !== lastIntent.dy) {
    ownerChannel.push('move', intent);
    lastIntent = intent;
  }
}

window.addEventListener('keydown', (e) => {
  if (e.repeat) return;
  if (e.key in keys) {
    keys[e.key as keyof typeof keys] = true;
    maybePushIntent();
  }
});
window.addEventListener('keyup', (e) => {
  if (e.key in keys) {
    keys[e.key as keyof typeof keys] = false;
    maybePushIntent();
  }
});

window.addEventListener('resize', () => {
  camera.aspect = window.innerWidth / window.innerHeight;
  camera.updateProjectionMatrix();
  renderer.setSize(window.innerWidth, window.innerHeight);
});

renderer.setAnimationLoop(() => {
  renderer.render(scene, camera);
});
