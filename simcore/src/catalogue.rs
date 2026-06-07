//! The kind→**Footprint** catalogue — collision shapes derivable from wire data
//! alone (kind + position), so the Mirror reconstructs the authority's exact
//! obstacle set from snapshots with zero extra wire bytes. Per-kind NPC max-hp
//! lives here for the same reason: the client bands Health from `(hp, kind)`
//! without the wire carrying the denominator.

use crate::Footprint;
use protocol::types::{NpcKind, ResourceKind, StructureKind};

/// Collision footprint of a structure.
pub fn structure_footprint(kind: StructureKind) -> Footprint {
    match kind {
        StructureKind::Wall => Footprint::Aabb { w: 1_000, h: 1_000 },
    }
}

/// Collision footprint of a resource node. Identical gatherable or depleted —
/// harvesting never opens a path.
pub fn resource_footprint(kind: ResourceKind) -> Footprint {
    match kind {
        ResourceKind::Tree => Footprint::Circle { radius: 300 },
    }
}

/// Starting / maximum HP of an NPC — static per kind (Region temperament
/// modulates Drives, never toughness).
pub fn npc_max_hp(kind: NpcKind) -> i64 {
    match kind {
        NpcKind::Wolf => 80,
        NpcKind::Deer => 50,
    }
}
