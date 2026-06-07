//! Cross-restart durability via the Postgres `DurableStore`, at the integration
//! level (no browser). Gated on `SIM_TEST_DATABASE_URL` so the normal
//! `cargo test` run does not require a database; when unset, the test self-skips.
//!
//! Run with, e.g.:
//!   SIM_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/sim_rust_test \
//!     cargo test --test pg_restart -- --nocapture

use sim::components::{Inventory, Item, Position, StructureKind, WireId};
use sim::datastore::DurableStore;
use sim::geometry::ChunkCoord;
use sim::pgstore::PgStore;
use sim::sim::{Action, Sim};
use sim::wire::{entity_states, EntityWire};
use std::time::{SystemTime, UNIX_EPOCH};

fn pg_url() -> Option<String> {
    std::env::var("SIM_TEST_DATABASE_URL").ok().filter(|s| !s.is_empty())
}

#[test]
fn cross_restart_durability_via_postgres() {
    let Some(url) = pg_url() else {
        eprintln!("skip: set SIM_TEST_DATABASE_URL to run the Postgres cross-restart test");
        return;
    };

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let user = format!("pgrt-{nonce}");

    // Session 1: connect, build a wall (spends 5 wood), flush, then "shut down"
    // (drop the Sim + store).
    {
        let mut store = PgStore::connect(&url).expect("connect pg");
        // Clear our keys in case a previous run left them.
        store.delete_structure(3_500, 3_000);
        let mut sim = Sim::with_store(store);

        let mut inv = Inventory::default();
        inv.items.insert(Item::Wood, 5);
        sim.connect_at(&user, Position { x: 2_700, y: 3_000 }, inv);
        sim.enqueue_action(&user, Action::Build { kind: StructureKind::Wall, x: 3_500, y: 3_000 }, 0, 0);
    sim.tick();
        assert_eq!(sim.inventory_of(&user).unwrap().items.get(&Item::Wood).copied().unwrap_or(0), 0);

        sim.flush_now(); // durable before "restart"
    }

    // Session 2: a fresh Sim over the same database (a process restart) resumes
    // the player and rehydrates the wall.
    {
        let store = PgStore::connect(&url).expect("reconnect pg");
        let mut sim = Sim::with_store(store);
        sim.connect(&user, ChunkCoord::new(0, 0));

        assert_eq!(
            sim.position(&user),
            Some(Position { x: 2_700, y: 3_000 }),
            "player position survives restart"
        );
        assert_eq!(
            sim.inventory_of(&user).unwrap().items.get(&Item::Wood).copied().unwrap_or(0),
            0,
            "spent inventory survives restart"
        );

        let states = entity_states(sim.overworld());
        match states.get(&WireId("structure:3500:3000".into())) {
            Some(EntityWire::Structure { hp, owner, .. }) => {
                assert_eq!(*hp, 100);
                assert_eq!(owner, &user);
            }
            other => panic!("wall should rehydrate from Postgres, got {other:?}"),
        }
    }

    // Tidy: remove the structure row we created.
    let mut store = PgStore::connect(&url).expect("cleanup connect");
    store.delete_structure(3_500, 3_000);
}
