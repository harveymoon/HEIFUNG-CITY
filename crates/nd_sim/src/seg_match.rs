//! Following street pieces through a road rebuild: which old piece a new
//! one continues, for the figures kept per segment index (`road_wear`,
//! `road_traffic`). A new piece takes an old one when its midpoint lies on
//! that old piece (within `ON_PIECE`, running the same way), so both halves
//! of a street cut in two, and a piece joined back up, keep what the old
//! piece had however long it is. Figures from saves made before the ends
//! were kept only have midpoints: those match midpoint to midpoint within
//! `MID_REACH`. Deterministic: ties go to the lowest old index.

use crate::geom::project_on_segment;
use glam::Vec2;
use std::collections::HashMap;

/// A new piece's midpoint this close to an old piece lies on it.
pub const ON_PIECE: f32 = 1.0;
/// Midpoint-only matching (old saves): this close.
pub const MID_REACH: f32 = 4.5;
/// The two pieces must run within about 18 degrees of each other.
const SAME_WAY: f32 = 0.95;
const CELL: f32 = 16.0;

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

/// An old piece: its midpoint and, when known, its two ends.
#[derive(Clone, Copy, Debug)]
pub struct OldPiece {
    pub mid: Vec2,
    pub ends: Option<(Vec2, Vec2)>,
}

/// The old pieces, gridded for lookup.
pub struct Matcher {
    pieces: Vec<(usize, OldPiece)>,
    grid: HashMap<(i32, i32), Vec<usize>>,
}

impl Matcher {
    /// `pieces`: `(old index, piece)` for every old piece worth carrying.
    pub fn new(pieces: impl IntoIterator<Item = (usize, OldPiece)>) -> Self {
        let pieces: Vec<(usize, OldPiece)> = pieces.into_iter().collect();
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (slot, (_, p)) in pieces.iter().enumerate() {
            let mut cells: Vec<(i32, i32)> = Vec::new();
            // Every cell within one of a point along the piece (points half
            // a cell apart): any point within `ON_PIECE` of it is in one.
            let (a, b) = p.ends.unwrap_or((p.mid, p.mid));
            let steps = ((a.distance(b) / (CELL * 0.5)).ceil() as usize).max(1);
            for k in 0..=steps {
                let (cx, cy) = cell(a.lerp(b, k as f32 / steps as f32));
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        cells.push((cx + dx, cy + dy));
                    }
                }
            }
            cells.sort_unstable();
            cells.dedup();
            for c in cells {
                grid.entry(c).or_default().push(slot);
            }
        }
        Self { pieces, grid }
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// The old index the new piece `a`–`b` continues, if any.
    pub fn find(&self, a: Vec2, b: Vec2) -> Option<usize> {
        let m = (a + b) * 0.5;
        let dir = (b - a).normalize_or_zero();
        let mut best: Option<(f32, usize)> = None;
        for &slot in self.grid.get(&cell(m)).map(|v| v.as_slice()).unwrap_or(&[]) {
            let (k, p) = self.pieces[slot];
            let d = match p.ends {
                Some((pa, pb)) => {
                    let od = (pb - pa).normalize_or_zero();
                    if dir != Vec2::ZERO && od != Vec2::ZERO && dir.dot(od).abs() < SAME_WAY {
                        continue;
                    }
                    let d = project_on_segment(m, pa, pb).1.distance(m);
                    if d > ON_PIECE {
                        continue;
                    }
                    d
                }
                None => {
                    let d = p.mid.distance(m);
                    if d > MID_REACH {
                        continue;
                    }
                    d
                }
            };
            if best.is_none_or(|(bd, bk)| d < bd || (d == bd && k < bk)) {
                best = Some((d, k));
            }
        }
        best.map(|b| b.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_halves_of_a_long_piece_find_it() {
        let old = Matcher::new([(3, OldPiece { mid: Vec2::new(50.0, 0.0), ends: Some((Vec2::ZERO, Vec2::new(100.0, 0.0))) })]);
        assert_eq!(old.find(Vec2::ZERO, Vec2::new(40.0, 0.0)), Some(3));
        assert_eq!(old.find(Vec2::new(40.0, 0.0), Vec2::new(100.0, 0.0)), Some(3));
        // A street crossing it is not it.
        assert_eq!(old.find(Vec2::new(50.0, -30.0), Vec2::new(50.0, 30.0)), None);
        // Nor is one 10 m off.
        assert_eq!(old.find(Vec2::new(0.0, 10.0), Vec2::new(40.0, 10.0)), None);
    }

    #[test]
    fn midpoints_only_match_close_by() {
        let old = Matcher::new([(0, OldPiece { mid: Vec2::new(50.0, 0.0), ends: None })]);
        assert_eq!(old.find(Vec2::new(46.0, 0.0), Vec2::new(56.0, 0.0)), Some(0));
        assert_eq!(old.find(Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0)), None);
    }
}
