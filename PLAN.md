# PLAN — next increment

Forward-looking working notes for the engineer's next increment (see the engineer brief). Not
a log of decisions already embodied in the system — those live in the code and its tests. The
canonical *why/what* is upstream in `design/`; the observable acceptance criteria are the user
stories in `stories/`; the architecture invariants are in `AGENTS.md`.

## Landed: the story acceptance layer

`sim/tests/stories.rs` makes the product owner's 14 `.feature` files executable — one module per
story, every scenario either pinned by a proving `#[test]` or cited to the test elsewhere that
proves it. The system satisfies **all 14** stories; coverage was expanded with the edge/negative/
boundary cases the stories leave to engineering (footprint blocking across full/depleted/built/
destroyed, one-way Player collision, continuous boundary crossing, Instance fixtures + teardown,
Carcass perishing, starving-deer-feeds-through-threat, wildlife identity/population, Region healing).

## Landed: freeze-on-overload via the unified intent model

`overload-backpressure` is wired and its proving test
(`players_freeze_under_overload_and_resume_intact`) is no longer `#[ignore]`d. The key move: **all
player input is now a fire-and-forget [`Action`] intent** (`harvest`/`build`/`damage` join `move`),
enqueued on receipt into a per-actor bounded FIFO and resolved only in the tick (before movement),
with outcomes async (`self` deltas, or an `action_rejected` push). With nothing resolving outside
the tick, the overload freeze is just **skip-the-tick** while the Datastore is `Backpressured` (clock
held, flush kept running so it self-relieves). There is one Datastore, so the freeze is global —
everyone sharing that persistence authority freezes together. Verb *logic* now lives only on the
realm (`RealmWorld::{harvest,build,damage}`); the synchronous verb replies left the wire contract,
replaced by the `action_rejected` event. Design + decision record: `design/backpressure-freeze.html`.

> Note: this proceeded at the engineer's direction ahead of a formal PO/designer reply on the open
> behaviour-gap thread (`messages/engineer-to-product_owner-backpressure-not-wired.md`); per-cluster
> backpressure (only the overloaded Island freezes) remains deferred as v2.

## Candidate next increments

Design + decision record for the next three: `design/targeting-and-wysiwyg.md`; terms in
`design/glossary.md` (Tick, Verb, Target, Frontier, Lawful render, Verb button, Target marker).

1. **Targeting + the Verb button** (+ seq-pinned Verbs, shipped together). Click selects a
   Target (click-priority heuristic dies; build stays click-on-ground for now); `E` +
   contextual HUD button issues the entity-directed Verb by WireId; diegetic Target marker
   (no target frame); Escape clears. Verbs carry the input seq and resolve at that named
   tick. Wire: harvest/damage lose `{x,y}`, gain target WireId + seq → contract regen, both
   sides. Server repair in scope: `build` gets the `in_range` check its siblings have
   (today it's client-gated only — hostile clients can build cross-chunk). Showcase gains
   the marker (per targetable kind) and the button (inert/lit/dimmed × verb labels).
2. **Lawful-render judging.** Frontier asserted on every input frame (monotone,
   never-future, delivered-tick-checked, `M − frontier ≤ LEAD_BOUND`); ~10-tick
   position/intent ring server-side; entity-directed Verb range judged in the press frame
   (own exact position vs Target's lawful render, shared-integrator recompute).
   Continuous-only forgiveness; effects at resolve tick. Revisit-at-PvP flag.
3. **Preemptive resolution.** Eager per-fact resolve/emit under could-affect shadows;
   emittable ⇔ Resolved; finalization internal; tick-state discard at successor-finalize
   (the judging ring is separate retention). Pure scheduling — semantics provably
   unchanged, so the existing test pyramid should pass untouched except for emission
   timing.

- Held story scenarios will arrive once the designer answers the PO's gaps (multi-member Party
  Instance entry; the one-authority / never-under-merge observable). Add their proving tests then.
- New stories needed from the PO for increment 1 (targeting, the Verb button, rejection
  honesty) — the 14 existing `.feature` files predate select-then-act.

## Landed: parallel tick + lossless crash on a tick panic

Production drives the **parallel tick** (the server enables the worker pool; `tick_or_flush`
dispatches to it). A tick panic — on the tick thread *or* a worker thread — no longer poisons the
shared mutex, hangs the pool, or silently freezes the world. Per the chosen model the runtime is
presumed corrupt and goes **down** (no in-process recovery / per-cluster re-home), but on the way:
worker panics are caught and re-raised on the tick thread, `flush_now()` makes the durable store as
current as possible (fresh player positions + drained persist events), and the transport aborts —
so loss is bounded to the unflushed window and a supervisor restarts from durable state.

## Deferred follow-ups

Fault tolerance — residual:

- **External supervisor + restart-from-Datastore** is deployment config (systemd/orchestrator), not
  code; the rehydrate-on-connect path already restores durable state.

Client / wire:

- **Cosmetic rendering gaps to confirm on a real display** (no GL in-container): portal ring, grid
  lines, dev chunk borders + coordinate labels, shadows; dev toggle is on `Tab` (no backtick in
  three-d's `Key`). Confirm via `bin/showcase` — it displays every UI element in every
  appearance-affecting state through the real render path (presence is machine-checked; only
  appearance needs eyes).

_Done: `contract.json` is now generated from the server types + freshness-checked
(`export-contract` bin); `WALL_COST` is sourced from `protocol::consts`._

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
