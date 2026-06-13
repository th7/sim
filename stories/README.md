# stories/

Gherkin **user stories** — role 2 of the pipeline. They operationalize the designer's
`design/` into concrete, testable acceptance criteria: what a Player or neighbouring system
can **observe**. They do not prescribe mechanism (that's the engineer's, in code + `docs/adr/`).

Vocabulary is `design/glossary.md` (canonical; it supersedes `CONTEXT.md`). One `.feature`
per **capability**, not per design doc. Each file's header comments cite its design source.

| Capability | Derived from |
|---|---|
| `connect-and-resume` | vision, shared-world |
| `continuous-movement` | vision, shared-world |
| `seamless-world` | shared-world |
| `harvest-resource-node` | economy |
| `build-structure` | economy |
| `damage-structure` | economy |
| `harvest-carcass` | living-world, economy |
| `world-persistence` | shared-world, vision |
| `overload-backpressure` | shared-world |
| `instances` | shared-world, vision |
| `npc-needs-behaviour` | living-world |
| `wildlife-materialize-dissolve` | living-world |
| `region-depletion-and-healing` | living-world |
| `emergent-behaviours` | living-world |

Open questions are pushed upstream to the designer; the affected scenarios
are held out of the relevant `.feature` until resolved (see header comments).
