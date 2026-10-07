//! Land value: a coarse grid over the city that sums accessibility,
//! amenities, nuisances, traffic and the tier of neighbouring buildings. It
//! drives which tier gets built where, rents, and therefore who can live
//! where. Recomputed every couple of sim hours.

use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LandValue {
    pub cell: f32,
    pub half: f32,
    pub n: usize,
    pub values: Vec<f32>,
    pub version: u64,
    /// How much of each cell's value was taken by nuisances (plants, heavy
    /// industry): homes feel all of it and more, industry little of it.
    /// See [`LandValue::for_use`]. Empty in old saves until the next update.
    #[serde(default)]
    pub nuisance: Vec<f32>,
    /// How far below zero each cell's sum went before clamping, so a land
    /// use that shrugs off nuisance still feels everything else.
    #[serde(default)]
    pub under: Vec<f32>,
    /// Minutes on foot from each cell to the nearest park, along the walk
    /// network: [`NO_DATA`] off the network (open country, the far side of
    /// the river), [`NONE_REACHABLE`] when no park can be walked to.
    /// Empty in old saves until the next update.
    #[serde(default)]
    pub park_min: Vec<f32>,
    /// The same to the nearest place to go out (entertainment).
    #[serde(default)]
    pub nightlife_min: Vec<f32>,
    /// What that access adds to (or takes off) each cell's value, already
    /// included in `values`; homes feel all of it, other uses take it back
    /// out in [`LandValue::for_use`] by [`access_sensitivity`].
    #[serde(default)]
    pub access: Vec<f32>,
}

/// `LandValue::park_min` / `nightlife_min` of a cell with no walk node near.
pub const NO_DATA: f32 = -1.0;
/// ... of a cell from which no such place can be walked to at all.
pub const NONE_REACHABLE: f32 = 999.0;
/// A cell centre further than this from every walk node is off the network.
pub const ACCESS_SNAP: f32 = 48.0;

/// Accessibility of a point: how many minutes' walk to the nearest park
/// and the nearest night out (`None`: none can be reached), and what that
/// is worth to a home there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Access {
    pub park_min: Option<f32>,
    pub nightlife_min: Option<f32>,
    pub value: f32,
}

impl Access {
    /// "parks 4 min walk · nightlife 12 min", for read-outs.
    pub fn describe(&self) -> String {
        let one = |m: Option<f32>, what: &str, walk: &str| match m {
            Some(m) => format!("{what} {} min{walk}", m.round().max(1.0) as i32),
            None => format!("no {what} within walking reach"),
        };
        format!("{} · {}", one(self.park_min, "parks", " walk"), one(self.nightlife_min, "nightlife", ""))
    }
}

/// Most a park on the doorstep adds to a home's land value, and most a
/// home with no park within reach loses. The loss is the larger: the rule
/// is about the long walk making a place cheaper, and a city where every
/// home is near a park should not simply inflate every rent (the demo's
/// homes average about +0.05 with these).
pub const PARK_ACCESS_BONUS: f32 = 0.03;
pub const PARK_ACCESS_PENALTY: f32 = 0.06;
/// The same for somewhere to go out: smaller, because nightlife next door
/// is also noise (the entertainment splat already takes some back).
pub const NIGHTLIFE_ACCESS_BONUS: f32 = 0.02;
pub const NIGHTLIFE_ACCESS_PENALTY: f32 = 0.04;

/// A piecewise-linear curve: the full `bonus` up to `good` minutes, nothing
/// at `fair`, the full `penalty` from `poor` on (and when unreachable).
fn access_curve(min: Option<f32>, good: f32, fair: f32, poor: f32, bonus: f32, penalty: f32) -> f32 {
    let Some(m) = min else { return -penalty };
    if m <= good {
        bonus
    } else if m <= fair {
        bonus * (fair - m) / (fair - good)
    } else if m < poor {
        -penalty * (m - fair) / (poor - fair)
    } else {
        -penalty
    }
}

/// What walking access to parks and nightlife is worth to a home (the
/// player's rule: a long way to any park or any night out makes a
/// neighbourhood cheaper). A park within 4 minutes is the full bonus, 10
/// minutes is neutral, 20 or more the full penalty; nightlife 5 / 12 / 25.
pub fn access_value(park_min: Option<f32>, nightlife_min: Option<f32>) -> f32 {
    access_curve(park_min, 4.0, 10.0, 20.0, PARK_ACCESS_BONUS, PARK_ACCESS_PENALTY) + access_curve(nightlife_min, 5.0, 12.0, 25.0, NIGHTLIFE_ACCESS_BONUS, NIGHTLIFE_ACCESS_PENALTY)
}

/// How much a land use cares about walking access to parks and nightlife:
/// homes fully; shops, works and civic buildings not at all (their value is
/// about customers, freight and the public, which other terms carry).
pub fn access_sensitivity(lu: LandUse) -> f32 {
    match lu {
        LandUse::Residential => 1.0,
        _ => 0.0,
    }
}

/// How much a land use minds the nuisance part of land value: 1 is the
/// grid as it is, more suffers extra, less shrugs part of it off.
pub fn nuisance_sensitivity(lu: LandUse) -> f32 {
    match lu {
        LandUse::Residential => 1.25,
        LandUse::Civic | LandUse::Park => 1.0,
        LandUse::Commercial => 0.85,
        LandUse::Entertainment => 0.7,
        LandUse::Industrial => 0.3,
    }
}

impl LandValue {
    pub fn new(half: f32, cell: f32) -> Self {
        let n = ((half * 2.0) / cell).ceil() as usize;
        Self { cell, half, n, values: vec![0.3; n * n], version: 0, nuisance: Vec::new(), under: Vec::new(), park_min: Vec::new(), nightlife_min: Vec::new(), access: Vec::new() }
    }

    /// Set every cell (tests and scenarios).
    pub fn fill(&mut self, v: f32) {
        self.values.iter_mut().for_each(|x| *x = v);
        self.version += 1;
    }

    /// Index of the cell holding `p` (edge cells extend outward).
    fn index(&self, p: Vec2) -> usize {
        let (x, y) = self.coords(p);
        y.clamp(0, self.n as i32 - 1) as usize * self.n + x.clamp(0, self.n as i32 - 1) as usize
    }

    /// Walking access to parks and nightlife at `p`, `None` before the
    /// first update or off the walk network.
    pub fn access_at(&self, p: Vec2) -> Option<Access> {
        let len = self.n * self.n;
        if self.park_min.len() != len || self.nightlife_min.len() != len || self.access.len() != len {
            return None;
        }
        let i = self.index(p);
        let (pm, nm) = (self.park_min[i], self.nightlife_min[i]);
        if pm == NO_DATA || nm == NO_DATA {
            return None;
        }
        let some = |m: f32| (m < NONE_REACHABLE).then_some(m);
        Some(Access { park_min: some(pm), nightlife_min: some(nm), value: self.access[i] })
    }

    /// The access term (see [`access_value`]) included in the cell at `p`.
    pub fn access_value_at(&self, p: Vec2) -> f32 {
        if self.access.len() != self.n * self.n {
            return 0.0;
        }
        self.access[self.index(p)]
    }

    fn coords(&self, p: Vec2) -> (i32, i32) {
        (((p.x + self.half) / self.cell).floor() as i32, ((p.y + self.half) / self.cell).floor() as i32)
    }

    pub fn cell_center(&self, ix: usize, iy: usize) -> Vec2 {
        Vec2::new(-self.half + (ix as f32 + 0.5) * self.cell, -self.half + (iy as f32 + 0.5) * self.cell)
    }

    /// Value in `0..1` at a point (edge cells extend outward).
    pub fn at(&self, p: Vec2) -> f32 {
        let (x, y) = self.coords(p);
        let x = x.clamp(0, self.n as i32 - 1) as usize;
        let y = y.clamp(0, self.n as i32 - 1) as usize;
        self.values[y * self.n + x]
    }

    /// Nuisance (0..1) at a point: the value plants and heavy industry took.
    pub fn nuisance_at(&self, p: Vec2) -> f32 {
        if self.nuisance.len() != self.n * self.n {
            return 0.0;
        }
        let (x, y) = self.coords(p);
        let x = x.clamp(0, self.n as i32 - 1) as usize;
        let y = y.clamp(0, self.n as i32 - 1) as usize;
        self.nuisance[y * self.n + x]
    }

    /// Value at a point for a building of land use `lu`: homes beside a
    /// power plant lose more than the grid shows, industry less.
    pub fn for_use(&self, p: Vec2, lu: LandUse) -> f32 {
        let n = self.nuisance_at(p);
        let under = if self.under.len() == self.n * self.n {
            let (x, y) = self.coords(p);
            self.under[y.clamp(0, self.n as i32 - 1) as usize * self.n + x.clamp(0, self.n as i32 - 1) as usize]
        } else {
            0.0
        };
        // Homes keep the access term; other uses take it back out.
        let access = self.access_value_at(p) * (1.0 - access_sensitivity(lu));
        (self.at(p) + under + n * (1.0 - nuisance_sensitivity(lu)) - access).clamp(0.0, 1.0)
    }

    pub fn average(&self) -> f32 {
        if self.values.is_empty() {
            return 0.0;
        }
        self.values.iter().sum::<f32>() / self.values.len() as f32
    }
}

/// Most the activity of working industry can lift run-down land.
pub const MAX_LIFT: f32 = 0.1;

/// Accumulates influences and produces the next value grid. Saved with
/// the world when a save falls mid-cycle.
#[derive(Serialize, Deserialize)]
pub struct LandValueBuilder {
    n: usize,
    cell: f32,
    half: f32,
    acc: Vec<f32>,
    nuisance: Vec<f32>,
    lift: Vec<f32>,
    /// Minutes to the nearest park and night out per cell, once
    /// [`set_access`](Self::set_access) ran (empty: no access term).
    park_min: Vec<f32>,
    nightlife_min: Vec<f32>,
}

impl LandValueBuilder {
    pub fn new(lv: &LandValue) -> Self {
        Self { n: lv.n, cell: lv.cell, half: lv.half, acc: vec![0.0; lv.n * lv.n], nuisance: vec![0.0; lv.n * lv.n], lift: vec![0.0; lv.n * lv.n], park_min: Vec::new(), nightlife_min: Vec::new() }
    }

    /// The grid's side in cells, cell size and half extent, for whoever
    /// fills per-cell terms.
    pub fn cells(&self) -> (usize, f32, f32) {
        (self.n, self.cell, self.half)
    }

    /// Walking minutes per cell (row-major, `n * n`) to the nearest park and
    /// night out, with [`NO_DATA`] and [`NONE_REACHABLE`] as in `LandValue`.
    /// Ignored unless both are the size of the grid.
    pub fn set_access(&mut self, park_min: Vec<f32>, nightlife_min: Vec<f32>) {
        if park_min.len() == self.n * self.n && nightlife_min.len() == self.n * self.n {
            self.park_min = park_min;
            self.nightlife_min = nightlife_min;
        }
    }

    /// Add `weight` at `center`, falling off linearly to zero at `radius`.
    pub fn splat(&mut self, center: Vec2, radius: f32, weight: f32) {
        Self::splat_into(self.n, self.cell, self.half, &mut self.acc, center, radius, weight);
    }

    /// A nuisance of `weight` (positive): taken off the value like a
    /// negative splat, and remembered so land uses can feel it differently.
    pub fn splat_nuisance(&mut self, center: Vec2, radius: f32, weight: f32) {
        Self::splat_into(self.n, self.cell, self.half, &mut self.acc, center, radius, -weight);
        Self::splat_into(self.n, self.cell, self.half, &mut self.nuisance, center, radius, weight);
    }

    /// Activity that lifts run-down land only: applied after the blur, in
    /// full where the land is nearly worthless and not at all from 0.35 up.
    pub fn splat_lift(&mut self, center: Vec2, radius: f32, weight: f32) {
        Self::splat_into(self.n, self.cell, self.half, &mut self.lift, center, radius, weight);
    }

    fn splat_into(n: usize, cell: f32, half: f32, buf: &mut [f32], center: Vec2, radius: f32, weight: f32) {
        let r_cells = (radius / cell).ceil() as i32;
        let cx = ((center.x + half) / cell).floor() as i32;
        let cy = ((center.y + half) / cell).floor() as i32;
        for dy in -r_cells..=r_cells {
            let y = cy + dy;
            if y < 0 || y >= n as i32 {
                continue;
            }
            for dx in -r_cells..=r_cells {
                let x = cx + dx;
                if x < 0 || x >= n as i32 {
                    continue;
                }
                let p = Vec2::new(-half + (x as f32 + 0.5) * cell, -half + (y as f32 + 0.5) * cell);
                let d = p.distance(center);
                if d < radius {
                    buf[y as usize * n + x as usize] += weight * (1.0 - d / radius);
                }
            }
        }
    }

    fn blurred(n: usize, buf: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0f32; n * n];
        for y in 0..n {
            for x in 0..n {
                let mut sum = 0.0;
                let mut count = 0.0;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                        if xx >= 0 && yy >= 0 && xx < n as i32 && yy < n as i32 {
                            sum += buf[yy as usize * n + xx as usize];
                            count += 1.0;
                        }
                    }
                }
                out[y * n + x] = sum / count;
            }
        }
        out
    }

    /// Clamp, blur once and write into `lv`. The lift from industry is
    /// added after the blur, scaled to nothing on land already worth 0.35.
    pub fn finish(self, base: f32, lv: &mut LandValue) {
        let n = self.n;
        let acc = Self::blurred(n, &self.acc);
        let lift = Self::blurred(n, &self.lift);
        let nuisance = Self::blurred(n, &self.nuisance);
        // Walking access is per cell already (a walk, not a radius), so it
        // goes in after the blur, unsmoothed.
        let some = |m: f32| (m < NONE_REACHABLE).then_some(m);
        let access: Vec<f32> = if self.park_min.len() == n * n {
            self.park_min.iter().zip(&self.nightlife_min).map(|(&p, &q)| if p == NO_DATA || q == NO_DATA { 0.0 } else { access_value(some(p), some(q)) }).collect()
        } else {
            Vec::new()
        };
        let acc: Vec<f32> = if access.is_empty() { acc } else { acc.iter().zip(&access).map(|(a, x)| a + x).collect() };
        lv.under = acc.iter().map(|a| (base + a).min(0.0)).collect();
        let out: Vec<f32> = acc
            .iter()
            .zip(&lift)
            .map(|(a, l)| {
                let v = (base + a).clamp(0.0, 1.0);
                let low = 1.0 - ((v - 0.1) / 0.25).clamp(0.0, 1.0);
                (v + l.clamp(0.0, MAX_LIFT) * low).clamp(0.0, 1.0)
            })
            .collect();
        lv.values = out;
        lv.nuisance = nuisance.into_iter().map(|x| x.clamp(0.0, 1.0)).collect();
        lv.access = access;
        lv.park_min = self.park_min;
        lv.nightlife_min = self.nightlife_min;
        lv.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_the_half_extent_and_fill_bumps_the_version() {
        let mut lv = LandValue::new(400.0, 32.0);
        assert_eq!(lv.n, 25, "800 m at 32 m cells rounds up to 25");
        assert_eq!(lv.values.len(), 25 * 25);
        assert_eq!(lv.version, 0);
        assert!((lv.at(Vec2::ZERO) - 0.3).abs() < 1e-6, "fresh land is worth a little");
        lv.fill(0.75);
        assert_eq!(lv.version, 1);
        assert!(lv.values.iter().all(|&v| v == 0.75));
        assert_eq!(lv.average(), 0.75);
        // Points off the grid read the edge cell rather than panicking.
        assert_eq!(lv.at(Vec2::new(10_000.0, -10_000.0)), 0.75);
        lv.values[0] = 0.1;
        assert_eq!(lv.at(Vec2::new(-10_000.0, -10_000.0)), 0.1, "the far south-west corner is cell 0");
        assert!(lv.cell_center(0, 0).distance(Vec2::new(-384.0, -384.0)) < 1e-4);
        assert_eq!(lv.at(lv.cell_center(0, 0)), 0.1);
        assert_eq!(LandValue::new(10.0, 32.0).average(), 0.3);
    }

    #[test]
    fn the_builder_splats_blurs_and_clamps() {
        let mut lv = LandValue::new(400.0, 32.0);
        let before = lv.version;
        let mut b = LandValueBuilder::new(&lv);
        b.splat(Vec2::ZERO, 150.0, 2.0);
        b.splat(Vec2::new(300.0, 300.0), 60.0, -5.0);
        b.finish(0.3, &mut lv);
        assert_eq!(lv.version, before + 1);
        assert!(lv.values.iter().all(|v| (0.0..=1.0).contains(v)), "clamped to 0..1");
        let centre = lv.at(Vec2::ZERO);
        let edge = lv.at(Vec2::new(120.0, 0.0));
        let far = lv.at(Vec2::new(-300.0, -300.0));
        assert!(centre > edge && edge > far, "value falls off from the splat: {centre} {edge} {far}");
        assert!((far - 0.3).abs() < 1e-4, "untouched land is the base");
        assert_eq!(lv.at(Vec2::new(300.0, 300.0)), 0.0, "a nuisance drags land to the floor");
        // A splat off the grid touches nothing.
        let mut b = LandValueBuilder::new(&lv);
        b.splat(Vec2::new(5000.0, 5000.0), 100.0, 9.0);
        let mut other = LandValue::new(400.0, 32.0);
        b.finish(0.3, &mut other);
        assert!(other.values.iter().all(|&v| (v - 0.3).abs() < 1e-6));
    }

    #[test]
    fn nuisance_hurts_homes_more_than_industry_and_lift_only_helps_cheap_land() {
        let mut lv = LandValue::new(400.0, 32.0);
        let mut b = LandValueBuilder::new(&lv);
        b.splat(Vec2::new(-200.0, 0.0), 120.0, 0.4);
        b.splat_nuisance(Vec2::new(200.0, 0.0), 150.0, 0.2);
        b.splat_lift(Vec2::new(-200.0, 0.0), 150.0, 0.2);
        b.splat_lift(Vec2::new(0.0, -300.0), 150.0, 0.2);
        b.finish(0.3, &mut lv);
        let p = Vec2::new(200.0, 0.0);
        let (home, shop, works) = (lv.for_use(p, LandUse::Residential), lv.for_use(p, LandUse::Commercial), lv.for_use(p, LandUse::Industrial));
        assert!(home < lv.at(p) && lv.at(p) < shop && shop < works, "{home} {} {shop} {works}", lv.at(p));
        assert!(lv.nuisance_at(p) > 0.1 && lv.nuisance_at(Vec2::new(-300.0, 300.0)) == 0.0);
        // Dear land gets no lift; cheap land some.
        let dear = lv.at(Vec2::new(-200.0, 0.0));
        assert!(dear > 0.5 && dear < 0.75, "{dear}");
        let cheap = lv.at(Vec2::new(0.0, -300.0));
        assert!(cheap > 0.3 && cheap <= 0.3 + MAX_LIFT, "{cheap}");
    }

    #[test]
    fn walking_access_lifts_homes_near_parks_and_costs_homes_far_from_any() {
        // The curve: a bonus close by, nothing at "fair", a penalty far off
        // or out of reach, never worth more for being further.
        assert!((access_value(Some(2.0), Some(3.0)) - (PARK_ACCESS_BONUS + NIGHTLIFE_ACCESS_BONUS)).abs() < 1e-6);
        assert!(access_value(Some(10.0), Some(12.0)).abs() < 1e-6);
        assert!((access_value(None, None) + PARK_ACCESS_PENALTY + NIGHTLIFE_ACCESS_PENALTY).abs() < 1e-6);
        let mut last = f32::INFINITY;
        for m in 0..60 {
            let v = access_value(Some(m as f32), Some(m as f32));
            assert!(v <= last + 1e-6, "{m} min worth more than {} min", m - 1);
            last = v;
        }
        assert!(access_value(Some(60.0), Some(60.0)) == access_value(None, None));

        // Through the builder: homes feel it, shops not at all.
        let mut lv = LandValue::new(400.0, 32.0);
        let mut b = LandValueBuilder::new(&lv);
        let (n, _, _) = b.cells();
        let (near, far, off) = (Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(0.0, 300.0));
        let mut park = vec![2.0; n * n];
        let mut night = vec![3.0; n * n];
        park[lv.index(far)] = NONE_REACHABLE;
        night[lv.index(far)] = 40.0;
        park[lv.index(off)] = NO_DATA;
        night[lv.index(off)] = NO_DATA;
        b.set_access(park, night);
        b.finish(0.4, &mut lv);
        assert!((lv.for_use(near, LandUse::Residential) - 0.45).abs() < 1e-4, "homes near both: {}", lv.for_use(near, LandUse::Residential));
        assert!((lv.for_use(near, LandUse::Commercial) - 0.4).abs() < 1e-4, "shops do not care");
        assert!((lv.for_use(far, LandUse::Residential) - 0.3).abs() < 1e-4, "homes far from both: {}", lv.for_use(far, LandUse::Residential));
        assert!((lv.for_use(far, LandUse::Industrial) - 0.4).abs() < 1e-4);
        assert!((lv.for_use(off, LandUse::Residential) - 0.4).abs() < 1e-4, "off the walk network: no term");
        let a = lv.access_at(near).unwrap();
        assert_eq!((a.park_min, a.nightlife_min), (Some(2.0), Some(3.0)));
        assert_eq!(a.describe(), "parks 2 min walk · nightlife 3 min");
        let a = lv.access_at(far).unwrap();
        assert_eq!((a.park_min, a.nightlife_min), (None, Some(40.0)));
        assert!(a.describe().starts_with("no parks within walking reach"));
        assert!(lv.access_at(off).is_none());
        // An old save: no access data at all.
        let old = LandValue::new(400.0, 32.0);
        assert!(old.access_at(near).is_none() && old.access_value_at(near) == 0.0);
        assert_eq!(old.for_use(near, LandUse::Residential), 0.3);
    }
}
