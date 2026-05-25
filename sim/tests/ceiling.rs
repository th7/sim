//! Phase 2 measurement: the single-core dense-cluster ceiling, and parallel
//! scaling across many independent clusters. Informational — asserts only
//! correctness (parallel == serial), printing timings with `--nocapture`. Run
//! under `--release` for representative numbers.

use sim::collision::Obstacle;
use sim::components::Footprint;
use sim::ids::ClusterId;
use sim::parallel::{run_cluster, ClusterJob, WorkerPool};
use std::collections::BTreeMap;
use std::time::Instant;

fn dense_job(cid: u64, movers: usize, obstacles: usize) -> ClusterJob {
    let obs = (0..obstacles)
        .map(|i| Obstacle {
            x: (i as i64 % 50) * 320,
            y: (i as i64 / 50) * 320,
            footprint: Footprint::Circle { radius: 300 },
        })
        .collect();
    let movers = (0..movers)
        .map(|i| (hecs::Entity::DANGLING, (i as i64 % 40) * 400, (i as i64 / 40) * 400, 4_000.0, 0.0))
        .collect();
    ClusterJob { cid: ClusterId(cid), obstacles: obs, movers, bounds: None }
}

#[test]
fn single_core_dense_cluster_ceiling() {
    // One indivisible cluster: this per-tick cost is the floor — it cannot be
    // parallelised away (IDEA.md's accepted single-core ceiling).
    let movers = 500;
    let obstacles = 1_500;
    let job = dense_job(0, movers, obstacles);

    // Warm + measure a few iterations.
    let mut total = 0.0;
    let iters = 20;
    for _ in 0..iters {
        let r = run_cluster(&job, 0.05);
        total += r.elapsed_secs;
        assert_eq!(r.positions.len(), movers);
    }
    let per_tick_ms = (total / iters as f64) * 1_000.0;
    eprintln!(
        "single-core dense-cluster ceiling: {per_tick_ms:.3} ms/tick \
         ({movers} movers × {obstacles} obstacles); 20 Hz budget is 50 ms/tick"
    );
}

#[test]
fn parallel_scaling_across_independent_clusters_matches_serial() {
    // Substantial per-cluster work so the compute dominates the per-tick thread
    // spawn cost. (A production system uses a persistent worker pool that
    // self-ticks — IDEA.md — rather than spawning per tick; this executor
    // spawns per call, so it only wins once the work is well above that fixed
    // overhead.)
    let cluster_count = 96;
    let jobs: Vec<ClusterJob> = (0..cluster_count).map(|i| dense_job(i, 200, 2_000)).collect();

    // Serial reference timing + result.
    let t0 = Instant::now();
    let serial: BTreeMap<ClusterId, Vec<(hecs::Entity, i64, i64)>> =
        jobs.iter().map(|j| (j.cid, run_cluster(j, 0.05).positions)).collect();
    let serial_ms = t0.elapsed().as_secs_f64() * 1_000.0;

    // Persistent pool across the machine's cores; each cluster on its own worker
    // (mod pool size) via a spread assignment. Warm the pool first so we measure
    // dispatch, not thread creation.
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let pool = WorkerPool::new(workers);
    let assignment: BTreeMap<ClusterId, u32> =
        (0..cluster_count).map(|i| (ClusterId(i), i as u32)).collect();

    let warm: Vec<ClusterJob> = (0..cluster_count).map(|i| dense_job(i, 200, 2_000)).collect();
    let _ = pool.run(warm, &assignment, 0.05);

    let jobs_par: Vec<ClusterJob> = (0..cluster_count).map(|i| dense_job(i, 200, 2_000)).collect();
    let t1 = Instant::now();
    let par = pool.run(jobs_par, &assignment, 0.05);
    let par_ms = t1.elapsed().as_secs_f64() * 1_000.0;

    // Correctness: identical results.
    for (cid, positions) in &serial {
        assert_eq!(&par[cid].positions, positions);
    }

    let speedup = if par_ms > 0.0 { serial_ms / par_ms } else { 0.0 };
    eprintln!(
        "parallel scaling: {cluster_count} independent clusters — serial {serial_ms:.2} ms, \
         {workers}-worker persistent pool {par_ms:.2} ms (speedup {speedup:.2}×)"
    );
}
