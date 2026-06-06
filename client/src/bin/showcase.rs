//! Showcase: renders every UI element in every appearance-affecting state for
//! manual visual verification on a real display. Scenes are synthetic wire
//! payloads fed through the real `ClientModel` → `View` pipeline (see
//! `client::showcase`); presence is machine-checked in `tests/showcase.rs`, so
//! the only judgement left here is whether things *look* right.
//!
//! Keys: Space cycles scenarios; Tab toggles the dev panel.
//!
//! NOTE: requires a display/GL context to run, like the game bin.

use client::render::View;
use client::showcase::scenarios;
use protocol::geometry::ChunkCoord;
use three_d::*;

fn main() {
    // Same headless guard as the game bin: winit panics without a backend.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("showcase: no display (DISPLAY/WAYLAND_DISPLAY unset) — cannot open a window; exiting.");
        return;
    }

    let window = match Window::new(WindowSettings {
        title: "sim — showcase".to_string(),
        max_size: Some((1280, 800)),
        ..Default::default()
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("showcase: cannot open a window (no display?): {e}");
            return;
        }
    };
    let context = window.gl();

    let scenes = scenarios();
    let mut idx = 0;
    let mut view = View::new(&context, window.viewport(), ChunkCoord::new(0, 0));
    let mut dev_view = true;

    window.render_loop(move |mut frame_input| {
        view.set_viewport(frame_input.viewport);

        for event in frame_input.events.iter() {
            match event {
                Event::KeyPress { kind: Key::Space, .. } => idx = (idx + 1) % scenes.len(),
                Event::KeyPress { kind: Key::Tab, .. } => dev_view = !dev_view,
                _ => {}
            }
        }

        let scene = &scenes[idx];
        let rs = scene.state(frame_input.accumulated_time);
        let name = scene.name;
        view.frame(&context, &mut frame_input, &rs, dev_view, |ctx| legend(ctx, name));
        FrameOutput::default()
    });
}

/// The legend window: which scenario is up, the keys, and the grid layout —
/// self-documenting at the display, where the source isn't open. The row list
/// mirrors the `overworld` builder's layout, top row first.
fn legend(ctx: &three_d::egui::Context, scenario: &str) {
    use three_d::egui::{Align2, Window as EWindow};
    EWindow::new("showcase").anchor(Align2::LEFT_BOTTOM, [8.0, -8.0]).show(ctx, |ui| {
        ui.label(format!("scenario: {scenario}"));
        ui.label("Space: next scenario   Tab: dev panel");
        ui.separator();
        ui.label("row 1: tree — live / depleted");
        ui.label("row 2: wall / carcass");
        ui.label("row 3: portal — into / out of instance");
        ui.label("row 4: wolf / deer");
        ui.label("row 5: players (six palette colours)");
        ui.label("row 6: pacing player (lerp)");
    });
}
