//! Parallel island execution (Phase 2).
//!
//! The Cartographer guarantees islands are **chunk-disjoint**, hence entity-disjoint:
//! every entity lives in exactly one chunk, and each chunk has exactly one
//! owning island (`chunk_owner` is a map). So the per-island movement
//! computation is embarrassingly parallel — distinct islands share no state.
//!
//! ## On the `unsafe` boundary
//!
//! Phase 2 was framed as "disjoint `&mut` into the shared world behind a
//! documented `unsafe` API". In practice the dominant cost is the collision
//! *computation* (O(movers × obstacles) per island), not the position
//! write-back. So we **extract** each island's inputs into owned data, run the
//! computation across a worker pool on that owned data (no shared access at
//! all — trivially `Send`, no `unsafe`), then **apply** the results serially.
//! This achieves the model's parallelism profile (throughput scales with the
//! number of independent islands; a single indivisible dense island is the
//! one-core floor) while keeping soundness *by construction* rather than by an
//! `unsafe` precondition that a Cartographer bug could violate. The island
//! disjointness is still the load-bearing invariant — it is what makes the jobs
//! independent — we simply don't need raw pointers to exploit it.
//!
//! Workers are assigned islands by the Phase-1 [`crate::repack`] policy; the
//! result is independent of worker count and thread scheduling (applied in a
//! deterministic order), which the tests assert against the serial tick.

use crate::collision::Obstacle;
use crate::ids::IslandId;
use crate::world::Bounds;
use hecs::Entity;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

/// One island's movement inputs, fully owned so it can cross to a worker thread.
pub struct IslandJob {
    pub iid: IslandId,
    pub obstacles: Vec<Obstacle>,
    /// `(entity, x, y, vx, vy)` for each player-actor in the island.
    pub movers: Vec<(Entity, i64, i64, f64, f64)>,
    pub bounds: Option<Bounds>,
}

/// Computed new positions for one island, plus the wall-time the job took
/// (used to update the island's tick-time EWMA for repack).
pub struct IslandResult {
    pub iid: IslandId,
    pub positions: Vec<(Entity, i64, i64)>,
    pub elapsed_secs: f64,
}

/// A worker's output for one dispatched batch: the per-island results, or the
/// panic payload if computing the batch panicked. The pool propagates the
/// payload to the calling (tick) thread so a worker bug crashes the runtime
/// cleanly via the tick's panic guard, rather than hanging on a missing result.
type WorkerOutput = std::thread::Result<Vec<IslandResult>>;

/// Test-only hook: when set, [`run_island`] panics, standing in for a worker
/// bug so the panic-propagation path can be exercised from a test.
#[cfg(test)]
pub(crate) static PANIC_IN_RUN_ISLAND: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Run one island's movement: integrate each mover's velocity, clamp against
/// the island's obstacles and bounds. Pure; no shared state.
pub fn run_island(job: &IslandJob, dt: f64) -> IslandResult {
    #[cfg(test)]
    if PANIC_IN_RUN_ISLAND.load(std::sync::atomic::Ordering::Relaxed) {
        panic!("injected run_island panic (test)");
    }
    let start = Instant::now();
    let mut positions = Vec::with_capacity(job.movers.len());
    for &(e, x, y, vx, vy) in &job.movers {
        let (nx, ny) = simcore::motion::step_actor(x, y, vx, vy, dt, &job.obstacles);
        let (nx, ny) = clamp_bounds(nx, ny, job.bounds);
        positions.push((e, nx, ny));
    }
    IslandResult { iid: job.iid, positions, elapsed_secs: start.elapsed().as_secs_f64() }
}

fn clamp_bounds(x: i64, y: i64, bounds: Option<Bounds>) -> (i64, i64) {
    match bounds {
        None => (x, y),
        Some((x0, y0, x1, y1)) => (x.clamp(x0, x1), y.clamp(y0, y1)),
    }
}

/// Execute all island jobs across `worker_count` OS threads, grouping jobs by
/// the repack `assignment` (`island → worker`). Returns results keyed by
/// island id (deterministic order). Falls back to serial when `worker_count`
/// ≤ 1 or there is nothing to parallelize.
pub fn execute(
    jobs: Vec<IslandJob>,
    assignment: &BTreeMap<IslandId, u32>,
    worker_count: usize,
    dt: f64,
) -> BTreeMap<IslandId, IslandResult> {
    if worker_count <= 1 || jobs.len() <= 1 {
        return jobs.into_iter().map(|j| (j.iid, run_island(&j, dt))).collect();
    }

    // Bucket jobs by assigned worker (default worker 0 if unassigned).
    let mut buckets: Vec<Vec<IslandJob>> = (0..worker_count).map(|_| Vec::new()).collect();
    for job in jobs {
        let w = assignment.get(&job.iid).copied().unwrap_or(0) as usize % worker_count;
        buckets[w].push(job);
    }

    let mut results: BTreeMap<IslandId, IslandResult> = BTreeMap::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = buckets
            .into_iter()
            .filter(|b| !b.is_empty())
            .map(|bucket| {
                scope.spawn(move || {
                    bucket
                        .iter()
                        .map(|job| run_island(job, dt))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            match h.join() {
                Ok(batch) => {
                    for r in batch {
                        results.insert(r.iid, r);
                    }
                }
                // Re-raise a worker panic on this thread so the tick's guard sees it.
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    results
}

/// A **persistent** pool of worker threads — the faithful realization of
/// the "workers self-tick" model. Spawned once and reused across ticks, so the
/// per-tick cost is just dispatch + join, not thread creation. Each tick the
/// caller hands it a batch of island jobs (already disjoint by construction);
/// each worker computes its assigned jobs on owned data and returns the results.
///
/// Measurement (`tests/ceiling.rs`) shows the per-tick OS-thread *spawn* model
/// loses on this workload because the compute is sub-millisecond; the pool
/// removes that overhead.
pub struct WorkerPool {
    senders: Vec<Sender<PoolMsg>>,
    results: Receiver<WorkerOutput>,
    handles: Vec<JoinHandle<()>>,
    size: usize,
}

enum PoolMsg {
    Run(Vec<IslandJob>, f64),
    Shutdown,
}

impl WorkerPool {
    /// Create a pool of `size` worker threads (clamped to ≥1).
    pub fn new(size: usize) -> Self {
        let size = size.max(1);
        let (result_tx, results) = channel::<WorkerOutput>();
        let mut senders = Vec::with_capacity(size);
        let mut handles = Vec::with_capacity(size);
        for _ in 0..size {
            let (task_tx, task_rx) = channel::<PoolMsg>();
            let result_tx = result_tx.clone();
            let handle = std::thread::spawn(move || {
                while let Ok(msg) = task_rx.recv() {
                    match msg {
                        PoolMsg::Run(jobs, dt) => {
                            // Isolate a worker bug: catch the panic and ship the
                            // payload back so the tick thread re-raises it (a clean
                            // crash) instead of this thread dying and the caller
                            // blocking forever on the missing result.
                            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                jobs.iter().map(|j| run_island(j, dt)).collect::<Vec<_>>()
                            }));
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
    /// results keyed by island id. Deterministic in output regardless of how
    /// work lands on threads.
    pub fn run(
        &self,
        jobs: Vec<IslandJob>,
        assignment: &BTreeMap<IslandId, u32>,
        dt: f64,
    ) -> BTreeMap<IslandId, IslandResult> {
        if jobs.is_empty() {
            return BTreeMap::new();
        }
        let mut buckets: Vec<Vec<IslandJob>> = (0..self.size).map(|_| Vec::new()).collect();
        for job in jobs {
            let w = assignment.get(&job.iid).copied().unwrap_or(0) as usize % self.size;
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
            match self.results.recv() {
                Ok(Ok(batch)) => {
                    for r in batch {
                        results.insert(r.iid, r);
                    }
                }
                // A worker panicked computing its batch: re-raise on this (the
                // tick) thread so the tick's panic guard flushes and takes the
                // runtime down — rather than blocking forever on a result that
                // will never arrive.
                Ok(Err(payload)) => std::panic::resume_unwind(payload),
                Err(_) => panic!("worker pool thread died before sending its result"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// A panic inside a worker must surface to the caller as a panic (so the
    /// tick's guard can crash cleanly) — never silently block `run()` forever on
    /// a result the dead worker will never send.
    #[test]
    fn a_worker_panic_propagates_to_the_caller_not_a_hang() {
        let pool = WorkerPool::new(2);
        let jobs = vec![IslandJob {
            iid: IslandId(0),
            obstacles: Vec::new(),
            movers: Vec::new(),
            bounds: None,
        }];

        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        PANIC_IN_RUN_ISLAND.store(true, Ordering::Relaxed);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pool.run(jobs, &BTreeMap::new(), 0.05)
        }));
        PANIC_IN_RUN_ISLAND.store(false, Ordering::Relaxed);
        std::panic::set_hook(prev);

        assert!(res.is_err(), "a worker panic must reach the caller, not hang or vanish");
    }
}
