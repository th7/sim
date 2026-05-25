//! The single-threaded simulation orchestrator (Phase 1).
//!
//! Holds the realms (one Overworld, zero-or-more Instances), an explicit
//! deterministic clock, and the player→realm routing. [`Sim::tick`] advances
//! every realm by one tick and the clock by [`consts::TICK_MS`]. Verbs and
//! Instance transitions are layered on in later modules; this core is enough to
//! prove the cluster model: movement, crossings, merges, splits, and the
//! never-under-merge invariant.

use crate::components::{Inventory, Position};
use crate::consts::TICK_MS;
use crate::geometry::{chunk_center, ChunkCoord};
use crate::ids::{ClusterId, Realm};
use crate::world::{instance_bounds, RealmWorld};
use std::collections::BTreeMap;

pub struct Sim {
    clock_ms: u64,
    tick_count: u64,
    overworld: RealmWorld,
    instances: BTreeMap<u64, RealmWorld>,
    player_realm: BTreeMap<String, Realm>,
    next_instance: u64,
}

impl Default for Sim {
    fn default() -> Self {
        Sim::new()
    }
}

impl Sim {
    pub fn new() -> Self {
        Sim {
            clock_ms: 0,
            tick_count: 0,
            overworld: RealmWorld::new(Realm::Overworld, None),
            instances: BTreeMap::new(),
            player_realm: BTreeMap::new(),
            next_instance: 1,
        }
    }

    pub fn clock_ms(&self) -> u64 {
        self.clock_ms
    }
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Connect a player, spawning their entity at the center of `initial_chunk`
    /// in the Overworld with an empty inventory. (Persistence-aware spawn comes
    /// in Phase 3.)
    pub fn connect(&mut self, username: &str, initial_chunk: ChunkCoord) {
        self.connect_with(username, initial_chunk, Inventory::default());
    }

    pub fn connect_with(&mut self, username: &str, initial_chunk: ChunkCoord, inv: Inventory) {
        let (x, y) = chunk_center(initial_chunk);
        self.overworld.spawn_player(username, Position { x, y }, inv);
        self.player_realm.insert(username.to_string(), Realm::Overworld);
    }

    /// Connect a player at an exact position (used by tests / persistence).
    pub fn connect_at(&mut self, username: &str, pos: Position, inv: Inventory) {
        self.overworld.spawn_player(username, pos, inv);
        self.player_realm.insert(username.to_string(), Realm::Overworld);
    }

    pub fn disconnect(&mut self, username: &str) {
        if let Some(realm) = self.player_realm.remove(username) {
            if let Some(rw) = self.realm_world_mut(realm) {
                rw.remove_player(username);
            }
            // An emptied Instance is torn down.
            if let Realm::Instance(id) = realm {
                if self.instance_is_empty(id) {
                    self.instances.remove(&id);
                }
            }
        }
    }

    pub fn set_intent(&mut self, username: &str, dx: f64, dy: f64) {
        if let Some(&realm) = self.player_realm.get(username) {
            if let Some(rw) = self.realm_world_mut(realm) {
                rw.set_intent(username, dx, dy);
            }
        }
    }

    /// Advance the whole world by one tick.
    pub fn tick(&mut self) {
        self.clock_ms += TICK_MS;
        self.tick_count += 1;
        self.overworld.tick(TICK_MS, self.clock_ms);
        for inst in self.instances.values_mut() {
            inst.tick(TICK_MS, self.clock_ms);
        }
    }

    // --- queries ---

    pub fn realm_of(&self, username: &str) -> Option<Realm> {
        self.player_realm.get(username).copied()
    }

    pub fn position(&self, username: &str) -> Option<Position> {
        let realm = self.realm_of(username)?;
        self.realm_world(realm)?.position_of(username)
    }

    pub fn cluster_of(&self, username: &str) -> Option<ClusterId> {
        let realm = self.realm_of(username)?;
        self.realm_world(realm)?.cluster_of_username(username)
    }

    pub fn overworld(&self) -> &RealmWorld {
        &self.overworld
    }

    pub fn overworld_mut(&mut self) -> &mut RealmWorld {
        &mut self.overworld
    }

    pub fn instance(&self, id: u64) -> Option<&RealmWorld> {
        self.instances.get(&id)
    }

    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    pub fn realm_world(&self, realm: Realm) -> Option<&RealmWorld> {
        match realm {
            Realm::Overworld => Some(&self.overworld),
            Realm::Instance(id) => self.instances.get(&id),
        }
    }

    pub fn realm_world_mut(&mut self, realm: Realm) -> Option<&mut RealmWorld> {
        match realm {
            Realm::Overworld => Some(&mut self.overworld),
            Realm::Instance(id) => self.instances.get_mut(&id),
        }
    }

    /// Player usernames currently in a given realm.
    pub fn players_in(&self, realm: Realm) -> Vec<String> {
        self.player_realm
            .iter()
            .filter(|(_, &r)| r == realm)
            .map(|(u, _)| u.clone())
            .collect()
    }

    fn instance_is_empty(&self, id: u64) -> bool {
        !self
            .player_realm
            .values()
            .any(|&r| r == Realm::Instance(id))
    }

    // --- Instance lifecycle (used by Phase 1b verb layer) ---

    /// Spawn a fresh Instance (a 3×3 bounded realm) and return its id.
    pub fn start_instance(&mut self) -> u64 {
        let id = self.next_instance;
        self.next_instance += 1;
        self.instances
            .insert(id, RealmWorld::new(Realm::Instance(id), Some(instance_bounds())));
        id
    }

    pub(crate) fn set_player_realm(&mut self, username: &str, realm: Realm) {
        self.player_realm.insert(username.to_string(), realm);
    }
}
