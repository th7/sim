//! Closed-enum catalogues for Structures and Resource yields. Mirrors the
//! Elixir `GameCore.Structure.Catalogue` and the tree→wood yield.

use crate::components::{Item, StructureKind};

/// Footprints live in `simcore` so the client's Mirror derives the authority's
/// exact obstacle set from kind + position; re-exported for server paths.
pub use simcore::catalogue::{resource_footprint, structure_footprint};

/// Build cost of a structure, as `(item, count)` stacks deducted on placement.
pub fn cost(kind: StructureKind) -> &'static [(Item, u32)] {
    match kind {
        StructureKind::Wall => &[(Item::Wood, crate::consts::WALL_COST)],
    }
}

/// Starting / maximum HP of a structure.
pub fn max_hp(kind: StructureKind) -> i64 {
    match kind {
        StructureKind::Wall => 100,
    }
}

/// What harvesting a resource node yields.
pub fn resource_yield(kind: crate::components::ResourceKind) -> Item {
    match kind {
        crate::components::ResourceKind::Tree => Item::Wood,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wall's wood cost is the shared `consts::WALL_COST`, so the server
    /// catalogue and the client's build gate cannot drift apart.
    #[test]
    fn wall_wood_cost_is_the_shared_constant() {
        let wood: u32 = cost(StructureKind::Wall)
            .iter()
            .filter(|(item, _)| *item == Item::Wood)
            .map(|(_, qty)| *qty)
            .sum();
        assert_eq!(wood, crate::consts::WALL_COST);
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
