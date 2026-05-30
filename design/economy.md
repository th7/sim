# The material economy

The core verb loop: **gather → build**. Players draw materials out of the world and spend them
shaping it. This doc states what that loop is for and the shape it must keep; the wild
ecosystem that feeds part of it has its own doc ([`living-world.md`](./living-world.md)).

## The loop

1. **Gather.** A Player harvests a **Gatherable** — a **Resource node** (tree, and later rock,
   ore, plant) or a **Carcass** — and receives **ItemStacks** into their **Inventory**.
2. **Carry.** The **Inventory** is what a Player holds; it persists across sessions.
3. **Build.** A Player spends **ItemStacks** to place a **Structure** in the **Overworld**,
   which then persists until destroyed.

The loop closes on the world itself: what you take from the world (materials) you put back
into the world (structures). Both endpoints are durable — the world remembers both the
depletion and the construction.

**Why it matters.** This is how a Player leaves a mark. Gathering makes the world's resources
*matter* (they deplete, they recover); building makes a Player's *effort* matter (it stays).
Together they turn a shared space into a shared, accumulating record of what its inhabitants
have done.

## Gatherables and the world's resources

A **Resource node** is world state, owned by no one. It **depletes** when harvested and
**respawns** on a timer — so a node is a renewable, contested commons, not a one-time pickup.
Its **Footprint** is the same whether full or depleted: harvesting yields materials, never a
shortcut through the world. Resource nodes are placed deterministically by Worldgen, so the
world's resource layout is a stable fact about a place, the same for everyone.

A **Carcass** is the other kind of Gatherable — the remains of a hunted animal, yielding
meat/hide. Unlike a Resource node it is *perishable* and *contested by NPCs as well as
Players*. It is the seam where the wild ecosystem feeds the material economy
(see [`living-world.md`](./living-world.md)).

## Items and Inventory

- An **Item** is a *kind* of substance (wood, stone, ore, meat, hide); an **ItemStack** is a
  quantity of one Item. Keep the type/quantity distinction sharp — it is the unit harvest
  yields and build costs are expressed in.
- A Player's **Inventory** is the set of ItemStacks they carry. It is filled by harvesting and
  drained by building, and it **persists**: what you carry at logout is what you carry on login.

## Building

A **Structure** is a persistent, Player-placed object anchored to a **Chunk**, owned by the
Player who placed it. Placing one **spends** its build cost (ItemStacks) from the placing
Player's Inventory. A Structure has a **Footprint** the world enforces — it blocks Player
movement — and a damage state: it can be damaged and destroyed, and both its existence and its
remaining integrity persist across restart.

**v1 scope.** Exactly one Structure type — a *wooden palisade* ("the wall"), cost 5 **wood**.
This is deliberately minimal: it proves the whole loop (harvest wood → spend wood → a
persistent, collidable, damageable thing in the shared world) end to end without committing to
a crafting tree.

## What v1 deliberately leaves out

- **Crafting recipes.** The only material transformation in v1 is a Structure's build cost.
  Multi-step recipes, stations, and intermediate goods are future work — which is why **Item**
  warns off "material" (it will collide with "crafting material" when recipes arrive).
- **Containers.** Materials live only in the carried **Inventory**; there are no chests or
  banks yet (which is why **Inventory** warns off "container").
- **More Gatherable kinds.** Trees are the only Resource node in v1; rock/ore/plant are named in
  the model as expected siblings but not yet present.

These are named here so that when they arrive they extend a known shape rather than reopening
the loop's foundations.
