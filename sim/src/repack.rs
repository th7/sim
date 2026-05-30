//! Repack policy — a **pure** decision (no threads here; Phase 2 drives real
//! workers with it). Given each cluster's smoothed tick-time, pack clusters
//! onto as few workers as possible without any worker exceeding a time budget.
//!
//! This is bin-packing; we use First-Fit-Decreasing, which is deterministic and
//! within a small constant of optimal. The key properties the model relies on:
//!
//! - **Whole clusters only.** A worker sheds clusters to a new worker; it never
//!   splits one. This is free because distinct clusters never interact (the
//!   Labeler guarantees it), so moving a cluster across the worker cut changes
//!   nothing about correctness.
//! - **The one-core floor.** A single cluster whose tick-time exceeds the budget
//!   gets a worker to itself and stays there — it cannot be subdivided. This is
//!   the accepted single-core dense-fight ceiling (DESIGN.md).

use crate::ids::{ClusterId, WorkerId};
use std::collections::BTreeMap;

/// Smoothing factor for the cluster tick-time EWMA (`new = α·sample + (1-α)·old`).
pub const EWMA_ALPHA: f64 = 0.2;

/// Update an exponentially-weighted moving average of tick-time.
pub fn ewma(prev: f64, sample: f64) -> f64 {
    EWMA_ALPHA * sample + (1.0 - EWMA_ALPHA) * prev
}

/// Assign clusters to workers via First-Fit-Decreasing under `budget` (the
/// max tick-time a worker should carry). Returns `cluster → worker`. Worker ids
/// are dense from 0. Deterministic: clusters are ordered by tick-time
/// descending, then by id ascending.
pub fn repack(times: &BTreeMap<ClusterId, f64>, budget: f64) -> BTreeMap<ClusterId, WorkerId> {
    let mut order: Vec<(ClusterId, f64)> = times.iter().map(|(&c, &t)| (c, t)).collect();
    // Heaviest first; ties broken by id for determinism.
    order.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut worker_loads: Vec<f64> = Vec::new();
    let mut assignment = BTreeMap::new();

    for (cluster, time) in order {
        // First worker that can still fit this cluster under budget.
        let fit = worker_loads
            .iter()
            .position(|&load| load + time <= budget);
        let w = match fit {
            Some(w) => w,
            None => {
                worker_loads.push(0.0);
                worker_loads.len() - 1
            }
        };
        worker_loads[w] += time;
        assignment.insert(cluster, WorkerId(w as u32));
    }

    assignment
}

/// How many workers a `repack` assignment uses.
pub fn worker_count(assignment: &BTreeMap<ClusterId, WorkerId>) -> usize {
    assignment
        .values()
        .map(|w| w.0)
        .max()
        .map(|m| m as usize + 1)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(pairs: &[(u64, f64)]) -> BTreeMap<ClusterId, f64> {
        pairs.iter().map(|&(c, t)| (ClusterId(c), t)).collect()
    }

    #[test]
    fn empty_uses_no_workers() {
        let a = repack(&times(&[]), 1.0);
        assert_eq!(worker_count(&a), 0);
    }

    #[test]
    fn light_clusters_share_one_worker() {
        let a = repack(&times(&[(1, 0.2), (2, 0.2), (3, 0.2)]), 1.0);
        assert_eq!(worker_count(&a), 1);
        assert!(a.values().all(|&w| w == WorkerId(0)));
    }

    #[test]
    fn over_budget_total_splits_across_workers() {
        // Three clusters at 0.6 each, budget 1.0 → two fit per... no: 0.6+0.6>1,
        // so each worker holds one → 3 workers? FFD: w0=0.6, next 0.6 doesn't
        // fit (1.2>1) → w1=0.6, next 0.6 → w2. Three workers.
        let a = repack(&times(&[(1, 0.6), (2, 0.6), (3, 0.6)]), 1.0);
        assert_eq!(worker_count(&a), 3);
    }

    #[test]
    fn packs_to_minimize_workers() {
        // 0.7, 0.5, 0.5, 0.3 with budget 1.0. FFD desc: 0.7→w0(0.7); 0.5→w1(0.5);
        // 0.5→w1(1.0); 0.3→w0(1.0). Two workers.
        let a = repack(&times(&[(1, 0.7), (2, 0.5), (3, 0.5), (4, 0.3)]), 1.0);
        assert_eq!(worker_count(&a), 2);
    }

    #[test]
    fn single_over_budget_cluster_gets_its_own_worker() {
        // The indivisible dense cluster: 2.5 > budget 1.0 → one worker, alone.
        let a = repack(&times(&[(1, 2.5), (2, 0.1)]), 1.0);
        assert_eq!(worker_count(&a), 2);
        // The heavy one is alone on its worker.
        let heavy = a[&ClusterId(1)];
        assert!(a.iter().filter(|(_, &w)| w == heavy).count() == 1);
    }

    #[test]
    fn deterministic() {
        let t = times(&[(1, 0.5), (2, 0.5), (3, 0.5), (4, 0.5)]);
        assert_eq!(repack(&t, 1.0), repack(&t, 1.0));
    }

    #[test]
    fn ewma_smooths() {
        let a = ewma(1.0, 2.0);
        assert!((a - (0.2 * 2.0 + 0.8 * 1.0)).abs() < 1e-9);
    }
}
