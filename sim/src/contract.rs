//! The wire contract, generated from the server's own types.
//!
//! `contract/contract.json` is the committed schema both the server and the
//! native client conform to. It is **generated** by [`contract`] (regenerate via
//! the `export-contract` bin) and a freshness test asserts the committed file
//! still matches — so the schema cannot drift from the code. The drift-prone
//! enum strings are sourced from the Rust types themselves (verb-error reasons
//! from [`crate::verbs::VerbError`], the structure type from `StructureKind`).

use crate::components::StructureKind;
use crate::verbs::VerbError;
use serde_json::{json, Value};

/// The full wire contract as a JSON value.
pub fn contract() -> Value {
    json!({
        "messages": [
            // Verbs are fire-and-forget intents (like `move`): no reply. The
            // outcome arrives asynchronously — effect deltas, a `self` push, or
            // an `action_rejected` push.
            intent("build", build_payload()),
            intent("damage", xy_payload()),
            intent("harvest", xy_payload()),
            // `join` errors are channel-join reasons (not VerbError), kept as literals.
            join_message(),
            move_message(),
            out("ack", "player", ack_payload()),
            out("action_rejected", "player", action_rejected_payload()),
            out("relocated", "player", relocated_payload()),
            out("self", "player", self_payload()),
            out("snapshot", "chunk", snapshot_payload()),
            out("stats", "dev", stats_payload()),
        ]
    })
}

// --- message shells ---

/// An inbound fire-and-forget intent on the `player` topic: a payload, no reply.
fn intent(event: &str, payload: Value) -> Value {
    json!({ "direction": "in", "event": event, "topic": "player", "payload": payload })
}

/// An outbound push (no reply).
fn out(event: &str, topic: &str, payload: Value) -> Value {
    json!({ "direction": "out", "event": event, "topic": topic, "payload": payload })
}

fn join_message() -> Value {
    json!({
        "direction": "in", "event": "join", "topic": "all",
        "reply": reply(enum_str(&["username_mismatch", "bad_topic", "unavailable"])),
    })
}

fn move_message() -> Value {
    // A seq-tagged per-tick input frame: the server consumes one per tick and
    // acks the last consumed seq (the `ack` event).
    json!({
        "direction": "in", "event": "move", "topic": "player",
        "payload": object(
            &[("seq", integer()), ("dx", number()), ("dy", number())],
            &["seq", "dx", "dy"],
        ),
    })
}

fn ack_payload() -> Value {
    object(&[("seq", integer()), ("tick", integer())], &["seq", "tick"])
}

// --- payload schemas ---

fn build_payload() -> Value {
    object(
        &[("type", enum_str(&[StructureKind::Wall.as_str()])), ("x", integer()), ("y", integer())],
        &["type", "x", "y"],
    )
}

fn xy_payload() -> Value {
    object(&[("x", integer()), ("y", integer())], &["x", "y"])
}

fn action_rejected_payload() -> Value {
    object(
        &[
            ("verb", enum_str(&["harvest", "build", "damage"])),
            ("x", integer()),
            ("y", integer()),
            ("reason", action_reasons()),
        ],
        &["verb", "x", "y", "reason"],
    )
}

/// The reasons an action intent can be refused: every tick-time verb error the
/// realm can return, plus `queue_full` (refused at the door when the per-actor
/// queue is full).
fn action_reasons() -> Value {
    let mut rs: Vec<&str> = [
        VerbError::NoPlayer,
        VerbError::TooFar,
        VerbError::Depleted,
        VerbError::NoTarget,
        VerbError::NoChunk,
        VerbError::OutOfChunk,
        VerbError::FootprintBlocked,
        VerbError::InsufficientMaterials,
        VerbError::NoBuildInInstance,
    ]
    .iter()
    .map(|e| e.as_str())
    .collect();
    rs.push("queue_full");
    enum_str(&rs)
}

fn relocated_payload() -> Value {
    let realm = json!({ "oneOf": [
        object(&[("kind", enum_str(&["overworld"]))], &["kind"]),
        object(&[("id", integer()), ("kind", enum_str(&["instance"]))], &["id", "kind"]),
    ]});
    let coord = json!({ "type": "array", "items": integer(), "minItems": 2, "maxItems": 2 });
    object(&[("coord", coord), ("realm", realm)], &["coord", "realm"])
}

fn self_payload() -> Value {
    object(&[("inventory", map_of(integer()))], &["inventory"])
}

fn snapshot_payload() -> Value {
    let players = map_of(object(
        &[("x", integer()), ("y", integer()), ("vx", number()), ("vy", number())],
        &["x", "y", "vx", "vy"],
    ));
    let npcs = map_of(object(
        &[
            ("type", string()), ("x", integer()), ("y", integer()), ("hp", integer()),
            ("vx", number()), ("vy", number()),
        ],
        &["type", "x", "y", "hp", "vx", "vy"],
    ));
    let carcasses = map_of(object(
        &[("x", integer()), ("y", integer()), ("meat", integer())],
        &["x", "y", "meat"],
    ));
    let portals = map_of(object(
        &[("direction", string()), ("type", string()), ("x", integer()), ("y", integer())],
        &["direction", "type", "x", "y"],
    ));
    let resource_nodes = map_of(object(
        &[("depleted", boolean()), ("type", string()), ("x", integer()), ("y", integer())],
        &["depleted", "type", "x", "y"],
    ));
    let structures = map_of(object(
        &[("hp", integer()), ("owner", string()), ("type", string()), ("x", integer()), ("y", integer())],
        &["hp", "owner", "type", "x", "y"],
    ));
    object(
        &[
            ("players", players), ("npcs", npcs), ("carcasses", carcasses), ("portals", portals),
            ("resource_nodes", resource_nodes), ("structures", structures), ("tick", integer()),
        ],
        &["players", "portals", "resource_nodes", "structures", "npcs", "carcasses", "tick"],
    )
}

fn stats_payload() -> Value {
    let around_item = object(
        &[
            ("cx", integer()), ("cy", integer()), ("entity_count", integer()),
            ("idle_ms_remaining", json!({ "type": ["integer", "null"] })),
            ("lifecycle", enum_str(&["hot", "idle_armed", "cold"])),
        ],
        &["cx", "cy", "entity_count", "idle_ms_remaining", "lifecycle"],
    );
    object(
        &[
            ("active_chunks", integer()),
            ("around", json!({ "type": "array", "items": around_item })),
            ("total_players", integer()), ("total_npcs", integer()),
        ],
        &["active_chunks", "around", "total_players", "total_npcs"],
    )
}

// --- schema-construction helpers ---

/// A strict object: the given properties, all required listed, no extras.
fn object(props: &[(&str, Value)], required: &[&str]) -> Value {
    let mut p = serde_json::Map::new();
    for (k, v) in props {
        p.insert((*k).to_string(), v.clone());
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": Value::Object(p),
        "required": required,
    })
}

/// A map-object: arbitrary string keys, each value matching `value`.
fn map_of(value: Value) -> Value {
    json!({ "type": "object", "additionalProperties": value })
}

/// An ok/error reply pair, the error carrying a `reason` from `reasons`.
/// (Only channel-join messages still reply; verbs are fire-and-forget intents.)
fn reply(reasons: Value) -> Value {
    json!({
        "ok": object(&[], &[]),
        "error": object(&[("reason", reasons)], &["reason"]),
    })
}

fn enum_str(variants: &[&str]) -> Value {
    json!({ "type": "string", "enum": variants })
}

fn integer() -> Value {
    json!({ "type": "integer" })
}
fn number() -> Value {
    json!({ "type": "number" })
}
fn string() -> Value {
    json!({ "type": "string" })
}
fn boolean() -> Value {
    json!({ "type": "boolean" })
}
