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
            unacked: VecDeque::new(),
            consumed: 0,
            own: None,
            obstacles: BTreeMap::new(),
        }
    }

    /// Frozen means: do not speculate. Born frozen — no baseline, no
    /// speculation.
    pub fn frozen(&self) -> bool {
        self.auth_tick.is_none()
    }

    /// Queue one of our own input frames (the same frame that goes to the
    /// server) for speculative consumption and eventual replay.
    pub fn push_input(&mut self, seq: u32, dx: f64, dy: f64) {
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

    /// The Mirror's position for an actor: own player speculative, everyone
    /// else their latest authoritative state.
    pub fn position_of(&self, name: &str) -> Option<(i64, i64)> {
        if name == self.username {
            if let Some(own) = self.own {
                return Some((own.x, own.y));
            }
        }
        self.players.get(name).map(|a| (a.x, a.y))
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
    use protocol::wire::PlayerWire;

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
