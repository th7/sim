Real-time multiplayer game. **Rust backend** (`sim/`): one shared ECS world per realm, partitioned by interaction locality into **clusters**; a serialized **Labeler** owns the partition; a Postgres-backed Datastore persists; a Phoenix-Channels-v2 WebSocket + static-file server is the wire. A Vite + Three.js client (`frontend/`) speaks that wire. See `IDEA.md` for the model and `CONTEXT.md` for the domain language.

## Project guidelines

- Prefer obvious tests and obvious code over documentation. When documentation is unavoidable, keep it terse.
- The wire contract (`contract/contract.json`) is the shared schema; the backend conforms to it (`sim/tests/contract.rs`) and the frontend generates its types from it (`npm run gen:contract`). Don't change one side's wire shape without the other.
- Before considering work done: `cd sim && cargo test` (and `cargo build --all-targets` warning-free), `cd frontend && npm test`.

## Rust guidelines

- Determinism matters: order with `BTreeMap`/`BTreeSet`, tick clusters in id order, keep the sim clock explicit. The never-under-merge invariant must hold *by construction* (the Labeler reconciles to the canonical partition), not "usually".
- Match the Elixir numeric constants and the wire contract exactly — positions are sub-units (1 unit = 1000); the client divides by 1000.
- Keep `unsafe` out (Phase 2 found it unnecessary). The blocking Postgres client must not run on a Tokio worker — it lives on its own thread (see `sim/src/pgstore.rs`).

## Test guidelines

- The Rust suite is unit + integration (`sim/tests/`); the cross-restart Postgres test self-skips unless `SIM_TEST_DATABASE_URL` is set.
- `frontend/e2e/` (Playwright) is the load-bearing end-to-end description; run with `npm run test:e2e`.
