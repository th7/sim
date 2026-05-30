Real-time multiplayer game. **Rust backend** (`sim/`): one shared ECS world per realm, partitioned by interaction locality into **clusters**; a serialized **Labeler** owns the partition; a Postgres-backed Datastore persists; a Phoenix-Channels-v2 WebSocket is the wire. A **native Rust client** (`client/`, three-d) speaks that wire over `/socket/websocket`; the shared codec + wire structs live in `protocol/`. Domain language is `design/glossary.md`; observable behaviour is the user stories in `stories/`; architecture rationale is `docs/adr/`.

## Project guidelines

- Prefer obvious tests and obvious code over documentation. When documentation is unavoidable, keep it terse.
- The wire contract (`contract/contract.json`) is the shared schema; the backend conforms to it (`sim/tests/contract.rs`) and the client (de)serializes the `protocol/` wire structs. Don't change one side's wire shape without the other.
- Before considering work done: `cargo test --workspace` (and `cargo build --workspace --all-targets` warning-free).

## Rust guidelines

- Determinism matters: order with `BTreeMap`/`BTreeSet`, tick clusters in id order, keep the sim clock explicit. The never-under-merge invariant must hold *by construction* (the Labeler reconciles to the canonical partition), not "usually".
- Match the Elixir numeric constants and the wire contract exactly — positions are sub-units (1 unit = 1000); the client divides by 1000.
- Keep `unsafe` out (Phase 2 found it unnecessary). The blocking Postgres client must not run on a Tokio worker — it lives on its own thread (see `sim/src/pgstore.rs`).

## Test guidelines

- The Rust suite is unit + integration (`sim/tests/`); the cross-restart Postgres test self-skips unless `SIM_TEST_DATABASE_URL` is set.
- `client/tests/integration.rs` is the load-bearing end-to-end description: it boots the real server in-process and drives the native client over a WebSocket, re-pinning every phase.

## Roles & flow

Work runs through a three-role pipeline — **designer** (`design/`) → **product owner**
(`stories/`) → **engineer** (code + tests, `PLAN.md`) — coordinating through `messages/`. Each
role's brief is the source of truth for how that role works; this file holds only the
cross-cutting *engineering* conventions above. The engineer drives every new behaviour
**test-first** (red → green) then a **stay-green refactor**, commits at every stable point
(warning-free, `cargo test --workspace` green), and keeps the next increment in `PLAN.md`.
