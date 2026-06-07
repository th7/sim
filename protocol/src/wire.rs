//! Wire payloads, shared verbatim by the server (which emits/parses them) and
//! the native client (which parses/emits them). Every struct derives both
//! `Serialize` and `Deserialize`, and matches the committed contract
//! (`contract/contract.json`) field-for-field. Positions are sub-unit integers.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// --- Outbound: server → client (the `snapshot` event, per chunk topic) ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct PlayerWire {
    pub x: i64,
    pub y: i64,
    /// Current velocity (sub-units/sec) — the actor's Intent in the
    /// integrator's units, integrated by the client's Mirror between
    /// authoritative snapshots.
    #[serde(default)]
    pub vx: f64,
    #[serde(default)]
    pub vy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeWire {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub depleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructureWire {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub hp: i64,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortalWire {
    #[serde(rename = "type")]
    pub kind: String,
    pub direction: String,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NpcWire {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub hp: i64,
    /// Current velocity (sub-units/sec) — see [`PlayerWire::vx`].
    #[serde(default)]
    pub vx: f64,
    #[serde(default)]
    pub vy: f64,
    /// The NPC's Demeanor (a [`crate::types::Demeanor`] wire string). Payloads
    /// predating the field parse as Calm — the neutral read.
    #[serde(default = "default_demeanor")]
    pub demeanor: String,
}

fn default_demeanor() -> String {
    crate::types::Demeanor::Calm.as_str().to_string()
}

impl Default for NpcWire {
    fn default() -> Self {
        NpcWire { kind: String::new(), x: 0, y: 0, hp: 0, vx: 0.0, vy: 0.0, demeanor: default_demeanor() }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CarcassWire {
    pub x: i64,
    pub y: i64,
    pub meat: i64,
}

/// The full `snapshot` payload for a single chunk. `npcs`/`carcasses` default to
/// empty so older payloads still parse.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ChunkSnapshot {
    /// The server tick this snapshot is the state of — the authoritative
    /// baseline the client's Mirror overrides at.
    #[serde(default)]
    pub tick: u64,
    pub players: BTreeMap<String, PlayerWire>,
    pub resource_nodes: BTreeMap<String, NodeWire>,
    pub structures: BTreeMap<String, StructureWire>,
    pub portals: BTreeMap<String, PortalWire>,
    #[serde(default)]
    pub npcs: BTreeMap<String, NpcWire>,
    #[serde(default)]
    pub carcasses: BTreeMap<String, CarcassWire>,
}

// --- Outbound: server → client (per-player events) ---

/// The `self` event: the player's current inventory (string-keyed counts).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SelfPayload {
    pub inventory: BTreeMap<String, u32>,
}

/// The realm a player is in, as it appears on the wire (`relocated.realm`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RealmWire {
    Overworld,
    Instance { id: u64 },
}

/// The `relocated` event: the player changed realm/chunk.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelocatedPayload {
    pub realm: RealmWire,
    pub coord: [i32; 2],
}

/// A chunk's lifecycle in the dev overlay ring:
/// - `Hot`: owned by a cluster and simulated this tick.
/// - `IdleArmed`: loaded but no cluster owns it — counting down to unload, with
///   `idle_ms_remaining` ms left of the idle timeout.
/// - `Cold`: not loaded.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChunkLifecycle {
    Hot,
    IdleArmed,
    Cold,
}

impl ChunkLifecycle {
    /// Every lifecycle — what the showcase enumerates to display them all. The
    /// guard match breaks this const's compile when a variant is added.
    pub const ALL: [Self; 3] = {
        let all = [ChunkLifecycle::Hot, ChunkLifecycle::IdleArmed, ChunkLifecycle::Cold];
        match all[0] {
            ChunkLifecycle::Hot | ChunkLifecycle::IdleArmed | ChunkLifecycle::Cold => {}
        }
        all
    };
}

/// One chunk's lifecycle status in the dev overlay ring.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkStatWire {
    pub cx: i32,
    pub cy: i32,
    pub lifecycle: ChunkLifecycle,
    pub idle_ms_remaining: Option<i64>,
    pub entity_count: u64,
}

/// The `stats` event (dev overlay).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StatsPayload {
    pub active_chunks: u64,
    pub total_players: u64,
    /// Live NPCs (wolves + deer) currently simulated in the Overworld.
    #[serde(default)]
    pub total_npcs: u64,
    /// Impossible Frontier claims observed (never-future or regressing) —
    /// clamped to worthlessness, counted here as the probe signal.
    #[serde(default)]
    pub frontier_violations: u64,
    pub around: Vec<ChunkStatWire>,
}

// --- Inbound: client → server (verb payloads the client sends) ---

/// One per-tick movement input frame. The client sends one per client tick
/// while moving (plus a final zero-frame on release); the server consumes
/// exactly one per simulation tick and acks the last consumed `seq`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct MovePayload {
    pub seq: u32,
    pub dx: f64,
    pub dy: f64,
}

/// The `ack` event: the server consumed input frames through `seq` as of
/// `tick` — the authoritative anchor the Mirror replays unacked frames on.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AckPayload {
    pub seq: u32,
    pub tick: u64,
}

/// An entity-directed Verb: acts on the Target's identity (its WireId), never
/// on a remembered place. `seq` is the movement input seq at press time — the
/// Verb resolves at the tick that seq's movement applies (press-frame own
/// position; see `design/targeting-and-wysiwyg.md`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HarvestPayload {
    pub target: String,
    pub seq: u32,
    /// The session's Frontier at press: the last authoritative tick its
    /// Mirror has incorporated — the basis of lawful-render judging.
    #[serde(default)]
    pub frontier: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildPayload {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i64,
    pub y: i64,
    /// The movement input seq at press time — like the entity-directed verbs,
    /// placement resolves at the tick that seq's movement applies.
    pub seq: u32,
}

/// An entity-directed Verb: see [`HarvestPayload`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DamagePayload {
    pub target: String,
    pub seq: u32,
    /// See [`HarvestPayload::frontier`].
    #[serde(default)]
    pub frontier: u64,
}

/// Params the client sends when joining its `player:<username>` channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerJoinParams {
    pub username: String,
    pub initial_chunk: [i32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload predating the demeanor field parses as Calm — the neutral
    /// default an observer assumes about any animal it knows nothing about.
    #[test]
    fn npc_without_demeanor_parses_calm() {
        let n: NpcWire = serde_json::from_value(serde_json::json!({
            "type": "wolf", "x": 1, "y": 2, "hp": 80
        }))
        .unwrap();
        assert_eq!(n.demeanor, crate::types::Demeanor::Calm.as_str());
    }

    #[test]
    fn chunk_lifecycle_serde_roundtrip_over_all() {
        for l in ChunkLifecycle::ALL {
            let v = serde_json::to_value(l).unwrap();
            assert_eq!(serde_json::from_value::<ChunkLifecycle>(v).unwrap(), l);
        }
    }

    #[test]
    fn realm_wire_matches_contract_shape() {
        assert_eq!(
            serde_json::to_value(RealmWire::Overworld).unwrap(),
            serde_json::json!({ "kind": "overworld" })
        );
        assert_eq!(
            serde_json::to_value(RealmWire::Instance { id: 7 }).unwrap(),
            serde_json::json!({ "kind": "instance", "id": 7 })
        );
        // Round-trips.
        let r: RealmWire = serde_json::from_value(serde_json::json!({"kind":"instance","id":3})).unwrap();
        assert_eq!(r, RealmWire::Instance { id: 3 });
    }

    #[test]
    fn snapshot_round_trips() {
        let mut snap = ChunkSnapshot::default();
        snap.players.insert(
            "alice".into(),
            PlayerWire { x: 8000, y: 8000, vx: 4000.0, vy: 0.0 },
        );
        snap.resource_nodes.insert(
            "tree:8000:8000".into(),
            NodeWire { kind: "tree".into(), x: 8000, y: 8000, depleted: false },
        );
        let json = serde_json::to_string(&snap).unwrap();
        let back: ChunkSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
        // The `type` rename is on the wire.
        assert!(json.contains("\"type\":\"tree\""));
    }

    #[test]
    fn verb_payloads_serialize_as_contract() {
        assert_eq!(
            serde_json::to_value(BuildPayload { kind: "wall".into(), x: 3000, y: 3000, seq: 9 })
                .unwrap(),
            serde_json::json!({ "type": "wall", "x": 3000, "y": 3000, "seq": 9 })
        );
        assert_eq!(
            serde_json::to_value(HarvestPayload { target: "tree:1:2".into(), seq: 9, frontier: 3 })
                .unwrap(),
            serde_json::json!({ "target": "tree:1:2", "seq": 9, "frontier": 3 })
        );
        assert_eq!(
            serde_json::to_value(MovePayload { seq: 7, dx: 1.0, dy: 0.0 }).unwrap(),
            serde_json::json!({ "seq": 7, "dx": 1.0, "dy": 0.0 })
        );
    }
}
