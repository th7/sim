//! Interaction-clustered simulation — Rust shared-memory prototype.
//!
//! See `IDEA.md` at the repo root for the model. In brief: one shared ECS
//! world, partitioned by *interaction locality* into **clusters**; clusters
//! are packed onto **workers** for execution; a single **Labeler** owns the
//! partition and serializes topology changes. Actions run inside a cluster
//! (single authority); observation is a separate changed-only delta stream.
//!
//! This crate is feature- and wire-compatible with the Elixir implementation
//! under `apps/`, but its internal structure is entirely different: there are
//! no per-chunk processes and no message handoffs.

pub mod catalogue;
pub mod chunkgraph;
pub mod collision;
pub mod components;
pub mod delta;
pub mod geometry;
pub mod harness;
pub mod ids;
pub mod labeler;
pub mod repack;
pub mod sim;
pub mod wire;
pub mod world;
pub mod worldgen;

/// Game-wide constants, matching the Elixir implementation's values exactly.
pub mod consts {
    /// Tick period in milliseconds (20 Hz internal simulation).
    pub const TICK_MS: u64 = 50;
    /// Snapshots broadcast every Nth tick (10 Hz observation).
    pub const BROADCAST_EVERY: u64 = 2;
    /// Default player speed: 4 world units/sec = 4000 sub-units/sec.
    pub const DEFAULT_SPEED: f64 = 4_000.0;
    /// Periodic persistence flush cadence.
    pub const FLUSH_MS: u64 = 5_000;
    /// Resource-node respawn delay after harvest.
    pub const RESPAWN_MS: u64 = 30_000;
    /// Interact range squared, in sub-units² (1.0 world unit, squared).
    pub const INTERACT_RANGE_SQ: i64 = 1_000 * 1_000;
    /// Portal-overlap trigger range squared (0.5 world units squared).
    pub const PORTAL_OVERLAP_RANGE_SQ: i64 = 250_000;
    /// Player body-circle radius, in sub-units.
    pub const PLAYER_BODY_RADIUS: i64 = 300;
    /// HP removed per damage click.
    pub const DAMAGE_PER_CLICK: i64 = 25;
    /// Chunk idle-deactivation timeout.
    pub const IDLE_TIMEOUT_MS: u64 = 5_000;
}
