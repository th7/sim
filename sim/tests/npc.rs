//! Integration tests for NPC behaviour: the Motivation engine driving real
//! actors through the Sim's tick (movement Intent). Combat/eating land in a
//! later slice; here we pin that NPCs *move* the way their Drives dictate.

use sim::components::{Inventory, Item, Position};
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

fn has_npc(sim: &Sim, kind: NpcKind) -> bool {
    sim.npcs().iter().any(|(_, k, _, _, _)| *k == kind)
}

fn hunger(sim: &Sim, kind: NpcKind) -> f64 {
    sim.npcs()
        .into_iter()
        .find(|(_, k, _, _, _)| *k == kind)
        .map(|(_, _, _, d, _)| d.hunger)
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

#[test]
fn player_kills_deer_into_carcass_then_harvests_meat_and_hide() {
    let mut sim = Sim::new();
    sim.connect_at("alice", pos(8_000, 8_000), Inventory::default());
    sim.spawn_npc(NpcKind::Deer, pos(8_300, 8_000), Drives::default());

    // Two 25-damage clicks kill the 50-HP deer (no tick between, so it can't flee).
    sim.damage("alice", 8_300, 8_000).unwrap();
    sim.damage("alice", 8_300, 8_000).unwrap();
    assert!(!has_npc(&sim, NpcKind::Deer), "deer should be dead");

    // The Carcass it left is harvestable into meat + hide.
    sim.harvest("alice", 8_300, 8_000).unwrap();
    let inv = sim.inventory_of("alice").unwrap();
    assert_eq!(inv.items.get(&Item::Meat).copied(), Some(3));
    assert_eq!(inv.items.get(&Item::Hide).copied(), Some(1));
}

#[test]
fn wolf_kills_and_eats_a_deer() {
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives { hunger: 0.9, ..Default::default() });
    sim.spawn_npc(NpcKind::Deer, pos(8_400, 8_000), Drives::default());

    for _ in 0..400 {
        sim.tick();
        if !has_npc(&sim, NpcKind::Deer) && hunger(&sim, NpcKind::Wolf) < 0.4 {
            break;
        }
    }
    assert!(!has_npc(&sim, NpcKind::Deer), "wolf should have killed the deer");
    assert!(hunger(&sim, NpcKind::Wolf) < 0.6, "wolf should have eaten and be less hungry");
    assert!(has_npc(&sim, NpcKind::Wolf), "wolf survives (deer cannot fight back)");
}

#[test]
fn attacked_wolf_flees_when_not_hungry() {
    let mut sim = Sim::new();
    sim.connect_at("alice", pos(8_000, 8_000), Inventory::default());
    sim.spawn_npc(NpcKind::Wolf, pos(8_500, 8_000), Drives { hunger: 0.2, ..Default::default() });

    sim.damage("alice", 8_500, 8_000).unwrap(); // provoke it
    let before = npc_x(&sim, NpcKind::Wolf);
    for _ in 0..5 {
        sim.tick();
    }
    assert!(npc_x(&sim, NpcKind::Wolf) > before, "a provoked, unhungry wolf flees the player");
}

// --- agent-invented extensions (see EXTENSIONS.md) ---

#[test]
fn scattered_deer_form_a_herd() {
    // Three unthreatened deer spread out should drift together (cohesion).
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Deer, pos(8_000, 8_000), Drives::default());
    sim.spawn_npc(NpcKind::Deer, pos(11_000, 8_000), Drives::default());
    sim.spawn_npc(NpcKind::Deer, pos(9_500, 10_500), Drives::default());

    let spread = |sim: &Sim| -> i64 {
        let xs: Vec<(i64, i64)> = sim.npcs().iter().map(|(_, _, p, _, _)| (p.x, p.y)).collect();
        let mut max = 0;
        for i in 0..xs.len() {
            for j in (i + 1)..xs.len() {
                let d = (xs[i].0 - xs[j].0).pow(2) + (xs[i].1 - xs[j].1).pow(2);
                max = max.max(d);
            }
        }
        max
    };
    let before = spread(&sim);
    for _ in 0..60 {
        sim.tick();
    }
    let after = spread(&sim);
    assert!(after < before, "deer should cluster: spread {before} -> {after}");
}

#[test]
fn wolves_pack_onto_a_single_deer() {
    // Two wolves that would naturally split onto different deer instead gang up
    // on the focal one, so they converge rather than diverge.
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives { hunger: 0.9, ..Default::default() });
    sim.spawn_npc(NpcKind::Wolf, pos(12_000, 8_000), Drives { hunger: 0.9, ..Default::default() });
    sim.spawn_npc(NpcKind::Deer, pos(8_800, 8_000), Drives::default());
    sim.spawn_npc(NpcKind::Deer, pos(11_500, 8_000), Drives::default());

    let wolf_gap = |sim: &Sim| -> i64 {
        let ws: Vec<(i64, i64)> = sim
            .npcs()
            .iter()
            .filter(|(_, k, _, _, _)| *k == NpcKind::Wolf)
            .map(|(_, _, p, _, _)| (p.x, p.y))
            .collect();
        if ws.len() < 2 {
            return 0;
        }
        (ws[0].0 - ws[1].0).pow(2) + (ws[0].1 - ws[1].1).pow(2)
    };
    let before = wolf_gap(&sim);
    for _ in 0..30 {
        sim.tick();
    }
    let after = wolf_gap(&sim);
    assert!(after < before, "pack should converge on one deer: gap² {before} -> {after}");
}

#[test]
fn a_herd_flees_a_predator_together() {
    // A wolf attacks one edge of a tight herd; the panic spreads so the whole
    // herd's centroid moves away from the wolf (stampede / fear contagion).
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Wolf, pos(7_400, 8_000), Drives { hunger: 1.0, ..Default::default() });
    for dx in [0_i64, 700, 1_400, 2_100] {
        sim.spawn_npc(NpcKind::Deer, pos(8_000 + dx, 8_000), Drives::default());
    }
    let herd_cx = |sim: &Sim| -> i64 {
        let xs: Vec<i64> = sim
            .npcs()
            .iter()
            .filter(|(_, k, _, _, _)| *k == NpcKind::Deer)
            .map(|(_, _, p, _, _)| p.x)
            .collect();
        if xs.is_empty() { 0 } else { xs.iter().sum::<i64>() / xs.len() as i64 }
    };
    let before = herd_cx(&sim);
    for _ in 0..25 {
        sim.tick();
    }
    let after = herd_cx(&sim);
    assert!(after > before, "the herd should flee east, away from the wolf ({before} -> {after})");
}
