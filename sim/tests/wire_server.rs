//! End-to-end wire compatibility: start the real WebSocket server in-process and
//! drive it with a Phoenix Channels v2 client, exercising the same frames the
//! frontend sends. Proves the server is a drop-in for the Elixir socket.

use futures_util::{SinkExt, StreamExt};
use sim::phx::PhxMessage;
use sim::transport::{serve, Shared};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

type Ws = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn start_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(serve(listener, Shared::new()));
    port
}

async fn connect(port: u16) -> Ws {
    let url = format!("ws://127.0.0.1:{port}/socket/websocket?vsn=2.0.0");
    let (ws, _resp) = tokio_tungstenite::connect_async(url).await.expect("connect");
    ws
}

async fn send(ws: &mut Ws, join_ref: &str, reference: &str, topic: &str, event: &str, payload: serde_json::Value) {
    let m = PhxMessage {
        join_ref: Some(join_ref.into()),
        reference: Some(reference.into()),
        topic: topic.into(),
        event: event.into(),
        payload,
    };
    ws.send(Message::Text(m.encode())).await.unwrap();
}

/// Read frames until `pred` matches one, or time out.
async fn recv_until(ws: &mut Ws, pred: impl Fn(&PhxMessage) -> bool) -> PhxMessage {
    loop {
        let frame = timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("timed out waiting for frame")
            .expect("stream ended")
            .expect("ws error");
        if let Message::Text(t) = frame {
            if let Ok(m) = PhxMessage::decode(&t) {
                if pred(&m) {
                    return m;
                }
            }
        }
    }
}

#[tokio::test]
async fn full_session_over_the_wire() {
    let port = start_server().await;
    let mut ws = connect(port).await;

    // Join the player channel.
    send(&mut ws, "1", "1", "player:alice", "phx_join",
        serde_json::json!({"username":"alice","initial_chunk":[0,0]})).await;
    let reply = recv_until(&mut ws, |m| m.event == "phx_reply" && m.reference.as_deref() == Some("1")).await;
    assert_eq!(reply.payload["status"], "ok", "player join ok");

    // The server pushes an initial `self` with the (empty) inventory.
    let self_msg = recv_until(&mut ws, |m| m.event == "self").await;
    assert!(self_msg.payload.get("inventory").is_some());

    // Join a chunk channel → immediate snapshot containing alice, trees, portal.
    send(&mut ws, "2", "2", "chunk:0:0", "phx_join", serde_json::json!({"username":"alice"})).await;
    let snap = recv_until(&mut ws, |m| m.event == "snapshot" && m.topic == "chunk:0:0").await;
    assert!(snap.payload["players"].get("alice").is_some(), "alice in snapshot");
    assert_eq!(snap.payload["resource_nodes"].as_object().unwrap().len(), 5, "5 trees");
    assert_eq!(snap.payload["portals"].as_object().unwrap().len(), 1, "the dungeon portal");

    // Heartbeat replies ok.
    send(&mut ws, "", "hb1", "phoenix", "heartbeat", serde_json::json!({})).await;
    let hb = recv_until(&mut ws, |m| m.event == "phx_reply" && m.reference.as_deref() == Some("hb1")).await;
    assert_eq!(hb.payload["status"], "ok");

    // Harvest the centre tree → ok reply, then a `self` push with wood:1.
    send(&mut ws, "1", "10", "player:alice", "harvest", serde_json::json!({"x":8000,"y":8000})).await;
    let hr = recv_until(&mut ws, |m| m.event == "phx_reply" && m.reference.as_deref() == Some("10")).await;
    assert_eq!(hr.payload["status"], "ok", "harvest ok");
    let inv = recv_until(&mut ws, |m| m.event == "self" && m.payload["inventory"]["wood"] == 1).await;
    assert_eq!(inv.payload["inventory"]["wood"], 1);

    // The snapshot now shows the tree depleted (next broadcast tick).
    let snap2 = recv_until(&mut ws, |m| {
        m.event == "snapshot"
            && m.topic == "chunk:0:0"
            && m.payload["resource_nodes"]["tree:8000:8000"]["depleted"] == true
    })
    .await;
    assert_eq!(snap2.payload["resource_nodes"]["tree:8000:8000"]["depleted"], true);
}

#[tokio::test]
async fn move_then_observe_position_change() {
    let port = start_server().await;
    let mut ws = connect(port).await;
    send(&mut ws, "1", "1", "player:bob", "phx_join",
        serde_json::json!({"username":"bob","initial_chunk":[0,0]})).await;
    recv_until(&mut ws, |m| m.event == "phx_reply" && m.reference.as_deref() == Some("1")).await;
    send(&mut ws, "2", "2", "chunk:0:0", "phx_join", serde_json::json!({"username":"bob"})).await;
    // Initial position is chunk centre (8000,8000).
    let s0 = recv_until(&mut ws, |m| m.event == "snapshot").await;
    assert_eq!(s0.payload["players"]["bob"]["x"], 8000);

    // Move east (move takes no reply); observe x increase over snapshots.
    send(&mut ws, "1", "3", "player:bob", "move", serde_json::json!({"dx":1.0,"dy":0.0})).await;
    let moved = recv_until(&mut ws, |m| {
        m.event == "snapshot" && m.payload["players"]["bob"]["x"].as_i64().unwrap_or(8000) > 8000
    })
    .await;
    assert!(moved.payload["players"]["bob"]["x"].as_i64().unwrap() > 8000);
}

#[tokio::test]
async fn bad_topic_rejected() {
    let port = start_server().await;
    let mut ws = connect(port).await;
    send(&mut ws, "1", "1", "garbage:topic", "phx_join", serde_json::json!({})).await;
    let reply = recv_until(&mut ws, |m| m.event == "phx_reply").await;
    assert_eq!(reply.payload["status"], "error");
    assert_eq!(reply.payload["response"]["reason"], "bad_topic");
}
