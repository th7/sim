//! Closed-enum catalogues for Structures and Resource yields. Mirrors the
//! Elixir `GameCore.Structure.Catalogue` and the tree→wood yield.

use crate::components::{Footprint, Item, ResourceKind, StructureKind};

/// Build cost of a structure, as `(item, count)` stacks deducted on placement.
pub fn cost(kind: StructureKind) -> &'static [(Item, u32)] {
    match kind {
        StructureKind::Wall => &[(Item::Wood, 5)],
    }
}

/// Starting / maximum HP of a structure.
pub fn max_hp(kind: StructureKind) -> i64 {
    match kind {
        StructureKind::Wall => 100,
    }
}

/// Collision footprint of a structure.
pub fn structure_footprint(kind: StructureKind) -> Footprint {
    match kind {
        StructureKind::Wall => Footprint::Aabb { w: 1_000, h: 1_000 },
    }
}

/// Collision footprint of a resource node.
pub fn resource_footprint(kind: ResourceKind) -> Footprint {
    match kind {
        ResourceKind::Tree => Footprint::Circle { radius: 300 },
    }
}

/// What harvesting a resource node yields.
pub fn resource_yield(kind: ResourceKind) -> Item {
    match kind {
        ResourceKind::Tree => Item::Wood,
    }
}

/// Starting / maximum HP of an NPC.
pub fn npc_max_hp(kind: crate::motivation::NpcKind) -> i64 {
    use crate::motivation::NpcKind;
    match kind {
        NpcKind::Wolf => 80,
        NpcKind::Deer => 50,
    }
}

/// Units of meat a kind's Carcass holds (one per eat/harvest unit).
pub fn carcass_meat(kind: crate::motivation::NpcKind) -> i64 {
    use crate::motivation::NpcKind;
    match kind {
        NpcKind::Deer => 3,
        NpcKind::Wolf => 2,
    }
}
