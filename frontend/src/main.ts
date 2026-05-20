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
const devOnStart = urlParams.get('dev') === '1';

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

  // The local cube may have migrated to a different chunk than it joined
  // through; follow it wherever the server reports it.
  const mine = findOwnCube();
  if (mine) maybeShiftWindow(mine);

  updateRenderedFromMerge();
  if (devEnabled) refreshHud();
}

function findOwnCube(): PlayerPos | undefined {
  for (const m of channelSnapshots.values()) {
    const p = m.get(username);
    if (p) return p;
  }
  return undefined;
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
socket.onOpen(() => console.log('socket:open'));
socket.onClose(() => console.log('socket:close'));
socket.onError((e: unknown) => console.log('socket:error', e));
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

// ---------- Dev mode (Phase 6.5) ----------

type Lifecycle = 'hot' | 'idle_armed' | 'cold';
type AroundEntry = {
  cx: number;
  cy: number;
  lifecycle: Lifecycle;
  idle_ms_remaining: number | null;
  entity_count: number;
};
type DevStats = {
  active_chunks: number;
  total_players: number;
  around: AroundEntry[];
};

const devOverlay = new THREE.Group();
devOverlay.visible = false;
scene.add(devOverlay);

let devEnabled = false;
let devStatsChannel: Channel | null = null;
let latestStats: DevStats | null = null;
let hudEl: HTMLDivElement | null = null;

function setDevMode(on: boolean): void {
  if (on === devEnabled) return;
  devEnabled = on;
  devOverlay.visible = on;

  if (on) {
    ensureHud();
    devStatsChannel = socket.channel('dev:stats', { username });
    devStatsChannel.on('stats', (s: DevStats) => {
      latestStats = s;
      refreshHud();
      refreshOverlay();
    });
    devStatsChannel.join().receive('error', (e: unknown) => {
      console.error('join dev:stats failed', e);
    });
  } else {
    if (devStatsChannel) {
      devStatsChannel.leave();
      devStatsChannel = null;
    }
    if (hudEl) {
      hudEl.remove();
      hudEl = null;
    }
    clearOverlay();
    latestStats = null;
  }
}

function ensureHud(): void {
  if (hudEl) return;
  hudEl = document.createElement('div');
  hudEl.id = 'dev-hud';
  Object.assign(hudEl.style, {
    position: 'fixed',
    top: '8px',
    left: '8px',
    padding: '6px 10px',
    background: 'rgba(0,0,0,0.6)',
    color: '#fff',
    font: '12px/1.4 ui-monospace, monospace',
    whiteSpace: 'pre',
    zIndex: '10',
    pointerEvents: 'none',
  } as CSSStyleDeclaration);
  document.body.appendChild(hudEl);
  refreshHud();
}

function refreshHud(): void {
  if (!hudEl) return;
  const me = findOwnCube();
  const pos = me ? `(${me.x.toFixed(1)}, ${me.y.toFixed(1)})` : '—';
  const chunkCoord: Coord = me
    ? [Math.floor(me.x / CHUNK_SIZE), Math.floor(me.y / CHUNK_SIZE)]
    : homeChunk;
  const view = Object.keys((window as unknown as { __game: { players(): Record<string, PlayerPos> } }).__game.players()).length;
  const active = latestStats?.active_chunks ?? '—';
  const total = latestStats?.total_players ?? '—';

  hudEl.textContent =
    `user:   ${username}\n` +
    `pos:    ${pos}  chunk: (${chunkCoord[0]}, ${chunkCoord[1]})\n` +
    `view:   ${view}  active: ${active}  total: ${total}`;
}

function clearOverlay(): void {
  for (const child of [...devOverlay.children]) devOverlay.remove(child);
}

function refreshOverlay(): void {
  clearOverlay();
  if (!latestStats) return;

  const me = findOwnCube();
  const myChunk: Coord = me
    ? [Math.floor(me.x / CHUNK_SIZE), Math.floor(me.y / CHUNK_SIZE)]
    : homeChunk;

  for (const e of latestStats.around) {
    devOverlay.add(buildChunkOverlay(e, myChunk));
  }
}

function buildChunkOverlay(e: AroundEntry, myChunk: Coord): THREE.Group {
  const g = new THREE.Group();
  const x0 = e.cx * CHUNK_SIZE;
  const z0 = e.cy * CHUNK_SIZE;
  const cx = x0 + CHUNK_SIZE / 2;
  const cz = z0 + CHUNK_SIZE / 2;

  const fillColor = e.lifecycle === 'hot' ? 0x244d24 : e.lifecycle === 'idle_armed' ? 0x6e5a1f : 0x222222;
  const fillOpacity = e.lifecycle === 'cold' ? 0.05 : 0.25;

  const fill = new THREE.Mesh(
    new THREE.PlaneGeometry(CHUNK_SIZE, CHUNK_SIZE),
    new THREE.MeshBasicMaterial({ color: fillColor, transparent: true, opacity: fillOpacity, depthWrite: false }),
  );
  fill.rotation.x = -Math.PI / 2;
  fill.position.set(cx, 0.005, cz);
  g.add(fill);

  // Shrinking countdown bar for idle-armed chunks.
  if (e.lifecycle === 'idle_armed' && e.idle_ms_remaining != null) {
    const fraction = Math.max(0, Math.min(1, e.idle_ms_remaining / 5000));
    const bar = new THREE.Mesh(
      new THREE.PlaneGeometry(CHUNK_SIZE * fraction, 0.5),
      new THREE.MeshBasicMaterial({ color: 0xffcc00, transparent: true, opacity: 0.8, depthWrite: false }),
    );
    bar.rotation.x = -Math.PI / 2;
    bar.position.set(x0 + (CHUNK_SIZE * fraction) / 2, 0.012, z0 + 0.5);
    g.add(bar);
  }

  // Border encoding: owner / view / warm-only / outside-warm.
  const ringD = Math.max(Math.abs(e.cx - myChunk[0]), Math.abs(e.cy - myChunk[1]));
  const borderColor =
    ringD === 0 ? 0xffffff : ringD <= 1 ? 0xcccccc : ringD <= 2 ? 0x888888 : 0x555555;
  const dashed = ringD > 2;
  const borderGeom = new THREE.BufferGeometry().setFromPoints([
    new THREE.Vector3(x0, 0.01, z0),
    new THREE.Vector3(x0 + CHUNK_SIZE, 0.01, z0),
    new THREE.Vector3(x0 + CHUNK_SIZE, 0.01, z0 + CHUNK_SIZE),
    new THREE.Vector3(x0, 0.01, z0 + CHUNK_SIZE),
    new THREE.Vector3(x0, 0.01, z0),
  ]);
  const border = new THREE.Line(
    borderGeom,
    dashed
      ? new THREE.LineDashedMaterial({ color: borderColor, dashSize: 0.5, gapSize: 0.5 })
      : new THREE.LineBasicMaterial({ color: borderColor, linewidth: ringD === 0 ? 4 : 2 }),
  );
  if (dashed) border.computeLineDistances();
  g.add(border);

  g.add(buildCoordLabel(e.cx, e.cy, x0, z0));
  return g;
}

function buildCoordLabel(cx: number, cy: number, x0: number, z0: number): THREE.Sprite {
  const canvas = document.createElement('canvas');
  canvas.width = 128;
  canvas.height = 32;
  const ctx = canvas.getContext('2d')!;
  ctx.fillStyle = '#ffffff';
  ctx.font = 'bold 20px ui-monospace, monospace';
  ctx.textBaseline = 'top';
  ctx.fillText(`${cx},${cy}`, 4, 4);
  const tex = new THREE.CanvasTexture(canvas);
  const mat = new THREE.SpriteMaterial({ map: tex, depthWrite: false });
  const sprite = new THREE.Sprite(mat);
  sprite.scale.set(3, 0.75, 1);
  sprite.position.set(x0 + 1.8, 0.02, z0 + 0.5);
  return sprite;
}

window.addEventListener('keydown', (e) => {
  if (e.key === '`') setDevMode(!devEnabled);
});

if (devOnStart) setDevMode(true);
