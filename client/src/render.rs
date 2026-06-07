//! The three-d/egui view layer: builds the 3D scene and HUD from a
//! [`RenderState`] each frame. Shared by the `game` bin (live session) and the
//! `showcase` bin (synthetic scenarios), so both render through the same path.
//!
//! NOTE: requires a GL context; nothing here is unit-testable headlessly. The
//! model side is tested; pixels are verified manually on a real display.

use crate::session::RenderState;
use protocol::consts::IDLE_TIMEOUT_MS;
use protocol::geometry::{ChunkCoord, SUB_UNITS_PER_UNIT};
use protocol::types::{NpcKind, PortalDirection};
use protocol::wire::{ChunkLifecycle, RealmWire, StatsPayload};
use std::collections::HashMap;
use three_d::*;

/// World units per chunk edge (matches the server's `CHUNK_SIZE`).
pub const CHUNK_SIZE: f32 = 16.0;
/// Positions are the Mirror's speculation, advancing at the 20 Hz client tick;
/// lerp toward each new target over one tick so motion stays smooth between
/// ticks. The same blend absorbs override corrections — state snaps exactly,
/// rendering glides.
const SNAPSHOT_INTERVAL_MS: f64 = 50.0;
/// Keep rendering a player this long after they vanish from snapshots, so a
/// chunk-boundary crossing (briefly in no snapshot) doesn't blink the cube out.
const PLAYER_REMOVE_GRACE_MS: f64 = 400.0;
/// Height of the dev chunk-lifecycle overlay above the ground plane (whose top
/// is y=0). The overlay is a flat transparent decal; it must clear the depth
/// buffer's resolution at the far edge of the 7×7 grid (~100 units from the
/// camera) or it z-fights the ground and flickers green as the camera moves. The
/// float is imperceptible on the featureless ground.
const DEV_OVERLAY_Y: f32 = 0.08;

/// sub-units → world units.
fn w(sub: i64) -> f32 {
    sub as f32 / SUB_UNITS_PER_UNIT as f32
}

/// Per-player interpolation state: lerp the visible cube from `from` toward
/// `target` over `SNAPSHOT_INTERVAL_MS` starting at `start`. `last_seen` drives
/// the removal grace.
#[derive(Clone, Copy)]
struct Lerp {
    from: (f32, f32),
    target: (f32, f32),
    start: f64,
    last_seen: f64,
}

impl Lerp {
    fn visible(&self, now: f64) -> (f32, f32) {
        let t = (((now - self.start) / SNAPSHOT_INTERVAL_MS).clamp(0.0, 1.0)) as f32;
        (
            self.from.0 + (self.target.0 - self.from.0) * t,
            self.from.1 + (self.target.1 - self.from.1) * t,
        )
    }
}

/// The view: camera, lights, GUI, and the per-frame interpolation state. One
/// per window; [`View::frame`] draws everything a [`RenderState`] describes.
pub struct View {
    camera: Camera,
    sun: DirectionalLight,
    ambient: AmbientLight,
    gui: GUI,
    lerps: HashMap<String, Lerp>,
    last_realm: RealmWire,
    cam_target: Vec3,
}

impl View {
    /// Camera, lights, and GUI for a window whose local player starts at `start`.
    pub fn new(context: &Context, viewport: Viewport, start: ChunkCoord) -> Self {
        let cam_target = vec3(start.cx as f32 * CHUNK_SIZE, 0.0, start.cy as f32 * CHUNK_SIZE);
        let camera = Camera::new_perspective(
            viewport,
            cam_target + cam_offset(),
            cam_target,
            vec3(0.0, 1.0, 0.0),
            degrees(50.0),
            // z_near/z_far bracket the scene: the camera sits ~20 units from the
            // local player and nothing is nearer than ~16, so a 0.1 near plane just
            // wrecked depth precision (a 5000:1 near:far ratio) and let the flat dev
            // overlay z-fight the ground. 4.0 clips nothing and is ~40× more precise.
            4.0,
            500.0,
        );
        // Key light aimed down the old client's (-6,10,-3) offset → travel dir (6,-10,3).
        let sun = DirectionalLight::new(context, 2.8, Srgba::WHITE, vec3(0.5, -0.83, 0.25));
        let ambient = AmbientLight::new(context, 1.0, Srgba::WHITE);
        View {
            camera,
            sun,
            ambient,
            gui: GUI::new(context),
            lerps: HashMap::new(),
            last_realm: RealmWire::Overworld,
            cam_target,
        }
    }

    /// Track the window viewport. Called at the top of the frame, before input
    /// handling, so picking through [`View::camera`] sees the current size.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.camera.set_viewport(viewport);
    }

    /// The scene camera (for unprojecting input clicks).
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Build and draw one frame from `rs`. `extra_ui` lets the caller add its
    /// own egui windows (the showcase legend); the game passes `|_| {}`.
    pub fn frame(
        &mut self,
        context: &Context,
        frame_input: &mut FrameInput,
        rs: &RenderState,
        dev_view: bool,
        mut extra_ui: impl FnMut(&three_d::egui::Context),
    ) {
        let now = frame_input.accumulated_time;

        // --- player interpolation + removal grace ---
        // A realm switch teleports the player; clear lerp state so the cube
        // doesn't slide across the jump (mirrors clearAllChunkSubscriptions).
        if rs.realm != self.last_realm {
            self.lerps.clear();
            self.last_realm = rs.realm;
        }
        for (name, p) in &rs.players {
            let target = (w(p.x), w(p.y));
            match self.lerps.get_mut(name) {
                None => {
                    self.lerps.insert(
                        name.clone(),
                        Lerp { from: target, target, start: now, last_seen: now },
                    );
                }
                Some(l) => {
                    l.last_seen = now;
                    if l.target != target {
                        l.from = l.visible(now); // keep motion continuous mid-segment
                        l.target = target;
                        l.start = now;
                    }
                }
            }
        }
        self.lerps.retain(|_, l| now - l.last_seen < PLAYER_REMOVE_GRACE_MS);

        // --- build the scene ---
        let mut objects: Vec<Gm<Mesh, PhysicalMaterial>> = Vec::new();

        // Ground plane.
        objects.push(box_at(context, 0.0, -0.01, 0.0, 4000.0, 0.02, 4000.0, rgb(0x3a3a3a)));

        // Players (body + head), at the interpolated position.
        for (name, l) in &self.lerps {
            let (x, z) = l.visible(now);
            objects.push(box_at(context, x, 0.5, z, 0.6, 1.0, 0.6, hash_color(name)));
            objects.push(box_at(context, x, 1.25, z, 0.5, 0.5, 0.5, rgb(0xeac9a0)));
        }
        // Resource nodes: trunk always; conical foliage only when not depleted.
        for n in rs.nodes.values() {
            let (x, z) = (w(n.x), w(n.y));
            objects.push(cylinder_at(context, x, 0.0, z, 0.16, 0.6, rgb(0x6d4c41)));
            if !n.depleted {
                objects.push(cone_at(context, x, 0.5, z, 0.55, 0.7, rgb(0x2e7d32)));
                objects.push(cone_at(context, x, 1.0, z, 0.40, 0.55, rgb(0x2e7d32)));
            }
        }
        // Structures (walls): three planks along x.
        const PLANK_COLORS: [u32; 3] = [0x8d6e63, 0x795548, 0x6d4c41];
        for s in rs.structures.values() {
            let (x, z) = (w(s.x), w(s.y));
            for (i, c) in PLANK_COLORS.iter().enumerate() {
                let px = x + (i as f32 - 1.0) * 0.29; // pitch 0.28 + 0.01 gap
                objects.push(box_at(context, px, 0.5, z, 0.28, 1.0, 0.9, rgb(*c)));
            }
        }
        // Portals: a flat disc, coloured by direction (the torus ring the old
        // client drew has no three-d primitive — omitted, see port notes).
        // An unmapped wire string renders loud magenta rather than silently
        // passing as some known kind; the exhaustive match makes a new enum
        // variant a compile error here.
        for p in rs.portals.values() {
            let color = match PortalDirection::parse(&p.direction) {
                Some(PortalDirection::IntoInstance) => rgb(0x7e57c2),
                Some(PortalDirection::OutOfInstance) => rgb(0xff7043),
                None => rgb(0xff00ff),
            };
            objects.push(cylinder_at(context, w(p.x), 0.0, w(p.y), 0.65, 0.04, color));
        }
        // NPCs: a body box coloured by kind (wolf grey, deer tan) + a small head.
        for n in rs.npcs.values() {
            let (x, z) = (w(n.x), w(n.y));
            let color = match NpcKind::parse(&n.kind) {
                Some(NpcKind::Wolf) => rgb(0x607d8b),
                Some(NpcKind::Deer) => rgb(0xbcaaa4),
                None => rgb(0xff00ff),
            };
            objects.push(box_at(context, x, 0.4, z, 0.7, 0.5, 0.35, color));
            objects.push(box_at(context, x, 0.75, z, 0.3, 0.3, 0.3, color));
        }
        // Carcasses: a low dark-red mound.
        for c in rs.carcasses.values() {
            objects.push(box_at(context, w(c.x), 0.12, w(c.y), 0.6, 0.24, 0.5, rgb(0x8e3b2e)));
        }

        // Dev chunk-lifecycle overlay (transparent, drawn after opaque geometry).
        if let Some(stats) = &rs.stats {
            dev_overlay(context, stats, &mut objects);
        }

        // Camera follows the local player's interpolated position.
        if let Some(l) = self.lerps.get(&rs.own) {
            let (x, z) = l.visible(now);
            self.cam_target = vec3(x, 0.0, z);
        }
        self.camera.set_view(self.cam_target + cam_offset(), self.cam_target, vec3(0.0, 1.0, 0.0));

        // HUD: inventory always; dev panel (user/realm/pos/chunk/view/active/total) with dev mode.
        self.gui.update(
            &mut frame_input.events,
            frame_input.accumulated_time,
            frame_input.viewport,
            frame_input.device_pixel_ratio,
            |ctx| {
                use three_d::egui::{Align2, Window as EWindow};
                EWindow::new("inventory").anchor(Align2::LEFT_TOP, [8.0, 8.0]).show(ctx, |ui| {
                    if rs.inventory.is_empty() {
                        ui.label("(empty)");
                    }
                    for (item, n) in &rs.inventory {
                        ui.label(format!("{item}: {n}"));
                    }
                    // The last server-side reject reason (cleared when the user
                    // retries — see `ClientModel::click`). Without this the
                    // user's clicks fail silently when the server says no.
                    if let Some(err) = &rs.last_error {
                        ui.separator();
                        ui.label(format!("⚠ {}", err.replace('_', " ")));
                    }
                });
                if dev_view {
                    EWindow::new("dev")
                        .anchor(Align2::RIGHT_TOP, [-8.0, 8.0])
                        .show(ctx, |ui| dev_panel(ui, rs));
                }
                // The Mirror is frozen (connecting, relocating, or at its Lead
                // bound): say so — a stall must read as "connection", never as
                // a broken game or silently stale state.
                if rs.frozen {
                    EWindow::new("frozen").anchor(Align2::CENTER_TOP, [0.0, 8.0]).show(
                        ctx,
                        |ui| {
                            ui.label("⏸ waiting for the server…");
                        },
                    );
                }
                extra_ui(ctx);
            },
        );

        let bg = if matches!(rs.realm, RealmWire::Instance { .. }) {
            (0.102, 0.063, 0.188) // INSTANCE_BG 0x1a1030
        } else {
            (0.063, 0.063, 0.063) // OVERWORLD_BG 0x101010
        };
        frame_input
            .screen()
            .clear(ClearState::color_and_depth(bg.0, bg.1, bg.2, 1.0, 1.0))
            .render(&self.camera, objects.iter(), &[&self.sun, &self.ambient])
            .write(|| self.gui.render())
            .unwrap();
    }
}

/// Fixed isometric offset; the camera re-frames the local player every frame.
fn cam_offset() -> Vec3 {
    vec3(12.0, 12.0, 12.0)
}

/// Dev mode grid: faint 1×1 lines at every world unit (so the click-snap cells
/// are visible) and brighter lines at every chunk boundary (every 16 units).
/// Sized to cover the stats `around` ring exactly.
fn dev_grid(context: &Context, stats: &StatsPayload, objects: &mut Vec<Gm<Mesh, PhysicalMaterial>>) {
    let Some((cx_min, cx_max, cy_min, cy_max)) = stats.around.iter().fold(
        None,
        |acc: Option<(i32, i32, i32, i32)>, e| match acc {
            None => Some((e.cx, e.cx, e.cy, e.cy)),
            Some((nx, xx, ny, xy)) => Some((nx.min(e.cx), xx.max(e.cx), ny.min(e.cy), xy.max(e.cy))),
        },
    ) else {
        return;
    };
    let x_min = cx_min as f32 * CHUNK_SIZE;
    let x_max = (cx_max + 1) as f32 * CHUNK_SIZE;
    let z_min = cy_min as f32 * CHUNK_SIZE;
    let z_max = (cy_max + 1) as f32 * CHUNK_SIZE;
    let span_x = x_max - x_min;
    let span_z = z_max - z_min;
    let mid_x = x_min + span_x / 2.0;
    let mid_z = z_min + span_z / 2.0;

    // World-unit grid — every 1 unit, faint, below the lifecycle tiles.
    let unit_y = DEV_OVERLAY_Y - 0.02;
    let unit_color = rgba(0x888888, 36);
    for x in x_min as i32..=x_max as i32 {
        objects.push(flat_quad(context, x as f32, unit_y, mid_z, 0.04, span_z, unit_color));
    }
    for z in z_min as i32..=z_max as i32 {
        objects.push(flat_quad(context, mid_x, unit_y, z as f32, span_x, 0.04, unit_color));
    }

    // Chunk boundaries — every 16 units, brighter and thicker, above the tiles
    // so they read even through the hot/idle tints.
    let chunk_y = DEV_OVERLAY_Y + 0.02;
    let chunk_color = rgba(0xffcc44, 140);
    for cx in cx_min..=cx_max + 1 {
        let x = cx as f32 * CHUNK_SIZE;
        objects.push(flat_quad(context, x, chunk_y, mid_z, 0.10, span_z, chunk_color));
    }
    for cy in cy_min..=cy_max + 1 {
        let z = cy as f32 * CHUNK_SIZE;
        objects.push(flat_quad(context, mid_x, chunk_y, z, span_x, 0.10, chunk_color));
    }
}

/// The dev chunk-lifecycle overlay: a translucent tile per chunk in the stats
/// ring, coloured by lifecycle, with a shrinking countdown bar over chunks armed
/// for idle unload. Built into `objects`, drawn after the opaque scene. The
/// world-unit grid sits *under* the tiles so cell snap is visible at a glance;
/// the chunk-boundary grid sits *above* the tiles so it reads even through the
/// hot/idle tints.
fn dev_overlay(context: &Context, stats: &StatsPayload, objects: &mut Vec<Gm<Mesh, PhysicalMaterial>>) {
    dev_grid(context, stats, objects);

    for e in &stats.around {
        let x0 = e.cx as f32 * CHUNK_SIZE;
        let z0 = e.cy as f32 * CHUNK_SIZE;
        let (fill, alpha) = match e.lifecycle {
            ChunkLifecycle::Hot => (0x244d24, 64),
            ChunkLifecycle::IdleArmed => (0x6e5a1f, 64),
            ChunkLifecycle::Cold => (0x222222, 13),
        };
        objects.push(flat_quad(
            context,
            x0 + CHUNK_SIZE / 2.0,
            DEV_OVERLAY_Y,
            z0 + CHUNK_SIZE / 2.0,
            CHUNK_SIZE,
            CHUNK_SIZE,
            rgba(fill, alpha),
        ));
        // Shrinking idle countdown bar, just above the tile overlay.
        if e.lifecycle == ChunkLifecycle::IdleArmed {
            if let Some(rem) = e.idle_ms_remaining {
                let frac = (rem as f32 / IDLE_TIMEOUT_MS as f32).clamp(0.0, 1.0);
                objects.push(flat_quad(
                    context,
                    x0 + CHUNK_SIZE * frac / 2.0,
                    DEV_OVERLAY_Y + 0.04,
                    z0 + 0.5,
                    CHUNK_SIZE * frac,
                    0.5,
                    rgba(0xffcc00, 204),
                ));
            }
        }
    }
}

/// The dev HUD panel: user, realm, position/chunk, and the global counters.
fn dev_panel(ui: &mut three_d::egui::Ui, rs: &RenderState) {
    ui.label(format!("user:   {}", rs.own));
    let realm = match rs.realm {
        RealmWire::Overworld => "overworld".to_string(),
        RealmWire::Instance { id } => format!("instance:{id}"),
    };
    ui.label(format!("realm:  {realm}"));
    let (pos, chunk) = match rs.players.get(&rs.own) {
        Some(p) => {
            let (px, py) = (w(p.x), w(p.y));
            (
                format!("({px:.1}, {py:.1})"),
                format!("({}, {})", (px / CHUNK_SIZE).floor() as i32, (py / CHUNK_SIZE).floor() as i32),
            )
        }
        None => ("—".to_string(), "—".to_string()),
    };
    ui.label(format!("pos:    {pos}  chunk: {chunk}"));
    let (active, total) =
        rs.stats.as_ref().map(|s| (s.active_chunks, s.total_players)).unwrap_or((0, 0));
    ui.label(format!("view: {}  active: {}  total: {}", rs.players.len(), active, total));
    let world_npcs = rs.stats.as_ref().map(|s| s.total_npcs).unwrap_or(0);
    ui.label(format!("npcs: {} in view / {world_npcs} in world", rs.npcs.len()));
}

fn rgb(hex: u32) -> Srgba {
    Srgba::new(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8, 255)
}

fn rgba(hex: u32, a: u8) -> Srgba {
    let c = rgb(hex);
    Srgba::new(c.r, c.g, c.b, a)
}

/// A box centred at `(x, y, z)` with full extents `(sx, sy, sz)`.
fn box_at(context: &Context, x: f32, y: f32, z: f32, sx: f32, sy: f32, sz: f32, color: Srgba) -> Gm<Mesh, PhysicalMaterial> {
    let mut mesh = Gm::new(
        Mesh::new(context, &CpuMesh::cube()),
        PhysicalMaterial::new_opaque(context, &CpuMaterial { albedo: color, ..Default::default() }),
    );
    // CpuMesh::cube spans [-1, 1] (side 2), so halve the extents.
    mesh.set_transformation(
        Mat4::from_translation(vec3(x, y, z)) * Mat4::from_nonuniform_scale(sx / 2.0, sy / 2.0, sz / 2.0),
    );
    mesh
}

/// A thin transparent quad centred at `(cx, y, cz)` spanning `sx × sz` on the
/// ground plane — used for the dev overlay.
fn flat_quad(context: &Context, cx: f32, y: f32, cz: f32, sx: f32, sz: f32, color: Srgba) -> Gm<Mesh, PhysicalMaterial> {
    let mut mesh = Gm::new(
        Mesh::new(context, &CpuMesh::cube()),
        PhysicalMaterial::new_transparent(context, &CpuMaterial { albedo: color, ..Default::default() }),
    );
    mesh.set_transformation(
        Mat4::from_translation(vec3(cx, y, cz)) * Mat4::from_nonuniform_scale(sx / 2.0, 0.001, sz / 2.0),
    );
    mesh
}

/// A vertical cylinder of `radius`/`height` standing on `base_y`. `CpuMesh::cylinder`
/// runs along +x in [0,1] with radius 1, so we scale then rotate +x→+y.
fn cylinder_at(context: &Context, x: f32, base_y: f32, z: f32, radius: f32, height: f32, color: Srgba) -> Gm<Mesh, PhysicalMaterial> {
    let mut mesh = Gm::new(
        Mesh::new(context, &CpuMesh::cylinder(16)),
        PhysicalMaterial::new_opaque(context, &CpuMaterial { albedo: color, ..Default::default() }),
    );
    mesh.set_transformation(
        Mat4::from_translation(vec3(x, base_y, z))
            * Mat4::from_angle_z(degrees(90.0))
            * Mat4::from_nonuniform_scale(height, radius, radius),
    );
    mesh
}

/// A vertical cone with its base (`radius`) on `base_y` and apex `height` above.
fn cone_at(context: &Context, x: f32, base_y: f32, z: f32, radius: f32, height: f32, color: Srgba) -> Gm<Mesh, PhysicalMaterial> {
    let mut mesh = Gm::new(
        Mesh::new(context, &CpuMesh::cone(16)),
        PhysicalMaterial::new_opaque(context, &CpuMaterial { albedo: color, ..Default::default() }),
    );
    mesh.set_transformation(
        Mat4::from_translation(vec3(x, base_y, z))
            * Mat4::from_angle_z(degrees(90.0))
            * Mat4::from_nonuniform_scale(height, radius, radius),
    );
    mesh
}

/// Stable per-name body colour, matching the old client's palette + hash.
fn hash_color(name: &str) -> Srgba {
    const PALETTE: [u32; 6] = [0x4caf50, 0x2196f3, 0xff9800, 0xe91e63, 0x9c27b0, 0xffeb3b];
    let mut h: i32 = 0;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as i32);
    }
    rgb(PALETTE[(h.unsigned_abs() as usize) % PALETTE.len()])
}
