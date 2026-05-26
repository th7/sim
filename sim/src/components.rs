//! ECS components. These mirror the Elixir `GameCore.Components.*` so the
//! simulation produces wire-identical state. Stored in a `hecs::World` per
//! realm (see [`crate::world`]).

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

/// Resource kinds (trees today; rock/ore later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Tree,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Tree => "tree",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tree" => Some(ResourceKind::Tree),
            _ => None,
        }
    }
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

/// Item kinds — the *type* of a stackable substance. A quantity of one is an
/// ItemStack (an entry in [`Inventory`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item {
    Wood,
}

impl Item {
    pub fn as_str(self) -> &'static str {
        match self {
            Item::Wood => "wood",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "wood" => Some(Item::Wood),
            _ => None,
        }
    }
}

/// Structure kinds (only the wooden palisade "wall" in v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    Wall,
}

impl StructureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StructureKind::Wall => "wall",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "wall" => Some(StructureKind::Wall),
            _ => None,
        }
    }
}

/// A player-placed Structure anchored to a chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    pub kind: StructureKind,
    pub owner: String,
    pub hp: i64,
}

/// Portal role discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalDirection {
    IntoInstance,
    OutOfInstance,
}

impl PortalDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            PortalDirection::IntoInstance => "into_instance",
            PortalDirection::OutOfInstance => "out_of_instance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalKind {
    Dungeon,
}

impl PortalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PortalKind::Dungeon => "dungeon",
        }
    }
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
