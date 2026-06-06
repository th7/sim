//! Phase 1b: game verbs (harvest/build/damage) and Instance entry/exit, with
//! reply semantics matching the Elixir implementation's error reasons and order.

use sim::components::{Inventory, Item, Position, StructureKind, WireId};
use sim::geometry::ChunkCoord;
use sim::ids::Realm;
use sim::sim::Sim;
use sim::verbs::VerbError;
use sim::wire::{entity_states, EntityWire};

fn at(x: i64, y: i64) -> Position {
    Position { x, y }
}

fn with_wood(n: u32) -> Inventory {
    let mut inv = Inventory::default();
    inv.items.insert(Item::Wood, n);
    inv
}

// The verb *logic* (range, depletion, materials, footprint, instance gate) lives
// on the realm. These helpers exercise it directly and return its `Result`, the
// natural layer for the error-reason assertions below. (Players send these as
// fire-and-forget intents over the wire; that path — enqueue + tick + async
// outcome — is covered by the sim/stories suites.)
fn harvest(sim: &mut Sim, who: &str, x: i64, y: i64) -> Result<(), VerbError> {
    let realm = sim.realm_of(who).ok_or(VerbError::NoPlayer)?;
    let clock = sim.clock_ms();
    sim.realm_world_mut(realm).ok_or(VerbError::NoChunk)?.harvest(who, x, y, clock).map(|_| ())
}
fn build(sim: &mut Sim, who: &str, kind: StructureKind, x: i64, y: i64) -> Result<(), VerbError> {
    let realm = sim.realm_of(who).ok_or(VerbError::NoPlayer)?;
    sim.realm_world_mut(realm).ok_or(VerbError::NoChunk)?.build(who, kind, x, y).map(|_| ())
}
fn damage(sim: &mut Sim, who: &str, x: i64, y: i64) -> Result<(), VerbError> {
    let realm = sim.realm_of(who).ok_or(VerbError::NoPlayer)?;
    let clock = sim.clock_ms();
    sim.realm_world_mut(realm).ok_or(VerbError::NoChunk)?.damage(who, x, y, clock).map(|_| ())
}

#[test]
fn harvest_yields_wood_and_depletes_then_respawns() {
    let mut sim = Sim::new();
    // Spawn on the center tree at (8000,8000) in chunk (0,0).
    sim.connect_at("alice", at(8_000, 8_000), Inventory::default());

    assert_eq!(harvest(&mut sim, "alice", 8_000, 8_000), Ok(()));
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&1));

    // Node is now depleted.
    let states = entity_states(sim.overworld());
    match states.get(&WireId("tree:8000:8000".into())) {
        Some(EntityWire::Node { depleted, .. }) => assert!(*depleted),
        other => panic!("expected depleted node, got {other:?}"),
    }
    // Re-harvesting a depleted node fails.
    assert_eq!(harvest(&mut sim, "alice", 8_000, 8_000), Err(VerbError::Depleted));

    // After RESPAWN_MS (30s = 600 ticks) it is gatherable again.
    for _ in 0..600 {
        sim.tick();
    }
    assert_eq!(harvest(&mut sim, "alice", 8_000, 8_000), Ok(()));
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&2));
}

#[test]
fn harvest_errors() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(8_000, 8_000), Inventory::default());

    // Far-away target → too_far (checked before target existence).
    assert_eq!(harvest(&mut sim, "alice", 50_000, 50_000), Err(VerbError::TooFar));
    // In-range empty cell → no_target.
    assert_eq!(harvest(&mut sim, "alice", 8_010, 8_010), Err(VerbError::NoTarget));
    // Unknown player → no_player.
    assert_eq!(harvest(&mut sim, "ghost", 0, 0), Err(VerbError::NoPlayer));
}

#[test]
fn build_places_wall_and_spends_wood() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(8_000, 8_000), with_wood(7));

    // Build on a clear cell in the player's chunk.
    assert_eq!(build(&mut sim, "alice", StructureKind::Wall, 3_000, 3_000), Ok(()));
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&2));

    // The structure appears on the wire with full HP.
    let states = entity_states(sim.overworld());
    match states.get(&WireId("structure:3000:3000".into())) {
        Some(EntityWire::Structure { hp, owner, .. }) => {
            assert_eq!(*hp, 100);
            assert_eq!(owner, "alice");
        }
        other => panic!("expected structure, got {other:?}"),
    }
}

#[test]
fn build_errors() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(8_000, 8_000), with_wood(4));

    // Not enough wood (have 4, need 5).
    assert_eq!(
        build(&mut sim, "alice", StructureKind::Wall, 3_000, 3_000),
        Err(VerbError::InsufficientMaterials)
    );

    // Out of the player's chunk (chunk 1).
    let mut rich = Sim::new();
    rich.connect_at("bob", at(8_000, 8_000), with_wood(10));
    assert_eq!(
        build(&mut rich, "bob", StructureKind::Wall, 20_000, 20_000),
        Err(VerbError::OutOfChunk)
    );

    // Footprint blocked: building on the center tree.
    assert_eq!(
        build(&mut rich, "bob", StructureKind::Wall, 8_000, 8_000),
        Err(VerbError::FootprintBlocked)
    );
}

#[test]
fn damage_reduces_hp_and_destroys_at_zero() {
    let mut sim = Sim::new();
    // Stand at the wall's west contact point: 800 sub-units from the wall
    // center (= body radius 300 + half-width 500), so the build is not
    // footprint-blocked yet the wall is inside the 1000-sub-unit interact range.
    sim.connect_at("alice", at(2_700, 3_000), with_wood(5));
    assert_eq!(build(&mut sim, "alice", StructureKind::Wall, 3_500, 3_000), Ok(()));

    // 100 HP, 25/hit → 4 hits to destroy. First three leave it standing.
    for _ in 0..3 {
        assert_eq!(damage(&mut sim, "alice", 3_500, 3_000), Ok(()));
    }
    let states = entity_states(sim.overworld());
    match states.get(&WireId("structure:3500:3000".into())) {
        Some(EntityWire::Structure { hp, .. }) => assert_eq!(*hp, 25),
        other => panic!("expected structure at 25hp, got {other:?}"),
    }
    // Fourth hit destroys it.
    assert_eq!(damage(&mut sim, "alice", 3_500, 3_000), Ok(()));
    let states = entity_states(sim.overworld());
    assert!(!states.contains_key(&WireId("structure:3500:3000".into())));
    // Now no target there.
    assert_eq!(damage(&mut sim, "alice", 3_500, 3_000), Err(VerbError::NoTarget));
}

#[test]
fn damage_too_far() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(2_700, 3_000), with_wood(5));
    build(&mut sim, "alice", StructureKind::Wall, 3_500, 3_000).unwrap();
    // A player far from the wall cannot damage it.
    sim.connect_at("bob", at(10_000, 10_000), Inventory::default());
    assert_eq!(damage(&mut sim, "bob", 3_500, 3_000), Err(VerbError::TooFar));
}

#[test]
fn portal_entry_and_exit_round_trip() {
    let mut sim = Sim::new();
    // Spawn overlapping the entry portal at (4000,4000) in chunk (0,0).
    sim.connect_at("alice", at(4_400, 4_000), Inventory::default());
    assert_eq!(sim.realm_of("alice"), Some(Realm::Overworld));

    // One tick: process_portals detects the overlap → enter Instance.
    sim.tick();
    let realm = sim.realm_of("alice").unwrap();
    assert!(matches!(realm, Realm::Instance(_)), "entered an instance");
    assert_eq!(sim.instance_count(), 1);
    // Spawned one unit west of the return portal (24000,24000).
    assert_eq!(sim.position("alice"), Some(at(23_000, 24_000)));

    // A relocated event was queued.
    let events = sim.drain_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, sim::sim::OutboundEvent::Relocated { coord, .. } if *coord == ChunkCoord::new(1, 1))));

    // Building in an Instance is refused.
    assert_eq!(build(&mut sim, "alice", StructureKind::Wall, 23_000, 24_000), Err(VerbError::NoBuildInInstance));

    // Walk east onto the return portal → exit back to the Overworld.
    sim.set_intent("alice", 1.0, 0.0);
    let mut exited = false;
    for _ in 0..20 {
        sim.tick();
        if sim.realm_of("alice") == Some(Realm::Overworld) {
            exited = true;
            break;
        }
    }
    assert!(exited, "stepping on the return portal exits the instance");
    // Re-emerged one unit west of the entry portal, instance destroyed.
    assert_eq!(sim.position("alice"), Some(at(3_000, 4_000)));
    assert_eq!(sim.instance_count(), 0);
}

#[test]
fn instance_movement_is_bounded() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(4_400, 4_000), Inventory::default());
    sim.tick(); // enter instance at (23000,24000)
    assert!(matches!(sim.realm_of("alice"), Some(Realm::Instance(_))));

    // Walk west hard into the wall; position clamps at x>=0, never negative.
    sim.set_intent("alice", -1.0, 0.0);
    for _ in 0..300 {
        sim.tick();
        // If we step on the return portal we'd exit; we're walking away from it,
        // so we stay inside.
        if sim.realm_of("alice") == Some(Realm::Overworld) {
            break;
        }
        let p = sim.position("alice").unwrap();
        assert!(p.x >= 0, "instance movement stays in bounds");
    }
}

/// Depleted-but-not-respawned trees keep their footprint by design — walls
/// can't be placed *on* the cluster (the existing `build_errors` pins that for
/// live trees, and depleted ones behave the same way) but they *can* be placed
/// in an adjacent cell. After harvesting the five worldgen trees at chunk
/// centre, walk clear of the cluster and drop a wall one tile north of the NE
/// tree — the still-solid footprints stop 300 sub-units out, so a wall whose
/// centre is 1000 from any tree centre fits.
#[test]
fn a_wall_can_be_built_next_to_the_depleted_cluster() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(8_000, 8_000), Inventory::default());

    // Harvest the cluster → 5 wood; all five footprints remain solid.
    for (dx, dy) in [(500, 500), (500, -500), (-500, 500), (-500, -500), (0, 0)] {
        harvest(&mut sim, "alice", 8_000 + dx, 8_000 + dy).unwrap();
    }
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&5));

    // Walk north of the cluster (alice is grandfathered through the still-solid
    // footprints while overlapping any of them), then a hair west so her body
    // clears the wall's AABB by more than her body radius.
    sim.set_intent("alice", 0.0, -1.0);
    while sim.position("alice").unwrap().y > 6_500 {
        sim.tick();
    }
    sim.set_intent("alice", -1.0, 0.0);
    while sim.position("alice").unwrap().x > 7_600 {
        sim.tick();
    }
    sim.set_intent("alice", 0.0, 0.0);

    // Now place the wall one tile north of the NE depleted tree (8500, 7500).
    assert_eq!(build(&mut sim, "alice", StructureKind::Wall, 8_500, 6_500), Ok(()));
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&0));
}
