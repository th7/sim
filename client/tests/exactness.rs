//! The keystone of the Mirror design: **exact replay**. The Mirror's
//! speculative own-player state — authoritative override at the acked tick
//! plus replay of unacked input frames through the shared integrator — is
//! bit-identical to what the server computes from the same frames, obstacles
//! included. Convergence is by construction, not by tuning: when nothing
//! external intervenes, there is never a correction to smooth.

use client::mirror::Mirror;
use protocol::geometry::ChunkCoord;
use sim::components::Position;
use sim::sim::Sim;
use sim::wire::chunk_snapshot;

/// Drive a real server and a Mirror with the same per-tick input frames on a
/// zero-latency wire (snapshot + ack delivered at broadcast cadence). At every
/// tick — between overrides and right after them — the Mirror's own position
/// must equal the server's exactly. The path walks into the worldgen tree
/// cluster, so `clamp_step` collision is exercised on both sides.
#[test]
fn override_plus_replay_is_bit_identical_to_the_server() {
    // Spawn clear of the worldgen tree cluster (a spawn inside a footprint is
    // grandfathered and would never clamp).
    let mut sim = Sim::new();
    sim.connect_at("alice", Position { x: 6_000, y: 7_500 }, Default::default());

    let coord = ChunkCoord::new(0, 0);
    let mut mirror = Mirror::new("alice");
    let snap = chunk_snapshot(&sim.overworld().snapshot_states(), coord, sim.tick_count());
    mirror.on_snapshot(coord, &snap);

    // Walk square into the tree at (7500,7500) — contact clamps x at 6900 —
    // then slide southeast along its flank, then pull away northeast.
    // Direction changes mid-flight exercise frame ordering; the clamp and the
    // slide exercise collision on both sides.
    let legs: [(f64, f64); 3] = [(1.0, 0.0), (0.7071, 0.7071), (0.7071, -0.7071)];
    let mut seq = 0u32;
    for (dx, dy) in legs {
        for _ in 0..8 {
            seq += 1;
            sim.enqueue_move("alice", seq, dx, dy);
            mirror.push_input(seq, dx, dy);
            sim.tick();
            mirror.tick();

            // Broadcast cadence: ack + snapshot every 2nd tick, like the wire.
            if sim.tick_count() % 2 == 0 {
                mirror.on_ack(seq, sim.tick_count());
                let snap =
                    chunk_snapshot(&sim.overworld().snapshot_states(), coord, sim.tick_count());
                mirror.on_snapshot(coord, &snap);
            }

            let server = sim.overworld().position_of("alice").unwrap();
            assert_eq!(
                mirror.position_of("alice"),
                Some((server.x, server.y)),
                "speculation diverged from the server at tick {} (seq {seq})",
                sim.tick_count(),
            );
        }
    }
}
