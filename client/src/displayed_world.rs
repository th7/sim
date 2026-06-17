//! The **displayed world**: the observable state the view draws — authoritative
//! per-chunk snapshot *facts* (entities, depletion, inventory side) overlaid
//! with the **Mirror**'s speculated *positions*. This is the one place `snaps`
//! and `mirror` combine; [`crate::session::RenderState`] is the owned snapshot of
//! it the render thread reads.
//!
//! A borrowing view over the [`ClientModel`](crate::model::ClientModel)'s state:
//! cheap to construct per frame, computed on demand. Positions for Players and
//! NPCs are the Mirror's; Resource nodes, Structures, Portals, and Carcasses are
//! pure snapshot facts (discrete state the Mirror never speculates).

use crate::mirror::Mirror;
use crate::model::ActionButton;
use protocol::consts::INTERACT_RANGE_SQ;
use protocol::geometry::ChunkCoord;
use protocol::wire::{
    CarcassWire, ChunkSnapshot, NodeWire, NpcWire, PlayerWire, PortalWire, StructureWire,
};
use std::collections::BTreeMap;

/// A borrowing view of the observable world: snapshot facts ∪ Mirror-speculated
/// positions. Built by [`ClientModel::displayed`](crate::model::ClientModel::displayed).
pub struct DisplayedWorld<'a> {
    pub(crate) snaps: &'a BTreeMap<ChunkCoord, ChunkSnapshot>,
    pub(crate) mirror: &'a Mirror,
    pub(crate) username: &'a str,
    pub(crate) target: Option<&'a str>,
}

impl DisplayedWorld<'_> {
    /// Every visible Player: facts merged across snapshots, positions the
    /// Mirror's. The own Player is present whenever the Mirror has it —
    /// independent of which chunk's snapshot last listed us, so a boundary
    /// crossing can never blink us out.
    pub fn players(&self) -> BTreeMap<String, PlayerWire> {
        let mut out = BTreeMap::new();
        for snap in self.snaps.values() {
            for (name, p) in &snap.players {
                out.insert(name.clone(), *p);
            }
        }
        for (name, p) in out.iter_mut() {
            if let Some((x, y)) = self.mirror.position_of(name) {
                p.x = x;
                p.y = y;
            }
        }
        if !out.contains_key(self.username) {
            if let Some((x, y)) = self.mirror.position_of(self.username) {
                out.insert(self.username.to_string(), PlayerWire { x, y, ..PlayerWire::default() });
            }
        }
        out
    }

    pub fn player_pos(&self, name: &str) -> Option<PlayerWire> {
        self.players().get(name).copied()
    }

    pub fn nodes(&self) -> BTreeMap<String, NodeWire> {
        merge(self.snaps, |s| &s.resource_nodes)
    }
    pub fn structures(&self) -> BTreeMap<String, StructureWire> {
        merge(self.snaps, |s| &s.structures)
    }
    pub fn portals(&self) -> BTreeMap<String, PortalWire> {
        merge(self.snaps, |s| &s.portals)
    }
    /// All NPCs (wolves/deer) currently visible: facts merged across snapshots,
    /// positions speculated by the Mirror.
    pub fn npcs(&self) -> BTreeMap<String, NpcWire> {
        let mut out: BTreeMap<String, NpcWire> = merge(self.snaps, |s| &s.npcs);
        for (id, n) in out.iter_mut() {
            if let Some((x, y)) = self.mirror.npc_position_of(id) {
                n.x = x;
                n.y = y;
            }
        }
        out
    }
    /// All Carcasses currently visible, merged across snapshots.
    pub fn carcasses(&self) -> BTreeMap<String, CarcassWire> {
        merge(self.snaps, |s| &s.carcasses)
    }

    /// The targetable entity at a click, if any: the WireId of the *nearest*
    /// Resource node, Structure, NPC, or Carcass whose rendered position is
    /// within `tol` (sub-units) of `(cx, cy)` — nearest, not first-category, so
    /// a wolf beside a tree doesn't lose the click to the tree. Players and
    /// Portals are not targetable.
    pub fn targetable_at(&self, cx: i64, cy: i64, tol: i64) -> Option<String> {
        let mut best: Option<(i64, String)> = None;
        let mut consider = |x: i64, y: i64, wid: String| {
            if (x - cx).abs() < tol && (y - cy).abs() < tol {
                let d = (x - cx) * (x - cx) + (y - cy) * (y - cy);
                if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, wid));
                }
            }
        };
        for (wid, n) in self.nodes() {
            consider(n.x, n.y, wid);
        }
        for (wid, s) in self.structures() {
            consider(s.x, s.y, wid);
        }
        for (wid, npc) in self.npcs() {
            consider(npc.x, npc.y, wid);
        }
        for (wid, c) in self.carcasses() {
            consider(c.x, c.y, wid);
        }
        best.map(|(_, wid)| wid)
    }

    /// The Action the current Target implies and the entity's rendered position
    /// — `None` when no Target is visible.
    fn target_action_and_pos(&self) -> Option<(&'static str, i64, i64)> {
        let wid = self.target?;
        if let Some(n) = self.nodes().get(wid) {
            return Some(("harvest", n.x, n.y));
        }
        if let Some(c) = self.carcasses().get(wid) {
            return Some(("harvest", c.x, c.y));
        }
        if let Some(npc) = self.npcs().get(wid) {
            return Some(("damage", npc.x, npc.y));
        }
        if let Some(s) = self.structures().get(wid) {
            return Some(("damage", s.x, s.y));
        }
        None
    }

    /// The Action button's display state — see [`ActionButton`]. The range hint
    /// reads the lawful render (own Mirror position vs the Target's rendered
    /// position), the same frame the Island judges in.
    pub fn action_button(&self) -> ActionButton {
        let Some((verb, tx, ty)) = self.target_action_and_pos() else {
            return ActionButton::Inert;
        };
        let Some(me) = self.player_pos(self.username) else {
            return ActionButton::Dimmed(verb);
        };
        let (dx, dy) = (me.x - tx, me.y - ty);
        if dx * dx + dy * dy <= INTERACT_RANGE_SQ {
            ActionButton::Ready(verb)
        } else {
            ActionButton::Dimmed(verb)
        }
    }
}

fn merge<T: Clone>(
    snaps: &BTreeMap<ChunkCoord, ChunkSnapshot>,
    pick: impl Fn(&ChunkSnapshot) -> &BTreeMap<String, T>,
) -> BTreeMap<String, T> {
    let mut out = BTreeMap::new();
    for snap in snaps.values() {
        for (id, v) in pick(snap) {
            out.insert(id.clone(), v.clone());
        }
    }
    out
}
