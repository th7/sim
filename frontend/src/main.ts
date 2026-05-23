import * as THREE from 'three';
import { Socket, type Channel } from 'phoenix';

type PlayerPos = { x: number; y: number };
type ResourceNode = { type: string; x: number; y: number; depleted: boolean };
type StructureEntry = { type: string; x: number; y: number; hp: number; owner: string };
type PortalEntry = { type: string; direction: string; x: number; y: number };
// Server snapshots carry positions in **sub-units** (1 world unit = 1000
// sub-units); we divide at the channel boundary so the rest of the
// frontend works in world-unit floats for Three.js.
type WireSnapshot = {
  players: Record<string, { x: number; y: number }>;
  resource_nodes: Record<string, { type: string; x: number; y: number; depleted: boolean }>;
  structures: Record<string, { type: string; x: number; y: number; hp: number; owner: string }>;
  portals: Record<string, { type: string; direction: string; x: number; y: number }>;
};
type Snapshot = {
  players: Record<string, PlayerPos>;
  resource_nodes: Record<string, ResourceNode>;
  structures: Record<string, StructureEntry>;
  portals: Record<string, PortalEntry>;
};
type Coord = readonly [number, number];
type Inventory = Record<string, number>;

const SUB_UNITS_PER_UNIT = 1000;
const CHUNK_SIZE = 16;
const INTERACT_RANGE = 1.0;
const WALL_COST = 5;
const SUB = SUB_UNITS_PER_UNIT;

function fromSubUnits(snap: WireSnapshot): Snapshot {
  const players: Record<string, PlayerPos> = {};
  for (const [name, p] of Object.entries(snap.players ?? {})) {
    players[name] = { x: p.x / SUB, y: p.y / SUB };
  }

  const resource_nodes: Record<string, ResourceNode> = {};
  for (const [id, n] of Object.entries(snap.resource_nodes ?? {})) {
    resource_nodes[id] = { type: n.type, x: n.x / SUB, y: n.y / SUB, depleted: n.depleted };
  }

  const structures: Record<string, StructureEntry> = {};
  for (const [id, s] of Object.entries(snap.structures ?? {})) {
    structures[id] = { type: s.type, x: s.x / SUB, y: s.y / SUB, hp: s.hp, owner: s.owner };
  }

  const portals: Record<string, PortalEntry> = {};
  for (const [id, p] of Object.entries(snap.portals ?? {})) {
    portals[id] = { type: p.type, direction: p.direction, x: p.x / SUB, y: p.y / SUB };
  }

  return { players, resource_nodes, structures, portals };
}

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

const OVERWORLD_BG = 0x101010;
const INSTANCE_BG = 0x1a1030;

const scene = new THREE.Scene();
scene.background = new THREE.Color(OVERWORLD_BG);

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
// Latest snapshot position per Player, plus a per-mesh "from" snapshot used to
// interpolate during animation frames. Snapshots arrive at ~10 Hz; lerping
// between consecutive ones over the snapshot interval (~100 ms) eliminates
// visible jitter without introducing client-side prediction.
const playerTargets = new Map<string, PlayerPos>();
const playerLerpFrom = new Map<string, PlayerPos>();
const playerLerpStart = new Map<string, number>();
const SNAPSHOT_INTERVAL_MS = 100;
const palette = [0x4caf50, 0x2196f3, 0xff9800, 0xe91e63, 0x9c27b0, 0xffeb3b];

function colorFor(name: string): number {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  return palette[Math.abs(h) % palette.length];
}

// One snapshot map per subscribed chunk. The rendered set is the union.
const channelSnapshots = new Map<string, Map<string, PlayerPos>>();
const channelNodes = new Map<string, Map<string, ResourceNode>>();
const channelStructures = new Map<string, Map<string, StructureEntry>>();
const channelPortals = new Map<string, Map<string, PortalEntry>>();
const nodeMeshes = new Map<string, THREE.Mesh>();
const structureMeshes = new Map<string, THREE.Mesh>();
const portalMeshes = new Map<string, THREE.Mesh>();

const NODE_GATHERABLE_COLOR = 0x2e7d32;
const NODE_DEPLETED_COLOR = 0x6d4c41;
const WALL_COLOR = 0x90a4ae;
const PORTAL_INTO_COLOR = 0x7e57c2;
const PORTAL_OUT_COLOR = 0xff7043;

function updateRenderedFromMerge(): void {
  const union = new Map<string, PlayerPos>();
  for (const m of channelSnapshots.values()) {
    for (const [name, pos] of m) union.set(name, pos);
  }
  const now = performance.now();
  for (const [name, pos] of union) {
    let mesh = playerMeshes.get(name);
    if (!mesh) {
      mesh = new THREE.Mesh(
        new THREE.BoxGeometry(1, 1, 1),
        new THREE.MeshBasicMaterial({ color: colorFor(name) }),
      );
      mesh.position.set(pos.x, 0.5, pos.y);
      scene.add(mesh);
      playerMeshes.set(name, mesh);
      playerLerpFrom.set(name, pos);
    } else {
      // Capture the mesh's current visible position as the start of the
      // next lerp segment (not the previous target — using the actual
      // visible position keeps motion continuous even if a snapshot
      // arrived mid-segment).
      playerLerpFrom.set(name, { x: mesh.position.x, y: mesh.position.z });
    }
    playerTargets.set(name, pos);
    playerLerpStart.set(name, now);
  }
  for (const [name, mesh] of playerMeshes) {
    if (!union.has(name)) {
      scene.remove(mesh);
      playerMeshes.delete(name);
      playerTargets.delete(name);
      playerLerpFrom.delete(name);
      playerLerpStart.delete(name);
    }
  }

  // Resource nodes (trees + stumps).
  const nodeUnion = new Map<string, ResourceNode>();
  for (const m of channelNodes.values()) {
    for (const [id, n] of m) nodeUnion.set(id, n);
  }
  for (const [id, n] of nodeUnion) {
    let mesh = nodeMeshes.get(id);
    if (!mesh) {
      mesh = new THREE.Mesh(
        new THREE.BoxGeometry(0.8, n.depleted ? 0.2 : 1.5, 0.8),
        new THREE.MeshBasicMaterial({
          color: n.depleted ? NODE_DEPLETED_COLOR : NODE_GATHERABLE_COLOR,
        }),
      );
      mesh.userData = { kind: 'node', id };
      scene.add(mesh);
      nodeMeshes.set(id, mesh);
    } else {
      const mat = mesh.material as THREE.MeshBasicMaterial;
      mat.color.setHex(n.depleted ? NODE_DEPLETED_COLOR : NODE_GATHERABLE_COLOR);
      const h = n.depleted ? 0.2 : 1.5;
      mesh.geometry.dispose();
      mesh.geometry = new THREE.BoxGeometry(0.8, h, 0.8);
    }
    const h = n.depleted ? 0.2 : 1.5;
    mesh.position.set(n.x, h / 2, n.y);
    mesh.userData.depleted = n.depleted;
    mesh.userData.x = n.x;
    mesh.userData.y = n.y;
  }
  for (const [id, mesh] of nodeMeshes) {
    if (!nodeUnion.has(id)) {
      scene.remove(mesh);
      nodeMeshes.delete(id);
    }
  }

  // Structures (walls).
  const structUnion = new Map<string, StructureEntry>();
  for (const m of channelStructures.values()) {
    for (const [id, s] of m) structUnion.set(id, s);
  }
  for (const [id, s] of structUnion) {
    let mesh = structureMeshes.get(id);
    if (!mesh) {
      mesh = new THREE.Mesh(
        new THREE.BoxGeometry(0.9, 1, 0.9),
        new THREE.MeshBasicMaterial({ color: WALL_COLOR }),
      );
      mesh.userData = { kind: 'structure', id };
      scene.add(mesh);
      structureMeshes.set(id, mesh);
    }
    mesh.position.set(s.x, 0.5, s.y);
    mesh.userData.x = s.x;
    mesh.userData.y = s.y;
  }
  for (const [id, mesh] of structureMeshes) {
    if (!structUnion.has(id)) {
      scene.remove(mesh);
      structureMeshes.delete(id);
    }
  }

  // Portals.
  const portalUnion = new Map<string, PortalEntry>();
  for (const m of channelPortals.values()) {
    for (const [id, p] of m) portalUnion.set(id, p);
  }
  for (const [id, p] of portalUnion) {
    let mesh = portalMeshes.get(id);
    if (!mesh) {
      mesh = new THREE.Mesh(
        new THREE.CylinderGeometry(0.5, 0.5, 1.5, 16),
        new THREE.MeshBasicMaterial({
          color: p.direction === 'into_instance' ? PORTAL_INTO_COLOR : PORTAL_OUT_COLOR,
          transparent: true,
          opacity: 0.7,
        }),
      );
      mesh.userData = { kind: 'portal', id };
      scene.add(mesh);
      portalMeshes.set(id, mesh);
    }
    mesh.position.set(p.x, 0.75, p.y);
  }
  for (const [id, mesh] of portalMeshes) {
    if (!portalUnion.has(id)) {
      scene.remove(mesh);
      portalMeshes.delete(id);
    }
  }
}

function ingestChunkSnapshot(key: string, snap: Snapshot): void {
  channelSnapshots.set(key, new Map(Object.entries(snap.players)));
  channelNodes.set(key, new Map(Object.entries(snap.resource_nodes)));
  channelStructures.set(key, new Map(Object.entries(snap.structures)));
  channelPortals.set(key, new Map(Object.entries(snap.portals)));

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

function maybeShiftWindow({ x, y }: PlayerPos): void {
  const [cx, cy] = [Math.floor(x / CHUNK_SIZE), Math.floor(y / CHUNK_SIZE)];
  if (cx === windowCenter[0] && cy === windowCenter[1]) return;

  const newCenter: Coord = [cx, cy];
  const oldKeys = new Set(windowCoords(windowCenter).map(chunkKey));
  const newKeys = new Set(windowCoords(newCenter).map(chunkKey));

  // Drop stale snapshot subscriptions.
  for (const k of oldKeys) {
    if (newKeys.has(k)) continue;
    const ch = channels.get(k);
    if (ch) {
      ch.leave();
      channels.delete(k);
      channelSnapshots.delete(k);
      channelNodes.delete(k);
      channelStructures.delete(k);
      channelPortals.delete(k);
    }
  }

  // Subscribe to newly-in-window chunks.
  for (const k of newKeys) {
    if (channels.has(k)) continue;
    const [ncx, ncy] = k.split(':').map((s) => parseInt(s, 10));
    subscribeChunk([ncx, ncy]);
  }

  windowCenter = newCenter;
}

let ownInventory: Inventory = {};

const invHudEl = document.createElement('div');
invHudEl.id = 'inv-hud';
Object.assign(invHudEl.style, {
  position: 'fixed',
  top: '8px',
  right: '8px',
  padding: '6px 10px',
  background: 'rgba(0, 0, 0, 0.6)',
  color: '#eee',
  font: '12px ui-monospace, monospace',
  whiteSpace: 'pre',
  pointerEvents: 'none',
  zIndex: '5',
});
document.body.appendChild(invHudEl);

function refreshHudInventory(): void {
  const lines = Object.entries(ownInventory)
    .filter(([, n]) => n > 0)
    .map(([k, n]) => `${k.padEnd(8)} ${n}`);
  invHudEl.textContent = lines.length ? lines.join('\n') : '(empty)';
}

refreshHudInventory();

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
  inventory(): Inventory {
    return { ...ownInventory };
  },
  structures(): Record<string, { x: number; y: number; hp: number; owner: string }> {
    const out: Record<string, { x: number; y: number; hp: number; owner: string }> = {};
    for (const m of channelStructures.values()) {
      for (const [id, s] of m) out[id] = { x: s.x, y: s.y, hp: s.hp, owner: s.owner };
    }
    return out;
  },
  resourceNodes(): Record<string, ResourceNode> {
    const out: Record<string, ResourceNode> = {};
    for (const m of channelNodes.values()) {
      for (const [id, n] of m) out[id] = n;
    }
    return out;
  },
  portals(): Record<string, PortalEntry> {
    const out: Record<string, PortalEntry> = {};
    for (const m of channelPortals.values()) {
      for (const [id, p] of m) out[id] = p;
    }
    return out;
  },
  realm(): Realm {
    return currentRealm;
  },
  click(worldX: number, worldY: number): void {
    handleWorldClick(worldX, worldY);
  },
  harvest(subX: number, subY: number): void {
    playerChannel.push('harvest', { x: subX, y: subY });
  },
  build(type: string, subX: number, subY: number): void {
    playerChannel.push('build', { type, x: subX, y: subY });
  },
  damage(subX: number, subY: number): void {
    playerChannel.push('damage', { x: subX, y: subY });
  },
};

const socket = new Socket('/socket');
socket.onOpen(() => console.log('socket:open'));
socket.onClose(() => console.log('socket:close'));
socket.onError((e: unknown) => console.log('socket:error', e));
socket.connect();

const channels = new Map<string, Channel>();
type Realm = { kind: 'overworld' } | { kind: 'instance'; id: number };
let currentRealm: Realm = { kind: 'overworld' };

function topicFor(realm: Realm, coord: Coord): string {
  return realm.kind === 'overworld'
    ? `chunk:${coord[0]}:${coord[1]}`
    : `instance:${realm.id}:chunk:${coord[0]}:${coord[1]}`;
}

function subscribeChunk(coord: Coord): Channel {
  const key = chunkKey(coord);
  const topic = topicFor(currentRealm, coord);
  const channel = socket.channel(topic, { username });
  channel.on('snapshot', (snap: WireSnapshot) =>
    ingestChunkSnapshot(key, fromSubUnits(snap)),
  );
  channel
    .join()
    .receive('error', (e: unknown) => console.error(`join ${topic} failed`, e));
  channels.set(key, channel);
  return channel;
}

function clearAllChunkSubscriptions(): void {
  for (const ch of channels.values()) ch.leave();
  channels.clear();
  channelSnapshots.clear();
  channelNodes.clear();
  channelStructures.clear();
  channelPortals.clear();
}

// One persistent player channel hosts all input verbs and per-Player events.
const playerChannel = socket.channel(`player:${username}`, {
  username,
  initial_chunk: [homeChunk[0], homeChunk[1]],
});
playerChannel.on('self', (payload: { inventory: Inventory }) => {
  ownInventory = payload.inventory ?? {};
  refreshHudInventory();
});
playerChannel.on('relocated', (payload: { realm: Realm; coord: Coord }) => {
  currentRealm = payload.realm;
  windowCenter = payload.coord;
  (scene.background as THREE.Color).setHex(
    currentRealm.kind === 'instance' ? INSTANCE_BG : OVERWORLD_BG,
  );
  clearAllChunkSubscriptions();
  for (const coord of windowCoords(payload.coord)) {
    subscribeChunk(coord);
  }
});
playerChannel
  .join()
  .receive('error', (e: unknown) => console.error(`join player:${username} failed`, e));

for (const coord of windowCoords(homeChunk)) {
  subscribeChunk(coord);
}

function handleWorldClick(worldX: number, worldY: number): void {
  const me = findOwnCube();
  if (!me) return;

  // 1) tree at the click position?
  for (const [, m] of channelNodes) {
    for (const [, n] of m) {
      if (n.depleted) continue;
      if (Math.abs(n.x - worldX) < 0.5 && Math.abs(n.y - worldY) < 0.5) {
        playerChannel.push('harvest', {
          x: Math.round(n.x * SUB),
          y: Math.round(n.y * SUB),
        });
        return;
      }
    }
  }

  // 2) structure at the click position?
  for (const [, m] of channelStructures) {
    for (const [, s] of m) {
      if (Math.abs(s.x - worldX) < 0.5 && Math.abs(s.y - worldY) < 0.5) {
        playerChannel.push('damage', { x: Math.round(s.x * SUB), y: Math.round(s.y * SUB) });
        return;
      }
    }
  }

  // 3) build on an empty cell (1.0u grid-snap, anchored at integer world units)
  //    if we have materials.
  const have = ownInventory.wood ?? 0;
  if (have < WALL_COST) return;
  const cellX = Math.floor(worldX) * SUB + SUB / 2;
  const cellY = Math.floor(worldY) * SUB + SUB / 2;
  const dx = me.x - cellX / SUB;
  const dy = me.y - cellY / SUB;
  if (dx * dx + dy * dy > INTERACT_RANGE * INTERACT_RANGE) return;
  playerChannel.push('build', { type: 'wall', x: cellX, y: cellY });
}

renderer.domElement.addEventListener('click', (ev) => {
  const rect = renderer.domElement.getBoundingClientRect();
  const ndcX = ((ev.clientX - rect.left) / rect.width) * 2 - 1;
  const ndcY = -((ev.clientY - rect.top) / rect.height) * 2 + 1;
  const raycaster = new THREE.Raycaster();
  raycaster.setFromCamera(new THREE.Vector2(ndcX, ndcY), camera);
  const plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
  const point = new THREE.Vector3();
  raycaster.ray.intersectPlane(plane, point);
  handleWorldClick(point.x, point.z);
});

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
    playerChannel.push('move', intent);
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
  // Lerp each Player mesh toward its latest target. Linear over
  // SNAPSHOT_INTERVAL_MS so a steady snapshot stream gives steady motion;
  // capped at 1.0 so late frames don't overshoot.
  const now = performance.now();
  for (const [name, mesh] of playerMeshes) {
    const target = playerTargets.get(name);
    const from = playerLerpFrom.get(name);
    const start = playerLerpStart.get(name);
    if (!target || !from || start === undefined) continue;
    const t = Math.min(1, (now - start) / SNAPSHOT_INTERVAL_MS);
    mesh.position.set(
      from.x + (target.x - from.x) * t,
      0.5,
      from.y + (target.y - from.y) * t,
    );
  }
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
  const realmStr =
    currentRealm.kind === 'overworld' ? 'overworld' : `instance:${currentRealm.id}`;

  hudEl.textContent =
    `user:   ${username}\n` +
    `realm:  ${realmStr}\n` +
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
