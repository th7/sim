# Plan

Build order for the game. Each phase ends in something runnable and demonstrable. See `CONTEXT.md` for domain language.

The plan is ordered to push *uncertainty* forward — the distributed-systems mechanics (migration, lazy lifecycle, distributed registry) are validated *before* gameplay content is built on top, so they can't quietly break content later.

## Phase 0 — Scaffolding

**Goal**: empty umbrella + frontend boot, one HTTP page served.

- Generate Elixir umbrella with three apps: `game_core`, `game_persistence`, `game_web`
- `game_web` is a Phoenix app, no Ecto, no LiveView (Channels only)
- `game_persistence` owns Ecto, the Repo, and migrations
- `game_core` is plain Elixir — no Phoenix, no Ecto deps
- Create `frontend/` as a Vite + TypeScript + Three.js project
- Wire dev: Vite runs on its own port; Phoenix proxies unknown paths to it
- Wire prod build: `mix assets.deploy` triggers `vite build` into `priv/static`
- Set up Postgres locally; `mix ecto.create` works

**Done when**: `mix phx.server` serves an empty Three.js scene at `localhost:3000` with HMR.

## Phase 1 — One Player in one Chunk

**Goal**: a single hard-coded **Chunk** runs as a GenServer; one **Player** connects, sees themselves, can move.

- `GameCore.Chunk` GenServer; one instance started under a `DynamicSupervisor`
- Player state held in chunk state (plain map; no ECS yet)
- Phoenix Channel topic `chunk:0:0`; client connects, sends `move` events, receives full snapshots
- Server tick: 20 Hz via `Process.send_after`
- Broadcast: every other tick (10 Hz), full snapshot of chunk entities
- Frontend: render a low-poly cube at the Player's reported position, accept WASD input, send to server, render server snapshots authoritatively (no prediction yet)

**Done when**: two browser tabs connect, both see each other move, server is authoritative.

## Phase 2 — ECS inside the Chunk

**Goal**: refactor chunk internals to ECS without changing observable behavior.

- Adopt ECSx (or hand-roll a minimal ECS over ETS) — decide here, document why in a comment
- Components for v1: `Position`, `Velocity`, `Renderable`, `PlayerControlled`
- Systems: `MovementSystem` (integrate velocity → position), `BroadcastSystem` (every 2nd tick, emit snapshot)
- Player input updates `Velocity`, not `Position` directly
- Keep snapshot format stable so frontend doesn't need to change

**Done when**: chunk behaves identically to phase 1, but internals are ECS. New entity types are now cheap to add.

## Phase 3 — Persistence

**Goal**: Player and world state survive a server restart.

- Ecto schemas: `Player` (username, last position, last chunk), `Structure` (chunk_x, chunk_y, owner_username, type, x, y, hp), `ResourceNode` (chunk_x, chunk_y, type, x, y, depleted_until)
- On socket connect: look up Player by username; create on first sight
- On disconnect: flush Player position to DB
- Periodic Player position flush every 5s (in case of crash)
- Chunk activation = SELECT all rows where `chunk_x=? AND chunk_y=?` for each relevant table; hydrate into ECS components
- Manual chunk deactivation: no-op for now (state already in DB for normalized tables)

**Done when**: kill the server, restart, your Player and the world look unchanged.

## Phase 4 — Multi-Chunk AOI

**Goal**: world has multiple Chunks. Players near boundaries see entities in neighboring Chunks.

- Chunk registry: `Registry` (local for now), keyed by `{chunk_x, chunk_y}`
- Spawn Chunk GenServers for a fixed 5×5 grid at startup (still no migration, no lazy lifecycle)
- Client subscribes to a 3×3 window of channels based on its starting Chunk
- Frontend merges 9 simultaneous snapshot streams, deduplicates entity IDs
- Window shift: when Player nears a Chunk edge, frontend subs to new neighbors and unsubs from stale ones (purely client-driven for now — server is unaware of "window")

**Done when**: walk to the edge of your starting Chunk, see a Player standing in the neighboring Chunk (which the server hosts in a different GenServer).

## Phase 5 — Chunk migration

**Goal**: Players cross Chunk boundaries cleanly.

- Each Chunk knows its 8 neighbors via the Registry
- On every tick, MovementSystem checks if any Player's `Position` exits the Chunk's bounds
- Two-phase migration:
  1. Source chunk calls `Chunk.migrate_in(dest, player_state)` (a synchronous GenServer call)
  2. Destination adds Player to its ECS, returns `:ok`
  3. Source removes Player and broadcasts `entity_left` to its subscribers
  4. Destination broadcasts `entity_entered` to its subscribers
- 5×5 warm radius: each Player's session tracks which Chunks must be hot; chunks outside the 5×5 are not yet relevant (still all 25 hardcoded chunks live anyway)
- Client doesn't need new code — the 3×3 sub window already covers the visual

**Done when**: walk continuously across multiple Chunk boundaries with no glitches, hiccups, or duplicate sightings.

## Phase 6 — Lazy chunk lifecycle

**Goal**: Chunks activate on demand and deactivate when idle.

- Chunk supervisor becomes truly dynamic (start chunks on first reference)
- Player session GenServer (`GameCore.Session`) per connected Player; owns the 5×5 warm set
- Session ensures all 25 warm Chunks are activated; activates new ones as the Player moves; releases interest in old ones
- Each Chunk tracks "interested sessions"; when the count drops to zero for N seconds, it deactivates (snapshot ECS state to Postgres, terminate)
- On reactivation: hydrate from Postgres (Phase 3 logic), resume tick
- Catch-up: `depleted_until` and similar time-based state catches up naturally on hydration

**Done when**: world feels infinite. Walk in one direction for 5 minutes — Chunks ahead activate, Chunks behind deactivate. Server memory stays flat.

## Phase 7 — Distributed BEAM

**Goal**: chunks distributed across multiple BEAM nodes.

- Add `libcluster` with the gossip strategy for local dev (3 nodes on one machine)
- Swap `Registry` for `Horde.Registry`
- Swap `DynamicSupervisor` for `Horde.DynamicSupervisor`
- Swap `Phoenix.PubSub` adapter to PG2
- Chunk spawning chooses a node via Horde's distribution strategy (consistent hashing on `{chunk_x, chunk_y}`)
- Migration handshake now potentially crosses nodes — the existing GenServer call works transparently but latency varies; add a tracing span around `migrate_in`
- Netsplit handling: accept that during a split, a Chunk may run on both sides. Document. Don't fix in v1.

**Done when**: run 3 BEAM nodes locally, players connect to any of them, can walk across chunks owned by different nodes without noticing.

## Phase 8 — Gameplay slice: gathering and building

**Goal**: the first real "game" content. Players gather and place Structures.

- Add `Inventory` component to Players
- Add `Gatherable` component to Resource Nodes; harvest action transfers items
- Build action: client sends `build` with type + position; server validates (in chunk, position clear, has materials), INSERTs into `structures` table, adds entity to chunk's ECS, broadcasts
- Destroy action: HP system on Structures, damage events, deletion on death
- Frontend: HUD for inventory (basic React or Svelte overlay on Three.js canvas — decide here)

**Done when**: log in, chop a tree, place a wall. Log out. Log back in. Wall is still there.

## Phase 9 — Instances

**Goal**: Party-spawned ephemeral dungeon Instances.

- Add `Party` GenServer (lives until last member leaves)
- Add `Instance` GenServer (one per active dungeon run); not in the chunk Registry
- Dungeon portal: a Structure in the overworld; interacting offers "enter as party"
- Entering: Players unsub from their chunk window, sub to the Instance's single topic
- Leaving / disconnection: Player rejoins their last-known overworld Chunk; Instance destroyed if Party empty
- No persistence — Instance state is pure in-memory

**Done when**: party of 2+, enter a dungeon, fight something, leave, instance is gone.

## Deferred

These are deliberately not in v1 — record them here so they're not forgotten:

- Auth, anti-cheat, public exposure, ops/observability
- Player housing, persistent dungeons, guild halls
- PvP (combat model exists, but no PvP-specific rules / safe zones / loot drops on death)
- Client-side prediction & reconciliation for own Player (Phase 1 uses authoritative snapshots only; smooth movement comes later via interpolation between snapshots and local prediction)
- Combat model (twitch / target-locked / ability-based) — decide before Phase 8 gameplay slice
- Progression (XP, levels, skills) — decide alongside combat
- Crafting recipes and stations — decide alongside building
- Asset pipeline / art direction — bootstrap with stock packs (Synty, Quaternius, Kenney)
