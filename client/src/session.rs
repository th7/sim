//! Ties the async [`PhxConn`] to the pure [`ClientModel`]: it dispatches inbound
//! wire events into the model and executes the model's commands (join/leave
//! chunk channels, push verbs) back out over the connection, mapping chunk
//! coords to topics by the current realm. This is the testable seam — drive it
//! against an in-process server (see `tests/`) without any rendering.

use crate::conn::PhxConn;
use crate::model::{ClientModel, Cmd, Outbound};
use protocol::geometry::ChunkCoord;
use protocol::wire::{ChunkSnapshot, RealmWire, RelocatedPayload, SelfPayload, StatsPayload};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub struct Session {
    conn: PhxConn,
    model: ClientModel,
    username: String,
    player_topic: String,
    player_join_ref: String,
    next_join_ref: u64,
    chunk_join_refs: BTreeMap<ChunkCoord, String>,
}

impl Session {
    /// Connect, join the player channel, and subscribe the initial 3×3 chunks.
    pub async fn connect(
        url: &str,
        username: &str,
        initial_chunk: ChunkCoord,
    ) -> Result<Self, String> {
        let mut conn = PhxConn::connect(url).await?;
        let (model, cmds) = ClientModel::new(username, initial_chunk);
        let player_topic = format!("player:{username}");
        let player_join_ref = "0".to_string();
        conn.join(
            &player_join_ref,
            &player_topic,
            json!({ "username": username, "initial_chunk": [initial_chunk.cx, initial_chunk.cy] }),
        )
        .await?;
        let mut s = Session {
            conn,
            model,
            username: username.to_string(),
            player_topic,
            player_join_ref,
            next_join_ref: 1,
            chunk_join_refs: BTreeMap::new(),
        };
        s.execute(cmds).await?;
        Ok(s)
    }

    pub fn model(&self) -> &ClientModel {
        &self.model
    }

    // --- input ---

    pub async fn movement(&mut self, n: bool, s: bool, e: bool, w: bool) -> Result<(), String> {
        let cmds = self.model.set_movement(n, s, e, w);
        self.execute(cmds).await
    }

    pub async fn click(&mut self, wx: f64, wy: f64) -> Result<(), String> {
        let cmds = self.model.click(wx, wy);
        self.execute(cmds).await
    }

    pub async fn heartbeat(&mut self) -> Result<(), String> {
        self.conn.heartbeat().await
    }

    // --- pump ---

    /// Read and dispatch frames until `pred(model)` holds or `timeout` elapses.
    /// Returns whether the predicate ended up true.
    pub async fn pump_until(
        &mut self,
        timeout: Duration,
        pred: impl Fn(&ClientModel) -> bool,
    ) -> bool {
        if pred(&self.model) {
            return true;
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = match deadline.checked_duration_since(Instant::now()) {
                Some(r) if !r.is_zero() => r,
                _ => return pred(&self.model),
            };
            match tokio::time::timeout(remaining, self.conn.recv()).await {
                Ok(Some(m)) => {
                    self.dispatch(m).await.ok();
                    if pred(&self.model) {
                        return true;
                    }
                }
                Ok(None) => return pred(&self.model), // socket closed
                Err(_) => return pred(&self.model),   // timed out
            }
        }
    }

    /// Pump for `dur` regardless of any predicate (drain whatever arrives).
    pub async fn pump_for(&mut self, dur: Duration) {
        self.pump_until(dur, |_| false).await;
    }

    async fn dispatch(&mut self, m: protocol::phx::PhxMessage) -> Result<(), String> {
        match m.event.as_str() {
            "snapshot" => {
                if let Some(coord) = parse_chunk_topic(&m.topic) {
                    if let Ok(snap) = serde_json::from_value::<ChunkSnapshot>(m.payload) {
                        let cmds = self.model.on_snapshot(coord, snap);
                        self.execute(cmds).await?;
                    }
                }
            }
            "self" => {
                if let Ok(p) = serde_json::from_value::<SelfPayload>(m.payload) {
                    self.model.on_self(p);
                }
            }
            "relocated" => {
                if let Ok(p) = serde_json::from_value::<RelocatedPayload>(m.payload) {
                    let cmds = self.model.on_relocated(p);
                    self.execute(cmds).await?;
                }
            }
            "stats" => {
                if let Ok(p) = serde_json::from_value::<StatsPayload>(m.payload) {
                    self.model.on_stats(p);
                }
            }
            _ => {} // phx_reply and lifecycle frames carry no model state
        }
        Ok(())
    }

    async fn execute(&mut self, cmds: Vec<Cmd>) -> Result<(), String> {
        for cmd in cmds {
            match cmd {
                Cmd::Subscribe(c) => {
                    let jr = self.next_join_ref();
                    let topic = self.chunk_topic(c);
                    self.chunk_join_refs.insert(c, jr.clone());
                    self.conn.join(&jr, &topic, json!({ "username": self.username })).await?;
                }
                Cmd::Unsubscribe(c) => {
                    if let Some(jr) = self.chunk_join_refs.remove(&c) {
                        let topic = self.chunk_topic(c);
                        self.conn.leave(&jr, &topic).await?;
                    }
                }
                Cmd::Send(out) => {
                    let (event, payload) = outbound_frame(&out);
                    self.conn
                        .push(&self.player_join_ref, &self.player_topic, event, payload)
                        .await?;
                }
            }
        }
        Ok(())
    }

    fn next_join_ref(&mut self) -> String {
        let j = self.next_join_ref;
        self.next_join_ref += 1;
        j.to_string()
    }

    fn chunk_topic(&self, c: ChunkCoord) -> String {
        match self.model.realm() {
            RealmWire::Overworld => format!("chunk:{}:{}", c.cx, c.cy),
            RealmWire::Instance { id } => format!("instance:{}:chunk:{}:{}", id, c.cx, c.cy),
        }
    }
}

fn outbound_frame(out: &Outbound) -> (&'static str, serde_json::Value) {
    match out {
        Outbound::Move(p) => ("move", serde_json::to_value(p).unwrap()),
        Outbound::Harvest(p) => ("harvest", serde_json::to_value(p).unwrap()),
        Outbound::Build(p) => ("build", serde_json::to_value(p).unwrap()),
        Outbound::Damage(p) => ("damage", serde_json::to_value(p).unwrap()),
    }
}

/// Parse a chunk topic to its coord: `chunk:x:y` or `instance:<id>:chunk:x:y`.
fn parse_chunk_topic(topic: &str) -> Option<ChunkCoord> {
    if let Some(rest) = topic.strip_prefix("chunk:") {
        let (x, y) = rest.split_once(':')?;
        return Some(ChunkCoord::new(x.parse().ok()?, y.parse().ok()?));
    }
    if let Some(rest) = topic.strip_prefix("instance:") {
        let (_id, after) = rest.split_once(":chunk:")?;
        let (x, y) = after.split_once(':')?;
        return Some(ChunkCoord::new(x.parse().ok()?, y.parse().ok()?));
    }
    None
}
