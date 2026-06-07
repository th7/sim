Real-time multiplayer game. **Rust backend** (`sim/`): one shared ECS world per realm, partitioned by interaction locality into **clusters**; a serialized **Labeler** owns the partition; a Postgres-backed Datastore persists; a Phoenix-Channels-v2 WebSocket is the wire. A **native Rust client** (`client/`, three-d) speaks that wire over `/socket/websocket`; the shared codec + wire structs live in `protocol/`. Domain language is `design/glossary.md`; observable behaviour is the user stories in `stories/`; the architecture invariants are below.

## Project guidelines

- Prefer obvious tests and obvious code over documentation. When documentation is unavoidable, keep it terse.
- The wire contract (`contract/contract.json`) is generated from the server types (`cargo run -p sim --bin export-contract`) and both conformance- and freshness-checked by `sim/tests/contract.rs`; the client (de)serializes the `protocol/` wire structs. Don't change one side's wire shape without the other; regenerate the contract after changing an emitted shape.
- Before considering work done: `cargo test --workspace` (and `cargo build --workspace --all-targets` warning-free).

## Architecture invariants

Distilled from the former `docs/adr/` — load-bearing; change them only deliberately.

- **Interaction-clustered authority.** A *cluster* is the single authority over a connected set of
  interacting entities + the Chunks they span; a Chunk is data, never a process. Never-under-merge holds
  *by construction* — the serialized Labeler reconciles to the canonical footprint-overlap partition, and
  `interaction_range ≤ chunk_size` forces a Chunk two clusters need into one.
- **Structural determinism.** `BTreeMap`/`BTreeSet` ordering, id-ordered ticks, explicit sim clock,
  seeded RNG — no wall-clock. Clusters are entity-disjoint, so the tick parallelises with no `unsafe`
  and stays identical to the serial run; one dense cluster on a single core is the accepted ceiling.
- **The Datastore is the durability boundary.** Clusters own runtime only; persistence flushes on
  SIGTERM and anchors the clock so timers survive restart; recovery is re-home + re-hydrate; the blocking
  Postgres client stays off the Tokio workers. Act through your cluster, observe geography (changed-only
  deltas → a Session's View window).
- **Native client, server-authoritative, Mirror-predicted.** A `three-d`/egui Rust app; logic is a
  pure tested `ClientModel` that owns a **Mirror** (`client/src/mirror.rs`, `design/glossary.md`): a
  speculative simulation of the View window running the server's own integrator from the shared
  `simcore` crate — own player by exact frame replay (bit-identical, pinned by
  `client/tests/exactness.rs`), others by last-known Intent, whole-Mirror freeze at
  `LEAD_BOUND_TICKS`. The `protocol` crate holds the wire structs both sides serialize; `simcore`
  holds movement integration, collision, and the kind→Footprint catalogue — one implementation, two
  consumers; the client carries none of the server's tokio/postgres/hecs. Positions are sub-units
  (1 unit = 1000); see the wire-contract guideline above.
- **Motivation is pure and RNG-free.** Pick the most-immediate actionable option at each level
  (chain → Goal → Plan → Intent); cross-need weighing happens only at goal arbitration — a static
  per-Need bias × a leaky, capped, sim-clock Pressure integral.
- **The cold world is a field, not a sim.** NPCs don't anchor the Warm set (only Players keep Chunks hot)
  and have no persistent identity — they materialize from a Region's level and dissolve into its
  Disturbance. Level is `clamp(Baseline(habitat, season, noise) + Δ·e^(−(t−t₀)/τ))`: no cold tick or
  population integrator; cost scales with Player activity, not map size.

## Test guidelines

- The Rust suite is unit + integration (`sim/tests/`); the cross-restart Postgres test self-skips unless `SIM_TEST_DATABASE_URL` is set.
- `client/tests/integration.rs` is the load-bearing end-to-end description: it boots the real server in-process and drives the native client over a WebSocket, re-pinning every phase.
