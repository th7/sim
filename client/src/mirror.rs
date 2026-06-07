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

use protocol::wire::ChunkSnapshot;
use std::collections::BTreeMap;

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
}

impl Mirror {
    pub fn new(username: &str) -> Self {
        Mirror { username: username.to_string(), auth_tick: None, players: BTreeMap::new() }
    }

    /// Frozen means: do not speculate. Born frozen — no baseline, no
    /// speculation.
    pub fn frozen(&self) -> bool {
        self.auth_tick.is_none()
    }

    /// Advance the Mirror one tick. Inert while frozen.
    pub fn tick(&mut self) {
        if self.frozen() {}
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
    }

    /// The Mirror's position for an actor, if it has one.
    pub fn position_of(&self, name: &str) -> Option<(i64, i64)> {
        let _ = &self.username; // own-player speculation lands in later slices
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
}
