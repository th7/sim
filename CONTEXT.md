# Game

A cooperative, persistent, isometric, real-time world where players fight, craft, and gather. PvP is an eventual concern, not a v1 concern.

## Language

**World**:
The total game universe; the union of the **Overworld** and all live **Instances**.
_Avoid_: Server, realm, shard (these are implementation terms).

**Overworld**:
The single shared, persistent, free-positioned 2D space that all players inhabit together.
_Avoid_: Map, world map, overland.

**Instance**:
An ephemeral, private 2D region spawned on demand for a Party entering a dungeon. Lives in memory only — no persistence. Destroyed when the Party leaves or disconnects. In v1, dungeons are the only kind of Instance; no player housing, no persistent dungeons, no guild halls. Internally, an Instance is partitioned into its own private grid of **Chunks** and simulated by **Islands** the same way the **Overworld** is (ECS, ticks, the **Cartographer**, the **Warm set**, the **View window**) — distinct only by scope, absence of persistence, and bounded extent.
_Avoid_: Dungeon (a dungeon is the *content* hosted by an Instance), private map, room.

**Party**:
A group of Players (1 or more) that enters an Instance together. The Instance is spawned for the Party and destroyed when the Party dissolves.
_Avoid_: Group, raid, team (group/raid/team have specific other-MMO meanings we may want later).

**Chunk**:
A fixed-size rectangular partition of the **Overworld** — the unit of **Worldgen** determinism, persistence keying, and spatial indexing. A Chunk is *data*, not a running process: worldgen output plus durably-stored state, identified by coordinate. The live simulation of whatever occupies a Chunk's area is owned at runtime by an **Island**, never by the Chunk itself.
_Avoid_: Tile (a tile would imply discrete movement, which we explicitly rejected), zone, region. Don't call a Chunk a process or an owner of live state — that role is the **Island**'s.

**Island**:
The single runtime authority over a connected cluster of interacting dynamic entities (**Players** today; NPCs in a later phase) together with the **Chunks** their activity currently spans. An Island simulates all movement, collision, and combat among its members, so every interaction is resolved by one authority, locally. Islands are ephemeral and reshaped continuously by the **Cartographer** — created, merged, and split as entities move — so that any two entities able to interact are always inside the same Island. An Island sizes itself to activity: many tiny Islands in a quiet world, one large Island in a dense fight (whose ceiling is a single core).
_Avoid_: Zone, region, shard (an Island is interaction-scoped and dynamic, not a fixed spatial division), cell, party (a **Party** is a social grouping; an Island is a simulation authority that may hold unrelated entities who merely happen to be near each other).

**Cartographer**:
The single authority that maps the world into **Islands**: it assigns each **Player** (and each hot **Chunk**) to an Island, and creates, merges, and splits Islands as entities move. Being the sole arbiter of these changes, it serializes them — two Islands never race to merge each other. The Cartographer assigns but does not simulate; gameplay runs inside the Islands.
_Avoid_: Navigator (implies steering entities' movement; the Cartographer arbitrates authority, it does not move anyone), coordinator/manager (too generic), scheduler.

**Player**:
A human participant, identified by a chosen username. Also refers to the in-world entity they control — we deliberately do not distinguish Player from Character; one username = one in-world entity.
_Avoid_: Character, user, account, avatar.

**Resource node**:
A gatherable world object (tree, rock, ore vein, plant). Depletes when harvested and respawns on a timer. World state — not owned by any Player.
_Avoid_: Resource (ambiguous — also means inventory material), node (too generic).

**Structure**:
A persistent object placed in the **Overworld** by a Player (building, wall, crafting station, fence). Survives indefinitely until destroyed. Anchored to a specific **Chunk**. In v1, the only Structure type is a *wooden palisade* (the "wall"); cost is 5 **wood**.
_Avoid_: Building (only one kind of Structure), object, placeable.

**Portal**:
A fixed Overworld entity that marks the entry point to an **Instance**. Placed deterministically by worldgen — not built by **Players**, not stored in the **Structure** table. Anchored to a specific **Chunk**. Interacting with a Portal triggers **Instance entry**.
_Avoid_: Structure (a Portal is not a Structure — Structures are player-placed; see the Structure entry), gate, dungeon entrance.

**Footprint**:
The world-space shape an obstacle occupies. A **Player** carries a body circle; they cannot move their position so that the body would overlap any Footprint. Collision is one-way — the world blocks the Player, but the Player blocks neither the world nor other Players. A **Resource node**'s Footprint is the same whether the node is gatherable or depleted: harvesting a node does not open a path.
_Avoid_: Hitbox (implies combat / damage zones), bounding box (only one of the two Footprint shapes is rectangular), collider (implementation term).

**Item**:
A *kind* of gatherable, stackable substance — wood, stone, iron ore. Abstract: an Item is a type, never a quantity. Items are produced by harvesting **Resource nodes** and consumed when **Players** build **Structures**.
_Avoid_: Material (collides with "crafting material" once recipes exist), Resource (already forbidden — see Resource node).

**ItemStack**:
A typed quantity of one **Item** — e.g. `{wood, 14}`. The unit inside an **Inventory** and the unit in which harvest yields and build costs are expressed.
_Avoid_: Item (Item is the type, ItemStack is the quantity — keep them distinct).

**Inventory**:
The **ItemStacks** carried by a **Player**. Filled by harvesting **Resource nodes**; drained by placing **Structures**. Persists across sessions — what you carry at logout is what you carry on login.
_Avoid_: Bag, backpack, container (Container may earn a glossary entry later if a second kind of container appears — chest, bank — but v1 has only the Inventory).

**Chunk activation**:
The transition of a **Chunk** from cold to hot: an **Island** takes ownership of the Chunk's area and hydrates its state through the **Datastore**, which returns the freshest view by merging its **pending writes** with the durably-stored set. Triggered when an Island's entities move into range of the Chunk.

**Chunk deactivation**:
The reverse — when no **Island** holds a **Chunk** any longer, its final state is emitted to the **Datastore** (responsible for the eventual flush to durable storage) and it goes cold. Triggered by sustained absence of entities from its area.

**Instance entry**:
A **Player** leaves the **Overworld** and enters an **Instance** by overlapping a **Portal**. The Player is re-homed onto a fresh Instance-scoped **Island** built around the Instance's center, the **View window** switches to the Instance's space, and on disconnect any in-Instance state is lost.
_Avoid_: Portal travel, teleport, dungeon enter, zone-in.

**Instance exit**:
The reverse — a **Player**'s entity overlaps the Instance's return-**Portal** and returns to the **Overworld Chunk** they entered from, at (a small offset from) the Portal cell. The **Instance** is destroyed when no Players remain.
_Avoid_: Portal exit, dungeon leave, zone-out.

**Warm set**:
The set of **Chunks** an **Island** keeps hot — the area its entities occupy plus a surrounding margin, so an entity reaching the edge doesn't stall on a cold **Chunk**. A Chunk stays hot as long as some Island holds it.
_Avoid_: Warm zone, warm radius (the radius is a parameter; the set is the concept).

**View window**:
The area around a **Player** that their **Session** streams to the client. The server owns and drives it: the Session pulls the changed state in that area from the world read-model and pushes it to the client, which subscribes only to its own **Player** topic and renders whatever it receives. Contained within the region the Player's **Island** keeps hot (the **Warm set**), whose margin hides **Chunk activation** latency as the Player moves.
_Avoid_: Visible chunks, subscription window, AOI (AOI is the general concept; the view window is our specific implementation).

**Datastore**:
The single in-memory persistence chokepoint per node. All durable reads and writes for the running world go through it. **Islands** emit state changes via synchronous calls; the Datastore buffers them as **pending writes** and flushes to durable storage on its own cadence. Islands hydrate through it too — it returns the freshest view by merging **pending writes** with the last-flushed DB state. Under overload it engages **backpressure** rather than dropping writes or crashing.
_Avoid_: Repo, persistence layer, cache, store. Don't say "actor" in domain language — that's its implementation, not its role.

**Pending writes**:
The **Datastore**'s in-memory buffer of state changes that have been emitted by **Islands** but not yet confirmed durable. Per-key, last-write-wins; a delete is a tombstone entry that supersedes any prior upsert at the same key. An entry leaves pending only when its DB flush is confirmed.
_Avoid_: WAL (the buffer is not a log — it's a keyed map of effective state), write queue, dirty set.

**Backpressure**:
The **Datastore**'s overload-protection mode. When **pending writes** exceed a size threshold or any entry has aged past a time threshold, the Datastore stops replying to incoming write calls — caller `GenServer.call`s block. Upstream **Islands** freeze whole-mailbox — every **Player** in a frozen **Island**, not only the one whose verb triggered the park, freezes with it. The mode clears when the Datastore drains — usually because the DB recovered, or an operator deployed a fix via hot code reload for a stuck flush. Parked callers then receive their replies in FIFO order and upstream resumes naturally.

### NPCs and Motivation

**NPC**:
A non-human dynamic entity simulated as an actor inside an **Island**, exactly like a **Player** — it moves, collides, and (later) fights under the same single authority. The one difference from a Player is the source of its **Intent**: a Player's comes from a remote session, an NPC's is produced each tick by its **Motivation**.
_Avoid_: Mob, monster (not all NPCs are hostile), creature, bot (implies external automation), agent (an AI/implementation term).

**Intent**:
The per-tick movement/action input one actor hands its **Island** at tick start, read to integrate movement and resolve interactions. A **Player**'s Intent comes from their session; an **NPC**'s comes from its **Motivation**. This is the single seam both kinds of actor share.
_Avoid_: Input, command, keypress.

**Motivation**:
The system that produces an **NPC**'s **Intent** each tick: a fixed set of root **Needs**, each driving a **Behavioral chain**, arbitrated into one **Goal** that expands into a **Plan**. The NPC's analogue of a Player's session.
_Avoid_: AI, brain, behaviour tree (the model is explicitly not a behaviour tree).

**Need**:
A root motivator of an **NPC** — hunger, safety, shelter. A fixed set per kind of NPC. Each Need roots exactly one **Behavioral chain** and carries a **Pressure**. Needs have a static priority **bias** relative to one another (e.g. safety outranks hunger) before Pressure is applied.
_Avoid_: Drive, motive, urge, desire.

**Behavioral chain**:
The ordered sequence of progressively more strategic sub-goals that serve one root **Need** — e.g. hunger → eat → carry food → stockpile → secure a food source. Each tick the chain offers exactly one **Bid**.
_Avoid_: Behaviour tree, goal stack, plan tree.

**Bid**:
The single candidate a **Behavioral chain** offers each tick: its most-immediate *actionable* node (a node is actionable only when its preconditions hold, so the chain naturally climbs toward the strategic end as nearer needs are satisfied). Bids are what compete in **Goal** arbitration.
_Avoid_: Proposal, vote, candidate.

**Pressure**:
A per-**Need** measure of how chronically that Need has gone unmet. Pressure plays **no part within a chain**; its sole role is to modulate **inter-chain** arbitration — it can lift a chronically-unmet Need's **Bid** past the static need-priority bias, so a long-hungry NPC will trade away safety. It never selects *what* to do within a goal, only *which* Need owns the goal.
_Avoid_: Urgency (that is a node's immediate activation, not the accumulated term), stress, mood.

**Goal**:
The one **Need** an **NPC** is currently acting on — the **Bid** that wins arbitration (static need bias, modulated by **Pressure**). Expanded into a **Plan**.
_Avoid_: Objective, target, Intent (Intent is the per-tick output, not the objective).

**Plan**:
The most-immediate *actionable* sequence of **Actions** pursuing the current **Goal**, chosen by the *same* precondition-gated immediacy rule as a chain **Bid**. A Plan adapts to circumstance — a `feed` Goal yields `fight-to-hold` rather than `feed-calmly` when a threat blocks calm feeding — and bottoms out in per-tick **Intent**.
_Avoid_: Script, routine, behaviour.

**Action**:
A primitive in a **shared** library that **Plans** compose — move-to, eat, pick-up, attack, flee. Actions are owned by no **Need** or chain: the same Action (e.g. attack) can serve different **Goals** (a wolf fighting *for* food, not *for* safety).
_Avoid_: Verb (a Verb is a Player-initiated server command — harvest/build/damage; an Action is an NPC-Plan primitive, even where the two resolve to the same effect), skill, ability.

### The wild ecosystem

**Region**:
A deterministic territory of the **Overworld** — a Worley/Voronoi cell given by a pure function of position, independent of the **Chunk** grid. Each Region has a **Habitat**. Regions, not Chunks, are the unit the ecosystem **Baseline** and player **Disturbances** are keyed on.
_Avoid_: Zone, area, biome (biome names the **Habitat** type, not the territory).

**Habitat**:
The ecological type of a **Region** (meadow, forest, …) that fixes its baseline grass and wildlife levels.
_Avoid_: Biome (we say Habitat), terrain (terrain is the rendered ground; Habitat is the ecological role).

**Baseline**:
The simulation-free wildlife level at a place and time — a pure function of the **Region**'s **Habitat**, a slow seasonal cycle, and local noise. What wildlife "should" be there absent players. The cold world is *computed*, never ticked.
_Avoid_: Default, equilibrium (equilibrium implies a simulation settling; the Baseline is evaluated, not settled).

**Disturbance**:
A sparse, persisted, per-**Region** delta recording how players have pushed wildlife away from **Baseline** (overhunting, …). It decays back toward zero over time — the Region heals — so the live wildlife level is always Baseline plus a shrinking Disturbance.
_Avoid_: Depletion (that is the Resource-node respawn mechanic), scar, damage.

**Carcass**:
The perishable remains of a killed animal — a **Gatherable** (like a **Resource node**) that yields meat/hide **Items**. Contestable: both **NPCs** and **Players** harvest it, and rival predators fight to hold it. Perishes on a timer if left unconsumed.
_Avoid_: Corpse (implies a **Player** death — Players don't die in v1), loot, drop, kill.

## Relationships

- A **World** is composed of one **Overworld** and zero-or-more live **Instances**
- The **Overworld** is partitioned into a grid of **Chunks**
- Dynamic simulation is partitioned by *interaction locality* into **Islands**, not by geography: the **Cartographer** assigns **Players** and hot **Chunks** to Islands and merges/splits them so any two Players who can interact share one Island
- There is exactly one **Cartographer**; it assigns authority but does not simulate
- A hot **Chunk**'s live state is owned by exactly one **Island** at a time
- An **Instance** is partitioned into its own private grid of **Chunks**, scoped to that Instance and disjoint from the **Overworld**'s grid
- A **Player** occupies one **Chunk**'s area (in the **Overworld**) or one **Instance** at a time, and is simulated by exactly one **Island**
- A username uniquely identifies a **Player**; there is no separate account or character roster
- An **Overworld Chunk** holds zero-or-more **Resource nodes**, zero-or-more **Structures**, and zero-or-more **Portals**; an **Instance Chunk** holds only **Portals** (no **Resource nodes**, no **Structures**)
- A **Structure** belongs to the **Chunk** it sits in; ownership is per-Structure (a Player owns the Structure)
- Every **Resource node** and every **Structure** has a **Footprint**; **Players** and **Portals** do not. A **Player** cannot move to a position where their body would overlap any Footprint
- Each **Player** has exactly one **Inventory**; an **Inventory** holds zero-or-more **ItemStacks**; each **ItemStack** is a quantity of exactly one **Item**
- A **Resource node** yields one or more **ItemStacks** when harvested; a **Structure**'s build cost is expressed as one or more **ItemStacks** drawn from the placing Player's **Inventory**
- A **Chunk** is either hot (held and simulated by an **Island**) or cold (state in the **Datastore** only)
- An **Island** keeps a **Warm set** of **Chunks** hot; each connected **Player**'s **Session** streams a **View window** to its client, contained within the region its Island holds hot
- A **Chunk** stays hot while some **Island** holds it; **Chunk deactivation** fires when no Island does
- All durable reads and writes flow through the **Datastore**; **Islands** do not talk to durable storage directly
- **Instance** **Chunks** do not emit to the **Datastore** — Instance state is in-memory only
- An **NPC** is an actor simulated by exactly one **Island**, the same as a **Player**; both feed their Island one **Intent** per tick. A Player's Intent comes from a session, an NPC's from its **Motivation**
- An **NPC** has one **Motivation**; a **Motivation** has a fixed set of root **Needs**; each **Need** roots exactly one **Behavioral chain** and carries one **Pressure**
- Each tick, every **Behavioral chain** offers one **Bid**; arbitration (static need bias modulated by **Pressure**) picks one winning Bid as the **Goal**; the Goal expands to a **Plan**; the Plan's head **Action** resolves to the tick's **Intent**
- **Actions** are a shared library, owned by no **Need**; a **Plan** for any **Goal** may compose any Action
- An **NPC** does **not** anchor the **Warm set** — only **Players** keep **Chunks** hot; NPCs are simulated only within Player-hot Chunks
- The **Overworld** is also partitioned, independently of **Chunks**, into deterministic **Regions**; each Region has a **Habitat** fixing its ecosystem **Baseline**
- A place's live wildlife level = its **Region**'s **Baseline** at the current time plus the Region's decaying **Disturbance**; a **Chunk** warming turns that level into seeded spawn chances, and warm hunting/grazing writes the Region's **Disturbance**
- An **NPC** has no persistent individual identity across a cold/warm cycle: it materializes from a **Region**'s wildlife level when a Chunk warms and dissolves back into that Region's **Disturbance** when the Chunk cools
- A killed animal leaves a **Carcass** (a **Gatherable**); **Players** harvest it for meat/hide **Items** (feeding the crafting economy), **NPCs** eat from it to satisfy hunger, and rival predators contest it
- An **NPC** may carry food **Items** in an **Inventory** (as **Players** do) and stockpile them while pursuing the strategic end of its hunger chain; such stockpiles are in-session and dissolve into the **Region**'s **Disturbance** on cooldown
- An **NPC** materializing into a warming **Chunk** spawns with initial **Needs**/**Pressure** derived deterministically from its **Region**'s current wildlife level: a depleted, high-**Disturbance** Region spawns hungry, high-pressure (aggressive) animals; a healthy Region spawns placid ones. The Region's history shapes both population *and* temperament

## Example dialogue

> **Dev:** "When a **Party** enters a dungeon, what happens to their place in the world?"
> **Designer:** "Their **Players** are re-homed onto fresh **Islands** scoped to the **Instance**. The **Overworld** is irrelevant to them until they leave, at which point they're placed back in the **Chunk** they entered from."
>
> **Dev:** "And if a **Player** disconnects mid-Instance?"
> **Designer:** "They leave the **Party**. If the **Party** is now empty, the **Instance** is destroyed."
>
> **Dev:** "What about a **Structure** they were standing on when they got pulled into the **Instance**?"
> **Designer:** "**Structures** are **Overworld**-only — there are no **Structures** inside **Instances**. The one in the **Overworld** doesn't move; the **Player** just leaves it behind."

## Flagged ambiguities

- "Player" vs "Character" — collapsed to a single concept (**Player**). Revisit if/when a roster feature is wanted.
- "Private" — earlier framing said "private Instances," but Instances are *Party-scoped*, not owned. There is no per-Player private space in v1.
- Cross-chunk collision — *resolved* by the **Island** model: an Island owns every **Chunk** its **Players** span, so collision is evaluated against the full local neighborhood rather than a single Chunk. The old "clip-and-stop" artifact at Chunk boundaries no longer arises.
