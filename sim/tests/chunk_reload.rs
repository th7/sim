//! Regression tests for "wildlife disappears after chunk reload" (was in BUGS.md).
//!
//! Root cause: the warm/cold boundary depleted a Region's wildlife Disturbance
//! from per-chunk dissolve accounting `(survivors − materialized)`, which counts a
//! *migrating* animal as a loss at every chunk it crosses. Depletion is now
//! event-sourced from actual deaths, so wandering and reload cycles are
//! population-neutral. (A secondary fix scales per-kill depletion to Region size.)

use sim::components::{Inventory, Position};
use sim::geometry::{chunk_center, ChunkCoord, CHUNK_SIZE};
use sim::sim::Sim;

fn deer_rich_center() -> (i64, i64) {
    let sim = Sim::new();
    for k in 0..80 {
        let (cx, cy) = chunk_center(ChunkCoord::new(k, 0));
        if sim.region_levels_at(cx, cy).deer > 0.55 {
            return (cx, cy);
        }
    }
    panic!("no deer-rich region");
}

#[test]
fn wildlife_repopulates_after_disconnect_reconnect() {
    let (cx, cy) = deer_rich_center();
    let mut sim = Sim::new();
    sim.set_wildlife(true);
    sim.connect_at("alice", Position { x: cx, y: cy }, Inventory::default());
    sim.tick();
    assert!(!sim.npcs().is_empty(), "wildlife materializes initially");

    sim.disconnect("alice");
    sim.tick();
    assert!(sim.npcs().is_empty(), "wildlife dissolves with no players");

    sim.connect_at("alice", Position { x: cx, y: cy }, Inventory::default());
    sim.tick();
    assert!(!sim.npcs().is_empty(), "wildlife re-materializes on reconnect");
}

#[test]
fn pacing_across_chunks_does_not_deplete_the_region() {
    // The original bug: walking back and forth (reloading chunks) drained the
    // Region to zero because migrating animals were mis-counted as deaths.
    let (cx, cy) = deer_rich_center();
    let py = cy + 3_000; // off the chunk-centre tree row so the player can move
    let mut sim = Sim::new();
    sim.set_wildlife(true);
    sim.connect_at("alice", Position { x: cx, y: py }, Inventory::default());
    sim.tick();
    let level0 = sim.region_levels_at(cx, py).deer;

    for _ in 0..8 {
        sim.set_intent("alice", 1.0, 0.0);
        for _ in 0..120 {
            sim.tick();
        }
        sim.set_intent("alice", -1.0, 0.0);
        for _ in 0..120 {
            sim.tick();
        }
    }
    sim.set_intent("alice", 0.0, 0.0);
    sim.tick();

    let level1 = sim.region_levels_at(cx, py).deer;
    assert!(
        level1 > 0.3,
        "pacing must not deplete the Region toward zero (deer level {level0:.3} -> {level1:.3})",
    );
    assert!(!sim.npcs().is_empty(), "wildlife is still present after pacing");
    let _ = CHUNK_SIZE;
}

#[test]
fn wildlife_present_after_walking_away_and_back() {
    let (cx, cy) = deer_rich_center();
    let py = cy + 3_000;
    let mut sim = Sim::new();
    sim.set_wildlife(true);
    sim.connect_at("alice", Position { x: cx, y: py }, Inventory::default());
    sim.tick();
    assert!(!sim.npcs().is_empty());

    sim.set_intent("alice", 1.0, 0.0);
    for _ in 0..300 {
        sim.tick();
    }
    assert!(sim.position("alice").unwrap().x > cx + 2 * CHUNK_SIZE, "player walked clear");

    sim.set_intent("alice", -1.0, 0.0);
    for _ in 0..500 {
        sim.tick();
        if sim.position("alice").map(|p| p.x <= cx).unwrap_or(false) {
            break;
        }
    }
    sim.set_intent("alice", 0.0, 0.0);
    sim.tick();
    assert!(!sim.npcs().is_empty(), "wildlife is present again after returning");
}
