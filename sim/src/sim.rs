//! The single-threaded simulation orchestrator (Phase 1).
//!
//! Holds the realms (one Overworld, zero-or-more Instances), an explicit
//! deterministic clock, and the player→realm routing. [`Sim::tick`] advances
//! every realm by one tick and the clock by [`consts::TICK_MS`]. Actions and
//! Instance transitions are layered on in later modules; this core is enough to
//! prove the cluster model: movement, crossings, merges, splits, and the
//! never-under-merge invariant.

use crate::components::{Inventory, Position, PortalDirection, StructureKind, WireId};
use crate::consts::{FLUSH_MS, TICK_MS};
use crate::motivation::{Drives, NpcKind};
use crate::datastore::{Datastore, DurableStore, MemStore, PersistEvent, PlayerRecord, Thresholds};
use crate::ecosystem::{self, Stratum};
use crate::geometry::{chunk_center, coord_for, ChunkCoord};
use crate::ids::{ClusterId, Realm};
use crate::actions::{ActionOutcome, ActionError};
use crate::world::{instance_bounds, RealmWorld};
use crate::worldgen;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A per-player server→client event the transport layer (Phase 4) pushes.
#[derive(Debug, Clone, PartialEq)]
pub enum OutboundEvent {
    /// The player's inventory changed (the `self` wire event).
    SelfInventory { username: String, inventory: Inventory },
    /// The player changed realm/chunk (the `relocated` wire event).
    Relocated { username: String, realm: Realm, coord: ChunkCoord },
    /// A queued action could not be carried out — either dropped at the door
    /// (`queue_full`) or rejected by the verb at tick-time (`too_far`, …). The
    /// single async outcome channel for action failures.
    ActionRejected { username: String, verb: &'static str, at: RejectedAt, reason: &'static str },
    /// The player's last-consumed movement input seq as of `tick` (the `ack`
    /// wire event) — the anchor the client's Mirror replays unacked frames on.
    MoveAck { username: String, seq: u32, tick: u64 },
}

/// What a rejected action was aimed at — a world cell for placement verbs, a
/// Target's WireId for entity-directed verbs.
#[derive(Debug, Clone, PartialEq)]
pub enum RejectedAt {
    Cell { x: i64, y: i64 },
    Entity(WireId),
}

/// A queued, fire-and-forget player action intent. Enqueued on receipt and
/// resolved in the tick (never on receipt) — the unified intent model, so an
/// overload freeze is simply "skip the tick". Entity-directed verbs (harvest)
/// name their Target's [`WireId`]; placement (build) stays cell-addressed.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Harvest { target: WireId },
    Build { kind: StructureKind, x: i64, y: i64 },
    Damage { target: WireId },
}

impl Action {
    /// The wire verb name and what it was aimed at — used to attribute an
    /// outcome event.
    fn attribution(&self) -> (&'static str, RejectedAt) {
        match self {
            Action::Harvest { target } => ("harvest", RejectedAt::Entity(target.clone())),
            Action::Build { x, y, .. } => ("build", RejectedAt::Cell { x: *x, y: *y }),
            Action::Damage { target } => ("damage", RejectedAt::Entity(target.clone())),
        }
    }
}

/// Most actions an actor may have queued for resolution. Beyond this, new
/// intents are refused at the door (`queue_full`) — bounding memory and a
/// post-freeze resume burst by construction.
const ACTION_QUEUE_CAP: usize = 8;

/// One seq-tagged movement input frame: the per-tick movement Intent a live
/// session renews. Consumed one-per-tick by the simulation (never on receipt),
/// so the client's Mirror can replay its unacked frames exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MoveFrame {
    seq: u32,
    dx: f64,
    dy: f64,
}

/// Most movement frames a player may have queued. Beyond this, the oldest is
/// dropped — bounding memory; the resulting divergence is corrected by the
/// client's next override like any other misprediction.
const MOVE_QUEUE_CAP: usize = 64;

pub struct Sim {
    clock_ms: u64,
    tick_count: u64,
    overworld: RealmWorld,
    instances: BTreeMap<u64, RealmWorld>,
    player_realm: BTreeMap<String, Realm>,
    /// Per-player Instance return info: `(entry chunk, entry portal pos)`.
    return_to: BTreeMap<String, (ChunkCoord, (i64, i64))>,
    pending: Vec<OutboundEvent>,
    /// Per-actor queued action intents, resolved in the tick (never on
    /// receipt), each pinned to the movement seq at press time: an action
    /// resolves in the tick its seq's movement integrates (post-movement), so
    /// eligibility is judged at the exact position the pressing client
    /// displayed.
    action_queues: BTreeMap<String, VecDeque<(Action, u32, u64)>>,
    /// Per-player highest Frontier asserted so far (monotone by law: a session
    /// never un-sees an authoritative tick; a regressing claim is clamped up).
    asserted_frontier: BTreeMap<String, u64>,
    /// Impossible Frontier claims seen (never-future or regressing — neither
    /// is producible by an honest session). Claims are clamped (worthless),
    /// never punished; this count is the probe signal the dev stats surface.
    frontier_violations: u64,
    /// Per-player queued movement input frames, consumed one per tick.
    move_queues: BTreeMap<String, VecDeque<MoveFrame>>,
    /// Per-player ticks since the last consumed frame. Intent is perishable:
    /// past [`crate::consts::INTENT_GRACE_TICKS`] it expires to zero.
    move_starved: BTreeMap<String, u64>,
    /// Per-player last-consumed movement seq, acked to the client's Mirror.
    last_move_seq: BTreeMap<String, u32>,
    next_instance: u64,
    pool: Option<crate::parallel::WorkerPool>,
    datastore: Datastore<BoxedStore>,
    /// Sparse, self-healing per-Region wildlife Disturbances. In
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
            action_queues: BTreeMap::new(),
            move_queues: BTreeMap::new(),
            move_starved: BTreeMap::new(),
            last_move_seq: BTreeMap::new(),
            asserted_frontier: BTreeMap::new(),
            frontier_violations: 0,
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

    /// Retune the Datastore's backpressure high/low-water marks (tests drive
    /// overload deterministically; a deployment could tune them).
    pub fn set_persist_thresholds(&mut self, thresholds: Thresholds) {
        self.datastore.set_thresholds(thresholds);
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

    /// Bring the durable store fully up to date — the graceful-shutdown and
    /// panic paths both call this so the runtime loses as little as possible on
    /// the way down. Refreshes every standing player's current position (the
    /// heartbeat otherwise only runs every `FLUSH_MS`), drains any buffered
    /// persist events, then flushes to durable.
    pub fn flush_now(&mut self) {
        for u in self.players_in(Realm::Overworld) {
            if let Some(ev) = self.overworld.player_upsert(&u) {
                self.datastore.apply(ev);
            }
        }
        self.drain_persistence();
        self.datastore.flush();
    }

    /// Consume the Sim and return its durable store (after flushing pending
    /// writes) — hand to [`Sim::with_persistence`] to model a restart.
    pub fn into_store(mut self) -> BoxedStore {
        self.datastore.flush();
        self.datastore.into_durable()
    }

    /// Whether the Datastore is backpressured — the world freezes (the tick is
    /// skipped) while true, resuming once the buffer drains. The single overload
    /// signal exposed to callers; the counter and thresholds stay the
    /// Datastore's own business.
    pub fn backpressured(&self) -> bool {
        self.datastore.backpressured()
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
    /// Total impossible Frontier claims observed (see `enqueue_action`).
    pub fn frontier_violations(&self) -> u64 {
        self.frontier_violations
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

    /// The warm/cold boundary: materialize wildlife from each
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
        // Region level, with spawn-derived temperament.
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
        self.move_queues.remove(username);
        self.move_starved.remove(username);
        self.last_move_seq.remove(username);
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

    /// Enqueue one seq-tagged movement input frame (fire-and-forget). Consumed
    /// one-per-tick by [`Sim::consume_move_frames`], never applied on receipt.
    /// At [`MOVE_QUEUE_CAP`] the oldest frame is dropped.
    pub fn enqueue_move(&mut self, username: &str, seq: u32, dx: f64, dy: f64) {
        let queue = self.move_queues.entry(username.to_string()).or_default();
        if queue.len() >= MOVE_QUEUE_CAP {
            queue.pop_front();
        }
        queue.push_back(MoveFrame { seq, dx, dy });
    }

    /// Tick-start consumption: exactly one queued movement frame per player
    /// becomes that player's Intent for this tick; its seq is recorded for the
    /// ack. Intent is perishable: an empty queue leaves the current Intent
    /// standing for [`crate::consts::INTENT_GRACE_TICKS`] ticks (absorbing
    /// jitter), then expires it to zero — a stalled or vanished session's
    /// player stands still rather than walking on stale Intent.
    fn consume_move_frames(&mut self) {
        let mut consumed: Vec<(String, MoveFrame)> = Vec::new();
        let mut expired: Vec<String> = Vec::new();
        for (u, q) in self.move_queues.iter_mut() {
            match q.pop_front() {
                Some(f) => {
                    self.move_starved.insert(u.clone(), 0);
                    consumed.push((u.clone(), f));
                }
                None => {
                    let starved = self.move_starved.entry(u.clone()).or_insert(0);
                    *starved += 1;
                    if *starved == crate::consts::INTENT_GRACE_TICKS + 1 {
                        expired.push(u.clone());
                    }
                }
            }
        }
        for (username, frame) in consumed {
            self.set_intent(&username, frame.dx, frame.dy);
            self.last_move_seq.insert(username, frame.seq);
        }
        for username in expired {
            self.set_intent(&username, 0.0, 0.0);
        }
    }

    /// The last movement seq consumed for a player — what the next ack will
    /// carry. Exposed for wire-fidelity tests.
    pub fn last_move_seq(&self, username: &str) -> Option<u32> {
        self.last_move_seq.get(username).copied()
    }

    /// At broadcast ticks, ack each player's last-consumed movement seq along
    /// with the tick it is current as of — the anchor the Mirror replays from.
    fn emit_move_acks(&mut self) {
        if self.tick_count % crate::consts::BROADCAST_EVERY != 0 {
            return;
        }
        for (username, &seq) in &self.last_move_seq {
            self.pending.push(OutboundEvent::MoveAck {
                username: username.clone(),
                seq,
                tick: self.tick_count,
            });
        }
    }

    /// Advance one tick under a panic guard. A panic means the runtime is
    /// presumed corrupt — we do **not** swallow it and limp on. Instead we flush
    /// durable state (bounding loss to the unflushed window) and return the panic
    /// payload so the caller takes the whole runtime down: a clean, lossless
    /// crash a supervisor can restart from the durable store.
    pub fn tick_or_flush(&mut self) -> std::thread::Result<()> {
        // Overload freeze: the Datastore can't keep up, so skip the tick body —
        // the world (clock included) does not advance, no new writes are made,
        // and no queued action resolves. We still flush, unconditionally, so the
        // backlog drains and the freeze self-relieves the moment it falls below
        // the low-water mark. "Freeze" is literally skip-the-tick.
        if self.datastore.backpressured() {
            return self.guard(|s| s.freeze_flush());
        }
        match self.pool.as_ref().map(|p| p.size()) {
            // With a pool, drive the parallel tick (movement compute on workers).
            // `workers` is ignored by `tick_parallel` when a pool is present; the
            // repack budget is the per-tick wall-clock budget.
            Some(workers) => {
                let budget = TICK_MS as f64 / 1000.0;
                self.guard(|s| s.tick_parallel(workers, budget))
            }
            None => self.guard(|s| s.tick()),
        }
    }

    /// The frozen-tick body: don't touch the clock or resolve anything; just
    /// drain any residual buffered writes and flush durable, which re-evaluates
    /// the backpressure mode and lets it disengage once the buffer is below the
    /// low-water mark.
    fn freeze_flush(&mut self) {
        self.drain_persistence();
        self.datastore.flush();
    }

    /// Run `body`; if it panics, flush durable state and return the panic payload.
    /// Catching here (while the caller still holds the Sim) keeps the panic from
    /// poisoning the shared lock, and lets us flush before going down.
    fn guard<R>(&mut self, body: impl FnOnce(&mut Sim) -> R) -> std::thread::Result<R> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(self))) {
            Ok(r) => Ok(r),
            Err(payload) => {
                self.flush_now();
                Err(payload)
            }
        }
    }

    /// Advance the whole world by one tick: movement + topology reconcile in
    /// every realm, then Instance entry/exit for any player overlapping a portal.
    pub fn tick(&mut self) {
        self.clock_ms += TICK_MS;
        self.tick_count += 1;
        // Movement first, verbs after — the simultaneity law. An intent is
        // processed *with its tick*: this tick's frame is consumed and
        // integrated, then verbs resolve, judged at exactly the position the
        // press frame displayed (a verb pinned to seq S resolves in the very
        // tick S integrates). Arrival-into beats placement: a build is judged
        // against final positions, so a wall can never appear under a body.
        // Within the phase, NPC actions (inside the realm tick) precede
        // player verbs; portals trigger last, so a verb always resolves in
        // the realm it was pressed in.
        self.consume_move_frames();
        self.overworld.tick(TICK_MS, self.clock_ms);
        for inst in self.instances.values_mut() {
            inst.tick(TICK_MS, self.clock_ms);
        }
        self.resolve_action_intents();
        self.process_portals();
        self.update_wildlife(self.clock_ms);
        self.emit_move_acks();
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

        self.consume_move_frames();

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
        // Same phase law as the serial tick: movement first, verbs after,
        // portals last.
        self.resolve_action_intents();
        self.process_portals();
        self.update_wildlife(self.clock_ms);
        self.emit_move_acks();
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

    // --- Actions ---

    // The synchronous verb-effect primitives the async intent path
    // ([`enqueue_action`] + tick) wraps. `resolve_action_intents` calls these
    // after its seq-pinning; tests call them directly for the effect's outcome
    // (range/depletion/materials/footprint reasons). They route the actor to its
    // realm and apply the effect there — the one door onto the verb logic, in
    // place of handing callers a whole `RealmWorld`.

    /// Apply the harvest verb now, returning its [`ActionOutcome`].
    pub fn harvest(&mut self, username: &str, target: &WireId) -> Result<ActionOutcome, ActionError> {
        let realm = self.realm_of(username).ok_or(ActionError::NoPlayer)?;
        let clock = self.clock_ms;
        self.realm_world_mut(realm).ok_or(ActionError::NoChunk)?.harvest(username, target, clock)
    }

    /// Place a Structure of `kind` at `(x, y)` now, returning its [`ActionOutcome`].
    pub fn build(
        &mut self,
        username: &str,
        kind: StructureKind,
        x: i64,
        y: i64,
    ) -> Result<ActionOutcome, ActionError> {
        let realm = self.realm_of(username).ok_or(ActionError::NoPlayer)?;
        self.realm_world_mut(realm).ok_or(ActionError::NoChunk)?.build(username, kind, x, y)
    }

    /// Damage the targeted entity now, judged at press-frame `frontier` (the
    /// async path supplies the Lead-clamped value; a synchronous caller passes
    /// the current tick). Returns its [`ActionOutcome`] (`Silent`).
    pub fn damage(
        &mut self,
        username: &str,
        target: &WireId,
        frontier: u64,
    ) -> Result<ActionOutcome, ActionError> {
        let realm = self.realm_of(username).ok_or(ActionError::NoPlayer)?;
        let clock = self.clock_ms;
        self.realm_world_mut(realm).ok_or(ActionError::NoChunk)?.damage(username, target, clock, frontier)
    }

    pub fn inventory_of(&self, username: &str) -> Option<Inventory> {
        let realm = self.realm_of(username)?;
        self.realm_world(realm)?.inventory_of(username)
    }

    // Player verbs (harvest/build/damage) are not applied on receipt: they are
    // fire-and-forget [`Action`] intents (see [`Sim::enqueue_action`]) resolved
    // in the tick by [`Sim::resolve_action_intents`]. The verb *logic* lives on
    // the realm (`RealmWorld::{harvest,build,damage}`).

    /// Enqueue a player action intent (fire-and-forget), pinned to the
    /// movement `seq` at press time and carrying the session's asserted
    /// `frontier` (the last authoritative tick it incorporated — the basis of
    /// lawful-render judging). Hard checks at the door: **never-future** (a
    /// claim past the present is clamped to it) and **monotone** (a session
    /// never un-sees a tick; regressing claims are clamped up to the highest
    /// asserted). Resolved in the tick — the very tick `seq`'s movement
    /// integrates, after movement — never on receipt.
    pub fn enqueue_action(&mut self, username: &str, action: Action, seq: u32, frontier: u64) {
        if frontier > self.tick_count {
            self.frontier_violations += 1; // never-future: impossible honestly
        }
        let f = frontier.min(self.tick_count);
        let stored = self.asserted_frontier.entry(username.to_string()).or_insert(0);
        if f < *stored {
            self.frontier_violations += 1; // regression: impossible honestly
        }
        let f = f.max(*stored); // monotone
        *stored = f;
        let queue = self.action_queues.entry(username.to_string()).or_default();
        if queue.len() >= ACTION_QUEUE_CAP {
            // Reject the newest; the committed queue is honoured first.
            let (verb, at) = action.attribution();
            self.pending.push(OutboundEvent::ActionRejected {
                username: username.to_string(),
                verb,
                at,
                reason: "queue_full",
            });
            return;
        }
        queue.push_back((action, seq, f));
    }

    /// Resolve queued action intents, in id (username) order, FIFO within an
    /// actor — run *after* this tick's movement has integrated (the
    /// simultaneity law: arrival-into beats placement, and a build judged
    /// against final positions can never appear under a body). An intent is
    /// processed with its tick: an action pinned to seq S resolves in the
    /// very tick that consumed S, judged at exactly the position its press
    /// frame displayed. An action pinned past everything the session ever
    /// sent (the move queue is empty and S is still unconsumed — a fabricated
    /// seq, or a test hook's 0 on a fresh session) resolves immediately: the
    /// Island judges with what it has rather than holding a verb hostage to a
    /// frame that will never come. "Never come" is the **reachability rule**:
    /// a pin is held only while its seq is ≤ the highest seq that exists or is
    /// en route (consumed ∪ queued); a seq above that envelope — a fabricated
    /// future, or a frame dropped under queue overflow — can never be
    /// satisfied, so it resolves now. Honest pins are untouched (a press's
    /// frame always precedes it on the wire); the rule has no magic timeout
    /// and a moving fabricator can only wedge their own FIFO, never escape it.
    fn resolve_action_intents(&mut self) {
        let users: Vec<String> = self.action_queues.keys().cloned().collect();
        for username in users {
            if self.realm_of(&username).is_none() {
                self.action_queues.remove(&username);
                continue;
            }
            loop {
                let consumed = self.last_move_seq.get(&username).copied().unwrap_or(0);
                // The highest seq that exists or is en route: a pin at or below
                // it is *reachable* and waited for (FIFO); a pin above it can
                // never be satisfied by any real Input frame and resolves now
                // (judged at the current position — the least-generous frame).
                let max_reachable = self
                    .move_queues
                    .get(&username)
                    .and_then(|q| q.iter().map(|f| f.seq).max())
                    .unwrap_or(0)
                    .max(consumed);
                let Some(queue) = self.action_queues.get_mut(&username) else { break };
                let Some(&(_, seq, _)) = queue.front() else { break };
                if seq > consumed && seq <= max_reachable {
                    break; // pinned to a frame still to integrate — wait, FIFO
                }
                let (action, _, frontier) = queue.pop_front().expect("front checked");
                // The Lead law: a press cannot reach further back than the
                // Mirror could lawfully have been — clamp into the window.
                let frontier =
                    frontier.max(self.tick_count.saturating_sub(crate::consts::LEAD_BOUND_TICKS));
                let (verb, at) = action.attribution();
                let outcome = match action {
                    Action::Harvest { target } => self.harvest(&username, &target),
                    Action::Build { kind, x, y } => self.build(&username, kind, x, y),
                    Action::Damage { target } => self.damage(&username, &target, frontier),
                };
                match outcome {
                    Ok(ActionOutcome::Inventory(inventory)) => self
                        .pending
                        .push(OutboundEvent::SelfInventory { username: username.clone(), inventory }),
                    Ok(ActionOutcome::Silent) => {}
                    Err(e) => self.pending.push(OutboundEvent::ActionRejected {
                        username: username.clone(),
                        verb,
                        at,
                        reason: e.as_str(),
                    }),
                }
            }
            if self.action_queues.get(&username).is_some_and(|q| q.is_empty()) {
                self.action_queues.remove(&username);
            }
        }
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

    pub(crate) fn realm_world_mut(&mut self, realm: Realm) -> Option<&mut RealmWorld> {
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::components::Item;

    /// The unified intent model: an enqueued action is recorded, not resolved, on
    /// receipt; the *tick* resolves it. (Tracer for the whole migration.)
    #[test]
    fn enqueued_action_resolves_on_the_next_tick_not_on_receipt() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 0, 0);

        // Not resolved on receipt — inventory unchanged.
        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood).copied().unwrap_or(0),
            0,
            "enqueue must not resolve the action on receipt"
        );

        sim.tick();

        // The tick resolved it.
        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood).copied(),
            Some(1),
            "the queued harvest resolves in the tick"
        );
    }

    fn queue_full_events(evs: &[OutboundEvent]) -> usize {
        evs.iter()
            .filter(|e| matches!(e, OutboundEvent::ActionRejected { reason: "queue_full", .. }))
            .count()
    }

    /// A full action queue (cap 8) refuses the *newest* intent with an async
    /// `queue_full` rejection; the first cap intents are accepted silently.
    #[test]
    fn a_full_action_queue_rejects_the_newest_intent() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());

        for _ in 0..ACTION_QUEUE_CAP {
            sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 0, 0);
        }
        assert_eq!(queue_full_events(&sim.drain_events()), 0, "the first cap intents are accepted");

        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 0, 0);
        assert_eq!(
            queue_full_events(&sim.drain_events()),
            1,
            "the intent past the cap is refused with queue_full"
        );
    }

    /// Seq-pinning: a Action carrying movement seq S resolves at the tick after
    /// S's movement has integrated, so its eligibility is judged at the exact
    /// position the pressing client displayed (press-frame own position).
    /// Here the press happens while approaching: out of range at send time,
    /// in range at the pinned frame — the old arrival-time judging would have
    /// rejected `too_far`; press-frame judging harvests.
    #[test]
    fn an_action_is_judged_at_its_press_frame_position_not_at_arrival() {
        let mut sim = Sim::new();
        // Approach the centre tree due east: 4 frames west at 200/tick walk
        // x from 9_431 toward the flanking trees' contact point (~8_831),
        // taking the distance to (8_000,8_000) from 1_431 (out of range) to
        // ~831 (in range).
        sim.connect_at("a", Position { x: 9_431, y: 8_000 }, Inventory::default());
        for seq in 1..=4u32 {
            sim.enqueue_move("a", seq, -1.0, 0.0);
        }
        // The press, pinned to frame 4 — sent before any frame has resolved.
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 4, 0);
        let _ = sim.drain_events();

        // The invariant: an intent is processed *with its tick* — frame 4
        // integrates in tick 4, and the verb pinned to it resolves in tick 4
        // (post-movement), never pushed to tick 5.
        for _ in 0..4 {
            sim.tick();
        }
        let rejected = sim
            .drain_events()
            .into_iter()
            .any(|e| matches!(e, OutboundEvent::ActionRejected { .. }));
        assert!(!rejected, "the press-frame position is in range — no too_far");
        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood),
            Some(&1),
            "the harvest lands in its own tick, judged at the pinned frame's position"
        );
    }

    /// The simultaneity tie-break: movement resolves before verbs within a
    /// tick, so *arrival-into beats placement* — a build into a cell someone
    /// walks into this same tick is judged against final positions and
    /// refused (`footprint_blocked`). The wall can never appear under a body.
    #[test]
    fn same_tick_arrival_into_a_cell_beats_placement() {
        let mut sim = Sim::new();
        // Builder in range of cell (3_500, 3_000); walker one frame east of
        // body-blocking it (contact = half-width 500 + body 300 = 800).
        sim.connect_at("b", Position { x: 2_700, y: 3_000 }, {
            let mut inv = Inventory::default();
            inv.items.insert(Item::Wood, 5);
            inv
        });
        sim.connect_at("a", Position { x: 4_350, y: 3_000 }, Inventory::default());
        sim.enqueue_move("a", 1, -1.0, 0.0); // → 4_150: inside the block band
        sim.enqueue_action(
            "b",
            Action::Build { kind: StructureKind::Wall, x: 3_500, y: 3_000 },
            0,
            0,
        );
        let _ = sim.drain_events();
        sim.tick();

        let reasons: Vec<_> = sim
            .drain_events()
            .into_iter()
            .filter_map(|e| match e {
                OutboundEvent::ActionRejected { reason, .. } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(reasons, vec!["footprint_blocked"], "the walker won the cell");
        assert_eq!(
            sim.inventory_of("b").unwrap().items.get(&Item::Wood),
            Some(&5),
            "no wood was spent on the refused wall"
        );
    }

    /// WireId of the first NPC on the wire.
    fn first_npc_wid(sim: &Sim) -> WireId {
        crate::wire::entity_states(sim.overworld())
            .into_iter()
            .find_map(|(wid, s)| {
                matches!(s, crate::wire::EntityWire::Npc { .. }).then_some(wid)
            })
            .expect("an NPC is on the wire")
    }

    /// Impossible Frontier claims are *worthless* (clamped) but never
    /// *invisible*: each one increments a counter the dev stats surface. An
    /// honest session can produce neither kind — its auth tick is never ahead
    /// of the server's and never regresses across presses (one ordered
    /// stream) — so the count is a pure probe signal.
    #[test]
    fn impossible_frontier_claims_are_counted_not_punished() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        sim.tick(); // tick 1
        let tree = || Action::Harvest { target: WireId("tree:8000:8000".into()) };

        // Honest claims: at or below the present, non-regressing → no count.
        sim.enqueue_action("a", tree(), 0, 0);
        sim.enqueue_action("a", tree(), 0, 1);
        assert_eq!(sim.frontier_violations(), 0, "honest claims are free");

        // A claim from the future: impossible — counted, clamped, play continues.
        sim.enqueue_action("a", tree(), 0, 99);
        assert_eq!(sim.frontier_violations(), 1, "never-future violation counted");

        // A regressing claim after a higher one: impossible — counted too.
        sim.enqueue_action("a", tree(), 0, 0);
        assert_eq!(sim.frontier_violations(), 2, "monotonicity violation counted");

        // And nothing was punished: the player still resolves verbs normally.
        sim.tick();
        assert!(
            sim.inventory_of("a").unwrap().items.get(&crate::components::Item::Wood).is_some(),
            "clamped claims still resolve (worthless, not fatal)"
        );
    }

    /// A Action pinned to an *unreachable* seq — one greater than everything
    /// consumed AND everything still queued — resolves immediately (judged at
    /// the current position, the least-generous frame), even while the player
    /// keeps moving. The reachability rule: a pin no real Input frame can ever
    /// satisfy is not held hostage. This closes the moving-fabricator wedge —
    /// a non-empty queue must not let a fabricated future seq wedge the FIFO.
    #[test]
    fn an_action_pinned_to_an_unreachable_seq_resolves_without_waiting() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        // The player is moving: frames 1,2,3 are queued (pending, not yet
        // consumed). A press fabricates seq 99 — past everything that exists.
        for seq in 1..=3u32 {
            sim.enqueue_move("a", seq, 0.0, 1.0);
        }
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 99, 0);
        let _ = sim.drain_events();

        // One tick: the unreachable pin resolves now (judged at the current
        // position — in range of the centre tree → wood), rather than waiting
        // behind the three real frames.
        sim.tick();
        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood),
            Some(&1),
            "the unreachable pin resolved this tick, not wedged behind the queue"
        );
    }

    /// The counterpart: a Action pinned to a *reachable* pending seq waits for
    /// that frame to integrate (FIFO), even though resolving now would also
    /// succeed — the pin is honored, not short-circuited.
    #[test]
    fn an_action_pinned_to_a_pending_seq_waits_for_it() {
        let mut sim = Sim::new();
        // Stand one frame's travel short of range; the pinned frame closes it.
        sim.connect_at("a", Position { x: 9_150, y: 8_000 }, Inventory::default());
        sim.enqueue_move("a", 1, -1.0, 0.0); // → 8_950: in range at frame 1
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 1, 0);
        let _ = sim.drain_events();
        // Before the tick, frame 1 is pending and seq 1 is reachable → the
        // verb must wait for it (not resolve at the start-of-tick 9_150, which
        // is out of range). One tick integrates frame 1, then the verb lands.
        sim.tick();
        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood),
            Some(&1),
            "the verb waited for its reachable pending frame, then landed in range"
        );
    }

    /// Confluence: a tick's outcome is a pure function of the locked intents —
    /// *when* an intent arrives (relative to other ticks, within its pin
    /// window) is scheduling, never semantics. The same logical inputs, three
    /// different arrival schedules, one bit-identical world.
    #[test]
    fn outcomes_are_invariant_to_intent_arrival_schedule() {
        // The approach scenario from the seq-pinning test, delivered three ways.
        let run = |deliver_verb_after_n_ticks: usize| {
            let mut sim = Sim::new();
            sim.connect_at("a", Position { x: 9_431, y: 8_000 }, Inventory::default());
            for seq in 1..=4u32 {
                sim.enqueue_move("a", seq, -1.0, 0.0);
            }
            let verb = || Action::Harvest { target: WireId("tree:8000:8000".into()) };
            if deliver_verb_after_n_ticks == 0 {
                sim.enqueue_action("a", verb(), 4, 0);
            }
            for t in 1..=6usize {
                if t == deliver_verb_after_n_ticks {
                    sim.enqueue_action("a", verb(), 4, 0);
                }
                sim.tick();
            }
            let _ = sim.drain_events();
            (sim.position("a").unwrap(), sim.inventory_of("a").unwrap().items.clone())
        };
        // Before any frame; after 1 tick; after 3 ticks (frames 1–3 already
        // integrated) — all before the pin's resolve tick.
        let a = run(0);
        let b = run(1);
        let c = run(3);
        assert_eq!(a, b, "arrival schedule is invisible (0 vs 1)");
        assert_eq!(b, c, "arrival schedule is invisible (1 vs 3)");
        assert_eq!(
            a.1.get(&crate::components::Item::Wood),
            Some(&1),
            "and the verb landed"
        );
    }

    /// Lawful-render judging: an entity-directed verb's range eligibility is
    /// judged in the press frame — the target's position as the asserting
    /// client lawfully displayed it (ring state at its Frontier, integrated
    /// forward by the last-known intent) — OR the authoritative present;
    /// either frame in range makes the press eligible. Here: a calm wolf sits
    /// in range; a first hit sends it fleeing out of authoritative range; a
    /// press asserting the pre-flight Frontier (the screen still showed it
    /// close) lands — while a press asserting a fresh Frontier is `too_far`.
    #[test]
    fn a_press_is_judged_against_the_targets_lawful_render() {
        let mut sim = Sim::new();
        // Clear of the tree cluster (y 12_000); wolf 0.9u east — in range,
        // calm (low hunger, players are not wolf threats until provoked).
        sim.connect_at("a", Position { x: 8_000, y: 12_000 }, Inventory::default());
        sim.spawn_npc(
            NpcKind::Wolf,
            Position { x: 8_900, y: 12_000 },
            Drives { hunger: 0.1, ..Default::default() },
        );
        let wolf = first_npc_wid(&sim);
        // Two calm ticks so the ring records the wolf at rest at 8_900.
        sim.tick();
        sim.tick();
        let calm_frontier = sim.tick_count();

        // Provoke: one hit lands (wolf at 8_900, in range), and the wolf flees
        // east at 210/tick.
        sim.enqueue_action("a", Action::Damage { target: wolf.clone() }, 0, calm_frontier);
        sim.tick();
        // Let it run authoritatively out of range (> 1_000 from alice).
        for _ in 0..3 {
            sim.tick();
        }
        let _ = sim.drain_events();

        // A press asserting the *calm* Frontier: the lawful render still has
        // the wolf at 8_900 with zero intent → in range → the hit lands.
        sim.enqueue_action("a", Action::Damage { target: wolf.clone() }, 0, calm_frontier);
        sim.tick();
        assert!(
            !sim.drain_events().iter().any(|e| matches!(e, OutboundEvent::ActionRejected { .. })),
            "the press the screen promised is honored (lawful render in range)"
        );

        // A press asserting a *fresh* Frontier: both frames agree it is gone.
        let fresh = sim.tick_count();
        sim.enqueue_action("a", Action::Damage { target: wolf }, 0, fresh);
        sim.tick();
        let rejected: Vec<_> = sim
            .drain_events()
            .into_iter()
            .filter_map(|e| match e {
                OutboundEvent::ActionRejected { reason, .. } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(rejected, vec!["too_far"], "an honest fresh frame is out of range");
    }

    /// The Frontier's hard checks: never-future (clamped to the present) and
    /// the Lead window (a press cannot reach further back than LEAD_BOUND
    /// ticks). An ancient Frontier is clamped into the window, where the wolf
    /// has already fled — so the stale-forever persona buys nothing.
    #[test]
    fn frontier_claims_are_clamped_to_the_lead_window() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 12_000 }, Inventory::default());
        sim.spawn_npc(
            NpcKind::Wolf,
            Position { x: 8_900, y: 12_000 },
            Drives { hunger: 0.1, ..Default::default() },
        );
        let wolf = first_npc_wid(&sim);
        sim.tick();
        sim.tick();
        let calm_frontier = sim.tick_count();
        sim.enqueue_action("a", Action::Damage { target: wolf.clone() }, 0, calm_frontier);
        sim.tick();
        // Flee until the calm frame falls out of the LEAD_BOUND window.
        for _ in 0..(crate::consts::LEAD_BOUND_TICKS + 2) {
            sim.tick();
        }
        let _ = sim.drain_events();

        // Asserting the (now ancient) calm Frontier: clamped to the window's
        // edge, where the wolf is long gone → too_far. Pretending to be
        // staler than the Lead bound buys nothing.
        sim.enqueue_action("a", Action::Damage { target: wolf }, 0, calm_frontier);
        sim.tick();
        let rejected: Vec<_> = sim
            .drain_events()
            .into_iter()
            .filter_map(|e| match e {
                OutboundEvent::ActionRejected { reason, .. } => Some(reason),
                _ => None,
            })
            .collect();
        assert_eq!(rejected, vec!["too_far"], "the claim is clamped into the Lead window");
    }

    /// A gameplay failure at tick-time (here: out of range) comes back as an
    /// async `ActionRejected` carrying the verb's reason — not a synchronous
    /// error, since resolution happens in the tick.
    #[test]
    fn a_failed_action_is_reported_async_with_its_reason() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        let _ = sim.drain_events(); // clear connect-time events

        // The neighbour chunk's centre tree — real, hydrated, far out of range.
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:24000:8000".into()) }, 0, 0);
        sim.tick();

        let rejected: Vec<_> = sim
            .drain_events()
            .into_iter()
            .filter(|e| matches!(e, OutboundEvent::ActionRejected { .. }))
            .collect();
        assert_eq!(
            rejected,
            vec![OutboundEvent::ActionRejected {
                username: "a".to_string(),
                verb: "harvest",
                at: RejectedAt::Entity(WireId("tree:24000:8000".into())),
                reason: "too_far",
            }],
            "an out-of-range harvest is reported async as too_far"
        );
    }

    /// Action intents resolve *after* movement integration — the simultaneity
    /// law. Same-tick: the mover coasts unobstructed and the build, judged at
    /// final positions, is refused if a body landed in its band (here the
    /// builder's own — a wall can never appear under anyone, the builder
    /// included). Built clear, the wall is solid from the next tick on.
    #[test]
    fn a_build_resolves_after_movement_and_is_solid_from_the_next_tick() {
        // Half 1: moving into your own placement band refuses the build.
        let mut sim = Sim::new();
        let mut inv = Inventory::default();
        inv.items.insert(Item::Wood, crate::consts::WALL_COST);
        sim.connect_at("p", Position { x: 2_600, y: 3_000 }, inv.clone());
        sim.set_intent("p", 1.0, 0.0); // east, into the wall's band
        sim.enqueue_action("p", Action::Build { kind: StructureKind::Wall, x: 3_500, y: 3_000 }, 0, 0);
        let _ = sim.drain_events();
        sim.tick();
        let x = sim.position("p").unwrap().x;
        assert_eq!(x, 2_800, "this tick's movement is unobstructed (no same-tick wall)");
        assert!(
            sim.drain_events().iter().any(|e| matches!(
                e,
                OutboundEvent::ActionRejected { reason: "footprint_blocked", .. }
            )),
            "the build is judged at final positions: the body in the band refuses it"
        );

        // Half 2: built clear (standing still), the wall is solid next tick.
        let mut sim = Sim::new();
        sim.connect_at("p", Position { x: 2_600, y: 3_000 }, inv);
        sim.enqueue_action("p", Action::Build { kind: StructureKind::Wall, x: 3_500, y: 3_000 }, 0, 0);
        sim.tick();
        sim.set_intent("p", 1.0, 0.0);
        for _ in 0..5 {
            sim.tick();
        }
        let x = sim.position("p").unwrap().x;
        assert_eq!(x, 2_700, "from the next tick on, the wall stops movement at contact");
    }

    /// All of an actor's queued actions drain in one tick, FIFO. Two harvests
    /// enqueued before a tick both resolve in that tick.
    #[test]
    fn all_queued_actions_drain_in_one_tick() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 0, 0);
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8500:8500".into()) }, 0, 0);

        sim.tick();

        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood).copied(),
            Some(2),
            "both queued harvests resolve in a single tick"
        );
    }

    /// Under backpressure the tick body is skipped: the world does not advance.
    /// Movement is frozen and the clock is held — "freeze" is literally
    /// skip-the-tick. (n_high=0/n_low=0 ⇒ a permanent freeze for the assertion.)
    #[test]
    fn a_backpressured_tick_freezes_the_world() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        sim.set_intent("a", 1.0, 0.0); // standing velocity east

        sim.set_persist_thresholds(Thresholds { n_high: 0, n_low: 0 });
        assert!(sim.backpressured(), "overload engaged");

        let pos_before = sim.position("a").unwrap();
        let clock_before = sim.clock_ms();
        sim.tick_or_flush().unwrap();

        assert_eq!(sim.position("a").unwrap(), pos_before, "frozen: the player does not move");
        assert_eq!(sim.clock_ms(), clock_before, "frozen: the clock does not advance");
    }

    /// The freeze keeps flushing, so it self-relieves: once the buffer drains
    /// below the low-water mark the mode disengages, and an intent queued before
    /// the freeze survives it and resolves on resume — nothing dropped.
    #[test]
    fn the_freeze_self_relieves_and_queued_intents_resume_intact() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());

        // A resolved harvest leaves pending writes in the buffer (un-flushed:
        // the DB-flush cadence hasn't elapsed) — our overload to drain.
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 0, 0);
        sim.tick();
        assert_eq!(sim.inventory_of("a").unwrap().items.get(&Item::Wood).copied(), Some(1));

        // Queue another intent and start moving, then trip the overload. The
        // harvest above left buffered writes, so lowering the high-water mark
        // engages backpressure.
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8500:8500".into()) }, 0, 0);
        sim.set_intent("a", 1.0, 0.0);
        sim.set_persist_thresholds(Thresholds { n_high: 1, n_low: 1 });
        assert!(sim.backpressured(), "overload engaged");

        let pos_before = sim.position("a").unwrap();

        // Frozen tick: world held, but the flush drains the buffer → disengages.
        sim.tick_or_flush().unwrap();
        assert_eq!(sim.position("a").unwrap(), pos_before, "frozen: no movement");
        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood).copied(),
            Some(1),
            "frozen: the queued harvest did not resolve"
        );
        assert!(!sim.backpressured(), "the flush drained the buffer — the freeze self-relieved");

        // Resume: the surviving queued intent resolves and movement continues.
        sim.tick_or_flush().unwrap();
        assert_eq!(
            sim.inventory_of("a").unwrap().items.get(&Item::Wood).copied(),
            Some(2),
            "the pre-freeze queued harvest resolved intact on resume"
        );
        assert!(sim.position("a").unwrap().x > pos_before.x, "movement resumed");
    }

    /// A panic inside the guarded body must not be swallowed (the runtime is
    /// presumed corrupt) — but durable state is flushed before it propagates, so
    /// the pending write window is emptied on the way down.
    #[test]
    fn guard_flushes_durable_state_then_reports_the_panic() {
        let mut sim = Sim::new();
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 0, 0);
        sim.tick(); // resolve the harvest → a pending durable write
        assert!(sim.datastore.pending_len() > 0, "there is an unflushed durable write");

        // Silence the default panic print for this expected, caught panic.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let res = sim.guard(|_| panic!("injected tick panic"));
        std::panic::set_hook(prev);

        assert!(res.is_err(), "the guard reports the panic rather than swallowing it");
        assert_eq!(
            sim.datastore.pending_len(),
            0,
            "the guard flushed durable state on the way out"
        );
    }

    /// With a pool enabled, `tick_or_flush` must drive the *parallel* tick (so the
    /// movement compute runs on worker threads), and a worker panic must surface
    /// as a lossless crash: durable state flushed, panic reported (not hung).
    #[test]
    fn pooled_tick_or_flush_crashes_losslessly_on_a_worker_panic() {
        use std::sync::atomic::Ordering;
        let mut sim = Sim::new();
        sim.enable_pool(2);
        sim.connect_at("a", Position { x: 8_000, y: 8_000 }, Inventory::default());
        sim.enqueue_action("a", Action::Harvest { target: WireId("tree:8000:8000".into()) }, 0, 0);
        sim.tick(); // resolve the harvest → a pending durable write
        assert!(sim.datastore.pending_len() > 0, "there is an unflushed durable write");

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        crate::parallel::PANIC_IN_RUN_CLUSTER.store(true, Ordering::Relaxed);
        let res = sim.tick_or_flush();
        crate::parallel::PANIC_IN_RUN_CLUSTER.store(false, Ordering::Relaxed);
        std::panic::set_hook(prev);

        assert!(res.is_err(), "a worker panic must surface through the parallel tick, not be lost");
        assert_eq!(sim.datastore.pending_len(), 0, "durable state flushed on the way down");
    }
}
