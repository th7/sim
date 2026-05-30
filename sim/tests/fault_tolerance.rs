//! Fault tolerance: a panicking tick takes the whole runtime down (we do not
//! limp on in a presumed-corrupt state) — but on the way down it flushes durable
//! state, so loss is bounded to the unflushed window. These pin the "lose as
//! little as possible" guarantee; taking the process down itself is the
//! transport's job (`tick_or_flush` → abort).

use sim::components::{Inventory, Position};
use sim::geometry::ChunkCoord;
use sim::sim::Sim;

fn at(x: i64, y: i64) -> Position {
    Position { x, y }
}

/// `flush_now` must capture each standing player's *current* position, not the
/// last heartbeat — the heartbeat only runs every FLUSH_MS (100 ticks), so a
/// shutdown/crash in between would otherwise lose all movement since it.
#[test]
fn flush_now_captures_the_current_player_position() {
    let mut sim = Sim::new();
    sim.connect_at("ada", at(8_000, 12_000), Inventory::default());
    sim.set_intent("ada", 1.0, 0.0);
    for _ in 0..5 {
        sim.tick(); // well within the 100-tick heartbeat period
    }
    let walked = sim.position("ada").unwrap();
    assert!(walked.x > 8_000, "ada walked east");

    sim.flush_now(); // lose as little as possible
    let store = sim.into_store();

    // Model a restart from the durable store: ada resumes where she actually was.
    let mut restarted = Sim::with_persistence(store);
    restarted.connect("ada", ChunkCoord::new(0, 0));
    assert_eq!(
        restarted.position("ada"),
        Some(walked),
        "after flush_now + restart, ada resumes her walked position, not a stale heartbeat"
    );
}

/// The supervised tick returns `Ok` and advances normally when nothing panics.
#[test]
fn tick_or_flush_is_ok_on_a_normal_tick() {
    let mut sim = Sim::new();
    sim.connect_at("ada", at(8_000, 12_000), Inventory::default());
    let before = sim.tick_count();
    assert!(sim.tick_or_flush().is_ok(), "a normal tick reports healthy");
    assert_eq!(sim.tick_count(), before + 1, "the tick advanced");
}
