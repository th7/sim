//! The cold **ecosystem** as a deterministic field (ADR-0006), not a simulation.
//!
//! The Overworld is partitioned by a Worley/Voronoi function into **Regions**,
//! each with a **Habitat**. Wildlife level at a place and time is a pure function
//! — `Baseline(habitat, season, noise)` — plus a sparse, self-healing per-Region
//! **Disturbance** that decays exponentially back to zero. No cold tick: a
//! Region's current state is *evaluated*, in closed form, whenever queried.
//!
//! Pure and deterministic: hashing replaces stored state, time is an explicit
//! sim-clock `t_ms`, and there is no wall-clock or global RNG.

use crate::geometry::{floor_div, CHUNK_SIZE};
use crate::motivation::{Drives, NpcKind};

/// Worley cell edge, in sub-units. ~8 chunks, so a Region is a few-chunk
/// territory and a warm session stays mostly within one.
const REGION_CELL: i64 = 8 * CHUNK_SIZE;

/// Disturbance recovery time-constants (seconds): grass heals fast, predators slow.
const GRASS_TAU_S: f64 = 120.0;
const DEER_TAU_S: f64 = 600.0;
const WOLF_TAU_S: f64 = 1_200.0;

/// A deterministic Region: the Worley cell (its feature-point grid coords).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId {
    pub gx: i32,
    pub gy: i32,
}

/// The ecological type of a Region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Habitat {
    Meadow,
    Forest,
}

/// The three ecosystem strata tracked per Region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stratum {
    Grass,
    Deer,
    Wolf,
}

/// Wildlife levels (0..1) for a Region at an instant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub grass: f64,
    pub deer: f64,
    pub wolf: f64,
}

impl Levels {
    pub fn get(&self, s: Stratum) -> f64 {
        match s {
            Stratum::Grass => self.grass,
            Stratum::Deer => self.deer,
            Stratum::Wolf => self.wolf,
        }
    }
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Deterministic hash of two signed coords plus a salt.
fn hash3(a: i64, b: i64, salt: u64) -> u64 {
    mix(mix(a as u64 ^ salt)
        .wrapping_add(0x9E3779B97F4A7C15)
        .wrapping_add(mix(b as u64).rotate_left(17)))
}

/// A deterministic unit float in [0,1) for a Region + salt.
fn unit(r: RegionId, salt: u64) -> f64 {
    hash3(r.gx as i64, r.gy as i64, salt) as f64 / (u64::MAX as f64 + 1.0)
}

/// The jittered feature point of Worley cell `(gx, gy)`, in sub-units.
fn feature_point(gx: i64, gy: i64) -> (i64, i64) {
    let hx = (hash3(gx, gy, 0xF1) % REGION_CELL as u64) as i64;
    let hy = (hash3(gx, gy, 0xF2) % REGION_CELL as u64) as i64;
    (gx * REGION_CELL + hx, gy * REGION_CELL + hy)
}

/// The Region owning world position `(x, y)` — the nearest Worley feature point,
/// ties broken by lowest `(gx, gy)` for determinism.
pub fn region(x: i64, y: i64) -> RegionId {
    let cgx = floor_div(x, REGION_CELL);
    let cgy = floor_div(y, REGION_CELL);
    let mut best = (cgx, cgy);
    let mut best_d = i64::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (gx, gy) = (cgx + dx, cgy + dy);
            let (fx, fy) = feature_point(gx, gy);
            let d = (fx - x) * (fx - x) + (fy - y) * (fy - y);
            if d < best_d || (d == best_d && (gx, gy) < best) {
                best_d = d;
                best = (gx, gy);
            }
        }
    }
    RegionId { gx: best.0 as i32, gy: best.1 as i32 }
}

/// A Region's Habitat (deterministic).
pub fn habitat(r: RegionId) -> Habitat {
    if hash3(r.gx as i64, r.gy as i64, 0x4AB) % 2 == 0 {
        Habitat::Meadow
    } else {
        Habitat::Forest
    }
}

/// Per-Habitat baseline levels (the "natural" wildlife absent players).
fn habitat_base(h: Habitat) -> Levels {
    match h {
        Habitat::Meadow => Levels { grass: 0.90, deer: 0.60, wolf: 0.15 },
        Habitat::Forest => Levels { grass: 0.40, deer: 0.30, wolf: 0.35 },
    }
}

/// Slow seasonal cycle. Constant 1.0 in v1 (hook for later).
fn season(_t_ms: u64) -> f64 {
    1.0
}

/// The deterministic Baseline levels of a Region at time `t_ms`.
pub fn baseline(r: RegionId, t_ms: u64) -> Levels {
    let b = habitat_base(habitat(r));
    let s = season(t_ms);
    let noise = |salt: u64| (unit(r, salt) - 0.5) * 0.2; // ±0.1
    Levels {
        grass: (b.grass * s + noise(0xA1)).clamp(0.0, 1.0),
        deer: (b.deer * s + noise(0xA2)).clamp(0.0, 1.0),
        wolf: (b.wolf * s + noise(0xA3)).clamp(0.0, 1.0),
    }
}

/// A sparse, self-healing per-Region delta from Baseline (ADR-0006). All strata
/// decay toward zero from `t0_ms`; writing a new delta first relaxes to `now`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Disturbance {
    pub grass: f64,
    pub deer: f64,
    pub wolf: f64,
    pub t0_ms: u64,
}

impl Disturbance {
    /// The delta values relaxed forward to `now` (closed form).
    pub fn relaxed(&self, now: u64) -> Levels {
        let dt = now.saturating_sub(self.t0_ms) as f64 / 1000.0;
        let decay = |d: f64, tau: f64| d * (-dt / tau).exp();
        Levels {
            grass: decay(self.grass, GRASS_TAU_S),
            deer: decay(self.deer, DEER_TAU_S),
            wolf: decay(self.wolf, WOLF_TAU_S),
        }
    }

    /// Add `amount` to one stratum's delta (negative = depletion), after first
    /// relaxing any existing delta to `now` and resetting the clock.
    pub fn disturb(&mut self, s: Stratum, amount: f64, now: u64) {
        let r = self.relaxed(now);
        self.grass = r.grass;
        self.deer = r.deer;
        self.wolf = r.wolf;
        self.t0_ms = now;
        match s {
            Stratum::Grass => self.grass += amount,
            Stratum::Deer => self.deer += amount,
            Stratum::Wolf => self.wolf += amount,
        }
    }

    /// True once every stratum's relaxed delta is below `eps` — the entry can be
    /// dropped from the sparse set (the Region has healed).
    pub fn is_settled(&self, now: u64, eps: f64) -> bool {
        let r = self.relaxed(now);
        r.grass.abs() < eps && r.deer.abs() < eps && r.wolf.abs() < eps
    }
}

/// The live level of one stratum: Baseline plus the decaying Disturbance, clamped.
pub fn level(r: RegionId, s: Stratum, t_ms: u64, dist: &Disturbance) -> f64 {
    (baseline(r, t_ms).get(s) + dist.relaxed(t_ms).get(s)).clamp(0.0, 1.0)
}

/// All live levels of a Region (Baseline + Disturbance, clamped).
pub fn levels(r: RegionId, t_ms: u64, dist: &Disturbance) -> Levels {
    Levels {
        grass: level(r, Stratum::Grass, t_ms, dist),
        deer: level(r, Stratum::Deer, t_ms, dist),
        wolf: level(r, Stratum::Wolf, t_ms, dist),
    }
}

/// Seeded spawn count for a level: the integer part of `level·capacity`, plus a
/// seeded chance for the fractional remainder. Deterministic in `seed`.
pub fn spawn_count(level: f64, capacity: u32, seed: u64) -> u32 {
    let expected = level.clamp(0.0, 1.0) * capacity as f64;
    let base = expected.floor();
    let frac = expected - base;
    let roll = mix(seed) as f64 / (u64::MAX as f64 + 1.0);
    base as u32 + if roll < frac { 1 } else { 0 }
}

/// **Spawn-derived temperament** (the keystone coupling): a materializing NPC's
/// initial Drives are a deterministic function of its Region's wildlife levels.
/// A depleted Region (scarce prey/grass) spawns hungry, high-pressure animals.
pub fn initial_drives(kind: NpcKind, levels: &Levels) -> Drives {
    match kind {
        NpcKind::Wolf => {
            let scarcity = (1.0 - levels.deer).clamp(0.0, 1.0); // few deer → hungry
            Drives {
                hunger: (0.3 + 0.6 * scarcity).clamp(0.0, 1.0),
                hunger_pressure: (0.8 * scarcity).clamp(0.0, 1.0),
                safety_pressure: 0.0,
            }
        }
        NpcKind::Deer => {
            let scarcity = (1.0 - levels.grass).clamp(0.0, 1.0);
            Drives {
                hunger: (0.2 + 0.6 * scarcity).clamp(0.0, 1.0),
                hunger_pressure: (0.6 * scarcity).clamp(0.0, 1.0),
                safety_pressure: (0.5 * levels.wolf).clamp(0.0, 1.0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_is_deterministic_and_partitions() {
        // Same point → same Region, every time.
        assert_eq!(region(1234, -5678), region(1234, -5678));
        // Distant points fall in different Regions.
        assert_ne!(region(0, 0), region(20 * REGION_CELL, 20 * REGION_CELL));
    }

    #[test]
    fn habitats_cover_both_types() {
        let mut meadow = false;
        let mut forest = false;
        for g in 0..40 {
            match habitat(RegionId { gx: g, gy: 0 }) {
                Habitat::Meadow => meadow = true,
                Habitat::Forest => forest = true,
            }
        }
        assert!(meadow && forest, "both habitats should occur across regions");
    }

    #[test]
    fn meadow_baselines_richer_in_grass_and_deer() {
        let m = habitat_base(Habitat::Meadow);
        let f = habitat_base(Habitat::Forest);
        assert!(m.grass > f.grass && m.deer > f.deer);
        assert!(f.wolf > m.wolf); // forests favour predators
    }

    #[test]
    fn baseline_levels_in_unit_range() {
        for g in -5..5 {
            let b = baseline(RegionId { gx: g, gy: g }, 0);
            for v in [b.grass, b.deer, b.wolf] {
                assert!((0.0..=1.0).contains(&v));
            }
        }
    }

    #[test]
    fn overhunting_lowers_level_then_it_heals() {
        let r = region(40_000, 40_000);
        let base = level(r, Stratum::Deer, 0, &Disturbance::default());
        let mut dist = Disturbance::default();
        dist.disturb(Stratum::Deer, -0.3, 1_000);
        let depleted = level(r, Stratum::Deer, 1_000, &dist);
        assert!(depleted < base, "overhunting should drop the level");
        // After many time-constants it heals back toward baseline.
        let healed = level(r, Stratum::Deer, 1_000 + 4_000_000, &dist);
        assert!((healed - base).abs() < 0.02, "should heal to baseline");
    }

    #[test]
    fn disturbance_settles_and_can_be_dropped() {
        let mut dist = Disturbance::default();
        dist.disturb(Stratum::Grass, -0.5, 0);
        assert!(!dist.is_settled(0, 0.01));
        assert!(dist.is_settled(10_000_000, 0.01));
    }

    #[test]
    fn spawn_count_tracks_level() {
        assert_eq!(spawn_count(0.0, 10, 1), 0);
        assert_eq!(spawn_count(1.0, 10, 1), 10);
        // Fractional: deterministic, and within [floor, ceil].
        let n = spawn_count(0.55, 10, 42);
        assert!(n == 5 || n == 6);
        assert_eq!(spawn_count(0.55, 10, 42), spawn_count(0.55, 10, 42));
    }

    #[test]
    fn depleted_region_spawns_hungrier_wolves() {
        let healthy = Levels { grass: 0.9, deer: 0.9, wolf: 0.1 };
        let depleted = Levels { grass: 0.1, deer: 0.1, wolf: 0.5 };
        let calm = initial_drives(NpcKind::Wolf, &healthy);
        let desperate = initial_drives(NpcKind::Wolf, &depleted);
        assert!(desperate.hunger > calm.hunger);
        assert!(desperate.hunger_pressure > calm.hunger_pressure);
    }

    #[test]
    fn deer_in_wolfy_region_spawn_wary() {
        let safe = Levels { grass: 0.9, deer: 0.6, wolf: 0.0 };
        let dangerous = Levels { grass: 0.9, deer: 0.6, wolf: 0.8 };
        assert!(
            initial_drives(NpcKind::Deer, &dangerous).safety_pressure
                > initial_drives(NpcKind::Deer, &safe).safety_pressure
        );
    }
}
