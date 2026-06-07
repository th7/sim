//! How an NPC presents to an observer (see `design/glossary.md`): banded
//! Health derived purely from wire facts — `(hp, kind)` — via the shared
//! `simcore` catalogue, and a fully diegetic pose. The two display axes are
//! orthogonal by construction: Demeanor owns body pitch, head height, and the
//! gait bob; the Health band owns body sag. Facing orients the pose along the
//! last nonzero movement direction.

use protocol::types::{Demeanor, NpcKind};

/// The three bands an observer reads an NPC's Health in. Never an exact
/// number — see the glossary's Health entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthBand {
    Unhurt,
    Wounded,
    Critical,
}

impl HealthBand {
    /// Every band — what the showcase enumerates to display them all. The
    /// guard match below breaks this const's compile when a variant is added,
    /// so the list cannot silently fall behind the enum.
    pub const ALL: [Self; 3] = {
        let all = [HealthBand::Unhurt, HealthBand::Wounded, HealthBand::Critical];
        match all[0] {
            HealthBand::Unhurt | HealthBand::Wounded | HealthBand::Critical => {}
        }
        all
    };
}

/// Band `hp` against the kind's catalogue max. Integer math, no float
/// thresholds: above two-thirds reads Unhurt, above one-third Wounded, the
/// rest Critical. The cut points are tuning; the partition is the contract.
pub fn health_band(hp: i64, kind: NpcKind) -> HealthBand {
    let max = simcore::catalogue::npc_max_hp(kind);
    if hp * 3 > max * 2 {
        HealthBand::Unhurt
    } else if hp * 3 > max {
        HealthBand::Wounded
    } else {
        HealthBand::Critical
    }
}

/// The body-language parameters one (Demeanor, band) pair poses: radians of
/// forward body pitch, head height offset (world units, relative to the calm
/// head), vertical body scale, and gait-bob amplitude (world units; the
/// renderer animates the phase, the pose only grants the amplitude).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub pitch: f32,
    pub head_dy: f32,
    pub sag: f32,
    pub bob_amp: f32,
}

/// Pose an NPC from its two observer-facing axes. Demeanor decides pitch,
/// head height, and bob (the distance-legibility concession on the two urgent
/// Demeanors); the band decides sag alone. All values are tuning — the
/// orthogonality and pairwise distinctness are the contract (pinned below).
pub fn npc_pose(demeanor: Demeanor, band: HealthBand) -> Pose {
    let (pitch, head_dy, bob_amp) = match demeanor {
        Demeanor::Calm => (0.0, 0.0, 0.0),
        Demeanor::Feeding => (0.0, -0.35, 0.0), // head down at the food
        Demeanor::Aggressive => (0.35, -0.10, 0.06), // pitched at its target
        Demeanor::Fleeing => (-0.15, 0.20, 0.06), // head up, craning away
    };
    let sag = match band {
        HealthBand::Unhurt => 1.0,
        HealthBand::Wounded => 0.78,
        HealthBand::Critical => 0.55,
    };
    Pose { pitch, head_dy, sag, bob_amp }
}

/// An NPC's facing: the last nonzero movement direction, persisted while
/// stationary so a stopped fight-to-hold wolf keeps pointing at its rival.
/// Angle is radians counter-clockwise from +x, in the world's x/y plane.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facing {
    angle: f32,
}

impl Facing {
    /// Re-aim along `(vx, vy)` if moving; keep the current facing if still.
    pub fn update(&mut self, vx: f64, vy: f64) {
        if vx != 0.0 || vy != 0.0 {
            self.angle = (vy as f32).atan2(vx as f32);
        }
    }

    pub fn angle(&self) -> f32 {
        self.angle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full health reads Unhurt, half reads Wounded, near-death reads
    /// Critical — for every kind, against the shared catalogue's max. The
    /// exact thresholds are tuning; the partition order is the behavior.
    #[test]
    fn health_bands_partition_full_half_and_near_death() {
        for k in NpcKind::ALL {
            let max = simcore::catalogue::npc_max_hp(k);
            assert_eq!(health_band(max, k), HealthBand::Unhurt, "{k:?} full");
            assert_eq!(health_band(max / 2, k), HealthBand::Wounded, "{k:?} half");
            assert_eq!(health_band(1, k), HealthBand::Critical, "{k:?} near-death");
        }
    }

    /// The two axes are orthogonal by construction: Demeanor owns pitch, head
    /// height, and bob; the Health band owns sag. Across all 12 combinations
    /// neither axis disturbs the other's parameters.
    #[test]
    fn demeanor_and_band_pose_axes_are_orthogonal() {
        for d in Demeanor::ALL {
            let reference = npc_pose(d, HealthBand::Unhurt);
            for b in HealthBand::ALL {
                let p = npc_pose(d, b);
                assert_eq!(
                    (p.pitch, p.head_dy, p.bob_amp),
                    (reference.pitch, reference.head_dy, reference.bob_amp),
                    "{d:?}/{b:?}: band must not disturb the Demeanor axis"
                );
            }
        }
        for b in HealthBand::ALL {
            let reference = npc_pose(Demeanor::Calm, b);
            for d in Demeanor::ALL {
                let p = npc_pose(d, b);
                assert_eq!(
                    p.sag, reference.sag,
                    "{d:?}/{b:?}: Demeanor must not disturb the band axis"
                );
            }
        }
    }

    /// Every Demeanor poses distinctly and every band sags distinctly — no two
    /// states are visually identical by construction.
    #[test]
    fn every_demeanor_and_band_is_visually_distinct() {
        for (i, a) in Demeanor::ALL.into_iter().enumerate() {
            for b in Demeanor::ALL.into_iter().skip(i + 1) {
                let (pa, pb) = (npc_pose(a, HealthBand::Unhurt), npc_pose(b, HealthBand::Unhurt));
                assert_ne!(
                    (pa.pitch, pa.head_dy, pa.bob_amp),
                    (pb.pitch, pb.head_dy, pb.bob_amp),
                    "{a:?} vs {b:?}"
                );
            }
        }
        for (i, a) in HealthBand::ALL.into_iter().enumerate() {
            for b in HealthBand::ALL.into_iter().skip(i + 1) {
                let (pa, pb) = (npc_pose(Demeanor::Calm, a), npc_pose(Demeanor::Calm, b));
                assert_ne!(pa.sag, pb.sag, "{a:?} vs {b:?}");
            }
        }
    }

    /// The gait bob is the distance-legibility concession: only the two
    /// urgent Demeanors carry it.
    #[test]
    fn only_aggressive_and_fleeing_bob() {
        for d in Demeanor::ALL {
            let bob = npc_pose(d, HealthBand::Unhurt).bob_amp;
            match d {
                Demeanor::Aggressive | Demeanor::Fleeing => {
                    assert!(bob > 0.0, "{d:?} should bob")
                }
                Demeanor::Calm | Demeanor::Feeding => assert_eq!(bob, 0.0, "{d:?} is steady"),
            }
        }
    }

    /// Facing is the last nonzero movement direction, persisted while
    /// stationary — a stopped fight-to-hold wolf keeps pointing at its rival.
    #[test]
    fn facing_persists_last_nonzero_movement() {
        let mut f = Facing::default();
        f.update(1_000.0, 0.0); // east
        let east = f.angle();
        f.update(0.0, 0.0); // stops
        assert_eq!(f.angle(), east, "stillness keeps the last facing");
        f.update(0.0, 1_000.0); // turns
        assert_ne!(f.angle(), east, "movement re-aims the facing");
    }
}
