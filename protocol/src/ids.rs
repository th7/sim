//! Identity types for the cluster model.

use serde::{Deserialize, Serialize};

/// A dynamic simulation member the Labeler partitions — a Player today, an NPC
/// later. Opaque and stable for the life of the entity. The mapping from
/// `ActorId` to its wire identity (a Player's username) lives in the sim layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActorId(pub u64);

/// A cluster: the single runtime authority over a connected group of actors and
/// the chunks their activity spans. Ids are minted monotonically by the Labeler
/// and survive merges (the survivor keeps the lower id) so worker assignment and
/// observers can track a cluster across topology changes where possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClusterId(pub u64);

/// A worker thread that ticks a set of assigned clusters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkerId(pub u32);

/// The space an actor lives in: the shared persistent Overworld, or one of the
/// ephemeral, numbered Instances. Mirrors the Elixir `realm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Realm {
    Overworld,
    Instance(u64),
}

impl Realm {
    pub fn is_overworld(self) -> bool {
        matches!(self, Realm::Overworld)
    }
}
