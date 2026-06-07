//! Game-wide constants, matching the Elixir implementation's values exactly.
//! Shared so the client and server agree on tick rate, ranges, and conversions.

/// Tick period in milliseconds (20 Hz internal simulation).
pub const TICK_MS: u64 = 50;
/// Snapshots broadcast every Nth tick (10 Hz observation).
pub const BROADCAST_EVERY: u64 = 2;
/// Default player speed: 4 world units/sec = 4000 sub-units/sec.
pub const DEFAULT_SPEED: f64 = 4_000.0;
/// Periodic player heartbeat cadence (re-upsert live positions).
pub const FLUSH_MS: u64 = 5_000;
/// Datastore flush-to-durable cadence (mirrors the Elixir Datastore's 1s).
pub const DB_FLUSH_MS: u64 = 1_000;
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
/// Wood cost to build a wall — the single source for the server catalogue and
/// the client's build-affordability gate.
pub const WALL_COST: u32 = 5;
/// Intent is perishable: when a player's input frames stop arriving, the last
/// Intent holds for this many ticks (absorbing network jitter), then expires
/// to zero — the player stands still rather than walking on stale Intent.
pub const INTENT_GRACE_TICKS: u64 = 3;
/// The Mirror's Lead bound: how many ticks the client's speculative simulation
/// may run ahead of the last authoritative tick. One shared constant — a
/// structural promise (lead ≤ bound, else whole-Mirror freeze), not a
/// per-connection tuning. Sized to support ~400ms RTT comfortably at 20 Hz.
pub const LEAD_BOUND_TICKS: u64 = 10;
