//! Phase 3: persistence behaviours — reconnect resume, structure/depletion
//! survival across a (modelled) restart, and mid-Instance disconnect resume.

use sim::components::{Inventory, Item, Position, StructureKind, WireId};
use sim::geometry::ChunkCoord;
use sim::ids::Realm;
use sim::sim::Sim;
use sim::wire::{entity_states, EntityWire};

fn at(x: i64, y: i64) -> Position {
    Position { x, y }
}

#[test]
fn reconnect_resumes_position_and_inventory() {
    let mut sim = Sim::new();
    // Spawn on a tree, harvest a wood, walk east a bit.
    sim.connect_at("alice", at(8_000, 8_000), Inventory::default());
    sim.harvest("alice", 8_000, 8_000).unwrap();
    sim.set_intent("alice", 1.0, 0.0);
    for _ in 0..5 {
        sim.tick();
    }
    let saved_pos = sim.position("alice").unwrap();
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&1));

    sim.disconnect("alice");
    assert_eq!(sim.position("alice"), None);

    // Reconnect at the same chunk → resume exact position + inventory.
    sim.connect("alice", saved_pos.chunk());
    assert_eq!(sim.position("alice"), Some(saved_pos));
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&1));
}

#[test]
fn reconnect_to_different_chunk_keeps_inventory_spawns_at_center() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(8_000, 8_000), Inventory::default());
    sim.harvest("alice", 8_000, 8_000).unwrap();
    sim.disconnect("alice");

    // Reconnect declaring a different chunk → spawn at that chunk's center, but
    // keep the saved inventory.
    sim.connect("alice", ChunkCoord::new(5, 5));
    assert_eq!(sim.position("alice"), Some(at(5 * 16_000 + 8_000, 5 * 16_000 + 8_000)));
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&1));
}

#[test]
fn structure_survives_restart() {
    let store = {
        let mut sim = Sim::new();
        sim.connect_at("alice", at(2_700, 3_000), {
            let mut i = Inventory::default();
            i.items.insert(Item::Wood, 5);
            i
        });
        sim.build("alice", StructureKind::Wall, 3_500, 3_000).unwrap();
        sim.into_store() // flushes pending → durable
    };

    // Restart from the durable store; a fresh player hydrates chunk (0,0).
    let mut sim2 = Sim::with_persistence(store);
    sim2.connect_at("bob", at(2_000, 3_000), Inventory::default());
    let states = entity_states(sim2.overworld());
    match states.get(&WireId("structure:3500:3000".into())) {
        Some(EntityWire::Structure { hp, owner, .. }) => {
            assert_eq!(*hp, 100);
            assert_eq!(owner, "alice");
        }
        other => panic!("structure should survive restart, got {other:?}"),
    }
}

#[test]
fn destroyed_structure_stays_gone_after_restart() {
    let store = {
        let mut sim = Sim::new();
        sim.connect_at("alice", at(2_700, 3_000), {
            let mut i = Inventory::default();
            i.items.insert(Item::Wood, 5);
            i
        });
        sim.build("alice", StructureKind::Wall, 3_500, 3_000).unwrap();
        // 100 hp / 25 → 4 hits.
        for _ in 0..4 {
            sim.damage("alice", 3_500, 3_000).unwrap();
        }
        sim.into_store()
    };

    let mut sim2 = Sim::with_persistence(store);
    sim2.connect_at("bob", at(2_000, 3_000), Inventory::default());
    let states = entity_states(sim2.overworld());
    assert!(
        !states.contains_key(&WireId("structure:3500:3000".into())),
        "a destroyed structure (tombstone) must not reappear after restart"
    );
}

#[test]
fn depletion_survives_restart() {
    let store = {
        let mut sim = Sim::new();
        sim.connect_at("alice", at(8_000, 8_000), Inventory::default());
        sim.harvest("alice", 8_000, 8_000).unwrap(); // depletes tree:8000:8000
        sim.into_store()
    };

    let mut sim2 = Sim::with_persistence(store);
    sim2.connect_at("bob", at(8_000, 8_000), Inventory::default());
    let states = entity_states(sim2.overworld());
    match states.get(&WireId("tree:8000:8000".into())) {
        Some(EntityWire::Node { depleted, .. }) => {
            assert!(*depleted, "depletion should survive restart");
        }
        other => panic!("expected depleted tree, got {other:?}"),
    }
}

#[test]
fn idle_chunk_deactivates_then_rehydrates_from_persistence() {
    let mut sim = Sim::new();
    let mut inv = Inventory::default();
    inv.items.insert(Item::Wood, 5);
    sim.connect_at("alice", at(2_700, 3_000), inv);
    sim.build("alice", StructureKind::Wall, 3_500, 3_000).unwrap();

    // A few ticks with alice present keep chunk (0,0) hot.
    for _ in 0..5 {
        sim.tick();
    }
    assert!(sim.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0)).0, "hot while owned");

    // Disconnect, then idle past the 5s deactivation window.
    sim.disconnect("alice");
    for _ in 0..110 {
        sim.tick();
    }
    let (hot, count) = sim.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0));
    assert!(!hot, "chunk goes cold once unowned past the idle window");
    assert_eq!(count, 0, "static content unloaded");

    // Reconnect → chunk re-hydrates; the persisted wall comes back.
    sim.connect("alice", ChunkCoord::new(0, 0));
    assert!(sim.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0)).0, "hot again");
    let states = entity_states(sim.overworld());
    assert!(
        states.contains_key(&WireId("structure:3500:3000".into())),
        "wall re-hydrated from persistence after deactivation"
    );
    // And worldgen content is back too.
    assert!(states.contains_key(&WireId("tree:8000:8000".into())));
}

#[test]
fn mid_instance_disconnect_resumes_west_of_entry_portal() {
    let mut sim = Sim::new();
    // Enter the instance via the entry portal at (4000,4000).
    sim.connect_at("alice", at(4_400, 4_000), Inventory::default());
    sim.tick();
    assert!(matches!(sim.realm_of("alice"), Some(Realm::Instance(_))));

    // Disconnect mid-Instance.
    sim.disconnect("alice");
    assert_eq!(sim.instance_count(), 0, "empty instance torn down");

    // Reconnect at the entry chunk → resume one unit west of the entry portal
    // (3000,4000), NOT on the portal (which would loop straight back in).
    sim.connect("alice", ChunkCoord::new(0, 0));
    assert_eq!(sim.realm_of("alice"), Some(Realm::Overworld));
    assert_eq!(sim.position("alice"), Some(at(3_000, 4_000)));
}
