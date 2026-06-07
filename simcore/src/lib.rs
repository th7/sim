//! The shared simulation core — the *same-code promise* made structural.
//!
//! Movement integration, collision, and the kind→Footprint catalogue live here,
//! consumed by both the server (`sim`) and the client's **Mirror**. The Mirror
//! speculates with the authority's own code, never a port of it: exact replay
//! (client speculation ≡ server integration, bit-identical) holds because there
//! is exactly one implementation to agree with.
//!
//! Scope is deliberately the Mirror's scope — **continuous state only**.
//! Discrete events (spawns, yields, depletion, placement) are decided by the
//! Island and stay in `sim`.

pub mod catalogue;
pub mod collision;
pub mod motion;

/// The shape an obstacle occupies for one-way movement collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Footprint {
    /// Circle of `radius` centered at the entity's Position.
    Circle { radius: i64 },
    /// Axis-aligned rectangle of full width × height centered at Position.
    Aabb { w: i64, h: i64 },
}
