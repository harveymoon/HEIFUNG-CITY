//! The ground as industry sees it: soil fertility, oil under the ground,
//! rock and sand, and how close the water is. All of that is read straight
//! from the landscape seed and features, so the same map always has the
//! same good grass and the same oil patches and nothing extra is saved.
//!
//! What industry does to the ground is saved: [`Contamination`] is a coarse
//! raster (0..1 per cell) that oil wells, refineries, mines, smelters,
//! chemical works, factory farms and coal ash raise, that spreads a little
//! to neighbouring cells every day and fades over months.
//! Contaminated ground loses land value and spoils crops.
//!
//! (Not to be confused with [`crate::landscape::Ground`], the raster of the
//! *built* ground heights the renderer draws.)

use crate::landscape::{fbm, Landscape, LandscapePreset, WaterKind};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Side of a contamination cell, metres.
pub const CONTAMINATION_CELL: f32 = 32.0;
/// Share of a cell's contamination that fades each day: slowly, so
/// poisoned ground stays poisoned for months after the plant is gone
/// (half of it after about ten weeks; parks and a clean-up go faster,
/// `world/dirt.rs`).
pub const CONTAMINATION_DECAY_PER_DAY: f32 = 0.01;
/// Share of a cell's contamination that seeps into its four neighbours each day.
pub const CONTAMINATION_SPREAD_PER_DAY: f32 = 0.12;
/// Cells below this are forgotten (kept sparse in the save).
const CONTAMINATION_EPS: f32 = 0.002;
/// [dirty-industry] Share of a cell carried one cell downwind a day in a
/// full gale (dust off poisoned ground; an ordinary day's wind about half).
pub const CONTAMINATION_WIND_DRIFT: f32 = 0.05;
/// Share of a cell over (or on the bank of) a river or stream carried one
/// cell downstream a day, at a current of 1 m/s or more (half that in still water).
pub const CONTAMINATION_RIVER_DRIFT: f32 = 0.3;
/// Below this a cell is left where it is: drift moves the stains, not the traces.
const DRIFT_MIN: f32 = 0.01;

/// What the ground is, for the map and the inspector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Soil {
    /// Deep, well-watered soil: farm country.
    Fertile,
    /// Ordinary grass.
    Grass,
    /// Beach and dune by the sea.
    Sand,
    /// Thin soil over rock.
    Rock,
    /// Oil seeps: black patches over a deposit.
    Oil,
    Water,
}

impl Soil {
    pub fn label(self) -> &'static str {
        match self {
            Soil::Fertile => "fertile",
            Soil::Grass => "grass",
            Soil::Sand => "sand",
            Soil::Rock => "rock",
            Soil::Oil => "oil",
            Soil::Water => "water",
        }
    }
}

/// The static ground at a point, every field `0..1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroundSample {
    /// How well crops grow: deep soil near rivers and on the plains.
    pub fertility: f32,
    /// How rich the oil deposit under the point is.
    pub oil: f32,
    pub rock: f32,
    pub sand: f32,
    /// Closeness to open water: 1 at the shore, 0 from about 160 m.
    pub water: f32,
}

impl GroundSample {
    pub fn soil(&self) -> Soil {
        if self.water >= 1.0 && self.fertility == 0.0 && self.oil == 0.0 {
            Soil::Water
        } else if self.oil > 0.5 {
            Soil::Oil
        } else if self.rock > 0.5 {
            Soil::Rock
        } else if self.sand > 0.5 {
            Soil::Sand
        } else if self.fertility > 0.6 {
            Soil::Fertile
        } else {
            Soil::Grass
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Oil under a point: patches a few hundred metres across where a noise
/// field peaks. Roughly a tenth of the land sits over some oil.
pub fn oil_at(land: &Landscape, p: Vec2) -> f32 {
    let n = fbm(land.params.seed ^ 0x0117_0C1D, p * 0.0028 + Vec2::new(31.7, -12.9), 3);
    smoothstep(0.60, 0.70, n)
}

/// The static ground at `p`, deterministic in the landscape seed.
pub fn sample(land: &Landscape, p: Vec2) -> GroundSample {
    let seed = land.params.seed;
    let flat = land.is_flat();
    let wd = land.water_distance(p);
    if wd < 0.0 {
        return GroundSample { water: 1.0, ..GroundSample::default() };
    }
    let water = if flat { 0.0 } else { 1.0 - smoothstep(30.0, 160.0, wd) };
    let oil = oil_at(land, p);
    // Rock where a coarse noise peaks, more of it on steep ground.
    let step = 8.0;
    let gx = land.height(p + Vec2::X * step) - land.height(p - Vec2::X * step);
    let gy = land.height(p + Vec2::Y * step) - land.height(p - Vec2::Y * step);
    let slope = (gx * gx + gy * gy).sqrt() / (2.0 * step);
    let rock = (smoothstep(0.64, 0.78, fbm(seed ^ 0x40C, p * 0.005, 2)) + smoothstep(0.08, 0.2, slope)).min(1.0);
    // Sand on the sea shore.
    let sand = if land.sea.is_some() && wd < 70.0 {
        land.nearest_shore(p, 70.0).filter(|s| s.water.kind == WaterKind::Sea).map_or(0.0, |s| 1.0 - smoothstep(15.0, 70.0, s.distance))
    } else {
        0.0
    };
    // Deep soil on river banks and the plains; never on oil, rock or sand.
    let n = fbm(seed ^ 0xF3A7, p * 0.0035, 3);
    let banks = if flat { 0.0 } else { 1.0 - smoothstep(20.0, 240.0, wd) };
    let plains = match land.params.preset {
        LandscapePreset::Plains => 0.18,
        LandscapePreset::Delta => 0.12,
        LandscapePreset::RiverValley => 0.05,
        _ => 0.0,
    };
    let fertility = (0.1 + 0.6 * n + 0.35 * banks + plains - 0.6 * oil - 0.6 * rock - 0.7 * sand).clamp(0.0, 1.0);
    GroundSample { fertility, oil, rock, sand, water }
}

/// Mean of the ground over a lot outline and its centre, so a lot that
/// only grazes a patch is not taken for it.
pub fn sample_area(land: &Landscape, center: Vec2, outline: &[Vec2]) -> GroundSample {
    let mut pts: Vec<Vec2> = outline.iter().map(|q| center.lerp(*q, 0.7)).collect();
    pts.push(center);
    let n = pts.len() as f32;
    let mut acc = GroundSample::default();
    for p in pts {
        let s = sample(land, p);
        acc.fertility += s.fertility / n;
        acc.oil += s.oil / n;
        acc.rock += s.rock / n;
        acc.sand += s.sand / n;
        acc.water = acc.water.max(s.water);
    }
    acc
}

/// What industry has done to the ground: a sparse raster of contamination
/// in `0..1`, saved with the world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Contamination {
    pub cell: f32,
    pub half: f32,
    pub n: usize,
    /// Cell index (`y * n + x`) to contamination; missing cells are clean.
    pub cells: BTreeMap<u32, f32>,
    /// Bumped on every change, for overlays.
    pub version: u64,
}

impl Default for Contamination {
    fn default() -> Self {
        Self::new(1100.0, CONTAMINATION_CELL)
    }
}

impl Contamination {
    pub fn new(half: f32, cell: f32) -> Self {
        let n = ((half * 2.0) / cell).ceil().max(1.0) as usize;
        Self { cell, half, n, cells: BTreeMap::new(), version: 0 }
    }

    fn coords(&self, p: Vec2) -> Option<(usize, usize)> {
        let x = ((p.x + self.half) / self.cell).floor();
        let y = ((p.y + self.half) / self.cell).floor();
        if x < 0.0 || y < 0.0 || x as usize >= self.n || y as usize >= self.n {
            return None;
        }
        Some((x as usize, y as usize))
    }

    pub fn cell_center(&self, ix: usize, iy: usize) -> Vec2 {
        Vec2::new(-self.half + (ix as f32 + 0.5) * self.cell, -self.half + (iy as f32 + 0.5) * self.cell)
    }

    fn get(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x as usize >= self.n || y as usize >= self.n {
            return 0.0;
        }
        self.cells.get(&((y as usize * self.n + x as usize) as u32)).copied().unwrap_or(0.0)
    }

    /// Contamination at a point, blended between cell centres.
    pub fn at(&self, p: Vec2) -> f32 {
        if self.cells.is_empty() {
            return 0.0;
        }
        let fx = (p.x + self.half) / self.cell - 0.5;
        let fy = (p.y + self.half) / self.cell - 0.5;
        let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let a = self.get(x0, y0);
        let b = self.get(x0 + 1, y0);
        let c = self.get(x0, y0 + 1);
        let d = self.get(x0 + 1, y0 + 1);
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        (top + (bot - top) * ty).clamp(0.0, 1.0)
    }

    /// Worst contamination over a lot: its centre and corners.
    pub fn over(&self, center: Vec2, outline: &[Vec2]) -> f32 {
        outline.iter().map(|q| self.at(center.lerp(*q, 0.7))).fold(self.at(center), f32::max)
    }

    /// Pour `amount` onto the ground around `center`, falling off linearly
    /// to nothing at `radius`; a cell never goes above 1.
    pub fn add(&mut self, center: Vec2, radius: f32, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        let r = (radius / self.cell).ceil() as i64;
        let Some((cx, cy)) = self.coords(center) else { return };
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx as i64 + dx, cy as i64 + dy);
                if x < 0 || y < 0 || x as usize >= self.n || y as usize >= self.n {
                    continue;
                }
                let d = self.cell_center(x as usize, y as usize).distance(center);
                let k = if radius <= 0.0 { if dx == 0 && dy == 0 { 1.0 } else { 0.0 } } else { (1.0 - d / radius.max(self.cell * 0.75)).max(0.0) };
                if k <= 0.0 {
                    continue;
                }
                let e = self.cells.entry((y as usize * self.n + x as usize) as u32).or_insert(0.0);
                *e = (*e + amount * k).min(1.0);
            }
        }
        self.version += 1;
    }

    /// One day: a share of every cell seeps into its four neighbours, then
    /// everything fades a little. Cells that fall below a trace are dropped.
    pub fn daily(&mut self) {
        if self.cells.is_empty() {
            return;
        }
        let n = self.n as i64;
        let mut next: BTreeMap<u32, f32> = BTreeMap::new();
        for (&i, &v) in &self.cells {
            let (x, y) = ((i as i64) % n, (i as i64) / n);
            let out = v * CONTAMINATION_SPREAD_PER_DAY;
            *next.entry(i).or_insert(0.0) += v - out;
            let mut kept = 0.0;
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (xx, yy) = (x + dx, y + dy);
                if xx < 0 || yy < 0 || xx >= n || yy >= n {
                    kept += out * 0.25;
                    continue;
                }
                *next.entry((yy * n + xx) as u32).or_insert(0.0) += out * 0.25;
            }
            *next.entry(i).or_insert(0.0) += kept;
        }
        for v in next.values_mut() {
            *v = (*v * (1.0 - CONTAMINATION_DECAY_PER_DAY)).min(1.0);
        }
        next.retain(|_, v| *v >= CONTAMINATION_EPS);
        self.cells = next;
        self.version += 1;
    }

    /// [dirty-industry] One day's drift: dust off poisoned ground blows a
    /// cell downwind (`wind`: the direction the air moves, its length the
    /// wind's strength `0..1`), and what lies in or on the bank of a river
    /// or stream is carried a cell downstream. Moves contamination without
    /// making any; what leaves the map is gone. Deterministic (cells in
    /// order), and cheap: only cells with a stain are looked at, and the
    /// rivers through a bounding-box test first.
    pub fn drift(&mut self, wind: Vec2, rivers: &[crate::landscape::River]) {
        if self.cells.is_empty() {
            return;
        }
        let n = self.n as i64;
        let strength = wind.length().min(1.0);
        let dir = wind.normalize_or_zero();
        let (wx, wy) = (dir.x.abs(), dir.y.abs());
        let wsum = (wx + wy).max(1e-6);
        let (sx, sy) = (dir.x.signum() as i64, dir.y.signum() as i64);
        // Rivers' reach, boxed once.
        let reach = |r: &crate::landscape::River| r.width * 0.5 + self.cell * 0.75;
        let boxes: Vec<(Vec2, Vec2)> = rivers
            .iter()
            .map(|r| {
                let e = Vec2::splat(reach(r));
                let lo = r.points.iter().copied().fold(Vec2::splat(f32::MAX), Vec2::min) - e;
                let hi = r.points.iter().copied().fold(Vec2::splat(f32::MIN), Vec2::max) + e;
                (lo, hi)
            })
            .collect();
        let mut moves: Vec<(u32, Option<u32>, f32)> = Vec::new();
        let index = |x: i64, y: i64| if x < 0 || y < 0 || x >= n || y >= n { None } else { Some((y * n + x) as u32) };
        for (&i, &v) in &self.cells {
            if v < DRIFT_MIN {
                continue;
            }
            let (x, y) = ((i as i64) % n, (i as i64) / n);
            let c = self.cell_center(x as usize, y as usize);
            let mut left = v;
            // Downstream first: water carries more than air.
            for (r, (lo, hi)) in rivers.iter().zip(&boxes) {
                if r.points.len() < 2 || c.x < lo.x || c.y < lo.y || c.x > hi.x || c.y > hi.y {
                    continue;
                }
                let (seg, _, d) = r.nearest(c);
                if d > reach(r) {
                    continue;
                }
                let flow = r.flow(seg);
                let to = c + flow * self.cell;
                let (tx, ty) = (((to.x + self.half) / self.cell).floor() as i64, ((to.y + self.half) / self.cell).floor() as i64);
                if (tx, ty) == (x, y) {
                    break;
                }
                let share = CONTAMINATION_RIVER_DRIFT * (0.5 + 0.5 * r.current.clamp(0.0, 1.0));
                let amount = left * share;
                moves.push((i, index(tx, ty), amount));
                left -= amount;
                break;
            }
            if strength > 0.0 && dir != Vec2::ZERO {
                let amount = left * CONTAMINATION_WIND_DRIFT * strength;
                if wx > 1e-3 {
                    moves.push((i, index(x + sx, y), amount * wx / wsum));
                }
                if wy > 1e-3 {
                    moves.push((i, index(x, y + sy), amount * wy / wsum));
                }
            }
        }
        if moves.is_empty() {
            return;
        }
        for (from, to, amount) in moves {
            if let Some(v) = self.cells.get_mut(&from) {
                *v -= amount;
            }
            if let Some(t) = to {
                let e = self.cells.entry(t).or_insert(0.0);
                *e = (*e + amount).min(1.0);
            }
        }
        self.cells.retain(|_, v| *v >= CONTAMINATION_EPS);
        self.version += 1;
    }

    /// [dirty-industry] Contamination in the cells whose centres lie within
    /// `radius` of `center`, summed (cell units).
    pub fn within(&self, center: Vec2, radius: f32) -> f32 {
        self.cells_within(center, radius).into_iter().filter_map(|i| self.cells.get(&i)).sum()
    }

    /// [dirty-industry] Cells within `radius` of `center` at or above `level`.
    pub fn cells_over_within(&self, center: Vec2, radius: f32, level: f32) -> usize {
        self.cells_within(center, radius).into_iter().filter(|i| self.cells.get(i).is_some_and(|v| *v >= level)).count()
    }

    /// [dirty-industry] Multiply the cells within `radius` of `center` by
    /// `k` (0 cleans them out).
    pub fn scale_within(&mut self, center: Vec2, radius: f32, k: f32) {
        let mut changed = false;
        for i in self.cells_within(center, radius) {
            if let Some(v) = self.cells.get_mut(&i) {
                *v *= k.clamp(0.0, 1.0);
                changed = true;
            }
        }
        if changed {
            self.cells.retain(|_, v| *v >= CONTAMINATION_EPS);
            self.version += 1;
        }
    }

    fn cells_within(&self, center: Vec2, radius: f32) -> Vec<u32> {
        let Some((cx, cy)) = self.coords(center) else { return Vec::new() };
        let r = (radius / self.cell).ceil() as i64 + 1;
        let mut out = Vec::new();
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx as i64 + dx, cy as i64 + dy);
                if x < 0 || y < 0 || x as usize >= self.n || y as usize >= self.n {
                    continue;
                }
                if self.cell_center(x as usize, y as usize).distance(center) <= radius.max(self.cell * 0.5) {
                    out.push((y as usize * self.n + x as usize) as u32);
                }
            }
        }
        out
    }

    /// Sum over the cells, in cell units (for stats and tests).
    pub fn total(&self) -> f32 {
        self.cells.values().sum()
    }

    pub fn max(&self) -> f32 {
        self.cells.values().copied().fold(0.0, f32::max)
    }

    /// Cells at or above `level`.
    pub fn cells_over(&self, level: f32) -> usize {
        self.cells.values().filter(|v| **v >= level).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::LandscapeParams;

    #[test]
    fn the_ground_is_deterministic_in_the_seed() {
        let a = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 11));
        let b = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 11));
        let c = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 12));
        let pts: Vec<Vec2> = (0..60).map(|i| Vec2::new(-700.0 + 23.7 * i as f32, 400.0 - 17.3 * i as f32)).collect();
        let sa: Vec<GroundSample> = pts.iter().map(|p| sample(&a, *p)).collect();
        let sb: Vec<GroundSample> = pts.iter().map(|p| sample(&b, *p)).collect();
        let sc: Vec<GroundSample> = pts.iter().map(|p| sample(&c, *p)).collect();
        assert_eq!(sa, sb, "same seed, same ground");
        assert_ne!(sa, sc, "another seed, other ground");
        for s in &sa {
            for v in [s.fertility, s.oil, s.rock, s.sand, s.water] {
                assert!((0.0..=1.0).contains(&v), "{s:?}");
            }
        }
    }

    #[test]
    fn oil_comes_in_patches_and_banks_are_fertile() {
        let land = Landscape::new(&LandscapeParams::preset(LandscapePreset::RiverValley, 7));
        let mut oily = 0;
        let mut n = 0;
        let (mut near, mut nn, mut far, mut nf) = (0.0, 0, 0.0, 0);
        for iy in 0..80 {
            for ix in 0..80 {
                let p = Vec2::new(-900.0 + ix as f32 * 22.5, -900.0 + iy as f32 * 22.5);
                let s = sample(&land, p);
                if s.water >= 1.0 && s.fertility == 0.0 {
                    continue;
                }
                n += 1;
                oily += (s.oil > 0.5) as i32;
                let wd = land.water_distance(p);
                if s.oil < 0.1 && s.rock < 0.1 {
                    if wd < 60.0 {
                        near += s.fertility;
                        nn += 1;
                    } else if wd > 150.0 {
                        far += s.fertility;
                        nf += 1;
                    }
                }
            }
        }
        let share = oily as f32 / n as f32;
        assert!(share > 0.02 && share < 0.3, "oil patches cover some of the land, not all: {share}");
        assert!(nn > 0 && nf > 0);
        assert!(near / nn as f32 > far / nf as f32 + 0.1, "river banks are better soil: {} vs {}", near / nn as f32, far / nf as f32);
    }

    #[test]
    fn contamination_spreads_decays_and_saves_sparse() {
        let mut c = Contamination::new(400.0, 32.0);
        assert_eq!(c.at(Vec2::ZERO), 0.0);
        c.add(Vec2::ZERO, 40.0, 0.8);
        let peak = c.at(Vec2::ZERO);
        assert!(peak > 0.4, "{peak}");
        let away = Vec2::new(80.0, 0.0);
        assert_eq!(c.at(away), 0.0);
        let total0 = c.total();
        for _ in 0..10 {
            c.daily();
        }
        assert!(c.at(away) > 0.0, "it seeps outward");
        assert!(c.at(Vec2::ZERO) < peak, "the source cell drains and fades");
        assert!(c.total() < total0, "the ground recovers overall");
        for _ in 0..1200 {
            c.daily();
        }
        assert!(c.cells.is_empty(), "after a long while the ground is clean: {}", c.total());
        // Never above one.
        for _ in 0..50 {
            c.add(Vec2::new(10.0, 10.0), 0.0, 0.5);
        }
        assert!(c.max() <= 1.0);
        let text = ron::to_string(&c).unwrap();
        let back: Contamination = ron::from_str(&text).unwrap();
        assert_eq!(back, c);
    }
}
