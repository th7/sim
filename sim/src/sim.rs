//! The single-threaded simulation orchestrator (Phase 1).
//!
//! Holds the realms (one Overworld, zero-or-more Instances), an explicit
//! deterministic clock, and the player→realm routing. [`Sim::tick`] advances
//! every realm by one tick and the clock by [`consts::TICK_MS`]. Verbs and
//! Instance transitions are layered on in later modules; this core is enough to
//! prove the cluster model: movement, crossings, merges, splits, and the
//! never-under-merge invariant.

use crate::components::{Inventory, Position, PortalDirection, StructureKind};
use crate::consts::{FLUSH_MS, TICK_MS};
use crate::datastore::{Datastore, MemStore, PersistEvent, PlayerRecord};
use crate::geometry::{chunk_center, coord_for, ChunkCoord};
use crate::ids::{ClusterId, Realm};
use crate::verbs::VerbError;
use crate::world::{instance_bounds, RealmWorld};
use crate::worldgen;
use std::collections::BTreeMap;

/// A per-player server→client event the transport layer (Phase 4) pushes.
#[derive(Debug, Clone, PartialEq)]
pub enum OutboundEvent {
    /// The player's inventory changed (the `self` wire event).
    SelfInventory { username: String, inventory: Inventory },
    /// The player changed realm/chunk (the `relocated` wire event).
    Relocated { username: String, realm: Realm, coord: ChunkCoord },
}

pub struct Sim {
    clock_ms: u64,
    tick_count: u64,
    overworld: RealmWorld,
    instances: BTreeMap<u64, RealmWorld>,
    player_realm: BTreeMap<String, Realm>,
    /// Per-player Instance return info: `(entry chunk, entry portal pos)`.
    return_to: BTreeMap<String, (ChunkCoord, (i64, i64))>,
    pending: Vec<OutboundEvent>,
    next_instance: u64,
    pool: Option<crate::parallel::WorkerPool>,
    datastore: Datastore<MemStore>,
}

impl Default for Sim {
    fn default() -> Self {
        Sim::new()
    }
}

impl Sim {
    pub fn new() -> Self {
        Sim::with_persistence(MemStore::default())
    }

    /// Construct a Sim over an existing durable store — modelling a process
    /// restart that resumes from persisted state.
    pub fn with_persistence(store: MemStore) -> Self {
        Sim {
            clock_ms: 0,
            tick_count: 0,
            overworld: RealmWorld::new(Realm::Overworld, None),
            instances: BTreeMap::new(),
            player_realm: BTreeMap::new(),
            return_to: BTreeMap::new(),
            pending: Vec::new(),
            next_instance: 1,
            pool: None,
            datastore: Datastore::new(store),
        }
    }

    /// Force a Datastore flush (test/operator hook).
    pub fn flush_now(&mut self) {
        self.datastore.flush();
    }

    /// Consume the Sim and return its durable store (after flushing pending
    /// writes) — hand to [`Sim::with_persistence`] to model a restart.
    pub fn into_store(mut self) -> MemStore {
        self.datastore.flush();
        self.datastore.into_durable()
    }

    pub fn datastore(&self) -> &Datastore<MemStore> {
        &self.datastore
    }

    /// Attach a persistent worker pool of `workers` threads. Subsequent
    /// [`Sim::tick_parallel`] calls dispatch cluster movement to it instead of
    /// spawning threads per tick. Output is unchanged (still equals [`Sim::tick`]).
    pub fn enable_pool(&mut self, workers: usize) {
        self.pool = Some(crate::parallel::WorkerPool::new(workers));
    }

    pub fn clock_ms(&self) -> u64 {
        self.clock_ms
    }
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Connect a player, resuming from the Datastore: if their saved position is
    /// in `initial_chunk` they spawn there; otherwise at the chunk's center with
    /// their saved inventory (empty if never seen). Mirrors the Elixir
    /// `hydrate_player`.
    pub fn connect(&mut self, username: &str, initial_chunk: ChunkCoord) {
        let (pos, inv) = match self.datastore.fetch_player(username) {
            Some(rec) if rec.chunk == initial_chunk => {
                (Position { x: rec.x, y: rec.y }, Inventory { items: rec.inventory })
            }
            Some(rec) => {
                let (x, y) = chunk_center(initial_chunk);
                (Position { x, y }, Inventory { items: rec.inventory })
            }
            None => {
                let (x, y) = chunk_center(initial_chunk);
                (Position { x, y }, Inventory::default())
            }
        };
        self.spawn_overworld(username, pos, inv);
    }

    /// Connect at the center of `initial_chunk` with an explicit inventory,
    /// ignoring any saved position (used by tests).
    pub fn connect_with(&mut self, username: &str, initial_chunk: ChunkCoord, inv: Inventory) {
        let (x, y) = chunk_center(initial_chunk);
        self.spawn_overworld(username, Position { x, y }, inv);
    }

    /// Connect a player at an exact position with an explicit inventory.
    pub fn connect_at(&mut self, username: &str, pos: Position, inv: Inventory) {
        self.spawn_overworld(username, pos, inv);
    }

    fn spawn_overworld(&mut self, username: &str, pos: Position, inv: Inventory) {
        self.overworld.spawn_player(username, pos, inv);
        self.player_realm.insert(username.to_string(), Realm::Overworld);
        self.overlay_persisted_overworld();
        self.drain_persistence();
    }

    pub fn disconnect(&mut self, username: &str) {
        self.return_to.remove(username);
        if let Some(realm) = self.player_realm.remove(username) {
            // Leave-flush the player's final Overworld position before removal.
            if realm.is_overworld() {
                if let Some(ev) = self.overworld.player_upsert(username) {
                    self.datastore.apply(ev);
                }
            }
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

    /// Advance the whole world by one tick: movement + topology reconcile in
    /// every realm, then Instance entry/exit for any player overlapping a portal.
    pub fn tick(&mut self) {
        self.clock_ms += TICK_MS;
        self.tick_count += 1;
        self.overworld.tick(TICK_MS, self.clock_ms);
        for inst in self.instances.values_mut() {
            inst.tick(TICK_MS, self.clock_ms);
        }
        self.process_portals();
        self.overlay_persisted_overworld();
        self.drain_persistence();
        self.maybe_flush();
    }

    /// Advance the whole world by one tick, ticking each realm's clusters across
    /// a pool of `workers` threads under per-worker tick-time `budget` (seconds).
    /// Produces state identical to [`Sim::tick`] for any `workers`/`budget`.
    pub fn tick_parallel(&mut self, workers: usize, budget: f64) {
        self.clock_ms += TICK_MS;
        self.tick_count += 1;
        let dt = TICK_MS as f64 / 1000.0;
        let clock = self.clock_ms;

        let tick_realm = |rw: &mut RealmWorld| {
            let jobs = rw.movement_jobs();
            let assignment = rw.repack_assignment(budget);
            let results = match &self.pool {
                Some(pool) => pool.run(jobs, &assignment, dt),
                None => crate::parallel::execute(jobs, &assignment, workers, dt),
            };
            rw.apply_movement(results, clock);
        };

        tick_realm(&mut self.overworld);
        for inst in self.instances.values_mut() {
            tick_realm(inst);
        }
        self.process_portals();
        self.overlay_persisted_overworld();
        self.drain_persistence();
        self.maybe_flush();
    }

    /// Overlay persisted structures + depletion state onto any Overworld chunks
    /// hydrated since the last call.
    fn overlay_persisted_overworld(&mut self) {
        let clock = self.clock_ms;
        for coord in self.overworld.take_newly_loaded() {
            let structs = self.datastore.fetch_structures(coord);
            self.overworld.seed_persisted_structures(&structs);
            let deps = self.datastore.fetch_depletions(coord);
            self.overworld.apply_persisted_depletions(&deps, clock);
        }
        // Instances don't persist; just clear their newly-loaded buffer.
        for inst in self.instances.values_mut() {
            inst.take_newly_loaded();
        }
    }

    /// Move emitted persistence changes from the realms into the Datastore.
    fn drain_persistence(&mut self) {
        let evs = self.overworld.take_persist_events();
        self.datastore.apply_all(evs);
        for inst in self.instances.values_mut() {
            inst.take_persist_events(); // guarded empty, but keep buffers clear
        }
    }

    /// Periodic flush + player heartbeat, on the [`FLUSH_MS`] cadence.
    fn maybe_flush(&mut self) {
        let period = (FLUSH_MS / TICK_MS).max(1);
        if self.tick_count % period == 0 {
            for u in self.players_in(Realm::Overworld) {
                if let Some(ev) = self.overworld.player_upsert(&u) {
                    self.datastore.apply(ev);
                }
            }
            self.datastore.flush();
        }
    }

    /// Drain the queued per-player outbound events (for the transport layer).
    pub fn drain_events(&mut self) -> Vec<OutboundEvent> {
        std::mem::take(&mut self.pending)
    }

    // --- Verbs ---

    pub fn inventory_of(&self, username: &str) -> Option<Inventory> {
        let realm = self.realm_of(username)?;
        self.realm_world(realm)?.inventory_of(username)
    }

    pub fn harvest(&mut self, username: &str, tx: i64, ty: i64) -> Result<(), VerbError> {
        let realm = self.realm_of(username).ok_or(VerbError::NoPlayer)?;
        let clock = self.clock_ms;
        let rw = self.realm_world_mut(realm).ok_or(VerbError::NoChunk)?;
        let inv = rw.harvest(username, tx, ty, clock)?;
        self.pending.push(OutboundEvent::SelfInventory {
            username: username.to_string(),
            inventory: inv,
        });
        self.drain_persistence();
        Ok(())
    }

    pub fn build(
        &mut self,
        username: &str,
        kind: StructureKind,
        x: i64,
        y: i64,
    ) -> Result<(), VerbError> {
        let realm = self.realm_of(username).ok_or(VerbError::NoPlayer)?;
        if let Realm::Instance(_) = realm {
            return Err(VerbError::NoBuildInInstance);
        }
        let rw = self.realm_world_mut(realm).ok_or(VerbError::NoChunk)?;
        let inv = rw.build(username, kind, x, y)?;
        self.pending.push(OutboundEvent::SelfInventory {
            username: username.to_string(),
            inventory: inv,
        });
        self.drain_persistence();
        Ok(())
    }

    pub fn damage(&mut self, username: &str, x: i64, y: i64) -> Result<(), VerbError> {
        let realm = self.realm_of(username).ok_or(VerbError::NoPlayer)?;
        let rw = self.realm_world_mut(realm).ok_or(VerbError::NoChunk)?;
        rw.damage(username, x, y)?;
        self.drain_persistence();
        Ok(())
    }

    // --- Instance transitions (portal-triggered) ---

    fn process_portals(&mut self) {
        // Collect one trigger per player (post-movement positions).
        let mut triggers: Vec<(String, Realm, PortalDirection, i64, i64)> = Vec::new();
        for (username, &realm) in &self.player_realm {
            if let Some(rw) = self.realm_world(realm) {
                if let Some(pos) = rw.position_of(username) {
                    if let Some(&(dir, px, py)) = rw.overlapping_portals(pos.x, pos.y).first() {
                        triggers.push((username.clone(), realm, dir, px, py));
                    }
                }
            }
        }
        for (username, realm, dir, px, py) in triggers {
            match (dir, realm) {
                (PortalDirection::IntoInstance, Realm::Overworld) => {
                    self.enter_instance(&username, px, py);
                }
                (PortalDirection::OutOfInstance, Realm::Instance(id)) => {
                    self.exit_instance(&username, id);
                }
                _ => {}
            }
        }
    }

    /// Move `username` from the Overworld into a fresh Instance, spawning west of
    /// the return portal. `(entry_px, entry_py)` is the entry portal's position,
    /// cached for the symmetric exit.
    fn enter_instance(&mut self, username: &str, entry_px: i64, entry_py: i64) {
        let Some((_pos, inv)) = self.overworld.remove_player(username) else { return };
        let from_coord = coord_for(entry_px, entry_py);
        // Persist a save one unit west of the entry portal, so a mid-Instance
        // disconnect reconnects there (not on the portal, which would loop back).
        let save_x = entry_px - 1_000;
        let save_y = entry_py;
        self.datastore.apply(PersistEvent::UpsertPlayer(PlayerRecord {
            username: username.to_string(),
            chunk: coord_for(save_x, save_y),
            x: save_x,
            y: save_y,
            inventory: inv.items.clone(),
        }));
        let id = self.start_instance();
        let (rpx, rpy) = worldgen::return_portal_pos();
        let spawn = Position { x: rpx - 1_000, y: rpy };
        if let Some(rw) = self.instances.get_mut(&id) {
            rw.spawn_player(username, spawn, inv);
        }
        self.player_realm.insert(username.to_string(), Realm::Instance(id));
        self.return_to.insert(username.to_string(), (from_coord, (entry_px, entry_py)));
        self.pending.push(OutboundEvent::Relocated {
            username: username.to_string(),
            realm: Realm::Instance(id),
            coord: spawn.chunk(),
        });
    }

    /// Move `username` from Instance `id` back to the Overworld, re-emerging west
    /// of the entry portal, and tear the Instance down if now empty.
    fn exit_instance(&mut self, username: &str, id: u64) {
        let inv = self
            .instances
            .get_mut(&id)
            .and_then(|rw| rw.remove_player(username))
            .map(|(_p, i)| i)
            .unwrap_or_default();
        let (_from_coord, (epx, epy)) = self
            .return_to
            .remove(username)
            .unwrap_or((ChunkCoord::new(0, 0), (4_000, 4_000)));
        let spawn = Position { x: epx - 1_000, y: epy };
        self.overworld.spawn_player(username, spawn, inv);
        self.player_realm.insert(username.to_string(), Realm::Overworld);
        // Destination (Overworld) eagerly persists the re-emergence position.
        if let Some(ev) = self.overworld.player_upsert(username) {
            self.datastore.apply(ev);
        }
        self.overlay_persisted_overworld();
        self.drain_persistence();
        if self.instance_is_empty(id) {
            self.instances.remove(&id);
        }
        self.pending.push(OutboundEvent::Relocated {
            username: username.to_string(),
            realm: Realm::Overworld,
            coord: spawn.chunk(),
        });
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
}
