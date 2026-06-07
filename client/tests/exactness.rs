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

/// The named tail, pinned: under network jitter speculation *does* diverge —
/// and authority pulls it back. Frames reach the server in a late burst (long
/// enough that its perishable Intent expires and the server stalls while the
/// Mirror keeps walking), and authority reaches the Mirror late and sparsely.
/// The promises under test:
///   1. divergence actually occurs (the test is not vacuous),
///   2. divergence never exceeds what the Lead bound allows,
///   3. once the player rests and authority catches up, Mirror and server
///      agree exactly — bit-identical, no residual drift, regardless of the
///      jitter history.
#[test]
fn diverges_under_jitter_and_reconverges_exactly_at_rest() {
    use protocol::consts::LEAD_BOUND_TICKS;

    let mut sim = Sim::new();
    sim.connect_at("alice", Position { x: 6_000, y: 7_500 }, Default::default());

    let coord = ChunkCoord::new(0, 0);
    let mut mirror = Mirror::new("alice");
    let snap = chunk_snapshot(&sim.overworld().snapshot_states(), coord, sim.tick_count());
    mirror.on_snapshot(coord, &snap);

    // The wire, with jitter: frames the server hasn't received yet, keyed by
    // the tick they arrive on.
    let mut in_flight: Vec<(u64, u32, f64, f64)> = Vec::new(); // (arrives_at, seq, dx, dy)
    let mut deliver = |sim: &mut Sim, in_flight: &mut Vec<(u64, u32, f64, f64)>| {
        let now = sim.tick_count();
        in_flight.retain(|&(at, seq, dx, dy)| {
            if at <= now {
                sim.enqueue_move("alice", seq, dx, dy);
                false
            } else {
                true
            }
        });
    };

    // Authority is delayed too: snapshots + acks captured at broadcast ticks
    // arrive at the Mirror 3 ticks stale — so every override lands with real
    // lead and genuinely replays unacked frames. It flows through every phase
    // (walk, jitter, rest), as the 10 Hz broadcast does in reality.
    const AUTH_DELAY: u64 = 3;
    let mut auth_in_flight: Vec<(u64, u32, sim::wire::ChunkSnapshot)> = Vec::new();
    let coord_b = coord;
    let broadcast = move |sim: &mut Sim,
                          mirror: &mut Mirror,
                          auth_in_flight: &mut Vec<(u64, u32, sim::wire::ChunkSnapshot)>| {
        if sim.tick_count() % 2 == 0 {
            let snap = chunk_snapshot(&sim.overworld().snapshot_states(), coord_b, sim.tick_count());
            auth_in_flight.push((
                sim.tick_count() + AUTH_DELAY,
                sim.last_move_seq("alice").unwrap_or(0),
                snap,
            ));
        }
        let now = sim.tick_count();
        for (_, ack_seq, snap) in auth_in_flight.iter().filter(|&&(at, ..)| at <= now) {
            mirror.on_ack(*ack_seq, snap.tick);
            mirror.on_snapshot(coord_b, snap);
        }
        auth_in_flight.retain(|&(at, ..)| at > now);
    };

    let mut max_divergence = 0i64;
    let mut seq = 0u32;
    // Walk east for 24 ticks. Frames sent during ticks 5..12 are delayed by 6
    // ticks — past INTENT_GRACE_TICKS, so the server's Intent expires and it
    // stalls while the Mirror keeps walking.
    for i in 1..=24u64 {
        seq += 1;
        let delay = if (5..12).contains(&i) { 6 } else { 0 };
        in_flight.push((sim.tick_count() + 1 + delay, seq, 1.0, 0.0));
        mirror.push_input(seq, 1.0, 0.0);

        deliver(&mut sim, &mut in_flight);
        sim.tick();
        mirror.tick();
        broadcast(&mut sim, &mut mirror, &mut auth_in_flight);

        let server = sim.overworld().position_of("alice").unwrap();
        let (mx, _my) = mirror.position_of("alice").unwrap();
        max_divergence = max_divergence.max((mx - server.x).abs());
        // Promise 2: divergence is bounded by what the Lead allows.
        assert!(
            (mx - server.x).abs() <= 200 * LEAD_BOUND_TICKS as i64,
            "divergence exceeded the Lead bound's allowance at tick {i}"
        );
    }
    // Promise 1: the jitter genuinely forced a misprediction.
    assert!(max_divergence > 0, "the jitter pattern never diverged — vacuous test");

    // Walk north into open space before resting — an at-rest spot pressed
    // against the tree would let a buggy replay clamp into the same wall and
    // hide its error (this leg is what gives the final assertion teeth).
    for _ in 0..8 {
        seq += 1;
        in_flight.push((sim.tick_count() + 1, seq, 0.0, -1.0));
        mirror.push_input(seq, 0.0, -1.0);
        deliver(&mut sim, &mut in_flight);
        sim.tick();
        mirror.tick();
        broadcast(&mut sim, &mut mirror, &mut auth_in_flight);
    }

    // Rest: one zero-frame, then let the wire drain and stale authority catch
    // up. The final override still arrives AUTH_DELAY ticks stale, so the
    // at-rest agreement is reached through a real replay, not a trivial snap.
    seq += 1;
    in_flight.push((sim.tick_count() + 1, seq, 0.0, 0.0));
    mirror.push_input(seq, 0.0, 0.0);
    for _ in 0..12 {
        deliver(&mut sim, &mut in_flight);
        sim.tick();
        mirror.tick();
        broadcast(&mut sim, &mut mirror, &mut auth_in_flight);
    }

    // Promise 3: at rest, speculation and authority agree exactly.
    let server = sim.overworld().position_of("alice").unwrap();
    assert_eq!(
        mirror.position_of("alice"),
        Some((server.x, server.y)),
        "at rest the Mirror must be bit-identical to the server"
    );
}

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
