# PLAN — next increment

The forward-looking plan for the **next increment of work** (see AGENTS.md → **Work Loop**). Rewritten at
the end of each *Clarify* step. The agent's decision log, design parameters, and follow-ups live in
`AGENT_LOG.md`; shipped behaviour is in `DESIGN.md`; rationale is in `docs/adr/` and `CONTEXT.md`.

_No increment currently in flight._

## Candidate next increments

Drawn from `AGENT_LOG.md` → "Recommended follow-ups". Pick and scope one during the next Clarify.

- **Cross-restart persistence of Region Disturbances** — a `PersistEvent` variant + Datastore/pg schema so
  the overhunt→deplete→heal field survives a restart.
- **Grass-grazing Disturbance** — deer grazing writes a grass Disturbance, closing the ecosystem feedback loop.
- **Strategic chain tail** — `carry → stockpile → secure-source` + the in-session cache object (likely an ADR).
