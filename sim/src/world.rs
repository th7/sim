//! A single realm's simulation state: a `hecs::World` of entities, the
//! [`Labeler`] partitioning its dynamic actors into clusters, and the tick that
//! advances it. The Overworld is one `RealmWorld`; each Instance is another.
//!
//! The tick (single-threaded in Phase 1) is: for each cluster in id order,
//! integrate its players' movement against the static footprints in the
//! cluster's chunks; then detect chunk crossings and let the Labeler reconcile
//! the partition (merge/split); then respawn due resource nodes. Chunk static
//! content is hydrated lazily when a chunk first becomes owned by a cluster.

use crate::catalogue::{
    carcass_meat as npc_carcass_meat, npc_max_hp, resource_footprint, resource_yield,
    structure_footprint,
};
use crate::collision::Obstacle;
use simcore::motion::{intent_velocity, step_actor};
use crate::components::*;
use crate::consts::{DAMAGE_PER_CLICK, IDLE_TIMEOUT_MS, RESPAWN_MS};
use crate::datastore::{DepletionRecord, PersistEvent, PlayerRecord, StructureRecord};
use crate::ecosystem;
use crate::geometry::{coord_for, ChunkCoord};
use crate::ids::{ActorId, ClusterId, Realm};
use crate::labeler::{Labeler, TopologyEvent};
use crate::motivation::{decide, Decision, Drives, NpcKind, Params, Perception, Sensed, P2};
use crate::verbs::VerbError;
use crate::worldgen;
use protocol::wire::ChunkLifecycle;
use hecs::{Entity, World};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Optional bounding rectangle (sub-units) the movement integrator clamps to.
/// Instances are bounded to their 3×3 grid; the Overworld is unbounded.
pub type Bounds = (i64, i64, i64, i64);

/// Damage one NPC `attack` deals.
const NPC_ATTACK_DAMAGE: i64 = 10;
/// Melee range² for an NPC attack / eat (≈0.7 unit).
const NPC_ATTACK_RANGE_SQ: i64 = 700 * 700;
/// NPC action (attack/eat) cooldown.
const NPC_ACT_COOLDOWN_MS: u64 = 500;
/// How long a Carcass lasts before rotting away.
const CARCASS_PERISH_MS: u64 = 60_000;
/// Hunger removed per unit of meat eaten.
const EAT_FEED: f64 = 0.4;
/// Hunger removed per graze action. Small nibbles: with the graze-bout
/// hysteresis (`Drives::grazing`) a bout runs from the hunger threshold down
/// to sated over several actions, so each graze is a sustained stretch and the
/// deer wanders between bouts as metabolism lifts hunger back.
const GRAZE_FEED: f64 = 0.02;
/// How long a fleeing deer stays alarmed (panic contagion window).
const ALARM_MS: u64 = 1_200;

/// What an NPC's perception classifies a nearby being as.
#[derive(Clone, Copy)]
enum Sensable {
    Npc(NpcKind),
    Player,
}

fn dist_sq(a: P2, b: P2) -> i64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

/// Convert a [`Decision`] into a movement-Intent velocity (sub-units/sec).
fn velocity_for(d: Decision, from: P2, speed: f64, seed_id: u64, clock_ms: u64) -> (f64, f64) {
    match d {
        Decision::Idle | Decision::Eat(..) | Decision::Graze => (0.0, 0.0),
        Decision::Wander => {
            // Seeded, sim-clock-bucketed so the drift direction is deterministic
            // and changes ~once a second rather than jittering every tick.
            let bucket = clock_ms / 1_000;
            let seed = seed_id.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(bucket);
            let (dx, dy) = crate::harness::Rng::new(seed).intent();
            (dx * speed, dy * speed)
        }
        Decision::Approach(p) | Decision::Attack(_, p) => unit_toward(from, p, speed),
        Decision::Flee(p) => unit_toward(p, from, speed), // away from the threat
    }
}

fn unit_toward(from: P2, to: P2, speed: f64) -> (f64, f64) {
    let dx = (to.x - from.x) as f64;
    let dy = (to.y - from.y) as f64;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        (0.0, 0.0)
    } else {
        (dx / len * speed, dy / len * speed)
    }
}

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
    /// NPC deaths this step `(chunk, kind)` — drained by the Sim to deplete the
    /// Region's wildlife Disturbance. Only *deaths* deplete; dissolve does not.
    wild_kills: Vec<(ChunkCoord, NpcKind)>,
    /// The lawful-render judging ring: end-of-tick NPC positions + intents for
    /// the last LEAD_BOUND ticks, keyed by the tick index. What a session with
    /// Frontier F lawfully displayed is reconstructible from the entry at F —
    /// position integrated forward by the recorded intent (the same rule the
    /// client's Mirror runs). Bounded; nothing static needs history.
    npc_history: VecDeque<(u64, BTreeMap<WireId, (i64, i64, f64, f64)>)>,
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
            wild_kills: Vec::new(),
            npc_history: VecDeque::new(),
        }
    }

    /// Drain NPC deaths recorded this step (for the Sim's wildlife Disturbance).
    pub fn take_wild_kills(&mut self) -> Vec<(ChunkCoord, NpcKind)> {
        std::mem::take(&mut self.wild_kills)
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

    /// Spawn an NPC at `pos` with initial [`Drives`], registering it as a Labeler
    /// actor (so it joins clusters and is simulated like a Player). Unlike a
    /// Player it does not extend the Warm set: we do not hydrate on its behalf —
    /// NPCs live inside Player-hot chunks.
    pub fn spawn_npc(&mut self, kind: NpcKind, pos: Position, drives: Drives) -> Entity {
        let actor = ActorId(self.next_actor);
        self.next_actor += 1;
        let max = npc_max_hp(kind);
        let wid = WireId(format!("npc:{}:{}", kind.as_str(), actor.0));
        let e = self.world.spawn((
            pos,
            Velocity { vx: 0.0, vy: 0.0 },
            Renderable,
            Npc { kind, actor },
            Health { hp: max, max },
            drives,
            Inventory::default(),
            wid.clone(),
        ));
        self.actor_index.insert(actor, e);
        self.wire_index.insert(wid, e);
        let _ = self.labeler.insert_actor(actor, pos.chunk());
        e
    }

    /// Despawn an NPC entity, deregistering its actor from the Labeler and the
    /// indices (used by the warm/cold boundary and chunk unload).
    pub fn despawn_npc(&mut self, e: Entity) {
        if let Ok(npc) = self.world.get::<&Npc>(e).map(|n| *n) {
            self.actor_index.remove(&npc.actor);
            self.labeler.remove_actor(npc.actor);
        }
        if let Ok(wid) = self.world.get::<&WireId>(e).map(|w| w.clone()) {
            self.wire_index.remove(&wid);
        }
        let _ = self.world.despawn(e);
    }

    /// Snapshot of every NPC in this realm: kind, position, drives, health.
    pub fn npcs(&self) -> Vec<(Entity, NpcKind, Position, Drives, Health)> {
        self.world
            .query::<(&Npc, &Position, &Drives, &Health)>()
            .iter()
            .map(|(e, (n, p, d, h))| (e, n.kind, *p, *d, *h))
            .collect()
    }

    /// The Player Warm set: every chunk within a 3×3 footprint of some Player.
    /// NPCs do not contribute — this is what keeps wildlife alive.
    pub fn player_warm_chunks(&self) -> BTreeSet<ChunkCoord> {
        let mut s = BTreeSet::new();
        for (_, (pos, _)) in self.world.query::<(&Position, &PlayerControlled)>().iter() {
            for c in pos.chunk().footprint_3x3() {
                s.insert(c);
            }
        }
        s
    }

    /// Despawn every NPC standing in chunk `coord` (dissolve on cooldown).
    pub fn despawn_npcs_in(&mut self, coord: ChunkCoord) {
        let es: Vec<Entity> = self
            .world
            .query::<(&Position, &Npc)>()
            .iter()
            .filter(|(_, (p, _))| p.chunk() == coord)
            .map(|(e, _)| e)
            .collect();
        for e in es {
            self.despawn_npc(e);
        }
    }

    /// The **Motivation** pre-movement phase: for each NPC, build
    /// its cluster-local [`Perception`], run [`decide`], and write the resulting
    /// movement Intent into its `Velocity`. Runs serially before the movement
    /// integrator, exactly where a Player's session writes intent. Deterministic.
    pub fn drive_npcs(&mut self, dt_ms: u64, clock_ms: u64) {
        let dt_s = dt_ms as f64 / 1000.0;

        // Snapshot every sensable being (NPCs + Players) once, by id/pos/kind.
        let mut beings: Vec<(u64, P2, Sensable)> = Vec::new();
        for (_, (pos, npc)) in self.world.query::<(&Position, &Npc)>().iter() {
            beings.push((npc.actor.0, P2::new(pos.x, pos.y), Sensable::Npc(npc.kind)));
        }
        for (_, (pos, pc)) in self.world.query::<(&Position, &PlayerControlled)>().iter() {
            beings.push((pc.actor.0, P2::new(pos.x, pos.y), Sensable::Player));
        }

        // Carcasses are edible food the wolves can sense.
        let carcasses: Vec<P2> = self
            .world
            .query::<(&Position, &Carcass)>()
            .iter()
            .map(|(_, (p, _))| P2::new(p.x, p.y))
            .collect();

        // Currently-alarmed deer, whose panic is contagious (agent extension).
        let alarmed: Vec<(u64, P2)> = self
            .world
            .query::<(&Position, &Npc, &Alarmed)>()
            .iter()
            .filter(|(_, (_, n, a))| n.kind == NpcKind::Deer && a.until_ms > clock_ms)
            .map(|(_, (p, n, _))| (n.actor.0, P2::new(p.x, p.y)))
            .collect();

        // The NPCs to drive this tick, with health + recent-damage memory.
        let npcs: Vec<(Entity, NpcKind, u64, Position, Drives, Health, Option<Hurt>)> = self
            .world
            .query::<(&Npc, &Position, &Drives, &Health, Option<&Hurt>)>()
            .iter()
            .map(|(e, (npc, pos, d, h, hurt))| {
                (e, npc.kind, npc.actor.0, *pos, *d, *h, hurt.copied())
            })
            .collect();

        let recent = (4 * dt_ms).max(1);
        let phase = ecosystem::day_phase(clock_ms);
        for (e, kind, self_id, pos, drives, health, hurt) in npcs {
            let self_p = P2::new(pos.x, pos.y);
            let params = Params::for_kind(kind);
            let mut perc = Perception::at(self_p);
            perc.phase = phase;
            perc.self_hp_frac = if health.max > 0 {
                health.hp as f64 / health.max as f64
            } else {
                0.0
            };

            for &(id, bp, what) in &beings {
                if id == self_id {
                    continue;
                }
                let d = dist_sq(self_p, bp);
                let near = d <= params.perception_range_sq;
                let social = d <= params.social_range_sq;
                match (kind, what) {
                    (NpcKind::Wolf, Sensable::Npc(NpcKind::Deer)) if near => {
                        perc.prey.push(Sensed { id, pos: bp })
                    }
                    // Rival/pack wolves: contest carcasses (near) and pack-hunt (social).
                    (NpcKind::Wolf, Sensable::Npc(NpcKind::Wolf)) => {
                        if near {
                            perc.rivals.push(Sensed { id, pos: bp });
                        }
                        if social {
                            perc.herd.push(Sensed { id, pos: bp });
                        }
                    }
                    (NpcKind::Deer, Sensable::Npc(NpcKind::Wolf)) if near => {
                        perc.threats.push(Sensed { id, pos: bp })
                    }
                    (NpcKind::Deer, Sensable::Player) if near => {
                        perc.threats.push(Sensed { id, pos: bp })
                    }
                    // Same-species peers form the herd (agent extension, wider sense).
                    (NpcKind::Deer, Sensable::Npc(NpcKind::Deer)) if social => {
                        perc.herd.push(Sensed { id, pos: bp })
                    }
                    _ => {}
                }
            }

            // Recent damage: spike safety and treat the attacker as a threat.
            if let Some(h) = hurt {
                if clock_ms.saturating_sub(h.last_ms) <= recent {
                    perc.being_attacked = true;
                    if let Some(&(_, bp, _)) = beings.iter().find(|(id, _, _)| *id == h.by) {
                        perc.threats.push(Sensed { id: h.by, pos: bp });
                    }
                }
            }

            if kind == NpcKind::Wolf {
                for &cp in &carcasses {
                    if dist_sq(self_p, cp) <= params.perception_range_sq {
                        perc.food.push(Sensed { id: 0, pos: cp });
                    }
                }
            }
            if kind == NpcKind::Deer {
                let region = ecosystem::region(self_p.x, self_p.y);
                perc.grass =
                    ecosystem::levels(region, clock_ms, &ecosystem::Disturbance::default()).grass;
                for &(id, ap) in &alarmed {
                    if id != self_id && dist_sq(self_p, ap) <= params.social_range_sq {
                        perc.alarmed.push(Sensed { id, pos: ap });
                    }
                }
            }

            let mut next = drives;
            let decision = decide(kind, &perc, &mut next, &params, dt_s);
            // An unhurried animal doesn't run: Calm Decisions move at the
            // kind's amble, urgent ones at full speed.
            let speed = if decision.demeanor() == protocol::types::Demeanor::Calm {
                params.calm_speed
            } else {
                params.speed
            };
            let (vx, vy) = velocity_for(decision, self_p, speed, self_id, clock_ms);

            if let Ok(mut d) = self.world.get::<&mut Drives>(e) {
                *d = next;
            }
            if let Ok(mut v) = self.world.get::<&mut Velocity>(e) {
                v.vx = vx;
                v.vy = vy;
            }
            // A fleeing deer becomes alarmed, propagating the panic next tick.
            if kind == NpcKind::Deer && matches!(decision, Decision::Flee(_)) {
                let _ = self.world.insert_one(e, Alarmed { until_ms: clock_ms + ALARM_MS });
            }
            let _ = self.world.insert_one(e, NpcDecision(decision));
        }
    }

    /// Post-movement resolution of NPC verbs: apply `attack`
    /// damage to in-range targets and `eat` drain from in-range carcasses, each
    /// gated by the actor's cooldown. Players take no damage (no `Health`) —
    /// structural invulnerability. Deaths become Carcasses.
    pub fn resolve_npc_actions(&mut self, clock_ms: u64) {
        let acts: Vec<(Entity, u64, Position, Decision, u64)> = self
            .world
            .query::<(&Npc, &Position, &NpcDecision, Option<&ActReady>)>()
            .iter()
            .map(|(e, (npc, pos, dec, ready))| {
                (e, npc.actor.0, *pos, dec.0, ready.map(|r| r.at_ms).unwrap_or(0))
            })
            .collect();

        let carcasses: Vec<(Entity, P2, i64)> = self
            .world
            .query::<(&Position, &Carcass)>()
            .iter()
            .map(|(e, (p, c))| (e, P2::new(p.x, p.y), c.meat))
            .collect();

        let mut damage: Vec<(Entity, i64, u64)> = Vec::new(); // (target, dmg, attacker id)
        let mut eats: Vec<(Entity, Entity)> = Vec::new(); // (npc, carcass)
        let mut cooldowns: Vec<(Entity, u64)> = Vec::new();

        for (e, self_id, pos, decision, ready) in acts {
            if clock_ms < ready {
                continue;
            }
            let self_p = P2::new(pos.x, pos.y);
            match decision {
                Decision::Attack(target_id, _) => {
                    let Some(&te) = self.actor_index.get(&ActorId(target_id)) else { continue };
                    let Ok(tp) = self.world.get::<&Position>(te).map(|p| *p) else { continue };
                    if dist_sq(self_p, P2::new(tp.x, tp.y)) <= NPC_ATTACK_RANGE_SQ {
                        damage.push((te, NPC_ATTACK_DAMAGE, self_id));
                        cooldowns.push((e, clock_ms + NPC_ACT_COOLDOWN_MS));
                    }
                }
                Decision::Eat(..) => {
                    if let Some(&(ce, _, _)) = carcasses
                        .iter()
                        .filter(|(_, cp, _)| dist_sq(self_p, *cp) <= NPC_ATTACK_RANGE_SQ)
                        .min_by_key(|(_, cp, _)| dist_sq(self_p, *cp))
                    {
                        eats.push((e, ce));
                        cooldowns.push((e, clock_ms + NPC_ACT_COOLDOWN_MS));
                    }
                }
                Decision::Graze => {
                    // Grazing actually sates: a nibble per action, on the same
                    // cooldown cadence as eating. (Grass is the computed
                    // ecosystem level — grazing doesn't deplete it.)
                    if let Ok(mut d) = self.world.get::<&mut Drives>(e) {
                        d.feed(GRAZE_FEED);
                    }
                    cooldowns.push((e, clock_ms + NPC_ACT_COOLDOWN_MS));
                }
                _ => {}
            }
        }

        // Apply damage; record deaths.
        let mut deaths: Vec<Entity> = Vec::new();
        for (te, dmg, by) in damage {
            let mut killed = false;
            if let Ok(mut h) = self.world.get::<&mut Health>(te) {
                h.hp -= dmg;
                killed = h.hp <= 0;
            }
            let _ = self.world.insert_one(te, Hurt { last_ms: clock_ms, by });
            if killed {
                deaths.push(te);
            }
        }

        // Apply eats: drain a carcass, feed the eater.
        for (npc, carc) in eats {
            let mut exhausted = false;
            if let Ok(mut c) = self.world.get::<&mut Carcass>(carc) {
                c.meat -= 1;
                exhausted = c.meat <= 0;
            }
            if let Ok(mut d) = self.world.get::<&mut Drives>(npc) {
                d.feed(EAT_FEED);
            }
            if exhausted {
                self.despawn_wire_entity(carc);
            }
        }

        for (e, at) in cooldowns {
            let _ = self.world.insert_one(e, ActReady { at_ms: at });
        }
        for e in deaths {
            self.kill_npc(e, clock_ms);
        }
    }

    /// Turn a dying NPC into a Carcass at its position, and record the death so
    /// the Region's wildlife Disturbance can be depleted (event-sourced — only
    /// real deaths deplete; materialize/dissolve/wander are population-neutral).
    fn kill_npc(&mut self, e: Entity, clock_ms: u64) {
        let pos = self.world.get::<&Position>(e).map(|p| *p).ok();
        let kind = self.world.get::<&Npc>(e).map(|n| n.kind).ok();
        self.despawn_npc(e);
        if let (Some(pos), Some(kind)) = (pos, kind) {
            self.wild_kills.push((pos.chunk(), kind));
            let id = self.next_actor;
            self.next_actor += 1;
            let wid = WireId(format!("carcass:{id}"));
            let ce = self.world.spawn((
                pos,
                Renderable,
                Carcass { meat: npc_carcass_meat(kind), perish_at_ms: clock_ms + CARCASS_PERISH_MS },
                wid.clone(),
            ));
            self.wire_index.insert(wid, ce);
        }
    }

    /// Remove carcasses that have rotted away (past their perish time).
    fn expire_carcasses(&mut self, clock_ms: u64) {
        let gone: Vec<Entity> = self
            .world
            .query::<&Carcass>()
            .iter()
            .filter(|(_, c)| clock_ms >= c.perish_at_ms)
            .map(|(e, _)| e)
            .collect();
        for e in gone {
            self.despawn_wire_entity(e);
        }
    }

    /// Despawn an entity that carries a WireId (carcass / static content),
    /// keeping the wire index consistent.
    fn despawn_wire_entity(&mut self, e: Entity) {
        if let Ok(wid) = self.world.get::<&WireId>(e).map(|w| w.clone()) {
            self.wire_index.remove(&wid);
        }
        let _ = self.world.despawn(e);
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
    /// [-1, 1]); scaled by the shared [`simcore::motion::intent_velocity`].
    /// No-op if the player is absent.
    pub fn set_intent(&mut self, username: &str, dx: f64, dy: f64) {
        if let Some(&e) = self.username_index.get(username) {
            if let Ok(mut v) = self.world.get::<&mut Velocity>(e) {
                let (vx, vy) = intent_velocity(dx, dy);
                v.vx = vx;
                v.vy = vy;
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
        // Motivation writes NPC Intent before the movement integrator.
        self.drive_npcs(dt_ms, clock_ms);
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
                let (nx, ny) = step_actor(pos.x, pos.y, vel.vx, vel.vy, dt, &obstacles);
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
        // NPC verbs (attack/eat) resolve here so it covers both tick paths.
        self.resolve_npc_actions(clock_ms);
        self.expire_carcasses(clock_ms);
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
        self.record_npc_history(clock_ms);
        events
    }

    /// Record this tick's end-of-tick NPC state into the lawful-render ring —
    /// the same facts a snapshot of this tick broadcasts, which is exactly
    /// what a session asserting this tick as its Frontier displayed.
    fn record_npc_history(&mut self, clock_ms: u64) {
        let tick = clock_ms / crate::consts::TICK_MS;
        let mut entry = BTreeMap::new();
        for (_e, (pos, vel, wid, _)) in
            self.world.query::<(&Position, &Velocity, &WireId, &Npc)>().iter()
        {
            entry.insert(wid.clone(), (pos.x, pos.y, vel.vx, vel.vy));
        }
        self.npc_history.push_back((tick, entry));
        while self.npc_history.len() > crate::consts::LEAD_BOUND_TICKS as usize + 2 {
            self.npc_history.pop_front();
        }
    }

    /// The position of NPC `wid` as a session with Frontier `frontier` lawfully
    /// displayed it at `resolve_tick`: the ring state at the Frontier (clamped
    /// into the recorded window) integrated forward by its recorded intent —
    /// the Mirror's own speculation rule, recomputed from authoritative data.
    /// `None` when the ring has no entry for the NPC (newly spawned, or no
    /// history yet) — the caller falls back to the authoritative present.
    fn lawful_npc_pos(&self, wid: &WireId, frontier: u64, resolve_tick: u64) -> Option<(i64, i64)> {
        let (tick, map) = self
            .npc_history
            .iter()
            .filter(|(t, _)| *t <= frontier)
            .next_back()
            .or_else(|| self.npc_history.front())?;
        let &(x, y, vx, vy) = map.get(wid)?;
        let lead_s = resolve_tick.saturating_sub(*tick) as f64
            * (crate::consts::TICK_MS as f64 / 1000.0);
        Some((x + (vx * lead_s) as i64, y + (vy * lead_s) as i64))
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
        let to_despawn: Vec<(Entity, WireId, Option<ActorId>)> = self
            .world
            .query::<(&Position, &WireId, Option<&PlayerControlled>, Option<&Npc>)>()
            .iter()
            .filter(|(_, (p, _, player, _))| p.chunk() == coord && player.is_none())
            .map(|(e, (_, wid, _, npc))| (e, wid.clone(), npc.map(|n| n.actor)))
            .collect();
        for (e, wid, actor) in to_despawn {
            self.wire_index.remove(&wid);
            if let Some(a) = actor {
                self.actor_index.remove(&a);
                self.labeler.remove_actor(a);
            }
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

    /// Dev-overlay lifecycle of `coord` at `now_ms`: `Hot` if a cluster owns it,
    /// `IdleArmed` (with ms left until unload) if loaded but unowned, `Cold` if
    /// not loaded. The `IdleArmed` window is the [`IDLE_TIMEOUT_MS`] grace before
    /// [`Self::deactivate_idle_chunks`] despawns the chunk's static content.
    pub fn chunk_lifecycle(&self, coord: ChunkCoord, now_ms: u64) -> (ChunkLifecycle, Option<i64>) {
        if self.is_chunk_hot(coord) {
            (ChunkLifecycle::Hot, None)
        } else if self.loaded.contains(&coord) {
            let last = self.chunk_last_owned.get(&coord).copied().unwrap_or(now_ms);
            let remaining = IDLE_TIMEOUT_MS as i64 - now_ms.saturating_sub(last) as i64;
            (ChunkLifecycle::IdleArmed, Some(remaining.max(0)))
        } else {
            (ChunkLifecycle::Cold, None)
        }
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

    /// Harvest the Gatherable named by `target` — a Resource node (deplete,
    /// respawn after [`RESPAWN_MS`]) or a Carcass (consume). Entity-directed:
    /// the Verb acts on the Target's identity, judged here against authoritative
    /// position. Returns the new inventory on success. Check order: no_player →
    /// no_target → too_far → no_target/depleted.
    pub fn harvest(
        &mut self,
        username: &str,
        target: &WireId,
        clock_ms: u64,
    ) -> Result<Inventory, VerbError> {
        let (px, py) = self.position_of(username).map(|p| (p.x, p.y)).ok_or(VerbError::NoPlayer)?;
        let node = self.wire_index.get(target).copied().ok_or(VerbError::NoTarget)?;
        let (tx, ty) = self
            .world
            .get::<&Position>(node)
            .map(|p| (p.x, p.y))
            .map_err(|_| VerbError::NoTarget)?;
        if !in_range(px, py, tx, ty) {
            return Err(VerbError::TooFar);
        }
        if self.world.get::<&Carcass>(node).is_ok() {
            return self.harvest_carcass(username, node);
        }

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

    /// Harvest the targeted Carcass into meat + hide Items. Reuses the harvest
    /// verb's range gate (already applied by the caller).
    fn harvest_carcass(&mut self, username: &str, ce: Entity) -> Result<Inventory, VerbError> {
        let meat = self.world.get::<&Carcass>(ce).map(|c| c.meat).unwrap_or(0);
        let player_e = *self.username_index.get(username).ok_or(VerbError::NoPlayer)?;
        {
            let mut inv = self.world.get::<&mut Inventory>(player_e).map_err(|_| VerbError::NoPlayer)?;
            if meat > 0 {
                *inv.items.entry(Item::Meat).or_insert(0) += meat as u32;
            }
            *inv.items.entry(Item::Hide).or_insert(0) += 1;
        }
        self.despawn_wire_entity(ce);
        if let Some(ev) = self.player_upsert(username) {
            self.emit(ev);
        }
        Ok(self.inventory_of(username).unwrap_or_default())
    }

    /// Build a structure of `kind` at `(x, y)`. Check order: no_build_in_instance
    /// → out_of_chunk → footprint_blocked → no_player → insufficient_materials.
    /// Returns the new inventory.
    pub fn build(
        &mut self,
        username: &str,
        kind: StructureKind,
        x: i64,
        y: i64,
    ) -> Result<Inventory, VerbError> {
        // Structures are an Overworld-only affordance; Instances are ephemeral.
        if !self.realm.is_overworld() {
            return Err(VerbError::NoBuildInInstance);
        }
        // Build cell must be in the player's current chunk, and in reach — the
        // Island judges range for every verb; the client's gate is only a hint.
        let player_pos = self.position_of(username).ok_or(VerbError::NoPlayer)?;
        if coord_for(x, y) != player_pos.chunk() {
            return Err(VerbError::OutOfChunk);
        }
        if !in_range(player_pos.x, player_pos.y, x, y) {
            return Err(VerbError::TooFar);
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

    /// Damage the Structure or NPC named by `target` by [`DAMAGE_PER_CLICK`].
    /// Entity-directed: the Verb acts on the Target's identity — naming a deer
    /// hits *that* deer, however it has moved, never a nearer one. Range
    /// eligibility for a moving target is judged in the **press frame**: the
    /// target's lawful render at the asserting session's `frontier` *or* its
    /// authoritative present — either in range makes the press eligible (the
    /// screen's promise is honored; so is a lunge the screen hasn't shown
    /// yet). The forgiveness is continuous-only: liveness is always judged
    /// now, and effects land now. Destroys/kills at ≤0 HP. Players are
    /// invulnerable (not targetable). Check order: no_player → no_target →
    /// too_far. Returns the target's remaining HP (`None` if destroyed).
    pub fn damage(
        &mut self,
        username: &str,
        target: &WireId,
        clock_ms: u64,
        frontier: u64,
    ) -> Result<Option<i64>, VerbError> {
        let (px, py) = self.position_of(username).map(|p| (p.x, p.y)).ok_or(VerbError::NoPlayer)?;
        let e = self.wire_index.get(target).copied().ok_or(VerbError::NoTarget)?;
        let (x, y) = self
            .world
            .get::<&Position>(e)
            .map(|p| (p.x, p.y))
            .map_err(|_| VerbError::NoTarget)?;
        let now_in_range = in_range(px, py, x, y);
        let lawful_in_range = || {
            let resolve_tick = clock_ms / crate::consts::TICK_MS;
            self.lawful_npc_pos(target, frontier, resolve_tick)
                .is_some_and(|(lx, ly)| in_range(px, py, lx, ly))
        };
        if !now_in_range && !lawful_in_range() {
            return Err(VerbError::TooFar);
        }
        if self.world.get::<&Structure>(e).is_ok() {
            let new_hp = self.world.get::<&Structure>(e).map(|s| s.hp).unwrap_or(0) - DAMAGE_PER_CLICK;
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
                return Ok(Some(new_hp));
            }
            self.despawn_wire(target);
            self.emit(PersistEvent::DeleteStructure { x, y });
            return Ok(None);
        }

        // An NPC: the player extends the same damage verb to wildlife.
        if self.world.get::<&Npc>(e).is_err() {
            return Err(VerbError::NoTarget);
        }
        let by = self
            .username_index
            .get(username)
            .and_then(|&pe| self.world.get::<&PlayerControlled>(pe).ok().map(|p| p.actor.0))
            .unwrap_or(0);
        let dead = {
            let mut h = self.world.get::<&mut Health>(e).map_err(|_| VerbError::NoTarget)?;
            h.hp -= DAMAGE_PER_CLICK;
            h.hp <= 0
        };
        let _ = self.world.insert_one(e, Hurt { last_ms: clock_ms, by });
        if dead {
            self.kill_npc(e, clock_ms);
            return Ok(None);
        }
        let hp = self.world.get::<&Health>(e).map(|h| h.hp).unwrap_or(0);
        Ok(Some(hp))
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
