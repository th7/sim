# The cold ecosystem is a deterministic field, not a simulation

Wildlife outside Player-hot **Chunks** is not simulated at all. The **Overworld** is partitioned by a deterministic Worley/Voronoi function into **Regions**, each carrying a **Habitat**. The wildlife level at any place and time is a pure function — `Baseline(habitat, season(t), local noise)` — adjusted by a sparse, persisted, per-Region **Disturbance** that decays exponentially back to zero:

```
level(region, t) = clamp( Baseline(region, t) + Δ · e^(−(t − t₀)/τ) )
```

Warming a Chunk turns `level` into **seeded spawn chances** (representative, not a literal head-count); warm hunting/grazing writes the Region's Disturbance `(Δ, t₀)`. There is **no cold tick, no diffusion step, no population integrator**.

## Considered and rejected

- **Reaction–diffusion cold sim** (per-chunk grass/deer/wolf scalars diffused each cold tick): yields genuinely emergent cross-map population waves, but needs a background tick over an unbounded chunk set, is Lotka–Volterra-unstable without careful tuning, and couples chunks so it cannot be evaluated lazily. Rejected for unbounded cost and for the "probably-stable" tail this project avoids.
- **Frozen persistent individuals + need catch-up** (the prior ADR-0005 draft): keeps individual animals but models no population dynamics (counts static while cold) and needs a per-entity persistence row. Superseded.

## Consequences

- **Cost ∝ player disturbances, not map size.** Untouched wilderness is free — it is a function. A Region's current state is O(1) and closed-form, so a warming Chunk needs no catch-up loop.
- **Deterministic and bounded by construction**: state is always `Baseline ± a decaying, clamped Δ`. Nothing collapses to extinction or explodes; there is no stability to tune.
- **No autonomous cold-layer predator–prey dynamics.** Wolves do not thin a herd while unobserved; real predator–prey play happens only in warm Chunks and leaves a healing **Disturbance**. Spatial structure (territories, corridors) is baked into the Baseline *statically* rather than emerging from diffusion.
- Composes with ADR-0005's activity-proportional gating: the cold world is a sparse Disturbance set plus a pure function — exactly the closed-form "evaluate forward" philosophy, lifted from per-NPC needs to the ecosystem field.
