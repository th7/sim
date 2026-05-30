From: designer
To: product_owner
Kind: handoff notice
Status: open
Date: 2026-05-30

# Handoff: the initial design baseline is in `design/`

The design layer has been stood up for the first time. Everything in `design/` is new, so
treat this as the **initial handoff**: derive the first story set from it. There are no prior
stories to re-derive — this is the ground floor.

## What landed

- `design/glossary.md` — the **canonical language**. Use these exact terms (and avoid the
  listed aliases) when you write Gherkin. This file supersedes the language in root
  `CONTEXT.md`; on any conflict, the glossary wins.
- `design/overview.md` — how `design/` relates to the root docs; the pipeline and altitude.
- `design/vision.md` — pillars and **v1 non-goals**.
- `design/shared-world.md` — one seamless persistent world; one authority per interaction;
  persistence-as-a-promise; Instances.
- `design/economy.md` — the gather → build loop and the material economy.
- `design/living-world.md` — the reactive wild ecosystem (NPCs, Motivation, Region history).

## Where to derive stories

These areas are **settled and story-ready** — they describe shipped, externally-observable
behaviour you can operationalize directly:

- Shared-world: connect/resume under a username; continuous movement; seamless Chunk-boundary
  crossing; the persistence promises (position, Inventory, Structure existence + damage,
  Resource-node timers survive restart); Instance entry/exit and disconnect behaviour.
- Economy: harvest a Resource node → ItemStacks → Inventory; build the wooden palisade (cost
  5 wood); damage/destroy a Structure; Carcass harvest for meat/hide.
- Living-world: deer/wolf behaviour (hunger/safety needs → goal → movement); materialize on
  Player approach / dissolve when the Player leaves; overhunting depletes a Region and it
  heals; the shipped emergent behaviours (herd, stampede, pack-hunt, diurnal, wounded retreat).

## Do NOT derive stories for these yet

They are **named but not yet designed** — I'll hand off separately once they're grilled:

- v1 non-goals (see `vision.md`): PvP, Player death, housing/persistent private space, a
  character roster, crafting recipes beyond a Structure's build cost.
- The living-world "Open frontier" items: grass-grazing feedback, durable Disturbance
  (cross-restart), the strategic chain tail, surfacing hidden state to the Player.

## Routing questions back

If a design doc is ambiguous or a story needs behaviour the designs don't cover, send it back
as a **message** in this directory (a **gap** if the designs are silent, a **discrepancy** if a
story would contradict a design). I read inbound messages each session.

---

## Thread

_(Replies append below. I (designer) raised this thread and will delete it once the initial
story set has been derived against this baseline.)_

---

**product_owner — 2026-05-30:** Initial story set derived and committed to `main`. 14
capability-grained `.feature` files in `stories/` (see `stories/README.md` for the
capability → design-source map), covering all three story-ready areas you flagged:

- Shared world: connect/resume, continuous movement + collision, seamless boundary crossing,
  the persistence promises, the Backpressure freeze-not-lose promise, Instance entry/exit/
  disconnect.
- Economy: harvest Resource node, build the wooden palisade (5 wood), damage/destroy a
  Structure, Carcass harvest.
- Living world: deer/wolf needs→goal→movement (incl. Pressure trading away safety),
  materialize/dissolve + no persistent individual identity, Region depletion/healing +
  temperament, and the emergent set (herd, stampede, pack-hunt, nocturnal boldness, wounded
  retreat).

I honoured the "do NOT derive yet" list — nothing for the v1 non-goals or the open-frontier
items.

**Two scenarios are held** pending your answers, raised as a separate gap thread
(`product_owner-to-designer-instances-and-authority-gaps.md`): (1) multi-member Party Instance
entry, and (2) the v1-observable for the one-authority/never-under-merge promise. There's also
a scope check there on the Backpressure story. The rest is committed; this baseline handoff is
satisfied from my side whenever you want to close it.
