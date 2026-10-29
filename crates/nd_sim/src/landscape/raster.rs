//! The height raster and the queries on it: heights, water, shores, lot
//! ground and slopes.

use super::*;

/// Heights and signed distance to water (negative in the water), sampled
/// every `CELL` metres over the generated square.
pub(super) struct Raster {
    /// [r4-tiles] Half the side it covers: the land's (`land_half`).
    pub(super) half: f32,
    pub(super) n: usize,
    pub(super) h: Vec<f32>,
    pub(super) d: Vec<f32>,
}

impl Clone for Raster {
    fn clone(&self) -> Self {
        Raster { half: self.half, n: self.n, h: self.h.clone(), d: self.d.clone() }
    }
}

impl Landscape {
    pub fn is_flat(&self) -> bool {
        self.params.preset == LandscapePreset::Flat && self.rivers.is_empty() && self.lakes.is_empty() && self.sea.is_none()
    }

    /// Height of the land with no water carved into it.
    pub fn base_height(&self, p: Vec2) -> f32 {
        let seed = self.params.seed;
        let r = p.length();
        let ramp = smoothstep(BASIN_RADIUS, LAND_HALF, r);
        let n = fbm(seed ^ 0x51, p * 0.0025, 4);
        if self.params.preset == LandscapePreset::Flat {
            return ramp * OUTER_HILLS * (0.4 + 0.6 * n);
        }
        let relief = self.params.relief;
        let outer = ramp * OUTER_HILLS * (0.4 + 0.6 * n) * (0.45 + relief);
        let hills = fbm(seed ^ 0x77, p * 0.0016, 4);
        let ridges = fbm(seed ^ 0x99, p * 0.0045, 3);
        BANK_HEIGHT + 0.8 + relief * (30.0 * hills * hills + 6.0 * ridges) + outer
    }

    /// Exact test for water at a point.
    pub fn water_at(&self, p: Vec2) -> Option<WaterHit> {
        for (i, r) in self.rivers.iter().enumerate() {
            let (seg, _, d) = r.nearest(p);
            if d <= r.width * 0.5 {
                return Some(WaterHit { kind: r.kind(), index: i, current: r.current, width: r.width, flow: r.flow(seg) });
            }
        }
        for (i, l) in self.lakes.iter().enumerate() {
            if point_in_polygon(p, &l.outline) {
                let size = l.outline.iter().map(|q| q.distance(l.center)).fold(0.0, f32::max) * 2.0;
                return Some(WaterHit { kind: WaterKind::Lake, index: i, current: 0.0, width: size, flow: Vec2::ZERO });
            }
        }
        if let Some(sea) = self.sea_now() {
            if sea.contains(p) {
                return Some(WaterHit { kind: WaterKind::Sea, index: 0, current: 0.4, width: f32::INFINITY, flow: Vec2::ZERO });
            }
        }
        None
    }

    pub fn is_water(&self, p: Vec2) -> bool {
        self.water_at(p).is_some()
    }

    /// The nearest waterline within `max` metres of a point on land.
    pub fn nearest_shore(&self, p: Vec2, max: f32) -> Option<Shore> {
        if self.is_water(p) {
            return None;
        }
        let mut best: Option<(f32, Vec2)> = None;
        let mut consider = |d: f32, q: Vec2| {
            if d <= max && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        };
        for r in &self.rivers {
            let (_, c, d) = r.nearest(p);
            let dir = (p - c).normalize_or(Vec2::X);
            consider(d - r.width * 0.5, c + dir * (r.width * 0.5));
        }
        for l in &self.lakes {
            let n = l.outline.len();
            for i in 0..n {
                let (_, q) = project_on_segment(p, l.outline[i], l.outline[(i + 1) % n]);
                consider(q.distance(p), q);
            }
        }
        if let Some(sea) = self.sea_now() {
            for line in sea.shores() {
                for w in line.windows(2) {
                    let (_, q) = project_on_segment(p, w[0], w[1]);
                    consider(q.distance(p), q);
                }
            }
        }
        let (d, q) = best?;
        let seaward = (q - p).normalize_or(Vec2::X);
        // The water just past the line.
        let probe = q + seaward * 2.0;
        let water = self.water_at(probe).or_else(|| self.water_at(q + seaward * 6.0))?;
        Some(Shore { point: q, seaward, distance: d, water })
    }

    pub(super) fn raster(&self) -> &Raster {
        self.raster.get_or_init(|| self.build_raster())
    }

    /// [island-lots] Tests: replace the natural heights with `f(point,
    /// generated height)` (the water stays where it was generated).
    #[cfg(test)]
    pub(crate) fn reshape_for_test(&mut self, f: impl Fn(Vec2, f32) -> f32) {
        let mut r = self.raster().clone();
        let (n, half) = (r.n, r.half);
        for j in 0..n {
            for i in 0..n {
                let p = Vec2::new(-half + i as f32 * CELL, -half + j as f32 * CELL);
                r.h[j * n + i] = f(p, r.h[j * n + i]);
            }
        }
        self.raster = OnceLock::new();
        let _ = self.raster.set(r);
    }

    pub(super) fn build_raster(&self) -> Raster {
        // [r4-tiles] Over the starting map, or with land bought the whole
        // ring round it (the land not yet bought there is as the horizon
        // showed it either way).
        let half = self.land_half();
        let n = (half * 2.0 / CELL) as usize + 1;
        let at = |i: usize, j: usize| Vec2::new(-half + i as f32 * CELL, -half + j as f32 * CELL);
        let mut d = vec![FAR; n * n];
        let cell_range = |lo: f32, hi: f32| -> (usize, usize) {
            let a = (((lo + half) / CELL).floor().max(0.0) as usize).min(n - 1);
            let b = (((hi + half) / CELL).ceil().max(0.0) as usize).min(n - 1);
            (a, b)
        };
        // Rivers: distance to the centreline less the half width.
        for r in &self.rivers {
            let hw = r.width * 0.5;
            for w in r.points.windows(2) {
                let reach = hw + BANK_REACH;
                let (i0, i1) = cell_range(w[0].x.min(w[1].x) - reach, w[0].x.max(w[1].x) + reach);
                let (j0, j1) = cell_range(w[0].y.min(w[1].y) - reach, w[0].y.max(w[1].y) + reach);
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let p = at(i, j);
                        let s = project_on_segment(p, w[0], w[1]).1.distance(p) - hw;
                        let k = j * n + i;
                        if s < d[k] {
                            d[k] = s;
                        }
                    }
                }
            }
        }
        // Lakes and the sea: distance to the outline, negative inside.
        let shore_d = |ring: &[Vec2], closed: bool, inside: &dyn Fn(Vec2) -> bool, d: &mut Vec<f32>| {
            let m = ring.len();
            let segs = if closed { m } else { m.saturating_sub(1) };
            let mut u = vec![FAR; n * n];
            let mut touched: Vec<usize> = Vec::new();
            for s in 0..segs {
                let (a, b) = (ring[s], ring[(s + 1) % m]);
                let (i0, i1) = cell_range(a.x.min(b.x) - BANK_REACH, a.x.max(b.x) + BANK_REACH);
                let (j0, j1) = cell_range(a.y.min(b.y) - BANK_REACH, a.y.max(b.y) + BANK_REACH);
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let p = at(i, j);
                        let k = j * n + i;
                        let dist = project_on_segment(p, a, b).1.distance(p);
                        if dist < u[k] {
                            if u[k] >= FAR {
                                touched.push(k);
                            }
                            u[k] = dist;
                        }
                    }
                }
            }
            for k in touched {
                let p = at(k % n, k / n);
                let s = if inside(p) { -u[k] } else { u[k] };
                if s < d[k] {
                    d[k] = s;
                }
            }
        };
        for l in &self.lakes {
            let outline = l.outline.clone();
            shore_d(&l.outline, true, &|p| point_in_polygon(p, &outline), &mut d);
            // Deep inside a big lake, beyond the reach of its outline.
            for (k, v) in d.iter_mut().enumerate() {
                if *v >= FAR && point_in_polygon(at(k % n, k / n), &l.outline) {
                    *v = -FAR;
                }
            }
        }
        if let Some(sea) = self.sea_now() {
            for line in sea.shores() {
                let closed = false;
                shore_d(&line, closed, &|p| sea.contains(p), &mut d);
            }
            for (k, v) in d.iter_mut().enumerate() {
                if *v >= FAR && sea.contains(at(k % n, k / n)) {
                    *v = -FAR;
                }
            }
        }
        let mut h = vec![0.0; n * n];
        let grown = self.is_grown();
        for j in 0..n {
            for i in 0..n {
                let k = j * n + i;
                let p = at(i, j);
                // [r4-tiles] Past the starting map, the ground the far
                // landscape drew there.
                h[k] = if grown && chebyshev(p) > LAND_HALF { self.grown_height(p) } else { self.carve(self.base_height(p), d[k]) };
            }
        }
        Raster { half, n, h, d }
    }

    /// Land height given the base height and the signed distance to water.
    /// [horizon] Public so the far landscape past the map (`nd_procgen::horizon`)
    /// carves its banks exactly as the map's own are.
    pub fn carve(&self, base: f32, d: f32) -> f32 {
        if d >= FAR {
            return base;
        }
        if d < 0.0 {
            return WATER_LEVEL - 1.2 - (-d).min(10.0) * 0.35;
        }
        // A flat strip along the water, then a slope no steeper than about
        // one in ten up to the hills.
        let rise = (base - BANK_HEIGHT).max(0.0);
        let reach = (50.0 + 3.0 * rise).clamp(50.0, BANK_REACH - 20.0);
        BANK_HEIGHT + rise * smoothstep(18.0, 18.0 + reach, d)
    }

    pub(super) fn sample(&self, grid: impl Fn(&Raster) -> &Vec<f32>, p: Vec2) -> f32 {
        let r = self.raster();
        let n = r.n;
        let fx = ((p.x + r.half) / CELL).clamp(0.0, (n - 1) as f32 - 1e-3);
        let fy = ((p.y + r.half) / CELL).clamp(0.0, (n - 1) as f32 - 1e-3);
        let (i, j) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let g = grid(r);
        let a = g[j * n + i];
        let b = g[j * n + i + 1];
        let c = g[(j + 1) * n + i];
        let d = g[(j + 1) * n + i + 1];
        let top = a + (b - a) * tx;
        let bot = c + (d - c) * tx;
        top + (bot - top) * ty
    }

    /// Natural ground height (bed height under water).
    pub fn height(&self, p: Vec2) -> f32 {
        if self.is_flat() {
            return self.base_height(p);
        }
        let half = self.land_half();
        if p.x.abs() > half || p.y.abs() > half {
            // [r4-tiles] With land bought, past it is the far land as drawn.
            if self.is_grown() {
                return self.grown_height(p);
            }
            return self.carve(self.base_height(p), self.far_water_distance(p));
        }
        self.sample(|r| &r.h, p)
    }

    /// Signed distance to water beyond the raster (the rivers carried off
    /// the map, the open sea), capped like the raster's. [r4-tiles] Always
    /// the generated sea (the ground the far landscape is drawn on).
    pub(super) fn far_water_distance(&self, p: Vec2) -> f32 {
        let mut d = FAR;
        for r in &self.rivers {
            let hw = r.width * 0.5;
            let reach = hw + BANK_REACH;
            for w in r.points.windows(2) {
                if p.x < w[0].x.min(w[1].x) - reach || p.x > w[0].x.max(w[1].x) + reach || p.y < w[0].y.min(w[1].y) - reach || p.y > w[0].y.max(w[1].y) + reach {
                    continue;
                }
                d = d.min(project_on_segment(p, w[0], w[1]).1.distance(p) - hw);
            }
        }
        if let Some(sea) = &self.sea {
            let mut u = FAR;
            let rings = std::iter::once(&sea.outline).chain(sea.islands.iter());
            for ring in rings {
                let m = ring.len();
                for k in 0..m {
                    let (a, b) = (ring[k], ring[(k + 1) % m]);
                    if p.x < a.x.min(b.x) - BANK_REACH || p.x > a.x.max(b.x) + BANK_REACH || p.y < a.y.min(b.y) - BANK_REACH || p.y > a.y.max(b.y) + BANK_REACH {
                        continue;
                    }
                    u = u.min(project_on_segment(p, a, b).1.distance(p));
                }
            }
            let s = if sea.contains(p) { -u } else { u };
            d = d.min(s);
        }
        d
    }

    /// Signed distance to the nearest water, negative in the water, capped
    /// at a couple of hundred metres. Read from the raster.
    pub fn water_distance(&self, p: Vec2) -> f32 {
        if self.is_flat() {
            return FAR;
        }
        self.sample(|r| &r.d, p)
    }

    /// [island-lots] Why a lot (or parcel) with outline `polygon` and
    /// inner point `center` cannot be built on, in plain words, or None if
    /// it can be levelled: standing in the water, a cliff steeper than
    /// [`MAX_GRADED_SLOPE`], or more than [`MAX_LOT_RELIEF`] of rise across
    /// it to cut and fill. Ordinary hillsides pass: their lots are graded.
    pub fn lot_ground_problem(&self, polygon: &[Vec2], center: Vec2) -> Option<&'static str> {
        match self.lot_ground(polygon, center) {
            LotGround::Problem(why) => Some(why),
            _ => None,
        }
    }

    /// [hillside] What the ground of a lot is: level enough to grade, a
    /// hillside to terrace (homes, shops and clubs only), or a problem (in
    /// the water, a sheer cliff, too much drop to terrace).
    pub fn lot_ground(&self, polygon: &[Vec2], center: Vec2) -> LotGround {
        if self.is_flat() {
            return LotGround::Level;
        }
        if self.is_water(center) {
            return LotGround::Problem("under water");
        }
        let n = polygon.len();
        let mut pts: Vec<Vec2> = Vec::with_capacity(n * 2 + 1);
        for k in 0..n {
            pts.push(polygon[k]);
            pts.push(polygon[k].lerp(polygon[(k + 1) % n], 0.5));
        }
        pts.push(center);
        let slope = self.slope_over(&pts);
        if slope > MAX_TERRACE_SLOPE {
            return LotGround::Problem("a sheer cliff: too steep even to terrace");
        }
        let (lo, hi) = pts.iter().map(|p| self.height(*p)).fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(h), hi.max(h)));
        if hi - lo > MAX_TERRACE_RELIEF {
            return LotGround::Problem("the hillside drops too far across this ground to terrace it");
        }
        if slope > MAX_GRADED_SLOPE || hi - lo > MAX_LOT_RELIEF {
            return LotGround::Hillside;
        }
        LotGround::Level
    }

    /// [r4-hillside] How steep a lot's ground is and what that allows, in
    /// plain words with the numbers (for the Zone tool's hover and the
    /// API), or None on ground level enough to grade: the slope and rise
    /// across it against the limits for graded lots (`MAX_GRADED_SLOPE`,
    /// `MAX_LOT_RELIEF`) and terraces (`MAX_TERRACE_SLOPE`,
    /// `MAX_TERRACE_RELIEF`).
    pub fn lot_ground_detail(&self, polygon: &[Vec2], center: Vec2) -> Option<String> {
        if self.is_flat() || self.is_water(center) {
            return None;
        }
        let n = polygon.len();
        let mut pts: Vec<Vec2> = Vec::with_capacity(n * 2 + 1);
        for k in 0..n {
            pts.push(polygon[k]);
            pts.push(polygon[k].lerp(polygon[(k + 1) % n], 0.5));
        }
        pts.push(center);
        let slope = self.slope_over(&pts);
        let (lo, hi) = pts.iter().map(|p| self.height(*p)).fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(h), hi.max(h)));
        let rise = hi - lo;
        let pc = |x: f32| (x * 100.0).round();
        if slope > MAX_TERRACE_SLOPE {
            Some(format!(
                "the ground here climbs {:.0}% (over {:.0} m across the plot): terraces go up to {:.0}%; a path with stairs still climbs it",
                pc(slope),
                rise,
                pc(MAX_TERRACE_SLOPE)
            ))
        } else if rise > MAX_TERRACE_RELIEF {
            Some(format!(
                "the ground falls {rise:.0} m across this plot ({:.0}%): a terrace takes at most {MAX_TERRACE_RELIEF:.0} m; streets closer together along the slope cut shallower plots that fit",
                pc(slope)
            ))
        } else if slope > MAX_GRADED_SLOPE || rise > MAX_LOT_RELIEF {
            Some(format!(
                "a hillside: the ground climbs {:.0}% ({rise:.0} m across the plot); terraced homes, shops and clubs go up to {:.0}% and {MAX_TERRACE_RELIEF:.0} m, works yards, offices and civic grounds need under {:.0}% and {MAX_LOT_RELIEF:.0} m",
                pc(slope),
                pc(MAX_TERRACE_SLOPE),
                pc(MAX_GRADED_SLOPE)
            ))
        } else {
            None
        }
    }

    /// Steepest rise over run between any two of `pts`.
    pub fn slope_over(&self, pts: &[Vec2]) -> f32 {
        let hs: Vec<f32> = pts.iter().map(|p| self.height(*p)).collect();
        let mut worst: f32 = 0.0;
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                let run = pts[i].distance(pts[j]);
                if run > 4.0 {
                    worst = worst.max((hs[i] - hs[j]).abs() / run);
                }
            }
        }
        worst
    }

    /// The river or stream beside an outline (a lot), within `reach` metres:
    /// (river index, current, nearest point on its bank).
    pub fn river_beside(&self, outline: &[Vec2], reach: f32) -> Option<(usize, f32, Vec2)> {
        let mut best: Option<(f32, usize, Vec2)> = None;
        for (i, r) in self.rivers.iter().enumerate() {
            for p in outline {
                let (_, c, d) = r.nearest(*p);
                let gap = d - r.width * 0.5;
                if gap <= reach && best.is_none_or(|(bd, ..)| gap < bd) {
                    best = Some((gap, i, c));
                }
            }
        }
        best.map(|(_, i, c)| (i, self.rivers[i].current, c))
    }

    /// Is an outline on the sea shore (within `reach` metres of the sea)?
    pub fn on_coast(&self, outline: &[Vec2], reach: f32) -> bool {
        outline.iter().any(|p| self.nearest_shore(*p, reach).is_some_and(|s| s.water.kind == WaterKind::Sea))
    }

    /// The hot spot under a point, if any.
    pub fn hot_spot_at(&self, p: Vec2) -> Option<&HotSpot> {
        self.hot_spots.iter().find(|h| h.pos.distance(p) <= h.radius + 25.0)
    }

    /// Water outlines for cutting lots: river strips (in short pieces so a
    /// bend never folds over), lakes, and the sea with its islands as holes.
    pub fn water_polygons(&self) -> Vec<(Vec<Vec2>, Vec<Vec<Vec2>>)> {
        let mut out = Vec::new();
        for r in &self.rivers {
            let hw = r.width * 0.5;
            let n = r.points.len();
            let mut start = 0;
            while start + 1 < n {
                let end = (start + 6).min(n - 1);
                let piece = &r.points[start..=end];
                let left = offset_polyline(piece, hw + 0.5);
                let mut right = offset_polyline(piece, -hw - 0.5);
                right.reverse();
                let mut ring = left;
                ring.extend(right);
                out.push((ring, Vec::new()));
                // Round the joint so consecutive pieces overlap.
                let joint = piece[piece.len() - 1];
                out.push(((0..12).map(|k| joint + Vec2::from_angle(k as f32 / 12.0 * std::f32::consts::TAU) * (hw + 0.5)).collect(), Vec::new()));
                start = end;
            }
        }
        for l in &self.lakes {
            out.push((l.outline.clone(), Vec::new()));
        }
        if let Some(sea) = self.sea_now() {
            out.push((sea.outline.clone(), sea.islands.clone()));
        }
        out
    }
}

/// [hillside] The ground of a lot (`Landscape::lot_ground`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LotGround {
    /// Gentle enough to level the whole lot to its street.
    Level,
    /// Steep: terraced, for homes, shops and clubs.
    Hillside,
    /// Not to be built on, and why.
    Problem(&'static str),
}
