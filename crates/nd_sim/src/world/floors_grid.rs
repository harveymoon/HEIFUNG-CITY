//! [review] Floors standing round a point, for a pass that asks it of many
//! lots in a row: the buildings sorted once into square cells as wide as
//! the radius asked about, so each question looks at the 5x5 cells round
//! it instead of every building. The sum is of whole floors, so it is the
//! same in any order: the answers are exactly the full scan's. Built for
//! one pass over a world that does not change during it.

use super::*;

pub(super) struct FloorsGrid {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<(LotId, Vec2, u32)>>,
}

impl FloorsGrid {
    /// `items`: (the building's lot, its centre, its floors). `cell`: the
    /// largest radius `floors_within` will be asked about.
    pub(super) fn new(cell: f32, items: impl Iterator<Item = (LotId, Vec2, u32)>) -> Self {
        let mut cells: HashMap<(i32, i32), Vec<(LotId, Vec2, u32)>> = HashMap::new();
        for it in items {
            cells.entry(Self::key(cell, it.1)).or_default().push(it);
        }
        Self { cell, cells }
    }

    fn key(cell: f32, p: Vec2) -> (i32, i32) {
        ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32)
    }

    /// Floors of the buildings whose lot centre is within `r` (at most the
    /// cell size) of `center`, leaving out those on lot `except`.
    pub(super) fn floors_within(&self, center: Vec2, r: f32, except: LotId) -> u32 {
        debug_assert!(r <= self.cell);
        let (kx, ky) = Self::key(self.cell, center);
        let mut sum = 0u32;
        // Two cells each way: a point within one cell width can round into
        // the next cell but one at a boundary.
        for dy in -2..=2 {
            for dx in -2..=2 {
                let Some(v) = self.cells.get(&(kx + dx, ky + dy)) else { continue };
                for &(lot, c, f) in v {
                    if lot != except && c.distance(center) <= r {
                        sum += f;
                    }
                }
            }
        }
        sum
    }
}
