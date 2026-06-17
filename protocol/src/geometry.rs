//! Chunk geometry — the single source of truth for chunk dimensions, in
//! **sub-units**. Mirrors the Elixir `GameCore.ChunkGeometry` exactly so the
//! island model lands actors in the same chunks the wire contract expects.
//!
//! 1 world unit = [`SUB_UNITS_PER_UNIT`] sub-units. Chunk `(cx, cy)` owns
//! sub-unit positions in `[cx*CHUNK_SIZE, cx*CHUNK_SIZE + CHUNK_SIZE)` on each
//! axis.

/// Sub-units per world unit. Positions are integers in sub-units; the wire
/// boundary converts to world-unit floats by dividing by this.
pub const SUB_UNITS_PER_UNIT: i64 = 1_000;

/// Chunk edge length in world units.
pub const CHUNK_SIZE_UNITS: i64 = 16;

/// Chunk edge length in sub-units (`CHUNK_SIZE_UNITS * SUB_UNITS_PER_UNIT`).
pub const CHUNK_SIZE: i64 = CHUNK_SIZE_UNITS * SUB_UNITS_PER_UNIT;

/// A chunk coordinate, identifying a fixed square of the world. Equivalent to
/// the Elixir `{cx, cy}` tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkCoord {
    pub cx: i32,
    pub cy: i32,
}

impl ChunkCoord {
    pub const fn new(cx: i32, cy: i32) -> Self {
        ChunkCoord { cx, cy }
    }

    /// The 8 chunks bordering this one (the ring of a 3×3 footprint).
    pub fn ring(self) -> [ChunkCoord; 8] {
        [
            ChunkCoord::new(self.cx - 1, self.cy - 1),
            ChunkCoord::new(self.cx, self.cy - 1),
            ChunkCoord::new(self.cx + 1, self.cy - 1),
            ChunkCoord::new(self.cx - 1, self.cy),
            ChunkCoord::new(self.cx + 1, self.cy),
            ChunkCoord::new(self.cx - 1, self.cy + 1),
            ChunkCoord::new(self.cx, self.cy + 1),
            ChunkCoord::new(self.cx + 1, self.cy + 1),
        ]
    }

    /// This chunk plus its 8-neighbour ring — the 3×3 interaction footprint
    /// of an actor standing in this chunk.
    pub fn footprint_3x3(self) -> [ChunkCoord; 9] {
        let mut out = [self; 9];
        let mut i = 1;
        for r in self.ring() {
            out[i] = r;
            i += 1;
        }
        out
    }

    /// Chebyshev (king-move) distance to another chunk.
    pub fn chebyshev(self, other: ChunkCoord) -> i32 {
        (self.cx - other.cx).abs().max((self.cy - other.cy).abs())
    }

    /// True iff the two chunks are equal or 8-adjacent (touch or border).
    pub fn touches(self, other: ChunkCoord) -> bool {
        self.chebyshev(other) <= 1
    }
}

/// Floor division (rounds toward negative infinity), matching Elixir's
/// `Integer.floor_div/2`. Rust's `/` truncates toward zero, which would put
/// negative-coordinate positions in the wrong chunk.
#[inline]
pub fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// The chunk owning sub-unit position `(x, y)`.
pub fn coord_for(x: i64, y: i64) -> ChunkCoord {
    ChunkCoord::new(
        floor_div(x, CHUNK_SIZE) as i32,
        floor_div(y, CHUNK_SIZE) as i32,
    )
}

/// Center of a chunk in sub-units (`cx*size + size/2`).
pub fn chunk_center(coord: ChunkCoord) -> (i64, i64) {
    let half = CHUNK_SIZE / 2;
    (
        coord.cx as i64 * CHUNK_SIZE + half,
        coord.cy as i64 * CHUNK_SIZE + half,
    )
}

/// The `(2*radius+1)²` square of chunk coords centered on `center`, by
/// Chebyshev distance. Mirrors `GameCore.ChunkGeometry.neighborhood/2`.
pub fn neighborhood(center: ChunkCoord, radius: i32) -> Vec<ChunkCoord> {
    let mut out = Vec::with_capacity(((2 * radius + 1) * (2 * radius + 1)) as usize);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            out.push(ChunkCoord::new(center.cx + dx, center.cy + dy));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coord_for_matches_floor_division() {
        assert_eq!(coord_for(0, 0), ChunkCoord::new(0, 0));
        assert_eq!(coord_for(15_999, 15_999), ChunkCoord::new(0, 0));
        assert_eq!(coord_for(16_000, 0), ChunkCoord::new(1, 0));
        // Negative coords floor toward -inf, not toward zero.
        assert_eq!(coord_for(-1, -1), ChunkCoord::new(-1, -1));
        assert_eq!(coord_for(-16_000, 0), ChunkCoord::new(-1, 0));
        assert_eq!(coord_for(-16_001, 0), ChunkCoord::new(-2, 0));
    }

    #[test]
    fn chunk_center_is_half_in() {
        assert_eq!(chunk_center(ChunkCoord::new(0, 0)), (8_000, 8_000));
        assert_eq!(chunk_center(ChunkCoord::new(1, 1)), (24_000, 24_000));
    }

    #[test]
    fn neighborhood_radius_sizes() {
        assert_eq!(neighborhood(ChunkCoord::new(0, 0), 0).len(), 1);
        assert_eq!(neighborhood(ChunkCoord::new(0, 0), 1).len(), 9);
        assert_eq!(neighborhood(ChunkCoord::new(3, 3), 2).len(), 25);
    }

    #[test]
    fn touches_is_chebyshev_one() {
        let c = ChunkCoord::new(0, 0);
        assert!(c.touches(ChunkCoord::new(1, 1)));
        assert!(c.touches(ChunkCoord::new(0, 0)));
        assert!(!c.touches(ChunkCoord::new(2, 0)));
    }
}
