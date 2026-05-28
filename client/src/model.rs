//! The pure client model: View-window subscription, snapshot merge, inventory,
//! realm, and the input→intent / click→verb decisions. No rendering, no I/O —
//! fed decoded wire events and user input, it emits subscription + send commands
//! and exposes the observable state the view renders. The native analog of the
//! old `window.__game`. Positions are sub-units (1 world unit = 1000).

use crate::dev::DevState;
use protocol::consts::INTERACT_RANGE_SQ;
use protocol::geometry::{coord_for, neighborhood, ChunkCoord, SUB_UNITS_PER_UNIT};
use protocol::wire::{
    BuildPayload, ChunkSnapshot, DamagePayload, HarvestPayload, MovePayload, NodeWire, PlayerWire,
    PortalWire, RealmWire, RelocatedPayload, SelfPayload, StatsPayload, StructureWire,
};
use std::collections::{BTreeMap, BTreeSet};

/// The 3×3 View-window radius (Chebyshev) the client keeps subscribed.
const VIEW_RADIUS: i32 = 1;
/// Wall build cost in wood — a UX gate; the server is authoritative. Matches the
/// server catalogue's `[(Wood, 5)]`.
const WALL_COST: u32 = 5;

/// A command the model asks the transport layer to perform.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// Join the chunk channel for this coord (in the current realm).
    Subscribe(ChunkCoord),
    /// Leave the chunk channel for this coord.
    Unsubscribe(ChunkCoord),
    /// Push a verb on the player channel.
    Send(Outbound),
    /// Join the `dev:stats` channel (dev overlay turned on).
    SubscribeDevStats,
    /// Leave the `dev:stats` channel (dev overlay turned off).
    UnsubscribeDevStats,
}

/// A verb the client sends to the server.
#[derive(Debug, Clone, PartialEq)]
pub enum Outbound {
    Move(MovePayload),
    Harvest(HarvestPayload),
    Build(BuildPayload),
    Damage(DamagePayload),
}

pub struct ClientModel {
    username: String,
    realm: RealmWire,
    window_center: ChunkCoord,
    snaps: BTreeMap<ChunkCoord, ChunkSnapshot>,
    subscribed: BTreeSet<ChunkCoord>,
    inventory: BTreeMap<String, u32>,
    dev: DevState,
    last_intent: (f64, f64),
}

impl ClientModel {
    /// Connect at `initial_chunk` in the Overworld; returns the initial 3×3
    /// chunk subscriptions.
    pub fn new(username: &str, initial_chunk: ChunkCoord) -> (Self, Vec<Cmd>) {
        let want = window(initial_chunk);
        let model = ClientModel {
            username: username.to_string(),
            realm: RealmWire::Overworld,
            window_center: initial_chunk,
            snaps: BTreeMap::new(),
            subscribed: want.iter().copied().collect(),
            inventory: BTreeMap::new(),
            dev: DevState::default(),
            last_intent: (0.0, 0.0),
        };
        let cmds = want.into_iter().map(Cmd::Subscribe).collect();
        (model, cmds)
    }

    // --- inbound wire events ---

    /// Ingest a chunk snapshot; if the player's own position has crossed into a
    /// new chunk, pan the View window (returns the subscribe/unsubscribe diff).
    pub fn on_snapshot(&mut self, coord: ChunkCoord, snap: ChunkSnapshot) -> Vec<Cmd> {
        self.snaps.insert(coord, snap);
        self.maybe_shift_window()
    }

    pub fn on_self(&mut self, payload: SelfPayload) {
        self.inventory = payload.inventory;
    }

    /// The player changed realm/chunk: switch realm, recenter, drop all chunk
    /// state, and re-subscribe the new 3×3 (mirrors clearAllChunkSubscriptions).
    pub fn on_relocated(&mut self, payload: RelocatedPayload) -> Vec<Cmd> {
        let mut cmds: Vec<Cmd> = self.subscribed.iter().copied().map(Cmd::Unsubscribe).collect();
        self.realm = payload.realm;
        self.window_center = ChunkCoord::new(payload.coord[0], payload.coord[1]);
        self.snaps.clear();
        let want = window(self.window_center);
        self.subscribed = want.iter().copied().collect();
        cmds.extend(want.into_iter().map(Cmd::Subscribe));
        cmds
    }

    pub fn on_stats(&mut self, payload: StatsPayload) {
        self.dev.on_stats(payload);
    }

    /// Turn the dev overlay on/off (see [`DevState::set`]).
    pub fn set_dev(&mut self, on: bool) -> Vec<Cmd> {
        self.dev.set(on)
    }

    // --- user input ---

    /// Set the WASD key state; emits a `move` only when the normalized intent
    /// changes (matching the old client's de-duped push). `dy` is south−north,
    /// `dx` is east−west, to match the server's axis convention.
    pub fn set_movement(&mut self, north: bool, south: bool, east: bool, west: bool) -> Vec<Cmd> {
        let dx = (east as i32 - west as i32) as f64;
        let dy = (south as i32 - north as i32) as f64;
        let len = (dx * dx + dy * dy).sqrt();
        let intent = if len == 0.0 { (0.0, 0.0) } else { (dx / len, dy / len) };
        if intent == self.last_intent {
            return Vec::new();
        }
        self.last_intent = intent;
        vec![Cmd::Send(Outbound::Move(MovePayload { dx: intent.0, dy: intent.1 }))]
    }

    /// A click at world-unit `(wx, wy)`: harvest a live tree there, else damage a
    /// structure there, else build a wall on the empty cell if affordable and in
    /// range. Mirrors the old `handleWorldClick`.
    pub fn click(&self, wx: f64, wy: f64) -> Vec<Cmd> {
        let Some(me) = self.player_pos(&self.username) else {
            return Vec::new();
        };
        let tol = SUB_UNITS_PER_UNIT / 2; // 0.5 world units, in sub-units
        let cx = (wx * SUB_UNITS_PER_UNIT as f64).round() as i64;
        let cy = (wy * SUB_UNITS_PER_UNIT as f64).round() as i64;

        // 1) live tree at the click?
        for node in self.nodes().values() {
            if !node.depleted && (node.x - cx).abs() < tol && (node.y - cy).abs() < tol {
                return vec![Cmd::Send(Outbound::Harvest(HarvestPayload { x: node.x, y: node.y }))];
            }
        }
        // 2) structure at the click?
        for s in self.structures().values() {
            if (s.x - cx).abs() < tol && (s.y - cy).abs() < tol {
                return vec![Cmd::Send(Outbound::Damage(DamagePayload { x: s.x, y: s.y }))];
            }
        }
        // 3) build on the empty cell, if we can afford it and it's in range.
        if self.inventory.get("wood").copied().unwrap_or(0) < WALL_COST {
            return Vec::new();
        }
        let sub = SUB_UNITS_PER_UNIT;
        let cell_x = (wx.floor() as i64) * sub + sub / 2;
        let cell_y = (wy.floor() as i64) * sub + sub / 2;
        let dx = me.x - cell_x;
        let dy = me.y - cell_y;
        if dx * dx + dy * dy > INTERACT_RANGE_SQ {
            return Vec::new();
        }
        vec![Cmd::Send(Outbound::Build(BuildPayload {
            kind: "wall".to_string(),
            x: cell_x,
            y: cell_y,
        }))]
    }

    // --- observable state (the view + tests read these) ---

    pub fn realm(&self) -> RealmWire {
        self.realm
    }
    pub fn inventory(&self) -> &BTreeMap<String, u32> {
        &self.inventory
    }
    pub fn stats(&self) -> Option<&StatsPayload> {
        self.dev.stats()
    }
    pub fn dev_enabled(&self) -> bool {
        self.dev.enabled()
    }
    pub fn subscribed(&self) -> &BTreeSet<ChunkCoord> {
        &self.subscribed
    }
    pub fn window_center(&self) -> ChunkCoord {
        self.window_center
    }

    /// All players currently visible, merged across subscribed chunk snapshots.
    pub fn players(&self) -> BTreeMap<String, PlayerWire> {
        let mut out = BTreeMap::new();
        for snap in self.snaps.values() {
            for (name, p) in &snap.players {
                out.insert(name.clone(), *p);
            }
        }
        out
    }

    pub fn player_pos(&self, name: &str) -> Option<PlayerWire> {
        self.players().get(name).copied()
    }

    pub fn nodes(&self) -> BTreeMap<String, NodeWire> {
        merge(&self.snaps, |s| &s.resource_nodes)
    }
    pub fn structures(&self) -> BTreeMap<String, StructureWire> {
        merge(&self.snaps, |s| &s.structures)
    }
    pub fn portals(&self) -> BTreeMap<String, PortalWire> {
        merge(&self.snaps, |s| &s.portals)
    }

    /// The dev-HUD "view" count: number of players currently rendered.
    pub fn view_count(&self) -> usize {
        self.players().len()
    }

    // --- internals ---

    fn maybe_shift_window(&mut self) -> Vec<Cmd> {
        let Some(me) = self.player_pos(&self.username) else {
            return Vec::new();
        };
        let now = coord_for(me.x, me.y);
        if now == self.window_center {
            return Vec::new();
        }
        let want: BTreeSet<ChunkCoord> = window(now).into_iter().collect();
        let mut cmds = Vec::new();
        for old in self.subscribed.difference(&want) {
            cmds.push(Cmd::Unsubscribe(*old));
            self.snaps.remove(old);
        }
        for new in want.difference(&self.subscribed) {
            cmds.push(Cmd::Subscribe(*new));
        }
        self.subscribed = want;
        self.window_center = now;
        cmds
    }
}

/// The 3×3 chunk window centered on `c`.
fn window(c: ChunkCoord) -> Vec<ChunkCoord> {
    neighborhood(c, VIEW_RADIUS)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(x: i32, y: i32) -> ChunkCoord {
        ChunkCoord::new(x, y)
    }

    fn snap_with_player(name: &str, x: i64, y: i64) -> ChunkSnapshot {
        let mut s = ChunkSnapshot::default();
        s.players.insert(name.into(), PlayerWire { x, y });
        s
    }

    #[test]
    fn new_subscribes_the_3x3() {
        let (m, cmds) = ClientModel::new("alice", cc(0, 0));
        assert_eq!(m.subscribed().len(), 9);
        assert!(m.subscribed().contains(&cc(-1, -1)) && m.subscribed().contains(&cc(1, 1)));
        assert_eq!(cmds.iter().filter(|c| matches!(c, Cmd::Subscribe(_))).count(), 9);
        assert_eq!(m.realm(), RealmWire::Overworld);
    }

    #[test]
    fn merges_players_across_chunks() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        m.on_snapshot(cc(0, 0), snap_with_player("alice", 8_000, 8_000));
        m.on_snapshot(cc(1, 0), snap_with_player("bob", 20_000, 8_000));
        let players = m.players();
        assert_eq!(players.len(), 2);
        assert_eq!(m.player_pos("alice"), Some(PlayerWire { x: 8_000, y: 8_000 }));
        assert_eq!(m.player_pos("bob"), Some(PlayerWire { x: 20_000, y: 8_000 }));
        assert_eq!(m.view_count(), 2);
    }

    #[test]
    fn crossing_a_boundary_pans_the_window() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        // alice was at (0,0); a snapshot puts her in chunk (1,0).
        let cmds = m.on_snapshot(cc(1, 0), snap_with_player("alice", 16_500, 8_000));
        assert_eq!(m.window_center(), cc(1, 0));
        // Window is now centered on (1,0): owns 0..2 × -1..1.
        assert!(m.subscribed().contains(&cc(2, 0)));
        assert!(!m.subscribed().contains(&cc(-1, 0)));
        // The diff unsubscribes the western column and subscribes the eastern one.
        assert!(cmds.contains(&Cmd::Unsubscribe(cc(-1, 0))));
        assert!(cmds.contains(&Cmd::Subscribe(cc(2, 0))));
        // No spurious change if she stays put.
        let again = m.on_snapshot(cc(1, 0), snap_with_player("alice", 16_600, 8_000));
        assert!(again.is_empty());
    }

    #[test]
    fn relocated_switches_realm_and_resubscribes() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        m.on_snapshot(cc(0, 0), snap_with_player("alice", 8_000, 8_000));
        let cmds = m.on_relocated(RelocatedPayload {
            realm: RealmWire::Instance { id: 7 },
            coord: [1, 1],
        });
        assert_eq!(m.realm(), RealmWire::Instance { id: 7 });
        assert_eq!(m.window_center(), cc(1, 1));
        // Old chunk state is cleared; the new 3×3 is subscribed.
        assert!(m.players().is_empty());
        assert!(m.subscribed().contains(&cc(1, 1)) && m.subscribed().contains(&cc(0, 0)));
        assert!(cmds.iter().any(|c| matches!(c, Cmd::Unsubscribe(_))));
        assert_eq!(cmds.iter().filter(|c| matches!(c, Cmd::Subscribe(_))).count(), 9);
    }

    #[test]
    fn self_event_sets_inventory() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        let mut inv = BTreeMap::new();
        inv.insert("wood".to_string(), 3);
        m.on_self(SelfPayload { inventory: inv });
        assert_eq!(m.inventory().get("wood"), Some(&3));
    }

    #[test]
    fn movement_intent_normalizes_and_dedupes() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        // East only → (1,0), one Move.
        let c1 = m.set_movement(false, false, true, false);
        assert_eq!(c1, vec![Cmd::Send(Outbound::Move(MovePayload { dx: 1.0, dy: 0.0 }))]);
        // Same keys again → no command (de-duped).
        assert!(m.set_movement(false, false, true, false).is_empty());
        // Diagonal SE → normalized.
        let c2 = m.set_movement(false, true, true, false);
        if let Cmd::Send(Outbound::Move(MovePayload { dx, dy })) = &c2[0] {
            assert!((dx - 0.70710678).abs() < 1e-6 && (dy - 0.70710678).abs() < 1e-6);
        } else {
            panic!("expected a Move");
        }
        // Release all → (0,0).
        let c3 = m.set_movement(false, false, false, false);
        assert_eq!(c3, vec![Cmd::Send(Outbound::Move(MovePayload { dx: 0.0, dy: 0.0 }))]);
    }

    fn model_with_player_at(x: i64, y: i64) -> ClientModel {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        m.on_snapshot(cc(0, 0), snap_with_player("alice", x, y));
        m
    }

    #[test]
    fn click_harvests_a_live_tree() {
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.resource_nodes.insert(
            "tree:8000:8000".into(),
            NodeWire { kind: "tree".into(), x: 8_000, y: 8_000, depleted: false },
        );
        m.on_snapshot(cc(0, 0), snap);
        // Click at world (8,8) — the tree.
        let cmds = m.click(8.0, 8.0);
        assert_eq!(cmds, vec![Cmd::Send(Outbound::Harvest(HarvestPayload { x: 8_000, y: 8_000 }))]);
    }

    #[test]
    fn click_ignores_a_depleted_tree() {
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.resource_nodes.insert(
            "tree:8000:8000".into(),
            NodeWire { kind: "tree".into(), x: 8_000, y: 8_000, depleted: true },
        );
        m.on_snapshot(cc(0, 0), snap);
        // Depleted tree → no harvest; empty inventory → no build either.
        assert!(m.click(8.0, 8.0).is_empty());
    }

    #[test]
    fn click_damages_a_structure() {
        let mut m = model_with_player_at(3_000, 3_000);
        let mut snap = snap_with_player("alice", 3_000, 3_000);
        snap.structures.insert(
            "structure:3500:3000".into(),
            StructureWire { kind: "wall".into(), x: 3_500, y: 3_000, hp: 100, owner: "bob".into() },
        );
        m.on_snapshot(cc(0, 0), snap);
        let cmds = m.click(3.5, 3.0);
        assert_eq!(cmds, vec![Cmd::Send(Outbound::Damage(DamagePayload { x: 3_500, y: 3_000 }))]);
    }

    #[test]
    fn click_builds_on_empty_cell_when_affordable_and_in_range() {
        let mut m = model_with_player_at(3_200, 3_200);
        let mut inv = BTreeMap::new();
        inv.insert("wood".to_string(), 5);
        m.on_self(SelfPayload { inventory: inv });
        // Click in the player's own cell (world 3.x → cell centre 3500,3500).
        let cmds = m.click(3.2, 3.2);
        assert_eq!(
            cmds,
            vec![Cmd::Send(Outbound::Build(BuildPayload { kind: "wall".into(), x: 3_500, y: 3_500 }))]
        );
    }

    #[test]
    fn click_build_gated_by_materials_and_range() {
        // Affordable but far away → out of range, no build.
        let mut far = model_with_player_at(0, 0);
        let mut inv = BTreeMap::new();
        inv.insert("wood".to_string(), 5);
        far.on_self(SelfPayload { inventory: inv });
        assert!(far.click(50.0, 50.0).is_empty(), "out of interact range");

        // In range but no wood → no build.
        let near = model_with_player_at(3_200, 3_200);
        assert!(near.click(3.2, 3.2).is_empty(), "insufficient materials");
    }
}
