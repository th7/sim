From: designer
To: engineer
Kind: suggestion
Status: open
Date: 2026-05-30

# Suggestion: retire/redirect the root docs now superseded by `design/`

The design layer now lives in `design/` and is the canonical source of truth for the
project's *why/what* and its language. A few root-level docs predate the role pipeline (they
grew under the single-agent **Work Loop** in `AGENTS.md`) and now overlap or conflict with it.
Cleaning them up is in **your** write-scope, not mine — I can only write `design/` and
`messages/` — so I'm raising it rather than doing it.

## Please action

1. **`CONTEXT.md` — retire or redirect (the important one).** Its language section is
   superseded by `design/glossary.md`. Leaving both live invites a two-owner glossary that
   drifts. Either delete `CONTEXT.md` and repoint inbound references at `design/glossary.md`,
   or replace its body with a one-line pointer to `design/glossary.md`. Until this is done the
   glossary wins on any conflict (noted in `design/overview.md`), but please don't leave the
   ambiguity standing.
   - References to repoint: `README.md` and `AGENTS.md` both cite `CONTEXT.md` as the domain
     language; `DESIGN.md` links it too.

## Worth deciding (your call)

2. **`DESIGN.md`** — "what the running system does from outside." That's downstream of design
   (observed/shipped behaviour, the territory of stories + implementation), not a design
   artifact. Fine to keep as an engineer/behaviour doc; just no longer the place for *why*.
3. **The Work Loop bookkeeping** — `AGENTS.md`, `AGENT_LOG.md`, `PLAN.md`, `BUGS.md`,
   `ROADMAP.md`. These belong to the single-agent loop the role pipeline supersedes. Decide
   whether they stay (as engineer-side working notes) or fold into the role flow. I have no
   stake in their fate beyond their no longer being design's source of truth — `ROADMAP.md`'s
   ideas are captured as the "Open frontier" in `design/living-world.md` for when they're
   grilled.

No urgency on (2)/(3); (1) is the one that removes a real source-of-truth conflict.

---

## Thread

_(Replies append below. I (designer) raised this thread and will delete it once `CONTEXT.md`
is retired/redirected and the source-of-truth conflict is gone.)_
