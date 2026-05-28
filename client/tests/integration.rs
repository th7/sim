//! End-to-end client integration: boot the real sim server in-process and drive
//! the native client's Session over a real WebSocket, re-pinning the phase
//! behaviours (the browser Playwright suite's job) without a browser.

use client::model::ClientModel;
use client::session::Session;
use protocol::geometry::ChunkCoord;
use protocol::wire::RealmWire;
use std::time::{Duration, Instant};

async fn start_server() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(sim::transport::serve(listener, sim::transport::Shared::new()));
    port
}

fn url(port: u16) -> String {
    format!("ws://127.0.0.1:{port}/socket/websocket?vsn=2.0.0")
}

const T: Duration = Duration::from_secs(5);

/// Is the instance's `out_of_instance` return portal currently in view?
fn return_portal_visible(m: &ClientModel) -> bool {
    m.portals().values().any(|p| p.direction == "out_of_instance")
}

/// Drive alice from her overworld spawn northwest onto the `into_instance`
/// portal at world (4,4) and wait for the realm switch, then release movement so
/// she ends up standing still inside a fresh instance.
async fn enter_instance(alice: &mut Session) {
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    assert_eq!(alice.model().realm(), RealmWire::Overworld);
    alice.movement(true, false, false, true).await.unwrap(); // northwest
    let entered = alice
        .pump_until(Duration::from_secs(15), |m| matches!(m.realm(), RealmWire::Instance { .. }))
        .await;
    alice.movement(false, false, false, false).await.unwrap();
    assert!(entered, "alice should relocate into an instance");
}

#[tokio::test]
async fn connect_and_see_self() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(
        alice.pump_until(T, |m| m.player_pos("alice").is_some()).await,
        "alice should appear in her own view after connecting"
    );
    // Spawned at chunk-(0,0) centre.
    let p = alice.model().player_pos("alice").unwrap();
    assert_eq!((p.x, p.y), (8_000, 8_000));
}

#[tokio::test]
async fn two_clients_see_each_other() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    let mut bob = Session::connect(&url(port), "bob", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("bob").is_some()).await, "alice sees bob");
    assert!(bob.pump_until(T, |m| m.player_pos("alice").is_some()).await, "bob sees alice");
}

#[tokio::test]
async fn movement_moves_the_player() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    let x0 = alice.model().player_pos("alice").unwrap().x;

    alice.movement(false, false, true, false).await.unwrap(); // east
    let moved = alice
        .pump_until(T, |m| m.player_pos("alice").map(|p| p.x > x0).unwrap_or(false))
        .await;
    assert!(moved, "walking east increases x");
}

#[tokio::test]
async fn harvest_yields_wood() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    // Wait for the chunk snapshot carrying the centre tree.
    assert!(
        alice.pump_until(T, |m| m.nodes().contains_key("tree:8000:8000")).await,
        "the centre tree should be visible"
    );
    // Alice spawns on the centre tree; click it.
    alice.click(8.0, 8.0).await.unwrap();
    assert!(
        alice.pump_until(T, |m| m.inventory().get("wood").copied().unwrap_or(0) >= 1).await,
        "harvesting the tree should yield wood (via the `self` event)"
    );
}

#[tokio::test]
async fn walking_across_multiple_chunk_boundaries_stays_visible() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);

    // Walk south past the first boundary, then far east across several more.
    alice.movement(false, true, false, false).await.unwrap(); // south
    assert!(
        alice.pump_until(T, |m| m.player_pos("alice").map(|p| p.y > 10_000).unwrap_or(false)).await
    );
    alice.movement(false, false, false, false).await.unwrap();

    // Walk east to x > 33u — across the boundaries at 16u and 32u — sampling as
    // we go. The window pans to keep alice's chunk subscribed, so she stays in
    // the merged view the whole way and her x never jumps backward.
    alice.movement(false, false, true, false).await.unwrap();
    let mut prev_x = alice.model().player_pos("alice").unwrap().x;
    let mut reached = false;
    for _ in 0..120 {
        alice.pump_for(Duration::from_millis(100)).await;
        let p = alice.model().player_pos("alice").expect("alice stays visible across boundaries");
        assert!(p.x + 1 >= prev_x, "x is monotonic (no backward glitch on a pan)");
        prev_x = p.x;
        if p.x > 33_000 {
            reached = true;
            break;
        }
    }
    alice.movement(false, false, false, false).await.unwrap();
    assert!(reached, "alice reaches x > 33u within the sampling budget");
    // She crossed into chunk x=2 and the view window followed her.
    assert_eq!(alice.model().window_center().cx, 2);
}

#[tokio::test]
async fn gather_build_and_destroy_a_wall() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    // The five worldgen trees around the chunk centre load in.
    assert!(alice.pump_until(T, |m| m.nodes().len() >= 5).await, "all five trees visible");

    // Chop all five (alice spawns within interact range of the cluster) → 5 wood.
    let (cx, cy) = (8_000_i64, 8_000_i64);
    for (dx, dy) in [(500, 500), (500, -500), (-500, 500), (-500, -500), (0, 0)] {
        alice.send_harvest(cx + dx, cy + dy).await.unwrap();
        alice.pump_for(Duration::from_millis(150)).await;
    }
    assert!(
        alice.pump_until(T, |m| m.inventory().get("wood").copied().unwrap_or(0) >= 5).await,
        "five trees yield five wood"
    );

    // Walk east, clear of the depleted-but-still-solid tree cluster, then settle.
    alice.movement(false, false, true, false).await.unwrap();
    assert!(
        alice.pump_until(T, |m| m.player_pos("alice").map(|p| p.x >= 10_500).unwrap_or(false)).await,
        "alice walks east out of the cluster"
    );
    alice.movement(false, false, false, false).await.unwrap();
    alice.pump_for(Duration::from_millis(500)).await;

    // Place the wall 1u east: its AABB clears alice's body and sits exactly at
    // interact range (mirrors the old phase-8 e2e's hand-computed placement).
    let me = alice.model().player_pos("alice").unwrap();
    let (wx, wy) = (me.x + 1_000, me.y);
    alice.send_build("wall", wx, wy).await.unwrap();

    assert!(alice.pump_until(T, |m| !m.structures().is_empty()).await, "the wall appears");
    let wall = alice.model().structures().values().next().cloned().unwrap();
    assert_eq!(wall.hp, 100);
    assert_eq!(wall.owner, "alice");
    assert_eq!(wall.kind, "wall");
    assert!(
        alice.pump_until(T, |m| m.inventory().get("wood").copied().unwrap_or(0) == 0).await,
        "the five wood is spent on the wall"
    );

    // Damage it to destruction: 4 clicks × 25 HP = 100.
    for _ in 0..4 {
        alice.send_damage(wx, wy).await.unwrap();
        alice.pump_for(Duration::from_millis(150)).await;
    }
    assert!(
        alice.pump_until(T, |m| m.structures().is_empty()).await,
        "the wall is destroyed after 100 damage"
    );
}

#[tokio::test]
async fn dev_mode_receives_stats() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    // No stats until dev mode is on.
    assert!(alice.model().stats().is_none());

    alice.set_dev(true).await.unwrap();
    // The server pushes dev:stats once per second to subscribers.
    let got = alice.pump_until(Duration::from_secs(3), |m| m.stats().is_some()).await;
    assert!(got, "enabling dev mode should start receiving dev:stats pushes");

    let stats = alice.model().stats().unwrap();
    assert!(stats.active_chunks >= 1, "alice's chunk is hot");
    assert!(stats.total_players >= 1);
    // The overlay ring is the 7×7 chunks centred on alice (chunk 0,0).
    assert_eq!(stats.around.len(), 49);
    assert!(stats.around.iter().any(|c| c.cx == 0 && c.cy == 0));

    // Turning dev off drops the cached stats.
    alice.set_dev(false).await.unwrap();
    assert!(alice.model().stats().is_none());
}

#[tokio::test]
async fn walking_into_the_portal_enters_an_instance() {
    let port = start_server().await;
    // chunk (0,0) holds the into_instance portal at world (4,4); spawn at (8,8).
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    assert!(alice.pump_until(T, |m| m.player_pos("alice").is_some()).await);
    assert_eq!(alice.model().realm(), RealmWire::Overworld);

    // Walk northwest toward the portal (west = -x, north = -y).
    alice.movement(true, false, false, true).await.unwrap();
    let entered = alice
        .pump_until(Duration::from_secs(15), |m| matches!(m.realm(), RealmWire::Instance { .. }))
        .await;
    alice.movement(false, false, false, false).await.unwrap();
    assert!(entered, "overlapping the portal relocates the player into an instance");
    // The client re-subscribed to the instance's chunks and sees the return portal.
    assert!(
        alice
            .pump_until(T, |m| m.portals().values().any(|p| p.direction == "out_of_instance"))
            .await,
        "the instance's return portal should be visible after the realm switch"
    );
}

// --- flicker regressions -----------------------------------------------------
//
// "Flicker" = a visible object blinks out and back while the player stays inside
// the instance. At the model layer that means an object disappears from the
// merged view (`portals()` / `players()`) on some broadcast and returns on a
// later one. These tests pin the invariant *do not flicker* by sampling the view
// at ~20 Hz (twice the 10 Hz broadcast rate, so no dropped broadcast can be
// skipped over) and asserting an object that has appeared never goes missing.
//
// The instance is a 3×3 chunk grid whose return portal sits in the centre chunk
// (1,1); the player is clamped to that grid, so she is never more than one chunk
// from centre. Hence (1,1) is *always* inside both her 3×3 view window and her
// cluster's footprint — the portal has no legitimate reason to ever leave view.
// If it does, that is the bug, and it is exercised end-to-end (real server, real
// client model over a real socket), so it catches the fault whether it lives in
// the server's per-chunk snapshot building or the client's subscription/merge.

/// Standing still just inside an instance, the return portal and the player must
/// stay continuously visible. Spans ~7 s — past the 5 s chunk idle-deactivation
/// timeout and ~70 broadcasts — so a per-broadcast drop *or* an idle-deactivation
/// of the centre chunk would surface as a missing object on some sample.
#[tokio::test]
async fn instance_objects_do_not_flicker_while_standing_still() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    enter_instance(&mut alice).await;

    // Let the instance's content load into view before we start watching.
    assert!(
        alice.pump_until(T, |m| return_portal_visible(m) && m.player_pos("alice").is_some()).await,
        "the return portal and alice should be visible once inside the instance"
    );

    let deadline = Instant::now() + Duration::from_secs(7);
    let mut samples = 0;
    while Instant::now() < deadline {
        alice.pump_for(Duration::from_millis(50)).await;
        samples += 1;
        assert!(
            return_portal_visible(alice.model()),
            "return portal vanished at sample {samples} — flicker while standing in the instance"
        );
        assert!(
            alice.model().player_pos("alice").is_some(),
            "player vanished at sample {samples} — flicker while standing in the instance"
        );
    }
    assert!(samples >= 50, "expected to sample many broadcast cycles, got {samples}");
}

/// Walking a circuit through the instance's chunks pans the view window (chunks
/// are subscribed and dropped), but the return portal in the centre chunk (1,1)
/// — and the player, who is always in her own window — must never blink out.
///
/// The circuit roams *away* from the return portal at (24000,24000): the player
/// spawns one unit west of it, so we head west/north/east/south through chunks
/// (0,1),(0,0),(1,0) and back, staying clear of the portal's overlap trigger (we
/// must not re-enter it and leave the instance) while keeping (1,1) in view the
/// whole way. Any disappearance is flicker, not a legitimate view change.
#[tokio::test]
async fn instance_objects_do_not_flicker_while_walking_around() {
    let port = start_server().await;
    let mut alice = Session::connect(&url(port), "alice", ChunkCoord::new(0, 0)).await.unwrap();
    enter_instance(&mut alice).await;
    assert!(
        alice.pump_until(T, |m| return_portal_visible(m)).await,
        "the return portal should be visible once inside the instance"
    );

    // (north, south, east, west, dwell_ms). From spawn (23000,24000): west into
    // chunk (0,1), north into (0,0), east along y≈8000 into (1,0) (far below the
    // portal), south into (1,1). ~4 world-units/sec keeps every leg inside the
    // 3×3 grid and well clear of the return portal.
    let legs = [
        (false, false, false, true, 3_000),
        (true, false, false, false, 4_000),
        (false, false, true, false, 4_000),
        (false, true, false, false, 2_000),
    ];
    let mut panned = false;
    for (n, s, e, w, dwell_ms) in legs {
        alice.movement(n, s, e, w).await.unwrap();
        let leg_end = Instant::now() + Duration::from_millis(dwell_ms);
        while Instant::now() < leg_end {
            alice.pump_for(Duration::from_millis(50)).await;
            assert!(
                matches!(alice.model().realm(), RealmWire::Instance { .. }),
                "the circuit must stay inside the instance (not re-enter the return portal)"
            );
            assert!(
                alice.model().player_pos("alice").is_some(),
                "player vanished while walking — flicker inside the instance"
            );
            assert!(
                return_portal_visible(alice.model()),
                "return portal flickered while walking inside the instance"
            );
            panned |= alice.model().window_center() != ChunkCoord::new(1, 1);
        }
    }
    alice.movement(false, false, false, false).await.unwrap();
    assert!(panned, "the circuit should have panned the view window off the centre chunk");
}
