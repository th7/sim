//! The pure client model: View-window subscription, snapshot merge, inventory,
//! realm, and the input→intent / click→verb decisions. No rendering, no I/O —
//! fed decoded wire events and user input, it emits subscription + send commands
//! and exposes the observable state the view renders. The native analog of the
//! old `window.__game`. Positions are sub-units (1 world unit = 1000).

use crate::dev::DevState;
use crate::mirror::Mirror;
use protocol::consts::{INTERACT_RANGE_SQ, WALL_COST};
use protocol::geometry::{coord_for, neighborhood, ChunkCoord, SUB_UNITS_PER_UNIT};
use protocol::wire::{
    AckPayload, BuildPayload, CarcassWire, ChunkSnapshot, DamagePayload, HarvestPayload,
    MovePayload, NodeWire, NpcWire, PlayerWire, PortalWire, RealmWire, RelocatedPayload,
    SelfPayload, StatsPayload, StructureWire,
};
use std::collections::{BTreeMap, BTreeSet};

/// The 3×3 View-window radius (Chebyshev) the client keeps subscribed.
const VIEW_RADIUS: i32 = 1;
// Wall build cost is `protocol::consts::WALL_COST` — the same source the server
// catalogue uses (a client-side UX gate; the server stays authoritative).

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
    /// The intent carried by the last sent input frame (None before the first
    /// frame) — decides whether a release still owes its final zero-frame.
    last_sent_intent: Option<(f64, f64)>,
    /// Seq of the last sent movement input frame (the server acks the last
    /// consumed seq back; the Mirror replays everything after it).
    move_seq: u32,
    last_error: Option<String>,
    /// The current Target: the one entity designated to receive the next
    /// entity-directed Verb (its WireId). Sticky observation — see the Target
    /// glossary entry; set by clicking a targetable entity, cleared explicitly
    /// or when the entity ceases to be visible.
    target: Option<String>,
    /// The speculative simulation of the View window (see `crate::mirror`).
    /// Owns the rendered positions; the per-chunk snaps remain the merge of
    /// authoritative *facts* (entities, depletion, inventory side of view).
    mirror: Mirror,
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
            last_sent_intent: None,
            move_seq: 0,
            last_error: None,
            target: None,
            mirror: Mirror::new(username),
        };
        let cmds = want.into_iter().map(Cmd::Subscribe).collect();
        (model, cmds)
    }

    // --- inbound wire events ---

    /// Ingest a chunk snapshot; if the player's own position has crossed into a
    /// new chunk, pan the View window (returns the subscribe/unsubscribe diff).
    pub fn on_snapshot(&mut self, coord: ChunkCoord, snap: ChunkSnapshot) -> Vec<Cmd> {
        self.mirror.on_snapshot(coord, &snap);
        self.snaps.insert(coord, snap);
        self.maybe_shift_window()
    }

    /// The server consumed our input frames through `seq` as of `tick` — the
    /// Mirror's replay anchor.
    pub fn on_ack(&mut self, payload: AckPayload) {
        self.mirror.on_ack(payload.seq, payload.tick);
    }

    /// Whether the Mirror is frozen (born, at the Lead bound, or reset) — the
    /// view shows this as a connection signal, not silently stale state.
    pub fn mirror_frozen(&self) -> bool {
        self.mirror.frozen()
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
        // Born frozen again: the new realm speculates only from its own authority.
        self.mirror.reset();
        let want = window(self.window_center);
        self.subscribed = want.iter().copied().collect();
        cmds.extend(want.into_iter().map(Cmd::Subscribe));
        cmds
    }

    pub fn on_stats(&mut self, payload: StatsPayload) {
        self.dev.on_stats(payload);
    }

    /// A verb (harvest/build/damage) was rejected by the server. Surface the
    /// reason so the view can show it instead of failing silently.
    pub fn on_verb_error(&mut self, reason: String) {
        self.last_error = Some(reason);
    }

    /// Turn the dev overlay on/off (see [`DevState::set`]).
    pub fn set_dev(&mut self, on: bool) -> Vec<Cmd> {
        self.dev.set(on)
    }

    // --- user input ---

    /// Set the WASD key state. State-only: frames go out via [`Self::input_frame`]
    /// on the tick cadence — Intent is perishable server-side, so holding a key
    /// means *renewing* it, not announcing it once. `dy` is south−north, `dx` is
    /// east−west, to match the server's axis convention.
    pub fn set_movement(&mut self, north: bool, south: bool, east: bool, west: bool) -> Vec<Cmd> {
        let dx = (east as i32 - west as i32) as f64;
        let dy = (south as i32 - north as i32) as f64;
        let len = (dx * dx + dy * dy).sqrt();
        self.last_intent = if len == 0.0 { (0.0, 0.0) } else { (dx / len, dy / len) };
        Vec::new()
    }

    /// One client tick: emit this tick's movement input frame if the session
    /// owes one (a frame per tick while the intent is nonzero, one final
    /// zero-frame on release, silence while idle), feed it to the Mirror, and
    /// advance the Mirror. While the Mirror is frozen, inputs stall — nothing
    /// is sent and nothing speculates.
    pub fn input_frame(&mut self) -> Vec<Cmd> {
        if self.mirror.frozen() {
            self.mirror.tick(); // inert; keeps the call-shape uniform
            return Vec::new();
        }
        let moving = self.last_intent != (0.0, 0.0);
        let releasing = !moving && self.last_sent_intent.is_some_and(|s| s != (0.0, 0.0));
        if !moving && !releasing {
            self.mirror.tick(); // idle ticks still advance everyone else
            return Vec::new();
        }
        self.last_sent_intent = Some(self.last_intent);
        self.move_seq += 1;
        let (dx, dy) = self.last_intent;
        self.mirror.push_input(self.move_seq, dx, dy);
        self.mirror.tick();
        vec![Cmd::Send(Outbound::Move(MovePayload { seq: self.move_seq, dx, dy }))]
    }

    /// A click at world-unit `(wx, wy)`: if a targetable entity (Resource node
    /// — live *or* depleted — Structure, NPC, or Carcass) is at the click,
    /// designate it the Target and do nothing else. Otherwise the click keeps
    /// its build meaning: place a wall on the empty cell if affordable and in
    /// range. Clicking elsewhere never clears the Target — [`Self::escape`]
    /// does.
    pub fn click(&mut self, wx: f64, wy: f64) -> Vec<Cmd> {
        let tol = SUB_UNITS_PER_UNIT / 2; // 0.5 world units, in sub-units
        let cx = (wx * SUB_UNITS_PER_UNIT as f64).round() as i64;
        let cy = (wy * SUB_UNITS_PER_UNIT as f64).round() as i64;
        if let Some(wid) = self.targetable_at(cx, cy, tol) {
            self.target = Some(wid);
            return Vec::new();
        }
        let cmds = self.build_click(wx, wy);
        if cmds.iter().any(|c| matches!(c, Cmd::Send(_))) {
            // Issuing a verb clears the stale `last_error` — the user is
            // retrying; the outcome arrives async.
            self.last_error = None;
        }
        cmds
    }

    /// The targetable entity at the click, if any: the WireId of a Resource
    /// node, Structure, NPC, or Carcass whose rendered position is within
    /// `tol`. Players and Portals are not targetable.
    fn targetable_at(&self, cx: i64, cy: i64, tol: i64) -> Option<String> {
        let hit = |x: i64, y: i64| (x - cx).abs() < tol && (y - cy).abs() < tol;
        for (wid, node) in self.nodes() {
            if hit(node.x, node.y) {
                return Some(wid);
            }
        }
        for (wid, s) in self.structures() {
            if hit(s.x, s.y) {
                return Some(wid);
            }
        }
        for (wid, npc) in self.npcs() {
            if hit(npc.x, npc.y) {
                return Some(wid);
            }
        }
        for (wid, c) in self.carcasses() {
            if hit(c.x, c.y) {
                return Some(wid);
            }
        }
        None
    }

    /// The click-to-build path: place a wall on the clicked empty cell if
    /// affordable and in range (a client-side UX gate; the server stays
    /// authoritative).
    fn build_click(&self, wx: f64, wy: f64) -> Vec<Cmd> {
        let Some(me) = self.player_pos(&self.username) else {
            return Vec::new();
        };
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

    /// Clear the Target (the Escape key). The only explicit clear — clicking
    /// elsewhere deliberately leaves the Target alone.
    pub fn escape(&mut self) {
        self.target = None;
    }

    /// The current Target's WireId, if any.
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// The Verb button: issue the entity-directed Verb the current Target
    /// implies — a Gatherable (Resource node or Carcass) → harvest. Inert
    /// without a Target. With one, it always sends: eligibility (range, state)
    /// is the Island's to judge, and a refusal arrives async as
    /// `action_rejected` — the client never suppresses a press on speculated
    /// data.
    pub fn press_verb(&mut self) -> Vec<Cmd> {
        let Some(wid) = self.target.clone() else {
            return Vec::new();
        };
        let is_gatherable =
            self.nodes().contains_key(&wid) || self.carcasses().contains_key(&wid);
        if is_gatherable {
            self.last_error = None;
            return vec![Cmd::Send(Outbound::Harvest(HarvestPayload {
                target: wid,
                seq: self.move_seq,
            }))];
        }
        Vec::new()
    }

    // --- observable state (the view + tests read these) ---

    pub fn username(&self) -> &str {
        &self.username
    }

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
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
    pub fn subscribed(&self) -> &BTreeSet<ChunkCoord> {
        &self.subscribed
    }
    pub fn window_center(&self) -> ChunkCoord {
        self.window_center
    }

    /// All players currently visible: entity facts merged across subscribed
    /// chunk snapshots, positions speculated by the Mirror. The own player is
    /// present whenever the Mirror has it — independent of which chunk's
    /// snapshot last listed us, so a boundary crossing can never blink us out.
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
        if !out.contains_key(&self.username) {
            if let Some((x, y)) = self.mirror.position_of(&self.username) {
                out.insert(
                    self.username.clone(),
                    PlayerWire { x, y, ..PlayerWire::default() },
                );
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
    /// All NPCs (wolves/deer) currently visible: facts merged across
    /// snapshots, positions speculated by the Mirror.
    pub fn npcs(&self) -> BTreeMap<String, NpcWire> {
        let mut out: BTreeMap<String, NpcWire> = merge(&self.snaps, |s| &s.npcs);
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
        merge(&self.snaps, |s| &s.carcasses)
    }

    /// The dev-HUD "view" count: number of players currently rendered.
    pub fn view_count(&self) -> usize {
        self.players().len()
    }

    /// Dev-HUD: number of NPCs currently in view.
    pub fn npc_count(&self) -> usize {
        self.npcs().len()
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
        s.players.insert(name.into(), PlayerWire { x, y, ..PlayerWire::default() });
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
        assert_eq!(m.player_pos("alice"), Some(PlayerWire { x: 8_000, y: 8_000, ..PlayerWire::default() }));
        assert_eq!(m.player_pos("bob"), Some(PlayerWire { x: 20_000, y: 8_000, ..PlayerWire::default() }));
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

    /// The model's own player is the Mirror's speculation: an input frame
    /// advances the rendered position immediately, ahead of any server echo.
    #[test]
    fn own_position_is_the_mirrors_speculation() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        m.on_snapshot(cc(0, 0), snap_with_player("alice", 8_000, 8_000));
        m.set_movement(false, false, true, false);
        m.input_frame(); // one client tick
        let p = m.player_pos("alice").unwrap();
        assert_eq!((p.x, p.y), (8_200, 8_000), "one tick east, locally, immediately");
    }

    /// Born frozen: until the first authoritative snapshot, inputs stall — no
    /// frames are emitted. Relocation returns to the same state.
    #[test]
    fn inputs_stall_while_the_mirror_is_frozen() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        m.set_movement(false, false, true, false);
        assert!(m.input_frame().is_empty(), "born frozen — inputs stall");
        m.on_snapshot(cc(0, 0), snap_with_player("alice", 8_000, 8_000));
        assert!(!m.input_frame().is_empty(), "authority thaws the Mirror — frames flow");

        // Relocation: born frozen again, inputs stall until the new realm speaks.
        m.on_relocated(RelocatedPayload { realm: RealmWire::Instance { id: 7 }, coord: [1, 1] });
        assert!(m.input_frame().is_empty(), "reset — inputs stall in the new realm");
    }

    /// Intent is perishable server-side: a held key *renews* it with one frame
    /// per tick; release owes exactly one zero-frame; idle is silence.
    #[test]
    fn movement_frames_renew_per_tick_and_release_once() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        m.on_snapshot(cc(0, 0), snap_with_player("alice", 8_000, 8_000));
        // Idle → silence.
        assert!(m.input_frame().is_empty());
        // East held → one frame per tick, seqs increasing.
        assert!(m.set_movement(false, false, true, false).is_empty(), "set_movement is state-only");
        let f1 = m.input_frame();
        assert_eq!(f1, vec![Cmd::Send(Outbound::Move(MovePayload { seq: 1, dx: 1.0, dy: 0.0 }))]);
        let f2 = m.input_frame();
        assert_eq!(f2, vec![Cmd::Send(Outbound::Move(MovePayload { seq: 2, dx: 1.0, dy: 0.0 }))]);
        // Diagonal SE → the next frame carries the normalized intent.
        m.set_movement(false, true, true, false);
        if let Cmd::Send(Outbound::Move(MovePayload { dx, dy, .. })) = &m.input_frame()[0] {
            assert!((dx - 0.70710678).abs() < 1e-6 && (dy - 0.70710678).abs() < 1e-6);
        } else {
            panic!("expected a Move");
        }
        // Release → exactly one zero-frame, then silence.
        m.set_movement(false, false, false, false);
        assert_eq!(
            m.input_frame(),
            vec![Cmd::Send(Outbound::Move(MovePayload { seq: 4, dx: 0.0, dy: 0.0 }))]
        );
        assert!(m.input_frame().is_empty(), "the zero-frame is owed once");
    }

    fn model_with_player_at(x: i64, y: i64) -> ClientModel {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        m.on_snapshot(cc(0, 0), snap_with_player("alice", x, y));
        m
    }

    #[test]
    fn snapshot_npcs_and_carcasses_are_visible() {
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.npcs.insert(
            "npc:wolf:3".into(),
            NpcWire { kind: "wolf".into(), x: 8_200, y: 8_100, hp: 80, ..NpcWire::default() },
        );
        snap.carcasses.insert("carcass:9".into(), CarcassWire { x: 8_300, y: 8_300, meat: 3 });
        m.on_snapshot(cc(0, 0), snap);
        assert_eq!(m.npc_count(), 1);
        assert_eq!(m.npcs().get("npc:wolf:3").unwrap().kind, "wolf");
        assert_eq!(m.carcasses().get("carcass:9").unwrap().meat, 3);
    }

    /// Demeanor and hp are discrete authoritative facts: the Mirror speculates
    /// an NPC's position between snapshots but never its Demeanor or Health —
    /// both read exactly as the last snapshot said, however far the Lead runs.
    #[test]
    fn mirror_speculates_npc_position_but_never_demeanor_or_hp() {
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.npcs.insert(
            "npc:wolf:3".into(),
            NpcWire {
                kind: "wolf".into(),
                x: 8_200,
                y: 8_100,
                hp: 27,
                vx: 2_000.0,
                vy: 0.0,
                demeanor: "aggressive".into(),
            },
        );
        m.on_snapshot(cc(0, 0), snap);
        for _ in 0..5 {
            let _ = m.input_frame();
        }
        let n = m.npcs().get("npc:wolf:3").cloned().unwrap();
        assert!(n.x > 8_200, "position speculates along the last-known Intent");
        assert_eq!(n.demeanor, "aggressive", "Demeanor is never speculated");
        assert_eq!(n.hp, 27, "Health is never speculated");
    }

    #[test]
    fn click_targets_an_npc_and_sends_nothing() {
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.npcs.insert(
            "npc:deer:5".into(),
            NpcWire { kind: "deer".into(), x: 8_200, y: 8_000, hp: 50, ..NpcWire::default() },
        );
        m.on_snapshot(cc(0, 0), snap);
        // Click on the deer at world (8.2, 8.0) → it becomes the Target, only.
        assert!(m.click(8.2, 8.0).is_empty(), "clicking selects only");
        assert_eq!(m.target(), Some("npc:deer:5"));
    }

    #[test]
    fn click_targets_a_tree_and_the_verb_button_harvests_by_identity() {
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.resource_nodes.insert(
            "tree:8000:8000".into(),
            NodeWire { kind: "tree".into(), x: 8_000, y: 8_000, depleted: false },
        );
        m.on_snapshot(cc(0, 0), snap);
        // Click at world (8,8) — the tree becomes the Target; nothing is sent.
        assert!(m.click(8.0, 8.0).is_empty(), "clicking selects only");
        assert_eq!(m.target(), Some("tree:8000:8000"));
        // The Verb button issues the harvest at the Target's identity.
        let cmds = m.press_verb();
        assert_eq!(
            cmds,
            vec![Cmd::Send(Outbound::Harvest(HarvestPayload {
                target: "tree:8000:8000".into(),
                seq: 0,
            }))]
        );
    }

    #[test]
    fn a_depleted_tree_is_targetable_and_the_press_still_sends() {
        // Always-send: state (depleted) is the Island's to judge — the client
        // never suppresses a press on its own facts. And the depleted-tree
        // click no longer falls through toward build (the old wasted-wood bug).
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.resource_nodes.insert(
            "tree:8000:8000".into(),
            NodeWire { kind: "tree".into(), x: 8_000, y: 8_000, depleted: true },
        );
        m.on_snapshot(cc(0, 0), snap);
        m.on_self(SelfPayload {
            inventory: BTreeMap::from([("wood".to_string(), WALL_COST)]),
        });
        let cmds = m.click(8.0, 8.0);
        assert!(cmds.is_empty(), "targets the depleted tree; never builds on it");
        assert_eq!(m.target(), Some("tree:8000:8000"));
        assert!(
            matches!(&m.press_verb()[..], [Cmd::Send(Outbound::Harvest(_))]),
            "the press sends; the Island answers `depleted`"
        );
    }

    #[test]
    fn click_targets_a_structure_and_sends_nothing() {
        let mut m = model_with_player_at(3_000, 3_000);
        let mut snap = snap_with_player("alice", 3_000, 3_000);
        snap.structures.insert(
            "structure:3500:3000".into(),
            StructureWire { kind: "wall".into(), x: 3_500, y: 3_000, hp: 100, owner: "bob".into() },
        );
        m.on_snapshot(cc(0, 0), snap);
        assert!(m.click(3.5, 3.0).is_empty(), "clicking selects only");
        assert_eq!(m.target(), Some("structure:3500:3000"));
    }

    #[test]
    fn escape_clears_the_target_and_the_button_goes_inert() {
        let mut m = model_with_player_at(8_000, 8_000);
        let mut snap = snap_with_player("alice", 8_000, 8_000);
        snap.resource_nodes.insert(
            "tree:8000:8000".into(),
            NodeWire { kind: "tree".into(), x: 8_000, y: 8_000, depleted: false },
        );
        m.on_snapshot(cc(0, 0), snap);
        m.click(8.0, 8.0);
        assert_eq!(m.target(), Some("tree:8000:8000"));
        m.escape();
        assert_eq!(m.target(), None);
        assert!(m.press_verb().is_empty(), "no Target → the Verb button is inert");
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
    fn build_gate_threshold_is_the_shared_wall_cost() {
        // Exactly WALL_COST wood → builds; one less → refused. The gate tracks
        // the shared constant, so it cannot drift from the server catalogue.
        let mut afford = model_with_player_at(3_200, 3_200);
        afford.on_self(SelfPayload {
            inventory: BTreeMap::from([("wood".to_string(), WALL_COST)]),
        });
        assert!(!afford.click(3.2, 3.2).is_empty(), "WALL_COST wood affords a wall");

        let mut short = model_with_player_at(3_200, 3_200);
        short.on_self(SelfPayload {
            inventory: BTreeMap::from([("wood".to_string(), WALL_COST - 1)]),
        });
        assert!(short.click(3.2, 3.2).is_empty(), "one below WALL_COST is refused");
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
        let mut near = model_with_player_at(3_200, 3_200);
        assert!(near.click(3.2, 3.2).is_empty(), "insufficient materials");
    }

    #[test]
    fn verb_error_is_captured_as_last_error() {
        let (mut m, _) = ClientModel::new("alice", cc(0, 0));
        assert!(m.last_error().is_none(), "starts clear");
        m.on_verb_error("footprint_blocked".into());
        assert_eq!(m.last_error(), Some("footprint_blocked"));
    }

    #[test]
    fn emitting_a_verb_clears_last_error() {
        // The user clicked again to retry — that intent makes the prior error
        // stale. The next phx_reply will either confirm success or replace the
        // error with whatever went wrong this time.
        let mut m = model_with_player_at(3_200, 3_200);
        let mut inv = BTreeMap::new();
        inv.insert("wood".to_string(), 5);
        m.on_self(SelfPayload { inventory: inv });
        m.on_verb_error("footprint_blocked".into());
        let cmds = m.click(3.2, 3.2);
        assert!(matches!(cmds.as_slice(), [Cmd::Send(Outbound::Build(_))]));
        assert_eq!(m.last_error(), None, "issuing a fresh verb clears the stale error");
    }
}
