//! Movement collision — axis-decomposed clamping of a body circle's step against
//! obstacle **Footprint**s, plus the build-time `aabb_blocked` predicate.
//!
//! A direct port of the Elixir `GameCore.Collision`, kept storage-agnostic
//! (operates on obstacle slices, not the ECS) so it is pure and unit-testable.
//! Per-axis stop gives free slide along axis-aligned walls; a body currently
//! overlapping any footprint is grandfathered (moves freely until clear) so
//! spawn-on-obstacle never sticks.
//!
//! Unlike the Elixir per-chunk world, a cluster owns its actors' full 3×3
//! footprint, so collision sees obstacles in neighbouring chunks too — this is
//! the intended resolution of the old chunk-boundary "clip-and-stop" artifact.
//! For obstacles away from boundaries the result is identical.

use crate::components::Footprint;
use crate::consts::PLAYER_BODY_RADIUS;

/// An obstacle: a footprint anchored at a sub-unit position.
#[derive(Debug, Clone, Copy)]
pub struct Obstacle {
    pub x: i64,
    pub y: i64,
    pub footprint: Footprint,
}

/// Clamp a body-circle step from `(cx, cy)` by `(dx, dy)` against `obstacles`,
/// returning the new position. The body radius is [`PLAYER_BODY_RADIUS`].
pub fn clamp_step(cx: i64, cy: i64, dx: i64, dy: i64, obstacles: &[Obstacle]) -> (i64, i64) {
    let r = PLAYER_BODY_RADIUS;

    // Grandfather: if already overlapping any footprint, move freely.
    if obstacles.iter().any(|o| overlaps(o, cx, cy, r)) {
        return (cx + dx, cy + dy);
    }

    let clamped_dx = obstacles
        .iter()
        .fold(dx, |step, o| limit_axis(o, cx, cy, r, step, Axis::X));
    let new_x = cx + clamped_dx;

    let clamped_dy = obstacles
        .iter()
        .fold(dy, |step, o| limit_axis(o, new_x, cy, r, step, Axis::Y));
    let new_y = cy + clamped_dy;

    (new_x, new_y)
}

/// Build-time predicate: would a footprint of `w×h` placed at `(x, y)` overlap
/// any existing footprint, or any player body (player positions in
/// `player_positions`)? Players carry a body circle of [`PLAYER_BODY_RADIUS`].
pub fn aabb_blocked(
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    obstacles: &[Obstacle],
    player_positions: &[(i64, i64)],
) -> bool {
    let vs_footprints = obstacles.iter().any(|o| match o.footprint {
        Footprint::Aabb { w: ow, h: oh } => aabb_aabb_overlap(x, y, w, h, o.x, o.y, ow, oh),
        Footprint::Circle { radius } => aabb_circle_overlap(x, y, w, h, o.x, o.y, radius),
    });
    let vs_players = player_positions
        .iter()
        .any(|&(px, py)| aabb_circle_overlap(x, y, w, h, px, py, PLAYER_BODY_RADIUS));
    vs_footprints || vs_players
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

fn overlaps(o: &Obstacle, cx: i64, cy: i64, r: i64) -> bool {
    match o.footprint {
        Footprint::Aabb { w, h } => aabb_circle_overlap(o.x, o.y, w, h, cx, cy, r),
        Footprint::Circle { radius } => {
            let ddx = cx - o.x;
            let ddy = cy - o.y;
            let rsum = r + radius;
            ddx * ddx + ddy * ddy < rsum * rsum
        }
    }
}

fn limit_axis(o: &Obstacle, cx: i64, cy: i64, r: i64, step: i64, axis: Axis) -> i64 {
    match o.footprint {
        Footprint::Aabb { w, h } => {
            let half_w = w / 2;
            let half_h = h / 2;
            let ax_min = o.x - half_w;
            let ax_max = o.x + half_w;
            let ay_min = o.y - half_h;
            let ay_max = o.y + half_h;
            match axis {
                Axis::X => {
                    if cy + r > ay_min && cy - r < ay_max {
                        limit_linear(step, cx, ax_min, ax_max, r)
                    } else {
                        step
                    }
                }
                Axis::Y => {
                    if cx + r > ax_min && cx - r < ax_max {
                        limit_linear(step, cy, ay_min, ay_max, r)
                    } else {
                        step
                    }
                }
            }
        }
        Footprint::Circle { radius } => {
            let rsum = r + radius;
            let rsum2 = rsum * rsum;
            let (center, obstacle_center, perp2) = match axis {
                Axis::X => (cx, o.x, (cy - o.y) * (cy - o.y)),
                Axis::Y => (cy, o.y, (cx - o.x) * (cx - o.x)),
            };
            if perp2 < rsum2 {
                let sqrt_term = ((rsum2 - perp2) as f64).sqrt();
                let root1 = (obstacle_center - center) as f64 - sqrt_term;
                let root2 = (obstacle_center - center) as f64 + sqrt_term;
                if step > 0 && root1 >= 0.0 {
                    step.min(root1.floor() as i64)
                } else if step < 0 && root2 <= 0.0 {
                    step.max(root2.ceil() as i64)
                } else {
                    step
                }
            } else {
                step
            }
        }
    }
}

fn limit_linear(step: i64, center: i64, lo: i64, hi: i64, r: i64) -> i64 {
    if step > 0 && lo > center {
        step.min(lo - r - center)
    } else if step < 0 && hi < center {
        step.max(hi + r - center)
    } else {
        step
    }
}

/// Rect (centered at `(x,y)`, full `w×h`) vs circle (center `(cx,cy)`, radius `r`).
fn aabb_circle_overlap(x: i64, y: i64, w: i64, h: i64, cx: i64, cy: i64, r: i64) -> bool {
    let half_w = w / 2;
    let half_h = h / 2;
    let nearest_x = cx.max(x - half_w).min(x + half_w);
    let nearest_y = cy.max(y - half_h).min(y + half_h);
    let ddx = cx - nearest_x;
    let ddy = cy - nearest_y;
    ddx * ddx + ddy * ddy < r * r
}

/// Rect vs rect, both centered, full extents.
fn aabb_aabb_overlap(x: i64, y: i64, w: i64, h: i64, ox: i64, oy: i64, ow: i64, oh: i64) -> bool {
    (x - ox).abs() * 2 < w + ow && (y - oy).abs() * 2 < h + oh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(x: i64, y: i64) -> Obstacle {
        Obstacle { x, y, footprint: Footprint::Aabb { w: 1_000, h: 1_000 } }
    }
    fn tree(x: i64, y: i64) -> Obstacle {
        Obstacle { x, y, footprint: Footprint::Circle { radius: 300 } }
    }

    #[test]
    fn no_obstacles_moves_freely() {
        assert_eq!(clamp_step(0, 0, 100, 50, &[]), (100, 50));
    }

    #[test]
    fn wall_stops_eastward_step_at_contact() {
        // Player at x=0, wall centered at x=2000 (half_w=500 → left face 1500).
        // Body radius 300 → contact when player center reaches 1500-300=1200.
        let obs = [wall(2_000, 0)];
        let (nx, _ny) = clamp_step(0, 0, 5_000, 0, &obs);
        assert_eq!(nx, 1_200, "stops just at the wall face minus body radius");
    }

    #[test]
    fn wall_allows_slide_along_y() {
        // Moving purely north past a wall to the east: no x-block, y free.
        let obs = [wall(2_000, 0)];
        let (nx, ny) = clamp_step(1_200, 0, 0, 400, &obs);
        assert_eq!((nx, ny), (1_200, 400));
    }

    #[test]
    fn grandfathered_when_already_overlapping() {
        // Player spawned inside a tree footprint moves freely until clear.
        let obs = [tree(0, 0)];
        assert_eq!(clamp_step(0, 0, 100, 0, &obs), (100, 0));
    }

    #[test]
    fn circle_blocks_approach() {
        // Approaching a tree (r=300) from the west; body r=300 → rsum=600.
        // Aligned on y, contact at center distance 600 → stop at x = -600 ... here
        // tree at x=1000, player from x=0 moving +x: contact at 1000-600=400.
        let obs = [tree(1_000, 0)];
        let (nx, _) = clamp_step(0, 0, 5_000, 0, &obs);
        assert_eq!(nx, 400);
    }

    #[test]
    fn build_blocked_by_existing_footprint() {
        let obs = [wall(0, 0)];
        // Overlapping wall placement is blocked.
        assert!(aabb_blocked(500, 0, 1_000, 1_000, &obs, &[]));
        // Far enough apart is clear.
        assert!(!aabb_blocked(2_000, 0, 1_000, 1_000, &obs, &[]));
    }

    #[test]
    fn build_blocked_by_player_body() {
        assert!(aabb_blocked(0, 0, 1_000, 1_000, &[], &[(200, 0)]));
        assert!(!aabb_blocked(0, 0, 1_000, 1_000, &[], &[(2_000, 0)]));
    }
}
