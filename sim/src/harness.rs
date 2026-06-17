//! Test harness for the island model: a deterministic PRNG, a scripted-intent
//! driver, and assertion helpers. Public (not `cfg(test)`) so integration tests
//! in `tests/` can use it. The [`Sim`](crate::sim::Sim) clock is already
//! explicit and deterministic, so the harness adds only randomized *input*
//! generation and topology/world assertions.

use crate::consts::INTERACT_RANGE_SQ;
use crate::geometry::coord_for;
use crate::sim::Sim;
use crate::ids::Realm;

/// SplitMix64 — a tiny, fully-deterministic PRNG. No external dependency, so
/// randomized property tests are reproducible from a seed forever.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform integer in `[0, n)`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// A unit-ish intent: one of 9 directions (8 compass + rest), normalized.
    pub fn intent(&mut self) -> (f64, f64) {
        const DIRS: [(f64, f64); 9] = [
            (0.0, 0.0),
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (0.707, 0.707),
            (-0.707, 0.707),
            (0.707, -0.707),
            (-0.707, -0.707),
        ];
        DIRS[self.below(DIRS.len() as u64) as usize]
    }
}

/// Assert the never-under-merge invariant for one realm of a [`Sim`]: any two
/// players whose body centers are within `interaction_range` share an Island.
/// The stronger structural guarantee (Chebyshev-1 chunk neighbours co-island)
/// is also checked, since that is what makes the range version a theorem.
pub fn assert_invariant(sim: &Sim, realm: Realm) {
    let usernames = sim.players_in(realm);
    let rw = match sim.realm_world(realm) {
        Some(rw) => rw,
        None => return,
    };

    for i in 0..usernames.len() {
        for j in (i + 1)..usernames.len() {
            let (ua, ub) = (&usernames[i], &usernames[j]);
            let (pa, pb) = match (rw.position_of(ua), rw.position_of(ub)) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };
            let ca = rw.island_of_username(ua);
            let cb = rw.island_of_username(ub);

            // Range version.
            let dx = pa.x - pb.x;
            let dy = pa.y - pb.y;
            if dx * dx + dy * dy <= INTERACT_RANGE_SQ {
                assert_eq!(
                    ca, cb,
                    "players {ua} and {ub} within interaction range must share an Island"
                );
            }

            // Structural version: chunks within Chebyshev 1 ⇒ same island.
            let cca = coord_for(pa.x, pa.y);
            let ccb = coord_for(pb.x, pb.y);
            if cca.chebyshev(ccb) <= 1 {
                assert_eq!(
                    ca, cb,
                    "players {ua}@{cca:?} and {ub}@{ccb:?} are chunk-neighbours and must co-island"
                );
            }
        }
    }
}
