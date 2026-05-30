# PLAN — next increment

Forward-looking working notes for the engineer's next increment (see the engineer brief). Not
a log of decisions already embodied in the system — those live in the code and its tests. The
canonical *why/what* is upstream in `design/`; the observable acceptance criteria are the user
stories in `stories/`; the architecture invariants are in `AGENTS.md`.

## Landed: the story acceptance layer

`sim/tests/stories.rs` makes the product owner's 14 `.feature` files executable — one module per
story, every scenario either pinned by a proving `#[test]` or cited to the test elsewhere that
proves it. The system satisfies **13 of 14** stories; coverage was expanded with the edge/negative/
boundary cases the stories leave to engineering (footprint blocking across full/depleted/built/
destroyed, one-way Player collision, continuous boundary crossing, Instance fixtures + teardown,
Carcass perishing, starving-deer-feeds-through-threat, wildlife identity/population, Region healing).

**One unmet story — `overload-backpressure`.** The Datastore `Mode` machine exists but is not wired
to freeze Player input, so the freeze observable can't be proven. Pushed upstream as a behaviour gap
(`messages/engineer-to-product_owner-backpressure-not-wired.md`); represented as an `#[ignore]`d
pending test. Blocked on v1-scope confirmation — do **not** implement until the PO/designer resolve it.

## Candidate next increments

- **Wire freeze-on-overload** — once the PO confirms v1 scope and the trigger/resume semantics.
  Consume `datastore::Mode::Backpressured` in the tick to stall a cluster's Player input, then
  un-ignore the pending test. (Behaviour gap thread open.)
- Held story scenarios will arrive once the designer answers the PO's gaps (multi-member Party
  Instance entry; the one-authority / never-under-merge observable). Add their proving tests then.

## Deferred follow-ups

Fault tolerance (was the ADR-0002 acceptance checklist; the runtime trades OTP's per-process
isolation for by-construction determinism, so these are owed):

- **Catch per-cluster / per-worker panics and re-home** the affected cluster from the Datastore
  (the durability boundary already bounds loss to the unflushed window).
- **Supervise the tick loop** so a panic restarts the runtime rather than exiting the process.
- **Fast, lossless restart-from-Datastore** to compensate for the loss of hot code reload (deploys
  are process restarts).

Client / wire (migrated from the retired `docs/frontend-port-notes.md`):

- **Generate `contract/contract.json` from the `protocol` structs** (+ a freshness check) instead of
  hand-maintaining it; today it is committed and only conformance-guarded (`sim/tests/contract.rs`).
- **`WALL_COST = 5` is hardcoded in the client model** and could drift from the server catalogue —
  consider exposing the cost via `protocol`.
- **Cosmetic rendering gaps to confirm on a real display** (no GL in-container): portal ring, grid
  lines, dev chunk borders + coordinate labels, shadows; dev toggle is on `Tab` (no backtick in
  three-d's `Key`).

Ecosystem / NPC depth (migrated from the retired AGENT_LOG):

- **Cross-restart persistence of Region Disturbances.** They live in memory (`Sim.wild_disturb`),
  so the overhunt→deplete→heal field resets on restart. Needs a `PersistEvent` variant +
  Datastore/pg schema. (Out of current story scope per the product owner's handoff.)
- **Grass-grazing Disturbance.** Deer graze abstractly against a Region's grass level; only
  deer/wolf population changes feed back into the field. Closing the loop (grazing writes a
  grass Disturbance) is a follow-up. (Out of scope; in `design/living-world.md` open frontier.)
- **Strategic chain tail — carry → stockpile → secure-source** (+ in-session cache object). The
  Motivation engine supports the full chain shape; v1 ships only `hunt → eat` + `flee /
  fight-to-hold`. Real v2 work; a behaviour gap for the product owner when it's grilled.
