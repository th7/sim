//! Changed-only observation deltas, per chunk (region).
//!
//! Each tick a cluster publishes only what changed in the chunks it touched:
//! `upserts` for entities that are new-to-the-chunk or whose wire state
//! changed, `removes` for entities that left the chunk. An entity entering a
//! chunk yields a full upsert (baseline-on-enter) since it was absent before;
//! re-publishing identical state yields nothing (idempotent). A move across a
//! chunk boundary is a `remove` from the old chunk and a baseline `upsert` in
//! the new one.
//!
//! This is the read-model feed (an `ArcSwap<Snapshot>` per region). The
//! `snapshot` wire event still carries a full [`ChunkSnapshot`](crate::wire),
//! reconstructable by applying deltas onto a baseline — both are derived from
//! the same entity states, so they cannot disagree.

use crate::components::WireId;
use crate::geometry::ChunkCoord;
use crate::wire::EntityWire;
use std::collections::BTreeMap;

/// What changed in one chunk since the last publish.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChunkDelta {
    /// Entities to insert-or-replace, with their full current state.
    pub upserts: BTreeMap<WireId, EntityWire>,
    /// Entities that left this chunk.
    pub removes: Vec<WireId>,
}

impl ChunkDelta {
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.removes.is_empty()
    }
}

/// Tracks the last-published state per chunk and computes per-chunk deltas.
#[derive(Debug, Default)]
pub struct DeltaTracker {
    last: BTreeMap<ChunkCoord, BTreeMap<WireId, EntityWire>>,
}

impl DeltaTracker {
    pub fn new() -> Self {
        DeltaTracker::default()
    }

    /// Diff `states` (all current entity wire states, keyed by id) against the
    /// last publish and return per-chunk deltas, then commit the new baseline.
    /// Only chunks that changed appear in the result.
    pub fn publish(
        &mut self,
        states: &BTreeMap<WireId, EntityWire>,
    ) -> BTreeMap<ChunkCoord, ChunkDelta> {
        // Bucket current states by chunk.
        let mut current: BTreeMap<ChunkCoord, BTreeMap<WireId, EntityWire>> = BTreeMap::new();
        for (wid, state) in states {
            current
                .entry(state.chunk())
                .or_default()
                .insert(wid.clone(), state.clone());
        }

        let mut deltas: BTreeMap<ChunkCoord, ChunkDelta> = BTreeMap::new();

        // Every chunk that has, or had, entities.
        let chunks: std::collections::BTreeSet<ChunkCoord> =
            current.keys().chain(self.last.keys()).copied().collect();

        for chunk in chunks {
            let now = current.get(&chunk);
            let before = self.last.get(&chunk);
            let mut delta = ChunkDelta::default();

            if let Some(now) = now {
                for (wid, state) in now {
                    let changed = before.and_then(|b| b.get(wid)) != Some(state);
                    if changed {
                        delta.upserts.insert(wid.clone(), state.clone());
                    }
                }
            }
            if let Some(before) = before {
                for wid in before.keys() {
                    let gone = now.map(|n| !n.contains_key(wid)).unwrap_or(true);
                    if gone {
                        delta.removes.push(wid.clone());
                    }
                }
            }

            if !delta.is_empty() {
                deltas.insert(chunk, delta);
            }
        }

        self.last = current;
        deltas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wid(s: &str) -> WireId {
        WireId(s.to_string())
    }

    fn player(x: i64, y: i64) -> EntityWire {
        EntityWire::Player { x, y, vx: 0.0, vy: 0.0 }
    }

    #[test]
    fn first_publish_is_all_upserts() {
        let mut t = DeltaTracker::new();
        let mut states = BTreeMap::new();
        states.insert(wid("alice"), player(8_000, 8_000)); // chunk (0,0)
        let d = t.publish(&states);
        assert_eq!(d.len(), 1);
        let c0 = &d[&ChunkCoord::new(0, 0)];
        assert_eq!(c0.upserts.len(), 1);
        assert!(c0.removes.is_empty());
    }

    #[test]
    fn idempotent_republish_yields_nothing() {
        let mut t = DeltaTracker::new();
        let mut states = BTreeMap::new();
        states.insert(wid("alice"), player(8_000, 8_000));
        t.publish(&states);
        let d = t.publish(&states);
        assert!(d.is_empty(), "republishing identical state emits no deltas");
    }

    #[test]
    fn only_changed_entity_is_upserted() {
        let mut t = DeltaTracker::new();
        let mut states = BTreeMap::new();
        states.insert(wid("alice"), player(8_000, 8_000));
        states.insert(wid("bob"), player(9_000, 9_000));
        t.publish(&states);

        // Move only alice (within the same chunk).
        states.insert(wid("alice"), player(8_100, 8_000));
        let d = t.publish(&states);
        let c0 = &d[&ChunkCoord::new(0, 0)];
        assert_eq!(c0.upserts.len(), 1);
        assert!(c0.upserts.contains_key(&wid("alice")));
    }

    #[test]
    fn cross_boundary_is_remove_then_baseline_upsert() {
        let mut t = DeltaTracker::new();
        let mut states = BTreeMap::new();
        states.insert(wid("alice"), player(15_900, 8_000)); // chunk (0,0)
        t.publish(&states);

        states.insert(wid("alice"), player(16_100, 8_000)); // chunk (1,0)
        let d = t.publish(&states);

        let old = &d[&ChunkCoord::new(0, 0)];
        assert_eq!(old.removes, vec![wid("alice")]);
        let new = &d[&ChunkCoord::new(1, 0)];
        assert!(new.upserts.contains_key(&wid("alice")), "baseline on enter");
    }

    #[test]
    fn disappearance_is_a_remove() {
        let mut t = DeltaTracker::new();
        let mut states = BTreeMap::new();
        states.insert(wid("alice"), player(8_000, 8_000));
        t.publish(&states);

        states.clear();
        let d = t.publish(&states);
        assert_eq!(d[&ChunkCoord::new(0, 0)].removes, vec![wid("alice")]);
    }
}
