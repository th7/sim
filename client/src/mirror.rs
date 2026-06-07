//! The **Mirror** — the client's non-authoritative, speculative simulation of
//! its Player's View window (see `design/glossary.md`).
//!
//! It runs the Island's own movement integration (the shared `simcore`), fed
//! by Intents — its own Player's locally and immediately, every other actor's
//! as last received — and is continuously overridden by authoritative state.
//! It speculates **continuous state only** (movement); discrete events reach
//! it solely as authoritative fact. Its speculation is bounded: the Lead never
//! exceeds [`protocol::consts::LEAD_BOUND_TICKS`]; at the bound it freezes
//! whole. And it is **born frozen** — at login, relocation, and Instance
//! entry/exit alike, it speculates only from an authoritative baseline.

use protocol::consts::TICK_MS;
use protocol::wire::ChunkSnapshot;
use simcore::motion::{intent_velocity, step_actor};
use std::collections::{BTreeMap, VecDeque};

/// One own input frame awaiting consumption — the Mirror consumes one per
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

pub struct Mirror {
    username: String,
    /// Highest authoritative tick seen; `None` until the first snapshot — the
    /// Mirror is born frozen.
    auth_tick: Option<u64>,
    /// Per-player authoritative baseline (latest tick wins per actor — chunk
    /// snapshots broadcast independently and may straddle a tick).
    players: BTreeMap<String, AuthActor>,
    /// Own input frames not yet consumed by a Mirror tick.
    own_frames: VecDeque<InputFrame>,
    /// Own speculative state: position + the Intent currently held, exactly
    /// the server's view of this player, some ticks ahead of it.
    own: Option<((i64, i64), (f64, f64))>,
}

impl Mirror {
    pub fn new(username: &str) -> Self {
        Mirror {
            username: username.to_string(),
            auth_tick: None,
            players: BTreeMap::new(),
            own_frames: VecDeque::new(),
            own: None,
        }
    }

    /// Queue one of our own input frames (the same frame that goes to the
    /// server) for speculative consumption — one per tick.
    pub fn push_input(&mut self, seq: u32, dx: f64, dy: f64) {
        self.own_frames.push_back(InputFrame { seq, dx, dy });
    }

    /// Frozen means: do not speculate. Born frozen — no baseline, no
    /// speculation.
    pub fn frozen(&self) -> bool {
        self.auth_tick.is_none()
    }

    /// Advance the Mirror one tick: consume one own input frame (or hold the
    /// current Intent — the client only goes silent when idle, and idle always
    /// follows its zero-frame) and integrate. Inert while frozen.
    pub fn tick(&mut self) {
        if self.frozen() {
            return;
        }
        let Some(((x, y), intent)) = self.own else { return };
        let intent = match self.own_frames.pop_front() {
            Some(f) => (f.dx, f.dy),
            None => intent,
        };
        let (vx, vy) = intent_velocity(intent.0, intent.1);
        let dt = TICK_MS as f64 / 1000.0;
        let (nx, ny) = step_actor(x, y, vx, vy, dt, &[]);
        self.own = Some(((nx, ny), intent));
    }

    /// Ingest one authoritative chunk snapshot: per-actor latest-tick-wins
    /// override. The first snapshot seeds the baseline and thaws the Mirror.
    pub fn on_snapshot(&mut self, snap: &ChunkSnapshot) {
        for (name, p) in &snap.players {
            let next = AuthActor { x: p.x, y: p.y, vx: p.vx, vy: p.vy, tick: snap.tick };
            match self.players.get(name) {
                Some(prev) if prev.tick > snap.tick => {}
                _ => {
                    self.players.insert(name.clone(), next);
                }
            }
        }
        self.auth_tick = Some(self.auth_tick.unwrap_or(0).max(snap.tick));
        // Seed own speculative state from the first authoritative sight of us;
        // overrides past the seed are the replay slice's concern.
        if self.own.is_none() {
            if let Some(a) = self.players.get(&self.username) {
                self.own = Some(((a.x, a.y), (0.0, 0.0)));
            }
        }
    }

    /// The Mirror's position for an actor: own player speculative, everyone
    /// else their latest authoritative state.
    pub fn position_of(&self, name: &str) -> Option<(i64, i64)> {
        if name == self.username {
            if let Some((pos, _)) = self.own {
                return Some(pos);
            }
        }
        self.players.get(name).map(|a| (a.x, a.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::wire::{ChunkSnapshot, PlayerWire};

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
        m.on_snapshot(&snap);
        assert!(!m.frozen(), "an authoritative baseline thaws the Mirror");
        assert_eq!(m.position_of("alice"), Some((8_000, 8_000)));
    }

    fn seeded(tick: u64, x: i64, y: i64) -> Mirror {
        let mut m = Mirror::new("alice");
        let mut snap = ChunkSnapshot { tick, ..ChunkSnapshot::default() };
        snap.players.insert("alice".into(), PlayerWire { x, y, vx: 0.0, vy: 0.0 });
        m.on_snapshot(&snap);
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
