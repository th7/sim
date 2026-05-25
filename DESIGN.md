# Design

What the running system does, from outside. See `CONTEXT.md` for domain language and `PLAN.md` for upcoming work.

> **Note:** This is the *as-built* system. The Chunk-as-process model and client-driven 3×3 View window
> below are superseded *in design* by [ADR-0001](./docs/adr/0001-islands-and-cartographer.md) (Islands +
> Cartographer, server-driven streaming) and the updated `CONTEXT.md` — but **not yet in code**, so this
> remains what actually runs.

## Player

A connected Player...

- ...connects via browser (Vite + Three.js) at `localhost:3000`. One username = one in-world entity; the same Player on reconnect resumes where they logged off.
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

- Single shared Overworld, partitioned into Chunks (24×24 world units; 1 world unit = 1000 sub-units).
- Each Chunk is a process. Tick rate 20 Hz internal, 10 Hz snapshot broadcast.
- Chunks activate on demand based on player proximity and deactivate after sustained inactivity.
- Each connected Player keeps a 5×5 Warm set of Chunks hot around their current Chunk; their 3×3 View window drives snapshot subscriptions.
- Resource nodes (trees) and Portals are placed deterministically by Worldgen.
- Instances are ephemeral, in-memory 3×3 grids spawned on Portal entry. Destroyed when the entering Player leaves or disconnects.

## Persistence

- The following survive server restart: Player position + Inventory, Structure existence + HP, Resource node depletion timers.
- Instance state is in-memory only and lost on disconnect or shutdown.

## Dev mode

- Toggle with backtick or `?dev=1` URL parameter.
- HUD shows: username, realm, world position, current Chunk, View window, active Chunk count, total Player count.
- Overlay: 7×7 grid around the Player, colored by Chunk lifecycle (hot / idle-armed / cold) and bordered by relationship to the Player's Warm set / View window. Shrinks to fill the bounded 3×3 grid inside an Instance.

## Operator

- Single BEAM node. `mix phx.server` runs everything.
- Postgres required; dev defaults to a local socket.
