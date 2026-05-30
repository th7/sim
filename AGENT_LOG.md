# Agent Log

The agent's working **review queue** (see AGENTS.md → **Work Loop**): decisions made autonomously,
recommendations the agent can't act on alone (ADRs, glossary changes, architectural refactors), deferred
follow-ups, and agent-invented features. During **Stabilize** with a human in the loop, these are reviewed
and **removed as they settle** — keepers graduating to `docs/adr/` (rationale), `CONTEXT.md` (glossary), or
`DESIGN.md` (shipped behaviour); git history holds whatever is removed. The forward plan is in `PLAN.md`.

## Recommended follow-ups (deferred / need a human)

- **Strategic chain tail — carry → stockpile → secure-source** (+ in-session cache object). The Motivation
  engine supports the full chain shape; v1 ships only `hunt → eat` + `flee / fight-to-hold`. Faithfully
  implementing the strategic nodes' activation semantics (how a sated animal still provisions) is real v2
  work — left deferred rather than approximated. Likely warrants a human-ratified ADR.
- **Cross-restart persistence of Region Disturbances.** They live in memory (`Sim.wild_disturb`), so the
  overhunt→deplete→heal field resets on restart. Needs a `PersistEvent` variant + Datastore/pg schema.
- **Grass-grazing Disturbance.** Deer graze abstractly against the Region's grass level; only deer/wolf
  population changes feed back into the field. Closing the loop (grazing writes a grass Disturbance) is a
  follow-up.

## Design parameters chosen — NPC + Motivation (v1)

Recommended answers applied during implementation (not separately grilled); tunable.

- **Region scale:** Worley feature points on a ~8-chunk grid; regions average a few-chunk territory.
- **Habitats:** `Meadow` (high grass/deer, low wolf), `Forest` (low grass/deer, high wolf).
- **Season:** `season(t)` constant 1.0 (hook; the day/night cycle reuses the same idea — see extensions).
- **Perception range** = 1 unit (`INTERACT_RANGE`-ish); melee `attack` range ~0.6–0.7 unit.
- **Pressure params:** hunger τ≈60 s, safety τ≈10 s, cap 1.0.
- **HP / damage:** deer 50, wolf 80; NPC attack 10/hit, ~0.5 s cooldown; player click 25 (existing).
- **Carcass:** deer 3 meat, wolf 2; player harvest also yields 1 Hide; perishes ~60 s.
- **New Items:** `Meat`, `Hide` (open inventory keys — no contract change for inventory).
- **NPC determinism:** SplitMix64 (`harness::Rng`) seeded by chunk/spawn-index/clock-bucket; no wall-clock.
- **NPC WireId:** `npc:<kind>:<spawn_id>` — ephemeral, not persisted (only Disturbances persist).

## Decision log — NPC + Motivation system

- **Pressure integrator = exponential low-pass toward `cap·activation`** (time constant τ), not a raw
  accumulator. Bounded/stable by construction; chronic-vs-acute via τ (hunger 60 s, safety 10 s).
- **Inter-chain score = `immediacy · bias · (1 + pressure)`** (pressure as a *gain*), so an acute threat
  can still win at low pressure while chronic pressure tips close calls. Bias: wolf safety 1.2 / deer 1.5 > hunger 1.0.
- **`decide()` raises hunger (metabolism); the ECS lowers it via `Drives::feed()` on a successful eat** —
  keeps the engine pure while need-level dynamics stay unit-testable.
- **Engine emits no RNG.** Wander/tie-breaks return `Decision::Wander`, resolved by the ECS with a seeded PRNG.
- **Wolf speed 4200 > deer 3800 sub-units/s**, so a pursuing wolf closes on fleeing prey.
- **Deer treat Players as threats and flee them; Wolves flee only once attacked** (`being_attacked`).
- **Motivation runs at the top of `RealmWorld::tick` and inside the parallel `tick_realm` closure** — the
  serial pre-movement Intent write, identical in both tick paths.
- **NPC verbs resolve in a post-movement phase inside `reconcile_after_movement`** (shared by both tick
  paths). Motivation stores `NpcDecision`; resolution applies `attack`/`eat` in range, gated by `ActReady` (500 ms).
- **Player invulnerability is structural:** Players carry no `Health`, so an NPC `attack` on a Player is a
  no-op by construction. NPC↔NPC and Player→NPC damage flow normally.
- **Player damage/harvest extend to wildlife by proximity, not exact key** — a click with no Structure/tree
  targets the nearest NPC / Carcass in range (NPCs move, so position-keyed wire ids don't fit them).
- **Carcass = its own component** (`meat`, `perish_at_ms`), not an overloaded `Gatherable`, to keep the
  tree respawn path clean; still harvested via the harvest verb.
- **Combat constants** (`world.rs`): attack 10 dmg, melee range² 700², cooldown 500 ms, eat feeds 0.4 hunger/meat.
- **Warm/cold boundary keyed on the Player Warm set** (`player_warm_chunks` = union of Players' 3×3), *not*
  `labeler.owned_chunks()` — this realizes ADR-0005: NPCs ride the Player-driven warm set and dissolve when it recedes.
- **Dissolve folds `survivors − materialized` per stratum into the Region Disturbance** (÷ capacity), so
  intra-region wandering nets ~0 while kills leave a healing depletion. Caps: deer 4, wolf 2/chunk.
- **Wildlife is a Sim toggle, default OFF** (`set_wildlife`); the game server enables it (`SIM_WILDLIFE`,
  default on in the bin). The toggle keeps the position-tuned build e2e from going flaky under auto-wildlife load.
- **Wire: `ChunkSnapshot` gains `npcs` + `carcasses`** (`#[serde(default)]`), added to `contract.json`
  (snapshot `required`). New `NpcWire{type,x,y,hp}` / `CarcassWire{x,y,meat}`, serialized from the same
  `entity_states` path as every entity.
- **Dev mode shows NPCs:** `StatsPayload.total_npcs` (server-authoritative, in `contract.json`) + a HUD
  line `npcs: <in view> / <in world>`; NPC/carcass meshes added to the GL scene.
- **e2e `T` timeout raised 5 s→10 s** so the integration suite stays reliable when every crate's in-process
  server runs concurrently under `cargo test --workspace`. No assertion weakened.

## Agent-invented features

Extensions invented by the agent on top of the shipped NPC + Motivation system, then driven to green with
tests. Kept distinct from the design owner's decisions. Each is **deterministic by construction**,
**cluster-local** (≤ chunk_size, so the never-under-merge invariant holds), and **cheap per tick**.

1. **Herd cohesion (deer)** — a non-fleeing deer steers toward the centroid of sensed peers until within a
   comfort radius, so scattered deer form loose herds; threats scatter them, they reform. Yields to a very
   hungry deer (won't socialize into starvation). *Tests:* dispersed deer converge; threatened deer still flees.
2. **Pack focus (wolves)** — a wolf with packmates targets the prey nearest the *pack centroid*, so the pack
   gangs up instead of splitting. Lone wolf unchanged. *Test:* two would-split wolves commit to one deer.
3. **Diurnal temperament** — a deterministic day/night phase (`day_phase(clock_ms)`) tilts goal arbitration:
   wolves bolder at night, deer warier. Zero new state. *Test:* same wolf hunts at night, disengages by day.
4. **Wounded retreat** — `self_hp_frac` amplifies the safety bias, so a wounded animal disengages from a
   fight it would take at full health. *Test:* a starving, pressured wolf fights at full HP, flees at 10%.
5. **Stampede (fear contagion)** — a fleeing deer becomes `Alarmed`; peers sensing an alarmed neighbour
   catch the panic and flee too, so the herd scatters from a threat only its edge can see (a 1-tick wave).
   *Tests:* a deer flees on an alarmed neighbour alone (unit); a struck herd's centroid flees (integration).
