//! Channel routing for the wire server — the pure half (no sockets, no async),
//! so it is unit-testable. Maps a decoded [`PhxMessage`] against a [`Sim`] to a
//! reply and any immediate pushes, and tracks a connection's joined topics. The
//! async binary (`bin/server.rs`) owns the sockets, the tick/broadcast loop, and
//! the subscriber registry; it calls [`route`] for every inbound frame.
//!
//! Topics mirror the Elixir routing exactly: `player:<username>`,
//! `chunk:<x>:<y>`, `instance:<id>:chunk:<x>:<y>`, `dev:stats`, and the
//! `phoenix` heartbeat.

use crate::components::{StructureKind, WireId};
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
            // A seq-tagged per-tick input frame: enqueued, consumed one per
            // tick (never applied on receipt), acked back for Mirror replay.
            if let (Some(seq), Some(dx), Some(dy)) =
                (int(&msg.payload, "seq"), num(&msg.payload, "dx"), num(&msg.payload, "dy"))
            {
                if let Some(u) = &conn.username {
                    sim.enqueue_move(&u.clone(), seq as u32, dx, dy);
                }
            }
            Outcome::default() // move takes no reply
        }
        // Verbs are fire-and-forget intents: enqueue and reply nothing (like
        // `move`). The outcome — effect deltas or an async `action_rejected` —
        // arrives later through the broadcast channel. Malformed frames are
        // dropped silently, as `move` drops a missing dx/dy.
        // Harvest/damage are entity-directed: the payload names the Target's
        // WireId.
        "harvest" => {
            enqueue_entity_verb(sim, conn, msg, |target| Action::Harvest { target });
            Outcome::default()
        }
        "damage" => {
            enqueue_entity_verb(sim, conn, msg, |target| Action::Damage { target });
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

/// Enqueue an entity-directed action intent for the connection's player, if
/// both the player and a target WireId are present.
fn enqueue_entity_verb(
    sim: &mut Sim,
    conn: &ConnState,
    msg: &PhxMessage,
    make: impl Fn(WireId) -> Action,
) {
    if let (Some(user), Some(target)) =
        (conn.username.clone(), msg.payload.get("target").and_then(|v| v.as_str()))
    {
        sim.enqueue_action(&user, make(WireId(target.to_string())));
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Item;
    use crate::sim::OutboundEvent;
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

    /// Exact replay needs "one input frame = one tick of integration": frames
    /// queue on receipt and the tick consumes exactly one per player, so the
    /// client can re-simulate its unacked frames knowing precisely which tick
    /// each one drove.
    #[test]
    fn move_frames_are_consumed_one_per_tick() {
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        let mut conn = ConnState { username: Some("alice".into()), ..Default::default() };
        let mv = |seq: u32, dx: f64, dy: f64| PhxMessage {
            join_ref: None,
            reference: None,
            topic: "player:alice".into(),
            event: "move".into(),
            payload: json!({"seq": seq, "dx": dx, "dy": dy}),
        };
        // Two frames arrive within the same inter-tick gap: east, then stop.
        route(&mut sim, &mut conn, &mv(1, 1.0, 0.0));
        route(&mut sim, &mut conn, &mv(2, 0.0, 0.0));
        // Tick 1 consumes frame 1 only: one tick east = 4000 * 0.05 = 200.
        sim.tick();
        let p = sim.overworld().position_of("alice").unwrap();
        assert_eq!((p.x, p.y), (8_200, 8_000), "frame 2 must not pre-empt frame 1");
        // Tick 2 consumes frame 2 (stop): no further movement.
        sim.tick();
        let p = sim.overworld().position_of("alice").unwrap();
        assert_eq!((p.x, p.y), (8_200, 8_000), "frame 2 stops the player");
    }

    /// Intent is perishable: it must be continuously renewed by a live session.
    /// When frames stop, the last Intent holds for a short grace (absorbing
    /// jitter), then expires to zero — a stalled or vanished client's player
    /// stands still instead of walking on stale Intent forever.
    #[test]
    fn intent_perishes_after_grace_when_frames_stop() {
        use crate::consts::INTENT_GRACE_TICKS;
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        sim.enqueue_move("alice", 1, 1.0, 0.0);
        sim.tick(); // consumes the frame: one tick east = 200
        assert_eq!(sim.overworld().position_of("alice").unwrap().x, 8_200);
        // Frames stop. The Intent holds through the grace window…
        for _ in 0..INTENT_GRACE_TICKS {
            sim.tick();
        }
        let held = 8_200 + 200 * INTENT_GRACE_TICKS as i64;
        assert_eq!(
            sim.overworld().position_of("alice").unwrap().x,
            held,
            "the last Intent holds through the grace window"
        );
        // …then perishes: the player stands still.
        sim.tick();
        sim.tick();
        assert_eq!(
            sim.overworld().position_of("alice").unwrap().x,
            held,
            "expired Intent stops the player"
        );
    }

    /// At broadcast ticks the server acks the last-consumed input seq together
    /// with the tick it is current as of — the anchor the Mirror replays from.
    #[test]
    fn broadcast_ticks_ack_the_last_consumed_move_seq() {
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        let _ = sim.drain_events(); // clear connect-time events
        sim.enqueue_move("alice", 7, 1.0, 0.0);
        sim.tick(); // consumes seq 7; tick 1 is not a broadcast tick
        assert!(
            !sim.drain_events().iter().any(|e| matches!(e, OutboundEvent::MoveAck { .. })),
            "acks only at broadcast cadence"
        );
        sim.tick(); // tick 2 broadcasts
        let evs = sim.drain_events();
        assert!(
            evs.iter().any(|e| matches!(
                e,
                OutboundEvent::MoveAck { username, seq: 7, tick: 2 } if username == "alice"
            )),
            "expected ack(seq=7, tick=2), got {evs:?}"
        );
    }

    /// The Mirror integrates every actor it sees by that actor's last-known
    /// Intent — so snapshots carry each actor's velocity (its Intent in the
    /// integrator's units), exactly as the server will integrate it.
    #[test]
    fn snapshot_carries_per_actor_velocity() {
        let mut sim = Sim::new();
        sim.connect_at("alice", crate::components::Position { x: 8_000, y: 8_000 }, Default::default());
        sim.set_intent("alice", 1.0, 0.0);
        let mut conn = ConnState::default();
        let out = route(&mut sim, &mut conn, &join("chunk:0:0", json!({"username":"alice"})));
        let snap = out.pushes.iter().find(|p| p.event == "snapshot").expect("snapshot push");
        let alice = &snap.payload["players"]["alice"];
        assert_eq!(alice["vx"], 4_000.0, "intent (1,0) scaled by the shared speed");
        assert_eq!(alice["vy"], 0.0);
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
            payload: json!({"target":"tree:8000:8000","seq":0}),
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
