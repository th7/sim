//! The **Datastore** — the single persistence chokepoint. Clusters
//! emit state changes here; durable reads go through here. Mirrors the Elixir
//! `GamePersistence.Datastore` behaviour: an in-memory **pending writes** buffer
//! (per-key last-write-wins, with delete tombstones) flushed to a durable
//! backend on a cadence, **merged reads** (pending overlaid on durable), and a
//! **backpressure** state machine for overload.
//!
//! The durable backend is a trait so the POC can use an in-memory store now and
//! a real database later. Only the Overworld persists; Instances are in-memory only, so the
//! sim layer simply doesn't emit for Instance realms.

use crate::components::{Item, ResourceKind, StructureKind};
use crate::geometry::{coord_for, ChunkCoord};
use std::collections::BTreeMap;

/// A persisted player: where they were and what they carried at last write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRecord {
    pub username: String,
    pub chunk: ChunkCoord,
    pub x: i64,
    pub y: i64,
    pub inventory: BTreeMap<Item, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureRecord {
    pub coord: ChunkCoord,
    pub owner: String,
    pub kind: StructureKind,
    pub x: i64,
    pub y: i64,
    pub hp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepletionRecord {
    pub coord: ChunkCoord,
    pub kind: ResourceKind,
    pub x: i64,
    pub y: i64,
    /// Sim-clock time the node respawns at.
    pub respawn_at_ms: u64,
}

/// A change a cluster emits toward persistence. Buffered as pending writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistEvent {
    UpsertPlayer(PlayerRecord),
    UpsertStructure(StructureRecord),
    DeleteStructure { x: i64, y: i64 },
    UpsertDepletion(DepletionRecord),
    DeleteDepletion { x: i64, y: i64 },
}

/// Durable backend. The POC ships [`MemStore`]; a Postgres impl can follow.
pub trait DurableStore {
    fn load_player(&self, username: &str) -> Option<PlayerRecord>;
    fn save_player(&mut self, rec: &PlayerRecord);
    fn load_structures(&self, coord: ChunkCoord) -> Vec<StructureRecord>;
    fn save_structure(&mut self, rec: &StructureRecord);
    fn delete_structure(&mut self, x: i64, y: i64);
    fn load_depletions(&self, coord: ChunkCoord) -> Vec<DepletionRecord>;
    fn save_depletion(&mut self, rec: &DepletionRecord);
    fn delete_depletion(&mut self, x: i64, y: i64);
}

/// In-memory durable backend. Retained across a `Sim` instance, so it models
/// "survives restart" within the process — the POC stub.
#[derive(Debug, Default)]
pub struct MemStore {
    players: BTreeMap<String, PlayerRecord>,
    structures: BTreeMap<(i64, i64), StructureRecord>,
    depletions: BTreeMap<(i64, i64), DepletionRecord>,
}

impl DurableStore for MemStore {
    fn load_player(&self, username: &str) -> Option<PlayerRecord> {
        self.players.get(username).cloned()
    }
    fn save_player(&mut self, rec: &PlayerRecord) {
        self.players.insert(rec.username.clone(), rec.clone());
    }
    fn load_structures(&self, coord: ChunkCoord) -> Vec<StructureRecord> {
        self.structures.values().filter(|s| s.coord == coord).cloned().collect()
    }
    fn save_structure(&mut self, rec: &StructureRecord) {
        self.structures.insert((rec.x, rec.y), rec.clone());
    }
    fn delete_structure(&mut self, x: i64, y: i64) {
        self.structures.remove(&(x, y));
    }
    fn load_depletions(&self, coord: ChunkCoord) -> Vec<DepletionRecord> {
        self.depletions.values().filter(|d| d.coord == coord).cloned().collect()
    }
    fn save_depletion(&mut self, rec: &DepletionRecord) {
        self.depletions.insert((rec.x, rec.y), rec.clone());
    }
    fn delete_depletion(&mut self, x: i64, y: i64) {
        self.depletions.remove(&(x, y));
    }
}

/// Lets a boxed trait object be used wherever a `DurableStore` is expected, so
/// `Sim` can hold either a [`MemStore`] or a Postgres store without generics.
impl DurableStore for Box<dyn DurableStore + Send> {
    fn load_player(&self, username: &str) -> Option<PlayerRecord> {
        (**self).load_player(username)
    }
    fn save_player(&mut self, rec: &PlayerRecord) {
        (**self).save_player(rec)
    }
    fn load_structures(&self, coord: ChunkCoord) -> Vec<StructureRecord> {
        (**self).load_structures(coord)
    }
    fn save_structure(&mut self, rec: &StructureRecord) {
        (**self).save_structure(rec)
    }
    fn delete_structure(&mut self, x: i64, y: i64) {
        (**self).delete_structure(x, y)
    }
    fn load_depletions(&self, coord: ChunkCoord) -> Vec<DepletionRecord> {
        (**self).load_depletions(coord)
    }
    fn save_depletion(&mut self, rec: &DepletionRecord) {
        (**self).save_depletion(rec)
    }
    fn delete_depletion(&mut self, x: i64, y: i64) {
        (**self).delete_depletion(x, y)
    }
}

/// Backpressure mode. `Flowing` accepts writes; `Backpressured` would park them
/// (the sim is single-threaded so we surface the mode rather than block).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Flowing,
    Backpressured,
}

/// Thresholds for the backpressure state machine (matching the Elixir defaults).
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub n_high: usize,
    pub n_low: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds { n_high: 1_000, n_low: 200 }
    }
}

pub struct Datastore<S: DurableStore> {
    durable: S,
    pending_players: BTreeMap<String, PlayerRecord>,
    /// `None` = delete tombstone.
    pending_structures: BTreeMap<(i64, i64), Option<StructureRecord>>,
    pending_depletions: BTreeMap<(i64, i64), Option<DepletionRecord>>,
    mode: Mode,
    thresholds: Thresholds,
}

impl<S: DurableStore> Datastore<S> {
    pub fn new(durable: S) -> Self {
        Datastore {
            durable,
            pending_players: BTreeMap::new(),
            pending_structures: BTreeMap::new(),
            pending_depletions: BTreeMap::new(),
            mode: Mode::Flowing,
            thresholds: Thresholds::default(),
        }
    }

    pub fn with_thresholds(durable: S, thresholds: Thresholds) -> Self {
        let mut ds = Datastore::new(durable);
        ds.thresholds = thresholds;
        ds
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Retune the backpressure thresholds and re-evaluate the mode immediately
    /// against the current buffer depth (so a config change takes effect now,
    /// not only on the next write/flush).
    pub fn set_thresholds(&mut self, thresholds: Thresholds) {
        self.thresholds = thresholds;
        self.maybe_engage_backpressure();
        self.maybe_disengage_backpressure();
    }

    /// Consume the Datastore, returning the durable backend (e.g. to hand to a
    /// fresh `Sim` — modelling a process restart). Flush first to not lose
    /// pending writes.
    pub fn into_durable(self) -> S {
        self.durable
    }

    pub fn pending_len(&self) -> usize {
        self.pending_players.len() + self.pending_structures.len() + self.pending_depletions.len()
    }

    /// Apply one emitted change to the pending buffer (last-write-wins).
    pub fn apply(&mut self, event: PersistEvent) {
        match event {
            PersistEvent::UpsertPlayer(rec) => {
                self.pending_players.insert(rec.username.clone(), rec);
            }
            PersistEvent::UpsertStructure(rec) => {
                self.pending_structures.insert((rec.x, rec.y), Some(rec));
            }
            PersistEvent::DeleteStructure { x, y } => {
                self.pending_structures.insert((x, y), None);
            }
            PersistEvent::UpsertDepletion(rec) => {
                self.pending_depletions.insert((rec.x, rec.y), Some(rec));
            }
            PersistEvent::DeleteDepletion { x, y } => {
                self.pending_depletions.insert((x, y), None);
            }
        }
        self.maybe_engage_backpressure();
    }

    pub fn apply_all(&mut self, events: impl IntoIterator<Item = PersistEvent>) {
        for e in events {
            self.apply(e);
        }
    }

    // --- merged reads (pending overlaid on durable) ---

    pub fn fetch_player(&self, username: &str) -> Option<PlayerRecord> {
        self.pending_players
            .get(username)
            .cloned()
            .or_else(|| self.durable.load_player(username))
    }

    pub fn fetch_structures(&self, coord: ChunkCoord) -> Vec<StructureRecord> {
        let mut merged: BTreeMap<(i64, i64), StructureRecord> = self
            .durable
            .load_structures(coord)
            .into_iter()
            .map(|s| ((s.x, s.y), s))
            .collect();
        for ((x, y), entry) in &self.pending_structures {
            if coord_for(*x, *y) != coord {
                continue;
            }
            match entry {
                Some(rec) => {
                    merged.insert((*x, *y), rec.clone());
                }
                None => {
                    merged.remove(&(*x, *y));
                }
            }
        }
        merged.into_values().collect()
    }

    pub fn fetch_depletions(&self, coord: ChunkCoord) -> Vec<DepletionRecord> {
        let mut merged: BTreeMap<(i64, i64), DepletionRecord> = self
            .durable
            .load_depletions(coord)
            .into_iter()
            .map(|d| ((d.x, d.y), d))
            .collect();
        for ((x, y), entry) in &self.pending_depletions {
            if coord_for(*x, *y) != coord {
                continue;
            }
            match entry {
                Some(rec) => {
                    merged.insert((*x, *y), rec.clone());
                }
                None => {
                    merged.remove(&(*x, *y));
                }
            }
        }
        merged.into_values().collect()
    }

    /// Flush all pending writes to durable storage and clear the buffer. May
    /// disengage backpressure if the buffer drained below `n_low`.
    pub fn flush(&mut self) {
        for rec in self.pending_players.values() {
            self.durable.save_player(rec);
        }
        for ((x, y), entry) in &self.pending_structures {
            match entry {
                Some(rec) => self.durable.save_structure(rec),
                None => self.durable.delete_structure(*x, *y),
            }
        }
        for ((x, y), entry) in &self.pending_depletions {
            match entry {
                Some(rec) => self.durable.save_depletion(rec),
                None => self.durable.delete_depletion(*x, *y),
            }
        }
        self.pending_players.clear();
        self.pending_structures.clear();
        self.pending_depletions.clear();
        self.maybe_disengage_backpressure();
    }

    fn maybe_engage_backpressure(&mut self) {
        if self.mode == Mode::Flowing && self.pending_len() >= self.thresholds.n_high {
            self.mode = Mode::Backpressured;
        }
    }

    fn maybe_disengage_backpressure(&mut self) {
        if self.mode == Mode::Backpressured && self.pending_len() < self.thresholds.n_low {
            self.mode = Mode::Flowing;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(n: u32) -> BTreeMap<Item, u32> {
        let mut m = BTreeMap::new();
        m.insert(Item::Wood, n);
        m
    }

    fn player(name: &str, x: i64, y: i64, wood: u32) -> PlayerRecord {
        PlayerRecord { username: name.into(), chunk: coord_for(x, y), x, y, inventory: inv(wood) }
    }

    #[test]
    fn pending_read_before_flush_then_durable_after() {
        let mut ds = Datastore::new(MemStore::default());
        assert_eq!(ds.fetch_player("alice"), None);
        ds.apply(PersistEvent::UpsertPlayer(player("alice", 8_000, 8_000, 3)));
        // Readable from pending.
        assert_eq!(ds.fetch_player("alice").unwrap().inventory, inv(3));
        ds.flush();
        // Still readable, now from durable.
        assert_eq!(ds.fetch_player("alice").unwrap().x, 8_000);
        assert_eq!(ds.pending_len(), 0);
    }

    #[test]
    fn last_write_wins_per_key() {
        let mut ds = Datastore::new(MemStore::default());
        ds.apply(PersistEvent::UpsertPlayer(player("alice", 1, 1, 1)));
        ds.apply(PersistEvent::UpsertPlayer(player("alice", 2, 2, 9)));
        let p = ds.fetch_player("alice").unwrap();
        assert_eq!((p.x, p.y), (2, 2));
        assert_eq!(p.inventory, inv(9));
    }

    #[test]
    fn structure_tombstone_supersedes_upsert() {
        let mut ds = Datastore::new(MemStore::default());
        let coord = coord_for(3_000, 3_000);
        ds.apply(PersistEvent::UpsertStructure(StructureRecord {
            coord,
            owner: "alice".into(),
            kind: StructureKind::Wall,
            x: 3_000,
            y: 3_000,
            hp: 100,
        }));
        ds.flush();
        assert_eq!(ds.fetch_structures(coord).len(), 1);

        ds.apply(PersistEvent::DeleteStructure { x: 3_000, y: 3_000 });
        // Tombstone hides the durable row even before flush.
        assert_eq!(ds.fetch_structures(coord).len(), 0);
        ds.flush();
        assert_eq!(ds.fetch_structures(coord).len(), 0);
    }

    #[test]
    fn depletion_round_trips_and_deletes() {
        let mut ds = Datastore::new(MemStore::default());
        let coord = coord_for(8_500, 8_500);
        ds.apply(PersistEvent::UpsertDepletion(DepletionRecord {
            coord,
            kind: ResourceKind::Tree,
            x: 8_500,
            y: 8_500,
            respawn_at_ms: 30_000,
        }));
        ds.flush();
        assert_eq!(ds.fetch_depletions(coord).len(), 1);
        ds.apply(PersistEvent::DeleteDepletion { x: 8_500, y: 8_500 });
        ds.flush();
        assert_eq!(ds.fetch_depletions(coord).len(), 0);
    }

    #[test]
    fn backpressure_engages_and_disengages() {
        let mut ds = Datastore::with_thresholds(
            MemStore::default(),
            Thresholds { n_high: 5, n_low: 2 },
        );
        assert_eq!(ds.mode(), Mode::Flowing);
        for i in 0..5 {
            ds.apply(PersistEvent::UpsertPlayer(player(&format!("p{i}"), i, i, 0)));
        }
        assert_eq!(ds.mode(), Mode::Backpressured, "engages at n_high");
        ds.flush();
        assert_eq!(ds.mode(), Mode::Flowing, "disengages once drained below n_low");
    }
}
