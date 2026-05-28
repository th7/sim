//! Native game client (ADR-0003). Runs the WebSocket session on a background
//! tokio thread and the three-d render loop on the main thread; they communicate
//! through a shared [`RenderState`] and an input channel.
//!
//! NOTE: requires a display/GL context to run. In a headless environment it
//! builds but cannot open a window — the manual visual pass (the rendering half
//! of ADR-0003's parity bar) must be done on a machine with a display.

use client::session::{Input, RenderState, Session};
use protocol::geometry::{ChunkCoord, SUB_UNITS_PER_UNIT};
use protocol::wire::RealmWire;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use three_d::*;

/// World units per chunk edge (matches the server's `CHUNK_SIZE`).
const CHUNK_SIZE: f32 = 16.0;
/// Snapshots arrive at ~10 Hz; lerp toward each new target over this window so
/// motion stays smooth without client-side prediction (mirrors the old client).
const SNAPSHOT_INTERVAL_MS: f64 = 100.0;
/// Keep rendering a player this long after they vanish from snapshots, so a
/// chunk-boundary crossing (briefly in no snapshot) doesn't blink the cube out.
const PLAYER_REMOVE_GRACE_MS: f64 = 400.0;
/// Height of the dev chunk-lifecycle overlay above the ground plane (whose top
/// is y=0). The overlay is a flat transparent decal; it must clear the depth
/// buffer's resolution at the far edge of the 7×7 grid (~100 units from the
/// camera) or it z-fights the ground and flickers green as the camera moves. The
/// float is imperceptible on the featureless ground.
const DEV_OVERLAY_Y: f32 = 0.08;

fn main() {
    let cfg = Args::parse();
    eprintln!("client: connecting to {} as {}", cfg.server, cfg.username);

    let shared = Arc::new(Mutex::new(RenderState {
        own: cfg.username.clone(),
        realm: RealmWire::Overworld,
        window_center: cfg.chunk,
        players: BTreeMap::new(),
        nodes: BTreeMap::new(),
        structures: BTreeMap::new(),
        portals: BTreeMap::new(),
        inventory: BTreeMap::new(),
        stats: None,
    }));
    let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<Input>();

    // WebSocket session on a background tokio runtime.
    {
        let shared = shared.clone();
        let (server, username, chunk) = (cfg.server.clone(), cfg.username.clone(), cfg.chunk);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            rt.block_on(async move {
                match Session::connect(&server, &username, chunk).await {
                    Ok(session) => session.run(input_rx, shared).await,
                    Err(e) => eprintln!("client: connection failed: {e}"),
                }
            });
        });
    }

    // Start in dev mode if requested (the session joins dev:stats on toggle).
    if cfg.dev {
        let _ = input_tx.send(Input::ToggleDev);
    }

    run_view(cfg, shared, input_tx);
}

struct Args {
    username: String,
    server: String,
    chunk: ChunkCoord,
    dev: bool,
}

impl Args {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let val = |flag: &str| {
            args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
        };
        let chunk = val("--chunk")
            .and_then(|s| {
                let (a, b) = s.split_once(':')?;
                Some(ChunkCoord::new(a.parse().ok()?, b.parse().ok()?))
            })
            .unwrap_or(ChunkCoord::new(0, 0));
        Args {
            username: val("--user").unwrap_or_else(|| format!("player-{}", std::process::id())),
            server: val("--server")
                .unwrap_or_else(|| "ws://localhost:4000/socket/websocket?vsn=2.0.0".to_string()),
            chunk,
            dev: args.iter().any(|a| a == "--dev"),
        }
    }
}

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

fn run_view(cfg: Args, shared: Arc<Mutex<RenderState>>, input_tx: tokio::sync::mpsc::UnboundedSender<Input>) {
    // On Linux, winit's `EventLoop::new` *panics* (rather than returning an
    // error) when there's no X11/Wayland backend, so the `Window::new` error
    // arm below can't catch a headless box — bail early on the env vars instead.
    // macOS/Windows use Cocoa/Win32 (no DISPLAY/WAYLAND_DISPLAY), so this guard
    // is Linux-only; elsewhere we let `Window::new` proceed.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("client: no display (DISPLAY/WAYLAND_DISPLAY unset) — cannot open a window; exiting.");
        return;
    }

    let window = match Window::new(WindowSettings {
        title: format!("sim — {}", cfg.username),
        max_size: Some((1280, 800)),
        ..Default::default()
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("client: cannot open a window (no display?): {e}");
            return;
        }
    };
    let context = window.gl();

    // Fixed isometric offset; the camera re-frames the local player every frame.
    let cam_offset = vec3(12.0, 12.0, 12.0);
    let mut cam_target = vec3(cfg.chunk.cx as f32 * CHUNK_SIZE, 0.0, cfg.chunk.cy as f32 * CHUNK_SIZE);
    let mut camera = Camera::new_perspective(
        window.viewport(),
        cam_target + cam_offset,
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
    let sun = DirectionalLight::new(&context, 2.8, Srgba::WHITE, vec3(0.5, -0.83, 0.25));
    let ambient = AmbientLight::new(&context, 1.0, Srgba::WHITE);

    let mut keys = Keys::default();
    let mut gui = GUI::new(&context);
    let mut dev_view = cfg.dev;
    let mut lerps: HashMap<String, Lerp> = HashMap::new();
    let mut last_realm = RealmWire::Overworld;

    window.render_loop(move |mut frame_input| {
        camera.set_viewport(frame_input.viewport);
        let now = frame_input.accumulated_time;

        // --- input ---
        let mut moved = false;
        for event in frame_input.events.iter() {
            match event {
                Event::KeyPress { kind: Key::Tab, .. } => {
                    dev_view = !dev_view;
                    let _ = input_tx.send(Input::ToggleDev);
                }
                Event::KeyPress { kind, .. } => moved |= keys.set(*kind, true),
                Event::KeyRelease { kind, .. } => moved |= keys.set(*kind, false),
                Event::MousePress { button: MouseButton::Left, position, .. } => {
                    if let Some((wx, wy)) = ground_pick(&camera, *position) {
                        let _ = input_tx.send(Input::Click { wx, wy });
                    }
                }
                _ => {}
            }
        }
        if moved {
            let _ = input_tx.send(Input::Movement {
                north: keys.w,
                south: keys.s,
                east: keys.d,
                west: keys.a,
            });
        }

        let rs = shared.lock().unwrap().clone();

        // --- player interpolation + removal grace ---
        // A realm switch teleports the player; clear lerp state so the cube
        // doesn't slide across the jump (mirrors clearAllChunkSubscriptions).
        if rs.realm != last_realm {
            lerps.clear();
            last_realm = rs.realm;
        }
        for (name, p) in &rs.players {
            let target = (w(p.x), w(p.y));
            match lerps.get_mut(name) {
                None => {
                    lerps.insert(
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
        lerps.retain(|_, l| now - l.last_seen < PLAYER_REMOVE_GRACE_MS);

        // --- build the scene ---
        let mut objects: Vec<Gm<Mesh, PhysicalMaterial>> = Vec::new();

        // Ground plane.
        objects.push(box_at(&context, 0.0, -0.01, 0.0, 4000.0, 0.02, 4000.0, rgb(0x3a3a3a)));

        // Players (body + head), at the interpolated position.
        for (name, l) in &lerps {
            let (x, z) = l.visible(now);
            objects.push(box_at(&context, x, 0.5, z, 0.6, 1.0, 0.6, hash_color(name)));
            objects.push(box_at(&context, x, 1.25, z, 0.5, 0.5, 0.5, rgb(0xeac9a0)));
        }
        // Resource nodes: trunk always; conical foliage only when not depleted.
        for n in rs.nodes.values() {
            let (x, z) = (w(n.x), w(n.y));
            objects.push(cylinder_at(&context, x, 0.0, z, 0.16, 0.6, rgb(0x6d4c41)));
            if !n.depleted {
                objects.push(cone_at(&context, x, 0.5, z, 0.55, 0.7, rgb(0x2e7d32)));
                objects.push(cone_at(&context, x, 1.0, z, 0.40, 0.55, rgb(0x2e7d32)));
            }
        }
        // Structures (walls): three planks along x.
        const PLANK_COLORS: [u32; 3] = [0x8d6e63, 0x795548, 0x6d4c41];
        for s in rs.structures.values() {
            let (x, z) = (w(s.x), w(s.y));
            for (i, c) in PLANK_COLORS.iter().enumerate() {
                let px = x + (i as f32 - 1.0) * 0.29; // pitch 0.28 + 0.01 gap
                objects.push(box_at(&context, px, 0.5, z, 0.28, 1.0, 0.9, rgb(*c)));
            }
        }
        // Portals: a flat disc, coloured by direction (the torus ring the old
        // client drew has no three-d primitive — omitted, see port notes).
        for p in rs.portals.values() {
            let color = if p.direction == "into_instance" { rgb(0x7e57c2) } else { rgb(0xff7043) };
            objects.push(cylinder_at(&context, w(p.x), 0.0, w(p.y), 0.65, 0.04, color));
        }

        // Dev chunk-lifecycle overlay (transparent, drawn after opaque geometry).
        if let Some(stats) = &rs.stats {
            for e in &stats.around {
                let x0 = e.cx as f32 * CHUNK_SIZE;
                let z0 = e.cy as f32 * CHUNK_SIZE;
                let (fill, alpha) = match e.lifecycle.as_str() {
                    "hot" => (0x244d24, 64),
                    "idle_armed" => (0x6e5a1f, 64),
                    _ => (0x222222, 13), // cold
                };
                objects.push(flat_quad(
                    &context,
                    x0 + CHUNK_SIZE / 2.0,
                    DEV_OVERLAY_Y,
                    z0 + CHUNK_SIZE / 2.0,
                    CHUNK_SIZE,
                    CHUNK_SIZE,
                    rgba(fill, alpha),
                ));
                // Shrinking idle countdown bar, just above the tile overlay.
                if e.lifecycle == "idle_armed" {
                    if let Some(rem) = e.idle_ms_remaining {
                        let frac = (rem as f32 / 5000.0).clamp(0.0, 1.0);
                        objects.push(flat_quad(
                            &context,
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

        // Camera follows the local player's interpolated position.
        if let Some(l) = lerps.get(&rs.own) {
            let (x, z) = l.visible(now);
            cam_target = vec3(x, 0.0, z);
        }
        camera.set_view(cam_target + cam_offset, cam_target, vec3(0.0, 1.0, 0.0));

        // HUD: inventory always; dev panel (user/realm/pos/chunk/view/active/total) with dev mode.
        gui.update(
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
                });
                if dev_view {
                    EWindow::new("dev").anchor(Align2::RIGHT_TOP, [-8.0, 8.0]).show(ctx, |ui| {
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
                                    format!(
                                        "({}, {})",
                                        (px / CHUNK_SIZE).floor() as i32,
                                        (py / CHUNK_SIZE).floor() as i32
                                    ),
                                )
                            }
                            None => ("—".to_string(), "—".to_string()),
                        };
                        ui.label(format!("pos:    {pos}  chunk: {chunk}"));
                        let (active, total) = rs
                            .stats
                            .as_ref()
                            .map(|s| (s.active_chunks, s.total_players))
                            .unwrap_or((0, 0));
                        ui.label(format!("view: {}  active: {}  total: {}", rs.players.len(), active, total));
                    });
                }
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
            .render(&camera, objects.iter(), &[&sun, &ambient])
            .write(|| gui.render())
            .unwrap();

        FrameOutput::default()
    });
}

/// Tracked WASD state; `set` returns true if the state changed.
#[derive(Default)]
struct Keys {
    w: bool,
    a: bool,
    s: bool,
    d: bool,
}

impl Keys {
    fn set(&mut self, key: Key, down: bool) -> bool {
        let slot = match key {
            Key::W => &mut self.w,
            Key::A => &mut self.a,
            Key::S => &mut self.s,
            Key::D => &mut self.d,
            _ => return false,
        };
        if *slot == down {
            return false;
        }
        *slot = down;
        true
    }
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

/// Unproject a screen pixel onto the ground plane (y=0); returns world (x, z).
fn ground_pick(camera: &Camera, pixel: PhysicalPoint) -> Option<(f64, f64)> {
    let pos = camera.position();
    let dir = camera.view_direction_at_pixel(pixel);
    if dir.y.abs() < 1e-6 {
        return None;
    }
    let t = -pos.y / dir.y; // intersect y = 0
    if t < 0.0 {
        return None;
    }
    let hit = pos + dir * t;
    Some((hit.x as f64, hit.z as f64))
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
