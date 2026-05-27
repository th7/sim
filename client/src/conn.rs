//! Low-level async Phoenix-Channels-v2 client over a WebSocket. Pure framing:
//! it joins/leaves topics, pushes events, heartbeats, and yields decoded
//! inbound frames. No game knowledge — [`crate::session`] layers the model on
//! top. Mirrors what the `phoenix` JS client did for the old browser frontend.

use futures_util::{SinkExt, StreamExt};
use protocol::phx::PhxMessage;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream,
};

pub struct PhxConn {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_ref: u64,
}

impl PhxConn {
    /// Open a WebSocket to `url` (e.g. `ws://host:port/socket/websocket?vsn=2.0.0`).
    pub async fn connect(url: &str) -> Result<Self, String> {
        let (ws, _resp) = connect_async(url).await.map_err(|e| e.to_string())?;
        Ok(PhxConn { ws, next_ref: 0 })
    }

    fn next_ref(&mut self) -> String {
        self.next_ref += 1;
        self.next_ref.to_string()
    }

    /// `phx_join` a topic with `payload`, tagged with the caller's `join_ref`.
    pub async fn join(&mut self, join_ref: &str, topic: &str, payload: Value) -> Result<(), String> {
        self.send(Some(join_ref), topic, "phx_join", payload).await
    }

    /// `phx_leave` a topic.
    pub async fn leave(&mut self, join_ref: &str, topic: &str) -> Result<(), String> {
        self.send(Some(join_ref), topic, "phx_leave", json!({})).await
    }

    /// Push a verb event on an already-joined `topic`.
    pub async fn push(
        &mut self,
        join_ref: &str,
        topic: &str,
        event: &str,
        payload: Value,
    ) -> Result<(), String> {
        self.send(Some(join_ref), topic, event, payload).await
    }

    /// Periodic liveness heartbeat on the `phoenix` topic.
    pub async fn heartbeat(&mut self) -> Result<(), String> {
        self.send(None, "phoenix", "heartbeat", json!({})).await
    }

    async fn send(
        &mut self,
        join_ref: Option<&str>,
        topic: &str,
        event: &str,
        payload: Value,
    ) -> Result<(), String> {
        let m = PhxMessage {
            join_ref: join_ref.map(String::from),
            reference: Some(self.next_ref()),
            topic: topic.to_string(),
            event: event.to_string(),
            payload,
        };
        self.ws.send(Message::Text(m.encode())).await.map_err(|e| e.to_string())
    }

    /// Next decoded inbound frame, or `None` when the socket closes.
    pub async fn recv(&mut self) -> Option<PhxMessage> {
        while let Some(frame) = self.ws.next().await {
            match frame {
                Ok(Message::Text(t)) => {
                    if let Ok(m) = PhxMessage::decode(&t) {
                        return Some(m);
                    }
                }
                Ok(Message::Close(_)) | Err(_) => return None,
                _ => continue,
            }
        }
        None
    }
}
