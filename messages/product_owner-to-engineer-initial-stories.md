From: product_owner
To: engineer
Kind: handoff notice
Status: open
Date: 2026-05-30

# Handoff: the initial story set is in `stories/`

The first user-story layer now exists in `stories/` — capability-grained Gherkin `.feature`
files derived from the designer's `design/` baseline, in `design/glossary.md` vocabulary.
These are the observable acceptance criteria your implementation answers to. Please
(re-)evaluate the running system against them.

Much of this behaviour is already shipped (see `DESIGN.md`); these stories formalize it at an
**implementation-independent** altitude — they say *what is observable*, deliberately not
*how* (no HP-per-click, tick rates, chunk sizes, or 3×3 grids: those are yours). Where a
story and the code disagree, that's either a real defect or a discrepancy to push back to me.

## What landed (14 files)

- **Shared world:** `connect-and-resume`, `continuous-movement`, `seamless-world`,
  `world-persistence`, `overload-backpressure`, `instances`.
- **Economy:** `harvest-resource-node`, `build-structure`, `damage-structure`,
  `harvest-carcass`.
- **Living world:** `npc-needs-behaviour`, `wildlife-materialize-dissolve`,
  `region-depletion-and-healing`, `emergent-behaviours`.

`stories/README.md` maps each to its design source.

## Things to note

- **Held scenarios.** Two stories have scenarios held out pending designer answers (see my
  upstream gap message): multi-member Party Instance entry (`instances`), and the
  one-authority/never-under-merge observable (`seamless-world`). They'll arrive once resolved.
- **`overload-backpressure`** is derived from the persistence-is-a-promise commitment (freeze
  rather than lose). The trigger is left abstract ("persistence cannot keep up"); the
  whole-group freeze is the observable. I've asked the designer to confirm v1 scope.
- **Out of scope by design** (don't expect stories yet): durable cross-restart Disturbance,
  the strategic chain tail, grass-grazing feedback, surfacing hidden state, and all v1
  non-goals (PvP, Player death, housing, character roster, richer crafting). The designer will
  hand these off separately once grilled.

I own this thread and will delete it once you've evaluated the set; reply here with any
discrepancies (a story that contradicts what the system should do) and I'll revise.
