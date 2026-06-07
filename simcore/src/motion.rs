//! Per-tick movement integration — the one place intent becomes displacement.
//!
//! Both the server's tick and the client's Mirror call these; the rounding and
//! clamping here *are* the movement semantics, so any change is automatically a
//! change for both sides.

use crate::collision::{clamp_step, Obstacle};
use protocol::consts::DEFAULT_SPEED;

/// Scale a normalized movement intent (each component in `[-1, 1]`) to a
/// per-second velocity in sub-units/sec.
pub fn intent_velocity(dx: f64, dy: f64) -> (f64, f64) {
    (dx * DEFAULT_SPEED, dy * DEFAULT_SPEED)
}

/// Advance one actor by one tick: scale velocity by `dt` seconds, round to
/// integer sub-units, clamp against `obstacles`. Returns the new position.
pub fn step_actor(
    x: i64,
    y: i64,
    vx: f64,
    vy: f64,
    dt: f64,
    obstacles: &[Obstacle],
) -> (i64, i64) {
    let step_x = (vx * dt).round() as i64;
    let step_y = (vy * dt).round() as i64;
    clamp_step(x, y, step_x, step_y, obstacles)
}
