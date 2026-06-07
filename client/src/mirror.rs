//! The **Mirror** — the client's non-authoritative, speculative simulation of
//! its Player's View window (see `design/glossary.md`).
//!
//! It runs the Island's own movement integration (the shared `simcore`), fed
//! by Intents — its own Player's locally and immediately, every other actor's
//! as last received — and is continuously overridden by authoritative state.
//! It speculates **continuous state only** (movement); discrete events reach
//! it solely as authoritative fact. It is **born frozen** — at login,
//! relocation, and Instance entry/exit alike, it speculates only from an
//! authoritative baseline.
//!
//! Own-player exactness, by construction: the server consumes one input frame
//! per tick and acks the last consumed seq; the Mirror holds every unacked
//! frame and re-derives its own state as *authoritative override at the
//! snapshot tick + one-frame-per-tick replay* through the same integrator over
//! the same obstacles. When nothing external intervenes, speculation and
//! authority are bit-identical (pinned by `tests/exactness.rs`).

use protocol::consts::TICK_MS;
use protocol::geometry::ChunkCoord;
use protocol::types::{ResourceKind, StructureKind};
use protocol::wire::ChunkSnapshot;
use simcore::catalogue::{resource_footprint, structure_footprint};
use simcore::collision::Obstacle;
use simcore::motion::{intent_velocity, step_actor};
use std::collections::{BTreeMap, VecDeque};

/// One own input frame awaiting the server's ack — the Mirror consumes one per
/// tick, exactly as the server does.
#[derive(Debug, Clone, Copy, PartialEq)]
struct InputFrame {
    seq: u32,
    dx: f64,
    dy: f64,
}

/// One actor's authoritative state: position + Intent-velocity, as of `tick`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AuthActor {
    x: i64,
    y: i64,
    vx: f64,
    vy: f64,
    tick: u64,
}

/// Own speculative state: position + the Intent-velocity currently held —
/// the server's view of this player, some ticks ahead of it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OwnState {
    x: i64,
    y: i64,
    vx: f64,
    vy: f64,
}

pub struct Mirror {
    username: String,
    /// Highest authoritative tick seen; `None` until the first snapshot — the
    /// Mirror is born frozen.
    auth_tick: Option<u64>,
    /// The tick the Mirror has speculated up to. Never behind `auth_tick`.
    mirror_tick: u64,
    /// Per-player authoritative baseline (latest tick wins per actor — chunk
    /// snapshots broadcast independently and may straddle a tick).
    players: BTreeMap<String, AuthActor>,
    /// Per-NPC authoritative baseline, keyed by wire id. Same rules.
    npcs: BTreeMap<String, AuthActor>,
    /// Sent-but-unacked own input frames, in seq order. The replay tape.
    unacked: VecDeque<InputFrame>,
    /// How many of `unacked` the Mirror's own ticks have consumed so far.
    consumed: usize,
    own: Option<OwnState>,
    /// Per-chunk obstacle sets derived from snapshots via the shared
    /// catalogue — the authority's own collision data, reconstructed from
    /// kind + position with zero extra wire bytes.
    obstacles: BTreeMap<ChunkCoord, Vec<Obstacle>>,
}

impl Mirror {
    pub fn new(username: &str) -> Self {
        Mirror {
            username: username.to_string(),
            auth_tick: None,
            mirror_tick: 0,
            players: BTreeMap::new(),
            npcs: BTreeMap::new(),
            unacked: VecDeque::new(),
            consumed: 0,
            own: None,
            obstacles: BTreeMap::new(),
        }
    }

    /// Frozen means: do not speculate. Born frozen (no baseline yet), and
    /// frozen whole at the Lead bound — the Mirror never runs more than
    /// [`protocol::consts::LEAD_BOUND_TICKS`] past the last authoritative
    /// tick (the client-side face of Backpressure).
    pub fn frozen(&self) -> bool {
        match self.auth_tick {
            None => true,
            Some(auth) => self.mirror_tick - auth >= protocol::consts::LEAD_BOUND_TICKS,
        }
    }

    /// Queue one of our own input frames (the same frame that goes to the
    /// server) for speculative consumption and eventual replay. Inputs stall
    /// while frozen: the frame is dropped, so the replay tape cannot grow
    /// during an outage. (The session also stops *sending* while frozen.)
    pub fn push_input(&mut self, seq: u32, dx: f64, dy: f64) {
        if self.frozen() {
            return;
        }
        self.unacked.push_back(InputFrame { seq, dx, dy });
    }

    /// The server consumed our frames through `seq` as of `tick`: drop them
    /// from the replay tape.
    pub fn on_ack(&mut self, seq: u32, _tick: u64) {
        while self.unacked.front().is_some_and(|f| f.seq <= seq) {
            self.unacked.pop_front();
            self.consumed = self.consumed.saturating_sub(1);
        }
    }

    /// Advance the Mirror one tick: consume the next unacked frame (or hold
    /// the current Intent — the client only goes silent when idle, and idle
    /// always follows its zero-frame) and integrate. Inert while frozen.
    pub fn tick(&mut self) {
        if self.frozen() {
            return;
        }
        self.mirror_tick += 1;
        let Some(own) = self.own else { return };
        let (vx, vy) = match self.unacked.get(self.consumed) {
            Some(f) => {
                self.consumed += 1;
                intent_velocity(f.dx, f.dy)
            }
            None => (own.vx, own.vy),
        };
        let obstacles = self.all_obstacles();
        let dt = TICK_MS as f64 / 1000.0;
        let (x, y) = step_actor(own.x, own.y, vx, vy, dt, &obstacles);
        self.own = Some(OwnState { x, y, vx, vy });
    }

    /// Ingest one authoritative chunk snapshot: per-actor latest-tick-wins
    /// override, obstacle reconstruction, and — for the own player — the
    /// override-and-replay that keeps speculation exact. The first snapshot
    /// seeds the baseline and thaws the Mirror.
    pub fn on_snapshot(&mut self, coord: ChunkCoord, snap: &ChunkSnapshot) {
        for (name, p) in &snap.players {
            let next = AuthActor { x: p.x, y: p.y, vx: p.vx, vy: p.vy, tick: snap.tick };
            match self.players.get(name) {
                Some(prev) if prev.tick > snap.tick => {}
                _ => {
                    self.players.insert(name.clone(), next);
                }
            }
        }
        for (id, n) in &snap.npcs {
            let next = AuthActor { x: n.x, y: n.y, vx: n.vx, vy: n.vy, tick: snap.tick };
            match self.npcs.get(id) {
                Some(prev) if prev.tick > snap.tick => {}
                _ => {
                    self.npcs.insert(id.clone(), next);
                }
            }
        }
        self.obstacles.insert(coord, chunk_obstacles(snap));
        self.auth_tick = Some(self.auth_tick.unwrap_or(0).max(snap.tick));
        // The Mirror is never behind authority.
        self.mirror_tick = self.mirror_tick.max(snap.tick);
        if let Some(a) = self.players.get(&self.username).copied() {
            if a.tick == snap.tick {
                self.replay_own(a);
            }
        }
    }

    /// Override-and-replay: rebase own state on the authoritative `a`, then
    /// re-apply the unacked frames one per tick up to the Mirror's tick —
    /// the same one-frame-per-tick consumption the server runs.
    fn replay_own(&mut self, a: AuthActor) {
        let mut own = OwnState { x: a.x, y: a.y, vx: a.vx, vy: a.vy };
        let obstacles = self.all_obstacles();
        let dt = TICK_MS as f64 / 1000.0;
        let mut idx = 0;
        for _ in a.tick..self.mirror_tick {
            if let Some(f) = self.unacked.get(idx) {
                let (vx, vy) = intent_velocity(f.dx, f.dy);
                own.vx = vx;
                own.vy = vy;
                idx += 1;
            }
            let (x, y) = step_actor(own.x, own.y, own.vx, own.vy, dt, &obstacles);
            own.x = x;
            own.y = y;
        }
        self.consumed = idx;
        self.own = Some(own);
    }

    /// The Mirror's position for a player: own player by frame replay,
    /// everyone else by their last-known Intent.
    pub fn position_of(&self, name: &str) -> Option<(i64, i64)> {
        if name == self.username {
            if let Some(own) = self.own {
                return Some((own.x, own.y));
            }
        }
        self.players.get(name).map(|a| self.speculate(a))
    }

    /// The Mirror's position for an NPC (by wire id), by its last-known Intent.
    pub fn npc_position_of(&self, id: &str) -> Option<(i64, i64)> {
        self.npcs.get(id).map(|a| self.speculate(a))
    }

    /// Advance an actor from its authoritative state to the Mirror's tick by
    /// holding its last-known Intent — one integrator step per tick, exactly
    /// as the server would integrate an unchanged Intent.
    fn speculate(&self, a: &AuthActor) -> (i64, i64) {
        let obstacles = self.all_obstacles();
        let dt = TICK_MS as f64 / 1000.0;
        let (mut x, mut y) = (a.x, a.y);
        for _ in a.tick..self.mirror_tick {
            (x, y) = step_actor(x, y, a.vx, a.vy, dt, &obstacles);
        }
        (x, y)
    }

    fn all_obstacles(&self) -> Vec<Obstacle> {
        self.obstacles.values().flatten().copied().collect()
    }
}

/// The authority's obstacle set for one chunk, reconstructed from wire data:
/// resource nodes (footprint identical gatherable or depleted — harvesting
/// never opens a path) and structures, shapes from the shared catalogue.
fn chunk_obstacles(snap: &ChunkSnapshot) -> Vec<Obstacle> {
    let mut out = Vec::new();
    for n in snap.resource_nodes.values() {
        if let Some(kind) = ResourceKind::parse(&n.kind) {
            out.push(Obstacle { x: n.x, y: n.y, footprint: resource_footprint(kind) });
        }
    }
    for s in snap.structures.values() {
        if let Some(kind) = StructureKind::parse(&s.kind) {
            out.push(Obstacle { x: s.x, y: s.y, footprint: structure_footprint(kind) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::wire::{NpcWire, PlayerWire};

    fn cc(x: i32, y: i32) -> ChunkCoord {
        ChunkCoord::new(x, y)
    }

    /// Born frozen: before the first authoritative snapshot there is no
    /// baseline, so the Mirror speculates nothing — ticks are inert and no
    /// actor has a position.
    #[test]
    fn born_frozen_until_the_first_authoritative_snapshot() {
        let mut m = Mirror::new("alice");
        assert!(m.frozen(), "no baseline yet — born frozen");
        m.tick();
        m.tick();
        assert!(m.frozen(), "ticks while frozen are inert");
        assert_eq!(m.position_of("alice"), None, "nothing to speculate from");

        // The first authoritative snapshot seeds the baseline and thaws it.
        let mut snap = ChunkSnapshot { tick: 5, ..ChunkSnapshot::default() };
        snap.players.insert(
            "alice".into(),
            PlayerWire { x: 8_000, y: 8_000, vx: 0.0, vy: 0.0 },
        );
        m.on_snapshot(cc(0, 0), &snap);
        assert!(!m.frozen(), "an authoritative baseline thaws the Mirror");
        assert_eq!(m.position_of("alice"), Some((8_000, 8_000)));
    }

    fn seeded(tick: u64, x: i64, y: i64) -> Mirror {
        let mut m = Mirror::new("alice");
        let mut snap = ChunkSnapshot { tick, ..ChunkSnapshot::default() };
        snap.players.insert("alice".into(), PlayerWire { x, y, vx: 0.0, vy: 0.0 });
        m.on_snapshot(cc(0, 0), &snap);
        m
    }

    /// The Backpressure promise, client-side: the Mirror never speculates more
    /// than LEAD_BOUND_TICKS past authority — at the bound the *whole* Mirror
    /// freezes (own player and everyone else; inputs stall), and the next
    /// authoritative tick thaws it.
    #[test]
    fn freezes_whole_at_the_lead_bound() {
        use protocol::consts::LEAD_BOUND_TICKS;
        let mut m = Mirror::new("alice");
        let mut snap = ChunkSnapshot { tick: 100, ..ChunkSnapshot::default() };
        snap.players.insert("alice".into(), PlayerWire { x: 8_000, y: 8_000, vx: 0.0, vy: 0.0 });
        snap.players.insert("bob".into(), PlayerWire { x: 20_000, y: 8_000, vx: 4_000.0, vy: 0.0 });
        m.on_snapshot(cc(0, 0), &snap);

        // Authority goes quiet; the client keeps trying to walk east.
        for i in 1..=(LEAD_BOUND_TICKS + 5) {
            m.push_input(i as u32, 1.0, 0.0);
            m.tick();
        }
        assert!(m.frozen(), "at the bound the Mirror freezes");
        let capped = 8_000 + 200 * LEAD_BOUND_TICKS as i64;
        assert_eq!(m.position_of("alice"), Some((capped, 8_000)), "own speculation capped at K");
        assert_eq!(
            m.position_of("bob"),
            Some((20_000 + 200 * LEAD_BOUND_TICKS as i64, 8_000)),
            "whole-Mirror: bob's speculation freezes at the same bound"
        );

        // Inputs stall while frozen: frames pushed at the bound are dropped,
        // so the replay tape cannot grow during an outage.
        let tape_before = m.position_of("alice");
        m.push_input(99, -1.0, 0.0);
        m.tick();
        assert_eq!(m.position_of("alice"), tape_before);

        // Authority catches up; the Mirror thaws and speculates again.
        let mut snap = ChunkSnapshot { tick: 102, ..ChunkSnapshot::default() };
        snap.players.insert("alice".into(), PlayerWire { x: 8_400, y: 8_000, vx: 4_000.0, vy: 0.0 });
        m.on_snapshot(cc(0, 0), &snap);
        assert!(!m.frozen(), "authority advancing thaws the Mirror");
        let before = m.position_of("alice").unwrap();
        m.tick();
        assert_ne!(m.position_of("alice").unwrap(), before, "speculation resumes");
    }

    /// Every other actor advances by its last-known Intent (the velocity the
    /// snapshot carried) — identical to the server until that actor changes
    /// intent, corrected by the next override.
    #[test]
    fn integrates_others_from_their_last_known_intent() {
        let mut m = Mirror::new("alice");
        let mut snap = ChunkSnapshot { tick: 10, ..ChunkSnapshot::default() };
        snap.players.insert("alice".into(), PlayerWire { x: 8_000, y: 8_000, vx: 0.0, vy: 0.0 });
        snap.players.insert("bob".into(), PlayerWire { x: 10_000, y: 8_000, vx: 4_000.0, vy: 0.0 });
        snap.npcs.insert(
            "npc:wolf:1".into(),
            NpcWire { kind: "wolf".into(), x: 12_000, y: 8_000, hp: 80, vx: -2_000.0, vy: 0.0 },
        );
        m.on_snapshot(cc(0, 0), &snap);

        m.tick();
        m.tick();
        assert_eq!(m.position_of("bob"), Some((10_400, 8_000)), "bob walks his held intent");
        assert_eq!(
            m.npc_position_of("npc:wolf:1"),
            Some((11_800, 8_000)),
            "the wolf walks its last-known intent"
        );

        // A fresh override rebases speculation: at zero lead the authoritative
        // position is shown as-is.
        let mut snap = ChunkSnapshot { tick: 12, ..ChunkSnapshot::default() };
        snap.players.insert("bob".into(), PlayerWire { x: 10_390, y: 8_000, vx: 4_000.0, vy: 0.0 });
        m.on_snapshot(cc(0, 0), &snap);
        assert_eq!(m.position_of("bob"), Some((10_390, 8_000)), "override wins at zero lead");
    }

    /// The Mirror's own player runs the server's exact frame semantics — one
    /// queued input frame consumed per tick — so a keypress moves the player
    /// locally, immediately, ahead of any server echo.
    #[test]
    fn speculates_own_movement_immediately() {
        let mut m = seeded(10, 8_000, 8_000);
        m.push_input(1, 1.0, 0.0);
        m.tick();
        assert_eq!(
            m.position_of("alice"),
            Some((8_200, 8_000)),
            "one tick east (4000 sub-units/s × 50ms), before any server echo"
        );
        // No frame this tick: the held Intent persists (the client only goes
        // silent when idle, and idle always follows its zero-frame).
        m.tick();
        assert_eq!(m.position_of("alice"), Some((8_400, 8_000)));
        // The zero-frame stops the speculation.
        m.push_input(2, 0.0, 0.0);
        m.tick();
        assert_eq!(m.position_of("alice"), Some((8_400, 8_000)));
    }
}
