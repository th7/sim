//! The **Cartographer** — sole, serialized authority over the island partition.
//!
//! It owns the registry (`actor → island`, `chunk → island`) and is the only
//! executor of topology changes: it places unclustered actors, merges islands
//! whose chunk-sets overlap, and splits islands whose chunk-sets disconnect.
//! (Worker assignment / repack lives in [`crate::repack`].)
//!
//! Every mutation funnels through [`Cartographer::reconcile`], which recomputes the
//! partition to its canonical form: two actors share an island iff their 3×3
//! chunk footprints transitively overlap. This is *correct by construction* —
//! see [`Cartographer::reconcile`] for the argument — so the never-under-merge
//! invariant holds after every change, not merely "usually". Island ids are
//! preserved across reconciles where possible (a merge survivor keeps the lower
//! id; a split keeps the id on the largest child) so workers and observers can
//! track islands through topology change.

use crate::chunkgraph::connected_components;
use crate::geometry::ChunkCoord;
use crate::ids::{ActorId, IslandId};
use std::collections::{BTreeMap, BTreeSet};

/// An island as the Cartographer tracks it: a set of member actors and the union of
/// their 3×3 chunk footprints. The chunk-set is derived from member homes and
/// kept in sync by [`Cartographer::reconcile`].
#[derive(Debug, Clone)]
pub struct Island {
    pub id: IslandId,
    pub actors: BTreeSet<ActorId>,
    pub chunk_set: BTreeSet<ChunkCoord>,
}

/// A topology change the Cartographer executed, surfaced for observers and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyEvent {
    /// A new island was minted (placement of an actor with no overlap).
    Created(IslandId),
    /// `retired` was absorbed into `survivor` (which keeps the lower id).
    Merged { survivor: IslandId, retired: IslandId },
    /// `source` split: it kept its id (largest child) and spawned `children`.
    Split { source: IslandId, children: Vec<IslandId> },
    /// An island's last actor left; the island was removed.
    Emptied(IslandId),
}

#[derive(Debug, Default)]
pub struct Cartographer {
    islands: BTreeMap<IslandId, Island>,
    actor_home: BTreeMap<ActorId, ChunkCoord>,
    actor_island: BTreeMap<ActorId, IslandId>,
    chunk_owner: BTreeMap<ChunkCoord, IslandId>,
    next_id: u64,
}

impl Cartographer {
    pub fn new() -> Self {
        Cartographer::default()
    }

    fn mint_id(&mut self) -> IslandId {
        let id = IslandId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Place a freshly-arrived actor at `home`. Creates a singleton island and
    /// reconciles, which merges it into any overlapping island. Returns the
    /// topology events produced.
    pub fn insert_actor(&mut self, actor: ActorId, home: ChunkCoord) -> Vec<TopologyEvent> {
        debug_assert!(!self.actor_home.contains_key(&actor), "actor already present");
        let id = self.mint_id();
        self.actor_home.insert(actor, home);
        let island = Island {
            id,
            actors: BTreeSet::from([actor]),
            chunk_set: home.footprint_3x3().into_iter().collect(),
        };
        self.islands.insert(id, island);
        self.actor_island.insert(actor, id);

        let mut events = vec![TopologyEvent::Created(id)];
        events.extend(self.reconcile());
        events
    }

    /// Update an actor's home chunk (it crossed a chunk boundary) and reconcile.
    /// May trigger a merge (now overlaps another island) and/or a split
    /// (drifted away from its clustermates).
    pub fn move_actor(&mut self, actor: ActorId, new_home: ChunkCoord) -> Vec<TopologyEvent> {
        match self.actor_home.get_mut(&actor) {
            Some(home) => *home = new_home,
            None => return Vec::new(),
        }
        self.reconcile()
    }

    /// Remove an actor (disconnect / death). Reconcile may empty or split its
    /// former island.
    pub fn remove_actor(&mut self, actor: ActorId) -> Vec<TopologyEvent> {
        let Some(iid) = self.actor_island.remove(&actor) else {
            return Vec::new();
        };
        self.actor_home.remove(&actor);
        if let Some(island) = self.islands.get_mut(&iid) {
            island.actors.remove(&actor);
        }
        self.reconcile()
    }

    // --- queries ---

    pub fn island_of(&self, actor: ActorId) -> Option<IslandId> {
        self.actor_island.get(&actor).copied()
    }

    pub fn owner_of_chunk(&self, chunk: ChunkCoord) -> Option<IslandId> {
        self.chunk_owner.get(&chunk).copied()
    }

    pub fn island(&self, id: IslandId) -> Option<&Island> {
        self.islands.get(&id)
    }

    pub fn islands(&self) -> impl Iterator<Item = &Island> {
        self.islands.values()
    }

    pub fn island_count(&self) -> usize {
        self.islands.len()
    }

    pub fn actor_count(&self) -> usize {
        self.actor_home.len()
    }

    /// All chunks currently owned by some island ("hot" chunks).
    pub fn owned_chunks(&self) -> impl Iterator<Item = ChunkCoord> + '_ {
        self.chunk_owner.keys().copied()
    }

    pub fn home_of(&self, actor: ActorId) -> Option<ChunkCoord> {
        self.actor_home.get(&actor).copied()
    }

    // --- reconcile ---

    /// Recompute the partition to canonical form and re-derive the registries.
    ///
    /// Correctness: after this returns, two actors are in the same island iff
    /// their footprints transitively overlap (share a chunk via a chain of
    /// members). The merge pass unions any two islands that co-claim a chunk —
    /// and two islands co-claim a chunk c exactly when each has a member whose
    /// footprint contains c, i.e. those members' footprints overlap, so the
    /// merge is *required*. The split pass separates an island's chunk-set into
    /// its connected components; an actor's footprint (itself a connected 3×3
    /// block) lies wholly within one component, so the actor partition is
    /// well-defined. The two passes therefore produce exactly the connected
    /// components of the footprint-overlap graph.
    fn reconcile(&mut self) -> Vec<TopologyEvent> {
        let mut events = Vec::new();

        // 1. Drop empty islands.
        let empty: Vec<IslandId> = self
            .islands
            .iter()
            .filter(|(_, c)| c.actors.is_empty())
            .map(|(id, _)| *id)
            .collect();
        for id in empty {
            self.islands.remove(&id);
            events.push(TopologyEvent::Emptied(id));
        }

        // 2. Recompute each island's chunk-set from its members' homes.
        for island in self.islands.values_mut() {
            let mut set = BTreeSet::new();
            for actor in &island.actors {
                let home = self.actor_home[actor];
                set.extend(home.footprint_3x3());
            }
            island.chunk_set = set;
        }

        events.extend(self.merge_pass());
        events.extend(self.split_pass());

        // 5. Re-derive registries from the final island set.
        self.actor_island.clear();
        self.chunk_owner.clear();
        for island in self.islands.values() {
            for &actor in &island.actors {
                self.actor_island.insert(actor, island.id);
            }
            for &chunk in &island.chunk_set {
                self.chunk_owner.insert(chunk, island.id);
            }
        }

        events
    }

    /// Merge every pair of islands whose chunk-sets share a chunk, to fixpoint.
    /// Uses union-find over island ids keyed by co-claimed chunks; each merge
    /// group's survivor is its lowest id.
    fn merge_pass(&mut self) -> Vec<TopologyEvent> {
        // Map chunk -> the island ids claiming it, to find conflicts.
        let mut claims: BTreeMap<ChunkCoord, Vec<IslandId>> = BTreeMap::new();
        for island in self.islands.values() {
            for &chunk in &island.chunk_set {
                claims.entry(chunk).or_default().push(island.id);
            }
        }

        // Union-find over island ids.
        let mut uf = UnionFind::new(self.islands.keys().copied());
        for ids in claims.values() {
            for w in ids.windows(2) {
                uf.union(w[0], w[1]);
            }
        }

        // Group islands by their union-find root.
        let mut groups: BTreeMap<IslandId, Vec<IslandId>> = BTreeMap::new();
        for &id in self.islands.keys() {
            groups.entry(uf.find(id)).or_default().push(id);
        }

        let mut events = Vec::new();
        for (_, mut members) in groups {
            if members.len() <= 1 {
                continue;
            }
            members.sort();
            let survivor = members[0];
            for &retired in &members[1..] {
                let absorbed = self.islands.remove(&retired).expect("retired exists");
                let s = self.islands.get_mut(&survivor).expect("survivor exists");
                s.actors.extend(absorbed.actors);
                s.chunk_set.extend(absorbed.chunk_set);
                events.push(TopologyEvent::Merged { survivor, retired });
            }
        }
        events
    }

    /// Split any island whose chunk-set has ≥2 connected components into one
    /// island per component. The largest component keeps the source id.
    fn split_pass(&mut self) -> Vec<TopologyEvent> {
        let ids: Vec<IslandId> = self.islands.keys().copied().collect();
        let mut events = Vec::new();

        for id in ids {
            let components = {
                let island = &self.islands[&id];
                connected_components(&island.chunk_set)
            };
            if components.len() <= 1 {
                continue;
            }

            // Partition actors by which component contains their home chunk.
            let island = self.islands.remove(&id).expect("island exists");
            let mut buckets: Vec<(BTreeSet<ActorId>, BTreeSet<ChunkCoord>)> =
                components.iter().map(|c| (BTreeSet::new(), c.clone())).collect();

            for actor in island.actors {
                let home = self.actor_home[&actor];
                let idx = components
                    .iter()
                    .position(|c| c.contains(&home))
                    .expect("actor home lies in some component");
                buckets[idx].0.insert(actor);
            }

            // Largest bucket (tie: lowest min chunk) keeps the source id.
            let keep_idx = (0..buckets.len())
                .max_by(|&a, &b| {
                    buckets[a]
                        .0
                        .len()
                        .cmp(&buckets[b].0.len())
                        .then_with(|| {
                            // Prefer the smaller min-chunk for a stable tiebreak.
                            buckets[b]
                                .1
                                .iter()
                                .next()
                                .cmp(&buckets[a].1.iter().next())
                        })
                })
                .expect("≥2 buckets");

            let mut children = Vec::new();
            for (i, (actors, chunk_set)) in buckets.into_iter().enumerate() {
                let iid = if i == keep_idx { id } else { self.mint_id() };
                if i != keep_idx {
                    children.push(iid);
                }
                self.islands.insert(iid, Island { id: iid, actors, chunk_set });
            }
            events.push(TopologyEvent::Split { source: id, children });
        }
        events
    }
}

/// Minimal union-find over `IslandId`s for the merge pass.
struct UnionFind {
    parent: BTreeMap<IslandId, IslandId>,
}

impl UnionFind {
    fn new(ids: impl Iterator<Item = IslandId>) -> Self {
        UnionFind {
            parent: ids.map(|id| (id, id)).collect(),
        }
    }

    fn find(&mut self, id: IslandId) -> IslandId {
        let p = self.parent[&id];
        if p == id {
            id
        } else {
            let root = self.find(p);
            self.parent.insert(id, root);
            root
        }
    }

    fn union(&mut self, a: IslandId, b: IslandId) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            // Point the larger root at the smaller so the lowest id stays root.
            let (root, child) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent.insert(child, root);
        }
    }
}

/// The canonical partition oracle: connected components of the footprint-overlap
/// graph over `(actor, home)` pairs. Used by tests to check the Cartographer always
/// reaches canonical form. Two actors are linked iff their 3×3 footprints share
/// a chunk.
pub fn canonical_partition(homes: &BTreeMap<ActorId, ChunkCoord>) -> Vec<BTreeSet<ActorId>> {
    let actors: Vec<ActorId> = homes.keys().copied().collect();
    let mut uf: BTreeMap<ActorId, ActorId> = actors.iter().map(|&a| (a, a)).collect();

    fn find(uf: &mut BTreeMap<ActorId, ActorId>, a: ActorId) -> ActorId {
        let p = uf[&a];
        if p == a {
            a
        } else {
            let r = find(uf, p);
            uf.insert(a, r);
            r
        }
    }

    for i in 0..actors.len() {
        for j in (i + 1)..actors.len() {
            let fa: BTreeSet<ChunkCoord> = homes[&actors[i]].footprint_3x3().into_iter().collect();
            let fb: BTreeSet<ChunkCoord> = homes[&actors[j]].footprint_3x3().into_iter().collect();
            if crate::chunkgraph::intersects(&fa, &fb) {
                let (ra, rb) = (find(&mut uf, actors[i]), find(&mut uf, actors[j]));
                if ra != rb {
                    uf.insert(ra.max(rb), ra.min(rb));
                }
            }
        }
    }

    let mut groups: BTreeMap<ActorId, BTreeSet<ActorId>> = BTreeMap::new();
    for &a in &actors {
        let r = find(&mut uf, a);
        groups.entry(r).or_default().insert(a);
    }
    groups.into_values().collect()
}
