//! [r4-tiles] Land past the starting map, bought tile by tile.
//!
//! The world is a grid of square tiles of [`TILE`] metres, tile `(i, j)`
//! centred on `(i, j) * TILE`. The starting map (the generated square of
//! half side [`LAND_HALF`]) is the nine tiles round the centre; the ring
//! of sixteen round it can be bought, each one next to land already
//! owned. Bought tiles are the only thing saved: their ground is the far
//! land the horizon showed there ([`far`](super::far)), worked out from the
//! seed and the map's features, so a bought tile is exactly what was seen.
//!
//! Why tiles a third of the map's side, and only one ring of them: a tile
//! of 1.2 km is a district's worth of land (the starting basin is 2 km
//! across), small enough that buying one is a step and not a second map.
//! The ring takes the land out to 3 km from the centre, 2.8 times the
//! starting area; past that the camera would come within sight of where
//! traffic from elsewhere appears ([`WAY_REACH`], 5 km out) and of the far
//! islands, which a later step (regions) handles. At 3 km an `f32` is
//! good to a quarter of a millimetre, so no floating origin is needed.

use super::*;

/// The side of a tile, in metres.
pub const TILE: f32 = 1200.0;
/// Tiles up to this ring (Chebyshev distance in tiles from the centre) are
/// the starting map.
pub const START_RING: i32 = 1;
/// The outermost ring that can be bought.
pub const MAX_RING: i32 = 2;
/// Half the side of the land square once any tile is bought.
pub const GROWN_HALF: f32 = LAND_HALF + TILE * (MAX_RING - START_RING) as f32;

/// A tile by column and row.
pub type Tile = (i32, i32);

/// The tile holding `p`.
pub fn tile_of(p: Vec2) -> Tile {
    ((p.x / TILE).round() as i32, (p.y / TILE).round() as i32)
}

/// Which ring a tile is in: 0 the centre, 1 the rest of the starting map.
pub fn tile_ring(t: Tile) -> i32 {
    t.0.abs().max(t.1.abs())
}

/// A tile's corners: (lowest x and y, highest).
pub fn tile_rect(t: Tile) -> (Vec2, Vec2) {
    let c = Vec2::new(t.0 as f32, t.1 as f32) * TILE;
    (c - Vec2::splat(TILE * 0.5), c + Vec2::splat(TILE * 0.5))
}

/// The tile's centre.
pub fn tile_center(t: Tile) -> Vec2 {
    Vec2::new(t.0 as f32, t.1 as f32) * TILE
}

/// Every tile that can ever be bought, in a fixed order (row by row).
pub fn outer_tiles() -> Vec<Tile> {
    let mut out = Vec::new();
    for j in -MAX_RING..=MAX_RING {
        for i in -MAX_RING..=MAX_RING {
            if tile_ring((i, j)) > START_RING {
                out.push((i, j));
            }
        }
    }
    out
}

/// The four tiles sharing an edge with `t`.
pub fn tile_neighbours(t: Tile) -> [Tile; 4] {
    [(t.0 + 1, t.1), (t.0 - 1, t.1), (t.0, t.1 + 1), (t.0, t.1 - 1)]
}

/// The compass name of a tile's side of the map: "north", "north-east",
/// or for a tile beside a corner "north (east end)".
pub fn tile_direction(t: Tile) -> &'static str {
    let (i, j) = t;
    let name = |x: i32, y: i32| match (x.signum(), y.signum()) {
        (0, 1) => "north",
        (0, -1) => "south",
        (1, 0) => "east",
        (-1, 0) => "west",
        (1, 1) => "north-east",
        (-1, 1) => "north-west",
        (1, -1) => "south-east",
        (-1, -1) => "south-west",
        _ => "centre",
    };
    if i.abs() == j.abs() || i == 0 || j == 0 {
        return name(i, j);
    }
    match (i.abs() > j.abs(), i.signum(), j.signum()) {
        (true, 1, 1) => "east (north end)",
        (true, 1, -1) => "east (south end)",
        (true, -1, 1) => "west (north end)",
        (true, -1, -1) => "west (south end)",
        (false, 1, 1) => "north (east end)",
        (false, -1, 1) => "north (west end)",
        (false, 1, -1) => "south (east end)",
        (false, -1, -1) => "south (west end)",
        _ => name(i, j),
    }
}

impl Landscape {
    /// Half the side of the square the land is generated over: the
    /// starting map's, or with land bought the whole ring's.
    pub fn land_half(&self) -> f32 {
        if self.tiles.is_empty() || self.is_flat() { LAND_HALF } else { GROWN_HALF }
    }

    /// Has any land past the starting map been bought?
    pub fn is_grown(&self) -> bool {
        !self.tiles.is_empty() && !self.is_flat()
    }

    pub fn owns_tile(&self, t: Tile) -> bool {
        tile_ring(t) <= START_RING || self.tiles.contains(&t)
    }

    /// Is `p` on the city's land (the starting map or a bought tile)?
    pub fn owns(&self, p: Vec2) -> bool {
        let r = chebyshev(p);
        r <= LAND_HALF || (r <= self.land_half() && self.tiles.contains(&tile_of(p)))
    }

    /// The bounds of the city's land: lowest corner, highest.
    pub fn owned_bounds(&self) -> (Vec2, Vec2) {
        let mut lo = Vec2::splat(-LAND_HALF);
        let mut hi = Vec2::splat(LAND_HALF);
        for &t in &self.tiles {
            let (a, b) = tile_rect(t);
            lo = lo.min(a);
            hi = hi.max(b);
        }
        (lo, hi)
    }

    /// How far `p` is inside the city's land from its edge (negative
    /// outside it), measured along the axes to the nearest edge of land not
    /// owned, out to `cap`.
    pub fn owned_depth(&self, p: Vec2, cap: f32) -> f32 {
        if !self.owns(p) {
            return -1.0;
        }
        let mut d = cap;
        for (x, sign, axis) in [(p.x, 1.0, 0), (p.x, -1.0, 0), (p.y, 1.0, 1), (p.y, -1.0, 1)] {
            // Every edge of land runs along a tile edge, (k + 0.5) * TILE.
            let mut k = if sign > 0.0 { (x / TILE - 0.5).floor() + 1.0 } else { (x / TILE + 0.5).ceil() - 1.0 };
            loop {
                let edge = (k + 0.5 * sign) * TILE;
                let gap = (edge - x) * sign;
                if gap >= d {
                    break;
                }
                let mut past = p;
                if axis == 0 {
                    past.x = edge + 0.5 * sign;
                } else {
                    past.y = edge + 0.5 * sign;
                }
                if !self.owns(past) {
                    d = gap;
                    break;
                }
                k += sign;
            }
        }
        d
    }

    /// Is `p` more than `out` metres past the edge of the map ([`MAP_EDGE`],
    /// 40 m inside the land's edge; `out` may be negative)? With land
    /// bought, the edge is the city's land's: the seam between the
    /// starting map and a bought tile is no edge, a side not bought is.
    pub fn beyond_map_edge(&self, p: Vec2, out: f32) -> bool {
        if !self.is_grown() {
            return chebyshev(p) > MAP_EDGE + out;
        }
        let inset = LAND_HALF - MAP_EDGE - out;
        if inset > 0.0 {
            return self.owned_depth(p, inset) < inset;
        }
        // Out past the land: how far past its bounds.
        let (lo, hi) = self.owned_bounds();
        let past = (lo.x - p.x).max(p.x - hi.x).max(lo.y - p.y).max(p.y - hi.y);
        !self.owns(p) && past > -inset
    }

    /// [`reach_elsewhere`](super::reach_elsewhere) on this land: with land
    /// bought, an end inside a bought tile is an ordinary end, and one at
    /// the edge of a bought tile, heading out over it, runs on out to
    /// elsewhere. Ends on the starting map and past the land are as before.
    pub fn reach_elsewhere(&self, points: &[Vec2]) -> Vec<Vec2> {
        if !self.is_grown() || points.len() < 2 {
            return super::reach_elsewhere(points);
        }
        let mut out = points.to_vec();
        let extend = |end: Vec2, prev: Vec2| -> Option<Vec2> {
            let r = chebyshev(end);
            if r < MAP_EDGE - 1.0 || r >= WAY_REACH - 1.0 {
                return None;
            }
            let d = (end - prev).normalize_or_zero();
            let bought = r > LAND_HALF && self.owns(end);
            let outward = if bought {
                // Out over the nearest edge of land not owned, if one is near.
                let edge = LAND_HALF - MAP_EDGE + 1.0;
                [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y].iter().filter(|&&dir| !self.owns(end + dir * edge)).map(|&dir| d.dot(dir)).fold(f32::NEG_INFINITY, f32::max)
            } else if end.x.abs() >= end.y.abs() {
                d.x * end.x.signum()
            } else {
                d.y * end.y.signum()
            };
            if outward < 0.3 {
                return None;
            }
            let mut t = f32::INFINITY;
            for (pc, dc) in [(end.x, d.x), (end.y, d.y)] {
                if dc.abs() > 1e-4 {
                    let k = (WAY_REACH * dc.signum() - pc) / dc;
                    if k > 0.0 {
                        t = t.min(k);
                    }
                }
            }
            t.is_finite().then(|| end + d * t)
        };
        if let Some(p) = extend(out[out.len() - 1], out[out.len() - 2]) {
            out.push(p);
        }
        if let Some(p) = extend(out[0], out[1]) {
            out.insert(0, p);
        }
        out
    }

    /// Tiles not yet owned that can be bought now: in the outer ring, each
    /// sharing an edge with land already owned. None on the flat map (it
    /// has no landscape to carry on).
    pub fn buyable_tiles(&self) -> Vec<Tile> {
        if self.is_flat() {
            return Vec::new();
        }
        outer_tiles().into_iter().filter(|&t| !self.owns_tile(t) && tile_neighbours(t).iter().any(|&n| tile_ring(n) <= MAX_RING && self.owns_tile(n))).collect()
    }

    /// This landscape with `t` bought: the same features and seed, fresh
    /// caches (the raster grows to the ring).
    pub fn with_tiles(&self, tiles: &[Tile]) -> Landscape {
        let mut all: Vec<Tile> = self.tiles.iter().chain(tiles.iter()).copied().filter(|&t| tile_ring(t) > START_RING && tile_ring(t) <= MAX_RING).collect();
        all.sort_unstable();
        all.dedup();
        Landscape { params: self.params.clone(), rivers: self.rivers.clone(), lakes: self.lakes.clone(), sea: self.sea.clone(), hot_spots: self.hot_spots.clone(), tiles: all, ..Landscape::default() }
    }

    /// The far land's carried-on coast and rivers, worked out once.
    pub fn far_core(&self) -> &far::FarCore {
        self.far.get_or_init(|| far::FarCore::new(self))
    }

    /// The sea as the water queries read it: with land bought, the coast
    /// carried on across it as the horizon showed it.
    pub fn sea_now(&self) -> Option<&Sea> {
        self.grown_sea.get_or_init(|| self.grow_sea()).as_ref().or(self.sea.as_ref())
    }

    fn grow_sea(&self) -> Option<Sea> {
        if !self.is_grown() {
            return None;
        }
        let sea = self.sea.as_ref()?;
        if sea.outward != SeaExit::South || sea.coast.len() < 2 {
            return None;
        }
        let reach = self.land_half() + 600.0;
        let (first, last) = (sea.coast[0], sea.coast[sea.coast.len() - 1]);
        let far = self.far_core().coast_points(-reach, reach)?;
        // The generated points as they are, the carried-on ones past them.
        let mut coast: Vec<Vec2> = far.iter().copied().filter(|p| p.x < first.x - 1.0).collect();
        coast.extend(sea.coast.iter().copied());
        coast.extend(far.iter().copied().filter(|p| p.x > last.x + 1.0));
        let (x0, x1) = (coast[0].x, coast[coast.len() - 1].x);
        let bottom = sea.outline.iter().map(|p| p.y).fold(-LAND_HALF * 3.0, f32::min);
        let mut outline = coast.clone();
        outline.push(Vec2::new(x1, bottom));
        outline.push(Vec2::new(x0, bottom));
        Some(Sea { outline, islands: sea.islands.clone(), coast, outward: sea.outward })
    }

    /// [r4-tiles] The natural ground past the starting map once land is
    /// bought: the map's own ground there (as `height` gave it before)
    /// blended into the far land over [`far::SEAM_BLEND`], exactly the
    /// ground the far landscape drew (its `extra` on the map's height).
    pub fn grown_height(&self, p: Vec2) -> f32 {
        let own = self.carve(self.base_height(p), self.far_water_distance(p));
        let w = far::FarLand::blend(p);
        if w <= 0.0 {
            return own;
        }
        let far = far::FarLand::new(self).natural(p);
        own + (far - own) * w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_map_is_the_nine_middle_tiles() {
        assert_eq!(tile_of(Vec2::new(0.0, 0.0)), (0, 0));
        assert_eq!(tile_of(Vec2::new(1799.0, -1799.0)), (1, -1));
        assert_eq!(tile_of(Vec2::new(1801.0, 0.0)), (2, 0));
        assert_eq!(outer_tiles().len(), 16);
        let l = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 3));
        assert!(l.owns(Vec2::new(1790.0, 1790.0)) && !l.owns(Vec2::new(1810.0, 0.0)));
        let b = l.buyable_tiles();
        assert_eq!(b.len(), 12, "the corners only once a side is bought: {b:?}");
        let g = l.with_tiles(&[(2, 0)]);
        assert!(g.owns(Vec2::new(2500.0, 100.0)) && !g.owns(Vec2::new(2500.0, 700.0)));
        assert!(g.buyable_tiles().contains(&(2, 1)) && g.buyable_tiles().contains(&(2, -1)));
        assert_eq!(g.land_half(), GROWN_HALF);
    }

    #[test]
    fn depth_is_to_the_nearest_edge_of_land_not_owned() {
        let l = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 3)).with_tiles(&[(2, 0)]);
        // In the east tile, 100 m below its north edge (y 600).
        assert!((l.owned_depth(Vec2::new(2500.0, 500.0), 400.0) - 100.0).abs() < 1.0);
        // On the start map by the bought tile: the seam is no edge.
        assert!((l.owned_depth(Vec2::new(1750.0, 0.0), 400.0) - 400.0).abs() < 1.0);
        assert!((l.owned_depth(Vec2::new(1750.0, 1500.0), 400.0) - 50.0).abs() < 1.0);
        assert!(l.owned_depth(Vec2::new(2500.0, 700.0), 400.0) < 0.0);
    }
}
