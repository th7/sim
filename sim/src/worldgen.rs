//! Deterministic, compile-time placement of Resource nodes and Portals per
//! chunk. Mirrors the Elixir `GameCore.Worldgen` and `GameCore.InstanceWorldgen`
//! exactly — same offsets, same coordinates — so the world is byte-identical.

use crate::components::{PortalDirection, PortalKind, ResourceKind};
use crate::geometry::{chunk_center, ChunkCoord, CHUNK_SIZE};

/// A worldgen-placed resource node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeSpec {
    pub kind: ResourceKind,
    pub x: i64,
    pub y: i64,
}

/// A worldgen-placed portal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalSpec {
    pub kind: PortalKind,
    pub direction: PortalDirection,
    pub x: i64,
    pub y: i64,
}

/// Tree offsets from chunk-center, in sub-units. Tight cluster so a player
/// spawning at chunk-center has a tree inside interact range.
const TREE_OFFSETS: [(i64, i64); 5] = [
    (500, 500),
    (500, -500),
    (-500, 500),
    (-500, -500),
    (0, 0),
];

/// Overworld Resource nodes for `coord`: five trees around chunk-center.
pub fn resource_nodes(coord: ChunkCoord) -> Vec<NodeSpec> {
    let (cx, cy) = chunk_center(coord);
    TREE_OFFSETS
        .iter()
        .map(|&(dx, dy)| NodeSpec { kind: ResourceKind::Tree, x: cx + dx, y: cy + dy })
        .collect()
}

/// Overworld Portals for `coord`: one `:into_instance` dungeon portal in chunk
/// `{0,0}` at a quarter-offset from the origin, `[]` elsewhere.
pub fn portals(coord: ChunkCoord) -> Vec<PortalSpec> {
    if coord == ChunkCoord::new(0, 0) {
        let quarter = CHUNK_SIZE / 4;
        vec![PortalSpec {
            kind: PortalKind::Dungeon,
            direction: PortalDirection::IntoInstance,
            x: quarter,
            y: quarter,
        }]
    } else {
        Vec::new()
    }
}

/// Instance-local Portals for `coord`: one `:out_of_instance` return portal at
/// the center of chunk `{1,1}` (middle of the 3×3 grid), `[]` elsewhere.
pub fn instance_portals(coord: ChunkCoord) -> Vec<PortalSpec> {
    if coord == ChunkCoord::new(1, 1) {
        let (x, y) = chunk_center(coord);
        vec![PortalSpec {
            kind: PortalKind::Dungeon,
            direction: PortalDirection::OutOfInstance,
            x,
            y,
        }]
    } else {
        Vec::new()
    }
}

/// Center of Instance chunk `{1,1}` in instance-local sub-units — where the
/// return portal sits.
pub fn return_portal_pos() -> (i64, i64) {
    chunk_center(ChunkCoord::new(1, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_trees_around_center() {
        let nodes = resource_nodes(ChunkCoord::new(0, 0));
        assert_eq!(nodes.len(), 5);
        // Center tree is exactly at chunk center.
        assert!(nodes.iter().any(|n| n.x == 8_000 && n.y == 8_000));
        assert!(nodes.iter().any(|n| n.x == 8_500 && n.y == 8_500));
    }

    #[test]
    fn portal_only_in_origin_chunk() {
        let p = portals(ChunkCoord::new(0, 0));
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].x, p[0].y), (4_000, 4_000));
        assert_eq!(p[0].direction, PortalDirection::IntoInstance);
        assert!(portals(ChunkCoord::new(1, 0)).is_empty());
    }

    #[test]
    fn instance_return_portal_at_center() {
        let p = instance_portals(ChunkCoord::new(1, 1));
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].x, p[0].y), (24_000, 24_000));
        assert_eq!(return_portal_pos(), (24_000, 24_000));
        assert!(instance_portals(ChunkCoord::new(0, 0)).is_empty());
    }
}
