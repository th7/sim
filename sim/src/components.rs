//! ECS components for the server's `hecs::World` per realm (see [`crate::world`]).
//! The shared game *kinds* (Item, ResourceKind, StructureKind, PortalKind,
//! PortalDirection) live in the `protocol` crate and are re-exported here so the
//! server's `crate::components::Item` etc. paths keep working; the components
//! that carry ECS/collision data stay server-side.

pub use protocol::types::{Item, PortalDirection, PortalKind, ResourceKind, StructureKind};

use crate::geometry::{coord_for, ChunkCoord};
use crate::ids::ActorId;
use std::collections::BTreeMap;

/// World-space position in **sub-units** (1 world unit = 1000 sub-units).
/// Integer everywhere on the server; the wire boundary divides by 1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: i64,
    pub y: i64,
}

impl Position {
    pub fn chunk(self) -> ChunkCoord {
        coord_for(self.x, self.y)
    }
}

/// Per-second velocity in sub-units/sec. Floats are fine: velocity is recomputed
/// from intent each `set_intent`, never accumulated, so Position (integer) does
/// not drift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity {
    pub vx: f64,
    pub vy: f64,
}

/// The shape an obstacle occupies for one-way movement collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Footprint {
    /// Circle of `radius` centered at the entity's Position.
    Circle { radius: i64 },
    /// Axis-aligned rectangle of full width × height centered at Position.
    Aabb { w: i64, h: i64 },
}

/// A gatherable Resource node (harvestable now). Mutually exclusive with
/// [`Depleted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gatherable {
    pub kind: ResourceKind,
    pub yields: Item,
}

/// A harvested Resource node awaiting respawn. `respawn_at_ms` is sim-clock
/// time. Mutually exclusive with [`Gatherable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Depleted {
    pub kind: ResourceKind,
    pub respawn_at_ms: u64,
}

/// A player-placed Structure anchored to a chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    pub kind: StructureKind,
    pub owner: String,
    pub hp: i64,
}

/// A worldgen-placed Overworld→Instance entry or Instance→Overworld exit point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Portal {
    pub kind: PortalKind,
    pub direction: PortalDirection,
}

/// Marker for an entity driven by a human Player, tagged with the Labeler actor
/// id so the cluster topology can track it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerControlled {
    pub actor: ActorId,
}

/// ItemStacks a Player carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub items: BTreeMap<Item, u32>,
}

/// The wire-facing identifier of an entity: a Player's username, or a derived
/// id like `tree:x:y` / `structure:x:y` / `portal:dungeon:x:y`. This is the key
/// the snapshot/delta wire format uses.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WireId(pub String);

/// Marker: include this entity in client-visible snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Renderable;

/// An NPC entity, tagged with its [`NpcKind`] and the Labeler actor id so the
/// cluster topology can track it (the same role `PlayerControlled.actor` plays
/// for Players). Its per-tick Intent is produced by [`crate::motivation`], not a
/// session. NPCs are actors but do not anchor the Warm set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Npc {
    pub kind: crate::motivation::NpcKind,
    pub actor: ActorId,
}

/// Hit points for a damageable actor (NPCs; Structures keep their own `hp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub hp: i64,
    pub max: i64,
}

/// The perishable remains of a killed animal — a Carcass. Holds the
/// remaining meat to eat/harvest and the sim-clock time it rots away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Carcass {
    pub meat: i64,
    pub perish_at_ms: u64,
}

/// Recent-damage memory on an actor: the sim-clock of the last hit and the actor
/// id that dealt it. Read by Motivation to spike the safety Need and target the
/// attacker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hurt {
    pub last_ms: u64,
    pub by: u64,
}

/// The sim-clock time an NPC may next act (attack or eat) — its action cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActReady {
    pub at_ms: u64,
}

/// Alarm state (agent extension): set while an animal is fleeing, so nearby herd
/// peers catch the panic and flee too (a stampede ripples outward). Cleared once
/// `until_ms` passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alarmed {
    pub until_ms: u64,
}

/// The Decision the Motivation phase committed to this tick, stored so the
/// post-movement resolution phase can apply its verb (attack/eat) in range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NpcDecision(pub crate::motivation::Decision);
