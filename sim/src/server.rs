//! Channel routing for the wire server — the pure half (no sockets, no async),
//! so it is unit-testable. Maps a decoded [`PhxMessage`] against a [`Sim`] to a
//! reply and any immediate pushes, and tracks a connection's joined topics. The
//! async binary (`bin/server.rs`) owns the sockets, the tick/broadcast loop, and
//! the subscriber registry; it calls [`route`] for every inbound frame.
//!
//! Topics mirror the Elixir routing exactly: `player:<username>`,
//! `chunk:<x>:<y>`, `instance:<id>:chunk:<x>:<y>`, `dev:stats`, and the
//! `phoenix` heartbeat.

use crate::components::StructureKind;
use crate::geometry::ChunkCoord;
use crate::ids::Realm;
use crate::phx::{push, PhxMessage};
use crate::sim::{Action, Sim};
use crate::wire::{chunk_snapshot, inventory_payload};
use serde_json::Value;
use std::collections::HashSet;

/// Per-connection channel state.
#[derive(Debug, Default, Clone)]
pub struct ConnState {
    /// Set once the connection joins its `player:<username>` channel.
    pub username: Option<String>,
    /// Set if the connection joined `dev:stats` (the username to centre the ring on).
    pub dev_username: Option<String>,
    pub topics: HashSet<String>,
}

/// The result of routing one inbound frame.
#[derive(Debug, Default)]
pub struct Outcome {
    /// A `phx_reply` to send back (absent for events that take no reply, e.g. `move`).
    pub reply: Option<PhxMessage>,
    /// Frames to push to *this* connection immediately (e.g. an initial snapshot).
    pub pushes: Vec<PhxMessage>,
    /// True if this frame disconnected the player (the caller cleans up).
    pub disconnected: bool,
}

/// A parsed topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Topic {
    Player(String),
    Chunk(Realm, ChunkCoord),
    DevStats,
    Phoenix,
}

pub fn parse_topic(topic: &str) -> Option<Topic> {
    if topic == "phoenix" {
        return Some(Topic::Phoenix);
    }
    if topic == "dev:stats" {
        return Some(Topic::DevStats);
    }
    if let Some(user) = topic.strip_prefix("player:") {
        return Some(Topic::Player(user.to_string()));
    }
    if let Some(rest) = topic.strip_prefix("chunk:") {
        let (x, y) = parse_xy(rest)?;
        return Some(Topic::Chunk(Realm::Overworld, ChunkCoord::new(x, y)));
    }
    if let Some(rest) = topic.strip_prefix("instance:") {
        // <id>:chunk:<x>:<y>
        let (id_str, after) = rest.split_once(":chunk:")?;
        let id: u64 = id_str.parse().ok()?;
        let (x, y) = parse_xy(after)?;
        return Some(Topic::Chunk(Realm::Instance(id), ChunkCoord::new(x, y)));
    }
    None
}

fn parse_xy(s: &str) -> Option<(i32, i32)> {
    let (xs, ys) = s.split_once(':')?;
    Some((xs.parse().ok()?, ys.parse().ok()?))
}

/// Route one inbound frame, mutating the Sim and the connection's topic set.
pub fn route(sim: &mut Sim, conn: &mut ConnState, msg: &PhxMessage) -> Outcome {
    match msg.event.as_str() {
        "phx_join" => on_join(sim, conn, msg),
        "phx_leave" => on_leave(sim, conn, msg),
        "heartbeat" => Outcome { reply: Some(msg.ok()), ..Default::default() },
        "move" => {
            if let (Some(dx), Some(dy)) = (num(&msg.payload, "dx"), num(&msg.payload, "dy")) {
                if let Some(u) = &conn.username {
                    sim.set_intent(u, dx, dy);
                }
            }
            Outcome::default() // move takes no reply (matches Elixir :noreply)
        }
        // Verbs are fire-and-forget intents: enqueue and reply nothing (like
        // `move`). The outcome — effect deltas or an async `action_rejected` —
        // arrives later through the broadcast channel. Malformed frames are
        // dropped silently, as `move` drops a missing dx/dy.
        "harvest" => {
            enqueue_xy(sim, conn, msg, |x, y| Action::Harvest { x, y });
            Outcome::default()
        }
        "damage" => {
            enqueue_xy(sim, conn, msg, |x, y| Action::Damage { x, y });
            Outcome::default()
        }
        "build" => {
            if let (Some(user), Some(kind), Some(x), Some(y)) = (
                conn.username.clone(),
                msg.payload.get("type").and_then(|v| v.as_str()).and_then(StructureKind::parse),
                int(&msg.payload, "x"),
                int(&msg.payload, "y"),
            ) {
                sim.enqueue_action(&user, Action::Build { kind, x, y });
            }
            Outcome::default()
        }
        _ => Outcome::default(),
    }
}

/// Enqueue an `(x, y)` action intent for the connection's player, if both the
/// player and a valid target cell are present.
fn enqueue_xy(sim: &mut Sim, conn: &ConnState, msg: &PhxMessage, make: impl Fn(i64, i64) -> Action) {
    if let (Some(user), Some((x, y))) = (conn.username.clone(), xy(&msg.payload)) {
        sim.enqueue_action(&user, make(x, y));
    }
}

fn on_join(sim: &mut Sim, conn: &mut ConnState, msg: &PhxMessage) -> Outcome {
    match parse_topic(&msg.topic) {
        Some(Topic::Player(user)) => {
            // username must match the topic.
            if msg.payload.get("username").and_then(|v| v.as_str()) != Some(user.as_str()) {
                return Outcome { reply: Some(msg.error_reason("username_mismatch")), ..Default::default() };
            }
            let initial = parse_initial_chunk(&msg.payload);
            sim.connect(&user, initial);
            conn.username = Some(user.clone());
            conn.topics.insert(msg.topic.clone());
            let mut pushes = Vec::new();
            if let Some(inv) = sim.inventory_of(&user) {
                pushes.push(push(&msg.topic, "self", inventory_payload(&inv)));
            }
            Outcome { reply: Some(msg.ok()), pushes, disconnected: false }
        }
        Some(Topic::Chunk(realm, coord)) => {
            conn.topics.insert(msg.topic.clone());
            let mut pushes = Vec::new();
            if let Some(p) = chunk_snapshot_push(sim, realm, coord, &msg.topic) {
                pushes.push(p);
            }
            Outcome { reply: Some(msg.ok()), pushes, disconnected: false }
        }
        Some(Topic::DevStats) => {
            conn.dev_username = msg.payload.get("username").and_then(|v| v.as_str()).map(String::from);
            conn.topics.insert(msg.topic.clone());
            let pushes =
                vec![push(&msg.topic, "stats", crate::dev::stats_payload(sim, conn.dev_username.as_deref()))];
            Outcome { reply: Some(msg.ok()), pushes, disconnected: false }
        }
        _ => Outcome { reply: Some(msg.error_reason("bad_topic")), ..Default::default() },
    }
}

fn on_leave(sim: &mut Sim, conn: &mut ConnState, msg: &PhxMessage) -> Outcome {
    conn.topics.remove(&msg.topic);
    let mut disconnected = false;
    if let Some(Topic::Player(user)) = parse_topic(&msg.topic) {
        if conn.username.as_deref() == Some(user.as_str()) {
            sim.disconnect(&user);
            conn.username = None;
            disconnected = true;
        }
    }
    Outcome { reply: Some(msg.ok()), pushes: Vec::new(), disconnected }
}

/// Build the `snapshot` push for one chunk topic, or `None` if the realm is gone.
pub fn chunk_snapshot_push(
    sim: &Sim,
    realm: Realm,
    coord: ChunkCoord,
    topic: &str,
) -> Option<PhxMessage> {
    let rw = sim.realm_world(realm)?;
    let states = rw.snapshot_states();
    let snap = chunk_snapshot(&states, coord, sim.tick_count());
    Some(push(topic, "snapshot", serde_json::to_value(snap).ok()?))
}

fn parse_initial_chunk(payload: &Value) -> ChunkCoord {
    payload
        .get("initial_chunk")
        .and_then(|v| v.as_array())
        .and_then(|a| {
            Some(ChunkCoord::new(a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32))
        })
        .unwrap_or(ChunkCoord::new(0, 0))
}

fn num(payload: &Value, key: &str) -> Option<f64> {
    payload.get(key).and_then(|v| v.as_f64())
}
fn int(payload: &Value, key: &str) -> Option<i64> {
    payload.get(key).and_then(|v| v.as_i64())
}
fn xy(payload: &Value) -> Option<(i64, i64)> {
    Some((int(payload, "x")?, int(payload, "y")?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Item;
    use serde_json::json;

    fn join(topic: &str, payload: Value) -> PhxMessage {
        PhxMessage {
            join_ref: Some("1".into()),
            reference: Some("2".into()),
            topic: topic.into(),
            event: "phx_join".into(),
            payload,
        }
    }

    #[test]
    fn topics_parse() {
        assert_eq!(parse_topic("player:alice"), Some(Topic::Player("alice".into())));
        assert_eq!(parse_topic("chunk:1:-2"), Some(Topic::Chunk(Realm::Overworld, ChunkCoord::new(1, -2))));
        assert_eq!(
            parse_topic("instance:7:chunk:1:1"),
            Some(Topic::Chunk(Realm::Instance(7), ChunkCoord::new(1, 1)))
        );
        assert_eq!(parse_topic("dev:stats"), Some(Topic::DevStats));
        assert_eq!(parse_topic("garbage"), None);
    }

    #[test]
    fn player_join_connects_and_pushes_self() {
        let mut sim = Sim::new();
        let mut conn = ConnState::default();
        let out = route(&mut sim, &mut conn, &join("player:alice", json!({"username":"alice","initial_chunk":[0,0]})));
        assert_eq!(out.reply.unwrap().payload["status"], "ok");
        assert_eq!(conn.username.as_deref(), Some("alice"));
        assert!(sim.realm_of("alice").is_some());
        // A self push with empty inventory.
        assert!(out.pushes.iter().any(|p| p.event == "self"));
    }

    #[test]
    fn username_mismatch_rejected() {
        let mut sim = Sim::new();
        let mut conn = ConnState::default();
        let out = route(&mut sim, &mut conn, &join("player:alice", json!({"username":"bob"})));
        assert_eq!(out.reply.unwrap().payload["response"]["reason"], "username_mismatch");
        assert!(conn.username.is_none());
    }

    /// The Mirror overrides its state *at* an authoritative tick — so every
    /// snapshot says which tick it is the state of.
    #[test]
    fn snapshot_carries_the_server_tick() {
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        sim.tick();
        sim.tick();
        sim.tick();
        let mut conn = ConnState::default();
        let out = route(&mut sim, &mut conn, &join("chunk:0:0", json!({"username":"alice"})));
        let snap = out.pushes.iter().find(|p| p.event == "snapshot").expect("snapshot push");
        assert_eq!(snap.payload["tick"], 3, "snapshot is stamped with the tick it captures");
    }

    #[test]
    fn chunk_join_pushes_snapshot() {
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        let mut conn = ConnState::default();
        let out = route(&mut sim, &mut conn, &join("chunk:0:0", json!({"username":"alice"})));
        assert_eq!(out.reply.unwrap().payload["status"], "ok");
        let snap = out.pushes.iter().find(|p| p.event == "snapshot").expect("snapshot push");
        // Chunk (0,0) has alice + worldgen trees + the portal.
        assert!(snap.payload["players"].get("alice").is_some());
        assert!(snap.payload["portals"].as_object().unwrap().len() >= 1);
    }

    #[test]
    fn harvest_enqueues_and_resolves_on_the_tick_with_no_reply() {
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        let mut conn = ConnState { username: Some("alice".into()), ..Default::default() };
        let msg = PhxMessage {
            join_ref: Some("1".into()),
            reference: Some("9".into()),
            topic: "player:alice".into(),
            event: "harvest".into(),
            payload: json!({"x":8000,"y":8000}),
        };
        let out = route(&mut sim, &mut conn, &msg);
        // Fire-and-forget: no synchronous reply, and nothing resolved yet.
        assert!(out.reply.is_none(), "a harvest intent is :noreply");
        assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), None);
        // The tick resolves it.
        sim.tick();
        assert_eq!(sim.inventory_of("alice").unwrap().items.get(&Item::Wood), Some(&1));
    }

    #[test]
    fn build_with_invalid_type_is_dropped() {
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        let mut conn = ConnState { username: Some("alice".into()), ..Default::default() };
        let msg = PhxMessage {
            join_ref: Some("1".into()),
            reference: Some("9".into()),
            topic: "player:alice".into(),
            event: "build".into(),
            payload: json!({"type":"castle","x":3000,"y":3000}),
        };
        let out = route(&mut sim, &mut conn, &msg);
        // A malformed frame is dropped silently (no reply, nothing enqueued).
        assert!(out.reply.is_none(), "a malformed build frame is dropped, not replied to");
        sim.tick();
        assert!(sim.drain_events().is_empty(), "nothing was enqueued, so nothing is rejected");
    }
}
