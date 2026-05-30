# Glossary

The project's canonical language: every domain term, one canonical name, the aliases to
avoid, and the relationships between them. This file is the **source of truth** for the
language — it supersedes the language section of the root `CONTEXT.md` (see
[`overview.md`](./overview.md) for that transition).

Altitude: this glossary defines terms at the level of *what a thing is and why it exists*.
Where a term names a runtime mechanism (the **Datastore**, for instance), the entry states
only the **design promise** it must keep; the mechanism that keeps it is the engineer's and
lives in `docs/adr/` and the code.

When a term resolves, it is added here immediately. A vague or overloaded term gets one
opinionated canonical name; the rest are listed as aliases to avoid.

---

## World & space

**World** — The total game universe: the union of the **Overworld** and all live
**Instances**.
_Avoid:_ Server, realm, shard (implementation terms).

**Overworld** — The single shared, persistent, free-positioned 2D space that all
**Players** inhabit together. There is exactly one.
_Avoid:_ Map, world map, overland.

**Instance** — An ephemeral, private region spawned on demand for one **Party** entering a
dungeon. In-memory only; never persisted; destroyed when the Party leaves or disconnects.
In v1 dungeons are the only kind of Instance — no housing, no persistent dungeons, no guild
halls. An Instance is simulated exactly as the Overworld is (Chunks, Islands, the
Cartographer, the Warm set, the View window); it differs only in scope, lack of
persistence, and bounded extent.
_Avoid:_ Dungeon (the *content* an Instance hosts, not the Instance), private map, room.

**Chunk** — A fixed-size rectangular partition of the **Overworld** — the unit of
**Worldgen** determinism, persistence keying, and spatial indexing. A Chunk is *data*
(worldgen output plus stored state, identified by coordinate), never a running process. The
live simulation of whatever occupies a Chunk's area is owned at runtime by an **Island**.
_Avoid:_ Tile (implies discrete movement, which we rejected), zone, region. Never call a
Chunk a process or an owner of live state — that is the Island's role.

**Region** — A deterministic territory of the **Overworld**, independent of the Chunk grid,
each carrying one **Habitat**. Regions — not Chunks — are the unit the ecosystem
**Baseline** and player **Disturbances** are keyed on.
_Avoid:_ Zone, area, biome (biome names the Habitat *type*, not the territory).

**Habitat** — The ecological type of a **Region** (meadow, forest, …) that fixes its
baseline grass and wildlife levels.
_Avoid:_ Biome (we say Habitat), terrain (terrain is the rendered ground; Habitat is the
ecological role).

---

## Presence & authority

**Player** — A human participant, identified by a chosen username; also the in-world entity
they control. We deliberately do not distinguish Player from Character — one username = one
in-world entity.
_Avoid:_ Character, user, account, avatar.

**Party** — A group of **Players** (one or more) that enters an **Instance** together. The
Instance is spawned for the Party and destroyed when the Party dissolves.
_Avoid:_ Group, raid, team (these carry specific other-MMO meanings we may want later).

**Island** — The single runtime authority over a connected cluster of interacting dynamic
entities (**Players**, and **NPCs** where present) together with the **Chunks** their
activity currently spans. Because one Island resolves every movement, collision, and combat
among its members, *every interaction is decided by exactly one authority, locally*. Islands
are ephemeral and continuously reshaped by the **Cartographer** — created, merged, split as
entities move — so any two entities able to interact are always inside the same Island. An
Island sizes itself to activity.
_Design promise:_ no interaction is ever split across two authorities (the *never-under-merge*
invariant).
_Avoid:_ Zone, region, shard, cell (an Island is interaction-scoped and dynamic, not a fixed
spatial division); Party (a Party is a social grouping; an Island may hold unrelated entities
that merely happen to be near each other).

**Cartographer** — The single authority that maps the world into **Islands**: it assigns
each **Player** (and each hot **Chunk**) to an Island and creates, merges, and splits Islands
as entities move. As sole arbiter, it serializes these changes — two Islands never race to
merge. It assigns authority; it does not simulate.
_Avoid:_ Navigator (implies steering movement), coordinator/manager (too generic), scheduler.

**Intent** — The per-tick movement/action input one actor hands its **Island** at tick
start, read to integrate movement and resolve interactions. A **Player**'s Intent comes from
their session; an **NPC**'s comes from its **Motivation**. This is the single seam both kinds
of actor share.
_Avoid:_ Input, command, keypress.

---

## Things in the world

**Resource node** — A gatherable world object (tree, rock, ore vein, plant). Depletes when
harvested and respawns on a timer. World state, owned by no Player.
_Avoid:_ Resource (ambiguous — also means inventory material), node (too generic).

**Structure** — A persistent object placed in the **Overworld** by a **Player** (building,
wall, station, fence). Survives until destroyed; anchored to a specific **Chunk**;
per-Structure ownership. In v1 the only Structure is a *wooden palisade* (the "wall"), cost
5 **wood**.
_Avoid:_ Building (only one kind exists), object, placeable.

**Portal** — A fixed Overworld entity marking the entry point to an **Instance**. Placed
deterministically by Worldgen — not built by Players, not a **Structure**. Anchored to a
Chunk. Overlapping a Portal triggers **Instance entry**.
_Avoid:_ Structure (Portals are not player-placed), gate, dungeon entrance.

**Footprint** — The world-space shape an obstacle occupies. A **Player** carries a body
circle and cannot move so that their body would overlap any Footprint. Collision is one-way:
the world blocks the Player; the Player blocks neither the world nor other Players. A
**Resource node**'s Footprint is identical gatherable or depleted — harvesting never opens a
path.
_Avoid:_ Hitbox (implies a damage zone), bounding box (only one Footprint shape is
rectangular), collider (implementation term).

**Carcass** — The perishable remains of a killed animal — a **Gatherable** that yields
meat/hide **Items**. Contestable: both **NPCs** and **Players** harvest it, and rival
predators fight to hold it. Perishes on a timer if left unconsumed.
_Avoid:_ Corpse (implies a Player death — Players don't die in v1), loot, drop, kill.

---

## Items & the economy

**Item** — A *kind* of gatherable, stackable substance — wood, stone, iron ore, meat, hide.
Abstract: an Item is a type, never a quantity. Produced by harvesting **Resource nodes** /
**Carcasses**; consumed when **Players** build **Structures**.
_Avoid:_ Material (collides with "crafting material" once recipes exist), Resource (forbidden
— see Resource node).

**ItemStack** — A typed quantity of one **Item** (e.g. `{wood, 14}`). The unit inside an
**Inventory** and the unit in which harvest yields and build costs are expressed.
_Avoid:_ Item (Item is the type, ItemStack the quantity — keep them distinct).

**Inventory** — The **ItemStacks** carried by a **Player**. Filled by harvesting; drained by
building. Persists across sessions — what you carry at logout is what you carry on login.
_Avoid:_ Bag, backpack, container (Container may earn an entry if chests/banks appear; v1 has
only the Inventory).

---

## Lifecycle & persistence

**Chunk activation** — The transition of a **Chunk** from cold to hot: an **Island** takes
ownership of the Chunk's area and hydrates its state through the **Datastore**. Triggered
when an Island's entities move into range.

**Chunk deactivation** — The reverse: when no **Island** holds a **Chunk** any longer, its
final state is handed to the **Datastore** and it goes cold. Triggered by sustained absence
of entities.

**Instance entry** — A **Player** leaves the **Overworld** and enters an **Instance** by
overlapping an `into_instance` **Portal**. The Player is re-homed onto a fresh Instance-scoped
**Island**; the **View window** switches to the Instance's space; in-Instance state is lost on
disconnect.
_Avoid:_ Portal travel, teleport, dungeon enter, zone-in.

**Instance exit** — The reverse: a **Player** overlaps the return-Portal and returns to the
**Overworld Chunk** they entered from, at a small offset from the Portal cell. The Instance is
destroyed when no Players remain.
_Avoid:_ Portal exit, dungeon leave, zone-out.

**Warm set** — The set of **Chunks** an **Island** keeps hot: the area its entities occupy
plus a surrounding margin, so an entity reaching the edge never stalls on a cold Chunk. A
Chunk stays hot as long as some Island holds it.
_Avoid:_ Warm zone, warm radius (the radius is a parameter; the set is the concept).

**View window** — The area around a **Player** that the server streams to their client. The
server owns and drives it; the client subscribes only to its own Player topic and renders what
it receives. Always contained within the Player's **Warm set**, whose margin hides activation
latency as the Player moves.
_Avoid:_ Visible chunks, subscription window, AOI (the View window is our specific form of AOI).

**Datastore** — The single persistence authority for the running world: all durable reads and
writes flow through it. *Design promise:* the world remembers (persisted state survives
restart), and the system **never silently loses or corrupts state** — under overload it stalls
(see **Backpressure**) rather than dropping writes or crashing. Its internal mechanism (the
pending-writes buffer, flush cadence, hydration merge) is engineer-owned — see `docs/adr/`.
_Avoid:_ Repo, persistence layer, cache, store. Don't say "actor" in domain language.

**Backpressure** — The system's overload-protection promise: when the **Datastore** cannot
keep up, affected **Players** *freeze* (their inputs stall) rather than the system losing their
state or crashing; play resumes when the Datastore recovers. The freeze is whole-**Island** —
everyone sharing a stalled authority waits together. The mechanism is engineer-owned.
_Avoid:_ Throttle, rate-limit, drop.

---

## NPCs & Motivation

**NPC** — A non-human dynamic entity simulated inside an **Island** exactly like a **Player**:
it moves, collides, and (later) fights under the same single authority. The one difference is
the source of its **Intent** — a Player's comes from a remote session, an NPC's is produced
each tick by its **Motivation**.
_Avoid:_ Mob, monster (not all NPCs are hostile), creature, bot, agent.

**Motivation** — The system that produces an **NPC**'s **Intent** each tick: a fixed set of
root **Needs**, each driving a **Behavioral chain**, arbitrated into one **Goal** that expands
into a **Plan**. The NPC's analogue of a Player's session.
_Avoid:_ AI, brain, behaviour tree (the model is explicitly not a behaviour tree).

**Need** — A root motivator of an **NPC** — hunger, safety, shelter. A fixed set per kind of
NPC. Each Need roots exactly one **Behavioral chain** and carries a **Pressure**, and has a
static priority **bias** relative to the others (e.g. safety outranks hunger) before Pressure
applies.
_Avoid:_ Drive, motive, urge, desire.

**Behavioral chain** — The ordered sequence of progressively more strategic sub-goals serving
one root **Need** (hunger → eat → carry food → stockpile → secure a food source). Each tick the
chain offers exactly one **Bid**.
_Avoid:_ Behaviour tree, goal stack, plan tree.

**Bid** — The single candidate a **Behavioral chain** offers each tick: its most-immediate
*actionable* node (a node is actionable only when its preconditions hold, so the chain climbs
toward the strategic end as nearer needs are met). Bids compete in **Goal** arbitration.
_Avoid:_ Proposal, vote, candidate.

**Pressure** — A per-**Need** measure of how chronically that Need has gone unmet. It plays
**no part within a chain**; its sole role is to modulate **inter-chain** arbitration — lifting
a chronically-unmet Need's **Bid** past the static bias, so a long-hungry animal trades away
safety. It never selects *what* to do, only *which* Need owns the Goal.
_Avoid:_ Urgency (a node's immediate activation, not the accumulated term), stress, mood.

**Goal** — The one **Need** an **NPC** is currently acting on: the **Bid** that wins
arbitration (static bias modulated by **Pressure**). Expanded into a **Plan**.
_Avoid:_ Objective, target, Intent (Intent is the per-tick output).

**Plan** — The most-immediate *actionable* sequence of **Actions** pursuing the current
**Goal**, chosen by the same precondition-gated immediacy rule as a chain **Bid**, and adapting
to circumstance (a `feed` Goal yields `fight-to-hold` when a threat blocks calm feeding). Bottoms
out in per-tick **Intent**.
_Avoid:_ Script, routine, behaviour.

**Action** — A primitive in a **shared** library that **Plans** compose — move-to, eat,
pick-up, attack, flee. Owned by no Need or chain: the same Action can serve different Goals (a
wolf fighting *for* food, not *for* safety).
_Avoid:_ Verb (a Verb is a Player-initiated server command — harvest/build/damage; an Action is
an NPC-Plan primitive, even where the two resolve to the same effect), skill, ability.

---

## The wild ecosystem

**Baseline** — The simulation-free wildlife level at a place and time: a pure function of the
**Region**'s **Habitat**, a slow seasonal cycle, and local noise. What wildlife "should" be
there absent players. The cold world is *computed*, never ticked.
_Avoid:_ Default, equilibrium (equilibrium implies a simulation settling; the Baseline is
evaluated, not settled).

**Disturbance** — A sparse, persisted, per-**Region** delta recording how players have pushed
wildlife away from **Baseline** (overhunting, …). It decays back toward zero over time — the
Region heals — so the live wildlife level is always Baseline plus a shrinking Disturbance.
_Avoid:_ Depletion (that is the Resource-node respawn mechanic), scar, damage.

**Gatherable** — The general category of harvestable world object that yields **Items** on
harvest; **Resource nodes** and **Carcasses** are its two kinds.

---

## Relationships

- A **World** is one **Overworld** plus zero-or-more live **Instances**.
- The **Overworld** is partitioned into a grid of **Chunks** — and, independently, into
  deterministic **Regions** (each with one **Habitat**).
- Dynamic simulation is partitioned by *interaction locality* into **Islands**, not by
  geography: the **Cartographer** assigns **Players** and hot **Chunks** to Islands and
  merges/splits them so any two entities that can interact share one Island.
- There is exactly one **Cartographer**; it assigns authority but does not simulate.
- A hot **Chunk**'s live state is owned by exactly one **Island** at a time; a Chunk is hot
  while some Island holds it and cold otherwise.
- An **Instance** has its own private Chunk grid, disjoint from the Overworld's; Instance
  Chunks hold only **Portals** and never emit to durable storage.
- A **Player** occupies one Overworld Chunk's area *or* one Instance at a time, and is
  simulated by exactly one Island. One username = one Player; there is no separate account or
  character roster.
- An **Overworld Chunk** holds zero-or-more **Resource nodes**, **Structures**, and
  **Portals**; a **Structure** belongs to the Chunk it sits in and is owned by its placing
  Player.
- Every **Resource node** and **Structure** has a **Footprint**; **Players** and **Portals**
  do not.
- Each **Player** has one **Inventory** of zero-or-more **ItemStacks**, each a quantity of one
  **Item**. Harvesting a **Gatherable** yields ItemStacks; building a **Structure** spends them.
- All durable reads/writes flow through the **Datastore**; **Islands** never touch durable
  storage directly. Instance state is in-memory only.
- An **NPC** is an Island actor like a **Player**; both feed one **Intent** per tick — a
  Player's from a session, an NPC's from its **Motivation**.
- An **NPC** has one Motivation; a Motivation has a fixed set of root **Needs**; each Need
  roots one **Behavioral chain** and carries one **Pressure**. Each tick every chain offers one
  **Bid**; arbitration (bias × Pressure) picks the winning **Goal**; the Goal expands to a
  **Plan**; the Plan's head **Action** resolves to the tick's Intent. **Actions** are a shared
  library owned by no Need.
- **NPCs do not anchor the Warm set** — only **Players** keep Chunks hot; NPCs are simulated
  only within Player-hot Chunks, and have **no persistent individual identity**: wildlife
  *materializes* from a Region's level when a Chunk warms and *dissolves* back into that
  Region's **Disturbance** when it cools.
- A place's live wildlife level = its **Region**'s **Baseline** now + the Region's decaying
  **Disturbance**. A Region's history shapes both the *population* spawned and its *temperament*
  (a depleted, high-Disturbance Region spawns hungry, aggressive animals; a healthy one, placid).
- A killed animal leaves a **Carcass**: **Players** harvest it for meat/hide, **NPCs** eat from
  it, rival predators contest it.

---

## Flagged ambiguities

- **Player vs Character** — collapsed to one concept (**Player**). Revisit if a character roster
  is ever wanted.
- **"Private" Instances** — Instances are *Party-scoped*, not owned. There is no per-Player
  private space in v1.
- **Cross-chunk collision** — resolved by the **Island** model: an Island owns every Chunk its
  members span, so collision is evaluated against the full local neighborhood, not one Chunk.
