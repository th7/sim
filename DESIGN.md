# Design

What the running system does, from outside. See `CONTEXT.md` for domain language, `docs/adr/` for the runtime decisions, `PLAN.md` for the next increment, and `AGENT_LOG.md` for the agent's decision log and follow-ups.

> **Note:** The running system is the **interaction-clustered** Rust backend
> ([ADR-0002](./docs/adr/0002-rust-clustered-simulation-runtime.md)): one shared ECS world per realm,
> partitioned into **clusters** by a serialized **Labeler** (the Cartographer in its shared-memory form),
> with no per-chunk processes. `CONTEXT.md`'s Island/Cartographer language describes this model; the wire
> behaviour below is unchanged from the prior Elixir implementation.

## Player

A connected Player...

- ...connects through the native client (`client/`, a three-d desktop app) over the WebSocket. One username = one in-world entity; the same Player on reconnect resumes where they logged off.
- ...moves their entity with WASD. The server is authoritative; clients render server snapshots without prediction.
- ...sees other Players and world entities in a 3×3 View window of Chunks around their current Chunk.
- ...crosses Chunk boundaries seamlessly within the Overworld.
- ...harvests Resource nodes (currently only trees → wood); the node depletes and respawns on a timer.
- ...builds Structures (currently only walls; cost: 5 wood). Placed Structures persist across server restart.
- ...damages Structures with mouse clicks (25 HP per click). At 0 HP the Structure is destroyed; persisted HP and destruction survive restart.
- ...enters an Instance by stepping on an `into_instance` Portal. Inside, movement is clamped to the Instance's bounded 3×3 grid; no Resource nodes, no Structures.
- ...exits via the return-Portal back to the entry Chunk at a cell offset from the entry Portal.
- ...disconnecting mid-Instance returns them next to the entry Portal on reconnect (not on it — the Instance does not loop back).

## World

- Single shared Overworld, partitioned into Chunks (16×16 world units; 1 world unit = 1000 sub-units).
- One shared ECS world, ticked at 20 Hz; snapshots broadcast at 10 Hz. Chunks are data, not processes.
- Chunks activate on demand when a cluster's footprint covers them and deactivate after sustained inactivity.
- Each connected Player's 3×3 View window drives snapshot subscriptions, contained within the chunks their cluster keeps hot.
- Resource nodes (trees) and Portals are placed deterministically by Worldgen.
- Instances are ephemeral, in-memory 3×3 grids spawned on Portal entry. Destroyed when the entering Player leaves or disconnects.

## NPCs & wildlife

Off by default; enable with `SIM_WILDLIFE` (on in the game server). Wildlife is **deterministic** and **simulated only near Players** — it never keeps Chunks hot on its own. See ADRs 0004–0006 for the model.

- Two NPC kinds inhabit the Overworld: **deer** (prey) and **wolves** (predator), each driven by a **Motivation** engine (needs → goal → plan → per-tick movement Intent).
- The Overworld is partitioned into deterministic **Regions** (habitat territories). A Region's wildlife level is a pure function of its habitat plus a healing **Disturbance**; no background simulation runs.
- Wildlife **materializes** from a Region's level when a Player approaches and **dissolves** back into the Region's Disturbance when the Player leaves. Overhunting depletes a Region; it heals over time, and a depleted Region spawns hungrier, more aggressive animals (history shapes temperament).
- **Combat:** Players damage NPCs with the same mouse-click verb (25 HP); NPCs damage prey and rival NPCs; Players are invulnerable in v1. A killed animal leaves a **Carcass** — harvest it for **meat**/**hide** (feeding the craft/gather economy); NPCs eat from it; rival predators contest it.
- Emergent behaviours: deer **herd** and **stampede**, wolves **pack-hunt**, animals are **bolder at night** and **warier when wounded** (agent extensions — see `AGENT_LOG.md`).

## Persistence

- The following survive server restart: Player position + Inventory, Structure existence + HP, Resource node depletion timers.
- Instance state is in-memory only and lost on disconnect or shutdown.

## Dev mode

- Toggle with backtick or `?dev=1` URL parameter.
- HUD shows: username, realm, world position, current Chunk, View window, active Chunk count, total Player count, and NPC count (in view / in world).
- Overlay: 7×7 grid around the Player, colored by Chunk lifecycle (hot / idle-armed / cold) and bordered by relationship to the Player's Warm set / View window. Shrinks to fill the bounded 3×3 grid inside an Instance.

## Operator

- A single Rust binary (`sim` server) runs everything: it serves the built client and the WebSocket on one port (`SIM_PORT`, default 4000).
- Postgres optional — set `SIM_DATABASE_URL` to persist (players, structures, depletions survive a restart); unset uses an in-memory store. Flushes pending writes on SIGTERM.
- `SIM_WILDLIFE` toggles the wildlife ecosystem (default on in the server binary; set `0` to disable). Region Disturbances are in-memory only — they do not yet survive a restart.
