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

/// Tick up to `max` times, stopping as soon as `pred` holds; assert it did.
fn tick_until(sim: &mut Sim, max: usize, pred: impl Fn(&Sim) -> bool) {
    for _ in 0..max {
        if pred(sim) {
            return;
        }
        sim.tick();
    }
    assert!(pred(sim), "predicate not satisfied within {max} ticks");
}

/// True iff the resource node `wid` is present and depleted in the live world.
fn tree_depleted(sim: &Sim, wid: &str) -> bool {
    matches!(
        entity_states(sim.overworld()).get(&WireId(wid.into())),
        Some(EntityWire::Node { depleted: true, .. })
    )
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
fn reconnect_replaces_prior_live_session() {
    let mut sim = Sim::new();
    sim.connect_at("alice", at(8_000, 8_000), Inventory::default());
    sim.harvest("alice", 8_000, 8_000).unwrap(); // wood 1

    // A second connect for the same username (a reconnect race) must replace the
    // old session, not duplicate it.
    sim.connect("alice", ChunkCoord::new(0, 0));
    assert_eq!(sim.player_count(), 1, "exactly one live session for the username");
    // Resumed from the freshly-flushed live state: same chunk → exact position + wood.
    assert_eq!(sim.position("alice"), Some(at(8_000, 8_000)));
    assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&1));

    // Exactly one player entity is on the wire.
    let states = entity_states(sim.overworld());
    let players = states.values().filter(|s| matches!(s, EntityWire::Player { .. })).count();
    assert_eq!(players, 1, "no duplicate player entity left behind");
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
fn tree_depletion_survives_walking_away_until_chunk_stops() {
    // A connected Player harvests a tree, then *walks* far enough that the
    // cluster releases the tree's chunk and it idle-deactivates (stops). When
    // the Player walks back, the chunk restarts and the tree must still be
    // depleted — its state was persisted and rehydrated, not regenerated.
    let mut sim = Sim::new();
    sim.connect_at("alice", at(8_000, 8_000), Inventory::default());
    assert_eq!(sim.harvest("alice", 8_000, 8_000), Ok(()));

    // Sanity: the centre tree is depleted and its chunk is hot (owned).
    assert!(tree_depleted(&sim, "tree:8000:8000"));
    assert!(sim.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0)).0, "chunk hot while owned");

    // Step south off the tree row (still in chunk row cy=0), then walk far east
    // — past chunk (0,0)'s 3×3 footprint — so the cluster releases chunk (0,0).
    sim.set_intent("alice", 0.0, 1.0);
    tick_until(&mut sim, 60, |s| s.position("alice").unwrap().y > 10_500);
    sim.set_intent("alice", 1.0, 0.0);
    tick_until(&mut sim, 300, |s| s.position("alice").unwrap().chunk().cx >= 3);
    sim.set_intent("alice", 0.0, 0.0); // stop, far away

    // The chunk is unowned the moment the Player leaves its footprint; the
    // actual stop (unload) follows after the idle window. Wait for the unload,
    // signalled by the chunk's entities being gone.
    assert!(!sim.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0)).0, "chunk released");
    tick_until(&mut sim, 200, |s| s.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0)).1 == 0);
    let (hot, count) = sim.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0));
    assert!(!hot, "chunk (0,0) stopped after the Player walked away");
    assert_eq!(count, 0, "its tree was unloaded with the chunk");
    assert!(
        entity_states(sim.overworld()).get(&WireId("tree:8000:8000".into())).is_none(),
        "the depleted tree is gone from the live world while the chunk is cold"
    );

    // Walk back west until chunk (0,0) is owned again → it restarts/rehydrates.
    sim.set_intent("alice", -1.0, 0.0);
    tick_until(&mut sim, 400, |s| s.chunk_status(Realm::Overworld, ChunkCoord::new(0, 0)).0);
    sim.tick(); // let the persisted-state overlay reapply

    // The depletion was maintained across the chunk stop + restart (and we are
    // well within the 30s respawn window, so it has not regenerated).
    assert!(
        tree_depleted(&sim, "tree:8000:8000"),
        "harvested tree must still be depleted after its chunk stopped and restarted"
    );
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
