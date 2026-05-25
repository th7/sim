//! Chunk-graph connectivity over 8-adjacency.
//!
//! A cluster's working set is a set of chunks (the union of its actors' 3×3
//! footprints). Two of the Labeler's three topology decisions are pure
//! functions of such sets:
//!
//! - **merge** — two clusters must merge iff their chunk-sets *intersect*
//!   (share a chunk); see [`intersects`].
//! - **split** — a cluster may split iff its chunk-set has ≥2 connected
//!   components under 8-adjacency; see [`connected_components`].
//!
//! Soundness rests on `interaction_range ≤ chunk_size` (IDEA.md): two actors
//! can only interact when their chunks are within Chebyshev distance 1, at
//! which point their 3×3 footprints share chunks — so any interacting pair is
//! already merged, by construction. The Chebyshev-distance-3 band (footprints
//! border but don't overlap) is the hysteresis gap: a single cluster spanning
//! it stays one connected component, while two separate clusters there don't
//! merge.

use crate::geometry::ChunkCoord;
use std::collections::BTreeSet;

/// True iff the two chunk-sets share at least one chunk. This is the
/// **merge** predicate: overlap ⇒ merge (never under-merge).
pub fn intersects(a: &BTreeSet<ChunkCoord>, b: &BTreeSet<ChunkCoord>) -> bool {
    // Iterate the smaller set against the larger for cheapness.
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    small.iter().any(|c| large.contains(c))
}

/// Partition a chunk-set into its 8-adjacency connected components. A set with
/// 0 or 1 components needs no split; ≥2 means the cluster's actors have drifted
/// into spatially disjoint groups and the cluster *may* split into them.
///
/// Components are returned in a deterministic order (sorted by their minimum
/// chunk) so callers — and tests — see stable results.
pub fn connected_components(chunks: &BTreeSet<ChunkCoord>) -> Vec<BTreeSet<ChunkCoord>> {
    let mut remaining: BTreeSet<ChunkCoord> = chunks.clone();
    let mut components: Vec<BTreeSet<ChunkCoord>> = Vec::new();

    while let Some(&seed) = remaining.iter().next() {
        // Flood-fill from `seed` over 8-adjacency, restricted to `chunks`.
        let mut component = BTreeSet::new();
        let mut stack = vec![seed];
        remaining.remove(&seed);

        while let Some(c) = stack.pop() {
            component.insert(c);
            for n in c.ring() {
                if remaining.remove(&n) {
                    stack.push(n);
                }
            }
        }

        components.push(component);
    }

    // Deterministic ordering by smallest member.
    components.sort_by_key(|comp| *comp.iter().next().expect("non-empty component"));
    components
}

/// Whether a chunk-set is a single connected blob (0 or 1 components).
pub fn is_connected(chunks: &BTreeSet<ChunkCoord>) -> bool {
    connected_components(chunks).len() <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(coords: &[(i32, i32)]) -> BTreeSet<ChunkCoord> {
        coords.iter().map(|&(x, y)| ChunkCoord::new(x, y)).collect()
    }

    #[test]
    fn empty_and_singleton_are_connected() {
        assert!(is_connected(&set(&[])));
        assert_eq!(connected_components(&set(&[])).len(), 0);
        assert!(is_connected(&set(&[(0, 0)])));
        assert_eq!(connected_components(&set(&[(5, 5)])).len(), 1);
    }

    #[test]
    fn adjacent_chunks_are_one_component() {
        // A full 3×3 block is connected.
        let block = set(&[
            (-1, -1), (0, -1), (1, -1),
            (-1, 0), (0, 0), (1, 0),
            (-1, 1), (0, 1), (1, 1),
        ]);
        assert!(is_connected(&block));
    }

    #[test]
    fn diagonal_touch_counts_as_connected() {
        // (0,0) and (1,1) are 8-adjacent.
        assert!(is_connected(&set(&[(0, 0), (1, 1)])));
    }

    #[test]
    fn gap_of_one_disconnects() {
        // (0,0) and (2,0) have a one-chunk gap → two components.
        let comps = connected_components(&set(&[(0, 0), (2, 0)]));
        assert_eq!(comps.len(), 2);
    }

    #[test]
    fn two_footprints_at_chebyshev_3_still_connected() {
        // Two 3×3 footprints centered 3 chunks apart border each other.
        let a = ChunkCoord::new(0, 0).footprint_3x3();
        let b = ChunkCoord::new(3, 0).footprint_3x3();
        let mut union: BTreeSet<ChunkCoord> = a.iter().copied().collect();
        union.extend(b.iter().copied());
        assert!(is_connected(&union), "footprints at distance 3 border → connected");
    }

    #[test]
    fn two_footprints_at_chebyshev_4_disconnect() {
        // Distance 4: the footprints neither overlap nor border → split.
        let a = ChunkCoord::new(0, 0).footprint_3x3();
        let b = ChunkCoord::new(4, 0).footprint_3x3();
        let mut union: BTreeSet<ChunkCoord> = a.iter().copied().collect();
        union.extend(b.iter().copied());
        assert_eq!(connected_components(&union).len(), 2);
    }

    #[test]
    fn footprints_overlap_at_chebyshev_2() {
        // Distance 2: footprints share a chunk → merge predicate fires.
        let a: BTreeSet<ChunkCoord> = ChunkCoord::new(0, 0).footprint_3x3().into_iter().collect();
        let b: BTreeSet<ChunkCoord> = ChunkCoord::new(2, 0).footprint_3x3().into_iter().collect();
        assert!(intersects(&a, &b));
        // Distance 3: footprints do NOT share (border only).
        let c: BTreeSet<ChunkCoord> = ChunkCoord::new(3, 0).footprint_3x3().into_iter().collect();
        assert!(!intersects(&a, &c));
    }
}
