# The shared world

The first pillar: **one shared world, always** — seamless, persistent, and consistent. This
doc states the goals that pillar imposes and the promises the system must keep. *How* those
promises are kept is the engineer's — see ADRs [0001](../docs/adr/0001-islands-and-cartographer.md),
[0002](../docs/adr/0002-rust-clustered-simulation-runtime.md).

## One place, shared

There is exactly one **Overworld**, and every **Player** is in it together. This is a design
commitment, not an implementation detail to be relaxed under load: we do not offer
per-Player copies, chosen shards, or parallel realms a group picks between. Standing next to
another Player means you are *actually* next to them, in the one world.

**Why it matters.** Shared presence is the point of a cooperative world. If two Players can
ever end up in different copies of the "same" place, cooperation becomes a matchmaking problem
and the world stops feeling real. Everything below serves keeping the world single and
seamless.

## Seamless movement

A Player moves through the Overworld continuously — no grid steps, no loading screens at
boundaries, no stutter or clip as they cross from one part of the world to another. The world
is partitioned internally (into **Chunks**, for generation and storage), but those seams are
**invisible to the Player**: crossing a Chunk boundary is a non-event.

**The promise:** a Player never perceives the world's internal partitioning. Boundary
artifacts — stalling on a not-yet-ready area, an interaction failing because two participants
sit on opposite sides of a seam — are design defects, not acceptable costs.

## One authority per interaction

Every interaction between dynamic entities — collision, and (later) combat — is resolved by a
**single authority**, locally and consistently. Two Players who can affect each other are
always simulated together; there is never a moment where each is governed by a different
authority that might disagree about what happened.

This is the design intent behind the **Island** / **Cartographer** model: the world is
partitioned by *who can interact with whom* (interaction locality), not by geography, and the
partition is reshaped continuously so that the "can interact" set and the "simulated together"
set are always the same. The standing invariant is *never-under-merge*: entities able to
interact are **never** left in separate authorities. (The Cartographer is the sole arbiter of
the partition, so the reshaping never races itself.)

**Why it matters.** A shared world with authoritative, consistent interactions is only
coherent if every interaction has exactly one decider. Split authority is how you get two
Players who each think they won the same exchange. We require this to hold *by construction*,
not "almost always" — see the project's preference for structural determinism.

**Cost we accept:** a single dense interaction (a big fight in one spot) is bounded by what one
authority can carry. We accept that ceiling in exchange for never splitting an interaction. A
quiet world costs almost nothing; a crowded one concentrates cost where the crowd is.

## The world persists and remembers

What a Player does to the Overworld outlives their session and survives a server restart:

- **Where you are and what you carry** — position and **Inventory** resume on login.
- **What you built** — a **Structure**'s existence and its damage state persist; a wall you
  raised, or one chipped down to near-destruction, is found exactly so on return.
- **How the world has been used** — **Resource node** depletion/respawn timers, and the wild
  ecosystem's **Disturbance** field (see [`living-world.md`](./living-world.md)).

**The promise — persistence is not best-effort.** The system must **never silently lose or
corrupt** a Player's state. Under overload it is acceptable for affected Players to *freeze*
(their inputs stall until the system recovers); it is **not** acceptable to drop a write or
crash. "Your wall might not have saved" is a broken promise; "the world paused for a moment
under load" is a tolerable one. (This is the **Datastore**'s and **Backpressure**'s reason for
existing; mechanism is engineer-owned.)

### What does *not* persist

**Instance** state is in-memory only and is gone on disconnect or shutdown — by design (see
below).

## Instances — private content off the shared world

Some content is meant to be *yours and your Party's*, not shared: dungeons. A **Party** steps
through a **Portal** and enters an **Instance** — an ephemeral, private region simulated the
same way the Overworld is, but scoped to that Party, never persisted, and destroyed when the
Party leaves or the last member disconnects.

Design intent and boundaries:

- **The Overworld is the home; the Instance is a detour.** A Player entering an Instance is
  *re-homed* into it and the Overworld is irrelevant to them until they leave. On exit they
  return to the **Chunk** they entered from, at the entry Portal. A Player who disconnects
  inside an Instance returns *next to* the entry Portal — never looped back into a lost
  Instance.
- **Instances carry no shared-world fixtures.** No **Resource nodes**, no **Structures** inside
  an Instance. A Structure a Player was standing on when they entered simply waits for them in
  the Overworld.
- **A Party owns the Instance, not a Player.** It exists for the Party and dies with it. A
  disconnecting Player leaves the Party; an empty Party destroys the Instance. There is no
  owned, persistent private space in v1.

**Why ephemeral.** Persistence is a promise we make about the *shared* world. Instances are
private, disposable content; making them persistent would mean per-Party persistent spaces —
explicitly out of v1 scope (see [`vision.md`](./vision.md) non-goals).
