//! Integration tests for the warm/cold boundary: wildlife
//! materializes from the deterministic ecosystem field near a Player and
//! dissolves back into a healing Region Disturbance when the Player leaves.

use sim::components::{Inventory, Position};
use sim::ecosystem::{initial_drives, Levels};
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

/// Scan east for a chunk whose warm set materializes a wolf sitting in a Region
/// with the wanted prey (deer) level, and return that wolf's Region levels and
/// the Drives it materialized with. Deer and wolf are independent level channels
/// (Meadow vs Forest habitats), so both prey-rich and prey-scarce wolf habitats
/// occur as we walk across the field.
fn materialized_wolf_in_region(prey_rich: bool) -> (Levels, Drives) {
    for k in 0..400 {
        let (cx, cy) = chunk_center(ChunkCoord::new(k, 0));
        let mut sim = Sim::new();
        sim.set_wildlife(true);
        sim.connect_at("scout", Position { x: cx, y: cy }, Inventory::default());
        sim.tick();
        for (_, kind, pos, drives, _) in sim.npcs() {
            if kind != NpcKind::Wolf {
                continue;
            }
            // Regions are a Worley/Voronoi partition, so a wolf's jittered
            // position can fall across a Region seam from its chunk; the
            // materializer keys off `region_of_chunk` (the chunk *center*), so
            // recover the wolf's spawn Region the same way.
            let (ccx, ccy) = chunk_center(pos.chunk());
            let lv = sim.region_levels_at(ccx, ccy);
            // The habitats leave a clean gap in deer level (Meadow ~0.5–0.7,
            // Forest ~0.2–0.4), so these thresholds separate prey-rich from
            // prey-scarce with margin.
            if (prey_rich && lv.deer >= 0.5) || (!prey_rich && lv.deer <= 0.35) {
                return (lv, drives);
            }
        }
    }
    panic!("no {} wolf region found", if prey_rich { "prey-rich" } else { "prey-scarce" });
}

/// The keystone coupling, end-to-end through materialization: a freshly
/// materialized wolf carries its Region's spawn-derived temperament, so a
/// prey-scarce (depleted) Region really does put hungrier wolves on the ground —
/// not merely in the pure `initial_drives`. (`update_wildlife` runs after the
/// per-tick drive integration, so a just-materialized NPC's Drives are exactly
/// its `initial_drives` this tick.)
#[test]
fn a_prey_scarce_region_materializes_hungrier_wolves() {
    let (rich_lv, rich_wolf) = materialized_wolf_in_region(true);
    let (scarce_lv, scarce_wolf) = materialized_wolf_in_region(false);

    // Wiring: each materialized wolf carries exactly its Region's spawn-derived
    // temperament — Sim::materialize applied ecosystem::initial_drives.
    assert_eq!(rich_wolf, initial_drives(NpcKind::Wolf, &rich_lv));
    assert_eq!(scarce_wolf, initial_drives(NpcKind::Wolf, &scarce_lv));

    // Observable: a prey-scarce (depleted) Region puts hungrier, higher-pressure
    // wolves on the ground than a prey-rich one.
    assert!(
        scarce_lv.deer < rich_lv.deer,
        "the scarce Region has less prey ({} !< {})",
        scarce_lv.deer,
        rich_lv.deer
    );
    assert!(scarce_wolf.hunger > rich_wolf.hunger, "prey-scarce wolves materialize hungrier");
    assert!(
        scarce_wolf.hunger_pressure > rich_wolf.hunger_pressure,
        "prey-scarce wolves materialize under more hunger pressure"
    );
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
