# NPC motivation: one immediacy rule at three levels, pressure only at goal arbitration

An **NPC** produces its per-tick **Intent** from a **Motivation** engine built on a *single* selection rule — *pick the most-immediate actionable option* — applied at three levels: the most-immediate actionable node of a **Behavioral chain** becomes the chain's **Bid**; the winning Bid across chains becomes the **Goal**; the most-immediate actionable **Action** sequence for that Goal becomes the **Plan**, whose head resolves to Intent. Cross-need weighing happens in exactly **one** place — goal arbitration — where a static per-Need priority **bias** is modulated by **Pressure**, a leaky, hard-capped, sim-clock integral of each Need's own activation that decays while the Need is satisfied. Pressure plays no part within a chain.

## Why this shape

Two behaviours the design wanted both fall out of this rule for free, rather than being authored:

- **Foresight** (stockpile before starving, fortify before attacked): a chain *climbs* toward its strategic end automatically, because each immediate node deactivates as it is satisfied, exposing the next-up node as the new most-immediate-actionable Bid.
- **Chronic-need trade-offs** ("been hungry often → sacrifice safety"): Pressure lifts a long-unmet Need's Bid past the static bias at goal arbitration — and *only* there, so it changes *which* goal is pursued, never the tactics within it. A hungry wolf under attack still never plans "eat"; it plans "fight-to-hold" because that is the actionable way to pursue the `feed` Goal the pressure selected.

## Considered and rejected

- **Behaviour tree / FSM** — predictable, but every transition is hand-authored; the long-horizon behaviour the design wants does not emerge, it must be written.
- **GOAP (goal-oriented planning)** — gives real planning, but per-tick replanning is too costly at 20 Hz across many NPCs and is awkward to make deterministic. We get "planning-like" behaviour from the chain climb without a search.
- **Per-plan multi-objective scoring** (score each candidate plan by its side-effects across *all* Needs) — more expressive, but expensive, and the same emergent behaviour is reproduced by placing the single cross-need decision at goal arbitration and keeping plan selection a pure precondition-gated climb.

## Consequences

- Per-NPC per-tick cost is small and bounded: a handful of scalar Pressure updates plus a precondition scan. Plans are *selected*, never *searched*.
- Deterministic by construction: Pressure is a sim-clock integral with fixed rates and a fixed cap; bias is static; immediacy is a function of precondition state. No wall-clock, no unseeded RNG.
- **Actions are a shared library**, owned by no Need — the same `attack` Action serves a `feed` Goal or a `survive` Goal. Adding a Need does not add Actions, and vice versa.
