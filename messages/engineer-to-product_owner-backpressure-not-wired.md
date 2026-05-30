From: engineer
To: product_owner
Kind: behaviour gap
Status: open
Date: 2026-05-30

# `overload-backpressure` has no observable to prove — the freeze isn't wired

Evaluating the running system against `stories/` (your initial-stories handoff), every story is
satisfied and traced to proving tests in `sim/tests/stories.rs` **except one**:
`overload-backpressure.feature`.

## What exists vs. what the story asserts

The Datastore has a real **backpressure state machine** — `Mode::Flowing` / `Mode::Backpressured`,
engaging at a high-water mark and disengaging at a low-water mark, unit-tested in
`sim::datastore::tests::backpressure_engages_and_disengages`. So the *persistence layer* knows when
it can't keep up.

But that mode is **read nowhere outside `datastore.rs`** — it is not wired to the sim tick or the
Player-input path. Nothing stalls a Player when the buffer floods. So the story's observable —

> those Players' inputs stall — they freeze together … the frozen Players resume … the actions
> they took before the freeze are intact

— is **not implemented**. I can't write a passing proving test for it without first building the
freeze, and I'd rather not guess the behaviour: this is observable and not obvious, and you already
flagged this story as derived from the persistence-is-a-promise commitment with v1 scope
*unconfirmed* (your handoff + your gap message to the designer).

## How I've left it

`mod overload_backpressure` in `sim/tests/stories.rs` documents the gap and carries an `#[ignore]`d
test (`players_freeze_under_overload_and_resume_intact`) describing the intended observable — it
becomes the proving test the moment the freeze is wired. Nothing else in the story set is blocked.

## What I need from you (or the designer)

1. **Is whole-group freeze-on-overload in v1 scope at all?** (Pending your designer thread.)
2. If yes, the design gap I'd need closed before implementing: *what* trips the freeze
   (the trigger is abstract in the story — "persistence cannot keep up"), and what "resume with
   actions intact" guarantees concretely (replay buffered inputs? hold the tick? drop nothing).

I'll pick this up as its own increment once scope is confirmed. I own this thread and will delete it
once the question is resolved (either the freeze ships with its proving test, or the story is
dropped/deferred from v1).
