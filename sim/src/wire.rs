//! Wire-facing observation: per-entity state and the per-chunk `snapshot`
//! payload, shaped to match the committed contract
//! (`contract/contract.json`) byte-for-byte. Positions are
//! sub-unit integers; the client divides by 1000.
//!
//! The cluster authors observation as changed-only deltas (see
//! [`crate::delta`]); a full [`ChunkSnapshot`] is just the current state of one
//! chunk, which is what the `snapshot` wire event carries.

use crate::components::*;
use crate::geometry::ChunkCoord;
use crate::ids::Realm;
use crate::world::RealmWorld;
use serde_json::{json, Value};
use std::collections::BTreeMap;

// The serializable per-chunk snapshot payloads now live in the shared `protocol`
// crate (so the client deserializes the exact structs the server serializes).
pub use protocol::wire::{
    CarcassWire, ChunkSnapshot, NodeWire, NpcWire, PlayerWire, PortalWire, StructureWire,
};

/// The `self` event payload: `{"inventory": {"wood": 3, ...}}` (string-keyed).
pub fn inventory_payload(inv: &Inventory) -> Value {
    let items: BTreeMap<String, u32> =
        inv.items.iter().map(|(k, v)| (k.as_str().to_string(), *v)).collect();
    json!({ "inventory": items })
}

/// Serialize a realm to the contract's `relocated.realm` shape.
pub fn realm_value(realm: Realm) -> Value {
    match realm {
        Realm::Overworld => json!({ "kind": "overworld" }),
        Realm::Instance(id) => json!({ "kind": "instance", "id": id }),
    }
}

/// The `relocated` event payload: `{"realm": {...}, "coord": [cx, cy]}`.
pub fn relocated_payload(realm: Realm, coord: ChunkCoord) -> Value {
    json!({ "realm": realm_value(realm), "coord": [coord.cx, coord.cy] })
}

/// The `action_rejected` push: a queued action could not be carried out, with the
/// originating verb + target cell (to correlate it to the player's input) and the
/// reason (`queue_full` or a tick-time verb error).
pub fn action_rejected_payload(verb: &str, x: i64, y: i64, reason: &str) -> Value {
    json!({ "verb": verb, "x": x, "y": y, "reason": reason })
}

/// The `ack` push: the last-consumed movement input seq as of `tick` — the
/// authoritative anchor the client's Mirror replays its unacked frames on.
pub fn move_ack_payload(seq: u32, tick: u64) -> Value {
    json!({ "seq": seq, "tick": tick })
}

/// The observable state of one entity, keyed on the wire by its [`WireId`].
/// Equality drives changed-only delta diffing.
#[derive(Debug, Clone, PartialEq)]
pub enum EntityWire {
    Player { x: i64, y: i64, vx: f64, vy: f64 },
    Node { kind: ResourceKind, x: i64, y: i64, depleted: bool },
    Structure { kind: StructureKind, owner: String, hp: i64, x: i64, y: i64 },
    Portal { kind: PortalKind, direction: PortalDirection, x: i64, y: i64 },
    Npc { kind: crate::motivation::NpcKind, hp: i64, x: i64, y: i64, vx: f64, vy: f64 },
    Carcass { meat: i64, x: i64, y: i64 },
}

impl EntityWire {
    pub fn position(&self) -> (i64, i64) {
        match *self {
            EntityWire::Player { x, y, .. }
            | EntityWire::Node { x, y, .. }
            | EntityWire::Structure { x, y, .. }
            | EntityWire::Portal { x, y, .. }
            | EntityWire::Npc { x, y, .. }
            | EntityWire::Carcass { x, y, .. } => (x, y),
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

    for (_e, (pos, vel, _pc, wid)) in
        world.query::<(&Position, Option<&Velocity>, &PlayerControlled, &WireId)>().iter()
    {
        let (vx, vy) = vel.map_or((0.0, 0.0), |v| (v.vx, v.vy));
        out.insert(wid.clone(), EntityWire::Player { x: pos.x, y: pos.y, vx, vy });
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
    for (_e, (pos, vel, npc, h, wid)) in
        world.query::<(&Position, Option<&Velocity>, &Npc, &Health, &WireId)>().iter()
    {
        let (vx, vy) = vel.map_or((0.0, 0.0), |v| (v.vx, v.vy));
        out.insert(
            wid.clone(),
            EntityWire::Npc { kind: npc.kind, hp: h.hp, x: pos.x, y: pos.y, vx, vy },
        );
    }
    for (_e, (pos, c, wid)) in world.query::<(&Position, &Carcass, &WireId)>().iter() {
        out.insert(wid.clone(), EntityWire::Carcass { meat: c.meat, x: pos.x, y: pos.y });
    }
    out
}

/// Build the full snapshot of one chunk from a set of entity states (those
/// whose position falls in `coord`), stamped with the server `tick` it
/// captures.
pub fn chunk_snapshot(
    states: &BTreeMap<WireId, EntityWire>,
    coord: ChunkCoord,
    tick: u64,
) -> ChunkSnapshot {
    let mut snap = ChunkSnapshot { tick, ..ChunkSnapshot::default() };
    for (wid, state) in states {
        if state.chunk() != coord {
            continue;
        }
        match state {
            EntityWire::Player { x, y, vx, vy } => {
                snap.players
                    .insert(wid.0.clone(), PlayerWire { x: *x, y: *y, vx: *vx, vy: *vy });
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
            EntityWire::Npc { kind, hp, x, y, vx, vy } => {
                snap.npcs.insert(
                    wid.0.clone(),
                    NpcWire {
                        kind: kind.as_str().to_string(),
                        x: *x,
                        y: *y,
                        hp: *hp,
                        vx: *vx,
                        vy: *vy,
                    },
                );
            }
            EntityWire::Carcass { meat, x, y } => {
                snap.carcasses.insert(wid.0.clone(), CarcassWire { x: *x, y: *y, meat: *meat });
            }
        }
    }
    snap
}
