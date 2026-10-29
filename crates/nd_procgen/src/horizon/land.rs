//! [horizon] The land past the map: the same ground, coast and sea the map
//! is made of, carried on out to the horizon.
//!
//! [`FarLand`] is a pure function of the saved landscape (its seed,
//! parameters and generated features) and world coordinates: the same map
//! answers the same height at the same point every time, wherever it is
//! asked from and in whatever order. Nothing here is simulated; the
//! renderer draws it, and a later step (land unlocked next to the city)
//! can generate new ground chunk by chunk from the same function.
//!
//! Past the map the sea and the rivers no longer end at the generated
//! square: a coast runs on east and west (only ever further from the
//! railway, never closer), rivers carried off the map run on, far islands
//! rise out of the open sea well beyond where ships leave, and mountain
//! ranges stand along the horizon. Close to the map the far land is the
//! map's own natural ground: [`FarLand::extra`] is zero along its edge, so
//! the far ground grows out of the playable ground with no step.

use glam::Vec2;
use nd_sim::geom::{point_in_polygon, project_on_segment, smoothstep};
use nd_sim::landscape::{fbm, Climate, Landscape, SeaExit, LAND_HALF};

/// Half the side of the square the far land is drawn out to.
pub const HORIZON: f32 = 20480.0;
/// Signed distances to water are capped here, as the map's raster caps
/// them (its bank reach and a metre): past it the banks have levelled out.
pub const FAR_D: f32 = 161.0;
/// From the map's edge, the far land takes over over this many metres.
pub const SEAM_BLEND: f32 = 400.0;
/// Spacing of the extended coastline's points: the generator's own.
const COAST_STEP: f32 = 25.0;
/// Far islands only rise past this distance from the centre: beyond
/// where ships leave the map (`landscape::WAY_REACH`), so none sails
/// through one.
const ISLES_FROM: f32 = 5600.0;
/// Mountain ranges start this far out.
const RANGES_FROM: f32 = 5500.0;
const RANGES_FULL: f32 = 11000.0;

fn chebyshev(p: Vec2) -> f32 {
    p.x.abs().max(p.y.abs())
}

/// The coast as `y(x)` sampled every [`COAST_STEP`] metres from `x0`: the
/// generated coastline where there is one, carried on past its ends.
struct Coast {
    x0: f32,
    ys: Vec<f32>,
}

impl Coast {
    fn y_at(&self, x: f32) -> f32 {
        let f = ((x - self.x0) / COAST_STEP).clamp(0.0, (self.ys.len() - 1) as f32 - 1e-3);
        let i = f.floor() as usize;
        let t = f - i as f32;
        self.ys[i] + (self.ys[i + 1] - self.ys[i]) * t
    }

    fn point(&self, i: usize) -> Vec2 {
        Vec2::new(self.x0 + i as f32 * COAST_STEP, self.ys[i])
    }

    /// Distance from `p` to the coastline, if under `cap`.
    fn distance(&self, p: Vec2, cap: f32) -> f32 {
        let n = self.ys.len();
        let lo = (((p.x - cap - self.x0) / COAST_STEP).floor().max(0.0) as usize).min(n - 1);
        let hi = (((p.x + cap - self.x0) / COAST_STEP).ceil().max(0.0) as usize).min(n - 1);
        let mut d = cap;
        for i in lo..hi {
            let (a, b) = (self.point(i), self.point(i + 1));
            if p.y < a.y.min(b.y) - cap || p.y > a.y.max(b.y) + cap {
                continue;
            }
            d = d.min(project_on_segment(p, a, b).1.distance(p));
        }
        d
    }
}

/// The far land of one map. Cheap to make (the coast and rivers are
/// carried on once) and `Sync`, so meshes can sample it from many threads.
pub struct FarLand<'a> {
    land: &'a Landscape,
    flat: bool,
    seed: u64,
    coast: Option<Coast>,
    /// Every river's centreline, those leaving the map carried on out past
    /// the horizon, and its half width.
    rivers: Vec<(Vec<Vec2>, f32)>,
    /// Islands far out at sea (the islands map, and off a coast).
    far_isles: bool,
    /// Height of the tallest mountain ranges.
    ranges: f32,
}

impl<'a> FarLand<'a> {
    pub fn new(land: &'a Landscape) -> Self {
        let params = &land.params;
        let flat = land.is_flat();
        let seed = params.seed ^ 0x40_12_20_E5;
        let coast = land.sea.as_ref().filter(|s| s.outward == SeaExit::South && s.coast.len() >= 2).map(|s| extend_coast(&s.coast, seed));
        let rivers = land.rivers.iter().map(|r| (extend_river(&r.points), r.width * 0.5)).collect();
        let far_isles = land.sea.is_some();
        let ranges = match params.climate {
            Climate::Arctic => 950.0,
            Climate::Volcanic => 850.0,
            Climate::Temperate => 650.0,
            Climate::Tropical => 520.0,
            Climate::Desert => 480.0,
        } * (0.6 + 0.8 * params.relief.clamp(0.0, 1.0));
        Self { land, flat, seed, coast, rivers, far_isles, ranges }
    }

    pub fn landscape(&self) -> &Landscape {
        self.land
    }

    /// How much of the far land shows at `p` (0 on the map and at its
    /// edge, 1 from [`SEAM_BLEND`] beyond).
    pub fn blend(p: Vec2) -> f32 {
        smoothstep(LAND_HALF, LAND_HALF + SEAM_BLEND, chebyshev(p))
    }

    /// The mountain ranges standing along the horizon: none near the map,
    /// ridges of `ranges` metres far out, in a few broad massifs.
    pub fn mountains(&self, p: Vec2) -> f32 {
        let t = smoothstep(RANGES_FROM, RANGES_FULL, p.length());
        if t <= 0.0 {
            return 0.0;
        }
        let n = fbm(self.seed ^ 0x4D0A, p * 0.00022, 5);
        let ridge = 1.0 - (2.0 * n - 1.0).abs();
        let mass = smoothstep(0.38, 0.62, fbm(self.seed ^ 0x4D0B, p * 0.00008, 3));
        t * self.ranges * ridge * ridge * (0.25 + 0.75 * mass)
    }

    /// Islands rising out of the open sea far from the map: positive on
    /// one (metres from its shore, roughly), negative in the sea.
    fn isle_distance(&self, p: Vec2) -> f32 {
        let r = p.length();
        if !self.far_isles || r < ISLES_FROM {
            return -FAR_D;
        }
        let thr = 1.3 - 0.68 * smoothstep(ISLES_FROM, ISLES_FROM + 3500.0, r);
        ((fbm(self.seed ^ 0x15_1A, p * 0.00042, 4) - thr) * 3000.0).clamp(-FAR_D, FAR_D)
    }

    /// Signed distance to the nearest water, negative in it, capped at
    /// [`FAR_D`]: the rivers carried on, the coast carried on, the sea
    /// open past the map (the map's own square edge of sea is gone).
    pub fn water_distance(&self, p: Vec2) -> f32 {
        if self.flat {
            return FAR_D;
        }
        let mut d = FAR_D;
        for (pts, hw) in &self.rivers {
            let reach = hw + FAR_D;
            for w in pts.windows(2) {
                if p.x < w[0].x.min(w[1].x) - reach || p.x > w[0].x.max(w[1].x) + reach || p.y < w[0].y.min(w[1].y) - reach || p.y > w[0].y.max(w[1].y) + reach {
                    continue;
                }
                d = d.min(project_on_segment(p, w[0], w[1]).1.distance(p) - hw);
            }
        }
        if let Some(sea) = &self.land.sea {
            let mut u = FAR_D;
            let mut on_isle = false;
            for ring in &sea.islands {
                let m = ring.len();
                for k in 0..m {
                    let (a, b) = (ring[k], ring[(k + 1) % m]);
                    if p.x < a.x.min(b.x) - FAR_D || p.x > a.x.max(b.x) + FAR_D || p.y < a.y.min(b.y) - FAR_D || p.y > a.y.max(b.y) + FAR_D {
                        continue;
                    }
                    u = u.min(project_on_segment(p, a, b).1.distance(p));
                }
                if !on_isle && point_in_polygon(p, ring) {
                    on_isle = true;
                }
            }
            let wet = match &self.coast {
                Some(c) => {
                    u = u.min(c.distance(p, FAR_D));
                    p.y < c.y_at(p.x) && !on_isle
                }
                // The sea all round (islands).
                None => !on_isle,
            };
            let s = if wet { (-u).max(self.isle_distance(p)) } else { u };
            d = d.min(s);
        }
        d.clamp(-FAR_D, FAR_D)
    }

    /// Is `p` in the sea (not a river or stream)?
    pub fn is_sea(&self, p: Vec2) -> bool {
        let Some(sea) = &self.land.sea else { return false };
        let on_isle = sea.islands.iter().any(|ring| point_in_polygon(p, ring));
        let open = match &self.coast {
            Some(c) => p.y < c.y_at(p.x),
            None => true,
        };
        open && !on_isle && self.isle_distance(p) < 0.0
    }

    pub fn is_water(&self, p: Vec2) -> bool {
        self.water_distance(p) < 0.0
    }

    /// The far land's natural height at `p`: the map's own hills (its
    /// `base_height`), the mountain ranges, and the banks and beds of the
    /// far water carved in as the map carves its own. For the flat map
    /// (whose ground is drawn by another generator) only the ranges.
    pub fn natural(&self, p: Vec2) -> f32 {
        if self.flat {
            return self.mountains(p);
        }
        self.land.carve(self.land.base_height(p) + self.mountains(p), self.water_distance(p))
    }

    /// What the far land adds to the map's own natural ground at `p`
    /// (`natural` less the landscape's `height`), weighted by [`blend`]:
    /// exactly zero on the map and along its edge. The far ground is the
    /// map's ground (graded to the ways running out) plus this.
    ///
    /// [`blend`]: Self::blend
    pub fn extra(&self, p: Vec2) -> f32 {
        let w = Self::blend(p);
        if w <= 0.0 {
            return 0.0;
        }
        if self.flat {
            return w * self.mountains(p);
        }
        w * (self.natural(p) - self.land.height(p))
    }

    /// The signed water distance the far ground is coloured by, blended
    /// from the map's (read at its edge) out to the far land's.
    pub fn colour_distance(&self, p: Vec2) -> f32 {
        if self.flat {
            return FAR_D;
        }
        let w = Self::blend(p);
        let far = self.water_distance(p);
        if w >= 1.0 {
            return far;
        }
        let near = self.land.water_distance(p);
        near + (far - near) * w
    }

    /// Is `p` dry, fairly level land a town could stand on?
    pub fn town_ground(&self, p: Vec2) -> bool {
        if self.water_distance(p) < FAR_D - 1.0 {
            return false;
        }
        let h = self.natural(p);
        let slope = [Vec2::X, Vec2::Y].iter().map(|d| (self.natural(p + *d * 120.0) - h).abs() / 120.0).fold(0.0, f32::max);
        slope < 0.18 && self.mountains(p) < 120.0
    }
}

/// The generated coastline carried on past both ends: bays and headlands
/// out of seeded noise, only ever south of the coast's northernmost point
/// (the railway and the highway run north of it), joining the generated
/// line smoothly.
fn extend_coast(coast: &[Vec2], seed: u64) -> Coast {
    let first = coast[0];
    let last = coast[coast.len() - 1];
    let north = coast.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
    let reach = HORIZON * 1.25;
    let x0 = first.x - ((first.x + reach) / COAST_STEP).ceil() * COAST_STEP;
    let n = ((reach - x0) / COAST_STEP).ceil() as usize + 1;
    let gen_at = |x: f32| -> Option<f32> {
        if x < first.x || x > last.x {
            return None;
        }
        // The generated line, linearly between its points.
        let k = coast.windows(2).position(|w| x >= w[0].x && x <= w[1].x)?;
        let (a, b) = (coast[k], coast[k + 1]);
        let t = if (b.x - a.x).abs() < 1e-3 { 0.0 } else { (x - a.x) / (b.x - a.x) };
        Some(a.y + (b.y - a.y) * t)
    };
    let ys = (0..n)
        .map(|i| {
            let x = x0 + i as f32 * COAST_STEP;
            if let Some(y) = gen_at(x) {
                return y;
            }
            let (end, from) = if x < first.x { (first, first.x - x) } else { (last, x - last.x) };
            let ramp = smoothstep(0.0, 1800.0, from);
            let bays = fbm(seed ^ 0xC0A5, Vec2::new(x * 0.00035, 3.7), 4);
            let wiggle = fbm(seed ^ 0xC0A6, Vec2::new(x * 0.004, 9.1), 2) - 0.5;
            let y = end.y - ramp * (2600.0 * smoothstep(0.3, 0.75, bays) + 140.0 * bays) + ramp * 70.0 * wiggle;
            y.min(north)
        })
        .collect();
    Coast { x0, ys }
}

/// A river's centreline with any end that leaves the map carried on
/// straight out past the horizon.
fn extend_river(points: &[Vec2]) -> Vec<Vec2> {
    let mut pts = points.to_vec();
    if pts.len() < 2 {
        return pts;
    }
    let out = |end: Vec2, prev: Vec2| -> Option<Vec2> {
        if chebyshev(end) < LAND_HALF + 200.0 {
            return None;
        }
        let d = (end - prev).normalize_or_zero();
        (d != Vec2::ZERO).then(|| end + d * (HORIZON * 1.5))
    };
    if let Some(p) = out(pts[pts.len() - 1], pts[pts.len() - 2]) {
        pts.push(p);
    }
    if let Some(p) = out(pts[0], pts[1]) {
        pts.insert(0, p);
    }
    pts
}
