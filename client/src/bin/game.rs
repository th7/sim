//! Native game client. Runs the WebSocket session on a background
//! tokio thread and the three-d render loop on the main thread; they communicate
//! through a shared [`RenderState`] and an input channel.
//!
//! NOTE: requires a display/GL context to run. In a headless environment it
//! builds but cannot open a window — the manual visual pass (the rendering half
//! of the parity bar) must be done on a machine with a display.

use client::render::View;
use client::session::{Input, RenderState, Session};
use protocol::geometry::ChunkCoord;
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
        npcs: BTreeMap::new(),
        carcasses: BTreeMap::new(),
        inventory: BTreeMap::new(),
        stats: None,
        last_error: None,
        target: None, // the Target is born empty, like the Mirror
        action_button: client::model::ActionButton::Inert,
        frozen: true, // born frozen — until the first authoritative snapshot
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

    let mut view = View::new(&context, window.viewport(), cfg.chunk);
    let mut keys = Keys::default();
    let mut dev_view = cfg.dev;

    window.render_loop(move |mut frame_input| {
        view.set_viewport(frame_input.viewport);

        // --- input ---
        let mut moved = false;
        for event in frame_input.events.iter() {
            match event {
                Event::KeyPress { kind: Key::Tab, .. } => {
                    dev_view = !dev_view;
                    let _ = input_tx.send(Input::ToggleDev);
                }
                // The Action button's key: act on the current Target.
                Event::KeyPress { kind: Key::E, .. } => {
                    let _ = input_tx.send(Input::PressAction);
                }
                // Escape: the explicit Target clear.
                Event::KeyPress { kind: Key::Escape, .. } => {
                    let _ = input_tx.send(Input::Escape);
                }
                Event::KeyPress { kind, .. } => moved |= keys.set(*kind, true),
                Event::KeyRelease { kind, .. } => moved |= keys.set(*kind, false),
                Event::MousePress { button: MouseButton::Left, position, .. } => {
                    if let Some((wx, wy)) = ground_pick(view.camera(), *position) {
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
        // The HUD Action button is the same button as `E`.
        if view.frame(&context, &mut frame_input, &rs, dev_view, |_| {}) {
            let _ = input_tx.send(Input::PressAction);
        }
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
