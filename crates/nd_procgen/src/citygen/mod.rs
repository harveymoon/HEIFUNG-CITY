//! Whole example cities from a seed: a street network traced through a
//! tensor field, after Chen et al. 2008 ("Interactive Procedural Street
//! Modeling") and the approach of ProbableTrain's MapGenerator (implemented
//! here from the published method; no code is shared).
//!
//! 1. A tensor field ([`field`]) blends grids, radial centres and the lines
//!    of the land (rivers, the coast, the highway, the railway).
//! 2. Streets are traced along its two eigenvector fields ([`trace`]):
//!    arterials far apart, then collectors, then local streets, each kept
//!    a separation from streets running the same way. The waterfront is
//!    a street of its own, traced along the shore before the rest.
//! 3. Dangling ends are joined to the street ahead, crossings become
//!    junctions and the network is cleaned to the game's rules
//!    ([`graph`]): no short pieces, no sharp junctions, no sliver blocks,
//!    one connected network with a way in from the highway.
//! 4. The blocks between the streets are zoned by a land-use model
//!    ([`landuse`]).
//!
//! Everything is deterministic from the preset, the seed and the
//! landscape; nothing here knows about Bevy. [`apply`] lays a plan out in a
//! [`nd_sim::World`] with the ordinary road and zone gestures.

pub mod apply;
pub mod field;
pub mod graph;
pub mod landuse;
pub mod trace;

use field::{Basis, FieldRaster, TensorField, Twist};
use glam::Vec2;
use nd_sim::landscape::{Landscape, LandscapeParams, LandscapePreset, HIGHWAY_Y};
use nd_sim::LandUse;
use rand::Rng;
use serde::{Deserialize, Serialize};
use trace::{Family, Ground, Level, LevelParams, Line, Site, Tracer, STEP};

pub use apply::{generate_world, lay_city, LaidCity};

/// The northern limit of the city: clear of the highway and its ramps.
pub const NORTH_LIMIT: f32 = HIGHWAY_Y - 80.0;
/// The buildable square's half-side, less a margin.
const HALF: f32 = 870.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CityPreset {
    /// A regular grid round one radial plaza, on open plains.
    GridPlaza,
    /// A port on the sea where a river comes down to it; streets follow the
    /// shore and the river.
    CoastalRiver,
    /// A twisting radial old town in a river valley with modern grids
    /// either side.
    OldTown,
    /// Two radial centres, each with its own grid, grown together.
    TwinCentres,
}

impl CityPreset {
    pub const ALL: [CityPreset; 4] = [CityPreset::GridPlaza, CityPreset::CoastalRiver, CityPreset::OldTown, CityPreset::TwinCentres];

    pub fn key(self) -> &'static str {
        match self {
            CityPreset::GridPlaza => "grid_plaza",
            CityPreset::CoastalRiver => "coastal_river",
            CityPreset::OldTown => "old_town",
            CityPreset::TwinCentres => "twin_centres",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CityPreset::GridPlaza => "Grid and plaza",
            CityPreset::CoastalRiver => "Coastal river town",
            CityPreset::OldTown => "Old town, new grid",
            CityPreset::TwinCentres => "Twin centres",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            CityPreset::GridPlaza => "A regular grid round a radial plaza on open plains.",
            CityPreset::CoastalRiver => "A port where a river meets the sea; streets follow the shore.",
            CityPreset::OldTown => "A twisting old town in a river valley, modern grids either side.",
            CityPreset::TwinCentres => "Two radial centres with their own grids, grown together.",
        }
    }

    /// Parse a key or label ("old_town", "Old town", "oldtown").
    pub fn parse(s: &str) -> Option<CityPreset> {
        let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '_', '-', ','], "");
        let s = norm(s);
        Self::ALL.into_iter().find(|p| norm(p.key()) == s || norm(p.label()) == s)
    }

    /// The landscape the preset's city is laid out on.
    pub fn landscape(self, seed: u64) -> LandscapeParams {
        match self {
            CityPreset::GridPlaza => LandscapeParams::preset(LandscapePreset::Plains, seed),
            CityPreset::CoastalRiver => LandscapeParams { water: 0.8, branching: 0.15, ..LandscapeParams::preset(LandscapePreset::Coast, seed) },
            CityPreset::OldTown => LandscapeParams { relief: 0.25, branching: 0.2, ..LandscapeParams::preset(LandscapePreset::RiverValley, seed) },
            CityPreset::TwinCentres => LandscapeParams { water: 0.2, ..LandscapeParams::preset(LandscapePreset::Plains, seed) },
        }
    }
}

/// Road hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoadClass {
    Arterial,
    Collector,
    Local,
}

impl RoadClass {
    fn of(level: Level) -> Self {
        match level {
            Level::Main => RoadClass::Arterial,
            Level::Major => RoadClass::Collector,
            Level::Minor => RoadClass::Local,
        }
    }

    /// Lanes each way: the Road tool's Street preset for arterials, Lane
    /// for the rest.
    pub fn lanes(self) -> (u8, u8) {
        match self {
            RoadClass::Arterial => (2, 2),
            RoadClass::Collector | RoadClass::Local => (1, 1),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CityRoad {
    /// The street's points: every junction is one; curves are already
    /// sampled about every `nd_sim::plan::CURVE_STEP`, so the game lays it
    /// with straight legs.
    pub points: Vec<Vec2>,
    pub class: RoadClass,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CityZone {
    pub polygon: Vec<Vec2>,
    pub land_use: LandUse,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CityPlan {
    pub preset: CityPreset,
    pub seed: u64,
    pub roads: Vec<CityRoad>,
    /// Painted in order: later zones win where they overlap.
    pub zones: Vec<CityZone>,
    /// Road ends at the north edge of town to join to the highway
    /// (`World::link_to_highway`).
    pub links: Vec<Vec2>,
    /// The town centres.
    pub cores: Vec<Vec2>,
}

impl CityPlan {
    pub fn road_length(&self, class: RoadClass) -> f32 {
        self.roads.iter().filter(|r| r.class == class).map(|r| nd_sim::geom::path_length(&r.points)).sum()
    }
}

/// What a preset puts into the field and the city.
struct Spec {
    centre: Vec2,
    radii: Vec2,
    cores: Vec<(Vec2, f32)>,
    /// Radial centres: the plazas.
    plazas: Vec<Vec2>,
    bases: Vec<Basis>,
    twists: Vec<Twist>,
    /// Works districts: (position, radius).
    industry: Vec<(Vec2, f32)>,
    levels: [LevelParams; 3],
}

fn levels() -> [LevelParams; 3] {
    [
        LevelParams { dsep: [330.0, 330.0], dtest: 150.0, min_len: 320.0, max_len: 4000.0, tries: 40 },
        LevelParams { dsep: [165.0, 165.0], dtest: 70.0, min_len: 140.0, max_len: 3000.0, tries: 80 },
        LevelParams { dsep: [64.0, 108.0], dtest: 28.0, min_len: 50.0, max_len: 1400.0, tries: 260 },
    ]
}

/// `p` moved away from the water until it is `clear` metres from it (a
/// radial centre on a riverbank makes a tangle).
fn dry(land: &Landscape, mut p: Vec2, clear: f32) -> Vec2 {
    if land.is_flat() {
        return p;
    }
    for _ in 0..40 {
        let d = land.water_distance(p);
        if d >= clear {
            break;
        }
        let e = 10.0;
        let g = Vec2::new(land.water_distance(p + Vec2::X * e) - land.water_distance(p - Vec2::X * e), land.water_distance(p + Vec2::Y * e) - land.water_distance(p - Vec2::Y * e));
        let step = g.normalize_or(-p.normalize_or(Vec2::X)) * 20.0;
        p = (p + step).clamp(Vec2::splat(-HALF + 150.0), Vec2::new(HALF - 150.0, NORTH_LIMIT - 150.0));
    }
    p
}

fn spec(preset: CityPreset, land: &Landscape, rng: &mut impl Rng) -> Spec {
    let mut j = |r: f32| Vec2::new(rng.random_range(-r..r), rng.random_range(-r..r));
    let lv = levels();
    match preset {
        CityPreset::GridPlaza => {
            let centre = Vec2::new(0.0, -120.0);
            let plaza = dry(land, centre + j(70.0), 220.0);
            let theta = j(0.18).x;
            let works = Vec2::new(if j(1.0).x > 0.0 { 620.0 } else { -620.0 }, -380.0);
            Spec {
                centre,
                radii: Vec2::new(820.0, 640.0),
                cores: vec![(plaza, 360.0)],
                plazas: vec![plaza],
                bases: vec![Basis::Grid { centre, size: 4000.0, theta, weight: 1.0 }, Basis::Radial { centre: plaza, size: 170.0, weight: 2.4 }],
                twists: Vec::new(),
                industry: vec![(works, 260.0)],
                levels: lv,
            }
        }
        CityPreset::CoastalRiver => {
            let centre = Vec2::new(0.0, -240.0);
            // The port: where the river reaches the sea, nearest the middle.
            let mouth = land.rivers.first().and_then(|r| r.points.iter().copied().filter(|p| p.x.abs() < HALF && p.y < -300.0).find(|p| land.water_distance(*p + Vec2::new(0.0, -60.0)) < -5.0 || land.sea.as_ref().is_some_and(|s| s.contains(*p + Vec2::new(0.0, -80.0)))));
            let port = mouth.map(|m| Vec2::new(m.x - m.x.signum() * 170.0, m.y + 120.0)).unwrap_or(Vec2::new(0.0, -560.0));
            let plaza = dry(land, Vec2::new(port.x * 0.4, -180.0) + j(60.0), 220.0);
            let works = mouth.map(|m| Vec2::new(m.x + m.x.signum() * 240.0, m.y + 90.0)).unwrap_or(Vec2::new(600.0, -600.0));
            Spec {
                centre,
                radii: Vec2::new(850.0, 640.0),
                cores: vec![(port, 300.0), (plaza, 260.0)],
                plazas: vec![plaza],
                bases: vec![Basis::Grid { centre, size: 3000.0, theta: j(0.1).x, weight: 0.7 }, Basis::Radial { centre: plaza, size: 150.0, weight: 2.0 }],
                twists: vec![Twist { centre: port, size: 260.0, angle: 0.25, wavelength: 260.0 }],
                industry: vec![(works, 240.0)],
                levels: lv,
            }
        }
        CityPreset::OldTown => {
            let centre = Vec2::new(0.0, -110.0);
            let old = dry(land, centre + j(60.0), 250.0);
            let west = Vec2::new(-560.0, -140.0) + j(60.0);
            let east = Vec2::new(560.0, -90.0) + j(60.0);
            let (tw, te) = (0.35 + j(0.15).x, -0.4 + j(0.15).x);
            let works = Vec2::new(if j(1.0).x > 0.0 { 560.0 } else { -560.0 }, -400.0);
            Spec {
                centre,
                radii: Vec2::new(850.0, 640.0),
                cores: vec![(old, 300.0), (east, 220.0), (west, 200.0)],
                plazas: vec![old],
                bases: vec![
                    Basis::Grid { centre, size: 5000.0, theta: 0.0, weight: 0.15 },
                    Basis::Radial { centre: old, size: 300.0, weight: 1.6 },
                    Basis::Grid { centre: west, size: 520.0, theta: tw, weight: 1.0 },
                    Basis::Grid { centre: east, size: 520.0, theta: te, weight: 1.0 },
                ],
                twists: vec![Twist { centre: old, size: 330.0, angle: 0.75, wavelength: 200.0 }],
                industry: vec![(works, 240.0)],
                levels: lv,
            }
        }
        CityPreset::TwinCentres => {
            let a = dry(land, Vec2::new(-430.0, -150.0) + j(60.0), 220.0);
            let b = dry(land, Vec2::new(430.0, -90.0) + j(60.0), 220.0);
            let (ta, tb) = (j(0.25).x, 0.5 + j(0.2).x);
            let works = Vec2::new((a.x + b.x) * 0.5, -420.0);
            Spec {
                centre: (a + b) * 0.5,
                radii: Vec2::new(860.0, 620.0),
                cores: vec![(a, 300.0), (b, 280.0)],
                plazas: vec![a, b],
                bases: vec![
                    Basis::Grid { centre: (a + b) * 0.5, size: 5000.0, theta: 0.0, weight: 0.25 },
                    Basis::Grid { centre: a, size: 480.0, theta: ta, weight: 0.9 },
                    Basis::Grid { centre: b, size: 480.0, theta: tb, weight: 0.9 },
                    Basis::Radial { centre: a, size: 160.0, weight: 2.2 },
                    Basis::Radial { centre: b, size: 150.0, weight: 2.2 },
                ],
                twists: Vec::new(),
                industry: vec![(works, 280.0)],
                levels: lv,
            }
        }
    }
}

/// The lines of the land the streets follow: rivers, lakes, the coast, the
/// highway and the railway.
fn land_bases(land: &Landscape, rail_y: f32) -> Vec<Basis> {
    let mut out = Vec::new();
    for r in &land.rivers {
        let (size, weight) = if r.stream { (60.0, 0.8) } else { (150.0, 1.8) };
        out.push(Basis::Line { points: r.points.clone(), size, weight });
    }
    for l in &land.lakes {
        let mut ring = l.outline.clone();
        ring.push(l.outline[0]);
        out.push(Basis::Line { points: ring, size: 90.0, weight: 1.2 });
    }
    if let Some(sea) = &land.sea {
        if sea.coast.len() >= 2 {
            out.push(Basis::Line { points: sea.coast.clone(), size: 260.0, weight: 2.2 });
        }
    }
    let far = 3000.0;
    out.push(Basis::Line { points: vec![Vec2::new(-far, HIGHWAY_Y), Vec2::new(far, HIGHWAY_Y)], size: 160.0, weight: 1.2 });
    out.push(Basis::Line { points: vec![Vec2::new(-far, rail_y), Vec2::new(far, rail_y)], size: 90.0, weight: 0.7 });
    out
}

/// A polyline offset sideways by `d` (left of its direction for d > 0).
fn offset(pts: &[Vec2], d: f32) -> Vec<Vec2> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let a = pts[i.saturating_sub(1)];
            let b = pts[(i + 1).min(n - 1)];
            let t = (b - a).normalize_or_zero();
            pts[i] + Vec2::new(-t.y, t.x) * d
        })
        .collect()
}

/// Resample a polyline every `step` metres.
fn resample(pts: &[Vec2], step: f32) -> Vec<Vec2> {
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let k = (w[0].distance(w[1]) / step).ceil().max(1.0) as usize;
        for i in 1..=k {
            out.push(w[0].lerp(w[1], i as f32 / k as f32));
        }
    }
    out
}

/// Waterfront streets: along both banks of each river and inland along
/// the coast, kept where the ground is good.
fn waterfronts(site: &Site) -> Vec<Line> {
    let land = site.landscape;
    let mut shores: Vec<(Vec<Vec2>, f32)> = Vec::new();
    for r in land.rivers.iter().filter(|r| !r.stream) {
        let pts: Vec<Vec2> = r.points.iter().copied().filter(|p| p.x.abs() < HALF + 300.0 && p.y.abs() < HALF + 300.0).collect();
        if pts.len() >= 2 {
            shores.push((pts.clone(), r.width * 0.5 + 30.0));
            shores.push((pts, -(r.width * 0.5 + 30.0)));
        }
    }
    if let Some(sea) = &land.sea {
        if sea.coast.len() >= 2 {
            // Inland: the side away from the sea.
            let k = sea.coast.len() / 2;
            let mid = sea.coast[k];
            let t = (sea.coast[(k + 1).min(sea.coast.len() - 1)] - sea.coast[k.saturating_sub(1)]).normalize_or(Vec2::X);
            let left = Vec2::new(-t.y, t.x);
            let side = if sea.contains(mid + left * 60.0) { -1.0 } else { 1.0 };
            shores.push((sea.coast.clone(), side * 40.0));
        }
    }
    let mut out = Vec::new();
    for (pts, d) in shores {
        let line = resample(&offset(&resample(&pts, 10.0), d), STEP);
        let mut run: Vec<Vec2> = Vec::new();
        let mut flush = |run: &mut Vec<Vec2>| {
            if run.len() >= 2 && nd_sim::geom::path_length(run) > 160.0 {
                out.push(Line { pts: std::mem::take(run), level: Level::Major, family: Family::Major, closed: false });
            }
            run.clear();
        };
        for p in line {
            if site.ground(p) == Ground::Ok {
                run.push(p);
            } else {
                flush(&mut run);
            }
        }
        flush(&mut run);
    }
    out
}

/// Generate a city on `land` (the landscape the preset names, made with
/// `CityPreset::landscape`). Deterministic.
pub fn generate(preset: CityPreset, seed: u64, land: &Landscape) -> CityPlan {
    let mut rng = nd_sim::rng_for(seed, 0xC17_6E4 ^ preset as u64);
    let rail_y = nd_sim::rail::TRUNK_Y;
    let spec = spec(preset, land, &mut rng);
    let mut bases = land_bases(land, rail_y);
    bases.extend(spec.bases.iter().cloned());
    let field = TensorField { bases, twists: spec.twists.clone(), seed };
    let site = Site {
        landscape: land,
        centre: spec.centre,
        radii: spec.radii,
        wobble_seed: seed ^ 0xB10B,
        lo: Vec2::new(-HALF, -HALF),
        hi: Vec2::new(HALF, NORTH_LIMIT),
        rail_y: Some(rail_y),
        cores: spec.cores.clone(),
        plazas: spec.plazas.clone(),
    };
    let raster = FieldRaster::new(&field, site.lo - Vec2::splat(20.0), site.hi + Vec2::splat(20.0), 6.0);
    let mut tracer = Tracer::new(&site, &raster);
    for l in waterfronts(&site) {
        tracer.add_line(l);
    }
    // Arterials start just off the centres (clear of the plazas).
    let anchors: Vec<Vec2> = spec.cores.iter().flat_map(|(c, _)| [*c + Vec2::new(80.0, 15.0), *c + Vec2::new(-15.0, 80.0)]).collect();
    for (k, level) in [Level::Main, Level::Major, Level::Minor].into_iter().enumerate() {
        let a: &[Vec2] = if k == 0 { &anchors } else { &[] };
        tracer.fill(level, &spec.levels[k], a, &mut rng);
    }
    let mut lines = std::mem::take(&mut tracer.lines);
    graph::join_ends(&mut lines, &site, [140.0, 90.0, 55.0]);
    graph::join_plazas(&mut lines, &spec.plazas);
    let mut g = graph::Graph::build(&lines);
    g.clean();
    for &c in &spec.plazas {
        g.thin_star(c, 4);
    }
    g.clean();
    let links = add_links(&mut g);
    let junctions: std::collections::HashSet<(u32, u32)> = g.alive_edges().flat_map(|(_, e)| [e.a, e.b]).map(|n| key(g.nodes[n].pos)).collect();
    let roads = g.ways().into_iter().map(|(pts, level)| CityRoad { points: sample_way(&pts, &junctions, rail_y), class: RoadClass::of(level) }).collect();
    let zones = zone(&g, &site, &spec, seed);
    CityPlan { preset, seed, roads, zones, links, cores: spec.cores.iter().map(|c| c.0).collect() }
}

/// A curved street runs straight for this far into a junction.
const JUNCTION_LEG: f32 = 12.0;

fn key(p: Vec2) -> (u32, u32) {
    (p.x.to_bits(), p.y.to_bits())
}

/// Even spacing along a polyline: `n` pieces.
fn even(pts: &[Vec2], n: usize) -> Vec<Vec2> {
    let total = nd_sim::geom::path_length(pts);
    let mut out = vec![pts[0]];
    for k in 1..n {
        out.push(nd_sim::geom::point_along(pts, total * k as f32 / n as f32).0);
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// A street as the game should lay it: straight legs where it runs
/// straight, curves sampled about every `CURVE_STEP` (7 m) along a spline
/// through the simplified points, every junction a vertex the street runs
/// straight into (so it meets the junction at the angle the network was
/// cleaned to), and a vertex where it crosses the railway, with no other
/// vertex within a few metres of it.
fn sample_way(pts: &[Vec2], junctions: &std::collections::HashSet<(u32, u32)>, rail_y: f32) -> Vec<Vec2> {
    // Fixed points: the ends, the junctions and the railway crossings.
    let mut marked: Vec<(Vec2, bool)> = Vec::new();
    let mut crossings: Vec<Vec2> = Vec::new();
    for (i, &p) in pts.iter().enumerate() {
        let fixed = i == 0 || i + 1 == pts.len() || junctions.contains(&key(p));
        if let Some(&(q, _)) = marked.last() {
            let d = p - q;
            let mut ts: Vec<f32> = Vec::new();
            if (q.y - rail_y) * (p.y - rail_y) < 0.0 {
                ts.push((rail_y - q.y) / (p.y - q.y));
            }
            ts.sort_by(f32::total_cmp);
            for t in ts {
                let x = q + d * t;
                marked.retain(|(m, f)| *f || m.distance(x) > 8.0);
                marked.push((x, true));
                crossings.push(x);
            }
        }
        if fixed || crossings.iter().all(|m| m.distance(p) > 8.0) {
            marked.push((p, fixed));
        }
    }
    let mut out = vec![marked[0].0];
    let mut span = vec![marked[0].0];
    for &(p, fixed) in &marked[1..] {
        span.push(p);
        if !fixed {
            continue;
        }
        if span.len() == 2 {
            out.push(p);
        } else {
            let dense = nd_sim::plan::sample_curve(&span);
            let n = ((nd_sim::geom::path_length(&dense) / nd_sim::plan::CURVE_STEP).round() as usize).max(1);
            let (a, b) = (span[0], p);
            // The last stretch into a junction runs straight, so the street
            // meets it at the angle the network was cleaned to.
            // (The spline's last sample is the span's end only to rounding:
            // the end itself goes in.)
            let samples = even(&dense, n);
            out.extend(samples[1..samples.len() - 1].iter().copied().filter(|q| q.distance(a) > JUNCTION_LEG && q.distance(b) > JUNCTION_LEG));
            out.push(b);
        }
        span = vec![p];
    }
    out
}

/// Up to two arterial stubs north to the highway's edge, far apart.
fn add_links(g: &mut graph::Graph) -> Vec<Vec2> {
    let mut cands: Vec<(usize, f32)> = (0..g.nodes.len())
        .filter(|&n| g.degree(n) > 0 && g.nodes[n].pos.y > NORTH_LIMIT - 120.0)
        .map(|n| {
            let best = g.nodes[n].edges.iter().map(|&e| g.edges[e].level).min().unwrap_or(Level::Minor);
            (n, g.nodes[n].pos.y + (2 - best as i32) as f32 * 60.0 - g.nodes[n].pos.x.abs() * 0.05)
        })
        .collect();
    if cands.is_empty() {
        // Nothing near the north edge: the northernmost junctions.
        cands = (0..g.nodes.len()).filter(|&n| g.degree(n) > 0).map(|n| (n, g.nodes[n].pos.y)).collect();
    }
    cands.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut links: Vec<Vec2> = Vec::new();
    for (n, _) in cands {
        if links.len() >= 2 {
            break;
        }
        let p = g.nodes[n].pos;
        if links.iter().any(|l| (l.x - p.x).abs() < 600.0) || p.x.abs() > 1400.0 {
            continue;
        }
        let top = Vec2::new(p.x, NORTH_LIMIT);
        // The stub runs clear of every other street.
        let clear = g.alive_edges().all(|(_, e)| {
            if e.a == n || e.b == n {
                // Its own streets leave the junction away from the stub.
                return e.from(n).iter().skip(1).all(|q| q.y < p.y + 2.0 || (q.x - p.x).abs() > 12.0);
            }
            e.pts.windows(2).all(|w| {
                let (_, q) = nd_sim::geom::project_on_segment(w[0], p, top);
                let (_, r) = nd_sim::geom::project_on_segment(w[1], p, top);
                q.distance(w[0]) > 20.0 && r.distance(w[1]) > 20.0 && graph_cross(w[0], w[1], p, top).is_none()
            })
        });
        if !clear {
            continue;
        }
        if top.y - p.y > 3.0 {
            g.nodes.push(graph::Node { pos: top, edges: Vec::new() });
            let t = g.nodes.len() - 1;
            let id = g.edges.len();
            g.edges.push(graph::Edge { a: n, b: t, pts: vec![p, top], level: Level::Main, line: usize::MAX - links.len(), alive: true });
            g.nodes[n].edges.push(id);
            g.nodes[t].edges.push(id);
        }
        links.push(top);
    }
    links
}

fn graph_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f32, f32)> {
    let r = b - a;
    let s = d - c;
    let den = r.perp_dot(s);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

fn hash01(seed: u64, p: Vec2) -> f32 {
    let mut h = seed ^ ((p.x.round() as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)) ^ ((p.y.round() as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// The blocks and the strips outside the outer streets, zoned.
fn zone(g: &graph::Graph, site: &Site, spec: &Spec, seed: u64) -> Vec<CityZone> {
    let land = site.landscape;
    let rail_y = site.rail_y.unwrap_or(f32::NEG_INFINITY);
    let faces = g.faces();
    // Edges round the outside of town.
    let mut outer_edges = std::collections::HashSet::new();
    for (ring, halves) in &faces {
        if nd_sim::geom::polygon_area(ring) < 0.0 {
            outer_edges.extend(halves.iter().map(|h| h.0));
        }
    }
    let feature = |polygon: Vec<Vec2>, frontage: f32, edges: &[usize], strip: bool| -> landuse::Patch {
        let c = nd_sim::geom::polygon_centroid(&polygon);
        let area = nd_sim::geom::polygon_area(&polygon).abs();
        let on_arterial = edges.iter().any(|&e| g.edges[e].level == Level::Main);
        let on_collector = edges.iter().any(|&e| g.edges[e].level == Level::Major);
        let waterfront = !land.is_flat() && land.water_distance(c) < 80.0;
        let edge = if strip { 1.0 } else if edges.iter().any(|e| outer_edges.contains(e)) { 0.6 } else { 0.0 };
        let rail = (-((c.y - rail_y).abs() - 30.0).max(0.0) / 150.0).exp();
        let highway = (-((HIGHWAY_Y - c.y) - 90.0).max(0.0) / 150.0).exp() * 0.8;
        let spot = spec.industry.iter().map(|(p, r)| (-(c.distance(*p) / r).powi(2)).exp()).fold(0.0, f32::max);
        // The blocks round a plaza are its park.
        let plaza = !strip && spec.plazas.iter().any(|p| nd_sim::geom::point_in_polygon(*p, &polygon) || polygon.iter().any(|q| q.distance(*p) < 1.0));
        landuse::Patch { centroid: c, frontage, area, core: site.core(c), on_arterial, on_collector, waterfront, edge, industry: rail.max(highway).max(spot), plaza, luck: hash01(seed, c), polygon }
    };
    let mut patches = Vec::new();
    // Strips first: the blocks are painted over them.
    for (ring, halves) in &faces {
        if nd_sim::geom::polygon_area(ring) >= 0.0 {
            continue;
        }
        for &(e, fwd) in halves {
            let ed = &g.edges[e];
            if ed.line >= usize::MAX - 4 {
                continue;
            }
            let pts: Vec<Vec2> = if fwd { ed.pts.clone() } else { ed.pts.iter().rev().copied().collect() };
            let pts = resample(&pts, 10.0);
            for chunk in pts.chunks(8) {
                if chunk.len() < 2 {
                    continue;
                }
                // The outside is on the left of the outer face's half-edges.
                let side: Vec<Vec2> = offset(chunk, 44.0);
                let mut poly = chunk.to_vec();
                poly.extend(side.into_iter().rev());
                if poly.len() < 4 || nd_sim::geom::self_intersects(&poly) {
                    continue;
                }
                let frontage = nd_sim::geom::path_length(chunk);
                patches.push(feature(poly, frontage, &[e], true));
            }
        }
    }
    for (ring, halves) in faces {
        if ring.len() < 3 || nd_sim::geom::polygon_area(&ring) <= 0.0 || nd_sim::geom::self_intersects(&ring) {
            continue;
        }
        let frontage = nd_sim::geom::perimeter(&ring);
        let edges: Vec<usize> = halves.iter().map(|h| h.0).collect();
        patches.push(feature(ring, frontage, &edges, false));
    }
    let uses = landuse::allocate(&patches);
    patches.into_iter().zip(uses).map(|(p, land_use)| CityZone { polygon: p.polygon, land_use }).collect()
}
