//! A single realm's simulation state: a `hecs::World` of entities, the
//! [`Labeler`] partitioning its dynamic actors into clusters, and the tick that
//! advances it. The Overworld is one `RealmWorld`; each Instance is another.
//!
//! The tick (single-threaded in Phase 1) is: for each cluster in id order,
//! integrate its players' movement against the static footprints in the
//! cluster's chunks; then detect chunk crossings and let the Labeler reconcile
//! the partition (merge/split); then respawn due resource nodes. Chunk static
//! content is hydrated lazily when a chunk first becomes owned by a cluster.

use crate::catalogue::{resource_footprint, resource_yield, structure_footprint};
use crate::collision::{clamp_step, Obstacle};
use crate::components::*;
use crate::consts::{DAMAGE_PER_CLICK, DEFAULT_SPEED, IDLE_TIMEOUT_MS, RESPAWN_MS};
use crate::datastore::{DepletionRecord, PersistEvent, PlayerRecord, StructureRecord};
use crate::geometry::{coord_for, ChunkCoord};
use crate::ids::{ActorId, ClusterId, Realm};
use crate::labeler::{Labeler, TopologyEvent};
use crate::verbs::VerbError;
use crate::worldgen;
use hecs::{Entity, World};
use std::collections::{BTreeMap, BTreeSet};

/// Optional bounding rectangle (sub-units) the movement integrator clamps to.
/// Instances are bounded to their 3×3 grid; the Overworld is unbounded.
pub type Bounds = (i64, i64, i64, i64);

pub struct RealmWorld {
    pub realm: Realm,
    pub world: World,
    pub labeler: Labeler,
    bounds: Option<Bounds>,
    /// Chunks whose static content has been seeded.
    loaded: BTreeSet<ChunkCoord>,
    username_index: BTreeMap<String, Entity>,
    actor_index: BTreeMap<ActorId, Entity>,
    wire_index: BTreeMap<WireId, Entity>,
    next_actor: u64,
    /// EWMA of per-cluster tick wall-time (seconds), feeding the repack policy.
    cluster_times: BTreeMap<ClusterId, f64>,
    /// Persistence changes emitted this step (Overworld only); drained by Sim.
    persist_events: Vec<PersistEvent>,
    /// Chunks first hydrated since the last drain; Sim overlays persisted state.
    newly_loaded: Vec<ChunkCoord>,
    /// Sim-clock time each loaded chunk was last owned by a cluster — drives
    /// idle deactivation (a chunk unowned for IDLE_TIMEOUT_MS goes cold).
    chunk_last_owned: BTreeMap<ChunkCoord, u64>,
}

impl RealmWorld {
    pub fn new(realm: Realm, bounds: Option<Bounds>) -> Self {
        RealmWorld {
            realm,
            world: World::new(),
            labeler: Labeler::new(),
            bounds,
            loaded: BTreeSet::new(),
            username_index: BTreeMap::new(),
            actor_index: BTreeMap::new(),
            wire_index: BTreeMap::new(),
            next_actor: 0,
            cluster_times: BTreeMap::new(),
            persist_events: Vec::new(),
            newly_loaded: Vec::new(),
            chunk_last_owned: BTreeMap::new(),
        }
    }

    /// Emit a persistence change — Overworld only (Instances don't persist).
    fn emit(&mut self, ev: PersistEvent) {
        if self.realm.is_overworld() {
            self.persist_events.push(ev);
        }
    }

    /// Drain persistence changes emitted since the last call (for the Datastore).
    pub fn take_persist_events(&mut self) -> Vec<PersistEvent> {
        std::mem::take(&mut self.persist_events)
    }

    /// Drain the chunks first hydrated since the last call (for persisted-state
    /// overlay by the Sim layer).
    pub fn take_newly_loaded(&mut self) -> Vec<ChunkCoord> {
        std::mem::take(&mut self.newly_loaded)
    }

    fn player_record(&self, username: &str) -> Option<PlayerRecord> {
        let pos = self.position_of(username)?;
        let inv = self.inventory_of(username)?;
        Some(PlayerRecord {
            username: username.to_string(),
            chunk: pos.chunk(),
            x: pos.x,
            y: pos.y,
            inventory: inv.items,
        })
    }

    /// Build a player upsert event for `username` at their current state.
    pub fn player_upsert(&self, username: &str) -> Option<PersistEvent> {
        self.player_record(username).map(PersistEvent::UpsertPlayer)
    }

    /// Seed persisted structures into a freshly-hydrated chunk.
    pub fn seed_persisted_structures(&mut self, records: &[StructureRecord]) {
        for r in records {
            self.insert_structure(r.x, r.y, r.kind, &r.owner, r.hp);
        }
    }

    /// Apply persisted depletion state to freshly-hydrated nodes: a node whose
    /// `respawn_at_ms` is still in the future becomes Depleted; past-due records
    /// are left gatherable (matching the Elixir hydrate_depletions).
    pub fn apply_persisted_depletions(&mut self, records: &[DepletionRecord], clock_ms: u64) {
        for r in records {
            if r.respawn_at_ms <= clock_ms {
                continue;
            }
            let wid = WireId(format!("{}:{}:{}", r.kind.as_str(), r.x, r.y));
            if let Some(e) = self.wire_index.get(&wid).copied() {
                let _ = self.world.remove_one::<Gatherable>(e);
                let _ = self.world.insert_one(
                    e,
                    Depleted { kind: r.kind, respawn_at_ms: r.respawn_at_ms },
                );
            }
        }
    }

    fn portals_for(&self, coord: ChunkCoord) -> Vec<worldgen::PortalSpec> {
        match self.realm {
            Realm::Overworld => worldgen::portals(coord),
            Realm::Instance(_) => worldgen::instance_portals(coord),
        }
    }

    /// Seed a chunk's static content (resource nodes + portals) once. Instances
    /// have no resource nodes. Structures are added by `build`/persistence.
    pub fn hydrate_chunk(&mut self, coord: ChunkCoord) {
        if !self.loaded.insert(coord) {
            return;
        }
        self.newly_loaded.push(coord);
        if self.realm.is_overworld() {
            for spec in worldgen::resource_nodes(coord) {
                let wid = WireId(format!("{}:{}:{}", spec.kind.as_str(), spec.x, spec.y));
                let e = self.world.spawn((
                    Position { x: spec.x, y: spec.y },
                    Renderable,
                    Gatherable { kind: spec.kind, yields: resource_yield(spec.kind) },
                    resource_footprint(spec.kind),
                    wid.clone(),
                ));
                self.wire_index.insert(wid, e);
            }
        }
        for spec in self.portals_for(coord) {
            let wid = WireId(format!("portal:{}:{}:{}", spec.kind.as_str(), spec.x, spec.y));
            let e = self.world.spawn((
                Position { x: spec.x, y: spec.y },
                Renderable,
                Portal { kind: spec.kind, direction: spec.direction },
                wid.clone(),
            ));
            self.wire_index.insert(wid, e);
        }
    }

    /// Spawn a player at `pos`, register it as a Labeler actor, and hydrate the
    /// chunks its cluster comes to own. Returns the new actor's entity.
    pub fn spawn_player(&mut self, username: &str, pos: Position, inventory: Inventory) -> Entity {
        let actor = ActorId(self.next_actor);
        self.next_actor += 1;
        let wid = WireId(username.to_string());
        let e = self.world.spawn((
            pos,
            Velocity { vx: 0.0, vy: 0.0 },
            Renderable,
            PlayerControlled { actor },
            inventory,
            wid.clone(),
        ));
        self.username_index.insert(username.to_string(), e);
        self.actor_index.insert(actor, e);
        self.wire_index.insert(wid, e);

        let _events = self.labeler.insert_actor(actor, pos.chunk());
        self.hydrate_owned_chunks();
        e
    }

    /// Remove a player from this realm. Returns its inventory + position (for a
    /// realm transition or persistence flush).
    pub fn remove_player(&mut self, username: &str) -> Option<(Position, Inventory)> {
        let e = self.username_index.remove(username)?;
        let pos = self.world.get::<&Position>(e).ok().map(|p| *p);
        let inv = self.world.get::<&Inventory>(e).ok().map(|i| (*i).clone());
        let actor = self.world.get::<&PlayerControlled>(e).ok().map(|p| p.actor);
        if let Some(actor) = actor {
            self.actor_index.remove(&actor);
            self.labeler.remove_actor(actor);
        }
        self.wire_index.remove(&WireId(username.to_string()));
        let _ = self.world.despawn(e);
        match (pos, inv) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        }
    }

    pub fn entity_of_username(&self, username: &str) -> Option<Entity> {
        self.username_index.get(username).copied()
    }

    pub fn contains_player(&self, username: &str) -> bool {
        self.username_index.contains_key(username)
    }

    /// Set a player's velocity from a normalized intent `(dx, dy)` (each in
    /// [-1, 1]); scaled by [`DEFAULT_SPEED`]. No-op if the player is absent.
    pub fn set_intent(&mut self, username: &str, dx: f64, dy: f64) {
        if let Some(&e) = self.username_index.get(username) {
            if let Ok(mut v) = self.world.get::<&mut Velocity>(e) {
                v.vx = dx * DEFAULT_SPEED;
                v.vy = dy * DEFAULT_SPEED;
            }
        }
    }

    pub fn position_of(&self, username: &str) -> Option<Position> {
        let e = self.username_index.get(username)?;
        self.world.get::<&Position>(*e).ok().map(|p| *p)
    }

    pub fn cluster_of_username(&self, username: &str) -> Option<ClusterId> {
        let e = self.username_index.get(username)?;
        let pc = self.world.get::<&PlayerControlled>(*e).ok()?;
        self.labeler.cluster_of(pc.actor)
    }

    /// Hydrate every owned-but-unloaded chunk (called after topology changes).
    fn hydrate_owned_chunks(&mut self) {
        let to_load: Vec<ChunkCoord> = self
            .labeler
            .owned_chunks()
            .filter(|c| !self.loaded.contains(c))
            .collect();
        for coord in to_load {
            self.hydrate_chunk(coord);
        }
    }

    /// Gather the obstacle footprints lying in `chunks` (for collision).
    fn obstacles_in(&self, chunks: &BTreeSet<ChunkCoord>) -> Vec<Obstacle> {
        let mut out = Vec::new();
        for (_e, (pos, fp)) in self.world.query::<(&Position, &Footprint)>().iter() {
            if chunks.contains(&pos.chunk()) {
                out.push(Obstacle { x: pos.x, y: pos.y, footprint: *fp });
            }
        }
        out
    }

    /// Advance the realm by one tick of `dt_ms` milliseconds at `clock_ms`.
    /// Returns the topology events produced by reconcile (for observers/tests).
    pub fn tick(&mut self, dt_ms: u64, clock_ms: u64) -> Vec<TopologyEvent> {
        let dt = dt_ms as f64 / 1000.0;

        // 1. Movement, per cluster, in id order (determinism). Gather each
        //    cluster's obstacle set from the chunks it owns.
        let cluster_ids: Vec<ClusterId> = self.labeler.clusters().map(|c| c.id).collect();
        for cid in cluster_ids {
            let Some(cluster) = self.labeler.cluster(cid) else { continue };
            let chunks = cluster.chunk_set.clone();
            let actors: Vec<ActorId> = cluster.actors.iter().copied().collect();
            let obstacles = self.obstacles_in(&chunks);

            for actor in actors {
                let Some(&e) = self.actor_index.get(&actor) else { continue };
                let (pos, vel) = {
                    let p = self.world.get::<&Position>(e).map(|p| *p);
                    let v = self.world.get::<&Velocity>(e).map(|v| *v);
                    match (p, v) {
                        (Ok(p), Ok(v)) => (p, v),
                        _ => continue,
                    }
                };
                let step_x = (vel.vx * dt).round() as i64;
                let step_y = (vel.vy * dt).round() as i64;
                let (nx, ny) = clamp_step(pos.x, pos.y, step_x, step_y, &obstacles);
                let (nx, ny) = self.clamp_bounds(nx, ny);
                if let Ok(mut p) = self.world.get::<&mut Position>(e) {
                    p.x = nx;
                    p.y = ny;
                }
            }
        }

        self.reconcile_after_movement(clock_ms)
    }

    /// Advance the realm by one tick using a worker pool of `worker_count`
    /// threads. Movement compute is parallel across clusters (assigned by the
    /// repack policy on cluster tick-times); topology reconcile stays serial in
    /// the Labeler. The result is identical to [`RealmWorld::tick`] regardless
    /// of `worker_count` — positions are applied in deterministic order.
    pub fn tick_parallel(
        &mut self,
        dt_ms: u64,
        clock_ms: u64,
        worker_count: usize,
        budget: f64,
    ) -> Vec<TopologyEvent> {
        let dt = dt_ms as f64 / 1000.0;
        let jobs = self.movement_jobs();
        let assignment = self.repack_assignment(budget);
        let results = crate::parallel::execute(jobs, &assignment, worker_count, dt);
        self.apply_movement(results, clock_ms)
    }

    /// Extract one owned movement job per cluster (read-only). Safe to hand to
    /// worker threads — distinct clusters are entity-disjoint by construction.
    pub fn movement_jobs(&self) -> Vec<crate::parallel::ClusterJob> {
        self.labeler
            .clusters()
            .map(|cluster| {
                let obstacles = self.obstacles_in(&cluster.chunk_set);
                let movers = cluster
                    .actors
                    .iter()
                    .filter_map(|a| {
                        let e = *self.actor_index.get(a)?;
                        let p = self.world.get::<&Position>(e).ok()?;
                        let v = self.world.get::<&Velocity>(e).ok()?;
                        Some((e, p.x, p.y, v.vx, v.vy))
                    })
                    .collect();
                crate::parallel::ClusterJob { cid: cluster.id, obstacles, movers, bounds: self.bounds }
            })
            .collect()
    }

    /// Repack assignment (`cluster → worker`) from the smoothed cluster
    /// tick-times under `budget`.
    pub fn repack_assignment(&self, budget: f64) -> BTreeMap<ClusterId, u32> {
        let times: BTreeMap<ClusterId, f64> = self
            .labeler
            .clusters()
            .map(|c| (c.id, self.cluster_times.get(&c.id).copied().unwrap_or(0.0)))
            .collect();
        crate::repack::repack(&times, budget).into_iter().map(|(c, w)| (c, w.0)).collect()
    }

    /// Apply computed cluster movement (deterministic order), update tick-time
    /// EWMAs, then run the serial topology reconcile + respawn.
    pub fn apply_movement(
        &mut self,
        results: BTreeMap<ClusterId, crate::parallel::ClusterResult>,
        clock_ms: u64,
    ) -> Vec<TopologyEvent> {
        for (cid, result) in &results {
            let smoothed = crate::repack::ewma(
                self.cluster_times.get(cid).copied().unwrap_or(result.elapsed_secs),
                result.elapsed_secs,
            );
            self.cluster_times.insert(*cid, smoothed);
            for &(e, nx, ny) in &result.positions {
                if let Ok(mut p) = self.world.get::<&mut Position>(e) {
                    p.x = nx;
                    p.y = ny;
                }
            }
        }
        let live: std::collections::BTreeSet<ClusterId> =
            self.labeler.clusters().map(|c| c.id).collect();
        self.cluster_times.retain(|c, _| live.contains(c));

        self.reconcile_after_movement(clock_ms)
    }

    /// Detect chunk crossings, let the Labeler reconcile (merge/split), hydrate
    /// any newly-owned chunks, and respawn due resource nodes. Shared by the
    /// serial and parallel ticks; this is the serialized Labeler domain.
    fn reconcile_after_movement(&mut self, clock_ms: u64) -> Vec<TopologyEvent> {
        let mut events = Vec::new();
        let crossings: Vec<(ActorId, ChunkCoord)> = self
            .actor_index
            .iter()
            .filter_map(|(&actor, &e)| {
                let pos = self.world.get::<&Position>(e).ok()?;
                let now = pos.chunk();
                let home = self.labeler.home_of(actor)?;
                (now != home).then_some((actor, now))
            })
            .collect();
        let crossed = !crossings.is_empty();
        for (actor, new_home) in crossings {
            events.extend(self.labeler.move_actor(actor, new_home));
        }
        // Hydrate whenever the owned chunk-set may have shifted — i.e. on any
        // crossing, not only on merge/split. A lone player walking into new
        // territory produces no topology events, but its cluster's footprint
        // still slides onto fresh chunks that need their content seeded.
        if crossed {
            self.hydrate_owned_chunks();
        }
        self.respawn_due(clock_ms);
        self.deactivate_idle_chunks(clock_ms);
        events
    }

    /// Unload chunks no cluster has owned for at least IDLE_TIMEOUT_MS — the
    /// cluster-model analogue of Chunk deactivation. Despawns the chunk's static
    /// content (it is re-seeded from worldgen + persistence on re-entry);
    /// players are never in unowned chunks, so no dynamic state is lost.
    fn deactivate_idle_chunks(&mut self, clock_ms: u64) {
        let owned: BTreeSet<ChunkCoord> = self.labeler.owned_chunks().collect();
        for c in &owned {
            self.chunk_last_owned.insert(*c, clock_ms);
        }
        let stale: Vec<ChunkCoord> = self
            .loaded
            .iter()
            .filter(|c| !owned.contains(c))
            .filter(|c| {
                let last = self.chunk_last_owned.get(c).copied().unwrap_or(0);
                clock_ms.saturating_sub(last) >= IDLE_TIMEOUT_MS
            })
            .copied()
            .collect();
        for coord in stale {
            self.unload_chunk(coord);
        }
    }

    fn unload_chunk(&mut self, coord: ChunkCoord) {
        let to_despawn: Vec<(Entity, WireId)> = self
            .world
            .query::<(&Position, &WireId, Option<&PlayerControlled>)>()
            .iter()
            .filter(|(_, (p, _, player))| p.chunk() == coord && player.is_none())
            .map(|(e, (_, wid, _))| (e, wid.clone()))
            .collect();
        for (e, wid) in to_despawn {
            self.wire_index.remove(&wid);
            let _ = self.world.despawn(e);
        }
        self.loaded.remove(&coord);
        self.chunk_last_owned.remove(&coord);
    }

    fn clamp_bounds(&self, x: i64, y: i64) -> (i64, i64) {
        match self.bounds {
            None => (x, y),
            Some((x0, y0, x1, y1)) => (x.clamp(x0, x1), y.clamp(y0, y1)),
        }
    }

    fn respawn_due(&mut self, clock_ms: u64) {
        let due: Vec<(Entity, ResourceKind, i64, i64)> = self
            .world
            .query::<(&Depleted, &Position)>()
            .iter()
            .filter(|(_, (d, _))| clock_ms >= d.respawn_at_ms)
            .map(|(e, (d, p))| (e, d.kind, p.x, p.y))
            .collect();
        for (e, kind, x, y) in due {
            let _ = self.world.remove_one::<Depleted>(e);
            let _ = self.world.insert_one(e, Gatherable { kind, yields: resource_yield(kind) });
            self.emit(PersistEvent::DeleteDepletion { x, y });
        }
    }

    // --- verb support (used by the Sim layer) ---

    pub fn wire_entity(&self, wid: &WireId) -> Option<Entity> {
        self.wire_index.get(wid).copied()
    }

    /// Number of chunks currently owned by some cluster (the "hot" chunks).
    pub fn owned_chunk_count(&self) -> usize {
        self.labeler.owned_chunks().count()
    }

    /// Whether `coord` is owned by a cluster (hot).
    pub fn is_chunk_hot(&self, coord: ChunkCoord) -> bool {
        self.labeler.owner_of_chunk(coord).is_some()
    }

    /// Count of positioned entities whose position falls in `coord`.
    pub fn entity_count_in(&self, coord: ChunkCoord) -> usize {
        self.world
            .query::<&Position>()
            .iter()
            .filter(|(_, p)| p.chunk() == coord)
            .count()
    }

    /// All entity wire states (for snapshot/delta building by the Sim layer).
    pub fn snapshot_states(&self) -> BTreeMap<WireId, crate::wire::EntityWire> {
        crate::wire::entity_states(self)
    }

    pub fn player_positions(&self) -> Vec<(i64, i64)> {
        self.world
            .query::<(&Position, &PlayerControlled)>()
            .iter()
            .map(|(_, (p, _))| (p.x, p.y))
            .collect()
    }

    /// Register a freshly-created entity (e.g. a built structure) in the wire
    /// index. Returns the entity.
    pub fn insert_structure(
        &mut self,
        x: i64,
        y: i64,
        kind: StructureKind,
        owner: &str,
        hp: i64,
    ) -> Entity {
        let wid = WireId(format!("structure:{x}:{y}"));
        let e = self.world.spawn((
            Position { x, y },
            Renderable,
            Structure { kind, owner: owner.to_string(), hp },
            structure_footprint(kind),
            wid.clone(),
        ));
        self.wire_index.insert(wid, e);
        e
    }

    pub fn despawn_wire(&mut self, wid: &WireId) {
        if let Some(e) = self.wire_index.remove(wid) {
            let _ = self.world.despawn(e);
        }
    }

    pub fn inventory_of(&self, username: &str) -> Option<Inventory> {
        let e = self.username_index.get(username)?;
        self.world.get::<&Inventory>(*e).ok().map(|i| (*i).clone())
    }

    /// Obstacles in the chunks owned by `username`'s cluster (for build checks).
    fn obstacles_for_cluster_of(&self, username: &str) -> Vec<Obstacle> {
        match self.cluster_of_username(username).and_then(|c| self.labeler.cluster(c)) {
            Some(cluster) => self.obstacles_in(&cluster.chunk_set),
            None => Vec::new(),
        }
    }

    // --- Verbs (mirroring GameCore.Chunk's with-chains in order) ---

    /// Harvest the tree at `(tx, ty)`. Adds one yield to the player's inventory
    /// and depletes the node (respawns after [`RESPAWN_MS`]). Returns the new
    /// inventory on success. Check order: no_player → too_far → no_target/depleted.
    pub fn harvest(
        &mut self,
        username: &str,
        tx: i64,
        ty: i64,
        clock_ms: u64,
    ) -> Result<Inventory, VerbError> {
        let (px, py) = self.position_of(username).map(|p| (p.x, p.y)).ok_or(VerbError::NoPlayer)?;
        if !in_range(px, py, tx, ty) {
            return Err(VerbError::TooFar);
        }
        let node_wid = WireId(format!("tree:{tx}:{ty}"));
        let node = self.wire_index.get(&node_wid).copied().ok_or(VerbError::NoTarget)?;

        let (kind, item) = {
            match self.world.get::<&Gatherable>(node) {
                Ok(g) => (g.kind, g.yields),
                Err(_) => {
                    // Present but not gatherable → depleted (or no_target).
                    return if self.world.get::<&Depleted>(node).is_ok() {
                        Err(VerbError::Depleted)
                    } else {
                        Err(VerbError::NoTarget)
                    };
                }
            }
        };

        // Yield +1 to inventory.
        let player_e = self.username_index[username];
        {
            let mut inv = self.world.get::<&mut Inventory>(player_e).map_err(|_| VerbError::NoPlayer)?;
            *inv.items.entry(item).or_insert(0) += 1;
        }
        // Deplete the node.
        let respawn_at = clock_ms + RESPAWN_MS;
        let _ = self.world.remove_one::<Gatherable>(node);
        let _ = self.world.insert_one(node, Depleted { kind, respawn_at_ms: respawn_at });

        // Persist: player inventory/position + the depletion.
        if let Some(ev) = self.player_upsert(username) {
            self.emit(ev);
        }
        self.emit(PersistEvent::UpsertDepletion(DepletionRecord {
            coord: coord_for(tx, ty),
            kind,
            x: tx,
            y: ty,
            respawn_at_ms: respawn_at,
        }));

        Ok(self.inventory_of(username).unwrap_or_default())
    }

    /// Build a structure of `kind` at `(x, y)`. Check order (the realm/instance
    /// gate is applied by the caller): out_of_chunk → footprint_blocked →
    /// no_player → insufficient_materials. Returns the new inventory.
    pub fn build(
        &mut self,
        username: &str,
        kind: StructureKind,
        x: i64,
        y: i64,
    ) -> Result<Inventory, VerbError> {
        // Build cell must be in the player's current chunk.
        let player_pos = self.position_of(username).ok_or(VerbError::NoPlayer)?;
        if coord_for(x, y) != player_pos.chunk() {
            return Err(VerbError::OutOfChunk);
        }

        // Footprint clear of obstacles and player bodies.
        let fp = crate::catalogue::structure_footprint(kind);
        let (w, h) = match fp {
            Footprint::Aabb { w, h } => (w, h),
            Footprint::Circle { radius } => (radius * 2, radius * 2),
        };
        let obstacles = self.obstacles_for_cluster_of(username);
        let players = self.player_positions();
        if crate::collision::aabb_blocked(x, y, w, h, &obstacles, &players) {
            return Err(VerbError::FootprintBlocked);
        }

        // Materials.
        let player_e = self.username_index[username];
        {
            let inv = self.world.get::<&Inventory>(player_e).map_err(|_| VerbError::NoPlayer)?;
            for &(item, qty) in crate::catalogue::cost(kind) {
                if inv.items.get(&item).copied().unwrap_or(0) < qty {
                    return Err(VerbError::InsufficientMaterials);
                }
            }
        }
        {
            let mut inv = self.world.get::<&mut Inventory>(player_e).unwrap();
            for &(item, qty) in crate::catalogue::cost(kind) {
                let e = inv.items.entry(item).or_insert(0);
                *e -= qty;
            }
        }

        let hp = crate::catalogue::max_hp(kind);
        self.insert_structure(x, y, kind, username, hp);

        // Persist: player inventory/position + the new structure.
        if let Some(ev) = self.player_upsert(username) {
            self.emit(ev);
        }
        self.emit(PersistEvent::UpsertStructure(StructureRecord {
            coord: coord_for(x, y),
            owner: username.to_string(),
            kind,
            x,
            y,
            hp,
        }));
        Ok(self.inventory_of(username).unwrap_or_default())
    }

    /// Damage the structure at `(x, y)` by [`DAMAGE_PER_CLICK`]. Destroys it at
    /// ≤0 HP. Check order: no_player → too_far → no_target. Returns the
    /// structure's remaining HP (`None` if destroyed).
    pub fn damage(&mut self, username: &str, x: i64, y: i64) -> Result<Option<i64>, VerbError> {
        let (px, py) = self.position_of(username).map(|p| (p.x, p.y)).ok_or(VerbError::NoPlayer)?;
        if !in_range(px, py, x, y) {
            return Err(VerbError::TooFar);
        }
        let wid = WireId(format!("structure:{x}:{y}"));
        let e = self.wire_index.get(&wid).copied().ok_or(VerbError::NoTarget)?;
        // Confirm it is a structure at exactly (x, y).
        let new_hp = {
            let s = self.world.get::<&Structure>(e).map_err(|_| VerbError::NoTarget)?;
            s.hp - DAMAGE_PER_CLICK
        };
        if new_hp > 0 {
            let owner = self.world.get::<&Structure>(e).map(|s| s.owner.clone()).unwrap_or_default();
            let kind = self.world.get::<&Structure>(e).map(|s| s.kind).unwrap_or(StructureKind::Wall);
            if let Ok(mut s) = self.world.get::<&mut Structure>(e) {
                s.hp = new_hp;
            }
            self.emit(PersistEvent::UpsertStructure(StructureRecord {
                coord: coord_for(x, y),
                owner,
                kind,
                x,
                y,
                hp: new_hp,
            }));
            Ok(Some(new_hp))
        } else {
            self.despawn_wire(&wid);
            self.emit(PersistEvent::DeleteStructure { x, y });
            Ok(None)
        }
    }

    /// Portals in this realm a player at `(px, py)` currently overlaps, with the
    /// portal's direction and position. Used for Instance entry/exit triggers.
    pub fn overlapping_portals(&self, px: i64, py: i64) -> Vec<(PortalDirection, i64, i64)> {
        let mut out = Vec::new();
        for (_e, (pos, portal)) in self.world.query::<(&Position, &Portal)>().iter() {
            let dx = px - pos.x;
            let dy = py - pos.y;
            if dx * dx + dy * dy <= crate::consts::PORTAL_OVERLAP_RANGE_SQ {
                out.push((portal.direction, pos.x, pos.y));
            }
        }
        out
    }
}

/// Within interact range (1.0 world unit) of a target, by squared distance.
fn in_range(px: i64, py: i64, tx: i64, ty: i64) -> bool {
    let dx = px - tx;
    let dy = py - ty;
    dx * dx + dy * dy <= crate::consts::INTERACT_RANGE_SQ
}

/// Bounds of an Instance: its 3×3 grid in instance-local sub-units.
pub fn instance_bounds() -> Bounds {
    let size = crate::geometry::CHUNK_SIZE;
    (0, 0, 3 * size, 3 * size)
}

/// Convenience: the chunk a sub-unit position falls in.
pub fn chunk_of(x: i64, y: i64) -> ChunkCoord {
    coord_for(x, y)
}
