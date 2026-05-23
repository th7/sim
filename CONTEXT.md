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
An ephemeral, private 2D region spawned on demand for a Party entering a dungeon. Lives in memory only — no persistence. Destroyed when the Party leaves or disconnects. In v1, dungeons are the only kind of Instance; no player housing, no persistent dungeons, no guild halls. Internally, an Instance is partitioned into its own private grid of **Chunks** — the same Chunk machinery as the **Overworld** (ECS, ticks, **Boundary crossing**, **Warm set**, **View window**) — distinct only by Registry scope, absence of persistence, and bounded extent.
_Avoid_: Dungeon (a dungeon is the *content* hosted by an Instance), private map, room.

**Party**:
A group of Players (1 or more) that enters an Instance together. The Instance is spawned for the Party and destroyed when the Party dissolves.
_Avoid_: Group, raid, team (group/raid/team have specific other-MMO meanings we may want later).

**Chunk**:
A fixed-size rectangular partition of the **Overworld**. The unit of ownership and spatial indexing.
_Avoid_: Tile (a tile would imply discrete movement, which we explicitly rejected), zone, region.

**Player**:
A human participant, identified by a chosen username. Also refers to the in-world entity they control — we deliberately do not distinguish Player from Character; one username = one in-world entity.
_Avoid_: Character, user, account, avatar.

**Resource node**:
A gatherable world object (tree, rock, ore vein, plant). Depletes when harvested and respawns on a timer. World state — not owned by any Player.
_Avoid_: Resource (ambiguous — also means inventory material), node (too generic).

**Structure**:
A persistent object placed in the **Overworld** by a Player (building, wall, crafting station, fence). Survives indefinitely until destroyed. Anchored to a specific **Chunk**.
_Avoid_: Building (only one kind of Structure), object, placeable.

**Portal**:
A fixed Overworld entity that marks the entry point to an **Instance**. Placed deterministically by worldgen — not built by **Players**, not stored in the **Structure** table. Anchored to a specific **Chunk**. Interacting with a Portal triggers **Instance entry**.
_Avoid_: Structure (a Portal is not a Structure — Structures are player-placed; see the Structure entry), gate, dungeon entrance.

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
The transition of a **Chunk** from cold to hot — a live GenServer holding state in memory. State is hydrated through the **Datastore**, which returns the freshest view by merging its **pending writes** with the durably-stored set. Triggered by player proximity.

**Chunk deactivation**:
The reverse — the **Chunk** emits its final state to the **Datastore** and terminates. The Datastore is responsible for the eventual flush to durable storage. Triggered by sustained absence of players.

**Boundary crossing**:
A **Player**'s entity exits the bounds of its owning **Chunk** and enters a neighbor **in the same realm** (i.e. an adjacent Chunk of the same **Overworld** or the same **Instance**). The entity is handed off from the source's process to the destination's; the Player's session updates its **Warm set** to the new center; the entity continues in the destination process on the next tick. **Instance entry** and **Instance exit** are *not* Boundary crossings — they cross realms.
_Avoid_: Migration (an implementation term — `ChunkMigration` is the module that performs the handoff; the event itself is a boundary crossing), chunk transfer, hop.

**Instance entry**:
A **Player**'s entity leaves the **Overworld** and enters an **Instance** by overlapping a **Portal**. Mechanically a process handoff like a **Boundary crossing**, but distinct: the Player's **Warm set** is torn down and a fresh one is built around the Instance's center, the **View window** switches to the Instance's topic space, and on disconnect any in-Instance state is lost.
_Avoid_: Portal travel, teleport, dungeon enter, zone-in.

**Instance exit**:
The reverse — a **Player**'s entity overlaps the Instance's return-**Portal** and returns to the **Overworld Chunk** they entered from, at (a small offset from) the Portal cell. The **Instance** is destroyed when no Players remain.
_Avoid_: Portal exit, dungeon leave, zone-out.

**Warm set**:
The set of **Chunks** a connected **Player**'s session keeps hot on their behalf — currently the 5×5 grid centered on the **Chunk** the Player occupies. A **Chunk** stays hot as long as at least one session has it in its warm set.
_Avoid_: Warm zone, warm radius (the radius is a parameter; the set is the concept).

**View window**:
The set of **Chunks** a connected client subscribes to for snapshot streams — currently the 3×3 grid centered on the **Chunk** the Player occupies. Strictly smaller than (or equal to, inside an **Instance**) the **Warm set**; the outer ring of the warm set is pre-activated to hide chunk-activation latency when the Player crosses a boundary into it.
_Avoid_: Visible chunks, subscription window, AOI (AOI is the general concept; the view window is our specific implementation).

**Datastore**:
The single in-memory persistence chokepoint per node. All durable reads and writes for the running world go through it. **Chunks** emit state changes via synchronous calls; the Datastore buffers them as **pending writes** and flushes to durable storage on its own cadence. Chunks hydrate through it too — it returns the freshest view by merging **pending writes** with the last-flushed DB state. Under overload it engages **backpressure** rather than dropping writes or crashing.
_Avoid_: Repo, persistence layer, cache, store. Don't say "actor" in domain language — that's its implementation, not its role.

**Pending writes**:
The **Datastore**'s in-memory buffer of state changes that have been emitted by **Chunks** but not yet confirmed durable. Per-key, last-write-wins; a delete is a tombstone entry that supersedes any prior upsert at the same key. An entry leaves pending only when its DB flush is confirmed.
_Avoid_: WAL (the buffer is not a log — it's a keyed map of effective state), write queue, dirty set.

**Backpressure**:
The **Datastore**'s overload-protection mode. When **pending writes** exceed a size threshold or any entry has aged past a time threshold, the Datastore stops replying to incoming write calls — caller `GenServer.call`s block. Upstream **Chunks** (and the **Players** whose verbs route through them) freeze. The mode clears when the Datastore drains — usually because the DB recovered, or an operator deployed a fix via hot code reload for a stuck flush. Parked callers then receive their replies in FIFO order and upstream resumes naturally.

## Relationships

- A **World** is composed of one **Overworld** and zero-or-more live **Instances**
- The **Overworld** is partitioned into a grid of **Chunks**
- Each **Chunk** is owned by exactly one process at a time (sharding)
- An **Instance** is partitioned into its own private grid of **Chunks**, scoped to that Instance and disjoint from the **Overworld**'s grid
- A **Player** exists in exactly one **Chunk** (if in the **Overworld**) or one **Instance** at a time
- A username uniquely identifies a **Player**; there is no separate account or character roster
- A **Chunk** holds zero-or-more **Resource nodes**, zero-or-more **Structures**, and zero-or-more **Portals**
- A **Structure** belongs to the **Chunk** it sits in; ownership is per-Structure (a Player owns the Structure)
- Each **Player** has exactly one **Inventory**; an **Inventory** holds zero-or-more **ItemStacks**; each **ItemStack** is a quantity of exactly one **Item**
- A **Resource node** yields one or more **ItemStacks** when harvested; a **Structure**'s build cost is expressed as one or more **ItemStacks** drawn from the placing Player's **Inventory**
- A **Chunk** is either hot (running) or cold (state in durable storage only)
- Each connected **Player** has a **Warm set** (kept hot) and a **View window** (snapshots subscribed); the View window is a strict subset of the Warm set
- A **Chunk** stays hot while it is in any session's **Warm set**; **Chunk deactivation** fires after the last interested session releases it
- All durable reads and writes flow through the **Datastore**; **Chunks** do not talk to durable storage directly
- **Instance** **Chunks** do not emit to the **Datastore** — Instance state is in-memory only

## Example dialogue

> **Dev:** "When a **Party** enters a dungeon, what happens to their **Chunk** subscriptions?"
> **Designer:** "They drop them. The **Party** is now in an **Instance** — the **Overworld** is irrelevant. When they leave the **Instance** they're placed back in the **Chunk** they entered from."
>
> **Dev:** "And if a **Player** disconnects mid-Instance?"
> **Designer:** "They leave the **Party**. If the **Party** is now empty, the **Instance** is destroyed."
>
> **Dev:** "What about a **Structure** they were standing on when they got pulled into the **Instance**?"
> **Designer:** "**Structures** are **Overworld**-only — there are no **Structures** inside **Instances**. The one in the **Overworld** doesn't move; the **Player** just leaves it behind."

## Flagged ambiguities

- "Player" vs "Character" — collapsed to a single concept (**Player**). Revisit if/when a roster feature is wanted.
- "Private" — earlier framing said "private Instances," but Instances are *Party-scoped*, not owned. There is no per-Player private space in v1.
