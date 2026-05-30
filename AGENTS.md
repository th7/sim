Real-time multiplayer game. **Rust backend** (`sim/`): one shared ECS world per realm, partitioned by interaction locality into **clusters**; a serialized **Labeler** owns the partition; a Postgres-backed Datastore persists; a Phoenix-Channels-v2 WebSocket is the wire. A **native Rust client** (`client/`, three-d) speaks that wire over `/socket/websocket`; the shared codec + wire structs live in `protocol/`. See `DESIGN.md` for the model and `CONTEXT.md` for the domain language.

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

## Work Loop

Substantive work runs a three-step loop — **stabilize → clarify → implement** — repeating per increment. It runs **human-in-the-loop** (the human answers/ratifies) or **autonomously** (the agent decides-and-logs); the steps are identical, only the check-in differs.

- **Stabilize** — leave the base healthy before taking on the increment: no known bugs, docs current. Per-increment, not per-commit. With a human in the loop, **review and settle `AGENT_LOG.md`** — walk its open items and remove each as the human reviews it (keepers graduate to an ADR / `CONTEXT.md` / `DESIGN.md`). Autonomous mode fixes what it owns and logs anything needing a human (ADRs, glossary, architectural refactors) to `AGENT_LOG.md`.
- **Clarify** — run `/grill-with-docs` to resolve the design tree. Human mode: the human answers; glossary/ADRs updated with them. Autonomous mode: the agent self-answers and records its answers (and any glossary/ADR recommendations) in `AGENT_LOG.md`. When the grill settles, rewrite `PLAN.md` as the next increment.
- **Implement** — drive every new behavior **test-first** (red → green), then a **stay-green refactor** to close out. Level by change: pure logic → unit; new game behavior → `sim/tests/` integration; new client↔server interaction → `client/tests/integration.rs` e2e. **Commit at every stable point** — coherent, self-contained, warning-free, `cargo test --workspace` green (the gate in Project guidelines). Log every non-obvious decision and deferred/recommended follow-up to `AGENT_LOG.md`.

**ADRs (`docs/adr/`) and the domain glossary (`CONTEXT.md`)** are written **only with a human in the loop** — and *should* be, then. Autonomously the agent never touches them; it logs a recommendation to `AGENT_LOG.md` instead. **Autonomous mode** edits only `AGENT_LOG.md` and `PLAN.md` among root `.md` files, prefers decide-and-log over asking, and hard-halts only on a true blocker (missing access, unresolvable contradiction, or an unauthorized irreversible/outward-facing action like push/deploy/delete) — logging it first.
