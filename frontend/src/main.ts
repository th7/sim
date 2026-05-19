import * as THREE from 'three';
import { Socket, type Channel } from 'phoenix';

type PlayerPos = { x: number; y: number };
type Snapshot = { players: Record<string, PlayerPos> };

const app = document.querySelector<HTMLDivElement>('#app')!;

const username =
  new URLSearchParams(window.location.search).get('u') ??
  `player-${Math.floor(Math.random() * 10000)}`;

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x101010);

const camera = new THREE.PerspectiveCamera(
  50,
  window.innerWidth / window.innerHeight,
  0.1,
  500,
);
camera.position.set(12, 12, 12);
camera.lookAt(0, 0, 0);

const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setPixelRatio(window.devicePixelRatio);
renderer.setSize(window.innerWidth, window.innerHeight);
app.appendChild(renderer.domElement);

const grid = new THREE.GridHelper(20, 20, 0x404040, 0x202020);
scene.add(grid);

const playerMeshes = new Map<string, THREE.Mesh>();
const palette = [0x4caf50, 0x2196f3, 0xff9800, 0xe91e63, 0x9c27b0, 0xffeb3b];

function colorFor(name: string): number {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  return palette[Math.abs(h) % palette.length];
}

function applySnapshot(snap: Snapshot): void {
  const seen = new Set<string>();
  for (const [name, pos] of Object.entries(snap.players)) {
    seen.add(name);
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
    if (!seen.has(name)) {
      scene.remove(mesh);
      playerMeshes.delete(name);
    }
  }
}

// Read-only view of rendered player positions, for E2E smoke tests.
(window as unknown as { __game: unknown }).__game = {
  username,
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
const channel: Channel = socket.channel('chunk:0:0', { username });
channel
  .join()
  .receive('ok', () => console.info(`joined as ${username}`))
  .receive('error', (e: unknown) => console.error('join failed', e));
channel.on('snapshot', (snap: Snapshot) => applySnapshot(snap));

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
    channel.push('move', intent);
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
