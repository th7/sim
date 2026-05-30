//! Integration tests for NPC behaviour: the Motivation engine driving real
//! actors through the Sim's tick (movement Intent). Combat/eating land in a
//! later slice; here we pin that NPCs *move* the way their Drives dictate.

use sim::components::Position;
use sim::motivation::{Drives, NpcKind};
use sim::sim::Sim;

fn pos(x: i64, y: i64) -> Position {
    Position { x, y }
}

/// X position of the first NPC of `kind`.
fn npc_x(sim: &Sim, kind: NpcKind) -> i64 {
    sim.npcs()
        .into_iter()
        .find(|(_, k, _, _, _)| *k == kind)
        .map(|(_, _, p, _, _)| p.x)
        .expect("npc present")
}

#[test]
fn wolf_hunts_deer_while_deer_flees() {
    let mut sim = Sim::new();
    // A hungry wolf to the west, a deer 800 sub-units east — within perception.
    sim.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives { hunger: 0.8, ..Default::default() });
    sim.spawn_npc(NpcKind::Deer, pos(8_800, 8_000), Drives::default());

    let (wolf0, deer0) = (npc_x(&sim, NpcKind::Wolf), npc_x(&sim, NpcKind::Deer));
    for _ in 0..20 {
        sim.tick();
    }
    let (wolf1, deer1) = (npc_x(&sim, NpcKind::Wolf), npc_x(&sim, NpcKind::Deer));

    assert!(wolf1 > wolf0, "wolf should pursue the deer eastward");
    assert!(deer1 > deer0, "deer should flee eastward, away from the wolf");
    let gap0 = deer0 - wolf0;
    let gap1 = deer1 - wolf1;
    assert!(gap1 < gap0, "the faster wolf should close the gap ({gap0} -> {gap1})");
}

#[test]
fn unthreatened_hungry_deer_grazes_in_place() {
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Deer, pos(8_000, 8_000), Drives { hunger: 0.6, ..Default::default() });
    let start = npc_x(&sim, NpcKind::Deer);
    for _ in 0..40 {
        sim.tick();
    }
    let end = npc_x(&sim, NpcKind::Deer);
    assert_eq!(start, end, "a safe, fed-on-grass deer grazes without moving");
}

#[test]
fn npc_motion_is_deterministic() {
    let run = || {
        let mut sim = Sim::new();
        sim.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives { hunger: 0.9, ..Default::default() });
        sim.spawn_npc(NpcKind::Deer, pos(9_000, 8_400), Drives::default());
        for _ in 0..30 {
            sim.tick();
        }
        (npc_x(&sim, NpcKind::Wolf), npc_x(&sim, NpcKind::Deer))
    };
    assert_eq!(run(), run(), "identical setups must produce identical motion");
}
