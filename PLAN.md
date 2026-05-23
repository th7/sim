# Plan

Build order for the game. Each phase ends in something runnable and demonstrable. See `CONTEXT.md` for domain language.

The plan is ordered to push *uncertainty* forward — the chunk-locality mechanics (boundary crossing, lazy lifecycle) are validated *before* gameplay content is built on top, so they can't quietly break content later. Cross-node distribution is deliberately *not* in v1; see the Deferred section for why.

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

## Phase 6.5 — Dev mode

**Goal**: a toggleable per-client overlay that visualizes Chunk lifecycle, the **Warm set**, and the **View window**, plus a small numeric HUD.

- Activation: URL param `?dev=1` sets initial state; backtick key toggles at runtime. When off, the `dev:stats` Channel is not joined and the overlay group is hidden.
- Server: new `dev:stats` Phoenix Channel. New `Chunk.dev_status/1` returning `{lifecycle, idle_ms_remaining, entity_count, interest_count}` — pure read, no behavior change on the gameplay tick. New `GameCore.Sessions.count/0` helper.
- Stats tick: channel handler runs a 1 Hz timer per dev client; resolves the Player's current Chunk via `Sessions.whereis`/`Session.current_chunk`, walks the 7×7 around it, pulls `dev_status` from every hot Chunk in the region, and combines with `Registry.count(GameCore.Chunks)` + `Sessions.count()`. One `stats` event per tick.
- Client overlay: a `THREE.Group` of ground-plane fills + borders + coord-label sprites. Fill encodes lifecycle (hot / idle-armed with shrinking bar / cold); border encodes the client's relationship (owner thick / view-window solid / warm-only thin / outside-warm dashed). Y-stack: GridHelper @ 0, fills @ 0.005, borders @ 0.01, labels @ 0.02.
- Client HUD: HTML `<div id="dev-hud">` in the top-left, monospace, listing username / world pos / chunk / nearby / active / total. Nearby updates at snapshot rate (10 Hz) from the client's existing merged `channelSnapshots`; the global counts update from the 1 Hz `stats` push.
- Tests: ExUnit covers `Chunk.dev_status/1` shape and the `dev:stats` Channel join/push. Playwright `phase6_5-devmode.spec.ts` toggles dev mode on, asserts the HUD appears, asserts `nearby` matches `__game.players()` length, asserts the `devOverlay` group is present in the scene.

**Done when**: toggle dev mode in a running game; the 7×7 grid around your Player is colored by lifecycle, bordered by relationship, and labeled with `{cx, cy}`. Walk away from a chunk and watch its fill turn yellow with a shrinking bar, then disappear when it deactivates. HUD numbers stay coherent with the world.

## Phase 7 — Distributed BEAM (deferred)

Originally planned as the cross-node distribution layer; deferred during the Phase 6.5 → Phase 8 transition. See the Deferred section for the rationale. The phase number is preserved (rather than renumbering Phase 8/9) so existing commits and code references stay legible.

## Phase 8 — Gameplay slice: gathering and building

**Goal**: the first real game content. Players harvest **Items** and place **Structures**; placed Structures persist; placed Structures can be damaged and destroyed.

Phase begins with a small foundational refactor — positions across `game_core`, `game_persistence`, the snapshot wire, and the frontend boundary switch from floats to scaled integers. Done first so the new gameplay verbs reason in integer terms from the start.

- **Integer positions, scale = 1000.** 1 world unit = 1000 sub-units. `Position`, `Velocity`, `ChunkGeometry`, and `MovementSystem` become integer-typed; three migrations convert float columns to integer on `players`, `structures`, `resource_nodes`. Snapshot payload emits integers; frontend divides by 1000 at the channel boundary so Three.js stays float-native. Interaction checks compare squared integer distances to avoid `sqrt`.

- **Closed catalogue.** Compile-time enums: `Item` :: `:wood`, Resource node type :: `:tree`, Structure type :: `:wall`. `GameCore.Item.valid?/1` for Inventory key validation; `GameCore.Structure.Catalogue` exposes `cost/1` (`:wall → [{:wood, 5}]`) and `max_hp/1` (`:wall → 100`). Damage per click is a fixed `25`.

- **Worldgen for Resource nodes.** `GameCore.Worldgen.resource_nodes({cx, cy})` is a pure deterministic function returning the positions of all trees in a chunk. The `resource_nodes` table is repurposed as a **depletion-state cache**: a row exists iff a node is currently depleted; identity is the spatial signature `(chunk_x, chunk_y, type, x, y)`; `depleted_until` is the only mutable column. The chunk GenServer is the sole writer for its own rows and reconciles via DELETE-then-INSERT inside a single transaction on each heartbeat, so duplicates can't arise without a unique constraint. A partial index on `depleted_until WHERE depleted_until IS NOT NULL` backs the pruner.

- **Resource node ECS shape.** `Position`, `Renderable`, and exactly one of `Gatherable` or `Depleted` (mutually exclusive). Hydration on chunk activation: call Worldgen, LEFT JOIN the depletion cache, add components accordingly; for each currently-depleted node schedule a respawn via `Process.send_after/3` for the remaining time; nodes whose `depleted_until` has already passed hydrate as Gatherable directly.

- **Inventory.** New `GameCore.Components.Inventory` — `defstruct items: %{}` — atom-keyed, validated against `Item.valid?/1`. New `players.inventory :: jsonb default '{}'` column; string-keyed JSON at rest, atom-keyed in memory, converted at the Repo boundary. Unbounded in v1. Hydrated on chunk join alongside position; flushed on the existing paths (5s heartbeat, leave, terminate).

- **Snapshot extension.** Parallel keys: `%{players: …, resource_nodes: …, structures: …}`. Resource node wire id = `"<type>:<x>:<y>"` (e.g. `"tree:5000:8000"`); Structure wire id = stringified DB id. Depleted nodes stay in the snapshot with `depleted: true` so the client can render stumps.

- **Self event.** New per-owner PubSub topic `"self:<username>"` subscribed only by that player's owner channel. Chunk publishes `{:self, %{inventory: %{...}}}` to it on inventory change; channel pushes a `self` event to the client.

- **Verbs.** All three use one constant `@interact_range_sq = 1_000_000` (1.0 world unit, in squared sub-units).
  - `harvest %{x, y}` — validate target is a Gatherable in this chunk within range; transfer `{wood, 1}`; flip to Depleted with `depleted_until = now + 30s`; schedule respawn; broadcast; publish self event. Async persistence rides the heartbeat. A background `GamePersistence.DepletionPruner` GenServer sweeps `WHERE depleted_until < now()` on its own cadence; chunk hydration also skips past-due rows defensively so the pruner is hygiene, not load-bearing.
  - `build %{type, x, y}` — validate in-chunk, cell empty (1.0u grid-snap, 1×1 footprint, no rotation), has materials. Single `Repo.transaction` (INSERT structure + UPDATE player inventory); on commit add to ECS; broadcast; publish self event. On any failure no state changes; client gets `{:error, reason}`.
  - `damage %{x, y}` — validate Structure at cell, player in range. Decrement HP by 25. If HP > 0 broadcast updated snapshot; if HP ≤ 0 `Repo.transaction` to DELETE the row, remove from ECS, broadcast. No material refund. Anyone can damage anyone's Structure (no PvP rules yet).

- **Frontend.** Plain DOM HUD — `<div id="hud">` in the top-right, monospace, inventory counts; updates on `self` push. Click handler raycasts and dispatches target-inferred: Gatherable → `harvest`, Structure → `damage`, empty cell within range AND player has materials → `build` (a translucent ghost cube renders at the snap-cell while these conditions hold). No hotbar.

- **Tests.** ExUnit for Item / Catalogue / Worldgen pure-function tests; all three verbs (happy paths + rejection cases: out-of-range, depleted, cell-occupied, insufficient materials, no-target); build atomicity (a forced DB failure must leave the Inventory untouched); respawn-on-hydration (depleted-then-time-passed hydrates as Gatherable; in-progress depletion gets a timer for the remaining time); Inventory round-trip across logout. Playwright `phase8.spec.ts`: chop a tree (HUD updates), place a wall (snapshot includes it, inventory decremented), damage to destruction (wall disappears), restart Phoenix, log back in, walls and inventory intact.

**Done when**: log in, chop a tree (HUD shows wood), place a wall, damage another player's wall to destruction. Log out. Log back in. Your Inventory and any surviving walls are exactly as you left them.

## Phase 9 — Instances

**Goal**: solo-player, ephemeral dungeon **Instances** — walk in via a **Portal**, walk around inside, walk back out. No combat, no Resource nodes, no in-Instance state changes; Phase 9 delivers the *container*, not content. **Parties** are deliberately deferred — solo only for v1.

Phase begins with two foundational refactors so the Instance work doesn't have to special-case channel topology or Chunk lookup.

- **`Chunks.whereis/1` → `/2` realm parameter.** One global `GameCore.Chunks` Registry keyed by `{realm, cx, cy}` where `realm :: :overworld | {:instance, id}`. All existing call sites pass `:overworld` explicitly. `Chunk.start_link/1` registers under the tagged key. `Session.state` gains `realm`; `current_chunk` is renamed `coord`. `WarmSet` becomes realm-scoped — a realm transition tears down and rebuilds the warm set rather than recentering it.

- **`PlayerChannel` extraction.** A new persistent `player:<username>` channel joined once per socket — hosts the Session, receives all input verbs (move/harvest/build/damage), subscribes to `self:<username>`. `ChunkChannel` is reduced to a single-responsibility observer-only snapshot pipe (no role param, no Session ownership). The Channel layer becomes realm-agnostic: realm transitions are a `relocated` push on the persistent player channel, with the new 9-topic list; the player channel doesn't restart.

Then the Instance work itself:

- **Portal as a new entity type.** Not a Structure (Structures are player-placed; Portals are not). New `Portal` component with a `direction :: :into_instance | :out_of_instance` field. `GameCore.Worldgen.portals/1` is a pure deterministic function returning the positions of all Portals in a chunk — v1 places exactly one `{:into_instance, :dungeon}` Portal at the center `(8000, 8000)` of Overworld chunk `{0, 0}` and `[]` everywhere else. Chunks hydrate Portals on activation alongside Resource nodes. Snapshot extension: new `portals: %{...}` key parallel to `resource_nodes` / `structures`; wire id format `"portal:<type>:<x>:<y>"`. No DB rows — Portals are stateless.

- **Instance topology — Chunk-shaped, 3×3 grid.** An Instance is internally partitioned into its own private grid of **Chunks**, reusing `GameCore.Chunk` (ECS, ticks, Boundary crossing, Warm set, View window) but with realm `{:instance, id}` and the **Null** ChunkRepo. The grid is exactly 3×3 in v1, so the View window equals the Instance — the Player always sees the whole dungeon. `MovementSystem` gains a per-host `bounds :: nil | {x_min, y_min, x_max, y_max}` field; `nil` for Overworld (unbounded), the integer sub-unit rect of the 3×3 for Instance. Out-of-bounds movement is **clamped** at the perimeter (no migration attempt, no error).

- **Per-Instance supervision.** New `GameCore.Instances` module + `InstancesSupervisor` (DynamicSupervisor). `Instances.start_new()` spawns a per-Instance `DynamicSupervisor` (`:one_for_all`) which in turn starts the 9 chunks; returns `{id, instance_sup_pid}`. `Instances.terminate(id)` stops the per-Instance supervisor — all 9 chunks die in dependency order, Registry entries clean up automatically. Identity is `System.unique_integer([:positive, :monotonic])`.

- **`InstanceWorldgen.portals/1` for the return-Portal.** Pure function placing one `{:out_of_instance, :dungeon}` Portal at the center of chunk `{1, 1}` of the Instance grid. Instance chunks hydrate it on activation, same path as Overworld Portals.

- **Movement-driven entry.** After `MovementSystem` integrates Position, each Chunk runs a Portal-overlap check: for every (Player, Portal-at-same-cell) pair, fire `Session.enter_instance(session, return_to)` (Overworld → Instance) or `Session.exit_instance(session)` (Instance → Overworld). The Chunk publishes the intent; the Session is the lifecycle owner.

- **Entry sequence.**
  1. Overworld chunk `A` detects overlap, calls `Session.enter_instance(session, {:overworld, A_coord, A_portal_pos})`.
  2. Session calls `Instances.start_new()` → fresh per-Instance supervisor + 9 Instance chunks with Null repo.
  3. Session calls `ChunkMigration.cross(eid, A_coord, {{:instance, id}, {1, 1}}, components, NullRepo)`. Position is **overridden by the destination** Instance center chunk to a hardcoded spawn point one cell off the return-Portal (avoid immediate re-trigger).
  4. Session updates state: `realm: {:instance, id}, coord: {1, 1}, return_to: {:overworld, A_coord, A_portal_pos}`.
  5. Session tears down Overworld WarmSet, builds an Instance WarmSet covering all 9 Instance chunks.
  6. Session signals `PlayerChannel` with `{:relocated, %{realm: {:instance, id}, coord: {1, 1}, topics: [...]}}`; channel pushes `relocated` to the client.
  7. Client unsubs the 9 Overworld snapshot topics, subs the 9 Instance topics `instance:<id>:chunk:0:0` … `instance:<id>:chunk:2:2`.

- **Exit sequence.** Symmetric. Instance chunk's overlap check fires `Session.exit_instance(session)`; Session migrates the entity back to the cached `return_to` chunk with Position overridden to `A_portal_pos + one-cell offset`; WarmSet rebuilt around the Overworld entry chunk; `Instances.terminate(id)` stops the per-Instance supervisor; client cycles snapshot subscriptions back to the 9 Overworld topics.

- **Disconnect mid-Instance.** `Session.terminate/2` extended: if `realm` matches `{:instance, id}`, call `Instances.terminate(id)` **before** `WarmSet.release_all` — eager teardown, no ghost-Instance left to idle out. The DB row for the Player still holds the pre-entry Overworld coord/position because Instance chunks never flushed (Null repo); reconnect places the Player back at the Overworld entry chunk at the last-heartbeat position. **Documented limit**: any in-Instance state change is lost on disconnect — irrelevant for Phase 9 (nothing in the Instance modifies inventory), but future combat/loot phases will need an explicit story.

- **Verb guards.** `Session.build/3` returns `{:error, :no_build_in_instance}` when realm is Instance (Structures are Overworld-only per CONTEXT.md). `harvest` and `damage` degrade naturally to `:no_target` (no Resource nodes / Structures inside).

- **Frontend.** Render `portals` from the snapshot as a distinct visual (placeholder mesh — a tinted cylinder or swirl). Cosmetic dressing for Instance realm — a different ground tint / fog color / overlay text — purely client-side based on the `realm` field of the latest `relocated` push. Refactor `main.ts`: one persistent `player:<username>` channel for input + `self`/`relocated`; a topic-keyed map of snapshot channels that cycles on `relocated`. No new HUD elements, no new input handling.

- **Tests.** ExUnit for `Worldgen.portals/1` and `InstanceWorldgen.portals/1` (pure-function); `Chunks.whereis/2` realm dispatch; `Session.enter_instance/2` and `Session.exit_instance/1` (state transitions, position override, WarmSet rebuild, supervisor lifecycle); `MovementSystem` bounds clamp; `Instances.start_new/0` + `Instances.terminate/1` lifecycle; eager-teardown on `Session.terminate/2` when realm is Instance; `Session.build/3` returns `:no_build_in_instance` in Instance. Playwright `phase9.spec.ts`: walk to Portal at `(8000, 8000)`, assert `relocated` event fires, assert ground tint changes, walk to return-Portal in Instance center, assert second `relocated` event, assert back on Overworld near the Portal cell. Disconnect-mid-Instance regression: open in-Instance, drop the socket, reconnect, assert reappearance at pre-entry Overworld position and the previous Instance's Registry keys are gone.

**Done when**: walk to the Portal at the center of Overworld chunk `{0,0}`, get smoothly migrated into a fresh 3×3 Instance with a distinct visual, walk around (bounded by clamp at the perimeter), step onto the return-Portal, get migrated back to the Overworld at the entry Portal cell. Disconnect mid-Instance, reconnect — you're back at the Portal, the Instance is gone (no leaked processes, no leaked Registry keys).

## Deferred

These are deliberately not in v1 — record them here so they're not forgotten:

- **Distributed BEAM** (former Phase 7). v1 runs on a single BEAM node. Realistic capacity for this game (low-thousands concurrent at the optimistic end) fits comfortably on one beefy box, and the legitimate reasons to ever go multi-node — fault tolerance, geographic distribution, memory bound — are all out of v1 scope. The work that *was* in Phase 7 (libcluster + Horde swap + cross-node tracing) is a costly architectural exercise with no realistic deployment target until one of those reasons becomes concrete. If/when picked up, the `Registry` / `DynamicSupervisor` APIs in `GameCore` are shaped to be Horde-compatible, and two sub-decisions need to be made then: (1) whether snapshot fan-out stays direct-send (`send/2` to subscriber pids, transparent cross-node) or moves to `Phoenix.PubSub` for uniform topic semantics; (2) whether the player's Session is found via a cluster-aware registry or via a Session-pid component carried with the entity through `ChunkMigration`. Static cluster only — node-death tolerance is its own further step.
- Auth, anti-cheat, public exposure, ops/observability
- Player housing, persistent dungeons, guild halls
- PvP (combat model exists, but no PvP-specific rules / safe zones / loot drops on death)
- Client-side prediction & reconciliation for own Player (Phase 1 uses authoritative snapshots only; smooth movement comes later via interpolation between snapshots and local prediction)
- Combat model (twitch / target-locked / ability-based) — decide before Phase 8 gameplay slice
- Progression (XP, levels, skills) — decide alongside combat
- Crafting recipes and stations — decide alongside building
- Asset pipeline / art direction — bootstrap with stock packs (Synty, Quaternius, Kenney)
