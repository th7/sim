# PLAN — next increment

Forward-looking working notes for the engineer's next increment (see the engineer brief). Not
a log of decisions already embodied in the system — those live in the code and its tests. The
canonical *why/what* is upstream in `design/`; the observable acceptance criteria are the user
stories in `stories/`; architecture rationale is in `docs/adr/`.

## In flight: make the story set executable as proving tests

The product owner handed off 14 Gherkin `.feature` files in `stories/`. The system already
implements most of this behaviour, but nothing links a story to the test that proves it. This
increment builds that traceability layer and evaluates the system against each story.

1. **Trace each story to its proving test(s).** For every `.feature`, identify the existing
   test(s) in `sim/tests/` / `client/tests/integration.rs` that prove its scenarios, or write
   the missing proving test. The test is the story's executable form.
2. **Expand coverage** beyond each story with the edge / negative / boundary cases the story
   deliberately leaves to engineering.
3. **Surface discrepancies upstream.** Where a story contradicts what the system does, push it
   to the product owner (a behaviour gap) or the designer (a design gap) via `messages/` —
   never silently conform the story to the code or vice versa.

## Deferred follow-ups (migrated from the retired AGENT_LOG)

- **Cross-restart persistence of Region Disturbances.** They live in memory (`Sim.wild_disturb`),
  so the overhunt→deplete→heal field resets on restart. Needs a `PersistEvent` variant +
  Datastore/pg schema. (Out of current story scope per the product owner's handoff.)
- **Grass-grazing Disturbance.** Deer graze abstractly against a Region's grass level; only
  deer/wolf population changes feed back into the field. Closing the loop (grazing writes a
  grass Disturbance) is a follow-up. (Out of scope; in `design/living-world.md` open frontier.)
- **Strategic chain tail — carry → stockpile → secure-source** (+ in-session cache object). The
  Motivation engine supports the full chain shape; v1 ships only `hunt → eat` + `flee /
  fight-to-hold`. Real v2 work; a behaviour gap for the product owner when it's grilled.
