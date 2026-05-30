//! Integration tests for the warm/cold boundary (ADR-0005/0006): wildlife
//! materializes from the deterministic ecosystem field near a Player and
//! dissolves back into a healing Region Disturbance when the Player leaves.

use sim::components::{Inventory, Position};
use sim::geometry::{chunk_center, ChunkCoord, CHUNK_SIZE};
use sim::motivation::{Drives, NpcKind};
use sim::sim::Sim;

/// Centre of a chunk on the x-axis whose Region is deer-rich (a Meadow), so
/// materialization reliably produces deer.
fn deer_rich_center() -> (i64, i64) {
    let sim = Sim::new();
    for k in 0..80 {
        let (cx, cy) = chunk_center(ChunkCoord::new(k, 0));
        if sim.region_levels_at(cx, cy).deer > 0.55 {
            return (cx, cy);
        }
    }
    panic!("no deer-rich region found");
}

#[test]
fn wildlife_materializes_near_a_player() {
    let (cx, cy) = deer_rich_center();
    let mut sim = Sim::new();
    sim.set_wildlife(true);
    sim.connect_at("alice", Position { x: cx, y: cy }, Inventory::default());
    sim.tick();
    assert!(!sim.npcs().is_empty(), "wildlife should materialize in the Player's warm chunks");
}

#[test]
fn wildlife_dissolves_when_the_player_leaves() {
    let (cx, cy) = deer_rich_center();
    let mut sim = Sim::new();
    sim.set_wildlife(true);
    sim.connect_at("alice", Position { x: cx, y: cy }, Inventory::default());
    sim.tick();
    assert!(!sim.npcs().is_empty());

    sim.disconnect("alice");
    sim.tick();
    assert!(sim.npcs().is_empty(), "no Player → all wildlife dissolves (NPCs don't anchor warmth)");
}

#[test]
fn overhunting_a_region_lowers_its_deer_level() {
    let (cx, cy) = deer_rich_center();
    let mut sim = Sim::new();
    sim.set_wildlife(true);
    sim.connect_at("alice", Position { x: cx, y: cy }, Inventory::default());
    // An extra ravenous wolf in the same chunk to thin the herd while observed.
    sim.spawn_npc(NpcKind::Wolf, Position { x: cx, y: cy }, Drives { hunger: 1.0, hunger_pressure: 1.0, ..Default::default() });
    sim.tick();
    let before = sim.region_levels_at(cx, cy).deer;

    for _ in 0..400 {
        sim.tick();
    }
    // The Player leaves; the chunk dissolves, folding the thinned herd into a
    // negative Region Disturbance.
    sim.disconnect("alice");
    sim.tick();

    let after = sim.region_levels_at(cx, cy).deer;
    assert!(after < before, "overhunting should deplete the Region's deer ({before} -> {after})");
}

#[test]
fn materialization_is_deterministic() {
    let (cx, cy) = deer_rich_center();
    let run = || {
        let mut sim = Sim::new();
        sim.set_wildlife(true);
        sim.connect_at("alice", Position { x: cx, y: cy }, Inventory::default());
        sim.tick();
        sim.npcs().len()
    };
    assert_eq!(run(), run());
    // And sanity: a Player far from the deer-rich chunk yields different counts is
    // not asserted (regions differ); determinism of the same setup is the contract.
    let _ = CHUNK_SIZE;
}
