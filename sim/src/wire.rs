//! Wire-facing observation: per-entity state and the per-chunk `snapshot`
//! payload, shaped to match the committed contract
//! (`apps/game_web/priv/contract/contract.json`) byte-for-byte. Positions are
//! sub-unit integers; the frontend divides by 1000.
//!
//! The cluster authors observation as changed-only deltas (see
//! [`crate::delta`]); a full [`ChunkSnapshot`] is just the current state of one
//! chunk, which is what the `snapshot` wire event carries.

use crate::components::*;
use crate::geometry::ChunkCoord;
use crate::world::RealmWorld;
use serde::Serialize;
use std::collections::BTreeMap;

/// The observable state of one entity, keyed on the wire by its [`WireId`].
/// Equality drives changed-only delta diffing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityWire {
    Player { x: i64, y: i64 },
    Node { kind: ResourceKind, x: i64, y: i64, depleted: bool },
    Structure { kind: StructureKind, owner: String, hp: i64, x: i64, y: i64 },
    Portal { kind: PortalKind, direction: PortalDirection, x: i64, y: i64 },
}

impl EntityWire {
    pub fn position(&self) -> (i64, i64) {
        match *self {
            EntityWire::Player { x, y }
            | EntityWire::Node { x, y, .. }
            | EntityWire::Structure { x, y, .. }
            | EntityWire::Portal { x, y, .. } => (x, y),
        }
    }
    pub fn chunk(&self) -> ChunkCoord {
        let (x, y) = self.position();
        crate::geometry::coord_for(x, y)
    }
}

/// Extract the wire state of every renderable entity in a realm, keyed by wire
/// id. Deterministic (BTreeMap) so deltas and snapshots are stable.
pub fn entity_states(rw: &RealmWorld) -> BTreeMap<WireId, EntityWire> {
    let mut out = BTreeMap::new();
    let world = &rw.world;

    for (_e, (pos, _pc, wid)) in world.query::<(&Position, &PlayerControlled, &WireId)>().iter() {
        out.insert(wid.clone(), EntityWire::Player { x: pos.x, y: pos.y });
    }
    for (e, (pos, g, wid)) in world.query::<(&Position, &Gatherable, &WireId)>().iter() {
        let _ = e;
        out.insert(
            wid.clone(),
            EntityWire::Node { kind: g.kind, x: pos.x, y: pos.y, depleted: false },
        );
    }
    for (_e, (pos, d, wid)) in world.query::<(&Position, &Depleted, &WireId)>().iter() {
        out.insert(
            wid.clone(),
            EntityWire::Node { kind: d.kind, x: pos.x, y: pos.y, depleted: true },
        );
    }
    for (_e, (pos, s, wid)) in world.query::<(&Position, &Structure, &WireId)>().iter() {
        out.insert(
            wid.clone(),
            EntityWire::Structure {
                kind: s.kind,
                owner: s.owner.clone(),
                hp: s.hp,
                x: pos.x,
                y: pos.y,
            },
        );
    }
    for (_e, (pos, p, wid)) in world.query::<(&Position, &Portal, &WireId)>().iter() {
        out.insert(
            wid.clone(),
            EntityWire::Portal { kind: p.kind, direction: p.direction, x: pos.x, y: pos.y },
        );
    }
    out
}

// --- Serializable per-chunk snapshot (the `snapshot` wire payload) ---

#[derive(Debug, Serialize, PartialEq)]
pub struct PlayerWire {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct NodeWire {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub depleted: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct StructureWire {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub hp: i64,
    pub owner: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PortalWire {
    #[serde(rename = "type")]
    pub kind: String,
    pub direction: String,
    pub x: i64,
    pub y: i64,
}

/// The full `snapshot` payload for a single chunk, matching the contract.
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct ChunkSnapshot {
    pub players: BTreeMap<String, PlayerWire>,
    pub resource_nodes: BTreeMap<String, NodeWire>,
    pub structures: BTreeMap<String, StructureWire>,
    pub portals: BTreeMap<String, PortalWire>,
}

/// Build the full snapshot of one chunk from a set of entity states (those
/// whose position falls in `coord`).
pub fn chunk_snapshot(states: &BTreeMap<WireId, EntityWire>, coord: ChunkCoord) -> ChunkSnapshot {
    let mut snap = ChunkSnapshot::default();
    for (wid, state) in states {
        if state.chunk() != coord {
            continue;
        }
        match state {
            EntityWire::Player { x, y } => {
                snap.players.insert(wid.0.clone(), PlayerWire { x: *x, y: *y });
            }
            EntityWire::Node { kind, x, y, depleted } => {
                snap.resource_nodes.insert(
                    wid.0.clone(),
                    NodeWire { kind: kind.as_str().to_string(), x: *x, y: *y, depleted: *depleted },
                );
            }
            EntityWire::Structure { kind, owner, hp, x, y } => {
                snap.structures.insert(
                    wid.0.clone(),
                    StructureWire {
                        kind: kind.as_str().to_string(),
                        x: *x,
                        y: *y,
                        hp: *hp,
                        owner: owner.clone(),
                    },
                );
            }
            EntityWire::Portal { kind, direction, x, y } => {
                snap.portals.insert(
                    wid.0.clone(),
                    PortalWire {
                        kind: kind.as_str().to_string(),
                        direction: direction.as_str().to_string(),
                        x: *x,
                        y: *y,
                    },
                );
            }
        }
    }
    snap
}
