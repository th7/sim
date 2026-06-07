//! The kind→**Footprint** catalogue — collision shapes derivable from wire data
//! alone (kind + position), so the Mirror reconstructs the authority's exact
//! obstacle set from snapshots with zero extra wire bytes.

use crate::Footprint;
use protocol::types::{ResourceKind, StructureKind};

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
