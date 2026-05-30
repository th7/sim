# The living world

The fourth pillar: **a living world, not a backdrop.** The wild world has its own life —
animals that pursue their own needs, and regions whose state is shaped by how players have
used them. This doc states what "living" must mean and the design intents behind it. The
runtime decisions that realize it are ADRs
[0004](../docs/adr/0004-npc-motivation-arbitration.md),
[0005](../docs/adr/0005-npcs-not-warm-set-anchors.md),
[0006](../docs/adr/0006-cold-ecosystem-deterministic-field.md).

## What "living" has to mean

Three commitments, in tension, that the model has to satisfy at once:

1. **Animals behave believably** — they pursue needs (eat, stay safe), react to threats and to
   each other, and look like creatures with their own agenda, not waypoints on a track.
2. **The world has history** — a place that has been overhunted is *different* afterward, and
   recovers over time. The world's state is a consequence of what players did to it, not a
   script reset each visit.
3. **It costs almost nothing where no one is.** A living world cannot mean simulating the whole
   Overworld forever. Life must be cheap in the empty places and rich only where Players are.

## Animals: needs, not scripts

An **NPC** is simulated exactly like a **Player** — same authority (**Island**), same single
seam (**Intent** per tick) — differing only in where its Intent comes from: a **Motivation**
engine rather than a remote session.

The Motivation model's design intent is **believable behaviour from one simple rule**, applied
the same way at every level: *pick the most-immediate actionable option.* A **Need** (hunger,
safety) roots a **Behavioral chain** that climbs from immediate to strategic; each tick the
chain offers its most-immediate actionable **Bid**; the winning Bid becomes the **Goal**; the
Goal expands to the most-immediate actionable **Plan**; the Plan's head becomes the tick's
Intent. Cross-need weighing happens in exactly one place — **Goal arbitration** — where a
static per-Need **bias** (safety outranks hunger) is modulated by **Pressure**, the slow
build-up of a chronically unmet Need. Pressure never picks *what* to do, only *which Need wins*
— so a long-starving animal will trade away safety, but an immediate threat can still win at low
pressure.

**Why this shape.** One rule at every level keeps behaviour legible and predictable (a designer
can reason about why an animal did something) while still producing adaptation: the same `feed`
Goal yields calm grazing or a fight-to-hold depending only on what's actionable right now.
v1 ships two kinds — **deer** (prey) and **wolves** (predator) — and the immediate end of the
hunger and safety chains; the strategic tail (carry → stockpile → secure a food source) is a
known future extension the model already has room for.

### Emergent behaviour is the goal, not extra rules

Group behaviour should *emerge* from individuals following their needs near each other, not
from a separate flock controller. The shipped extensions — deer **herd** and **stampede**,
wolves **pack-hunt**, animals **bolder at night** and **warier when wounded** — are all local
tweaks to one animal's perception or arbitration, never global scripts. This is the design
preference: reach group-scale life by composing local rules.

## The world has history: regions, baseline, disturbance

The world's ecological state is keyed on **Regions** — deterministic territories, each with a
**Habitat** (meadow, forest) — *not* on the Chunk grid. A Region's wildlife level is:

> **Baseline**(habitat, season, local noise) + a decaying **Disturbance**

- **Baseline** is what *should* be there absent players — a pure function of place and time,
  evaluated, never simulated.
- **Disturbance** is the **history**: a persisted, per-Region record of how players pushed
  wildlife away from Baseline (overhunting). It **decays back toward zero** — the Region
  *heals* — so the live level is always Baseline plus a shrinking scar.

**Why a field, not tracked individuals.** History at the scale of a whole world can't be a
ledger of individual animals. A sparse per-Region delta captures the part that matters — *this
area was overhunted and is recovering* — cheaply and durably, while individual animals remain
ephemeral.

### History shapes population *and* temperament

This is the intent that makes the field feel alive rather than statistical: a Region's
Disturbance shapes not only *how many* animals spawn but *how they behave*. A depleted,
high-Disturbance Region spawns **hungry, high-pressure (aggressive)** animals; a healthy Region
spawns **placid** ones. Overhunting an area doesn't just thin it — it makes what remains
desperate. The world's past is legible in the mood of its present inhabitants.

## Cheap where empty: materialize and dissolve

Wildlife is **simulated only near Players** and is otherwise *computed, never ticked*:

- **NPCs do not keep the world hot.** Only Players anchor the **Warm set**; an NPC is simulated
  only inside a Player-hot **Chunk**.
- When a Chunk warms, wildlife **materializes** — animals are seeded from the Region's current
  level (count *and* temperament), with no individual history carried in.
- When the Chunk cools, survivors **dissolve** back into the Region's **Disturbance**:
  intra-region wandering nets out to nothing, while kills leave a healing depletion behind.

So an NPC has **no persistent individual identity** across a cold/warm cycle. The continuity a
Player perceives is the *Region's* continuity (its level and mood), not any one animal's.

**Why accept fungible individuals.** Persisting every animal's exact state would be the
expensive ledger we rejected. The design bet is that players read continuity at the *place*
level — "this forest is hunted out and the wolves here are vicious" — not at the
individual-animal level, so the Region field is the right grain to make durable.

## The carcass: where the wild world meets the economy

A killed animal leaves a **Carcass** — a perishable **Gatherable** that **Players** harvest for
meat/hide (feeding [`economy.md`](./economy.md)), that **NPCs** eat from to satisfy hunger, and
that rival predators **contest**. It is the single object where the ecosystem and the material
economy touch: hunting is both an ecological act (it writes the Region's Disturbance) and an
economic one (it yields Items).

## Open frontier (named, not yet designed)

Surfaced from the project's follow-ups; each needs a design pass before build:

- **Grass-grazing closes the loop.** Today only hunting writes Disturbance; deer graze against a
  grass level that doesn't yet feed back. Closing it would make grazing pressure part of the
  history a Region carries.
- **Durable history.** The Disturbance field is the world's memory of how it's been used — it
  should survive a restart like the rest of the persistent world (it does not yet).
- **The strategic chain tail.** carry → stockpile → secure-a-source: the believable behaviour of
  a *sated* animal still provisioning for later. The model has room for it; the activation
  semantics are real design work.
- **Surfacing hidden state to the Player** (see root `ROADMAP.md`): letting a Player *read* an
  animal's goal/status and a Region's depletion at a glance. This is the design question of how
  much of the living world's inner life should be legible in the client.
