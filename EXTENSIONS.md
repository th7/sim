# Extensions — agent-invented features

These features are **not** part of the original design conversation in `PLAN.md` / the ADRs. They are
extensions I (the agent) invented on top of the shipped NPC + Motivation system, then drove to green with
tests. They are kept separate so it stays clear which decisions came from the design owner and which are
my own proposals. Each obeys the same constraints as the base system: **deterministic by construction**
(no wall-clock, no unseeded RNG), **cluster-local perception** (≤ chunk_size, so the never-under-merge
invariant holds), and **cheap per tick** (no search, no extra simulation tiers).

## 1. Herd cohesion (deer) — *emergent herds*

**Idea.** A deer that is not fleeing steers toward the centroid of the same-species peers it can sense,
until it is within a comfort radius. Scattered deer coalesce into loose herds; a threat scatters them
(safety still wins arbitration), and they reform afterwards. No new drive and no flocking simulation —
just a steering override on the *idle/graze* branch, blended from data the perception already gathers.

**Why it fits.** Peers are sensed within perception range, so cohesion is cluster-local by construction.
The centroid is an integer mean and the steer is a unit vector — fully deterministic. Cost is O(peers).

**Behaviour pinned by tests.** Dispersed, unthreatened deer converge (max pairwise distance shrinks); a
threatened deer still flees regardless of its herd.

## 2. Pack focus (wolves) — *coordinated hunts*

**Idea.** When several wolves can sense each other (a pack), a hunting wolf picks the prey nearest to the
**pack centroid** rather than the prey nearest to itself. The pack converges on one focal animal instead
of splitting up, so hunts succeed faster and read as coordinated. A lone wolf is unchanged (nearest prey).

**Why it fits.** "Pack" = the same same-species peers already gathered as `rivals`/perception. Purely a
change to *which* prey the existing `attack` plan targets — deterministic, O(prey + pack), cluster-local.

**Behaviour pinned by tests.** Two wolves positioned so "nearest-to-self" would split them onto different
deer instead both commit to the same focal deer.

## 3. Diurnal temperament — *day/night mood*

**Idea.** Reuse the ecosystem's `season(t)` hook as a deterministic day/night phase and let it modulate
goal arbitration: wolves grow bolder (hunger bias up) at night, deer warier (safety bias up). The world
gains a visible rhythm with zero new state — it is a pure function of the sim clock.

**Why it fits.** A closed-form function of `clock_ms`; no stored state, trivially deterministic, and it
slots into the one place cross-need weighing already happens (goal arbitration bias).

**Behaviour pinned by tests.** The same wolf in the same situation chooses to hunt at night where it would
disengage by day (and the inverse for a deer's flight threshold), driven only by the clock phase.

## 4. Wounded retreat — *self-preservation*

**Idea.** An animal's *own* health feeds its safety drive: as HP falls, its safety bias is amplified, so a
wounded animal disengages from a fight (or flees a threat) it would have stood through at full health. A
desperate, near-death wolf abandons even a contested carcass.

**Why it fits.** Health is already on the entity; the engine reads `self_hp_frac` (a perception input) and
scales the safety bias at goal arbitration — the one place cross-need weighing happens. Pure, deterministic.

**Behaviour pinned by tests.** A starving, pressured wolf that fights-to-hold at full HP *flees* the same
situation at 10% HP — only the health fraction differs.
