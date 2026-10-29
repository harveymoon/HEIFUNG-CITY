//! [r4-tiles] Buying the land round the city, tile by tile
//! (`landscape::tiles`): what each tile holds, what it costs, and the
//! purchase, after which the tile is the city's to build on like the
//! starting map.

use super::*;
use crate::landscape::{outer_tiles, tile_direction, tile_rect, tile_ring, Tile, WaterKind, MAX_GRADED_SLOPE, MAX_TERRACE_SLOPE};

/// What the first tile bought costs before its land is weighed in.
pub const TILE_BASE_PRICE: f32 = 30_000.0;
/// Each tile bought puts this share of the base on every later one.
pub const TILE_PRICE_STEP: f32 = 0.4;
/// Samples along a tile's side for its survey (every 50 m).
const SURVEY_SIDE: usize = 24;

/// The ground of a tile, which never changes (a pure function of the map):
/// shares are of the tile's area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileGround {
    pub tile: Tile,
    /// Dry land.
    pub land: f32,
    pub sea: f32,
    /// Rivers and streams.
    pub river: f32,
    pub lake: f32,
    /// Dry land gentle enough to level a lot on.
    pub level: f32,
    /// Dry land that can be built on at all (levelled or terraced).
    pub buildable: f32,
    /// Lowest, mean and highest ground on the dry land (metres).
    pub low: f32,
    pub mean: f32,
    pub high: f32,
    /// Mean slope of the dry land (rise over run).
    pub slope: f32,
    /// Mean fertility of the dry land, 0..1.
    pub fertility: f32,
    /// Share of the dry land over oil.
    pub oil: f32,
    /// Mean rock of the dry land, 0..1.
    pub rock: f32,
    /// Geothermal hot spots in the tile.
    pub hot_spots: usize,
    /// A river (not a stream) crosses or touches it.
    pub river_crosses: bool,
    pub stream_crosses: bool,
}

/// A tile as the Buy land view and the API show it.
#[derive(Clone, Debug, PartialEq)]
pub struct LandTile {
    pub ground: TileGround,
    pub owned: bool,
    /// Next to the city's land and not yet bought.
    pub buyable: bool,
    /// What it costs now (0 when owned).
    pub price: f32,
    pub highway: bool,
    pub rail: bool,
    /// Streets and paths already drawn on it (ways to elsewhere).
    pub roads: usize,
}

impl LandTile {
    pub fn tile(&self) -> Tile {
        self.ground.tile
    }

    /// The tile in a few plain words: its ground, its water, what runs
    /// through it.
    pub fn summary(&self) -> String {
        let g = &self.ground;
        let pc = |x: f32| (x * 100.0).round();
        let mut parts: Vec<String> = Vec::new();
        if g.land < 0.02 {
            parts.push("open water".into());
        } else {
            let relief = g.high - g.low;
            let lie = if g.slope > 0.25 {
                "steep hills"
            } else if relief > 40.0 || g.slope > 0.12 {
                "hills"
            } else if relief > 12.0 {
                "rolling ground"
            } else {
                "flat ground"
            };
            parts.push(format!("{lie} {:.0} to {:.0} m", g.low, g.high));
            parts.push(format!("{:.0}% dry land, {:.0}% buildable", pc(g.land), pc(g.buildable)));
        }
        if g.sea > 0.02 {
            parts.push(format!("{:.0}% sea", pc(g.sea)));
        }
        if g.river_crosses {
            parts.push("a river crosses".into());
        } else if g.stream_crosses {
            parts.push("a stream crosses".into());
        }
        if g.lake > 0.02 {
            parts.push("a lake".into());
        }
        if g.land >= 0.02 {
            if g.fertility > 0.45 {
                parts.push("fertile soil".into());
            } else if g.fertility < 0.15 {
                parts.push("poor soil".into());
            }
            if g.oil > 0.05 {
                parts.push(format!("oil under {:.0}%", pc(g.oil)));
            }
            if g.rock > 0.45 {
                parts.push("rocky".into());
            }
        }
        if g.hot_spots > 0 {
            parts.push(format!("{} hot spot{}", g.hot_spots, if g.hot_spots == 1 { "" } else { "s" }));
        }
        if self.highway {
            parts.push("the highway runs through".into());
        }
        if self.rail {
            parts.push("the railway runs through".into());
        }
        parts.join(", ")
    }
}

/// How a tile's land weighs on its price: level dry land most, hillsides
/// less, water little; good soil, oil and hot ground add, and so does a
/// waterfront.
pub fn land_factor(g: &TileGround) -> f32 {
    let waterfront = g.land > 0.12 && g.land < 0.95 && (g.sea > 0.02 || g.river_crosses || g.lake > 0.02);
    0.3 + 0.8 * g.level + 0.3 * (g.buildable - g.level).max(0.0) + 0.3 * g.fertility * g.land + 0.5 * g.oil * g.land + 0.1 * (g.hot_spots.min(2) as f32) + if waterfront { 0.2 } else { 0.0 }
}

/// A tile's price with `bought` tiles already bought: the base, a step
/// more for each one bought, times its land, a fifth more with the highway
/// through it and a tenth with the railway; to the nearest 500.
pub fn tile_price(g: &TileGround, bought: usize, highway: bool, rail: bool) -> f32 {
    let growth = 1.0 + TILE_PRICE_STEP * bought as f32;
    let access = 1.0 + if highway { 0.2 } else { 0.0 } + if rail { 0.1 } else { 0.0 };
    ((TILE_BASE_PRICE * growth * land_factor(g) * access / 500.0).round() * 500.0).max(5_000.0)
}

/// Survey one tile's ground on `land` (a landscape with the ring grown).
pub fn survey_tile(land: &Landscape, t: Tile) -> TileGround {
    let (lo, hi) = tile_rect(t);
    let step = (hi.x - lo.x) / SURVEY_SIDE as f32;
    let n = (SURVEY_SIDE * SURVEY_SIDE) as f32;
    let mut g = TileGround { tile: t, low: f32::MAX, high: f32::MIN, ..Default::default() };
    let (mut dry, mut level, mut buildable, mut hsum, mut ssum, mut fert, mut oil, mut rock) = (0usize, 0usize, 0usize, 0.0f64, 0.0f64, 0.0f64, 0usize, 0.0f64);
    for j in 0..SURVEY_SIDE {
        for i in 0..SURVEY_SIDE {
            let p = lo + Vec2::new((i as f32 + 0.5) * step, (j as f32 + 0.5) * step);
            match land.water_at(p).map(|w| w.kind) {
                Some(WaterKind::Sea) => g.sea += 1.0,
                Some(WaterKind::Lake) => g.lake += 1.0,
                Some(_) => g.river += 1.0,
                None => {
                    dry += 1;
                    let h = land.height(p);
                    let d = 10.0;
                    let gx = land.height(p + Vec2::X * d) - land.height(p - Vec2::X * d);
                    let gy = land.height(p + Vec2::Y * d) - land.height(p - Vec2::Y * d);
                    let slope = (gx * gx + gy * gy).sqrt() / (2.0 * d);
                    if slope <= MAX_GRADED_SLOPE {
                        level += 1;
                    }
                    if slope <= MAX_TERRACE_SLOPE {
                        buildable += 1;
                    }
                    g.low = g.low.min(h);
                    g.high = g.high.max(h);
                    hsum += h as f64;
                    ssum += slope as f64;
                    let s = crate::ground::sample(land, p);
                    fert += s.fertility as f64;
                    rock += s.rock as f64;
                    if s.oil > 0.3 {
                        oil += 1;
                    }
                }
            }
        }
    }
    g.sea /= n;
    g.lake /= n;
    g.river /= n;
    g.land = dry as f32 / n;
    g.level = level as f32 / n;
    g.buildable = buildable as f32 / n;
    if dry > 0 {
        let k = dry as f64;
        g.mean = (hsum / k) as f32;
        g.slope = (ssum / k) as f32;
        g.fertility = (fert / k) as f32;
        g.rock = (rock / k) as f32;
        g.oil = oil as f32 / dry as f32;
    } else {
        g.low = 0.0;
        g.high = 0.0;
    }
    g.hot_spots = land.hot_spots.iter().filter(|h| h.pos.x >= lo.x && h.pos.x < hi.x && h.pos.y >= lo.y && h.pos.y < hi.y).count();
    for r in &land.rivers {
        if r.points.windows(2).any(|w| segment_box_overlap(w[0], w[1], lo, hi)) {
            if r.stream {
                g.stream_crosses = true;
            } else {
                g.river_crosses = true;
            }
        }
    }
    g
}

impl World {
    /// The ground of every tile that can be bought, surveyed once per world.
    pub fn tile_grounds(&self) -> &[TileGround] {
        self.land_tile_grounds.get_or_init(|| {
            // Any land bought grows the whole ring the same way; survey it
            // as it is (or will be) once land is bought.
            let grown;
            let land: &Landscape = if self.landscape.is_grown() {
                &self.landscape
            } else {
                grown = self.landscape.with_tiles(&outer_tiles());
                &grown
            };
            if land.is_flat() {
                return Vec::new();
            }
            outer_tiles().into_iter().map(|t| survey_tile(land, t)).collect()
        })
    }

    /// Ways crossing tile `t`: (the highway, the railway, streets and paths).
    fn tile_ways(&self, t: Tile) -> (bool, bool, usize) {
        let (lo, hi) = tile_rect(t);
        let (mut highway, mut rail, mut roads) = (false, false, 0);
        for (id, s) in self.roads.alive_segments() {
            let (a, b) = self.roads.segment_points(id);
            if !segment_box_overlap(a, b, lo, hi) {
                continue;
            }
            if s.is_rail() {
                rail = true;
            } else if s.is_highway() {
                highway = true;
            } else {
                roads += 1;
            }
        }
        (highway, rail, roads)
    }

    /// Every tile round the starting map: its ground, whether it is the
    /// city's or can be bought now, and its price.
    pub fn land_tiles(&self) -> Vec<LandTile> {
        let bought = self.landscape.tiles.len();
        let buyable = self.landscape.buyable_tiles();
        self.tile_grounds()
            .iter()
            .map(|g| {
                let (highway, rail, roads) = self.tile_ways(g.tile);
                let owned = self.landscape.owns_tile(g.tile);
                LandTile { ground: g.clone(), owned, buyable: buyable.contains(&g.tile), price: if owned { 0.0 } else { tile_price(g, bought, highway, rail) }, highway, rail, roads }
            })
            .collect()
    }

    pub fn land_tile(&self, t: Tile) -> Option<LandTile> {
        self.land_tiles().into_iter().find(|l| l.tile() == t)
    }

    /// Why tile `t` cannot be bought now, or its price.
    pub fn tile_offer(&self, t: Tile) -> Result<f32, String> {
        if self.landscape.is_flat() {
            return Err("the flat map has no land round it to buy".into());
        }
        if self.landscape.owns_tile(t) {
            return Err("that land is already the city's".into());
        }
        if tile_ring(t) > crate::landscape::MAX_RING {
            return Err(format!("land can be bought out to {:.0} km from the centre, one tile round the starting map", crate::landscape::GROWN_HALF / 1000.0));
        }
        let Some(tile) = self.land_tile(t) else { return Err("no such tile".into()) };
        if !tile.buyable {
            return Err("not next to the city's land: buy a tile beside it first".into());
        }
        if self.economy.treasury < tile.price {
            return Err(format!("the treasury has {:.0}; this land costs {:.0}", self.economy.treasury.max(0.0), tile.price));
        }
        Ok(tile.price)
    }

    /// Buy tile `t`: pay for it, and the land is the city's: parcels are
    /// cut along any road on it, the ground, grids and camera reach take it
    /// in. Returns the price paid.
    pub fn buy_tile(&mut self, t: Tile) -> Result<f32, String> {
        let price = self.tile_offer(t)?;
        self.economy.debit_item(crate::economy::Flow::Construction, price, crate::economy::Item::Named(format!("land: the {} tile", tile_direction(t))));
        let land = self.landscape.with_tiles(&[t]);
        self.landscape.set(land);
        self.grow_grids();
        let half = self.landscape.land_half();
        self.contamination.grow_to(half);
        self.rebuild_from_plan();
        Ok(price)
    }
}

/// Does the segment `a`-`b` touch the box `lo`-`hi`?
pub fn segment_box_overlap(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> bool {
    let inside = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    if inside(a) || inside(b) {
        return true;
    }
    if a.x.max(b.x) < lo.x || a.x.min(b.x) > hi.x || a.y.max(b.y) < lo.y || a.y.min(b.y) > hi.y {
        return false;
    }
    let c = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    (0..4).any(|k| crate::geom::segment_intersection_closed(a, b, c[k], c[(k + 1) % 4], 1e-4).is_some())
}
