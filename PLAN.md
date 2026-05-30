# PLAN: NPCs and the Motivation system

What we're building and how, for the NPC + Motivation feature. Domain language is in
[`CONTEXT.md`](./CONTEXT.md); the load-bearing decisions are in
[`docs/adr/0004`](./docs/adr/0004-npc-motivation-arbitration.md),
[`0005`](./docs/adr/0005-npcs-not-warm-set-anchors.md), and
[`0006`](./docs/adr/0006-cold-ecosystem-deterministic-field.md). The existing runtime
(interaction-clustered Rust sim) is described in [`DESIGN.md`](./DESIGN.md) and ADR-0002;
prior build history is in git.

## The feature in brief

Wildlife **NPCs** (a **wolf** predator and a **deer** prey) live in the **Overworld**, driven
by a **Motivation** engine. The world has a deterministic **cold ecosystem**: an animal is a
fungible materialization of a **Region**'s wildlife level, simulated only while a **Player** is
near, dissolving back into a healing **Disturbance** when unobserved. Players hunt animals for
meat/hide **Items**, feeding the existing craft/gather economy. Players are invulnerable in v1;
damage flows player→NPC and NPC↔NPC.

## The Motivation engine (ADR-0004)

One selection rule — *most-immediate actionable option* — at three levels:

1. **Within a chain:** the most-immediate actionable node is the chain's **Bid**. Chains climb
   toward their strategic end as near nodes satisfy and deactivate.
2. **Across chains:** the winning Bid is the **Goal**. *This is the only place cross-need weighing
   happens:* a static per-**Need** priority **bias**, modulated by **Pressure**.
3. **Within a goal:** the most-immediate actionable **Action** sequence is the **Plan**; its head
   resolves to the tick's **Intent**.

**Pressure** is a leaky, sim-clock integral of a Need's own activation, decaying on satisfaction,
hard-capped. It modulates only goal arbitration.

**Actions** are a shared library (move-to, eat, pick-up, attack, flee, graze, wander), owned by no
Need.

## Runtime integration (ADR-0005)

- An NPC is an ECS **actor** in a **cluster**, exactly like a Player; its **Intent** is written by
  Motivation in a **serial pre-movement phase**, then the existing movement/collision tick runs
  unchanged. (Parallelizable later by the same cluster-disjointness.)
- **Perception is interaction:** perception range ≤ `chunk_size`, so anything an NPC can sense is
  already in its own cluster — the never-under-merge invariant holds by construction.
- NPCs are **not Warm-set anchors**: only Players keep chunks hot. NPCs simulate only inside
  Player-hot chunks.

## Cold ecosystem (ADR-0006)

- `region(x,y)` = deterministic Worley/Voronoi cell, each with a **Habitat**.
- `level(region, t) = clamp(Baseline(habitat, season(t), noise) + Δ·e^(−(t−t₀)/τ))` — a pure
  function plus a sparse per-Region **Disturbance** that decays to zero. No cold tick.
- **Warm boundary:** materialize wildlife from `level` via seeded spawn chance; warm hunting/grazing
  writes the Region's Disturbance; survivors dissolve back on cooldown.
- **Spawn-derived temperament:** a materializing NPC's initial Needs/Pressure are a deterministic
  function of the Region level — a depleted Region spawns hungry, aggressive animals.

## Combat & food loop

- Actors gain **HP**. Player click-damage (existing 25/click verb) extends from Structures to NPCs.
  NPC `attack` deals contact damage to prey and rival NPCs. Players take no damage (invulnerable).
- A killed animal leaves a **Carcass** — a perishable **Gatherable** yielding meat/hide **Items**.
  NPCs eat from it (hunger); Players harvest it (economy); rival predators contest it.

## NPC kinds for v1

- **Deer** — Needs: Hunger (→ graze), Safety (→ flee). Minimal.
- **Wolf** — Needs: Hunger (→ hunt → eat → carry → stockpile), Safety (→ flee / fight-to-hold).

## Build plan (TDD slices)

1. **Motivation engine** (`motivation.rs`) — pure, no ECS. Need/Pressure/Bid/Goal/Plan/Action over a
   `Perception` snapshot → a `Decision`. Test-first, every behavior pinned. **(highest value first)**
2. **Ecosystem field** (`ecosystem.rs`) — pure. `region()`, `Habitat`, `Baseline`, `Disturbance`
   relaxation, `level()`, spawn-chance + spawn-derived temperament. Test-first.
3. **NPC ECS integration** — components (Npc, Health, Needs…), spawn/despawn, the serial Motivation
   pre-movement phase writing Intent; NPCs as cluster actors that don't anchor the Warm set.
4. **Combat** — HP/damage to actors, NPC attack Action, Carcass entity, death, player harvest of
   carcasses into Items.
5. **Warm/cold boundary** — materialize from `level`, dissolve to Disturbance, persist Disturbances
   through the Datastore.
6. **Wire & client** — NPCs + carcasses on the snapshot wire; contract update; native client render.

Each slice: `cargo test --workspace` + warning-free `cargo build --workspace --all-targets` before a
commit.

## Remaining questions — my recommended answers (proceeding on these unless revised)

These were not separately grilled; I am implementing the recommended answer and logging any that
prove consequential in the Decision log below.

- **Region scale:** Worley feature points on a coarse grid of ~8 chunks; regions average a few-chunk
  territory so a warm session stays mostly within one Region. Tunable constant.
- **Habitats (v1):** `Meadow` (high grass, high deer, low wolf), `Forest` (low grass, low deer, high
  wolf). A third (`Barren`) optional.
- **Season:** `season(t)` ships as a constant 1.0 in v1 (hook present, no cycle yet).
- **Perception range:** = `INTERACT_RANGE` (≤ chunk_size). Melee `attack` range smaller (~0.6 unit).
- **Pressure params:** hunger τ ≈ 60 s, safety τ ≈ 10 s, cap 1.0; chosen as constants, tuned later.
- **HP / damage:** deer 50, wolf 80; NPC attack 10/hit on a ~0.5 s cooldown; player click 25 (existing).
- **Carcass:** yields `{Meat, n}` (+ `Hide` later); perishes ~60 s if unconsumed; a Gatherable reusing
  the Resource-node path.
- **New Items:** `Meat` (and `Hide`). Requires a contract/wire addition for the economy.
- **NPC determinism:** all stochastic choices use SplitMix64 (`harness::Rng`) seeded by
  `hash(region, chunk, spawn_index, clock_bucket)` — reproducible, no wall-clock, no global RNG.
- **NPC WireId:** `npc:<kind>:<spawn_id>` — ephemeral, not persisted (only Disturbances persist).
- **Strategic chain tail (carry/stockpile/secure-source):** scoped to a later slice; v1 wolf chain
  ships `hunt → eat` + `flee / fight-to-hold`. The engine supports the full chain; the strategic
  nodes + the in-session cache object are deferred so slice 1–4 stay tractable. **Flagged.**
- **Grass in warm chunks:** no per-tile grass entities; deer graze abstractly against the Region's
  grass level, writing a grass Disturbance. **Flagged.**

## Decision log (appended during implementation)

- **Pressure integrator = exponential low-pass toward `cap·activation`** (time constant τ), not a raw
  accumulator. Bounded and stable by construction: sustained max activation saturates at `cap`, quiet
  decays to 0. Chronic-vs-acute is expressed via τ (hunger τ=60s slow, safety τ=10s fast). Satisfies
  ADR-0004's "leaky integral, decays on satisfaction, hard-capped" without unbounded growth.
- **Inter-chain score = `immediacy · bias · (1 + pressure)`** (pressure as a *gain*, not additive), so
  an acute threat (immediacy→1) can still win at low pressure, while chronic pressure tips close calls —
  matching the grilled "might sacrifice safety" semantics. Bias: wolf safety 1.2 / deer safety 1.5 > hunger 1.0.
- **`decide()` raises hunger (metabolism); the ECS lowers it via `Drives::feed()` on a successful eat.**
  Keeps the engine pure (world-effects stay in the ECS) while the need-level dynamics remain unit-testable.
- **Engine emits no RNG.** Wander direction / tie-breaks are returned as `Decision::Wander` and resolved
  by the ECS with a seeded PRNG, keeping the engine a deterministic pure function.
- **Wolf speed 4200 > deer 3800 sub-units/s**, so a pursuing wolf closes on fleeing prey (hunts terminate).
- **Deer treat Players as threats and flee them; Wolves flee only once attacked** (`being_attacked`, slice 4).
  Gives "deer run from you" for free while keeping wolves bold until provoked.
- **Motivation runs at the top of `RealmWorld::tick` and inside the parallel `tick_realm` closure** — the
  serial pre-movement Intent write, identical in both tick paths.
- **FLAGGED (for slice 5):** an NPC is a Labeler actor, so its cluster currently still marks its chunks
  owned → it *does* keep them warm. Proper warm-set gating (only Players anchor) belongs with the
  materialize/dissolve boundary; until then NPCs only exist where tests/players put them.
- **Wander direction is sim-clock-bucketed (≈1 Hz) and seeded by actor id**, so drift is deterministic
  and doesn't jitter every tick.
- **NPC verbs resolve in a post-movement phase inside `reconcile_after_movement`** — one insertion point
  shared by the serial and parallel ticks. The Motivation phase stores `NpcDecision`; resolution applies
  `attack`/`eat` when in range, gated by an `ActReady` cooldown (500 ms).
- **Player invulnerability is structural:** Players carry no `Health`, so an NPC `attack` resolving on a
  Player is a no-op by construction — no special-case check. NPC↔NPC and Player→NPC damage flow normally.
- **Player damage/harvest extend to wildlife by proximity, not exact key:** a click with no Structure/tree
  at the cell targets the nearest NPC / Carcass within interact range. NPCs move, so position-keyed wire
  ids (used for trees/structures) don't fit them.
- **Carcass = its own component** (`meat`, `perish_at_ms`), not an overloaded `Gatherable`, to keep the
  tree respawn path clean. It is still harvested via the harvest verb (CONTEXT's "a Gatherable"). Deer
  yield 3 meat, wolf 2; player harvest also yields 1 Hide; carcasses rot after 60 s.
- **Combat constants** (`world.rs`): attack 10 dmg, melee range² 700², cooldown 500 ms, eat feeds 0.4
  hunger/meat. Tunable; chosen so hunts and feeds terminate within a short observation.
