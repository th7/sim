import * as THREE from 'three';

const PALETTE = [0x4caf50, 0x2196f3, 0xff9800, 0xe91e63, 0x9c27b0, 0xffeb3b];

const HEAD_COLOR = 0xeac9a0;

const TRUNK_COLOR = 0x6d4c41;
const FOLIAGE_COLOR = 0x2e7d32;

const PLANK_COLORS = [0x8d6e63, 0x795548, 0x6d4c41];

const PORTAL_INTO_COLOR = 0x7e57c2;
const PORTAL_OUT_COLOR = 0xff7043;

function lambert(color: number, opts: { transparent?: boolean; opacity?: number } = {}): THREE.MeshLambertMaterial {
  return new THREE.MeshLambertMaterial({
    color,
    flatShading: true,
    transparent: opts.transparent ?? false,
    opacity: opts.opacity ?? 1,
  });
}

function hashColor(name: string): number {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  return PALETTE[Math.abs(h) % PALETTE.length];
}

export function createPlayerMesh(name: string): THREE.Group {
  const group = new THREE.Group();

  const body = new THREE.Mesh(
    new THREE.BoxGeometry(0.6, 1.0, 0.6),
    lambert(hashColor(name)),
  );
  body.name = 'body';
  body.position.y = 0.5;
  body.castShadow = true;
  group.add(body);

  const head = new THREE.Mesh(
    new THREE.BoxGeometry(0.5, 0.5, 0.5),
    lambert(HEAD_COLOR),
  );
  head.name = 'head';
  head.position.y = 1.25;
  head.castShadow = true;
  group.add(head);

  return group;
}

export function createTreeMesh(): THREE.Group {
  const group = new THREE.Group();

  const trunk = new THREE.Mesh(
    new THREE.CylinderGeometry(0.15, 0.18, 0.6, 6),
    lambert(TRUNK_COLOR),
  );
  trunk.name = 'trunk';
  trunk.position.y = 0.3;
  trunk.castShadow = true;
  group.add(trunk);

  const foliage = new THREE.Group();
  foliage.name = 'foliage';

  const lower = new THREE.Mesh(
    new THREE.ConeGeometry(0.55, 0.7, 6),
    lambert(FOLIAGE_COLOR),
  );
  lower.position.y = 0.85;
  lower.castShadow = true;
  foliage.add(lower);

  const upper = new THREE.Mesh(
    new THREE.ConeGeometry(0.4, 0.55, 6),
    lambert(FOLIAGE_COLOR),
  );
  upper.position.y = 1.3;
  upper.castShadow = true;
  foliage.add(upper);

  group.add(foliage);
  return group;
}

export function setTreeDepleted(group: THREE.Group, depleted: boolean): void {
  const foliage = group.getObjectByName('foliage');
  if (foliage) foliage.visible = !depleted;
}

export function createWallMesh(): THREE.Group {
  const group = new THREE.Group();

  const PLANK_W = 0.28;
  const PLANK_H = 1.0;
  const PLANK_D = 0.9;
  const GAP = 0.01;
  const pitch = PLANK_W + GAP;
  const startX = -pitch;

  for (let i = 0; i < 3; i++) {
    const plank = new THREE.Mesh(
      new THREE.BoxGeometry(PLANK_W, PLANK_H, PLANK_D),
      lambert(PLANK_COLORS[i]),
    );
    plank.position.set(startX + i * pitch, PLANK_H / 2, 0);
    plank.castShadow = true;
    group.add(plank);
  }

  return group;
}

export function createPortalMesh(direction: string): THREE.Group {
  const group = new THREE.Group();
  const color = direction === 'into_instance' ? PORTAL_INTO_COLOR : PORTAL_OUT_COLOR;

  const disc = new THREE.Mesh(
    new THREE.CylinderGeometry(0.65, 0.65, 0.04, 12),
    lambert(color),
  );
  disc.name = 'disc';
  disc.position.y = 0.02;
  disc.castShadow = true;
  group.add(disc);

  const ring = new THREE.Mesh(
    new THREE.TorusGeometry(0.6, 0.12, 8, 16),
    lambert(color, { transparent: true, opacity: 0.7 }),
  );
  ring.name = 'ring';
  ring.position.y = 0.75;
  group.add(ring);

  return group;
}
