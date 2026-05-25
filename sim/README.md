# sim — interaction-clustered simulation (Rust)

A Rust prototype that is **feature- and wire-compatible** with the Elixir game under `apps/`, built on a
deliberately different internal structure: one shared ECS world per realm, dynamic entities partitioned by
*interaction locality* into **clusters**, a single serialized **Labeler** owning the partition, and a
changed-only observation stream. No per-chunk processes, no message handoffs. See `../IDEA.md` for the
model and the phase-by-phase build log.

## Layout

| module | role |
|---|---|
| `geometry`, `chunkgraph` | chunk coords; 8-adjacency merge/split predicates |
| `labeler` | the serialized topology authority (place / merge / split), canonical by construction |
| `components`, `world` | ECS components and the per-realm `RealmWorld` (movement, collision, verbs, hydration) |
| `collision` | axis-decomposed body-circle clamping + build predicate (ported from Elixir) |
| `worldgen`, `catalogue` | deterministic trees/portals; structure costs/HP/footprints |
| `sim` | orchestrator: realms, clock, players, instances, verbs, persistence wiring |
| `repack`, `parallel` | repack policy + parallel cluster execution (persistent worker pool) |
| `datastore` | pending-writes buffer (LWW + tombstones), merged reads, flush, backpressure |
| `delta`, `wire` | changed-only deltas; per-chunk snapshot + event payloads (contract-shaped) |
| `phx`, `server`, `transport` | Phoenix Channels v2 codec, channel routing, async WebSocket runtime |

## Run

```sh
cargo test                 # 90+ unit/integration tests, incl. an end-to-end WebSocket session
cargo run --release --bin server   # serves Phoenix Channels v2 on :4000 (SIM_PORT to override)
cargo test --release --test ceiling -- --nocapture   # single-core ceiling + parallel scaling numbers
```

The server is a drop-in for the Elixir `GameWeb` socket: same topics (`player:<u>`, `chunk:x:y`,
`instance:<id>:chunk:x:y`, `dev:stats`), events, and payloads (`../apps/game_web/priv/contract`). In dev,
Vite on :3000 proxies `/socket` to it; the existing frontend connects unchanged.
