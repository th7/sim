//! Dev-overlay telemetry. Assembles the `dev:stats` payload — the chunk
//! lifecycle ring around a watched player plus the global counters — purely from
//! the Sim's public queries. Read-only: no gameplay logic depends on anything
//! here, so the dev tooling can grow or be dropped without touching the player
//! path. The async `dev:stats` push loop lives in `transport`; this module owns
//! only *what* the stats are.

use crate::geometry::{coord_for, neighborhood, ChunkCoord};
use crate::sim::Sim;
use protocol::wire::{ChunkLifecycle, ChunkStatWire, StatsPayload};
use serde_json::Value;

/// Build the `stats` payload for the dev overlay, centred on `dev_username`'s
/// chunk: a 7×7 lifecycle ring in the Overworld, 3×3 inside an Instance, plus
/// the global active-chunk and player counts.
pub fn stats_payload(sim: &Sim, dev_username: Option<&str>) -> Value {
    let around = dev_username
        .and_then(|u| Some((u, sim.realm_of(u)?)))
        .map(|(u, realm)| {
            let radius = if realm.is_overworld() { 3 } else { 1 };
            let center =
                sim.position(u).map(|p| coord_for(p.x, p.y)).unwrap_or(ChunkCoord::new(0, 0));
            let now = sim.clock_ms();
            let rw = sim.realm_world(realm);
            neighborhood(center, radius)
                .into_iter()
                .map(|c| {
                    let (lifecycle, idle_ms_remaining) =
                        rw.map_or((ChunkLifecycle::Cold, None), |rw| rw.chunk_lifecycle(c, now));
                    let entity_count = rw.map_or(0, |rw| rw.entity_count_in(c)) as u64;
                    ChunkStatWire { cx: c.cx, cy: c.cy, lifecycle, idle_ms_remaining, entity_count }
                })
                .collect()
        })
        .unwrap_or_default();
    let payload = StatsPayload {
        active_chunks: sim.active_chunk_count() as u64,
        total_players: sim.player_count() as u64,
        around,
    };
    serde_json::to_value(payload).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Position;

    fn around(v: &Value, cx: i32, cy: i32) -> (String, Value) {
        let e = v["around"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["cx"] == cx && e["cy"] == cy)
            .unwrap_or_else(|| panic!("({cx},{cy}) not in ring"));
        (e["lifecycle"].as_str().unwrap().to_string(), e["idle_ms_remaining"].clone())
    }

    #[test]
    fn shape_and_hot_centre() {
        let mut sim = Sim::new();
        sim.connect_at("dev", Position { x: 8_000, y: 8_000 }, Default::default());
        let v = stats_payload(&sim, Some("dev"));
        assert!(v["active_chunks"].as_u64().unwrap() >= 1);
        assert_eq!(v["total_players"], 1);
        // 7×7 ring in the Overworld, centred on the player's chunk.
        assert_eq!(v["around"].as_array().unwrap().len(), 49);
        // The player's own chunk is owned → hot, no countdown.
        let (life, idle) = around(&v, 0, 0);
        assert_eq!(life, "hot");
        assert!(idle.is_null());
    }

    #[test]
    fn a_chunk_cools_hot_then_idle_armed_then_cold() {
        let mut sim = Sim::new();
        sim.connect_at("dev", Position { x: 8_000, y: 8_000 }, Default::default());
        // Chunk (-1,0) is in the player's starting 3×3 footprint → hot.
        assert_eq!(around(&stats_payload(&sim, Some("dev")), -1, 0).0, "hot");

        // Walk east into chunk (1,0); the footprint slides off (-1,0), which stays
        // loaded — so it's now idle-armed, counting down to unload.
        sim.set_intent("dev", 1.0, 0.0);
        for _ in 0..50 {
            sim.tick();
        }
        sim.set_intent("dev", 0.0, 0.0);
        let (life, idle) = around(&stats_payload(&sim, Some("dev")), -1, 0);
        assert_eq!(life, "idle_armed");
        let rem = idle.as_i64().expect("idle_armed carries a countdown");
        assert!((0..=5_000).contains(&rem), "remaining {rem} within the idle window");

        // After the idle timeout elapses with the player parked, (-1,0) unloads.
        for _ in 0..120 {
            sim.tick();
        }
        assert_eq!(around(&stats_payload(&sim, Some("dev")), -1, 0).0, "cold");
    }
}
