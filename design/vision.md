# Vision

## What this is

A **cooperative, persistent, real-time** world that **Players** share. You drop into one
living place alongside everyone else, move through it freely, and leave a mark on it —
gathering, building, and (over time) fighting your way through a world that remembers what
you did and reacts to it.

The camera is isometric; movement is continuous (no grid steps); the server is authoritative
and everyone sees one consistent truth.

## Pillars

These are the commitments every design decision answers to.

1. **One shared world, always.** There is a single **Overworld** that all Players inhabit
   together — not instanced copies, not separate shards a Player chooses between. Two Players
   standing in the same place are *in the same place*. The world is seamless: a Player crosses
   it without loading screens, region gates, or boundary stutter.

2. **The world persists and remembers.** What you carry, what you build, and how the world has
   been used outlive your session and survive a restart. A wall you raised is still standing
   when you return; a forest you overhunted is still recovering. Persistence is a *promise*,
   not a best-effort — the system stalls before it loses your state (see
   [`shared-world.md`](./shared-world.md)).

3. **Cooperative first.** Players work *with* each other. The core loop — gather, build, and
   fight the world — is something a group does together. PvP is an eventual concern, explicitly
   **not** a v1 goal; nothing in v1 should foreclose it, but nothing in v1 should assume it.

4. **A living world, not a backdrop.** The wild world has its own life: animals that hunt,
   flee, herd, and pack up; regions that deplete when overused and heal when left alone; a
   world whose state was shaped by history, not scripted on the spot (see
   [`living-world.md`](./living-world.md)).

5. **Real-time and reactive.** The world runs continuously at a fixed tick; interactions
   resolve immediately and locally. A Player's actions have prompt, legible consequences.

## Who it's for

Players who want a sense of a *real, continuous, shared place* — where presence matters
(others are genuinely here), where effort accumulates (building and gathering persist), and
where the environment pushes back (a world that reacts rather than waits).

## Non-goals (v1)

Stated so design stays honest about scope. Each may return later; none is assumed in v1.

- **PvP.** Players do not fight each other in v1. Players are invulnerable.
- **Player death.** Players do not die in v1 (hence **Carcass**, never "corpse").
- **Player housing / persistent private space.** **Instances** are *Party-scoped* dungeon
  content, not owned spaces. There is no per-Player private world.
- **A character roster.** One username = one in-world entity. No account/character split.
- **Crafting recipes beyond build cost.** The only "recipe" in v1 is a **Structure**'s build
  cost in **Items**. A richer crafting economy is future work.

## The shape of a session

A Player connects under their username and resumes exactly where they logged off, carrying
what they carried. They move through the shared Overworld, harvest **Resource nodes** and hunt
wildlife for **Items**, spend those Items building **Structures**, and — with a **Party** —
step through a **Portal** into a private **Instance** for dungeon content, returning to the
spot they left. Everything in the Overworld they touch persists; the Instance is theirs only
for as long as the Party is inside it.
