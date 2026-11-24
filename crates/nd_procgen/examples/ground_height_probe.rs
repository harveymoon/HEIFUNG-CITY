//! [r16-ground] Where the heights things are set at part from the ground as
//! drawn, from a save. Not part of the game.
//!
//! `cargo run --profile fast -p nd_procgen --example ground_height_probe -- <save.ron> [step]`
//!
//! The drawn surfaces at a point: the landscape's ground mesh as the
//! renderer builds it (8 m cells, lots and water edges cut out), the caps of
//! the cut lots (`lot_ground`), a lot's floor, the carriageway and pavement
//! of any way there (its height), any bridge deck, the water's surface.
//! A thing is "off" when it is more than `TOL` from every one of them:
//! floating over the nearest, or sunk under it.
//!
//! Things measured, each at the height the renderer sets it at:
//! - walkers and riders (`lift_movers`: their way's height, else the
//!   surface nearest the ground);
//! - cars (`car_height_at`);
//! - laid things on a grid of points off the ways (`lie_height`: props,
//!   parks, lot dressing, fire crews, marks);
//! - the bare `ground_height` on the same grid (figures' tunnel test, foot
//!   wear, grime, holo strips, survey, undercroft).
use glam::Vec2;
use nd_procgen::palette::Palette;
use nd_sim::{LotId, World};

const TOL: f32 = 0.25;

struct Tris {
    cell: f32,
    lo: Vec2,
    n: usize,
    bins: Vec<Vec<u32>>,
    pts: Vec<[glam::Vec3; 3]>,
}

impl Tris {
    fn new(meshes: &[&nd_procgen::MeshData], half: f32) -> Self {
        let cell = 8.0;
        let lo = Vec2::splat(-half - 16.0);
        let n = ((half * 2.0 + 32.0) / cell).ceil() as usize + 1;
        let mut bins = vec![Vec::new(); n * n];
        let mut pts = Vec::new();
        for m in meshes {
            for t in 0..m.indices.len() / 3 {
                let i = &m.indices[t * 3..t * 3 + 3];
                let v = [0, 1, 2].map(|k| glam::Vec3::from(m.positions[i[k] as usize]));
                let (mut a, mut b) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for p in v {
                    a = a.min(Vec2::new(p.x, p.z));
                    b = b.max(Vec2::new(p.x, p.z));
                }
                let c = |q: f32, o: f32| (((q - o) / cell).floor().max(0.0) as usize).min(n - 1);
                let id = pts.len() as u32;
                pts.push(v);
                for j in c(a.y, lo.y)..=c(b.y, lo.y) {
                    for i in c(a.x, lo.x)..=c(b.x, lo.x) {
                        bins[j * n + i].push(id);
                    }
                }
            }
        }
        Self { cell, lo, n, bins, pts }
    }

    /// Every drawn triangle's height over `p`.
    fn heights(&self, p: Vec2, out: &mut Vec<f32>) {
        let c = |q: f32, o: f32| ((q - o) / self.cell).floor() as isize;
        let (i, j) = (c(p.x, self.lo.x), c(p.y, self.lo.y));
        if i < 0 || j < 0 || i as usize >= self.n || j as usize >= self.n {
            return;
        }
        for &t in &self.bins[j as usize * self.n + i as usize] {
            let [a, b, cc] = self.pts[t as usize];
            let (a2, b2, c2) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z), Vec2::new(cc.x, cc.z));
            let d = (b2 - a2).perp_dot(c2 - a2);
            if d.abs() < 1e-8 {
                continue;
            }
            let u = (p - a2).perp_dot(c2 - a2) / d;
            let v = (b2 - a2).perp_dot(p - a2) / d;
            if u < -1e-4 || v < -1e-4 || u + v > 1.0 + 1e-4 {
                continue;
            }
            out.push(a.y + (b.y - a.y) * u + (cc.y - a.y) * v);
        }
    }
}

struct LotBins {
    cell: f32,
    lo: Vec2,
    n: usize,
    bins: Vec<Vec<LotId>>,
}

impl LotBins {
    fn new(w: &World) -> Self {
        let half = w.landscape.land_half().max(2000.0);
        let cell = 32.0;
        let lo = Vec2::splat(-half);
        let n = (half * 2.0 / cell).ceil() as usize + 1;
        let mut bins = vec![Vec::new(); n * n];
        for (id, l) in w.lots.iter() {
            if l.polygon.len() < 3 {
                continue;
            }
            let (a, b) = nd_sim::geom::bounds(&l.polygon);
            let c = |q: f32, o: f32| (((q - o) / cell).floor().max(0.0) as usize).min(n - 1);
            for j in c(a.y, lo.y)..=c(b.y, lo.y) {
                for i in c(a.x, lo.x)..=c(b.x, lo.x) {
                    bins[j * n + i].push(id);
                }
            }
        }
        Self { cell, lo, n, bins }
    }
    fn at(&self, w: &World, p: Vec2) -> Option<LotId> {
        let c = |q: f32, o: f32| ((q - o) / self.cell).floor() as isize;
        let (i, j) = (c(p.x, self.lo.x), c(p.y, self.lo.y));
        if i < 0 || j < 0 || i as usize >= self.n || j as usize >= self.n {
            return None;
        }
        self.bins[j as usize * self.n + i as usize].iter().copied().find(|&id| w.lots[id].contains(p))
    }
}

struct Surfaces<'a> {
    w: &'a World,
    tris: Tris,
    lots: LotBins,
}

impl Surfaces<'_> {
    /// Every drawn surface over `p` and whether `p` is on a way.
    fn at(&self, p: Vec2, out: &mut Vec<f32>) -> bool {
        out.clear();
        let w = self.w;
        self.tris.heights(p, out);
        if let Some(id) = self.lots.at(w, p) {
            if let Some((pad, step)) = nd_procgen::lot_ground::lot_floor(w, id) {
                out.push(step.map_or(pad, |s| s.pad_at(p, pad)));
            }
        }
        let mut on_way = false;
        let r = Vec2::splat(30.0);
        for sid in w.roads.segments_in_box(p - r, p + r) {
            let Some(s) = w.roads.try_segment(sid) else { continue };
            let (a, b) = w.roads.segment_points(sid);
            let (t, q) = nd_sim::roads::project_on_segment(p, a, b);
            // The carriageway and a pavement's width beside it.
            if q.distance(p) <= s.half_width() + if s.is_rail() { 0.5 } else { 3.0 } {
                let h = w.road_height(sid, t);
                out.push(h);
                out.push(h + 0.15);
                on_way = true;
            }
        }
        out.extend(w.ground.decks_at(p));
        if !w.landscape.is_flat() && w.landscape.is_water(p) {
            out.push(nd_sim::landscape::WATER_LEVEL);
        }
        on_way
    }
}

#[derive(Default)]
struct Tally {
    n: usize,
    float: usize,
    sink: usize,
    worst: Vec<(f32, Vec2, String)>,
}

impl Tally {
    fn add(&mut self, p: Vec2, placed: f32, surf: &[f32], what: impl FnOnce() -> String) {
        self.n += 1;
        let Some(near) = surf.iter().copied().min_by(|a, b| (a - placed).abs().total_cmp(&(b - placed).abs())) else { return };
        let d = placed - near;
        if d.abs() <= TOL {
            return;
        }
        if d > 0.0 {
            self.float += 1;
        } else {
            self.sink += 1;
        }
        self.worst.push((d, p, what()));
    }
    fn print(&mut self, name: &str) {
        println!("{name}: {} points, off by >{TOL} m: {} ({:.2}%) floating {} sunk {}", self.n, self.float + self.sink, 100.0 * (self.float + self.sink) as f32 / self.n.max(1) as f32, self.float, self.sink);
        self.worst.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
        // Worst spots, one per 40 m.
        let mut shown: Vec<Vec2> = Vec::new();
        for (d, p, s) in &self.worst {
            if shown.iter().any(|q| q.distance(*p) < 40.0) {
                continue;
            }
            shown.push(*p);
            println!("  {d:+.2} m at ({:.1},{:.1}) {s}", p.x, p.y);
            if shown.len() >= 10 {
                break;
            }
        }
        // How far off, in bands.
        let bands = [0.5, 1.0, 2.0, 5.0, f32::MAX];
        let mut c = [0usize; 5];
        for (d, _, _) in &self.worst {
            c[bands.iter().position(|b| d.abs() <= *b).unwrap()] += 1;
        }
        println!("  by size: <=0.5 m {} <=1 m {} <=2 m {} <=5 m {} more {}", c[0], c[1], c[2], c[3], c[4]);
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = World::read_save(&a[1]).expect("load");
    let step: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let _ = step;
    let pal = Palette::default();
    let t0 = std::time::Instant::now();
    let terrain = nd_procgen::terrain::build_landscape_terrain(&w, &pal, nd_procgen::lot_ground::LAND_CELL);
    let (caps, _) = nd_procgen::lot_ground::lot_ground(&w, &pal);
    let s = Surfaces { w: &w, tris: Tris::new(&[&terrain, &caps], w.landscape.land_half()), lots: LotBins::new(&w) };
    eprintln!("surfaces in {:.1}s", t0.elapsed().as_secs_f32());
    let mut buf = Vec::new();
    // [r16-ground] `ND_PROBE_OLD=1`: things set at the sim's heights, as
    // before; else at the drawn ground (`drawn_ground`), as the game now does.
    let old = std::env::var("ND_PROBE_OLD").is_ok();
    let drawn = nd_procgen::drawn_ground::DrawnGround::new(&w);
    let lie_at = |p: Vec2| if old { w.surface_height_near(p, w.ground_height(p)) } else { drawn.lie(&w, p) };
    let bare_at = |p: Vec2| if old { w.ground_height(p) } else { drawn.height(&w, p) };
    // `at x y`: everything about one point.
    if a.get(2).map(String::as_str) == Some("at") {
        let p = Vec2::new(a[3].parse().unwrap(), a[4].parse().unwrap());
        let on_way = s.at(p, &mut buf);
        let probe = nd_procgen::lot_ground::TerrainProbe::new(&w);
        let mut t = Vec::new();
        s.tris.heights(p, &mut t);
        println!("drawn {:.2} drawn lie {:.2}", drawn.height(&w, p), drawn.lie(&w, p));
        println!("at ({:.1},{:.1}): ground {:.2} surface {:.2} lie {:.2} probe {:.2} on_way {on_way} lot {:?} decks {:?} water {}", p.x, p.y, w.ground_height(p), w.surface_height(p), w.surface_height_near(p, w.ground_height(p)), probe.height(p), s.lots.at(&w, p), w.ground.decks_at(p), w.landscape.is_water(p));
        println!("  mesh tris {t:?}
  all surfaces {buf:?}");
        for dy in [-8.0f32, -4.0, 0.0, 4.0, 8.0] {
            let row: Vec<String> = [-8.0f32, -4.0, 0.0, 4.0, 8.0].iter().map(|dx| format!("{:7.2}", w.ground_height(p + Vec2::new(*dx, dy)))).collect();
            println!("  ground row {dy:+}: {}", row.join(" "));
        }
        return;
    }

    // Walkers and riders.
    let (mut walk, mut ride, mut raised) = (Tally::default(), Tally::default(), 0usize);
    for (id, c) in w.citizens.iter() {
        let nd_sim::Activity::Traveling { .. } = &c.activity else { continue };
        if c.on_platform() || w.citizen_in_tunnel(c) {
            continue;
        }
        let p = c.pos;
        if p.x.abs() >= w.landscape.land_half() || p.y.abs() >= w.landscape.land_half() {
            continue;
        }
        // On a raised way (a footbridge's stairs and deck, an elevated
        // path, a viaduct's walk): its own deck, not the ground; not counted.
        let way = w.walker_way_height(c, p);
        if w.walker_level(c, p) > 0.0 || way.is_some_and(|h| (h - w.ground_height(p)).abs() > 0.5) {
            raised += 1;
            continue;
        }
        let lift = way.unwrap_or_else(|| lie_at(p));
        s.at(p, &mut buf);
        let t = if c.mode == nd_sim::citizens::TravelMode::Bike { &mut ride } else { &mut walk };
        t.add(p, lift, &buf, || format!("citizen {id:?} ground {:.2}", w.ground_height(p)));
    }
    println!("walkers and riders on a raised way (not counted): {raised}");
    walk.print("walkers");
    ride.print("riders");

    // Cars.
    let mut cars = Tally::default();
    for (id, car) in w.traffic.cars.iter() {
        if w.car_in_tunnel(car) {
            continue;
        }
        let p = car.pos;
        let y = w.car_height_at(car, p);
        s.at(p, &mut buf);
        cars.add(p, y, &buf, || format!("car {id:?} ground {:.2}", w.ground_height(p)));
    }
    cars.print("cars");

    // Laid things and the bare ground on a grid, near the built city (a
    // way or lot within 40 m).
    let (mut lie, mut bare, mut lie_way, mut lie_lot, mut lie_free) = (Tally::default(), Tally::default(), Tally::default(), Tally::default(), Tally::default());
    let half = w.landscape.land_half() - 8.0;
    let mut y = -half;
    while y < half {
        let mut x = -half;
        while x < half {
            let p = Vec2::new(x, y);
            x += step;
            let r = Vec2::splat(40.0);
            if w.roads.segments_in_box(p - r, p + r).next().is_none() {
                continue;
            }
            let on_way = s.at(p, &mut buf);
            if w.ground.deck_at(p).is_some() {
                continue;
            }
            let g = bare_at(p);
            let l = lie_at(p);
            let lot = s.lots.at(&w, p);
            let why = || format!("{} ground {g:.2} lie {l:.2}", if on_way { "way" } else if lot.is_some() { "lot" } else { "free" });
            lie.add(p, l, &buf, why);
            bare.add(p, g, &buf, why);
            let t = if on_way { &mut lie_way } else if lot.is_some() { &mut lie_lot } else { &mut lie_free };
            t.add(p, l, &buf, why);
        }
        y += step;
    }
    lie.print("lie_height (props, parks, crews) on the grid");
    lie_way.print("  of which on a way");
    lie_lot.print("  of which in a lot");
    lie_free.print("  of which free ground");
    bare.print("bare ground_height on the grid");
}
