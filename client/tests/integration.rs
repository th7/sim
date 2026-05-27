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
