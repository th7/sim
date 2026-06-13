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

## Time & resolution

**Tick** — The atomic unit of simulated time; all world change happens *in* some named
tick, and every Intent is **processed with its tick** — never deferred to the next.
_Design promise:_ a tick's outcome is a **pure function of the locked Intents** it
was given — simultaneity inside a tick is resolved by a fixed neutral law (movement
first, then **Actions** judged at final positions — *arrival-into beats placement*, so a
placed Structure can never appear under a body and is solid from the next tick),
**never by network arrival order**; replaying the Intent log reproduces history
bit-identically. Facts of a tick may *resolve and emit eagerly* (before every input has
arrived) when no missing input could affect them — eagerness is scheduling, never
semantics — and **an emitted fact is final**: authority never revises what it has
published.
_Avoid:_ Frame (a client render concept), step, cycle.

**Input frame** — The per-**Tick** unit of a **Player**'s movement **Intent**: one
`{seq, direction}` the session sends each client tick while moving (plus a final
zero-frame on release; silence while idle). Its **seq** is a per-session counter that
advances *only when a frame is sent* — an index into the Player's own input history, not
a clock. The server consumes exactly one per Tick in order and acks the last consumed
seq; the **Mirror** replays its unacked tail on that anchor (own-position prediction is
bit-identical to the authority's integration). The Player's displayed position at any
moment is *all frames through the latest seq, integrated* — so the seq an **Action** carries
names the exact Input frame whose post-integration position the Player was looking at:
the **press frame** of lawful-render judging.
_Avoid:_ Keypress (an Input frame is per-tick renewed Intent, not a key event), packet,
message; seq alone (the seq identifies an Input frame — keep the thing and its index
distinct).

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
_Design promise:_ every Intent is **bound to the named Tick it applies to** and locks
exactly once (idempotent against retransmission, never revisable). Intent is also
*perishable* — it must be continuously renewed by its source. A **Motivation** renews
natively every tick; a session renews for as long as it is live, and when its renewals
stop, the Intent expires (after a short grace) to an empty lock and the Player stands
still. A stalled or vanished session never leaves a Player acting on stale Intent — and
never holds the world's resolution hostage beyond that grace.
_Avoid:_ Input, command, keypress.

**Action** — A **Player**-initiated world-changing command — *harvest*, *build*, *damage* —
resolved by the Player's **Island** inside the tick, fire-and-forget: outcomes arrive
asynchronously as world deltas or a rejection. The Player counterpart of an NPC-Plan
**Decision**, even where the two resolve to the same effect.
_Design promise:_ an entity-directed Action acts on the **Target**'s *identity*, never on a
remembered place, and resolves at the Tick the Player pressed in. Its *eligibility* (range)
is judged in the **press frame** — the Player's own exact position against the Target's
**Lawful render** — so what the screen lawfully showed in range, the Island honors. The
forgiveness is *continuous-only*: liveness, depletion, yields, and existence are always
judged in the authoritative present (a stale screen never resurrects or double-pays), and
effects always land at the resolve Tick — eligibility is forgiven, time is not.
_Avoid:_ Verb (the retired name for this); Decision (an NPC-Plan primitive); command, ability, skill.

**Target** — The one world entity a **Player** has designated to receive their next
entity-directed **Action**. Targetable: **Gatherables**, **Structures**, **NPCs**. Not
targetable: **Players** (no PvP in v1), **Portals** (entry is by overlap, not an Action).
*Build* is placement at a cell, not entity-directed, so it never involves a Target.
_Design promise:_ a Target is *sticky observation* — it persists until the Player explicitly
clears it, designates another, or the entity ceases to be visible (despawn, leaving the
**View window**, a world transition). Distance never clears it, and a depleted **Resource
node** stays targeted; range and state gate the *Action*, not the Target.
_Avoid:_ Selection (HUD-flavored), focus, lock; Goal's avoided alias "target" (NPC-internal)
is unrelated.

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
_Avoid:_ Repo, persistence layer, cache, store. Don't say "actor" in the actor-model
(concurrency) sense in domain language; informally "actor" means a Player-or-NPC — an
entity that feeds an **Intent** per tick.

**Backpressure** — The system's overload-protection promise: when the **Datastore** cannot
keep up, affected **Players** *freeze* (their inputs stall) rather than the system losing their
state or crashing; play resumes when the Datastore recovers. The freeze is **whole-world** — there is one **Datastore**, so everyone sharing that single
persistence authority freezes together (the freeze is keyed on the Datastore, not on any one
**Island**). The mechanism is engineer-owned.
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

**Plan** — The most-immediate *actionable* sequence of **Decisions** pursuing the current
**Goal**, chosen by the same precondition-gated immediacy rule as a chain **Bid**, and adapting
to circumstance (a `feed` Goal yields `fight-to-hold` when a threat blocks calm feeding). Bottoms
out in per-tick **Intent**.
_Avoid:_ Script, routine, behaviour.

**Decision** — A primitive in a **shared** library that **Plans** compose — move-to, eat,
pick-up, attack, flee. Owned by no Need or chain: the same Decision can serve different Goals (a
wolf fighting *for* food, not *for* safety). The NPC counterpart of a Player **Action**.
_Avoid:_ Action (a Player-initiated server command — harvest/build/damage; a Decision is
an NPC-Plan primitive, even where the two resolve to the same effect), skill, ability.

**Demeanor** — How an **NPC** outwardly carries itself: the observer-facing classification
of its committed **Decision**, one of **Calm**, **Feeding**, **Aggressive**, or **Fleeing**.
A projection made *for* observers, deliberately coarser than the Decision itself — each
Demeanor changes what a watching **Player** should do; nothing finer is actionable. Authored
by the **Island** (never speculated by the **Mirror**).
_Avoid:_ Activity, behavior, state (all collide with the Decision/Goal/Plan family), mood
(mood suggests the inner Pressure; Demeanor is the outward read).

**Health** — How wounded a damageable **NPC** is, from unhurt to dead; reaching zero kills
the animal and leaves a **Carcass**. Observers never read an exact number: a watching
**Player** sees Health only as one of three bands — **Unhurt**, **Wounded**, **Critical**.
**Players** have no Health in v1 — Players don't die.
_Avoid:_ Condition (vague — Health is the only condition an NPC has), hit points (the
mechanism's unit, not the concept), health bar (a display mechanism we deliberately don't
use).

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

## Client & display

**UI element** — Anything the client can draw: the HUD panels *and* the in-world renderings
of entities (**Players**, **Resource nodes**, **Structures**, **Portals**, **NPCs**,
**Carcasses**) and overlays. Deliberately broader than typical usage — a rendered wolf is a
UI element.
_Avoid:_ Widget, control (imply HUD-only); component (web connotation).

**Action button** — The single context-sensitive **UI element** through which a **Player**
issues entity-directed **Actions**: it acts on the current **Target**, and which Action it
issues follows from what the Target *is* (a **Gatherable** → *harvest*; a **Structure** or
**NPC** → *damage*). Inert when no Target exists.
_Design promise:_ its readiness hint is *truthful by construction* — it reads the **Lawful
render**, the same frame the **Island** judges in, so a lit button is refused only for
discrete staleness (the thing no client can know).
_Avoid:_ Verb button (the retired name); interact key, hotkey.

**Target marker** — The diegetic in-world annotation showing which entity is the current
**Target** — and the *only* Target display: there is deliberately no HUD target frame, no
name plate, no health readout. **Demeanor** and banded **Health** stay readable from the
entity itself; the marker adds only "this one."
_Avoid:_ Target frame, unit frame, nameplate (the mechanisms we deliberately don't use).

**Showcase** — The client utility that displays every **UI element** in every
appearance-affecting state, for manual visual verification on a real display.
_Design promise:_ completeness — a new kind of drawable thing cannot be added without the
Showcase displaying it. The mechanism that keeps the promise is engineer-owned.
_Avoid:_ Gallery (implies a widget library), demo (implies audience-facing).

**Mirror** — The client's non-authoritative, speculative simulation of its Player's **View
window**: the **Island**'s own movement integration, fed by **Intents** — its own Player's
locally and immediately, every other actor's as last received from the server — and
continuously overridden by authoritative state as it arrives. The Mirror decides nothing;
every divergence is temporary and resolves in the authority's favor.
_Design promise:_ the Mirror speculates **continuous state only** (movement) and never
speculates discrete events — spawns, despawns, yields, depletion, placement reach it solely
as authoritative fact. And its speculation is bounded: the Mirror's **Lead** never exceeds a
fixed bound; at the bound it freezes whole rather than speculating further (the client-side
face of **Backpressure**). The Mirror is *born frozen* — at login, relocation, and Instance
entry/exit alike, it speculates only from an authoritative baseline.
_Avoid:_ Island (an Island is the authority; the Mirror has none), shadow/replica (implies
a faithful copy; the Mirror knowingly speculates), client prediction (names the technique,
not the thing).

**Lead** — How far ahead of the last authoritative state the **Mirror** has speculated,
measured in ticks. Deliberate, not error: a healthy connection always carries some Lead.
Bounded by construction — at the bound the Mirror freezes until authority catches up.
_Avoid:_ Drift (implies error), lag (the thing Lead compensates for), prediction window.

**Frontier** — A session's continuously-asserted statement of the last authoritative
**Tick** it has incorporated — asserted on every input frame, not per action, so it is a
*standing persona*, never a per-press claim.
_Design promise:_ the Frontier is **monotone** (never retreats), **never-future** (naming
an undelivered tick is proof of cheating, not error), and **Lead-bounded** (a session
asserting staleness must also freeze its own inputs, exactly as an honestly lagged
**Mirror** would — pretending to have lag costs what lag costs). Feigned staleness within
those laws is indistinguishable from honest lag *by design*, and bounded by the same
constant.
_Avoid:_ Ack (transport flavor), client tick, last-seen.

**Lawful render** — What the **Mirror** algorithm *necessarily displayed* given a session's
**Frontier**: the authoritative state at the Frontier, speculated forward by the asserted
**Lead** under the shared integrator. Lawful because the authority can recompute it
bit-for-bit from data it itself delivered — no client-supplied geometry is ever trusted.
The frame in which an entity-directed **Action**'s eligibility is judged.
_Avoid:_ Client view (unverifiable), claimed render, screenshot.

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
  **Plan**; the Plan's head **Decision** resolves to the tick's Intent. **Decisions** are a shared
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
- Every **NPC** presents two independent observer-facing axes: a **Demeanor** (the outward
  read of its committed **Decision**) and a banded **Health**. Both are authoritative facts
  from the **Island** — the **Mirror** never speculates either.
- The client runs one **Mirror** of its Player's **View window**. The Mirror speculates ahead
  of authoritative state by at most its **Lead** bound, decides no interaction, and yields to
  the **Island**'s authority on every divergence.

---

## Flagged ambiguities

- **Player vs Character** — collapsed to one concept (**Player**). Revisit if a character roster
  is ever wanted.
- **"Private" Instances** — Instances are *Party-scoped*, not owned. There is no per-Player
  private space in v1.
- **Cross-chunk collision** — resolved by the **Island** model: an Island owns every Chunk its
  members span, so collision is evaluated against the full local neighborhood, not one Chunk.
