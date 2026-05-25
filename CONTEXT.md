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
