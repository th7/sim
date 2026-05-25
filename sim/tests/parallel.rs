//! Phase 2: parallel cluster execution must be observationally identical to the
//! serial tick (soundness by construction), preserve the never-under-merge
//! invariant, and exhibit the one-core floor for a single indivisible cluster.

use sim::components::{Inventory, Position};
use sim::harness::{assert_invariant, Rng};
use sim::ids::Realm;
use sim::sim::Sim;
use std::collections::BTreeMap;

fn at(x: i64, y: i64) -> Position {
    Position { x, y }
}

fn spawn_spread(sim: &mut Sim, names: &[&str]) {
    for (i, n) in names.iter().enumerate() {
        // Spread across several chunks so multiple clusters form and split/merge.
        let x = 8_000 + (i as i64 % 4) * 9_000;
        let y = 8_000 + (i as i64 / 4) * 9_000;
        sim.connect_at(n, at(x, y), Inventory::default());
    }
}

/// Final observable state: each player's (position, cluster-id), plus cluster
/// count.
fn observe(sim: &Sim, names: &[&str]) -> (Vec<(String, Position, u64)>, usize) {
    let v = names
        .iter()
        .map(|n| (n.to_string(), sim.position(n).unwrap(), sim.cluster_of(n).unwrap().0))
        .collect();
    (v, sim.overworld().labeler.cluster_count())
}

fn drive(sim: &mut Sim, names: &[&str], ticks: usize, parallel: Option<(usize, f64)>) {
    let mut rng = Rng::new(0x5151_2323);
    for _ in 0..ticks {
        for n in names {
            if rng.below(3) == 0 {
                let (dx, dy) = rng.intent();
                sim.set_intent(n, dx, dy);
            }
        }
        match parallel {
            Some((w, b)) => sim.tick_parallel(w, b),
            None => sim.tick(),
        }
        assert_invariant(sim, Realm::Overworld);
    }
}

#[test]
fn parallel_matches_serial_for_any_worker_count() {
    let names = ["a", "b", "c", "d", "e", "f", "g", "h"];

    let mut serial = Sim::new();
    spawn_spread(&mut serial, &names);
    drive(&mut serial, &names, 250, None);
    let expected = observe(&serial, &names);

    for workers in [1usize, 2, 3, 8] {
        let mut par = Sim::new();
        spawn_spread(&mut par, &names);
        // budget 0.0 ⇒ clusters spread across workers once they have nonzero
        // measured time, genuinely exercising the worker threads.
        drive(&mut par, &names, 250, Some((workers, 0.0)));
        assert_eq!(
            observe(&par, &names),
            expected,
            "parallel tick with {workers} workers must equal the serial tick"
        );
    }
}

#[test]
fn pooled_sim_matches_serial() {
    let names = ["a", "b", "c", "d", "e", "f"];

    let mut serial = Sim::new();
    spawn_spread(&mut serial, &names);
    drive(&mut serial, &names, 200, None);
    let expected = observe(&serial, &names);

    let mut pooled = Sim::new();
    pooled.enable_pool(4);
    spawn_spread(&mut pooled, &names);
    drive(&mut pooled, &names, 200, Some((4, 0.0)));
    assert_eq!(observe(&pooled, &names), expected, "pooled Sim must equal serial");
}

#[test]
fn single_dense_cluster_is_one_job_the_one_core_floor() {
    // Pack many players into a single chunk: they are all chunk-neighbours, so
    // the Labeler keeps them in ONE cluster — which the repack policy can only
    // place on a single worker. This is the accepted single-core ceiling.
    let mut sim = Sim::new();
    let names: Vec<String> = (0..64).map(|i| format!("p{i}")).collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    for (i, n) in refs.iter().enumerate() {
        // All within chunk (0,0), clear of the central trees (y ≈ 12_000).
        sim.connect_at(n, at(2_000 + (i as i64 % 8) * 1_000, 12_000), Inventory::default());
    }
    assert_eq!(sim.overworld().labeler.cluster_count(), 1, "one dense cluster");

    // Even with 8 workers offered, the cluster cannot be subdivided.
    for _ in 0..20 {
        sim.tick_parallel(8, 0.0);
    }
    assert_invariant(&sim, Realm::Overworld);
    assert_eq!(sim.overworld().labeler.cluster_count(), 1, "still one cluster — the floor");
}

#[test]
fn execute_spreads_clusters_and_matches_serial_compute() {
    use sim::collision::Obstacle;
    use sim::ids::ClusterId;
    use sim::parallel::{execute, run_cluster, ClusterJob};

    // Build several independent jobs with a forced spread assignment.
    let make_job = |cid: u64, x: i64| ClusterJob {
        cid: ClusterId(cid),
        obstacles: vec![Obstacle {
            x: x + 5_000,
            y: 0,
            footprint: sim::components::Footprint::Circle { radius: 300 },
        }],
        movers: vec![(hecs::Entity::DANGLING, x, 0, 4_000.0, 0.0)],
        bounds: None,
    };
    let jobs: Vec<ClusterJob> = (0..6).map(|i| make_job(i, i as i64 * 20_000)).collect();

    // Serial reference.
    let serial: BTreeMap<ClusterId, Vec<(hecs::Entity, i64, i64)>> = jobs
        .iter()
        .map(|j| (j.cid, run_cluster(j, 0.05).positions))
        .collect();

    // Force each cluster onto its own worker (mod the pool size).
    let assignment: BTreeMap<ClusterId, u32> =
        (0..6).map(|i| (ClusterId(i), i as u32)).collect();
    let par = execute(jobs, &assignment, 4, 0.05);

    for (cid, positions) in &serial {
        assert_eq!(&par[cid].positions, positions, "cluster {cid:?} compute must match");
    }
}
