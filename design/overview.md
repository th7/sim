# design/ — the project's design layer

This directory is the head of the workflow pipeline: the **why** (purpose) and the
**what / who / when / where** (a goal's shape and how it integrates) of the game. It is
written by the **designer** role, whose input is the human, not another role's output.

Mechanism — *how* a goal is met (storage, formats, data shapes, APIs, the runtime model) —
is deliberately **out of scope** here. That lives in the engineer's code and in
`docs/adr/`. A design doc names a goal and the promise it must keep; it does not prescribe
the implementation.

## What's here

- [`glossary.md`](./glossary.md) — the **canonical language**: every domain term, one
  canonical name, the aliases to avoid, and the relationships between terms. The source of
  truth for what words mean in this project.
- **Design docs**, one per coherent goal area:
  - [`vision.md`](./vision.md) — the north star: the kind of game, its pillars, its non-goals.
  - [`shared-world.md`](./shared-world.md) — the seamless, persistent, single shared world,
    and the private Instances branching off it.
  - [`economy.md`](./economy.md) — the gather → build loop and the material economy.
  - [`living-world.md`](./living-world.md) — the reactive wild ecosystem and its animals.

## Relationship to the root docs

This project predates the role pipeline; it grew under a single-agent **Work Loop**
(`AGENTS.md`) that kept its language and behaviour in root-level `*.md` files. As the design
layer is stood up, those settle as follows:

- **`CONTEXT.md` (root) is superseded by [`glossary.md`](./glossary.md).** The canonical
  language now lives here. `CONTEXT.md` should be retired or redirected to point at this
  file — an action outside the designer's write-scope, left to the engineer or the human.
  Until then, `glossary.md` wins on any conflict.
- **`DESIGN.md` (root)** describes *what the running system does from outside* — observed,
  shipped behaviour. That is downstream of design (the territory of the product owner's
  stories and the engineer's implementation), not a design artifact. It is not owned here.
- **`docs/adr/`** holds the *how* and its rationale — architecture decisions, human-ratified.
  Design docs reference an ADR where a goal is realized by a specific decision, but never
  restate it.
- **`AGENTS.md`, `AGENT_LOG.md`, `PLAN.md`, `BUGS.md`, `ROADMAP.md`** are the single-agent
  Work Loop's bookkeeping. They are not design artifacts.

## How design is written

- One canonical term per concept; conflicts called out and resolved in `glossary.md` the
  moment a term lands.
- The design tree is walked branch by branch, dependencies resolved one at a time, by
  interviewing the human — not by guessing.
- A doc is committed and pushed to `main` only once its open questions are resolved; anything
  with a live question is held.

## Coordinating with other roles

The `messages/` directory (created when the first message exists) is the role-to-role
channel. When a design doc that the product owner depends on changes, the designer pushes a
**handoff notice** so the affected stories are re-derived; the designer also reads `stories/`
against these designs and flags a **discrepancy** (a story contradicts a design) or a **gap**
(a story covers behaviour the designs don't address).

As of this writing the designer is the only active role — no product owner or engineer clone
is running against the remote yet — so there are no inbound messages to act on.
