# Bugs

Observed defects to work down during **Stabilize** (AGENTS.md → Work Loop). Reported from running the
game; the test suite is green, so these are gaps between the tests and real behaviour.

- [ ] **Wildlife disappears after chunk reload.**

- [ ] **Player clicking a deer seems to have no effect.**
  _Suspected: the client click router (`client/src/model.rs::decide_click`) has no NPC case — it harvests a
  tree, damages a structure, else builds. A click on a deer matches none, so no `damage` verb is ever sent.
  The server-side NPC damage path exists but is never invoked from the client._

- [ ] **Wildlife does not seem to interact (no pursuing or killing).**
  _Suspected: perception range is ~1 world unit, but materialized wildlife spawns roughly one animal per
  chunk (~16 units apart), so predator and prey are rarely within sensing range in the live world. The
  integration tests place them <1 unit apart. Likely a spawn-density / perception-range tuning gap._
