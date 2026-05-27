//! End-to-end client integration: boot the real sim server in-process and drive
//! the native client's Session over a real WebSocket, re-pinning the phase
//! behaviours (the browser Playwright suite's job) without a browser.

use client::session::Session;
use protocol::geometry::ChunkCoord;
use protocol::wire::RealmWire;
use std::time::Duration;

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
