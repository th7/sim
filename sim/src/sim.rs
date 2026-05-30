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
use crate::motivation::{Drives, NpcKind};
use crate::datastore::{Datastore, DurableStore, MemStore, PersistEvent, PlayerRecord};
use crate::ecosystem::{self, Stratum};
use crate::geometry::{chunk_center, coord_for, ChunkCoord};
use crate::ids::{ClusterId, Realm};
use crate::verbs::VerbError;
use crate::world::{instance_bounds, RealmWorld};
use crate::worldgen;
use std::collections::{BTreeMap, BTreeSet};

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
    datastore: Datastore<BoxedStore>,
    /// Sparse, self-healing per-Region wildlife Disturbances (ADR-0006). In
    /// memory for now; cross-restart persistence is a flagged follow-up.
    wild_disturb: BTreeMap<ecosystem::RegionId, ecosystem::Disturbance>,
    /// Chunks currently materialized into live wildlife.
    wild_pop: BTreeSet<ChunkCoord>,
    /// Whether the warm/cold wildlife boundary runs. Off by default so core
    /// tests/e2e see an empty world; the game server turns it on.
    wildlife: bool,
}

/// Max wildlife per chunk at a level of 1.0 (the spawn-count capacities).
const DEER_CAP: u32 = 4;
const WOLF_CAP: u32 = 2;

/// Kills to fully deplete a Region's stratum (a territory spans many chunks, so a
/// kill moves the Region level far less than a chunk's worth). Tunable.
const REGION_DEER_CAPACITY: f64 = 24.0;
const REGION_WOLF_CAPACITY: f64 = 12.0;

/// Deterministic seed for a chunk's spawn rolls, bucketed by ~10 s of sim time so
/// the same chunk is stable while a Player lingers but varies across long gaps.
fn wild_seed(c: ChunkCoord, salt: u64, clock_ms: u64) -> u64 {
    (c.cx as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (c.cy as u64).rotate_left(21)
        ^ salt.wrapping_mul(0x632BE5AB1A55F0F1)
        ^ (clock_ms / 10_000)
}

/// A seeded spawn position inside chunk `c` (margin off the edges).
fn seeded_pos(c: ChunkCoord, i: u64, salt: u64) -> Position {
    use crate::geometry::CHUNK_SIZE;
    let mut rng = crate::harness::Rng::new(wild_seed(c, salt ^ i.wrapping_mul(0x9E3779B1), 0));
    let margin = 1_000;
    let span = (CHUNK_SIZE - 2 * margin).max(1) as u64;
    Position {
        x: c.cx as i64 * CHUNK_SIZE + margin + rng.below(span) as i64,
        y: c.cy as i64 * CHUNK_SIZE + margin + rng.below(span) as i64,
    }
}

/// The durable backend `Sim` persists through — boxed so it can be either an
/// in-memory [`MemStore`] (tests, fast default) or a Postgres store (the server).
pub type BoxedStore = Box<dyn DurableStore + Send>;

impl Default for Sim {
    fn default() -> Self {
        Sim::new()
    }
}

impl Sim {
    pub fn new() -> Self {
        Sim::with_store(MemStore::default())
    }

    /// Construct a Sim over any durable backend (in-memory or Postgres),
    /// resuming from whatever state it already holds — i.e. modelling a process
    /// restart.
    pub fn with_store(store: impl DurableStore + Send + 'static) -> Self {
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
            datastore: Datastore::new(Box::new(store)),
            wild_disturb: BTreeMap::new(),
            wild_pop: BTreeSet::new(),
            wildlife: false,
        }
    }

    /// Enable or disable the wildlife ecosystem (NPCs materializing near players).
    /// Off by default; the game server enables it.
    pub fn set_wildlife(&mut self, on: bool) {
        self.wildlife = on;
    }

    /// Resume from an existing store (kept for tests that round-trip
    /// `into_store` → `with_persistence` to model a restart).
    pub fn with_persistence(store: impl DurableStore + Send + 'static) -> Self {
        Sim::with_store(store)
    }

    /// Set the simulation clock's starting value (sub-unit-free, milliseconds).
    /// The server anchors this to wall-clock so depletion respawn timing is
    /// absolute and survives a real restart; tests leave it at 0 (deterministic).
    pub fn set_clock_ms(&mut self, ms: u64) {
        self.clock_ms = ms;
    }

    /// Force a Datastore flush (test/operator/shutdown hook).
    pub fn flush_now(&mut self) {
        self.datastore.flush();
    }

    /// Consume the Sim and return its durable store (after flushing pending
    /// writes) — hand to [`Sim::with_persistence`] to model a restart.
    pub fn into_store(mut self) -> BoxedStore {
        self.datastore.flush();
        self.datastore.into_durable()
    }

    pub fn datastore(&self) -> &Datastore<BoxedStore> {
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
        // Clean reconnect: tear down any live session for this username first
        // (flushing its position), so the resume below reads the freshest state
        // and no duplicate entity lingers. Mirrors the Elixir PlayerChannel.
        self.disconnect_if_present(username);
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

    /// Disconnect a username if it currently has a live session anywhere.
    fn disconnect_if_present(&mut self, username: &str) {
        if self.player_realm.contains_key(username) {
            self.disconnect(username);
        }
    }

    /// Spawn an NPC into the Overworld with initial drives. Returns its entity.
    pub fn spawn_npc(&mut self, kind: NpcKind, pos: Position, drives: Drives) -> hecs::Entity {
        self.overworld.spawn_npc(kind, pos, drives)
    }

    /// The warm/cold boundary (ADR-0005/0006): materialize wildlife from each
    /// Region's level into chunks that became Player-warm, and dissolve chunks
    /// that went cold back into their Region's Disturbance. Overworld only.
    fn update_wildlife(&mut self, clock_ms: u64) {
        if !self.wildlife {
            self.overworld.take_wild_kills(); // don't let kill events accumulate
            return;
        }

        // Depletion is **event-sourced from deaths** (player hunting + predation),
        // not from dissolve accounting: a wandering animal that crosses a chunk
        // boundary is the same animal, so only an actual kill lowers a Region.
        for (chunk, kind) in self.overworld.take_wild_kills() {
            let region = ecosystem::region_of_chunk(chunk);
            // Per-kill depletion is scaled to the Region's carrying capacity (a
            // territory spans many chunks), not one chunk — so overhunting takes
            // sustained killing, and incidental predation only dips-and-heals.
            let (stratum, per_kill) = match kind {
                NpcKind::Deer => (Stratum::Deer, -1.0 / REGION_DEER_CAPACITY),
                NpcKind::Wolf => (Stratum::Wolf, -1.0 / REGION_WOLF_CAPACITY),
            };
            self.wild_disturb.entry(region).or_default().disturb(stratum, per_kill, clock_ms);
        }

        let warm = self.overworld.player_warm_chunks();

        // Dissolve: chunks that left the warm set simply despawn their wildlife
        // (population-neutral — the field, not the live entities, is the truth).
        let cold: Vec<ChunkCoord> = self.wild_pop.iter().filter(|c| !warm.contains(c)).copied().collect();
        for c in cold {
            self.wild_pop.remove(&c);
            self.overworld.despawn_npcs_in(c);
        }
        // Drop fully-healed Disturbances to keep the set sparse.
        self.wild_disturb.retain(|_, d| !d.is_settled(clock_ms, 0.01));

        // Materialize: chunks newly in the warm set spawn wildlife from their
        // Region level, with spawn-derived temperament (ADR-0006 keystone).
        let fresh: Vec<ChunkCoord> = warm.iter().filter(|c| !self.wild_pop.contains(c)).copied().collect();
        for c in fresh {
            let region = ecosystem::region_of_chunk(c);
            let dist = self.wild_disturb.get(&region).copied().unwrap_or_default();
            let lv = ecosystem::levels(region, clock_ms, &dist);
            let dn = ecosystem::spawn_count(lv.deer, DEER_CAP, wild_seed(c, 0xD, clock_ms));
            let wn = ecosystem::spawn_count(lv.wolf, WOLF_CAP, wild_seed(c, 0x7, clock_ms));
            for i in 0..dn {
                let p = seeded_pos(c, i as u64, 0xDEE2);
                self.overworld.spawn_npc(NpcKind::Deer, p, ecosystem::initial_drives(NpcKind::Deer, &lv));
            }
            for i in 0..wn {
                let p = seeded_pos(c, i as u64, 0x401F);
                self.overworld.spawn_npc(NpcKind::Wolf, p, ecosystem::initial_drives(NpcKind::Wolf, &lv));
            }
            self.wild_pop.insert(c);
        }
    }

    /// Region wildlife levels at a world point, given current Disturbances (test
    /// observability of the healing field).
    pub fn region_levels_at(&self, x: i64, y: i64) -> ecosystem::Levels {
        let region = ecosystem::region(x, y);
        let dist = self.wild_disturb.get(&region).copied().unwrap_or_default();
        ecosystem::levels(region, self.clock_ms, &dist)
    }

    /// Snapshot of every Overworld NPC (kind, position, drives, health).
    pub fn npcs(&self) -> Vec<(hecs::Entity, NpcKind, Position, Drives, crate::components::Health)> {
        self.overworld.npcs()
    }

    /// Count of live NPCs in the Overworld (dev telemetry).
    pub fn npc_count(&self) -> usize {
        self.overworld.npcs().len()
    }

    fn spawn_overworld(&mut self, username: &str, pos: Position, inv: Inventory) {
        self.disconnect_if_present(username);
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
        self.update_wildlife(self.clock_ms);
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
            rw.drive_npcs(TICK_MS, clock);
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
        self.update_wildlife(self.clock_ms);
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

    /// Player heartbeat (every FLUSH_MS) + Datastore flush-to-durable (every
    /// DB_FLUSH_MS). Heartbeat keeps standing players' positions fresh; the
    /// flush makes pending writes durable so they survive a restart.
    fn maybe_flush(&mut self) {
        let heartbeat_period = (FLUSH_MS / TICK_MS).max(1);
        if self.tick_count % heartbeat_period == 0 {
            for u in self.players_in(Realm::Overworld) {
                if let Some(ev) = self.overworld.player_upsert(&u) {
                    self.datastore.apply(ev);
                }
            }
        }
        let flush_period = (crate::consts::DB_FLUSH_MS / TICK_MS).max(1);
        if self.tick_count % flush_period == 0 {
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
        let clock = self.clock_ms;
        let rw = self.realm_world_mut(realm).ok_or(VerbError::NoChunk)?;
        rw.damage(username, x, y, clock)?;
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

    /// Total hot chunks across all realms — the `stats.active_chunks` value.
    pub fn active_chunk_count(&self) -> usize {
        self.overworld.owned_chunk_count()
            + self.instances.values().map(|i| i.owned_chunk_count()).sum::<usize>()
    }

    /// Connected player count — the `stats.total_players` value.
    pub fn player_count(&self) -> usize {
        self.player_realm.len()
    }

    /// `(hot, entity_count)` for `coord` in `realm` — a general chunk query
    /// (chunk hotness + occupancy). The dev overlay's richer lifecycle (idle
    /// countdown) lives in [`RealmWorld::chunk_lifecycle`].
    pub fn chunk_status(&self, realm: Realm, coord: ChunkCoord) -> (bool, usize) {
        match self.realm_world(realm) {
            Some(rw) => (rw.is_chunk_hot(coord), rw.entity_count_in(coord)),
            None => (false, 0),
        }
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
