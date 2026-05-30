# NPCs are not Warm-set anchors; wildlife is fungible across the cold boundary

An **NPC** is a full cluster actor while a **Player** is near, but unlike a Player it does **not** anchor the **Warm set**: only Players keep **Chunks** hot, and NPCs are simulated only inside Player-hot Chunks. NPCs have **no persistent individual identity**. When a Chunk warms, wildlife *materializes* from the local ecosystem level (a seeded spawn chance — see ADR-0006); when it cools, the survivors *dissolve* back into that **Region**'s **Disturbance**. There is no per-individual freeze, no per-entity persistence row, and no per-individual need catch-up.

## Why

Cost then scales with **Player activity, not total NPC population**, preserving the architecture's activity-proportional promise ("many tiny Islands in a quiet world"). The alternative — every NPC seeding its own cluster and ticking at 20 Hz forever — keeps the whole inhabited map permanently hot. Rejected.

Making individuals *fungible* (rather than freezing and persisting each one) means the cold world carries only a sparse per-Region **Disturbance**, not a row per animal — and it composes with the deterministic ecosystem field (ADR-0006) instead of fighting it.

## Consequences

- The long-horizon motivation behaviour (ADR-0004: stockpiling, securing a food source) is **observable within a warm visit but does not accrue as individual world state** — when the Player leaves, the individual dissolves; only its *aggregate* mark on the Region's **Disturbance** persists, and that heals over time.
- No NPC persistence schema beyond the per-Region Disturbance set.
- A returning Player may find a Region's wildlife shifted (a lingering Disturbance) but never a *specific* animal they remember — identity is per-visit.
