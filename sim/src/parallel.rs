//! Parallel cluster execution (Phase 2).
//!
//! The Labeler guarantees clusters are **chunk-disjoint**, hence entity-disjoint:
//! every entity lives in exactly one chunk, and each chunk has exactly one
//! owning cluster (`chunk_owner` is a map). So the per-cluster movement
//! computation is embarrassingly parallel — distinct clusters share no state.
//!
//! ## On the `unsafe` boundary DESIGN.md anticipated
//!
//! DESIGN.md framed Phase 2 as "disjoint `&mut` into the shared world behind a
//! documented `unsafe` API". In practice the dominant cost is the collision
//! *computation* (O(movers × obstacles) per cluster), not the position
//! write-back. So we **extract** each cluster's inputs into owned data, run the
//! computation across a worker pool on that owned data (no shared access at
//! all — trivially `Send`, no `unsafe`), then **apply** the results serially.
//! This achieves the model's parallelism profile (throughput scales with the
//! number of independent clusters; a single indivisible dense cluster is the
//! one-core floor) while keeping soundness *by construction* rather than by an
//! `unsafe` precondition that a Labeler bug could violate. The cluster
//! disjointness is still the load-bearing invariant — it is what makes the jobs
//! independent — we simply don't need raw pointers to exploit it.
//!
//! Workers are assigned clusters by the Phase-1 [`crate::repack`] policy; the
//! result is independent of worker count and thread scheduling (applied in a
//! deterministic order), which the tests assert against the serial tick.

use crate::collision::{clamp_step, Obstacle};
use crate::ids::ClusterId;
use crate::world::Bounds;
use hecs::Entity;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

/// One cluster's movement inputs, fully owned so it can cross to a worker thread.
pub struct ClusterJob {
    pub cid: ClusterId,
    pub obstacles: Vec<Obstacle>,
    /// `(entity, x, y, vx, vy)` for each player-actor in the cluster.
    pub movers: Vec<(Entity, i64, i64, f64, f64)>,
    pub bounds: Option<Bounds>,
}

/// Computed new positions for one cluster, plus the wall-time the job took
/// (used to update the cluster's tick-time EWMA for repack).
pub struct ClusterResult {
    pub cid: ClusterId,
    pub positions: Vec<(Entity, i64, i64)>,
    pub elapsed_secs: f64,
}

/// Run one cluster's movement: integrate each mover's velocity, clamp against
/// the cluster's obstacles and bounds. Pure; no shared state.
pub fn run_cluster(job: &ClusterJob, dt: f64) -> ClusterResult {
    let start = Instant::now();
    let mut positions = Vec::with_capacity(job.movers.len());
    for &(e, x, y, vx, vy) in &job.movers {
        let step_x = (vx * dt).round() as i64;
        let step_y = (vy * dt).round() as i64;
        let (nx, ny) = clamp_step(x, y, step_x, step_y, &job.obstacles);
        let (nx, ny) = clamp_bounds(nx, ny, job.bounds);
        positions.push((e, nx, ny));
    }
    ClusterResult { cid: job.cid, positions, elapsed_secs: start.elapsed().as_secs_f64() }
}

fn clamp_bounds(x: i64, y: i64, bounds: Option<Bounds>) -> (i64, i64) {
    match bounds {
        None => (x, y),
        Some((x0, y0, x1, y1)) => (x.clamp(x0, x1), y.clamp(y0, y1)),
    }
}

/// Execute all cluster jobs across `worker_count` OS threads, grouping jobs by
/// the repack `assignment` (`cluster → worker`). Returns results keyed by
/// cluster id (deterministic order). Falls back to serial when `worker_count`
/// ≤ 1 or there is nothing to parallelize.
pub fn execute(
    jobs: Vec<ClusterJob>,
    assignment: &BTreeMap<ClusterId, u32>,
    worker_count: usize,
    dt: f64,
) -> BTreeMap<ClusterId, ClusterResult> {
    if worker_count <= 1 || jobs.len() <= 1 {
        return jobs.into_iter().map(|j| (j.cid, run_cluster(&j, dt))).collect();
    }

    // Bucket jobs by assigned worker (default worker 0 if unassigned).
    let mut buckets: Vec<Vec<ClusterJob>> = (0..worker_count).map(|_| Vec::new()).collect();
    for job in jobs {
        let w = assignment.get(&job.cid).copied().unwrap_or(0) as usize % worker_count;
        buckets[w].push(job);
    }

    let mut results: BTreeMap<ClusterId, ClusterResult> = BTreeMap::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = buckets
            .into_iter()
            .filter(|b| !b.is_empty())
            .map(|bucket| {
                scope.spawn(move || {
                    bucket
                        .iter()
                        .map(|job| run_cluster(job, dt))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            for r in h.join().expect("worker thread panicked") {
                results.insert(r.cid, r);
            }
        }
    });
    results
}

/// A **persistent** pool of worker threads — the faithful realization of
/// DESIGN.md's "workers self-tick". Spawned once and reused across ticks, so the
/// per-tick cost is just dispatch + join, not thread creation. Each tick the
/// caller hands it a batch of cluster jobs (already disjoint by construction);
/// each worker computes its assigned jobs on owned data and returns the results.
///
/// Measurement (`tests/ceiling.rs`) shows the per-tick OS-thread *spawn* model
/// loses on this workload because the compute is sub-millisecond; the pool
/// removes that overhead.
pub struct WorkerPool {
    senders: Vec<Sender<PoolMsg>>,
    results: Receiver<Vec<ClusterResult>>,
    handles: Vec<JoinHandle<()>>,
    size: usize,
}

enum PoolMsg {
    Run(Vec<ClusterJob>, f64),
    Shutdown,
}

impl WorkerPool {
    /// Create a pool of `size` worker threads (clamped to ≥1).
    pub fn new(size: usize) -> Self {
        let size = size.max(1);
        let (result_tx, results) = channel::<Vec<ClusterResult>>();
        let mut senders = Vec::with_capacity(size);
        let mut handles = Vec::with_capacity(size);
        for _ in 0..size {
            let (task_tx, task_rx) = channel::<PoolMsg>();
            let result_tx = result_tx.clone();
            let handle = std::thread::spawn(move || {
                while let Ok(msg) = task_rx.recv() {
                    match msg {
                        PoolMsg::Run(jobs, dt) => {
                            let out: Vec<ClusterResult> =
                                jobs.iter().map(|j| run_cluster(j, dt)).collect();
                            if result_tx.send(out).is_err() {
                                break;
                            }
                        }
                        PoolMsg::Shutdown => break,
                    }
                }
            });
            senders.push(task_tx);
            handles.push(handle);
        }
        WorkerPool { senders, results, handles, size }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Run all `jobs` on the pool, grouped by the repack `assignment`, and return
    /// results keyed by cluster id. Deterministic in output regardless of how
    /// work lands on threads.
    pub fn run(
        &self,
        jobs: Vec<ClusterJob>,
        assignment: &BTreeMap<ClusterId, u32>,
        dt: f64,
    ) -> BTreeMap<ClusterId, ClusterResult> {
        if jobs.is_empty() {
            return BTreeMap::new();
        }
        let mut buckets: Vec<Vec<ClusterJob>> = (0..self.size).map(|_| Vec::new()).collect();
        for job in jobs {
            let w = assignment.get(&job.cid).copied().unwrap_or(0) as usize % self.size;
            buckets[w].push(job);
        }

        let mut expected = 0;
        for (i, bucket) in buckets.into_iter().enumerate() {
            if bucket.is_empty() {
                continue;
            }
            self.senders[i]
                .send(PoolMsg::Run(bucket, dt))
                .expect("worker alive");
            expected += 1;
        }

        let mut results = BTreeMap::new();
        for _ in 0..expected {
            let batch = self.results.recv().expect("worker result");
            for r in batch {
                results.insert(r.cid, r);
            }
        }
        results
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        for s in &self.senders {
            let _ = s.send(PoolMsg::Shutdown);
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}
