//! Phase 1 topology cases: placement, crossing, merge, split,
//! hysteresis — driven through the Labeler and checked against the canonical
//! partition oracle.

use sim::geometry::ChunkCoord;
use sim::ids::ActorId;
use sim::labeler::{canonical_partition, Labeler, TopologyEvent};
use std::collections::{BTreeMap, BTreeSet};

fn c(x: i32, y: i32) -> ChunkCoord {
    ChunkCoord::new(x, y)
}

/// Assert the Labeler's partition equals the oracle's, grouping actors the same
/// way (ignoring cluster ids).
fn assert_matches_oracle(lab: &Labeler, homes: &BTreeMap<ActorId, ChunkCoord>) {
    let oracle = canonical_partition(homes);

    // Group actors by their Labeler cluster id.
    let mut by_cluster: BTreeMap<_, BTreeSet<ActorId>> = BTreeMap::new();
    for (&actor, _) in homes {
        let cid = lab.cluster_of(actor).expect("actor is clustered");
        by_cluster.entry(cid).or_default().insert(actor);
    }
    let mut lab_groups: Vec<BTreeSet<ActorId>> = by_cluster.into_values().collect();
    lab_groups.sort();

    let mut oracle_sorted = oracle;
    oracle_sorted.sort();

    assert_eq!(lab_groups, oracle_sorted, "Labeler partition must match oracle");
}

#[test]
fn placement_new_cluster_then_join_overlapping() {
    let mut lab = Labeler::new();
    let mut homes = BTreeMap::new();

    // First actor → a fresh cluster.
    let a = ActorId(1);
    homes.insert(a, c(0, 0));
    let ev = lab.insert_actor(a, c(0, 0));
    assert!(matches!(ev[0], TopologyEvent::Created(_)));
    assert_eq!(lab.cluster_count(), 1);

    // Second actor far away → its own cluster.
    let b = ActorId(2);
    homes.insert(b, c(10, 10));
    lab.insert_actor(b, c(10, 10));
    assert_eq!(lab.cluster_count(), 2);

    // Third actor overlapping `a` → joins a's cluster (no new cluster).
    let d = ActorId(3);
    homes.insert(d, c(1, 0));
    lab.insert_actor(d, c(1, 0));
    assert_eq!(lab.cluster_count(), 2);
    assert_eq!(lab.cluster_of(a), lab.cluster_of(d));
    assert_ne!(lab.cluster_of(a), lab.cluster_of(b));

    assert_matches_oracle(&lab, &homes);
}

#[test]
fn crossing_within_cluster_no_topology_change() {
    let mut lab = Labeler::new();
    let a = ActorId(1);
    lab.insert_actor(a, c(0, 0));
    let cid = lab.cluster_of(a).unwrap();

    // Cross one chunk east. A lone actor's cluster just follows it — no merge,
    // no split, same id.
    let ev = lab.move_actor(a, c(1, 0));
    assert!(ev.is_empty(), "a lone crossing produces no merge/split: {ev:?}");
    assert_eq!(lab.cluster_of(a), Some(cid));
    assert!(lab.cluster(cid).unwrap().chunk_set.contains(&c(2, 0)));
    assert!(!lab.cluster(cid).unwrap().chunk_set.contains(&c(-1, 0)));
}

#[test]
fn merge_triggered_by_a_crossing() {
    let mut lab = Labeler::new();
    let mut homes = BTreeMap::new();

    // Two clusters, far enough apart to be separate (Chebyshev 4 → no overlap).
    let a = ActorId(1);
    let b = ActorId(2);
    homes.insert(a, c(0, 0));
    homes.insert(b, c(4, 0));
    lab.insert_actor(a, c(0, 0));
    lab.insert_actor(b, c(4, 0));
    assert_eq!(lab.cluster_count(), 2);

    // `a` crosses east to chunk 2 → footprints now share chunk 3 → must merge.
    homes.insert(a, c(2, 0));
    let ev = lab.move_actor(a, c(2, 0));
    assert!(
        ev.iter().any(|e| matches!(e, TopologyEvent::Merged { .. })),
        "expected a merge: {ev:?}"
    );
    assert_eq!(lab.cluster_count(), 1);
    assert_eq!(lab.cluster_of(a), lab.cluster_of(b));
    assert_matches_oracle(&lab, &homes);
}

#[test]
fn merge_survivor_keeps_lower_id() {
    let mut lab = Labeler::new();
    let a = ActorId(1);
    let b = ActorId(2);
    let _ = lab.insert_actor(a, c(0, 0));
    let cid_a = lab.cluster_of(a).unwrap();
    let _ = lab.insert_actor(b, c(4, 0));
    let cid_b = lab.cluster_of(b).unwrap();
    assert!(cid_a < cid_b);

    lab.move_actor(a, c(2, 0));
    // Both actors now live under the lower of the two original ids.
    assert_eq!(lab.cluster_of(a), Some(cid_a));
    assert_eq!(lab.cluster_of(b), Some(cid_a));
}

#[test]
fn split_when_chunk_set_disconnects() {
    let mut lab = Labeler::new();
    let mut homes = BTreeMap::new();

    // Two actors merged into one cluster (Chebyshev 2 → overlap).
    let a = ActorId(1);
    let b = ActorId(2);
    homes.insert(a, c(0, 0));
    homes.insert(b, c(2, 0));
    lab.insert_actor(a, c(0, 0));
    lab.insert_actor(b, c(2, 0));
    assert_eq!(lab.cluster_count(), 1);

    // `b` walks east to chunk 5 (Chebyshev 5 → fully disconnected) → split.
    homes.insert(b, c(5, 0));
    let ev = lab.move_actor(b, c(5, 0));
    assert!(
        ev.iter().any(|e| matches!(e, TopologyEvent::Split { .. })),
        "expected a split: {ev:?}"
    );
    assert_eq!(lab.cluster_count(), 2);
    assert_ne!(lab.cluster_of(a), lab.cluster_of(b));
    assert_matches_oracle(&lab, &homes);
}

#[test]
fn hysteresis_band_at_chebyshev_3_does_not_churn() {
    // Once merged, a cluster spanning the distance-3 band stays one cluster —
    // it neither splits (footprints border → one component) nor re-merges.
    let mut lab = Labeler::new();
    let a = ActorId(1);
    let b = ActorId(2);
    lab.insert_actor(a, c(0, 0));
    lab.insert_actor(b, c(2, 0)); // merge at distance 2
    assert_eq!(lab.cluster_count(), 1);

    // Drift `b` out to distance 3 — still bordering, so still one cluster.
    let ev = lab.move_actor(b, c(3, 0));
    assert!(ev.is_empty(), "distance 3 is the hysteresis band: {ev:?}");
    assert_eq!(lab.cluster_count(), 1);

    // Back in to distance 2 — still one cluster, no churn.
    let ev = lab.move_actor(b, c(2, 0));
    assert!(ev.is_empty(), "no spurious events: {ev:?}");
    assert_eq!(lab.cluster_count(), 1);

    // Out to distance 4 — now it splits.
    let ev = lab.move_actor(b, c(4, 0));
    assert!(ev.iter().any(|e| matches!(e, TopologyEvent::Split { .. })));
    assert_eq!(lab.cluster_count(), 2);
}

#[test]
fn three_way_merge_collapses_to_one() {
    let mut lab = Labeler::new();
    let mut homes = BTreeMap::new();
    for (i, ch) in [c(0, 0), c(4, 0), c(8, 0)].into_iter().enumerate() {
        let a = ActorId(i as u64 + 1);
        homes.insert(a, ch);
        lab.insert_actor(a, ch);
    }
    assert_eq!(lab.cluster_count(), 3);

    // Move the middle actor so all three chains overlap → single cluster.
    // Place actor 4 bridging 0 and 4, etc. — instead, march actor at c(4,0) and
    // c(8,0) inward to bridge.
    homes.insert(ActorId(2), c(2, 0));
    lab.move_actor(ActorId(2), c(2, 0)); // bridges 0 and (former) 4
    homes.insert(ActorId(3), c(4, 0));
    lab.move_actor(ActorId(3), c(4, 0)); // bridges 2 and itself
    assert_eq!(lab.cluster_count(), 1);
    assert_matches_oracle(&lab, &homes);
}

#[test]
fn remove_actor_can_split_or_empty() {
    let mut lab = Labeler::new();
    let mut homes = BTreeMap::new();
    // a — bridge — c arrangement: a@0, bridge@3? No: make a@0, b@3, d@6 linear,
    // each bordering the next via a chain of overlaps.
    let a = ActorId(1);
    let bridge = ActorId(2);
    let d = ActorId(3);
    homes.insert(a, c(0, 0));
    homes.insert(bridge, c(2, 0));
    homes.insert(d, c(4, 0));
    lab.insert_actor(a, c(0, 0));
    lab.insert_actor(bridge, c(2, 0));
    lab.insert_actor(d, c(4, 0));
    assert_eq!(lab.cluster_count(), 1, "chain of overlaps is one cluster");

    // Remove the bridge → a and d disconnect → split into two.
    homes.remove(&bridge);
    let ev = lab.remove_actor(bridge);
    assert!(ev.iter().any(|e| matches!(e, TopologyEvent::Split { .. })));
    assert_eq!(lab.cluster_count(), 2);
    assert_matches_oracle(&lab, &homes);

    // Remove the rest → clusters empty out.
    lab.remove_actor(a);
    lab.remove_actor(d);
    assert_eq!(lab.cluster_count(), 0);
    assert_eq!(lab.actor_count(), 0);
}
