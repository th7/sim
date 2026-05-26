# IDEA: interaction-clustered simulation — Rust shared-memory prototype

**Status: exploratory.** This is the *Rust shared-memory* branch of the design conversation. It
consciously **diverges from the committed BEAM design in [ADR-0001](./docs/adr/0001-islands-and-cartographer.md)**
(process-per-Island, message handoffs). Goal here: a **proof-of-concept that the core simulation
model holds** before deciding whether to pursue a Rust path at all. Nothing here supersedes
ADR-0001 yet — it's a parallel investigation.

Glossary note: the **Labeler** is the **Cartographer** in its shared-memory form — it *relabels a
partition* rather than brokering process handoffs.

## The model in brief

One shared ECS world. Dynamic simulation is partitioned by *interaction locality* into **clusters**;
clusters are packed onto **workers** (threads) for execution; a single **Labeler** owns the partition
and the topology changes. Actions run inside a cluster (single authority → every interaction resolved
in one place); observation is a separate changed-only delta stream.

### The simplification that makes the POC tractable: chunk-granular membership

An actor's interaction footprint is its **chunk + the surrounding ring** (3×3). From that, everything
discrete:

- A **cluster owns a set of chunks** (the union of its actors' 3×3 footprints) plus the actors in them.
- **Membership / merge:** two clusters whose chunk-sets touch (share or border a chunk) **must merge** —
  detected as a chunk claimed by two clusters. No continuous union-find; just chunk-graph overlap.
- **Split (`should_split?`):** a cluster whose chunk-set has **≥2 disconnected components** can split into
  them. The cluster computes this itself over chunk adjacency.

**Soundness rests on one inequality: `interaction_range ≤ chunk_size`.** Given that, two actors can only
interact when within ~1 chunk, while clusters merge when within ~2 chunks (their rings overlap) — a full
chunk of margin. So *any two actors that can interact already share a cluster, by construction.* (Holds
because interactions are "very local, no long range.")

## Components

**Actor** — rows in the ECS (Position, Velocity, …). Carries a **per-tick intent** slot (movement /
action input), written from outside, read by its cluster at tick start. Interaction footprint =
its chunk + surrounding ring.

**Cluster** — owns a set of actors + the chunks they span. Each tick: drain intents → integrate
movement → collide against **static footprints in its chunks** → resolve intra-cluster interactions.
Outputs three things the Labeler consumes:
- **boundary** = its chunk-set (for merge detection),
- **tick-time** = EWMA of wall time per tick (for repack),
- **`should_split?`** = chunk-set connected-components (proposed partition, or "indivisible" single component).
Also emits **changed-only deltas** (the entities it touched this tick) to the read-model.

**Worker** — a thread that ticks its assigned clusters. (Phase 1: a single worker, sequential — no
`unsafe`. Phase 2: many workers with disjoint `&mut` into the shared world behind a documented `unsafe`
boundary justified by the partition's disjointness.)

**Labeler** — owns the registry (`actor→cluster`, `chunk→cluster`, `cluster→worker`) and is the **sole,
serialized executor of topology changes**:
- **places unclustered actors** (into an overlapping cluster, else a new one),
- **merges** clusters whose chunk-sets overlap,
- **executes splits** proposed by clusters,
- **repacks** clusters across workers by tick-time (minimize worker count; split an over-budget worker by
  reassigning whole clusters — free, since clusters don't interact across the cut; a single indivisible
  cluster is the one-core floor).
Analysis is distributed (clusters self-assess splits; cheap central merge broad-phase over chunk-sets);
**execution is serialized through the Labeler** to keep the partition single-writer.

**Read-model** — `ArcSwap<Snapshot>` (per cluster or per region), holding published changed-only deltas.
Observers read it lock-free.

**Datastore (stub in POC)** — clusters **periodically flush upserts** of changed entities. Real Datastore
integration is later.

**Sessions / transport (deferred detail)** — a session is the per-player endpoint that feeds intent in
and pulls a **View window** (chunks around the player) out of the read-model, streaming changed-only
upserts/removes over the **existing wire contract** (`apps/game_web/priv/contract`). Exact shape **TBD** —
resolved when we wire transport (Phase 4). POC stubs it.

## Plan — POC first

The POC is **test-driven**: the core simulation layer is proven by the unit + integration suite in
`sim/tests` before any parallelism, persistence, or transport is added.

**Phase 0 — skeleton.** A standalone Rust crate (e.g. `/sim`, *not* under `apps/`). ECS via `hecs`
(we want our own tick scheduling, not Bevy's frame scheduler), chunk geometry, the id/registry types,
`serde` deltas shaped to the existing contract. Stand up the **test harness** here: a deterministic
clock, a scripted-intent driver, and world/topology assertion helpers.

**Phase 1 — prove the core model (PRIORITY, test-first, single-threaded, no `unsafe`).** Structure the
simulation core as **pure, testable functions** — chunk-graph connectivity, placement, merge detection,
`should_split?`, repack *policy*, delta diffing — and drive their development **test-first**. One worker
ticks all clusters sequentially; actors are driven by scripted/synthetic intent (no real sessions). The
deliverable is the unit + integration suite in `sim/tests` covering every topology case (placement,
crossing, merge, split, repack, deltas, determinism, and the never-under-merge invariant property test);
only once it is green is the *model* proven — the genuinely novel logic, with zero concurrency risk.

**Phase 2 — parallelism + the `unsafe` boundary.** Multiple workers; disjoint `&mut` into the shared
world behind a small documented `unsafe` API whose soundness precondition is the Labeler's disjoint
partition; safe-point flag-check for relabels (workers self-tick, check a "pending relabel?" flag at
tick boundaries — no per-tick Labeler permission). Drive worker assignment with the **repack policy
already unit-tested in Phase 1**. Stress-test, and **measure the single-core dense-cluster ceiling** (the
limit we accepted). Validate disjointness with `miri`/stress where feasible.

**Phase 3 — persistence.** Periodic upsert flush to a Datastore (stub → real), decoupled from the tick.

**Phase 4 — observation & transport.** Wire deltas to the existing contract; define **sessions** and the
**View window** pull; connect a real client. (This is where the "sessions — not sure" question gets
settled.)

**Deferred:** NPCs and their AI (run inside clusters when added), combat specifics, crash/fault handling
(explicitly not a concern for now).

## Invariants & parameters to pin

- `interaction_range ≤ chunk_size` — the soundness precondition. Per-actor interaction radius allowed, but
  the **max** radius present must stay ≤ chunk_size (else the 3×3 footprint margin is insufficient).
- **Never under-merge:** chunk-set overlap ⇒ merge. Over-merging only costs parallelism; under-merging is
  a correctness break.
- **Hysteresis:** split distance (chunk-disconnection) strictly looser than merge (overlap) so a cluster at
  the boundary doesn't churn.
- **Conflict-check cadence** must be faster than chunk-crossing time (`chunk_size / max_speed`) — with a
  full chunk of margin this is many ticks, so it's cheap.
- **Tick-time** smoothed (EWMA); repack thresholds banded (e.g. split a worker >~75%, consolidate <~20%).

## Build log — decisions made during implementation

The prototype lives in `/sim` (Rust). Decisions resolved while building Phase 0/1:

- **Merge predicate = chunk-set *overlap* (share a chunk), not "border".** The prose said "share or
  border"; the precise, sound rule is *overlap* — it captures every interacting pair (interaction
  range ≤ chunk_size ⇒ interacting actors' footprints share chunks) with a full chunk of margin.
  The hysteresis band is exactly **Chebyshev distance 3** between two actors' chunks: at ≤2 their
  footprints overlap (merge); at 3 they border (one connected component — no split, no merge); at ≥4
  they disconnect (split). Merged at ≤2, stays merged through 3, splits at ≥4 — churn-free.
- **Reconcile-to-canonical after every mutation.** Each insert/move/remove recomputes the partition to
  its canonical form (two actors co-cluster iff their footprints transitively overlap), so
  never-under-merge holds *by construction*, verified against a canonical-partition oracle and a
  randomized property test. Cluster ids are preserved incrementally: a merge survivor keeps the lower id;
  a split keeps the id on its largest child.
- **Cross-chunk collision is now correct.** A cluster owns its actors' full 3×3, so collision sees
  obstacles in neighbouring chunks — the intended resolution of the Elixir per-chunk "clip-and-stop"
  artifact (CONTEXT.md). A blessed divergence, identical away from chunk boundaries.
- **Determinism** comes from an explicit sim clock, `BTreeMap`/`BTreeSet` ordering everywhere, and
  ticking clusters in id order. Depletion respawn uses sim-clock ms, not wall-clock `DateTime`.
- **Wire still carries full per-chunk `snapshot` events** (current Elixir behaviour); the changed-only
  deltas are the internal read-model feed (`delta.rs`), derived from the same entity states as the
  snapshot so they can't disagree.
- **Chunk hydration is lazy on cluster ownership.** Deactivation/persistence of cold chunks is deferred
  to Phase 3.

Phase 2 (parallelism):

- **No `unsafe` was needed.** IDEA.md anticipated "disjoint `&mut` into the shared world behind a
  documented `unsafe` API". In practice the dominant cost is the collision *compute*
  (O(movers × obstacles)), not the position write-back, so each cluster's inputs are **extracted** to
  owned data, computed across a worker pool with no shared access (trivially `Send`, zero `unsafe`),
  and **applied** serially. This gives the model's parallelism profile while keeping soundness *by
  construction* rather than by an `unsafe` precondition a Labeler bug could violate — strictly better
  for the determinism this project wants. The cluster disjointness is still load-bearing (it's what
  makes the jobs independent). hecs's per-column borrow model also makes per-entity disjoint `&mut`
  from multiple threads impossible without replacing the ECS, which independently rules the raw-pointer
  approach out. Parallel output is asserted identical to the serial tick (across multiple worker counts
  and a pooled `Sim`).
- **Persistent worker pool**, not per-tick spawn. The compute is so cheap that spawning OS threads each
  tick loses (measured 0.25–0.74×); a reused pool (IDEA's "workers self-tick") wins.
- **Measured single-core dense-cluster ceiling ≈ 0.085 ms/tick** for 500 movers × 1500 obstacles —
  vs a 20 Hz budget of 50 ms/tick. The accepted one-core floor is very generous: a single indivisible
  dense cluster has ~600× headroom at that density. **Parallel scaling ≈ 3.2×** across 10 cores on 96
  independent clusters (sublinear: dispatch overhead + sub-ms work). Takeaway: the model runs
  comfortably single-threaded at realistic loads; the pool is a tail-load accelerator.

Phase 3 (persistence):

- **Datastore** (`datastore.rs`): pending-writes buffer (per-key LWW + delete tombstones), merged reads
  (pending overlaid on durable), flush, and a backpressure state machine — over a `DurableStore` trait.
  The POC ships an in-memory `MemStore`; a Postgres impl can follow. Only the Overworld emits (Instances
  are in-memory only) — the realm guard lives in `RealmWorld::emit`.
- **Emission** mirrors the Elixir verbs: harvest → player + depletion; build → player + structure;
  damage → structure upsert or tombstone; respawn → depletion delete; plus a periodic player heartbeat
  and a leave-flush on disconnect, on the [`FLUSH_MS`] cadence.
- **Hydration** is split: `RealmWorld` seeds worldgen (trees/portals) on chunk load and reports
  newly-loaded chunks; the Sim overlays persisted structures + depletion state from the Datastore.
- **Resume** matches `hydrate_player`: reconnect in the saved chunk → exact position; elsewhere → chunk
  centre; inventory always restored. Mid-Instance disconnect saves one unit west of the entry portal, so
  reconnect doesn't loop straight back in. Structures and depletions survive a modelled restart
  (`Sim::into_store` → `Sim::with_persistence`). Depletion respawn time is sim-clock-relative — true
  cross-restart wall-clock timing needs clock persistence (deferred).

Phase 4 (observation & transport):

- **Wire-compatible Phoenix Channels v2 server**: same topics, events, and payloads as the Elixir socket
  (see `sim/README.md` for the module layout and topic list), so the frontend's `phoenix` JS client
  connects unchanged through Vite's `/socket` proxy.
- **Sessions** (resolved): a connection that joins its `player` channel *is* the session — it routes
  intent in and the tick loop pushes the View window out. The client subscribes to a 3×3 of chunk topics;
  the server broadcasts each subscribed chunk's full `snapshot` every `BROADCAST_EVERY` ticks and routes
  `self`/`relocated` to the owning player. No per-player server object is needed beyond the connection's
  channel state.
- **Delta ↔ contract mapping** (resolved): the wire carries full per-chunk `snapshot` payloads (the
  current Elixir behaviour), built from the same entity states as the changed-only deltas, so they can't
  disagree. A conformance validator checks every emitted payload against `contract.json` directly.
- **`unsafe` validation** (resolved): not applicable — Phase 2 needs no `unsafe` (see above), so there is
  no disjoint-access boundary to validate with miri/loom; the parallel-equals-serial stress tests cover it.
- **dev:stats** maps chunk lifecycle to hot (cluster-owned) / cold; the cluster model has no idle-armed
  timer, so `idle_ms_remaining` is always null — a documented divergence.

## Verdict (after Phases 0–4)

The interaction-clustered model is **proven and fully wire/feature-compatible** with the Elixir
implementation, on the radically different internal structure described above. Never-under-merge holds by
construction; the single-core dense-cluster ceiling is generous (~0.085 ms/tick at 500×1500); parallelism
is sound with zero `unsafe`. Remaining work is real-DB persistence and NPC/combat (deferred).

## Open questions (remaining)

- **Real Datastore**: swap `MemStore` for Postgres behind the `DurableStore` trait; persist the sim clock
  so depletion respawn timing survives a true process restart.
- Whether to pursue this Rust path over ADR-0001's BEAM design — recorded in
  [ADR-0002](./docs/adr/0002-rust-clustered-simulation-runtime.md) (proposed: adopt the Rust runtime
  contingent on closing the fault-tolerance/persistence gap; else stay on ADR-0001).
