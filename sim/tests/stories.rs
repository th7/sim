//! Story acceptance layer — the executable form of the user stories in
//! `stories/*.feature`.
//!
//! Each module is one `.feature`. For every scenario there is either a proving
//! `#[test]` here, or a citation to the test elsewhere in the suite that already
//! proves it (kept here so the story → test map is navigable from one place).
//! New tests here are the scenarios not yet pinned plus the edge/negative/
//! boundary cases the stories deliberately leave to engineering.
//!
//! Level: these assert the *observable* via the authoritative `Sim` (the wire
//! path itself is re-pinned end-to-end in `client/tests/integration.rs`).

use sim::components::{Inventory, Item, Position, StructureKind, WireId};
use sim::geometry::{chunk_center, ChunkCoord};
use sim::motivation::{Drives, NpcKind};
use sim::sim::Sim;
use sim::verbs::VerbError;
use sim::wire::{entity_states, EntityWire};

fn at(x: i64, y: i64) -> Position {
    Position { x, y }
}

fn with_wood(n: u32) -> Inventory {
    let mut inv = Inventory::default();
    inv.items.insert(Item::Wood, n);
    inv
}

fn wood(sim: &Sim, who: &str) -> u32 {
    sim.inventory_of(who).unwrap().items.get(&Item::Wood).copied().unwrap_or(0)
}

fn player_wire_count(sim: &Sim) -> usize {
    entity_states(sim.overworld())
        .values()
        .filter(|s| matches!(s, EntityWire::Player { .. }))
        .count()
}

fn has_npc(sim: &Sim, kind: NpcKind) -> bool {
    sim.npcs().iter().any(|(_, k, _, _, _)| *k == kind)
}

/// Walk `who` with the given unit intent until `pred` holds or `max` ticks pass.
fn walk_until(sim: &mut Sim, who: &str, dx: f64, dy: f64, max: usize, pred: impl Fn(&Sim) -> bool) {
    sim.set_intent(who, dx, dy);
    for _ in 0..max {
        if pred(sim) {
            return;
        }
        sim.tick();
    }
}

// ===========================================================================
// connect-and-resume.feature
// ===========================================================================
mod connect_and_resume {
    use super::*;

    /// Scenario: A new Player connects under a username.
    #[test]
    fn a_new_username_yields_exactly_one_controlled_entity() {
        let mut sim = Sim::new();
        sim.connect_at("ada", at(8_000, 8_000), Inventory::default());

        assert_eq!(sim.player_count(), 1, "exactly one in-world Player for 'ada'");
        assert_eq!(player_wire_count(&sim), 1, "exactly one Player entity on the wire");
        assert!(sim.position("ada").is_some(), "'ada' has an in-world entity");
    }

    /// Scenario: A returning Player resumes where they logged off.
    /// Proven by `persistence::reconnect_resumes_position_and_inventory` and
    /// `persistence::reconnect_replaces_prior_live_session`.
    #[test]
    fn returning_player_resumes_position_and_inventory() {
        let mut sim = Sim::new();
        sim.connect_at("ada", at(8_000, 8_000), Inventory::default());
        sim.harvest("ada", 8_000, 8_000).unwrap(); // wood 1
        walk_until(&mut sim, "ada", 1.0, 0.0, 5, |_| false);
        let saved = sim.position("ada").unwrap();

        sim.disconnect("ada");
        assert_eq!(sim.position("ada"), None, "logged off → no live entity");

        sim.connect("ada", saved.chunk());
        assert_eq!(sim.position("ada"), Some(saved), "resumes at the logoff position");
        assert_eq!(wood(&sim, "ada"), 1, "Inventory is exactly what it was at logoff");
    }
}

// ===========================================================================
// continuous-movement.feature
// ===========================================================================
mod continuous_movement {
    use super::*;

    /// Scenario: Movement is free and continuous.
    /// The per-tick step pins the continuous integrator in
    /// `core_model::movement_integrates_at_four_units_per_second`; here we pin
    /// that many ticks accumulate smoothly with no discrete grid snapping.
    #[test]
    fn movement_is_free_and_continuous() {
        let mut sim = Sim::new();
        sim.connect_at("p", at(8_000, 12_000), Inventory::default()); // y clear of trees
        sim.set_intent("p", 1.0, 0.0);
        let mut last = sim.position("p").unwrap().x;
        for _ in 0..40 {
            sim.tick();
            let x = sim.position("p").unwrap().x;
            let step = x - last;
            assert!((150..=250).contains(&step), "continuous ~200/tick step, got {step}");
            last = x;
        }
    }

    /// Scenario: The world blocks the Player at a Footprint.
    #[test]
    fn the_world_blocks_the_player_at_a_footprint() {
        let mut sim = Sim::new();
        // West of the centre-chunk tree cluster (trees sit around (8000,8000)).
        sim.connect_at("p", at(6_000, 8_000), Inventory::default());
        walk_until(&mut sim, "p", 1.0, 0.0, 100, |_| false);
        let x = sim.position("p").unwrap().x;
        // Unobstructed, 100 ticks east would reach ~26_000. Being held below the
        // tree centre proves the Footprint stopped the Player.
        assert!(x > 6_000, "the Player did move east");
        assert!(x < 8_000, "the Player is stopped at the Footprint, not through it (x={x})");
    }

    /// Scenario: Collision is one-way — the Player blocks nothing.
    #[test]
    fn collision_is_one_way_players_do_not_block_each_other() {
        let mut sim = Sim::new();
        sim.connect_at("a", at(8_000, 10_000), Inventory::default()); // y clear of trees
        sim.connect_at("b", at(9_000, 10_000), Inventory::default());
        walk_until(&mut sim, "a", 1.0, 0.0, 30, |s| s.position("a").unwrap().x > 9_600);
        assert!(
            sim.position("a").unwrap().x > 9_000,
            "'a' walked through the space 'b' occupies — Players don't block each other"
        );
        assert_eq!(sim.position("b").unwrap().x, 9_000, "'b' was not pushed");
    }

    /// Scenario: A depleted Resource node still blocks.
    #[test]
    fn a_depleted_node_still_blocks() {
        let mut sim = Sim::new();
        // Deplete the centre tree (the harvester is grandfathered through it).
        sim.connect_at("h", at(8_000, 8_000), Inventory::default());
        sim.harvest("h", 8_000, 8_000).unwrap();
        // A fresh Player who never overlapped it is still stopped by its Footprint.
        sim.connect_at("p", at(6_000, 8_000), Inventory::default());
        walk_until(&mut sim, "p", 1.0, 0.0, 100, |_| false);
        let x = sim.position("p").unwrap().x;
        assert!(x > 6_000 && x < 8_000, "a depleted node still blocks the Player (x={x})");
    }
}

// ===========================================================================
// seamless-world.feature
// ===========================================================================
mod seamless_world {
    use super::*;
    use sim::ids::Realm;

    /// Scenario: Crossing an internal Chunk boundary is a non-event.
    /// (Cluster footprint-following across the boundary is also pinned in
    /// `core_model::lone_player_cluster_owns_its_3x3_and_follows`.)
    #[test]
    fn crossing_a_chunk_boundary_is_continuous() {
        let mut sim = Sim::new();
        sim.connect_at("p", at(15_000, 12_000), Inventory::default()); // approaching x=16_000
        let start_chunk = sim.position("p").unwrap().chunk().cx;
        sim.set_intent("p", 1.0, 0.0);
        let mut last = sim.position("p").unwrap().x;
        let mut crossed = false;
        for _ in 0..40 {
            sim.tick();
            let p = sim.position("p").unwrap();
            let step = p.x - last;
            // No stall, no stutter, no teleport at the seam: the step stays the
            // steady continuous rate every tick, including the boundary tick.
            assert!((150..=250).contains(&step), "continuous across the boundary, got {step}");
            assert_eq!(sim.realm_of("p"), Some(Realm::Overworld), "still one Overworld");
            if p.chunk().cx > start_chunk {
                crossed = true;
            }
            last = p.x;
        }
        assert!(crossed, "the Player crossed into the next Chunk's area");
    }

    // Scenario: Reaching the edge of already-active space does not stall — a lone
    // Player's cluster keeps a 3×3 footprint hot ahead of them, so they never
    // walk into a not-yet-ready area. The footprint-follow is proven by
    // `core_model::lone_player_cluster_owns_its_3x3_and_follows`; the continuous
    // step above would catch any stall at the edge.

    // Held pending designer: the one-authority / never-under-merge promise has no
    // clean v1-observable (Players are invulnerable and don't interact). The
    // invariant itself is pinned structurally in `core_model::invariant_holds_over_random_walk`
    // and `parallel.rs`.
}

// ===========================================================================
// world-persistence.feature
// ===========================================================================
mod world_persistence {
    // Scenario Outline: Persisted facts survive a restart.
    //   - Player position + Inventory: `persistence::structure_survives_restart`
    //     setup + `persistence::reconnect_resumes_position_and_inventory`.
    //   - Structure existence + integrity: `persistence::structure_survives_restart`,
    //     `persistence::destroyed_structure_stays_gone_after_restart`.
    //   - Resource node depletion + respawn timer: `persistence::depletion_survives_restart`,
    //     `persistence::tree_depletion_survives_walking_away_until_chunk_stops`.
    //   Cross-restart durability against a real Postgres is `pg_restart.rs`.
    //
    // Scenario: Instance state does not persist —
    //   `persistence::mid_instance_disconnect_resumes_west_of_entry_portal`
    //   (the in-Instance session is gone; the Player re-homes to the Overworld)
    //   and `instances::an_instance_is_destroyed_when_the_last_player_leaves`.
}

// ===========================================================================
// overload-backpressure.feature
// ===========================================================================
mod overload_backpressure {
    // GAP — not yet implemented. The Datastore has a backpressure state machine
    // (`Mode::Flowing`/`Backpressured`, unit-tested in
    // `sim::datastore::tests::backpressure_engages_and_disengages`), but that
    // mode is read *nowhere outside datastore.rs*: it is not wired to stall
    // Player input, so the story's observable (a group of Players sharing one
    // authority freeze together, then resume with state intact) cannot be proven.
    //
    // Surfaced to the product owner — this story was itself flagged as derived /
    // v1-scope-uncertain. See `messages/engineer-to-product_owner-backpressure-not-wired.md`.
    //
    // When the freeze is wired, the ignored test below becomes the proving test.
    use super::*;

    #[test]
    #[ignore = "freeze-on-overload not wired to Player input; only the Datastore Mode machine exists (see messages/engineer-to-product_owner-backpressure-not-wired.md)"]
    fn players_freeze_under_overload_and_resume_intact() {
        // Intended: drive a shared-authority group's persistence into sustained
        // overload, assert their inputs stall (positions stop advancing) with no
        // state dropped, then assert play resumes once the buffer drains.
        let _ = Sim::new();
        unimplemented!("freeze-on-overload behaviour");
    }
}

// ===========================================================================
// instances.feature
// ===========================================================================
mod instances {
    use super::*;
    use sim::ids::Realm;

    /// Helper: connect overlapping the entry Portal and step into the Instance.
    fn enter_instance(sim: &mut Sim, who: &str) {
        sim.connect_at(who, at(4_400, 4_000), Inventory::default());
        sim.tick(); // process_portals detects the overlap → enter
        assert!(matches!(sim.realm_of(who), Some(Realm::Instance(_))), "entered the Instance");
    }

    // Scenario: Entering an Instance through a Portal, and
    // Scenario: Exiting returns the Player to where they entered —
    //   `verbs::portal_entry_and_exit_round_trip`.
    // Scenario: Disconnecting inside an Instance returns the Player beside the
    //   entry Portal — `persistence::mid_instance_disconnect_resumes_west_of_entry_portal`.

    /// Scenario: An Instance carries no shared-world fixtures.
    #[test]
    fn an_instance_has_no_resource_nodes_or_structures() {
        let mut sim = Sim::new();
        enter_instance(&mut sim, "p");
        // No Structures: building is refused outright inside an Instance.
        assert_eq!(
            sim.build("p", StructureKind::Wall, 23_000, 24_000),
            Err(VerbError::NoBuildInInstance),
            "an Instance hosts no Structures"
        );
        // No Resource nodes: a harvest in range finds nothing to gather.
        let p = sim.position("p").unwrap();
        assert_eq!(
            sim.harvest("p", p.x, p.y),
            Err(VerbError::NoTarget),
            "an Instance hosts no Resource nodes"
        );
    }

    /// Scenario: An Instance is destroyed when no one remains in it.
    #[test]
    fn an_instance_is_destroyed_when_the_last_player_leaves() {
        let mut sim = Sim::new();
        enter_instance(&mut sim, "p");
        assert_eq!(sim.instance_count(), 1, "one live Instance while occupied");

        sim.disconnect("p"); // the last (only) Player leaves
        assert_eq!(sim.instance_count(), 0, "the empty Instance is destroyed");
    }
}

// ===========================================================================
// harvest-resource-node.feature
// ===========================================================================
mod harvest_resource_node {
    use super::*;

    // Scenario: Harvesting a tree yields wood; Scenario: a harvested node
    // depletes; Scenario: a depleted node respawns on a timer —
    //   `verbs::harvest_yields_wood_and_depletes_then_respawns`.

    /// Scenario: A node's Footprint is unchanged by depletion.
    /// Movement-blocking of a depleted node is in
    /// `continuous_movement::a_depleted_node_still_blocks`; here the build path:
    /// a Structure still cannot be placed on a depleted node's cell.
    #[test]
    fn a_nodes_footprint_is_unchanged_by_depletion() {
        let mut sim = Sim::new();
        sim.connect_at("p", at(8_000, 8_000), Inventory::default());
        // Harvest the whole centre cluster → 5 wood; every Footprint stays solid.
        for (dx, dy) in [(0, 0), (500, 500), (500, -500), (-500, 500), (-500, -500)] {
            sim.harvest("p", 8_000 + dx, 8_000 + dy).unwrap();
        }
        assert_eq!(wood(&sim, "p"), 5);
        // Building on the depleted centre node is still Footprint-blocked, exactly
        // as it is for a full node (`verbs::build_errors`).
        assert_eq!(
            sim.build("p", StructureKind::Wall, 8_000, 8_000),
            Err(VerbError::FootprintBlocked),
            "a depleted node keeps its Footprint"
        );
    }
}

// ===========================================================================
// build-structure.feature
// ===========================================================================
mod build_structure {
    use super::*;

    // Scenario: Building a wooden palisade spends its cost (and is owned by the
    // placer); Scenario: building requires enough wood —
    //   `verbs::build_places_wall_and_spends_wood`, `verbs::build_errors`.

    /// Scenario: A built Structure blocks Player movement.
    #[test]
    fn a_built_structure_blocks_player_movement() {
        let mut sim = Sim::new();
        // Builder at the wall's west contact point places a wall at (3500,3000).
        sim.connect_at("builder", at(2_700, 3_000), with_wood(5));
        sim.build("builder", StructureKind::Wall, 3_500, 3_000).unwrap();

        // A fresh Player walking east into it is stopped before the wall.
        sim.connect_at("p", at(1_500, 3_000), Inventory::default());
        walk_until(&mut sim, "p", 1.0, 0.0, 100, |_| false);
        let x = sim.position("p").unwrap().x;
        assert!(x > 1_500, "the Player moved toward the wall");
        assert!(x < 3_500, "the Player is blocked by the palisade (x={x})");
    }
}

// ===========================================================================
// damage-structure.feature
// ===========================================================================
mod damage_structure {
    use super::*;

    // Scenario: A Structure can be damaged; Scenario: enough damage destroys it
    // (it no longer exists) — `verbs::damage_reduces_hp_and_destroys_at_zero`.

    /// Scenario: a destroyed Structure no longer blocks Player movement.
    #[test]
    fn a_destroyed_structure_no_longer_blocks_movement() {
        let mut sim = Sim::new();
        sim.connect_at("builder", at(2_700, 3_000), with_wood(5));
        sim.build("builder", StructureKind::Wall, 3_500, 3_000).unwrap();
        for _ in 0..4 {
            sim.damage("builder", 3_500, 3_000).unwrap(); // 100hp / 25 → 4 hits
        }
        assert!(
            !entity_states(sim.overworld()).contains_key(&WireId("structure:3500:3000".into())),
            "the wall is destroyed"
        );

        // A Player can now walk straight through where the wall stood.
        sim.connect_at("p", at(1_500, 3_000), Inventory::default());
        walk_until(&mut sim, "p", 1.0, 0.0, 120, |s| s.position("p").unwrap().x > 3_600);
        assert!(
            sim.position("p").unwrap().x > 3_500,
            "with the wall gone, the Player passes the former Footprint"
        );
    }
}

// ===========================================================================
// harvest-carcass.feature
// ===========================================================================
mod harvest_carcass {
    use super::*;

    // Scenario: A killed animal leaves a Carcass; Scenario: a Player harvests a
    // Carcass for meat and hide —
    //   `npc::player_kills_deer_into_carcass_then_harvests_meat_and_hide`.
    // Scenario: A Carcass is contested by NPCs as well as Players — a Player
    // draws via harvest (above); an NPC draws via eat in
    //   `npc::wolf_kills_and_eats_a_deer`.

    /// Scenario: A Carcass perishes if left.
    #[test]
    fn a_carcass_perishes_if_left() {
        let mut sim = Sim::new();
        sim.connect_at("alice", at(8_000, 8_000), Inventory::default());
        sim.spawn_npc(NpcKind::Deer, at(8_300, 8_000), Drives::default());
        // Two 25-dmg clicks kill the 50-HP deer, leaving a Carcass (proven harvestable
        // elsewhere — here we leave it untouched).
        sim.damage("alice", 8_300, 8_000).unwrap();
        sim.damage("alice", 8_300, 8_000).unwrap();
        assert!(!has_npc(&sim, NpcKind::Deer), "the deer is dead, leaving a Carcass");

        // CARCASS_PERISH_MS = 60_000 → 1200 ticks at 50 ms. Let it rot.
        for _ in 0..1_300 {
            sim.tick();
        }
        assert!(
            sim.harvest("alice", 8_300, 8_000).is_err(),
            "a left Carcass perishes and can no longer be harvested"
        );
    }
}

// ===========================================================================
// npc-needs-behaviour.feature
// ===========================================================================
mod npc_needs_behaviour {
    use super::*;

    // Scenario: A deer grazes when hungry and safe — `npc::unthreatened_hungry_deer_grazes_in_place`,
    //   `motivation::tests::deer_grazes_when_hungry_on_grass`.
    // Scenario: A deer flees a threat instead of feeding — `npc::wolf_hunts_deer_while_deer_flees`,
    //   `motivation::tests::deer_flees_threat_even_when_hungry`.
    // Scenario: A wolf hunts to satisfy its hunger — `npc::wolf_hunts_deer_while_deer_flees`,
    //   `npc::wolf_kills_and_eats_a_deer`, `motivation::tests::wolf_hunts_perceived_prey`.

    /// Scenario: A long-starving deer trades away safety to feed.
    /// A normally-hungry deer flees a wolf; a *long-starving* one (hunger pressure
    /// at the cap) whose acute fear has not yet built up grazes despite the threat.
    #[test]
    fn a_long_starving_deer_feeds_despite_a_threat() {
        let mut sim = Sim::new();
        // Deer starved to the cap, fear not yet integrated; a non-hungry wolf
        // (a pure threat, won't attack) just appeared one perception-range east.
        sim.spawn_npc(
            NpcKind::Deer,
            at(8_000, 8_000),
            Drives { hunger: 1.0, hunger_pressure: 1.0, safety_pressure: 0.0, ..Default::default() },
        );
        sim.spawn_npc(NpcKind::Wolf, at(8_800, 8_000), Drives { hunger: 0.0, ..Default::default() });

        let deer_x = |s: &Sim| {
            s.npcs().iter().find(|(_, k, _, _, _)| *k == NpcKind::Deer).map(|(_, _, p, _, _)| p.x).unwrap()
        };
        let start = deer_x(&sim);
        // Short window: before acute safety pressure overtakes chronic hunger.
        for _ in 0..5 {
            sim.tick();
        }
        assert_eq!(
            deer_x(&sim),
            start,
            "the starving deer grazes in place rather than fleeing the wolf"
        );
    }
}

// ===========================================================================
// wildlife-materialize-dissolve.feature
// ===========================================================================
mod wildlife_materialize_dissolve {
    use super::*;

    /// Centre of a chunk whose Region is strongly deer-rich.
    fn region_center(rich: bool) -> (i64, i64) {
        let sim = Sim::new();
        for k in 0..400 {
            let (cx, cy) = chunk_center(ChunkCoord::new(k, 0));
            let deer = sim.region_levels_at(cx, cy).deer;
            if (rich && deer > 0.6) || (!rich && deer < 0.25) {
                return (cx, cy);
            }
        }
        panic!("no {} region found", if rich { "deer-rich" } else { "deer-poor" });
    }

    // Scenario: Wildlife appears as a Player approaches — `ecosystem_world::wildlife_materializes_near_a_player`.
    // Scenario: Wildlife dissolves when no Player remains nearby — `ecosystem_world::wildlife_dissolves_when_the_player_leaves`.

    /// Scenario: Animals have no persistent individual identity.
    /// Leaving dissolves all wildlife; returning re-materializes a fresh set from
    /// the Region's level rather than restoring the same individuals.
    #[test]
    fn wildlife_has_no_persistent_individual_identity() {
        let (cx, cy) = region_center(true);
        let mut sim = Sim::new();
        sim.set_wildlife(true);
        sim.connect_at("alice", at(cx, cy), Inventory::default());
        sim.tick();
        let first: Vec<hecs::Entity> = sim.npcs().iter().map(|(e, ..)| *e).collect();
        assert!(!first.is_empty(), "wildlife materialized");

        // Leave: everything dissolves (NPCs don't anchor warmth).
        sim.disconnect("alice");
        sim.tick();
        assert!(sim.npcs().is_empty(), "wildlife dissolved when no Player remained");

        // Return: wildlife is back, consistent with the Region — but a freshly
        // materialized set, not the preserved individuals.
        sim.connect_at("alice", at(cx, cy), Inventory::default());
        sim.tick();
        assert!(!sim.npcs().is_empty(), "wildlife re-materialized on return");
        let second: Vec<hecs::Entity> = sim.npcs().iter().map(|(e, ..)| *e).collect();
        assert!(
            second.iter().all(|e| !first.contains(e)),
            "the returning wildlife are new entities, not the same individuals"
        );
    }

    /// Scenario: Population reflects the Region's current level.
    #[test]
    fn population_reflects_region_level() {
        let count_at = |x: i64, y: i64| {
            let mut sim = Sim::new();
            sim.set_wildlife(true);
            sim.connect_at("alice", at(x, y), Inventory::default());
            sim.tick();
            sim.npcs().iter().filter(|(_, k, _, _, _)| *k == NpcKind::Deer).count()
        };
        let (rx, ry) = region_center(true);
        let (px, py) = region_center(false);
        let rich = count_at(rx, ry);
        let poor = count_at(px, py);
        assert!(
            rich > poor,
            "a richer Region materializes more deer than a poorer one ({rich} vs {poor})"
        );
    }
}

// ===========================================================================
// region-depletion-and-healing.feature
// ===========================================================================
mod region_depletion_and_healing {
    use super::*;

    fn deer_rich_center() -> (i64, i64) {
        let sim = Sim::new();
        for k in 0..400 {
            let (cx, cy) = chunk_center(ChunkCoord::new(k, 0));
            if sim.region_levels_at(cx, cy).deer > 0.6 {
                return (cx, cy);
            }
        }
        panic!("no deer-rich region found");
    }

    // Scenario: Overhunting depletes a Region — `ecosystem_world::overhunting_a_region_lowers_its_deer_level`.
    // Scenario: A depleted Region spawns fewer / more aggressive animals — the
    //   depleted level drives `population_reflects_region_level` (fewer), and
    //   `ecosystem::initial_drives` makes a depleted Region's animals hungrier
    //   (more aggressive); the spawn path is `Sim::materialize`.

    /// Scenario: A Region heals when left alone.
    #[test]
    fn a_depleted_region_heals_toward_baseline_over_time() {
        let (cx, cy) = deer_rich_center();
        let mut sim = Sim::new();
        sim.set_wildlife(true);
        let baseline = sim.region_levels_at(cx, cy).deer;

        // Overhunt: a ravenous wolf thins the observed herd, then the Player
        // leaves so the kill folds into a negative Region Disturbance.
        sim.connect_at("alice", at(cx, cy), Inventory::default());
        sim.spawn_npc(
            NpcKind::Wolf,
            at(cx, cy),
            Drives { hunger: 1.0, hunger_pressure: 1.0, ..Default::default() },
        );
        for _ in 0..400 {
            sim.tick();
        }
        sim.disconnect("alice");
        sim.tick();
        let depleted = sim.region_levels_at(cx, cy).deer;
        assert!(depleted < baseline, "overhunting depleted the Region ({baseline} -> {depleted})");

        // Left alone, the Disturbance heals (deer τ = 600 s); after 200 s the level
        // has recovered measurably back toward baseline without overshooting.
        for _ in 0..4_000 {
            sim.tick();
        }
        let healed = sim.region_levels_at(cx, cy).deer;
        assert!(healed > depleted, "the Region heals toward baseline ({depleted} -> {healed})");
        assert!(healed <= baseline + 1e-6, "healing recovers toward — not past — baseline");
    }
}

// ===========================================================================
// emergent-behaviours.feature
// ===========================================================================
mod emergent_behaviours {
    // Scenario: Deer herd together — `npc::scattered_deer_form_a_herd`,
    //   `motivation::tests::calm_deer_steers_toward_its_herd`.
    // Scenario: A startled herd stampedes — `npc::a_herd_flees_a_predator_together`,
    //   `motivation::tests::deer_catches_a_neighbours_panic_and_flees`.
    // Scenario: Wolves pack-hunt — `npc::wolves_pack_onto_a_single_deer`,
    //   `motivation::tests::pack_wolf_targets_prey_nearest_the_pack_centroid`.
    // Scenario: Animals are bolder at night — `motivation::tests::wolf_is_bolder_at_night`,
    //   `motivation::tests::deer_is_warier_at_night`.
    // Scenario: A wounded animal is warier —
    //   `motivation::tests::a_wounded_wolf_disengages_from_a_fight_it_would_take_at_full_health`.
    //
    // These emergent behaviours are pinned at the unit level (the arbitration in
    // `motivation.rs`) and the integration level (real actors through the Sim tick
    // in `npc.rs`); no further proving test is needed here.
}
