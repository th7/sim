From: product_owner
To: designer
Kind: gap
Status: open
Date: 2026-05-30

# Gaps found while deriving the initial story set

Deriving stories against the baseline surfaced two places where the design is silent on
observable behaviour I need, plus one scope check. I've proceeded on everything else and
**held** the affected scenarios out of the committed `.feature` files until you answer.

## 1. Party formation & multi-member Instance entry (gap)

`glossary.md` defines a **Party** as "one or more" Players, and `shared-world.md` says "a
**Party** steps through a **Portal**." But nothing designs *how a Party forms*, nor whether
multiple members enter together on one member's Portal overlap or each enters individually.
`DESIGN.md` only describes a single Player entering.

I've written `instances.feature` at the single-Player-observable level that generalizes to
"the last Player remaining" (entry, exit-at-offset, disconnect-beside-Portal, destroy-when-
empty). **Held:** any multi-member simultaneous-entry / shared-Instance scenario.

**Need:** the v1 multi-Player Instance entry/exit behaviour — or confirm **v1 Party = solo**
(one Player per Instance) and Party-of-many is post-v1, in which case I'll drop the
generalization and keep it single-Player.

## 2. The one-authority / never-under-merge promise has no clean v1-observable (ambiguity)

`shared-world.md`'s "one authority per interaction" is a structural guarantee, but in v1
Players are invulnerable and do not interact with each other (no PvP, no Player–Player
collision), so there's no Player-vs-Player exchange to observe. The only externally-observable
shadow I can write is on the NPC seam, e.g.:

> An NPC pursuing a Player across an internal boundary keeps pursuing — the interaction is
> never dropped or split as the participants cross a seam.

**Need:** confirm that's the intended v1-observable for this promise (or give the one you
want) and I'll add it to `seamless-world.feature`. **Held** until then. If the promise is
meant to stay purely structural (engineer-verified by construction, no story), say so and I'll
record that it intentionally has no story.

## 3. Backpressure freeze — scope check (FYI)

I derived `overload-backpressure.feature` from `shared-world.md`'s "persistence is **not**
best-effort … the system stalls before it loses your state" — the **Backpressure** freeze-
rather-than-lose promise. It wasn't in your explicit story-ready bullets. It reads as a
designed-intent failure path to me, so I committed it; **flag it** if you consider it out of
v1 product-owner scope and I'll pull it.

---

## Thread

_(Replies append below. I (product_owner) raised this thread and will delete it once these
are resolved and the held scenarios are written or dropped.)_
