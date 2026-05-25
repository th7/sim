//! Phase 1 integration: drive the single-threaded Sim with scripted/random
//! intent and assert movement, topology transitions, the never-under-merge
//! invariant, and determinism.

use sim::components::{Inventory, Position};
use sim::geometry::ChunkCoord;
use sim::harness::{assert_invariant, Rng};
use sim::ids::Realm;
use sim::sim::Sim;

fn pos(x: i64, y: i64) -> Position {
    Position { x, y }
}

#[test]
fn movement_integrates_at_four_units_per_second() {
    let mut sim = Sim::new();
    sim.connect_at("alice", pos(8_000, 8_000), Inventory::default());
    // Clear the tree at chunk-center so it doesn't block: move alice off-center
    // first is unnecessary — she's grandfathered out. Drive east one tick.
    sim.set_intent("alice", 1.0, 0.0);
    sim.tick();
    // 4000 sub-units/sec * 0.05s = 200 sub-units/tick.
    assert_eq!(sim.position("alice").unwrap().x, 8_200);
}

#[test]
fn lone_player_cluster_owns_its_3x3_and_follows() {
    let mut sim = Sim::new();
    sim.connect("alice", ChunkCoord::new(0, 0));
    let cid = sim.cluster_of("alice").unwrap();
    let rw = sim.overworld();
    let cluster = rw.labeler.cluster(cid).unwrap();
    assert_eq!(cluster.chunk_set.len(), 9, "lone player owns a 3×3 footprint");
    assert!(cluster.chunk_set.contains(&ChunkCoord::new(-1, -1)));
    assert!(cluster.chunk_set.contains(&ChunkCoord::new(1, 1)));

    // Walk east across the chunk-1 boundary (8000 → past 16000 ≈ 40+ ticks).
    sim.set_intent("alice", 1.0, 0.0);
    for _ in 0..50 {
        sim.tick();
    }
    assert!(sim.position("alice").unwrap().x >= 16_000, "crossed into chunk 1");
    let cluster = {
        let cid = sim.cluster_of("alice").unwrap();
        sim.overworld().labeler.cluster(cid).unwrap().chunk_set.clone()
    };
    // Footprint now centered on chunk (1,0)-ish: owns chunk 2, dropped chunk -1.
    assert!(cluster.contains(&ChunkCoord::new(2, 0)));
    assert!(!cluster.contains(&ChunkCoord::new(-1, 0)));
}

#[test]
fn approach_triggers_merge_then_separation_splits() {
    let mut sim = Sim::new();
    // Two players five chunks apart → two clusters. Walk along y=10_000, which
    // is clear of every chunk's tree footprints (trees sit near y≈8_000).
    sim.connect_at("alice", pos(8_000, 10_000), Inventory::default());
    sim.connect_at("bob", pos(5 * 16_000 + 8_000, 10_000), Inventory::default());
    assert_ne!(sim.cluster_of("alice"), sim.cluster_of("bob"));

    // Walk bob west toward alice until they merge.
    sim.set_intent("bob", -1.0, 0.0);
    let mut merged = false;
    for _ in 0..400 {
        sim.tick();
        assert_invariant(&sim, Realm::Overworld);
        if sim.cluster_of("alice") == sim.cluster_of("bob") {
            merged = true;
            break;
        }
    }
    assert!(merged, "approaching players must merge into one cluster");

    // Now walk bob back east; eventually they split again.
    sim.set_intent("bob", 1.0, 0.0);
    let mut split = false;
    for _ in 0..400 {
        sim.tick();
        assert_invariant(&sim, Realm::Overworld);
        if sim.cluster_of("alice") != sim.cluster_of("bob") {
            split = true;
            break;
        }
    }
    assert!(split, "separating players must split back into two clusters");
    assert_eq!(sim.overworld().labeler.cluster_count(), 2);
}

#[test]
fn invariant_holds_over_random_walk() {
    // Several seeds, several players, hundreds of ticks of random intent. The
    // never-under-merge invariant must hold on every tick.
    for seed in 0..16u64 {
        let mut sim = Sim::new();
        let mut rng = Rng::new(seed.wrapping_mul(0x1234_5678).wrapping_add(1));

        let names = ["p0", "p1", "p2", "p3", "p4", "p5"];
        for (i, n) in names.iter().enumerate() {
            // Spread starting positions over a few chunks so they mix.
            let x = 8_000 + (i as i64 % 3) * 6_000;
            let y = 8_000 + (i as i64 / 3) * 6_000;
            sim.connect_at(n, pos(x, y), Inventory::default());
        }

        for _ in 0..300 {
            // Occasionally change each player's intent.
            for n in names {
                if rng.below(4) == 0 {
                    let (dx, dy) = rng.intent();
                    sim.set_intent(n, dx, dy);
                }
            }
            sim.tick();
            assert_invariant(&sim, Realm::Overworld);
        }
    }
}

#[test]
fn deterministic_under_identical_inputs() {
    let run = || -> Vec<(String, Position, u64)> {
        let mut sim = Sim::new();
        let mut rng = Rng::new(0xDEAD_BEEF);
        let names = ["a", "b", "c", "d"];
        for (i, n) in names.iter().enumerate() {
            sim.connect_at(n, pos(8_000 + i as i64 * 5_000, 8_000), Inventory::default());
        }
        for _ in 0..250 {
            for n in names {
                if rng.below(3) == 0 {
                    let (dx, dy) = rng.intent();
                    sim.set_intent(n, dx, dy);
                }
            }
            sim.tick();
        }
        // Final observable state: each player's position + cluster id.
        names
            .iter()
            .map(|n| {
                (
                    n.to_string(),
                    sim.position(n).unwrap(),
                    sim.cluster_of(n).unwrap().0,
                )
            })
            .collect()
    };

    assert_eq!(run(), run(), "identical inputs must produce identical output");
}
