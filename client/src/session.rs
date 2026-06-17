//! Ties the async [`PhxConn`] to the pure [`ClientModel`]: it dispatches inbound
//! wire events into the model and executes the model's commands (join/leave
//! chunk channels, push verbs) back out over the connection, mapping chunk
//! coords to topics by the current realm. This is the testable seam — drive it
//! against an in-process server (see `tests/`) without any rendering.

use crate::conn::PhxConn;
use crate::model::{ClientModel, Cmd, Outbound};
use protocol::geometry::ChunkCoord;
use protocol::wire::{
    AckPayload, CarcassWire, ChunkSnapshot, NodeWire, NpcWire, PlayerWire, PortalWire, RealmWire,
    RelocatedPayload, SelfPayload, StatsPayload, StructureWire,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A cloneable snapshot of everything the view renders, published by the session
/// task after each update and read by the render thread each frame.
#[derive(Debug, Clone)]
pub struct RenderState {
    pub own: String,
    pub realm: RealmWire,
    pub window_center: ChunkCoord,
    pub players: BTreeMap<String, PlayerWire>,
    pub nodes: BTreeMap<String, NodeWire>,
    pub structures: BTreeMap<String, StructureWire>,
    pub portals: BTreeMap<String, PortalWire>,
    pub npcs: BTreeMap<String, NpcWire>,
    pub carcasses: BTreeMap<String, CarcassWire>,
    pub inventory: BTreeMap<String, u32>,
    pub stats: Option<StatsPayload>,
    pub last_error: Option<String>,
    /// The current Target's WireId — the entity wearing the Target marker.
    pub target: Option<String>,
    /// The Action button's display state (Inert / Ready / Dimmed).
    pub action_button: crate::model::ActionButton,
    /// The Mirror is frozen (born, at its Lead bound, or reset): the view
    /// shows a connection signal instead of silently stale state.
    pub frozen: bool,
}

impl RenderState {
    /// Snapshot everything the view renders out of the model. The single point
    /// where model state becomes a render frame — the live session and the
    /// showcase's synthetic scenarios both go through here.
    pub fn from_model(model: &ClientModel) -> Self {
        let dw = model.displayed();
        RenderState {
            own: model.username().to_string(),
            realm: model.realm(),
            window_center: model.window_center(),
            players: dw.players(),
            nodes: dw.nodes(),
            structures: dw.structures(),
            portals: dw.portals(),
            npcs: dw.npcs(),
            carcasses: dw.carcasses(),
            inventory: model.inventory().clone(),
            stats: model.stats().cloned(),
            last_error: model.last_error().map(str::to_string),
            target: model.target().map(str::to_string),
            action_button: dw.action_button(),
            frozen: model.mirror_frozen(),
        }
    }
}

/// User input the render thread hands to the session task.
#[derive(Debug, Clone, Copy)]
pub enum Input {
    Movement { north: bool, south: bool, east: bool, west: bool },
    Click { wx: f64, wy: f64 },
    /// The Action button (`E` or the HUD button): act on the current Target.
    PressAction,
    /// Escape: clear the Target.
    Escape,
    ToggleDev,
}

/// A joined chunk channel: the `join_ref` we used and the exact topic string we
/// joined on. We keep the topic so we leave the channel we actually joined even
/// after the realm has switched (on relocate the model flips to the new realm
/// before the leave commands run), and so we can ignore snapshots arriving on
/// any other topic for the same coord.
struct ChunkSub {
    join_ref: String,
    topic: String,
}

pub struct Session {
    conn: PhxConn,
    model: ClientModel,
    username: String,
    player_topic: String,
    player_join_ref: String,
    next_join_ref: u64,
    chunk_join_refs: BTreeMap<ChunkCoord, ChunkSub>,
    dev_join_ref: Option<String>,
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
            dev_join_ref: None,
        };
        s.execute(cmds).await?;
        Ok(s)
    }

    pub fn model(&self) -> &ClientModel {
        &self.model
    }

    /// A cloneable render snapshot of the current model state.
    pub fn render_state(&self) -> RenderState {
        RenderState::from_model(&self.model)
    }

    /// Drive the session for the life of the connection: dispatch inbound frames,
    /// apply input from `input_rx`, heartbeat, and publish a [`RenderState`] into
    /// `shared` after every change. Returns when the socket or input channel closes.
    pub async fn run(
        mut self,
        mut input_rx: tokio::sync::mpsc::UnboundedReceiver<Input>,
        shared: Arc<Mutex<RenderState>>,
    ) {
        *shared.lock().unwrap() = self.render_state();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
        // The input-frame cadence: Intent is perishable server-side, so a held
        // key is renewed with one frame per tick (and a release sends its one
        // zero-frame). Idle ticks emit nothing.
        let mut input_tick =
            tokio::time::interval(Duration::from_millis(protocol::consts::TICK_MS));
        loop {
            tokio::select! {
                frame = self.conn.recv() => match frame {
                    Some(m) => { self.dispatch(m).await.ok(); }
                    None => break,
                },
                inp = input_rx.recv() => match inp {
                    Some(Input::Movement { north, south, east, west }) => {
                        let cmds = self.model.set_movement(north, south, east, west);
                        self.execute(cmds).await.ok();
                    }
                    Some(Input::Click { wx, wy }) => {
                        let cmds = self.model.click(wx, wy);
                        self.execute(cmds).await.ok();
                    }
                    Some(Input::PressAction) => {
                        let cmds = self.model.press_action();
                        self.execute(cmds).await.ok();
                    }
                    Some(Input::Escape) => {
                        self.model.escape();
                    }
                    Some(Input::ToggleDev) => {
                        let on = !self.model.dev_enabled();
                        let cmds = self.model.set_dev(on);
                        self.execute(cmds).await.ok();
                    }
                    None => break,
                },
                _ = input_tick.tick() => {
                    let cmds = self.model.input_frame();
                    self.execute(cmds).await.ok();
                }
                _ = heartbeat.tick() => { self.conn.heartbeat().await.ok(); }
            }
            *shared.lock().unwrap() = self.render_state();
        }
    }

    // --- input ---

    pub async fn movement(&mut self, n: bool, s: bool, e: bool, w: bool) -> Result<(), String> {
        // State-only: frames go out on the pump/tick cadence, exactly one per
        // tick — so a brief tap moves exactly one tick's distance.
        let cmds = self.model.set_movement(n, s, e, w);
        self.execute(cmds).await
    }

    pub async fn click(&mut self, wx: f64, wy: f64) -> Result<(), String> {
        let cmds = self.model.click(wx, wy);
        self.execute(cmds).await
    }

    /// The Action button (`E` / the HUD button): issue the entity-directed Action
    /// the current Target implies.
    pub async fn press_action(&mut self) -> Result<(), String> {
        let cmds = self.model.press_action();
        self.execute(cmds).await
    }

    /// Clear the Target (the Escape key).
    pub fn escape(&mut self) {
        self.model.escape();
    }

    /// Turn the dev overlay on/off (joins or leaves `dev:stats`).
    pub async fn set_dev(&mut self, on: bool) -> Result<(), String> {
        let cmds = self.model.set_dev(on);
        self.execute(cmds).await
    }

    // --- raw verb pushes ---
    //
    // The analog of the old client's `__game.harvest/build/damage` test hooks:
    // push a verb at exact sub-unit coordinates, bypassing `click`'s tree/grid
    // heuristic. Used by tests that need to place a target at a precise spot
    // (e.g. a wall just clear of the player's body) where cell-snapping the
    // click would land it out of range.

    /// Entity-directed: harvest names its Target's WireId (`seq` 0 — these raw
    /// hooks bypass the model, so there is no press tick to pin).
    pub async fn send_harvest(&mut self, target: &str) -> Result<(), String> {
        self.push_verb("harvest", json!({ "target": target, "seq": 0, "frontier": 0 })).await
    }

    pub async fn send_build(&mut self, kind: &str, x: i64, y: i64) -> Result<(), String> {
        self.push_verb("build", json!({ "type": kind, "x": x, "y": y, "seq": 0 })).await
    }

    pub async fn send_damage(&mut self, target: &str) -> Result<(), String> {
        self.push_verb("damage", json!({ "target": target, "seq": 0, "frontier": 0 })).await
    }

    async fn push_verb(&mut self, event: &str, payload: serde_json::Value) -> Result<(), String> {
        self.conn.push(&self.player_join_ref, &self.player_topic, event, payload).await
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
        let tick = Duration::from_millis(protocol::consts::TICK_MS);
        // Renew at pump entry: callers that pump in short bursts (≤ one tick)
        // would otherwise never reach the renewal point and the held Intent
        // would perish mid-walk.
        let mut next_frame = Instant::now();
        loop {
            let now = Instant::now();
            if now >= deadline {
                return pred(&self.model);
            }
            // Renew the movement input frame on the tick cadence — Intent is
            // perishable server-side; a live session keeps renewing it.
            if now >= next_frame {
                let cmds = self.model.input_frame();
                self.execute(cmds).await.ok();
                next_frame = Instant::now() + tick;
            }
            let wait = deadline
                .min(next_frame)
                .saturating_duration_since(now)
                .max(Duration::from_millis(1));
            match tokio::time::timeout(wait, self.conn.recv()).await {
                Ok(Some(m)) => {
                    self.dispatch(m).await.ok();
                    if pred(&self.model) {
                        return true;
                    }
                }
                Ok(None) => return pred(&self.model), // socket closed
                Err(_) => {} // cadence wake-up; the loop re-checks deadline/frames
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
                // Only ingest a snapshot for a chunk channel we are currently
                // joined to on this exact topic. Drops stragglers from a realm we
                // have left (whose coord-keyed snapshot would otherwise clobber
                // the current realm's in the model's by-coord merge — the source
                // of the in-instance flicker).
                if let Some(coord) = parse_chunk_topic(&m.topic) {
                    let joined = self.chunk_join_refs.get(&coord).map(|s| s.topic.as_str());
                    if joined == Some(m.topic.as_str()) {
                        if let Ok(snap) = serde_json::from_value::<ChunkSnapshot>(m.payload) {
                            let cmds = self.model.on_snapshot(coord, snap);
                            self.execute(cmds).await?;
                        }
                    }
                }
            }
            "self" => {
                if let Ok(p) = serde_json::from_value::<SelfPayload>(m.payload) {
                    self.model.on_self(p);
                }
            }
            "ack" => {
                if let Ok(p) = serde_json::from_value::<AckPayload>(m.payload) {
                    self.model.on_ack(p);
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
            "action_rejected" => {
                // Actions are fire-and-forget intents resolved server-side in the
                // tick; a refusal (a tick-time verb error, or `queue_full` under
                // overload) comes back as this async push. Surface its reason so
                // the user doesn't see clicks fail silently.
                if let Some(reason) = m.payload.get("reason").and_then(|s| s.as_str()) {
                    self.model.on_action_error(reason.to_string());
                }
            }
            _ => {} // join/leave lifecycle frames carry no model state
        }
        Ok(())
    }

    async fn execute(&mut self, cmds: Vec<Cmd>) -> Result<(), String> {
        for cmd in cmds {
            match cmd {
                Cmd::Subscribe(c) => {
                    let jr = self.next_join_ref();
                    let topic = self.chunk_topic(c);
                    self.chunk_join_refs.insert(c, ChunkSub { join_ref: jr.clone(), topic: topic.clone() });
                    self.conn.join(&jr, &topic, json!({ "username": self.username })).await?;
                }
                Cmd::Unsubscribe(c) => {
                    if let Some(sub) = self.chunk_join_refs.remove(&c) {
                        self.conn.leave(&sub.join_ref, &sub.topic).await?;
                    }
                }
                Cmd::Send(out) => {
                    let (event, payload) = outbound_frame(&out);
                    self.conn
                        .push(&self.player_join_ref, &self.player_topic, event, payload)
                        .await?;
                }
                Cmd::SubscribeDevStats => {
                    let jr = self.next_join_ref();
                    self.dev_join_ref = Some(jr.clone());
                    self.conn.join(&jr, "dev:stats", json!({ "username": self.username })).await?;
                }
                Cmd::UnsubscribeDevStats => {
                    if let Some(jr) = self.dev_join_ref.take() {
                        self.conn.leave(&jr, "dev:stats").await?;
                    }
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
