# Plan

The remaining work on Sim, triaged into three buckets:

- **Needs Follow Up** — a test passes (or a feature appears to work in the dev client) but the server doesn't actually enforce the semantics yet; the green suite hides the gap.
- **Ready to Implement** — design is locked (in CONTEXT.md or DESIGN.md); build it against new tests, no further decisions needed.
- **Needs Grill** — design isn't settled; needs a design conversation, then tests illustrating the agreed design, before implementation.

For the vision and repo layout, see [README.md](./README.md). The locked design and terminology live in [CONTEXT.md](./CONTEXT.md); behavior that already works is specified in [DESIGN.md](./DESIGN.md) and verified by the suites under `apps/*/test` and `frontend/{test,e2e}` — so it isn't repeated here. Glossary terms in **bold** are defined in CONTEXT.md.

## Ready to Implement

- **Decouple frontend and backend along an explicit API contract.** Design locked. The Phoenix Channels wire surface (topics; client→server verbs + replies; server→client pushes `snapshot`/`self`/`relocated`/`stats`) is declared as data in a `GameWeb.Contract` module; `mix contract.export` emits one JSON Schema per payload plus an event descriptor (event → direction, topic, reply) to `apps/game_web/priv/contract/`. Artifacts are committed and CI fails on a stale regen. The frontend generates `frontend/src/contract/types.ts` from those files and validates its test mocks against them (ajv); the backend proves conformance with a `Phoenix.ChannelTest` provider suite that validates real push/reply payloads against the same schemas (`ex_json_schema`). Validation is test-time only — prod keeps the existing channel pattern-match guards. Target test topology, three seams: frontend contract-style (mocked wire, no Phoenix) for client logic plus a consumer test; backend ChannelTest (no browser) for all server/wire semantics — the live-`:4000` vitest specs (`frontend/test/{channel,collision}.spec.ts`) are backend tests in disguise and move here; a thin Playwright golden-path layer (~5 paths) for semantic/units mismatches schema conformance can't catch (e.g. sub-units vs the client's ÷1000). Land as five strangler PRs: (1) contract pipeline + provider verification, (2) frontend codegen + consumer test, (3) migrate live vitest → ChannelTest (delete the `:4000` dependency), (4) remaining frontend specs → contract-style, (5) thin Playwright. Sequence before the DESIGN.md strangler below, which then pins each behavior at the appropriate seam rather than defaulting to e2e.

## Needs Grill

- **Strangle DESIGN.md.** Walk each behavior currently described in `DESIGN.md` (Player verbs, World/Chunk lifecycle, Persistence, Dev mode, Operator surface). For every behavior: confirm a test pins it; if not, write one — typically an e2e spec, occasionally a chunk-level integration test where the behavior isn't visible from the client. Once a behavior is pinned, delete its description from `DESIGN.md` (and from `PLAN.md`'s preamble link), or move the bit that's actually load-bearing language into `CONTEXT.md`. Goal: `DESIGN.md` empties out and is deleted. Open: per-behavior triage (which need new tests vs. which are already e2e-covered), how to handle behaviors that are inherently hard to e2e (operator-level claims like "single BEAM node", supervision-cascade semantics), and whether the strangler should be one PR per behavior or batched.

### Deferred

These are deliberately deprioritized for now — listed so they aren't forgotten, not because they're queued.

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
