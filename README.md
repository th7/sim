# Sim

A cooperative, persistent, isometric, real-time world where players fight, craft, and gather. PvP is an eventual concern, not v1.

A **Rust** backend simulating one shared ECS world, partitioned by *interaction locality* into **clusters** (the interaction-clustered model), and a **native Rust client** (three-d) that connects over the Phoenix Channels v2 WebSocket protocol.

## Layout

- `protocol/` — the shared wire crate: the Phoenix-Channels codec, geometry, ids, and the bidirectional wire payload structs both sides serialize.
- `simcore/` — the shared simulation core: movement integration, collision, and the kind→Footprint catalogue — one implementation consumed by both the server and the client's Mirror, so speculation and authority cannot drift.
- `sim/` — the Rust backend: ECS + Islands, the serialized Cartographer, worldgen, the Postgres-backed Datastore, and the Phoenix-Channels-v2 WebSocket server (`sim/src/bin/server.rs`). See [`sim/README.md`](./sim/README.md).
- `client/` — the native client: a pure `ClientModel` + `Session` (WS/phx) bridged to a `three-d` + egui view (`client/src/bin/game.rs`), with a **Mirror** (`client/src/mirror.rs`) speculating the View window ahead of the wire — own player by exact replay, bounded by `LEAD_BOUND_TICKS`.
- `contract/contract.json` — the wire contract (the shared schema both sides conform to; guarded by `sim/tests/contract.rs`).
- [`design/`](./design/) — the canonical design layer: vision, the domain glossary, and per-area design docs (the *why/what*)
- [`stories/`](./stories/) — Gherkin user stories: the observable acceptance criteria the implementation answers to
- [`AGENTS.md`](./AGENTS.md) — engineering conventions + the architecture invariants (the *how*: runtime, client, ecosystem)

## Running locally

Requires Rust. Postgres is optional (for persistence).

```bash
bin/dev                  # builds + runs the server on :4000 and a client as "alice"
bin/dev --user bob       # name the player
bin/dev --dev            # start with the dev overlay (extra flags forward to the client)
```

`bin/dev` blocks until you interrupt it (Ctrl-C) or either process exits, then shuts both down. To persist across restarts, give it a database: `SIM_DATABASE_URL=postgres://postgres@127.0.0.1:5432/sim_rust bin/dev` (without it the backend uses an in-memory store). Change the port with `SIM_PORT`.

To run the pieces separately:

```bash
cargo run --release --bin server                                   # server on :4000
cargo run --release -p client --bin game -- --user alice           # client (default --server ws://localhost:4000/socket/websocket?vsn=2.0.0)
```

## Tests

```bash
cargo test --workspace
```

`client/tests/integration.rs` is the load-bearing end-to-end description of what the game does: it boots the real server in-process and drives the native client over a real WebSocket, re-pinning every phase (connect, two clients, movement, multi-chunk-boundary walk, harvest→build→damage, dev stats, portal→instance) — read it when you want to know what "works" means.
