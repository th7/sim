---
status: proposed (decision record; supersedes ADR-0001 if accepted)
---

# Simulation runtime: Rust shared-memory clusters vs BEAM Islands

## Context

[ADR-0001](./0001-islands-and-cartographer.md) chose to partition the **dynamic** simulation by
*interaction locality* — an **Island** is the single authority over a connected cluster of interacting
entities — to get **single-authority-per-interaction** *by construction* rather than "overwhelmingly
likely". It realised that on the BEAM: process-per-Island under a `DynamicSupervisor`, a singleton
**Cartographer** serialising topology changes, merge-before-redirect handshakes, and an ETS read-model
for observation. ADR-0001 is accepted as *design, not yet implemented*.

[IDEA.md](../../IDEA.md) asked a narrower question: does the interaction-clustered *model* actually hold,
and could a **Rust shared-memory** structure deliver the same by-construction property more simply — and
what is the single-core dense-cluster ceiling we are accepting either way? We built that POC end to end
(`/sim`): one shared ECS world per realm, dynamic actors partitioned into **clusters**, a single
serialized **Labeler** (the Cartographer in shared-memory form), parallel cluster execution, a Datastore
with flush/backpressure, and a Phoenix-Channels-v2 server. It is **feature- and wire-compatible** with the
Elixir game (the existing frontend connects unchanged) and is covered by ~93 tests.

This ADR records the resulting decision: which runtime to carry forward.

## Findings from the POC

- **Never-under-merge holds by construction.** Every topology mutation reconciles to the canonical
  footprint-overlap partition; "two entities that can interact share a cluster" is a theorem, checked
  against an oracle and a randomized property test, not a margin we tune. (The same determinism goal as
  ADR-0001, reached without merge-before-redirect handshakes or cross-process ordering.)
- **The single-core ceiling is generous.** A single indivisible dense cluster cost ≈ **0.085 ms/tick** at
  500 movers × 1500 obstacles, against a 50 ms (20 Hz) budget — ~600× headroom at that density. This is
  the ceiling *both* designs accept (ADR-0001: "the biggest fight is bounded by a single core. Accepted.").
- **Parallelism needs no `unsafe`.** Clusters are entity-disjoint by construction, so per-cluster compute
  parallelises over owned data with zero `unsafe`; output is asserted identical to the serial tick. The
  shared-memory `unsafe` boundary ADR-0001-era reasoning feared never materialised.
- **Cross-chunk collision is resolved**, and the whole class of boundary handoffs / cross-sender ordering
  races is *absent* (single process, single shared world) rather than mitigated.
- **Full wire/feature parity** with no change to the committed contract or the frontend.

## Decision

**Recommend adopting the Rust interaction-clustered runtime as the simulation authority, contingent on
closing the fault-tolerance gap below.** It satisfies ADR-0001's primary goal (single-authority-per-
interaction *by construction*) with a structurally simpler design, and this project explicitly weights
by-construction guarantees over "probably fine". The decision is not unconditional — see Consequences and
the acceptance checklist.

If the fault-tolerance / operational costs are judged to outweigh the determinism and simplicity gains,
**stay on ADR-0001** (BEAM Islands); the model is identical, only the host changes.

## Considered options

1. **BEAM Islands (ADR-0001).** Process-per-Island, Cartographer, merge-before-redirect, ETS read-model.
   *For:* OTP fault isolation (a crash is contained to one Island and re-homed), hot code reload, the
   mature Phoenix/PubSub/Ecto stack already in use, one language. *Against:* topology changes are a
   distributed protocol (drain → absorb → flip routing → retire) whose correctness rests on careful
   sequencing and message ordering; observation needs an ETS read-model bolted beside message-passing;
   the determinism is argued, not mechanically checked.

2. **Rust shared-memory clusters (this POC).** One shared world per realm, serialized Labeler,
   reconcile-to-canonical, parallel-by-disjointness. *For:* determinism is a theorem and is property-
   tested; no handoff/ordering races to reason about; cross-chunk collision falls out for free; the
   single-core ceiling is measured and generous; parallelism without `unsafe`. *Against:* gives up OTP
   fault isolation and hot reload; needs a new persistence integration (Ecto → Rust); crash/fault handling
   is deferred; adds a second language to the stack.

3. **Hybrid (Rust simulation core embedded in the BEAM, e.g. via a NIF/port).** Keep OTP supervision,
   PubSub and Ecto; run the cluster core in Rust. *For:* fault isolation + ecosystem *and* the Rust core.
   *Against:* a NIF that ticks at 20 Hz and holds the world is a long-running, panic-sensitive boundary
   (a NIF panic can take the whole node, erasing the OTP benefit); marshalling the read-model across the
   boundary reintroduces copying. Not evaluated by the POC; recorded as a fallback to investigate only if
   option 2's fault-tolerance gap proves unacceptable but the determinism is still wanted.

## Consequences

What the Rust path **gives up** relative to ADR-0001, and what it owes before acceptance:

- **Fault isolation.** A crash takes the whole node, not one Island. Mitigations the POC has *not* yet
  built: catch per-cluster/per-worker panics and re-home the affected cluster from the Datastore
  (the durability boundary already bounds data loss to the unflushed window, exactly as for Islands);
  supervise the tick loop so a panic restarts the runtime rather than exits. **Acceptance checklist item.**
- **Hot code reload** is gone; deploys are process restarts. Restart-from-Datastore must be fast and
  lossless to compensate. **Acceptance checklist item.**
- **Persistence.** *Done* — a Postgres `DurableStore` (`sim/src/pgstore.rs`) now persists
  players/structures/depletions; the server flushes on SIGTERM and anchors its clock to wall-clock so
  depletion timing survives a real restart. Proven by the e2e specs phase3 + phase8 running green against
  the Rust backend, and by `sim/tests/pg_restart.rs`. (Uses its own DB schema, not Ecto's — fine for a
  standalone backend.)
- **Two languages.** The web/persistence apps are Elixir; the sim core would be Rust. Either the transport
  + persistence move to Rust (the POC already speaks the wire protocol) or a boundary is maintained.
- **NPCs / combat** — the interaction the model exists for beyond movement — are still unbuilt in both
  designs; the cluster model is ready for them (intra-cluster, single authority).

What it **gains**: determinism as a theorem; no handoff/ordering protocol to get right; cross-chunk
collision resolved; a measured, generous ceiling; parallelism without `unsafe`; and a running,
wire-compatible server today.

**Retired / changed if accepted:** ADR-0001's BEAM realisation (process-per-Island, Cartographer as a
GenServer, merge-before-redirect, ETS read-model) is superseded by the Rust realisation; the *model* and
its glossary (Island↔cluster, Cartographer↔Labeler) carry over unchanged. CONTEXT.md keeps its language;
only the implementation notes change.

**If rejected:** ADR-0001 stands; `/sim` remains a validated reference for the model (the property tests
and the ceiling number are reusable evidence regardless of host).
