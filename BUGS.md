# Bugs

Observed defects to work down during **Stabilize** (AGENTS.md → Work Loop). Reported from running the
game; the test suite is green, so these are gaps between the tests and real behaviour.

- [x] **Wildlife disappears after chunk reload.** _Fixed._ The warm/cold boundary depleted a Region's
  Disturbance from per-chunk dissolve accounting `(survivors − materialized)`, counting a *migrating*
  animal as a loss at every chunk boundary it crossed — so pacing drained Regions to zero. Depletion is
  now event-sourced from actual deaths; dissolve is population-neutral. (Secondary: per-kill depletion
  scaled to Region size, so incidental predation dips-and-heals instead of zeroing a territory in ~4 kills.)
  Regression: `sim/tests/chunk_reload.rs`.

- [x] **Player clicking a deer seems to have no effect.** _Fixed._ The client click router
  (`client/src/model.rs::decide_click`) had no NPC case — it harvested a tree, damaged a structure, else
  built — so a deer click matched none and sent no `damage` verb. Added an NPC branch (tree → structure →
  NPC → build). Regression: `client/src/model.rs::click_damages_an_npc`.

- [ ] **Wildlife does not seem to interact (no pursuing or killing).**
  _Suspected: perception range is ~1 world unit, but materialized wildlife spawns roughly one animal per
  chunk (~16 units apart), so predator and prey are rarely within sensing range in the live world. The
  integration tests place them <1 unit apart. Likely a spawn-density / perception-range tuning gap._
