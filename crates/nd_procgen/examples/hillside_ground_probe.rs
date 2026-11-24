//! [r15-hillside] How the drawn ground meets the lots and their walls, from
//! a save: the coarse terrain mesh (as the renderer draws it, 8 m cells,
//! holes cut) against each lot's flat floor and each retaining wall's foot.
//! Not part of the game.
//!
//! `cargo run --profile fast -p nd_procgen --example hillside_ground_probe -- <save.ron> [x y radius]`
//! With `ND_PROBE_OLD=1` the ground and walls as drawn before r15-hillside
//! (only stepped lots and benches cut out, no lot sides, wall feet on the
//! straight line between their ends).
//!
//! Counts:
//! - lots with exposed terrain: the terrain mesh rises over the lot's floor
//!   (its pad, the back pad behind a step) by more than 0.1 m somewhere
//!   inside the outline;
//! - hanging walls (saw-tooth skirts): a wall standing over the ground beyond
//!   it whose foot is above the drawn terrain in front of it by more than
//!   0.3 m somewhere along it (a gap under the wall);
//! - buried walls: the drawn terrain in front of a wall rises over its top.
use glam::Vec2;
use nd_procgen::palette::Palette;
use nd_sim::World;

struct Index {
    cell: f32,
    lo: Vec2,
    n: usize,
    bins: Vec<Vec<u32>>,
}

fn tri_at(m: &nd_procgen::MeshData, t: usize) -> [glam::Vec3; 3] {
    let i = &m.indices[t * 3..t * 3 + 3];
    [0, 1, 2].map(|k| glam::Vec3::from(m.positions[i[k] as usize]))
}

impl Index {
    fn new(m: &nd_procgen::MeshData, half: f32) -> Self {
        let cell = 8.0;
        let lo = Vec2::splat(-half - 16.0);
        let n = ((half * 2.0 + 32.0) / cell).ceil() as usize + 1;
        let mut bins = vec![Vec::new(); n * n];
        for t in 0..m.indices.len() / 3 {
            let v = tri_at(m, t);
            let (mut a, mut b) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for p in v {
                a = a.min(Vec2::new(p.x, p.z));
                b = b.max(Vec2::new(p.x, p.z));
            }
            let c = |q: f32, o: f32| (((q - o) / cell).floor().max(0.0) as usize).min(n - 1);
            for j in c(a.y, lo.y)..=c(b.y, lo.y) {
                for i in c(a.x, lo.x)..=c(b.x, lo.x) {
                    bins[j * n + i].push(t as u32);
                }
            }
        }
        Self { cell, lo, n, bins }
    }

    /// The highest drawn terrain over `p`, if any is (none in a hole).
    fn height(&self, m: &nd_procgen::MeshData, p: Vec2) -> Option<f32> {
        let c = |q: f32, o: f32| ((q - o) / self.cell).floor() as isize;
        let (i, j) = (c(p.x, self.lo.x), c(p.y, self.lo.y));
        if i < 0 || j < 0 || i as usize >= self.n || j as usize >= self.n {
            return None;
        }
        let mut best: Option<f32> = None;
        for &t in &self.bins[j as usize * self.n + i as usize] {
            let [a, b, cc] = tri_at(m, t as usize);
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
            let h = a.y + (b.y - a.y) * u + (cc.y - a.y) * v;
            best = Some(best.map_or(h, |x: f32| x.max(h)));
        }
        best
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let old = std::env::var("ND_PROBE_OLD").is_ok();
    let w = World::read_save(&a[1]).expect("load");
    let focus = (a.len() >= 5).then(|| (Vec2::new(a[2].parse().unwrap(), a[3].parse().unwrap()), a[4].parse::<f32>().unwrap()));
    let near = |p: Vec2| focus.is_none_or(|(c, r)| c.distance(p) <= r);
    let pal = Palette::default();
    let t0 = std::time::Instant::now();
    let probe = nd_procgen::lot_ground::TerrainProbe::new(&w);
    let mesh = if old {
        // The grid as drawn, cut only where it was: benches, stepped lots.
        let half = w.landscape.land_half();
        let n = ((half * 2.0) / 8.0).ceil() as i32 + 1;
        let mut m = nd_procgen::MeshData::default();
        for j in 0..n {
            for i in 0..n {
                let p = probe.at(i, j);
                m.positions.push([p.x, probe.vertex(i, j), p.y]);
                m.normals.push([0.0, 1.0, 0.0]);
                m.colors.push([1.0; 4]);
                m.uvs.push([0.0; 2]);
            }
        }
        for j in 0..n as u32 - 1 {
            for i in 0..n as u32 - 1 {
                let a = j * n as u32 + i;
                let (b, c) = (a + 1, a + n as u32);
                m.indices.extend_from_slice(&[a, c, b, b, c, c + 1]);
            }
        }
        let mut holes = nd_procgen::holes::bench_holes(&w);
        let mut steps: Vec<nd_sim::LotId> = w.ground.steps.keys().copied().collect();
        steps.sort();
        holes.extend(steps.into_iter().filter_map(|id| w.lots.get(id)).map(|l| l.polygon.clone()));
        nd_procgen::holes::cut_holes(&mut m, &holes);
        m
    } else {
        nd_procgen::terrain::build_landscape_terrain(&w, &pal, 8.0)
    };
    let idx = Index::new(&mesh, w.landscape.land_half());
    eprintln!("terrain {} tris in {:.1}s", mesh.indices.len() / 3, t0.elapsed().as_secs_f32());
    // Lots: the terrain over the floor.
    let (mut lots, mut exposed, mut exposed_built, mut exposed_m2) = (0, 0, 0, 0.0f32);
    let mut worst: Vec<(f32, String)> = Vec::new();
    for (id, l) in w.lots.iter() {
        let Some(&pad) = w.ground.pads.get(&id) else { continue };
        if l.polygon.len() < 3 || !near(l.center) || l.park.is_some() {
            continue;
        }
        lots += 1;
        let step = w.ground.steps.get(&id).copied();
        let (lo, hi) = nd_sim::geom::bounds(&l.polygon);
        let (mut over, mut cells) = (0.0f32, 0);
        let mut y = lo.y + 0.75;
        while y < hi.y {
            let mut x = lo.x + 0.75;
            while x < hi.x {
                let p = Vec2::new(x, y);
                if l.contains(p) {
                    let floor = step.map_or(pad, |s| s.pad_at(p, pad));
                    if let Some(h) = idx.height(&mesh, p) {
                        if h > floor + 0.1 {
                            over = over.max(h - floor);
                            cells += 1;
                        }
                    }
                }
                x += 1.5;
            }
            y += 1.5;
        }
        if cells > 0 {
            exposed += 1;
            exposed_m2 += cells as f32 * 2.25;
            if l.building.is_some() {
                exposed_built += 1;
            }
            worst.push((cells as f32 * 2.25, format!("lot {id:?} at ({:.0},{:.0}) pad {pad:.1} exposed {:.0} m2 up to {over:.1} m built {}", l.center.x, l.center.y, cells as f32 * 2.25, l.building.is_some())));
        }
    }
    // Lot edges where the floor and the drawn ground part with nothing
    // between them: no sim wall, and (r15) not a cut lot (whose sides are
    // drawn by `lot_ground`).
    let cut: std::collections::HashSet<nd_sim::LotId> = if old { Default::default() } else { nd_procgen::lot_ground::cut_lots_with(&w, &probe).into_iter().collect() };
    let (mut open_m, mut open_lots, mut open_tall) = (0.0f32, 0, 0);
    for (id, l) in w.lots.iter() {
        let Some((pad, step)) = nd_procgen::lot_ground::lot_floor(&w, id) else { continue };
        if l.polygon.len() < 3 || !near(l.center) || (l.building.is_none() && l.park.is_some()) || cut.contains(&id) {
            continue;
        }
        let (mut m, mut tall) = (0.0f32, false);
        for (ea, eb) in nd_sim::landscape::lot_edges(&l.polygon, step.as_ref()) {
            let len = ea.distance(eb);
            let n = (len / 1.0).ceil().max(1.0) as usize;
            for k in 0..n {
                let q = ea.lerp(eb, (k as f32 + 0.5) / n as f32);
                if w.ground.retaining.iter().any(|r| r.lot == id && nd_sim::geom::project_on_segment(q, r.a, r.b).1.distance(q) < 0.05) {
                    continue;
                }
                let floor = step.map_or(pad, |s| s.pad_at(q, pad));
                let g = probe.height(q) + 0.08;
                if (g - floor).abs() > 0.5 {
                    m += len / n as f32;
                    tall |= (g - floor).abs() > 6.0;
                }
            }
        }
        if m > 0.0 {
            open_m += m;
            open_lots += 1;
            if tall {
                open_tall += 1;
            }
        }
    }
    // Cut lots whose cap does not cover their hole (a see-through patch).
    if !old {
        let pal2 = Palette::default();
        for &id in cut.iter() {
            let l = &w.lots[id];
            let tri: f32 = nd_procgen::mesh::triangulate(&l.polygon).iter().map(|t| (l.polygon[t[1]] - l.polygon[t[0]]).perp_dot(l.polygon[t[2]] - l.polygon[t[0]]).abs() * 0.5).sum();
            let mut m = nd_procgen::MeshData::default();
            m.ground_polygon(&l.polygon, 0.0, [1.0; 4]);
            let cap: f32 = m.indices.chunks_exact(3).map(|t| {
                let q = |i: u32| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2]);
                (q(t[1]) - q(t[0])).perp_dot(q(t[2]) - q(t[0])).abs() * 0.5
            }).sum();
            if (tri - cap).abs() > 1.0 {
                println!("  cap/hole mismatch lot {id:?} at ({:.0},{:.0}): hole {tri:.0} m2 cap {cap:.0} m2 ({} vertices)", l.center.x, l.center.y, l.polygon.len());
            }
        }
        let _ = pal2;
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("lot edges open (floor and drawn ground >0.5 m apart, nothing between): {open_lots} lots, {open_m:.0} m; lots with an open drop >6 m: {open_tall}");
    println!("lots with pads: {lots}  with terrain over the floor: {exposed} (built {exposed_built})  area {exposed_m2:.0} m2");
    for (_, s) in worst.iter().take(8) {
        println!("  {s}");
    }
    // Walls: the drawn terrain under and over each wall standing over the drop.
    let (mut walls, mut hanging, mut buried, mut gap_m) = (0, 0, 0, 0.0f32);
    let mut wworst: Vec<(f32, String)> = Vec::new();
    for r in &w.ground.retaining {
        if r.holds_back || !near(r.a) {
            continue;
        }
        walls += 1;
        let len = r.a.distance(r.b);
        let n = (len / 0.5).ceil().max(1.0) as usize;
        let (mut gap, mut cover, mut gap_len) = (0.0f32, 0.0f32, 0.0f32);
        for k in 0..=n {
            let f = k as f32 / n as f32;
            let q = r.a.lerp(r.b, f) - r.inward * 0.4;
            // The face's foot as drawn (`terrain::retaining_walls`).
            let mut foot = r.foot_a.min(r.top) + (r.foot_b.min(r.top) - r.foot_a.min(r.top)) * f - 0.3;
            if !old {
                // r15: down to the lowest drawn ground in front of it.
                let g_lo = probe.breaks(r.a, r.b).into_iter().map(|t| probe.height(r.a.lerp(r.b, t))).fold(f32::MAX, f32::min);
                foot = foot.min(g_lo - 0.3);
            }
            let Some(h) = idx.height(&mesh, q) else { continue };
            if foot - h > 0.3 {
                gap = gap.max(foot - h);
                gap_len += len / n as f32;
            }
            cover = cover.max(h - r.top - 0.12);
            let _ = &probe;
        }
        if gap > 0.0 {
            hanging += 1;
            gap_m += gap_len;
            wworst.push((gap, format!("wall of lot {:?} ({:.0},{:.0})-({:.0},{:.0}) top {:.1} feet {:.1}/{:.1} gap up to {gap:.1} m over {gap_len:.0} m", r.lot, r.a.x, r.a.y, r.b.x, r.b.y, r.top, r.foot_a, r.foot_b)));
        }
        if cover > 0.3 {
            buried += 1;
        }
    }
    wworst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("walls over a drop: {walls}  hanging (gap under the foot >0.3 m): {hanging} ({gap_m:.0} m of wall)  buried (terrain over the top >0.3 m): {buried}");
    for (_, s) in wworst.iter().take(8) {
        println!("  {s}");
    }
}
