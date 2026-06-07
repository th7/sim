//! Integration tests for NPC behaviour: the Motivation engine driving real
//! actors through the Sim's tick (movement Intent). Combat/eating land in a
//! later slice; here we pin that NPCs *move* the way their Drives dictate.

use sim::components::{Inventory, Item, Position};
use sim::motivation::{Drives, NpcKind};
use sim::sim::{Action, Sim};
use sim::wire::{entity_states, EntityWire};

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


/// WireId of the first NPC of `kind` on the wire.
fn npc_wid(sim: &Sim, kind: NpcKind) -> sim::components::WireId {
    let prefix = format!("npc:{}:", kind.as_str());
    entity_states(sim.overworld())
        .into_iter()
        .find_map(|(wid, s)| (matches!(s, EntityWire::Npc { .. }) && wid.0.starts_with(&prefix)).then_some(wid))
    .expect("npc on the wire")
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

    // Two 25-damage clicks kill the 50-HP deer. Both resolve in one tick (FIFO,
    // before movement), so the deer can't flee between them — the intent-model
    // equivalent of "no tick between".
    let deer = npc_wid(&sim, NpcKind::Deer);
    sim.enqueue_action("alice", Action::Damage { target: deer.clone() });
    sim.enqueue_action("alice", Action::Damage { target: deer });
    sim.tick();
    assert!(!has_npc(&sim, NpcKind::Deer), "deer should be dead");

    // The Carcass it left is harvestable into meat + hide — by its identity.
    let carcass = entity_states(sim.overworld())
        .into_iter()
        .find_map(|(wid, s)| matches!(s, EntityWire::Carcass { .. }).then_some(wid))
        .expect("the dead deer leaves a Carcass");
    sim.enqueue_action("alice", Action::Harvest { target: carcass });
    sim.tick();
    let inv = sim.inventory_of("alice").unwrap();
    assert_eq!(inv.items.get(&Item::Meat).copied(), Some(3));
    assert_eq!(inv.items.get(&Item::Hide).copied(), Some(1));
}

/// Entity-directed damage acts on *identity, not place*: with two deer in
/// range, naming the farther one hits the farther one — even though the old
/// position-based verb would have resolved to the nearer.
#[test]
fn damage_by_identity_hits_the_named_deer_not_the_nearest() {
    let mut sim = Sim::new();
    sim.connect_at("alice", pos(8_000, 8_000), Inventory::default());
    sim.spawn_npc(NpcKind::Deer, pos(8_300, 8_000), Drives::default()); // nearer
    sim.spawn_npc(NpcKind::Deer, pos(8_800, 8_000), Drives::default()); // farther

    // Find the farther deer's WireId.
    let farther = entity_states(sim.overworld())
        .into_iter()
        .find_map(|(wid, s)| match s {
            EntityWire::Npc { x, .. } if x == 8_800 => Some(wid),
            _ => None,
        })
        .expect("the farther deer is on the wire");

    sim.enqueue_action("alice", Action::Damage { target: farther.clone() });
    sim.tick();

    let hps: Vec<(String, i64)> = entity_states(sim.overworld())
        .into_iter()
        .filter_map(|(wid, s)| match s {
            EntityWire::Npc { hp, .. } => Some((wid.0, hp)),
            _ => None,
        })
        .collect();
    let hp_of = |w: &str| hps.iter().find(|(wid, _)| wid == w).map(|(_, hp)| *hp);
    assert_eq!(hp_of(&farther.0), Some(25), "the named deer took the hit");
    assert!(
        hps.iter().any(|(wid, hp)| wid != &farther.0 && *hp == 50),
        "the nearer, unnamed deer is untouched"
    );
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

    sim.enqueue_action("alice", Action::Damage { target: npc_wid(&sim, NpcKind::Wolf) }); // provoke it
    sim.tick();
    let before = npc_x(&sim, NpcKind::Wolf);
    for _ in 0..5 {
        sim.tick();
    }
    assert!(npc_x(&sim, NpcKind::Wolf) > before, "a provoked, unhungry wolf flees the player");
}

// --- agent-invented extensions ---

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

/// The Demeanor of the first NPC of `kind`, as the snapshot wire carries it.
fn wire_demeanor(sim: &Sim, kind: NpcKind) -> protocol::types::Demeanor {
    sim.overworld()
        .snapshot_states()
        .into_values()
        .find_map(|e| match e {
            sim::wire::EntityWire::Npc { kind: k, demeanor, .. } if k == kind => Some(demeanor),
            _ => None,
        })
        .expect("npc on the wire")
}

/// Demeanor is an authoritative snapshot fact: a fresh NPC reads Calm (it has
/// committed to nothing yet); a hunting wolf reads Aggressive and the deer it
/// chases reads Fleeing.
#[test]
fn snapshot_carries_the_npcs_demeanor() {
    use protocol::types::Demeanor;
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives { hunger: 0.8, ..Default::default() });
    sim.spawn_npc(NpcKind::Deer, pos(8_800, 8_000), Drives::default());

    assert_eq!(wire_demeanor(&sim, NpcKind::Wolf), Demeanor::Calm, "born calm");
    assert_eq!(wire_demeanor(&sim, NpcKind::Deer), Demeanor::Calm, "born calm");

    for _ in 0..5 {
        sim.tick();
    }
    assert_eq!(wire_demeanor(&sim, NpcKind::Wolf), Demeanor::Aggressive, "hunting wolf");
    assert_eq!(wire_demeanor(&sim, NpcKind::Deer), Demeanor::Fleeing, "hunted deer");
}

/// The wire velocity magnitude of the first NPC of `kind`.
fn wire_speed(sim: &Sim, kind: NpcKind) -> f64 {
    sim.overworld()
        .snapshot_states()
        .into_values()
        .find_map(|e| match e {
            sim::wire::EntityWire::Npc { kind: k, vx, vy, .. } if k == kind => {
                Some((vx * vx + vy * vy).sqrt())
            }
            _ => None,
        })
        .expect("npc on the wire")
}

/// A calm wolf ambles; an aggressive one runs. The lope/charge contrast is
/// observable on the wire, so the client's Mirror integrates it too.
#[test]
fn calm_wolf_wanders_slower_than_it_hunts() {
    // A lone, sated wolf: nothing to hunt, nothing to fear → Calm wander.
    let mut calm = Sim::new();
    calm.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives::default());
    calm.tick();
    let amble = wire_speed(&calm, NpcKind::Wolf);
    assert!(amble > 0.0, "a wandering wolf moves");

    // A hungry wolf with prey in sight → Aggressive chase.
    let mut hunt = Sim::new();
    hunt.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives { hunger: 0.8, ..Default::default() });
    hunt.spawn_npc(NpcKind::Deer, pos(8_800, 8_000), Drives::default());
    hunt.tick();
    let charge = wire_speed(&hunt, NpcKind::Wolf);

    assert!(
        amble * 1.5 < charge,
        "a calm wolf should amble well below its hunting speed ({amble} vs {charge})"
    );
}

/// A grazing bout sates the deer; a sated deer wanders; metabolism brings it
/// back to the grass. The rhythm is observable as the Demeanor alternating
/// Feeding → Calm → Feeding, and needs no state beyond hunger itself.
#[test]
fn deer_alternates_grazing_bouts_with_wandering() {
    use protocol::types::Demeanor;
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Deer, pos(8_000, 8_000), Drives { hunger: 0.3, ..Default::default() });

    // ~20 s of sim time, sampling the wire Demeanor each tick.
    let mut seq = Vec::new();
    for _ in 0..400 {
        sim.tick();
        seq.push(wire_demeanor(&sim, NpcKind::Deer));
    }

    assert!(seq.contains(&Demeanor::Feeding), "the deer grazes");
    assert!(seq.contains(&Demeanor::Calm), "the deer wanders between meals");
    let bouts = seq
        .windows(2)
        .filter(|w| w[0] != Demeanor::Feeding && w[1] == Demeanor::Feeding)
        .count();
    assert!(bouts >= 2, "grazing should recur after wandering (got {bouts} bout starts)");
}

/// A calm deer ambles; a hunted one springs. Same lope/charge contrast the
/// wolf has, observable on the wire.
#[test]
fn calm_deer_wanders_slower_than_it_flees() {
    // A lone, sated deer on grass-poor ground: nothing urgent → Calm wander.
    let mut calm = Sim::new();
    calm.spawn_npc(NpcKind::Deer, pos(8_000, 8_000), Drives::default());
    calm.tick();
    let amble = wire_speed(&calm, NpcKind::Deer);
    assert!(amble > 0.0, "a wandering deer moves");

    // The same deer with a wolf on it → Fleeing at full spring.
    let mut hunted = Sim::new();
    hunted.spawn_npc(NpcKind::Deer, pos(8_800, 8_000), Drives::default());
    hunted.spawn_npc(NpcKind::Wolf, pos(8_000, 8_000), Drives { hunger: 0.8, ..Default::default() });
    hunted.tick();
    let spring = wire_speed(&hunted, NpcKind::Deer);

    assert!(
        amble * 1.5 < spring,
        "a calm deer should amble well below its flee speed ({amble} vs {spring})"
    );
}

/// A graze bout, once begun, persists until the deer is properly sated — not
/// just until hunger dips back under the start threshold (which would make
/// every recurring bout a single threshold-edge nibble). Recurring bouts —
/// those started by metabolism at the threshold, not by the seeded hunger —
/// must each span a sustained stretch of Feeding.
#[test]
fn graze_bouts_persist_until_sated() {
    use protocol::types::Demeanor;
    let mut sim = Sim::new();
    sim.spawn_npc(NpcKind::Deer, pos(8_000, 8_000), Drives { hunger: 0.1, ..Default::default() });

    let mut seq = Vec::new();
    for _ in 0..400 {
        sim.tick();
        seq.push(wire_demeanor(&sim, NpcKind::Deer));
    }

    // Completed Feeding runs after the first wander (a trailing, still-open
    // bout at the window edge is dropped — only finished bouts are judged).
    let first_calm = seq.iter().position(|d| *d == Demeanor::Calm).expect("the deer wanders");
    let mut runs: Vec<usize> = Vec::new();
    let mut run = 0;
    for d in &seq[first_calm..] {
        if *d == Demeanor::Feeding {
            run += 1;
        } else if run > 0 {
            runs.push(run);
            run = 0;
        }
    }
    assert!(!runs.is_empty(), "grazing recurs after wandering");
    assert!(
        runs.iter().all(|r| *r >= 20),
        "every recurring bout should sate the deer (~1s+), got runs of {runs:?} ticks"
    );
}
