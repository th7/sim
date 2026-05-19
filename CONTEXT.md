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
An ephemeral, private 2D region spawned on demand for a Party entering a dungeon. Lives in memory only — no persistence. Destroyed when the Party leaves or disconnects. In v1, dungeons are the only kind of Instance; no player housing, no persistent dungeons, no guild halls.
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

**Chunk activation**:
The transition of a **Chunk** from cold (state-on-disk-only) to hot (live GenServer holding state in memory). Triggered by player proximity.

**Chunk deactivation**:
The reverse — snapshot the live state to durable storage and terminate the GenServer. Triggered by sustained absence of players.

## Relationships

- A **World** is composed of one **Overworld** and zero-or-more live **Instances**
- The **Overworld** is partitioned into a grid of **Chunks**
- Each **Chunk** is owned by exactly one process at a time (sharding)
- An **Instance** is not partitioned into **Chunks** (it's small enough to live in one process)
- A **Player** exists in exactly one **Chunk** (if in the **Overworld**) or one **Instance** at a time
- A username uniquely identifies a **Player**; there is no separate account or character roster
- A **Chunk** holds zero-or-more **Resource nodes** and zero-or-more **Structures**
- A **Structure** belongs to the **Chunk** it sits in; ownership is per-Structure (a Player owns the Structure)
- A **Chunk** is either hot (running) or cold (state in durable storage only)

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
