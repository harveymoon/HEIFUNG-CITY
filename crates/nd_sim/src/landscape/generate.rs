//! Generating a landscape from its parameters: the main river and its
//! tributaries, lakes, the sea or islands, hot spots and climate extras.

use super::*;

/// A wobbly closed outline around `c`.
pub(super) fn blob(rng: &mut impl Rng, c: Vec2, r: f32, wobble: f32, n: usize) -> Vec<Vec2> {
    let (p1, p2): (f32, f32) = (rng.random_range(0.0..6.28), rng.random_range(0.0..6.28));
    let (k1, k2) = (rng.random_range(2..4) as f32, rng.random_range(4..7) as f32);
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let rr = r * (1.0 + wobble * (0.65 * (a * k1 + p1).sin() + 0.35 * (a * k2 + p2).sin()));
            c + Vec2::new(a.cos(), a.sin()) * rr
        })
        .collect()
}

/// Keep the stretch of a polyline inside the square of half-side `half`,
/// starting at the first point inside.
pub(super) fn clip_to_box(pts: &[Vec2], half: f32) -> Vec<Vec2> {
    let inside = |p: &Vec2| p.x.abs() <= half && p.y.abs() <= half;
    let start = pts.iter().position(inside).unwrap_or(0);
    let mut out: Vec<Vec2> = Vec::new();
    if start > 0 {
        out.push(pts[start - 1]);
    }
    for p in &pts[start..] {
        out.push(*p);
        if !inside(p) {
            break;
        }
    }
    out
}

/// A meandering line along `dir` at `offset` across it, from `s0` to `s1`.
pub(super) fn meander(rng: &mut impl Rng, dir: Vec2, offset: f32, s0: f32, s1: f32, amp: f32) -> Vec<Vec2> {
    let across = Vec2::new(-dir.y, dir.x);
    let (p1, p2): (f32, f32) = (rng.random_range(0.0..6.28), rng.random_range(0.0..6.28));
    let (k1, k2) = (std::f32::consts::TAU / rng.random_range(800.0..1100.0), std::f32::consts::TAU / rng.random_range(320.0..420.0));
    let mut out = Vec::new();
    let mut s = s0;
    while s <= s1 {
        let lat = offset + amp * (k1 * s + p1).sin() + amp * 0.22 * (k2 * s + p2).sin();
        out.push(dir * s + across * lat);
        s += 20.0;
    }
    out
}

/// Distance from `p` to a polyline.
pub(super) fn polyline_distance(pts: &[Vec2], p: Vec2) -> f32 {
    crate::geom::distance_to_polyline(p, pts)
}

/// What one try at a tributary gave.
pub(super) enum Branch {
    /// The main river is too short for tributaries.
    Stop,
    /// Too short to keep.
    Skip,
    Grown(River),
}

impl Landscape {
    /// Generate a landscape from its parameters. Deterministic.
    pub fn generate(params: &LandscapeParams) -> Self {
        let params = params.clamped();
        let mut l = Landscape { params: params.clone(), ..Landscape::default() };
        if params.preset == LandscapePreset::Flat {
            return l;
        }
        let mut rng = crate::rng_for(params.seed, 0x1A4D_5CA9);
        let water = params.water;
        let coast_y = -(1020.0 - water * 360.0);
        match params.preset {
            LandscapePreset::Coast | LandscapePreset::Delta => {
                l.sea = Some(Self::coast_sea(&mut rng, coast_y));
            }
            LandscapePreset::Islands => {
                l.sea = Some(Self::island_sea(&mut rng, water));
            }
            _ => {}
        }
        // The main river.
        let wants_river = match params.preset {
            LandscapePreset::Islands => false,
            LandscapePreset::Coast => water > 0.2,
            _ => true,
        };
        if wants_river {
            let width = match params.preset {
                LandscapePreset::Plains => 24.0 + water * 30.0,
                _ => 34.0 + water * 46.0,
            };
            let current = 0.55 + params.relief * 1.2 + rng.random_range(0.0..0.3);
            let angle: f32 = rng.random_range(-0.22..0.22);
            let dir = Vec2::new(angle.sin(), -angle.cos());
            let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
            let amp = rng.random_range(90.0..130.0);
            let offset = side * rng.random_range(430.0..600.0);
            let mut pts = meander(&mut rng, dir, offset, -2700.0, 2700.0, amp);
            // Keep the middle dry: push the river out until it clears.
            for _ in 0..8 {
                let near = pts.iter().map(|p| p.length()).fold(f32::INFINITY, f32::min);
                if near >= CLEAR_RADIUS + width * 0.5 + 40.0 {
                    break;
                }

                let shift = Vec2::new(-dir.y, dir.x) * side * 60.0;
                for p in pts.iter_mut() {
                    *p += shift;
                }
            }
            let mut pts = clip_to_box(&pts, LAND_HALF + 60.0);
            if let Some(sea) = &l.sea {
                // Runs into the sea and a little beyond the shore.
                if let Some(i) = pts.iter().position(|p| sea.contains(*p)) {
                    pts.truncate((i + 3).min(pts.len()));
                }
            }
            if params.preset == LandscapePreset::Delta && pts.len() > 10 {
                // Split some way above the coast into channels fanning out.
                let split = pts.iter().position(|p| p.y < coast_y + 330.0).unwrap_or(pts.len() / 2).max(2);
                let at = pts[split];
                let main_dir = (pts[split] - pts[split - 1]).normalize_or(Vec2::NEG_Y);
                pts.truncate(split + 1);
                let channels = 2 + (params.branching * 3.0).round() as usize;
                for k in 0..channels {
                    let spread = (k as f32 / (channels - 1).max(1) as f32 - 0.5) * 1.3;
                    let mut d = Vec2::from_angle(spread).rotate(main_dir);
                    let mut p = at;
                    let mut ch = vec![p];
                    for _ in 0..80 {
                        d = Vec2::from_angle(rng.random_range(-0.12..0.12)).rotate(d).normalize();
                        // Pulled back towards the sea.
                        d = (d + main_dir * 0.08).normalize();
                        p += d * 20.0;
                        ch.push(p);
                        if l.sea.as_ref().is_some_and(|s| s.contains(p)) && ch.len() > 4 {
                            p += d * 40.0;
                            ch.push(p);
                            break;
                        }
                    }
                    l.rivers.push(River { points: ch, width: width * 0.55, current, stream: false });
                }
            }
            extend_off_map(&mut pts);
            l.rivers.insert(0, River { points: pts, width, current, stream: false });
        }
        // Tributaries and streams.
        let tributaries = (params.branching * 4.0).round() as usize;
        if !l.rivers.is_empty() {
            for _ in 0..tributaries {
                match l.grow_tributary(&mut rng) {
                    Branch::Stop => break,
                    Branch::Skip => continue,
                    Branch::Grown(r) => l.rivers.push(r),
                }
            }
        }
        // A stream on the big island.
        if params.preset == LandscapePreset::Islands && params.branching > 0.1 {
            let a: f32 = rng.random_range(0.0..6.28);
            let mut p = Vec2::from_angle(a) * (CLEAR_RADIUS + 120.0);
            let mut d = Vec2::from_angle(a);
            let mut line = vec![p];
            for _ in 0..80 {
                d = Vec2::from_angle(rng.random_range(-0.15..0.15)).rotate(d).normalize();
                p += d * 20.0;
                line.push(p);
                if l.sea.as_ref().is_some_and(|s| s.contains(p)) {
                    break;
                }
            }
            l.rivers.push(River { points: line, width: 12.0 + water * 6.0, current: 1.4 + params.relief * 2.0, stream: true });
        }
        // Lakes.
        let lakes = match params.preset {
            LandscapePreset::RiverValley | LandscapePreset::Plains => (water * 3.0).round() as usize,
            LandscapePreset::Coast => (water * 1.5).round() as usize,
            _ => 0,
        };
        l.add_lakes(&mut rng, lakes);
        // Hot spots.
        let spots = 1 + rng.random_range(0..2) + (params.relief > 0.5) as usize;
        l.add_hot_spots(&mut rng, spots, 280.0..880.0, 300.0, 0.7..1.0);
        l.climate_extras();
        l
    }

    /// One tributary or stream off the main river. It draws exactly what
    /// the generator always drew, so Desert maps (and old saves) are unchanged.
    pub(super) fn grow_tributary(&self, rng: &mut impl Rng) -> Branch {
        let params = &self.params;
        let water = params.water;
        let main = &self.rivers[0];
        let n = main.points.len();
        if n < 20 {
            return Branch::Stop;
        }
        let j = rng.random_range(n / 8..n - n / 8);
        let at = main.points[j];
        let up = -main.flow(j);
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let mut d = Vec2::from_angle(side * rng.random_range(0.7..1.3)).rotate(up);
        let big = params.branching > 0.55 && rng.random_bool(0.3);
        let width = if big { 22.0 + water * 12.0 } else { 9.0 + water * 9.0 + rng.random_range(0.0..4.0) };
        let length = rng.random_range(500.0..1400.0);
        let mut p = at;
        let mut line = vec![p];
        let mut s = 0.0;
        while s < length {
            d = Vec2::from_angle(rng.random_range(-0.16..0.16)).rotate(d).normalize();
            let guard = CLEAR_RADIUS + width * 0.5 + 50.0;
            if (p + d * 20.0).length() < guard {
                // Turn away from the middle of the map.
                let out = p.normalize_or(Vec2::X);
                d = (d + out * 0.9).normalize();
            }
            p += d * 20.0;
            s += 20.0;
            if p.x.abs() > LAND_HALF + 40.0 || p.y.abs() > LAND_HALF + 40.0 || self.sea.as_ref().is_some_and(|sea| sea.contains(p)) {
                break;
            }
            line.push(p);
        }
        if line.len() < 6 {
            return Branch::Skip;
        }
        line.reverse();
        extend_off_map(&mut line);
        let current = if big { 0.9 + params.relief * 1.5 } else { 1.2 + params.relief * 2.2 } + rng.random_range(0.0..0.6);
        Branch::Grown(River { points: line, width, current, stream: !big })
    }

    /// Lakes until there are `want` (or 60 tries), clear of the middle, the
    /// highway, the railway, rivers, other lakes, the sea and hot spots.
    pub(super) fn add_lakes(&mut self, rng: &mut impl Rng, want: usize) {
        let water = self.params.water;
        let mut tries = 0;
        while self.lakes.len() < want && tries < 60 {
            tries += 1;
            let r = rng.random_range(50.0..(80.0 + water * 110.0));
            let c = Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(380.0..1150.0);
            if c.length() - r < CLEAR_RADIUS + 40.0 || (c.y - HIGHWAY_Y).abs() < r + 60.0 || (c.y - crate::rail::TRUNK_Y).abs() < r + 50.0 {
                continue;
            }
            if self.rivers.iter().any(|rv| polyline_distance(&rv.points, c) < r * 1.25 + rv.width + 40.0) || self.lakes.iter().any(|o| o.center.distance(c) < r * 2.5 + 120.0) || self.sea.as_ref().is_some_and(|s| s.contains(c) || polyline_distance(&s.coast, c) < r * 1.3 + 60.0) {
                continue;
            }
            // Only a climate's extra lakes can meet a hot spot: the
            // generator's own are placed before there are any.
            if self.hot_spots.iter().any(|h| h.pos.distance(c) < r + h.radius + 60.0) {
                continue;
            }
            let outline = blob(rng, c, r, 0.2, 28);
            self.lakes.push(Lake { outline, center: c });
        }
    }

    /// Hot spots until there are `want` (or the tries run out), `spacing`
    /// apart, on dry ground clear of the water, the highway and the railway.
    pub(super) fn add_hot_spots(&mut self, rng: &mut impl Rng, want: usize, ring: std::ops::Range<f32>, spacing: f32, heat: std::ops::Range<f32>) {
        let mut tries = 0;
        let budget = (want * 15).max(60);
        while self.hot_spots.len() < want && tries < budget {
            tries += 1;
            let p = Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(ring.clone());
            if (p.y - HIGHWAY_Y).abs() < 70.0 || (p.y - crate::rail::TRUNK_Y).abs() < 60.0 || self.hot_spots.iter().any(|h| h.pos.distance(p) < spacing) {
                continue;
            }
            if self.water_at(p).is_some() || self.nearest_shore(p, 90.0).is_some() {
                continue;
            }
            self.hot_spots.push(HotSpot { pos: p, radius: rng.random_range(45.0..70.0), heat: rng.random_range(heat.clone()) });
        }
    }

    /// What the climate adds to the generator's map, from its own random
    /// stream so the shared features stay where they were: more streams and
    /// lakes where it is wet, lakes to freeze in the Arctic, a field of hot
    /// ground on a volcanic map. Desert adds nothing: it is the original
    /// generator, the driest of them.
    pub(super) fn climate_extras(&mut self) {
        let params = self.params.clone();
        let mut rng = crate::rng_for(params.seed, 0xC11_A7E0 + params.climate as u64);
        let (streams, lakes) = match params.climate {
            Climate::Desert => return,
            Climate::Temperate => (1 + (params.branching * 2.0).round() as usize, 1 + (params.water * 1.5).round() as usize),
            Climate::Tropical => (2 + (params.branching * 2.0).round() as usize, 1 + (params.water * 2.0).round() as usize),
            Climate::Arctic => (0, 2),
            Climate::Volcanic => (1, 1),
        };
        if !self.rivers.is_empty() {
            for _ in 0..streams {
                match self.grow_tributary(&mut rng) {
                    Branch::Stop => break,
                    Branch::Skip => {}
                    Branch::Grown(r) => self.rivers.push(r),
                }
            }
        }
        // Lakes only where the preset has open land for them.
        if !matches!(params.preset, LandscapePreset::Islands | LandscapePreset::Delta) {
            let want = self.lakes.len() + lakes;
            self.add_lakes(&mut rng, want);
        }
        if params.climate == Climate::Volcanic {
            // Iceland: a field of vents and hot ground, several of them strong.
            let want = self.hot_spots.len() + 4 + rng.random_range(0..3usize);
            self.add_hot_spots(&mut rng, want, 260.0..1200.0, 220.0, 0.8..1.0);
        }
    }

    pub(super) fn coast_sea(rng: &mut impl Rng, coast_y: f32) -> Sea {
        let (p1, p2): (f32, f32) = (rng.random_range(0.0..6.28), rng.random_range(0.0..6.28));
        let mut coast = Vec::new();
        let mut x = -LAND_HALF - 300.0;
        while x <= LAND_HALF + 300.0 {
            let y = coast_y + 55.0 * (x * 0.0021 + p1).sin() + 22.0 * (x * 0.0067 + p2).sin();
            coast.push(Vec2::new(x, y));
            x += 25.0;
        }
        let mut outline = coast.clone();
        outline.push(Vec2::new(LAND_HALF + 300.0, -LAND_HALF * 3.0));
        outline.push(Vec2::new(-LAND_HALF - 300.0, -LAND_HALF * 3.0));
        Sea { outline, islands: Vec::new(), coast, outward: SeaExit::South }
    }

    pub(super) fn island_sea(rng: &mut impl Rng, water: f32) -> Sea {
        let big = LAND_HALF * 3.0;
        let outline = vec![Vec2::new(-big, -big), Vec2::new(big, -big), Vec2::new(big, big), Vec2::new(-big, big)];
        let r0 = 1080.0 - water * 380.0;
        let main = blob(rng, Vec2::ZERO, r0, 0.12, 72);
        Sea { outline, islands: vec![main], coast: Vec::new(), outward: SeaExit::Radial }
    }

    /// Small islands out in the sea; called after generation so they never
    /// sit on a river mouth.
    pub(super) fn add_islands(&mut self) {
        let Some(sea) = self.sea.as_ref() else { return };
        // A tropical sea is dotted with more of them.
        let want = self.params.islands as usize + if self.params.climate == Climate::Tropical { 3 } else { 0 };
        if want == 0 {
            return;
        }
        let mut rng = crate::rng_for(self.params.seed, 0x15_1A2D);
        let r0 = sea.islands.first().map(|m| m.iter().map(|p| p.length()).fold(0.0, f32::max)).unwrap_or(0.0);
        let mut made: Vec<(Vec2, f32)> = Vec::new();
        let mut tries = 0;
        while made.len() < want && tries < 200 {
            tries += 1;
            let r = rng.random_range(60.0..200.0);
            let c = match sea.outward {
                SeaExit::Radial => Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(r0 + 140.0 + r..LAND_HALF - r).max(r0 + 140.0 + r),
                SeaExit::South => Vec2::new(rng.random_range(-1500.0..1500.0), rng.random_range(-1700.0..-900.0)),
            };
            let shore_gap = if sea.coast.len() >= 2 { polyline_distance(&sea.coast, c) } else { f32::INFINITY };
            if !sea.contains(c) || shore_gap < r * 1.25 + 80.0 || made.iter().any(|(o, orr)| o.distance(c) < r + orr + 80.0) {
                continue;
            }
            if self.rivers.iter().any(|rv| polyline_distance(&rv.points, c) < r * 1.3 + rv.width + 60.0) {
                continue;
            }
            made.push((c, r));
        }
        let outlines: Vec<Vec<Vec2>> = made.iter().map(|&(c, r)| blob(&mut rng, c, r, 0.18, 32)).collect();
        if let Some(sea) = self.sea.as_mut() {
            sea.islands.extend(outlines);
        }
    }

    /// `generate` plus the offshore islands.
    pub fn new(params: &LandscapeParams) -> Self {
        let mut l = Self::generate(params);
        l.add_islands();
        l
    }
}
