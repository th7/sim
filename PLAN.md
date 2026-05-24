# Plan

Work locked in but not yet implemented. See `DESIGN.md` for what currently works and `CONTEXT.md` for domain language.

## Datastore

Replace the current chunk-owned persistence model (each Chunk calls a `ChunkRepo` directly) with a single per-node **Datastore** that handles all durable reads and writes for the running world.

### Contract

- Chunks send all state changes to the Datastore via synchronous calls. The call returns once the Datastore has accepted the change into its **pending writes**.
- Chunks read all durable state through the Datastore. The Datastore merges pending writes with the last-flushed DB state and returns the freshest view.
- The Chunk's responsibility ends when its call returns. The Datastore owns durability from there.
- Instance Chunks do not emit to the Datastore. Instance state is in-memory only.
- On cross-realm migration, the source Chunk eagerly emits the Player's `save_pos` before handing off the entity. The destination Overworld Chunk eagerly emits on receiving the entity in `migrate_in`. (Same-realm boundary crossings do not emit — the destination's next heartbeat covers them.)
- Chunks emit a Player upsert on their tick heartbeat only if that Player's `(position, inventory)` changed since the last heartbeat. Idle Players do not generate work.

### Pending writes

- Shape: `%{aggregate => %{key => value | :tombstone}}`. Aggregates: `player` (keyed by username), `depletion` (keyed by `{realm, coord, type, x, y}`), `structure` (keyed by `(x, y)`).
- Coalesce on emit; the last emission per key wins.
- `:tombstone` represents a delete intent; cancels any prior upsert at the same key.

### Flushing

- Periodic timer; ~1s cadence (numeric tuning under load).
- One `Repo.transaction` per flush, wrapping per-aggregate `insert_all` and `delete_all`. Compound atomicity (e.g., build emits a structure upsert + an inventory upsert) is free — both land in the same transaction.
- On flush success: clear all flushed entries from pending.
- On flush failure: keep all entries; retry next cycle. No partial drop. Persistent failures eventually trip backpressure.

### Backpressure

- Triggered when pending size exceeds `N_high` **or** the oldest entry age exceeds `T_high`.
- Released when pending size drops below `N_low` **and** the oldest age drops below `T_low`. (Hysteresis prevents flapping.)
- Mechanism: under load, the Datastore withholds replies to incoming write calls. Caller Chunks block in their `GenServer.call`. World freezes upstream.
- Recovery: DB recovers OR operator deploys a fix via hot code reload → next flush succeeds → drain → Datastore replies to parked callers in FIFO order → upstream resumes naturally. No explicit re-activation needed.

### Failure semantics

- Flush failures → backpressure. No crashes, no dropped data.
- Datastore process crash (OOM, internal bug) → cascade. The Datastore's supervisor uses `intensity: 0` so any crash escalates to `GameCore.Supervisor`, which escalates to the `:temporary` Application, which halts. Operator hot-reloads the fix and restarts the application; world rehydrates from DB. Pending state at crash time is lost.

### Supervision

- Datastore is declared **first** in `GameCore.Supervisor`'s child list. OTP starts children in declared order and stops in reverse, so the Datastore starts before any Chunk that might call it and stops only after every Chunk has emitted its final state.
- Datastore's `terminate/2` runs one final flush. Child spec uses `shutdown: 30_000` to give that flush room.

### Schema

- Drop the SERIAL `structures.id`. Primary key becomes the natural key `(x, y)`. Keep `chunk_x, chunk_y` as indexed columns for partition-style lookups.
- ECS structure eid becomes `"structure:#{x}:#{y}"`, matching the resource-node eid pattern.

### Test strategy

- Chunk tests: `start_supervised!({GameCore.Datastore, …})` with the real Repo behind `Ecto.Adapters.SQL.Sandbox`. No production-code branching for tests.
- Datastore tests: same setup, focused on backpressure transitions, retry behavior, transaction shape.
- The `GameCore.ChunkRepo` behaviour, `GamePersistence.ChunkRepo` implementation, and `GameCore.ChunkRepo.Null` are removed.

## Deferred

- **Auth, anti-cheat, public exposure, ops/observability.**
- **Player housing, persistent dungeons, guild halls.**
- **PvP** — combat model exists, but no PvP-specific rules / safe zones / loot drops on death.
- **Client-side prediction & reconciliation** for own Player. Currently authoritative snapshots only; smooth movement comes later via interpolation between snapshots and local prediction.
- **Combat model** (twitch / target-locked / ability-based) — decide before any combat phase.
- **Progression** (XP, levels, skills) — decide alongside combat.
- **Crafting recipes and stations** — decide alongside building.
- **Asset pipeline / art direction** — v1 ships hand-coded composite Three.js primitives (see `frontend/src/models.ts`) under a flat-shaded Lambert lighting rig. Stock packs (Synty, Quaternius, Kenney) are deferred to a later visual-identity phase.
- **Identity-via-integer-position invariant**, deeper pass. The invariant is asserted by convention only. A later phase should enumerate every place a position participates in identity (ECS Position, Worldgen-derived wire ids, depletion-cache `(chunk_x, chunk_y, type, x, y)` keys, structure cells), name the invariant in CONTEXT.md so it's first-class language, and decide whether to enforce it via a type / constraint / property test rather than relying on each new caller noticing.
- **Tick-based time instead of clock time.** `depleted_until` and similar time-sensitive state currently use `DateTime`. Switching to a tick number would give deterministic simulation (good for tests + replay), eliminate NTP/clock-skew edge cases, and make timing-sensitive state an integer comparison. Couples to the world-level-tick question below.
- **World-level monotonic tick.** Today each chunk has its own `tick_count`; the world has no global notion of "now." A world-level tick — referenced by all chunks — would underpin tick-based time, simplify cross-chunk time comparisons, and provide a single source of truth for any timing-sensitive feature. Open: chunks run their own tick schedules; strict alignment to a global tick isn't free.
- **Datastore batch flush internals.** The architecture is settled (single tx per flush; per-aggregate `insert_all`/`delete_all`). Open at implementation: ON CONFLICT clause per table, FK ordering within the transaction (Players → Structures.owner_username), how natural-key collisions interact with pending tombstones, exact retry behavior on `serialization_failure`/`deadlock_detected`. Decide alongside the `structures.id` → natural-key schema migration.
