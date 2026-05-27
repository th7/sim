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
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use three_d::*;

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

fn run_view(cfg: Args, shared: Arc<Mutex<RenderState>>, input_tx: tokio::sync::mpsc::UnboundedSender<Input>) {
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

    let mut camera = Camera::new_perspective(
        window.viewport(),
        vec3(8.0, 16.0, 24.0),
        vec3(8.0, 0.0, 8.0),
        vec3(0.0, 1.0, 0.0),
        degrees(45.0),
        0.1,
        1000.0,
    );
    let sun = DirectionalLight::new(&context, 2.0, Srgba::WHITE, vec3(-0.5, -1.0, -0.3));
    let ambient = AmbientLight::new(&context, 0.5, Srgba::WHITE);

    let mut keys = Keys::default();
    let mut gui = GUI::new(&context);
    let dev = cfg.dev;

    window.render_loop(move |mut frame_input| {
        camera.set_viewport(frame_input.viewport);

        // --- input ---
        let mut moved = false;
        for event in frame_input.events.iter() {
            match event {
                Event::KeyPress { kind, .. } => {
                    moved |= keys.set(*kind, true);
                }
                Event::KeyRelease { kind, .. } => {
                    moved |= keys.set(*kind, false);
                }
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

        // --- build the scene from the latest render state ---
        let rs = shared.lock().unwrap().clone();
        let mut objects: Vec<Gm<Mesh, PhysicalMaterial>> = Vec::new();

        // Players (own = brighter).
        for (name, p) in &rs.players {
            let color = if *name == rs.own { Srgba::new(80, 200, 120, 255) } else { player_color(name) };
            objects.push(box_at(&context, w(p.x), 0.5, w(p.y), 0.6, 1.0, 0.6, color));
        }
        // Resource nodes (trees): live = green, depleted = brown stump.
        for n in rs.nodes.values() {
            let (h, color) = if n.depleted {
                (0.4, Srgba::new(110, 80, 50, 255))
            } else {
                (1.4, Srgba::new(46, 125, 50, 255))
            };
            objects.push(box_at(&context, w(n.x), h / 2.0, w(n.y), 0.5, h, 0.5, color));
        }
        // Structures (walls).
        for s in rs.structures.values() {
            objects.push(box_at(&context, w(s.x), 0.5, w(s.y), 1.0, 1.0, 1.0, Srgba::new(141, 110, 99, 255)));
        }
        // Portals.
        for p in rs.portals.values() {
            let color = if p.direction == "out_of_instance" {
                Srgba::new(255, 112, 67, 255)
            } else {
                Srgba::new(126, 87, 194, 255)
            };
            objects.push(box_at(&context, w(p.x), 0.5, w(p.y), 0.9, 0.2, 0.9, color));
        }

        // Camera follows the player.
        if let Some(me) = rs.players.get(&rs.own) {
            let (tx, tz) = (w(me.x), w(me.y));
            camera.set_view(vec3(tx, 16.0, tz + 16.0), vec3(tx, 0.0, tz), vec3(0.0, 1.0, 0.0));
        }

        // HUD: inventory always; dev overlay with ?dev.
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
                if dev {
                    EWindow::new("dev").anchor(Align2::RIGHT_TOP, [-8.0, 8.0]).show(ctx, |ui| {
                        ui.label(format!("user:   {}", rs.own));
                        let realm = match rs.realm {
                            RealmWire::Overworld => "overworld".to_string(),
                            RealmWire::Instance { id } => format!("instance:{id}"),
                        };
                        ui.label(format!("realm:  {realm}"));
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
            (0.10, 0.06, 0.18)
        } else {
            (0.06, 0.06, 0.07)
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

fn box_at(
    context: &Context,
    x: f32,
    y: f32,
    z: f32,
    sx: f32,
    sy: f32,
    sz: f32,
    color: Srgba,
) -> Gm<Mesh, PhysicalMaterial> {
    let mut mesh = Gm::new(
        Mesh::new(context, &CpuMesh::cube()),
        PhysicalMaterial::new_opaque(
            context,
            &CpuMaterial { albedo: color, ..Default::default() },
        ),
    );
    // CpuMesh::cube spans [-1, 1] (side 2), so halve the scale.
    mesh.set_transformation(
        Mat4::from_translation(vec3(x, y, z)) * Mat4::from_nonuniform_scale(sx / 2.0, sy / 2.0, sz / 2.0),
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

fn player_color(name: &str) -> Srgba {
    const PALETTE: [(u8, u8, u8); 6] = [
        (76, 175, 80),
        (33, 150, 243),
        (255, 152, 0),
        (233, 30, 99),
        (156, 39, 176),
        (255, 235, 59),
    ];
    let h = name.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let (r, g, b) = PALETTE[(h as usize) % PALETTE.len()];
    Srgba::new(r, g, b, 255)
}
