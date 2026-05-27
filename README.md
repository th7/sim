# Sim

A cooperative, persistent, isometric, real-time world where players fight, craft, and gather. PvP is an eventual concern, not v1.

A **Rust** backend simulating one shared ECS world, partitioned by *interaction locality* into **clusters** (the interaction-clustered model — see [ADR-0002](./docs/adr/0002-rust-clustered-simulation-runtime.md)); a Vite + Three.js client speaking the Phoenix Channels wire protocol. The backend serves both the built client and the socket from one process.

## Layout

- `sim/` — the Rust backend: ECS + clusters, the serialized Labeler, collision, worldgen, the Postgres-backed Datastore, and a Phoenix-Channels-v2 WebSocket + static-file server (`sim/src/bin/server.rs`). See [`sim/README.md`](./sim/README.md).
- `frontend/` — Vite + Three.js client
  - `frontend/test/` — vitest contract specs (no backend needed)
  - `frontend/e2e/` — Playwright golden-path specs against the running backend
- `contract/contract.json` — the wire contract (the shared schema both sides conform to)
- [`CONTEXT.md`](./CONTEXT.md) — the locked language: glossary + relationships
- [`DESIGN.md`](./DESIGN.md) — what the running system does today, from outside
- [`docs/adr/`](./docs/adr/) — architecture decision records
- [`IDEA.md`](./IDEA.md) — the design + build log of the interaction-clustered Rust backend

## Running locally

Requires Rust, Node, and a running Postgres (reachable as `postgres@127.0.0.1:5432`).

```bash
createdb -h 127.0.0.1 -U postgres sim_rust          # once

# Single binary: the backend serves the built client + the socket on :4000.
(cd frontend && npm install && npm run build)
(cd sim && SIM_DATABASE_URL=postgres://postgres@127.0.0.1:5432/sim_rust \
           SIM_STATIC_DIR="$PWD/../frontend/dist" \
           cargo run --release --bin server)
# → open http://localhost:4000/?u=alice

# Or, for frontend hot-reload, run Vite in front and let it proxy /socket:
(cd sim && SIM_DATABASE_URL=postgres://postgres@127.0.0.1:5432/sim_rust cargo run --release --bin server)
(cd frontend && npm run dev)   # Vite on :3000, proxies /socket to :4000
# → open http://localhost:3000/?u=alice
```

`SIM_DATABASE_URL` is optional — without it the backend uses an in-memory store (no persistence across restart). `?dev=1` shows the dev HUD.

## Tests

```bash
(cd sim && cargo test)             # Rust unit + integration (sim core, wire, persistence, contract)
(cd frontend && npm test)          # vitest contract specs
(cd frontend && npm run test:e2e)  # Playwright golden-path browser specs
```

The e2e specs under `frontend/e2e/` are the load-bearing description of what the game does end-to-end — read them when you want to know what "works" means. `npm run test:e2e` (→ `bin/e2e`) builds the bundle, spins up a dedicated backend on `:4001` against a fresh `sim_e2e` database, runs the specs, and tears it down.
