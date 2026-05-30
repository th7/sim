//! Client-side dev-overlay state, kept separate from the player model: whether
//! the chunk-status overlay is on, and the latest `dev:stats` snapshot. Nothing
//! on the player path reads this; the [`ClientModel`](crate::model::ClientModel)
//! merely composes a `DevState` and forwards to it.

use crate::model::Cmd;
use protocol::wire::StatsPayload;

/// The dev overlay's observable state.
#[derive(Default)]
pub struct DevState {
    enabled: bool,
    stats: Option<StatsPayload>,
}

impl DevState {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn stats(&self) -> Option<&StatsPayload> {
        self.stats.as_ref()
    }

    /// Ingest a `dev:stats` push.
    pub fn on_stats(&mut self, payload: StatsPayload) {
        self.stats = Some(payload);
    }

    /// Turn the overlay on/off: (un)subscribe `dev:stats`, dropping cached stats
    /// when turned off. Idempotent — an empty `Vec` if already in that state.
    pub fn set(&mut self, on: bool) -> Vec<Cmd> {
        if on == self.enabled {
            return Vec::new();
        }
        self.enabled = on;
        if on {
            vec![Cmd::SubscribeDevStats]
        } else {
            self.stats = None;
            vec![Cmd::UnsubscribeDevStats]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_toggles_subscription_and_clears_on_off() {
        let mut dev = DevState::default();
        assert!(!dev.enabled());
        // Turning on subscribes to dev:stats.
        assert_eq!(dev.set(true), vec![Cmd::SubscribeDevStats]);
        assert!(dev.enabled());
        // Idempotent.
        assert!(dev.set(true).is_empty());
        // Stats can flow in.
        dev.on_stats(StatsPayload {
            active_chunks: 1,
            total_players: 1,
            total_npcs: 2,
            around: Vec::new(),
        });
        assert_eq!(dev.stats().unwrap().total_npcs, 2);
        assert!(dev.stats().is_some());
        // Turning off unsubscribes and drops cached stats.
        assert_eq!(dev.set(false), vec![Cmd::UnsubscribeDevStats]);
        assert!(!dev.enabled());
        assert!(dev.stats().is_none());
    }
}
