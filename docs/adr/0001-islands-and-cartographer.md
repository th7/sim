---
status: accepted (design; not yet implemented)
---

# Simulation authority is interaction-clustered (Islands), not geographic (Chunks)

## Context

Dynamic entities that interact continuously — players colliding, NPCs chasing and fighting players — need a **single authority resolving each interaction**, or two owners compute the same collision against each other's stale state and disagree. A fixed geographic partition (one process per **Chunk**) inevitably draws authority boundaries *through* interactions, which forces entity handoffs at boundaries, per-tick neighbor reads (or replicated "halos") for cross-boundary collision, and races whose only guarantees are probabilistic — cross-sender message ordering, clock beats between independent tick timers. v1 dodged all of this because the only moving entity is the Player and Players don't collide with each other; NPCs and combat end the dodge.

The bar we set: behavior that holds *by construction*, not "overwhelmingly likely."

## Decision

Partition the **dynamic** simulation by *interaction locality* instead of geography.

- An **Island** is the single runtime authority over a connected cluster of interacting entities (**Players** now; NPCs later) together with the **Chunks** their activity spans. It simulates all movement, collision, and combat among its members in one process, so every interaction is resolved by one authority, locally.
- A singleton **Cartographer** assigns entities and Chunks to Islands and **creates, merges, and splits** Islands as entities move, maintaining the invariant that any two entities able to interact are already in the same Island. As the sole arbiter, it serializes every topology change — no two Islands race to merge.
- **Chunk** is demoted to *data*: worldgen output + durably-stored state + a region id, used for worldgen determinism, persistence keying, and spatial indexing. It is no longer a process. **Boundary crossing** as a process handoff is retired.
- **Actions** flow through an entity's Island (strict, single-authority). **Observation** is a separate, looser concern: Islands publish changed-only per-tick deltas to a geographic read-model that each **Session** pulls its **View window** from. You *act* through your Island; you *observe* geography.

## Considered options

1. **Keep one process per Chunk (status quo) + halos/routing for cross-boundary interaction.** Authority boundaries still cut through interactions. Crossing observation depends on cross-sender message ordering, which cannot be made a theorem; per-tick synchronous neighbor queries for collision risk mutual-call deadlock. Rejected: can't reach by-construction determinism for dynamic interaction.
2. **Per-entity authority (Session-owned Player position, process-per-NPC).** Excellent for v1 movement and makes single-writer-per-entity structural — but it *splits* interaction authority, so pairwise dynamic interaction (collision, combat) is resolved by two owners against stale state, and every continuous interaction becomes a per-tick cross-process read of every nearby mover. Rejected: optimizes the case we don't have and is worst for the case that's coming.
3. **Full shared spatial index (all entity state in ETS, ticked by one or more workers).** Boundaryless and strong for dense interaction, but discards OTP fault isolation, needs either a single-core ticker or a sharded write discipline, and couples the whole world to one backpressure blast radius. Rejected: gives up too much of the platform's strengths.

Islands were chosen as the only option that gets **single-authority-per-interaction** (determinism) *and* keeps **OTP fault isolation** (per-Island supervision; a crash is contained and re-homed) *and* scales parallelism *with activity* — many Islands tick in parallel, collapsing to one core only for a single genuinely dense cluster, which we accept.

## How it works (conceptually — all ordinary OTP)

- Islands run under a `DynamicSupervisor` (`:temporary` — a crashed Island is re-homed, never resurrected with stale state) and register in a `Registry` (`island_id → pid`, auto-removed on death).
- The Cartographer is a `:permanent` singleton holding `entity/region → Island` in its own state and messaging Sessions on (re)assignment. Resolution is rare (connect / merge / split), so this needs no shared table.
- A Session monitors its Island and re-resolves on `:DOWN` (covers merge and crash) or a split-notice (the one remap where the source Island doesn't die — reuses the existing per-Player event channel; today's `relocated` generalizes).
- **Merge** is *successor-ready-before-redirect*, sequenced entirely by the Cartographer (drain source → absorb into survivor → flip routing → retire source), so the old Island forwards late input during the brief window and nothing is lost or double-applied. Because merges happen in the interaction-free margin, no live interaction is ever split across two authorities.
- **Observation** uses one geographic ETS read-model: Islands publish changed-only deltas each tick; Sessions pull the deltas for their View window and stream changed-only upserts/removes. ETS earns its place here (hot, many-reader, lock-free, no message ordering) and nowhere else — routing stays plain message-passing.

## Consequences

- **Determinism (the goal):** topology changes are totally ordered by one arbiter; each interaction has exactly one authority; the merge-before-interaction margin makes "two entities that can touch are co-located" a theorem. Margin rule: `merge_threshold ≥ interaction_range + max_closing_speed × handoff_latency + hysteresis`, and `≥ chunk size` so a Chunk needed by two Islands forces their merge. (These numbers are still to be pinned against the tick rate and max speed.)
- **Changed-only streaming is preserved** — the Island authors the deltas, so the wire still carries only what changed; the Session no longer brute-diffs its whole view.
- **Crash blast radius grows** from one Chunk to one Island's region + its entities' live positions. Mitigated: the **Datastore remains the durability boundary** (Islands own runtime only — the same relationship Chunks had), prompt emission keeps the unflushed window tiny, and recovery is re-home + re-hydrate.
- **The biggest fight is bounded by a single core.** Accepted.
- **Retired / changed:** `ChunkMigration` and the per-Chunk snapshot channels go away; **Chunk** becomes data; **Boundary crossing** is removed from the glossary; the cross-Chunk collision ("clip-and-stop") artifact is resolved because an Island owns every Chunk its entities span.
- **New language:** **Island** and **Cartographer** enter CONTEXT.md; **Warm set**, **View window**, **Chunk activation/deactivation**, **Datastore**, and **Backpressure** are reframed around Islands.
