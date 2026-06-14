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
enqueued on receipt into a per-actor bounded FIFO and resolved only in the tick (post-movement
since the simultaneity-law revision — see `design/targeting-and-wysiwyg.md`),
with outcomes async (`self` deltas, or an `action_rejected` push). With nothing resolving outside
the tick, the overload freeze is just **skip-the-tick** while the Datastore is `Backpressured` (clock
held, flush kept running so it self-relieves). There is one Datastore, so the freeze is global —
everyone sharing that persistence authority freezes together. Action *logic* now lives only on the
realm (`RealmWorld::{harvest,build,damage}`); the synchronous action replies left the wire contract,
replaced by the `action_rejected` event. Design + decision record: `design/backpressure-freeze.html`.

> Note: per-cluster backpressure (only the overloaded Island freezes) remains deferred as v2.

## Landed: targeting, the Action button, and WYSIWYG judging

All four increments of `design/targeting-and-wysiwyg.md` are in (terms in
`design/glossary.md`: Tick, Action, Target, Frontier, Lawful render, Action button, Target
marker). Click selects a Target (the click-priority heuristic died with its wasted-wood
flaw; click hit-testing is nearest-within-tolerance); `E`/the HUD button is the one issuer
of entity-directed Actions (Gatherable→harvest, Structure/NPC→damage, by WireId); the
diegetic Target marker is the only Target display; Escape clears; despawn/View-exit/
relocation clear, distance and depletion never do. Actions are seq-pinned (judged at the
press frame's own position — `an_action_is_judged_at_its_press_frame_position_not_at_arrival`,
plus the moment-it-lights e2e) and Frontier-asserting (range judged against the Target's
lawful render OR the authoritative present; never-future/monotone/Lead-window clamps —
`a_press_is_judged_against_the_targets_lawful_render`,
`frontier_claims_are_clamped_to_the_lead_window`). `build` gained the server-side
`in_range` law. Schedule-confluence and bounded retention are pinned
(`outcomes_are_invariant_to_intent_arrival_schedule`,
`the_judging_ring_never_outgrows_the_lead_window`). Wire: harvest/damage are
`{target, seq, frontier?}`, build gained `seq`; `action_rejected` attributes entity actions
by target id; contract regenerated. The e2e layer covers the tracer (click→target→press→
wood), the hunt loop (kill by identity → Carcass retarget → meat+hide), rejection
surfacing, and the moving press.

The view layer (Target marker disc, the contextual Action button in its inert/ready/dimmed
states, `E`/Escape) was **visually confirmed on a real display**. Action-button labels are
final as the raw action names ("harvest (E)" / "damage (E)") — domain vocabulary == display
vocabulary, no presentation mapping. (Scope note: this pass covered the new targeting
visuals only; the older cosmetic-gap list under "Deferred follow-ups" was not part of it.)

> Engineering deviations (frontier-on-actions, either-frame eligibility, preemptive
> resolution as pinned-promises-not-machinery) were reviewed and ratified in the
> after-the-fact grill; that grill also revised the simultaneity law to movement-first
> ("intents are processed with their tick" is now an invariant) and added the
> frontier_violations probe counter. Details in `design/targeting-and-wysiwyg.md`.

## Candidate next increments

- **PO: new `.feature` stories for targeting** (select-then-act, the Action button, rejection
  honesty, lawful-render judging) — the 14 existing stories predate the feature; the sim
  tests above are ready citations.
- Held story scenarios will arrive once the designer answers the PO's gaps (multi-member Party
  Instance entry; the one-authority / never-under-merge observable). Add their proving tests then.
- Parked with revisit triggers (see the design doc): the lockstep trio (anti-backdating;
  trigger = PvP) — the could-affect shadow scheduler slots under it when it lands — and
  async Island clocks (pacing; trigger = straggler pain).

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
