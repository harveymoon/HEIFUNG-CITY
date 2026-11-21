//! [r12-odd-lots] Odd lots in a save, as the renderer draws them (Harvey's
//! Null Pointer Karaoke: a small venue on a huge waterfront plot, most of it
//! grey paving, a pier for no reason, a diagonal cut). Every building is
//! drawn as the renderer draws it and measured:
//!
//! - **huge**: a shop or leisure building on a site over 3x its use's lot
//!   area (`LandUse::lot_area`), with its area per job;
//! - **filler**: a site over 1,200 m² of which the building (everything
//!   drawn over 2.5 m) covers under a fifth: the rest is yard paving;
//! - **set back**: the building's nearest wall over 12 m from the street;
//! - **dock**: something drawn out over the water off a waterfront lot, by
//!   a use that does not use the water (`odd`) or one that does (`ok`);
//! - **shape**: a sliver (under 5 m thick), a notch (hull fill under 0.8),
//!   a diagonal cut (the outline fills under 65% of its frame), vertices
//!   doubled within 1.5 m, an outline that crosses itself, or ground drawn
//!   over 1.5 m off the lot inland (`spill`: a yard spilling onto the
//!   neighbours).
//!
//! `cargo run --profile fast -p nd_procgen --example odd_lots_probe -- save.ron [top=12] [x y]`
//! (with `x y`: only the building on the lot there, in detail). Not part of
//! the game.
//!
//! - **seam**: the lot's gravel floor as the renderer lays it rising over
//!   the building's yard (a wedge of gravel across the plot: Harvey's
//!   "large triangular cut through it in a diagonal"). The floor is set on
//!   the pad (`render/lots.rs`); before r12-odd-lots it was lifted onto the
//!   ground and pavements vertex by vertex, and a street-side vertex at
//!   pavement height tilted a whole triangle of it up through the yard.
//!   This counts the lots where the old lift did that (`FILL_AT_PAD=1`
//!   once the game sets floors on the pad: then nothing rises over it).
use glam::Vec2;
use nd_procgen::{BuildingRules, Extras, Palette, StyleBook};
use nd_sim::geom::{distance_to_outline, point_in_polygon, segments_cross};
use nd_sim::zoning::{hull_fill, polygon_area, site_thickness};
use nd_sim::{BuildingId, LandUse, World};
use slotmap::Key;

fn key(id: BuildingId) -> String {
    let f = id.data().as_ffi();
    format!("{}:{}", f & 0xffff_ffff, f >> 32)
}

#[derive(Default, Clone)]
struct Row {
    id: String,
    name: String,
    style: String,
    kind: String,
    pos: Vec2,
    area: f32,
    ratio: f32,
    per_job: f32,
    cover: f32,
    setback: f32,
    dock: Option<bool>,
    shape: Vec<&'static str>,
    seam: f32,
    huge: bool,
    filler: bool,
}

fn hull_area(pts: &[Vec2]) -> f32 {
    let mut p = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| a.distance(*b) < 0.01);
    if p.len() < 3 {
        return 0.0;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut h: Vec<Vec2> = Vec::new();
    for pass in 0..2 {
        let start = h.len();
        let it: Vec<Vec2> = if pass == 0 { p.clone() } else { p.iter().rev().copied().collect() };
        for q in it {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(q);
        }
        h.pop();
    }
    polygon_area(&h).abs()
}

/// Does the trade use the water (a jetty, moorings, boats, fish)?
fn water_use(w: &World, id: BuildingId) -> bool {
    let b = &w.buildings[id];
    match b.business.as_ref() {
        None => b.land_use == LandUse::Residential || b.port.is_some(),
        Some(z) => nd_sim::shore::uses_water(z.kind),
    }
}

fn shape_flags(poly: &[Vec2], w: f32, d: f32) -> Vec<&'static str> {
    let mut v = Vec::new();
    let a = polygon_area(poly).abs();
    if site_thickness(poly) < 5.0 {
        v.push("sliver");
    }
    if hull_fill(poly) < 0.8 {
        v.push("notch");
    }
    if w * d > 0.0 && a / (w * d) < 0.65 {
        v.push("diagonal");
    }
    let n = poly.len();
    if (0..n).any(|i| poly[i].distance(poly[(i + 1) % n]) < 1.5) {
        v.push("doubled");
    }
    let crosses = (0..n).any(|i| (i + 2..n).any(|j| (j + 1) % n != i && segments_cross(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n])));
    if crosses {
        v.push("crossing");
    }
    v
}

fn measure(w: &World, id: BuildingId, rules: &BuildingRules, pal: &Palette, book: &StyleBook) -> Option<Row> {
    let b = w.buildings.get(id)?;
    let fp = w.footprint(id)?;
    let lot = w.lots.get(b.lot)?;
    let extras = Extras { powered: b.powered, park: fp.park.as_ref(), construction: b.construction.as_ref().map(|c| c.progress), ..nd_procgen::style_extras(w, id, book.neighbours(id)) };
    let g = nd_procgen::generate_building(&fp, b.land_use, b.floors, b.tier, b.condition, b.seed, rules, pal, &extras);
    let mut tall = Vec::new();
    let mut off_water = 0;
    let mut spill = 0;
    let sea = lot.shore.seaward;
    let wet = lot.shore.is_waterfront() && sea != Vec2::ZERO;
    let mut look = |m: &nd_procgen::MeshData| {
        for p in &m.positions {
            let q = Vec2::new(p[0], p[2]);
            if p[1] > 2.5 {
                tall.push(q);
            }
            if p[1] < 0.1 && p[1] > -0.1 && !point_in_polygon(q, &fp.polygon) && distance_to_outline(q, &fp.polygon) > 1.5 && !(wet && (q - fp.center).dot(sea) > 0.0) {
                spill += 1;
            }
            if wet && p[1] < 1.5 && !point_in_polygon(q, &fp.polygon) && distance_to_outline(q, &fp.polygon) > 2.0 && (q - fp.center).dot(sea) > 0.0 {
                off_water += 1;
            }
        }
    };
    for (_, m) in g.iter() {
        look(m);
    }
    for k in 0..2 {
        for m in &g.detail[k] {
            look(m);
        }
    }
    let area = fp.area();
    let lu = b.land_use;
    let ratio = area / lu.lot_area();
    let cover = if area > 0.0 { (hull_area(&tall) / area).min(1.0) } else { 0.0 };
    let (fa, fb) = (lot.front_edge[0], lot.front_edge[1]);
    let setback = tall
        .iter()
        .filter(|q| point_in_polygon(**q, &fp.polygon))
        .map(|q| {
            let (_, on) = nd_sim::geom::project_on_segment(*q, fa, fb);
            q.distance(on)
        })
        .fold(f32::MAX, f32::min);
    let jobs = b.jobs.max(b.homes).max(1) as f32;
    // How far the lot floor would stand over the yard at its highest vertex.
    let pad = w.lot_pad(b.lot);
    let at_pad = std::env::var("FILL_AT_PAD").is_ok();
    let seam = b
        .plot
        .lots(b.lot)
        .iter()
        .filter_map(|l| w.lots.get(*l))
        .flat_map(|l| l.polygon.iter())
        .map(|v| if at_pad { pad } else { w.surface_height_near(*v, w.ground_height(*v)) } - pad)
        .fold(0.0f32, f32::max);
    let small_use = lu.is_commercial() && b.plot.form.is_none() && !w.is_city_building(id) && b.port.is_none();
    Some(Row {
        id: key(id),
        name: b.name.clone(),
        style: book.style(id).map_or("-".into(), |s| s.def().label.to_string()),
        kind: b.business.as_ref().map_or(lu.label().to_string(), |z| z.kind.label().to_string()),
        pos: fp.center,
        area,
        ratio,
        per_job: area / jobs,
        cover,
        setback: if setback == f32::MAX { -1.0 } else { setback },
        dock: (off_water > 4).then(|| water_use(w, id)),
        shape: {
            let mut v = shape_flags(&fp.polygon, fp.width(), fp.depth());
            if spill > 0 {
                v.push("spill");
            }
            v
        },
        seam,
        huge: small_use && ratio > 3.0,
        filler: small_use && area > 1200.0 && cover < 0.2,
    })
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = World::read_save(&a[1]).expect("load");
    let top: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(12);
    let at = (a.len() >= 5).then(|| Vec2::new(a[3].parse().unwrap(), a[4].parse().unwrap()));
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut book = StyleBook::default();
    book.refresh(&w, &rules);
    let ids: Vec<BuildingId> = w
        .buildings
        .iter()
        .filter(|(_, b)| b.construction.is_none() && b.demolition.is_none() && b.land_use != LandUse::Park && !b.is_tenant())
        .filter(|(id, _)| at.is_none_or(|p| w.footprint(*id).is_some_and(|l| l.contains(p))))
        .map(|(id, _)| id)
        .collect();
    let threads = 6;
    let chunk = ids.len().div_ceil(threads).max(1);
    let rows: Vec<Row> = std::thread::scope(|s| {
        let hs: Vec<_> = ids.chunks(chunk).map(|c| s.spawn(|| c.iter().filter_map(|id| measure(&w, *id, &rules, &pal, &book)).collect::<Vec<_>>())).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let line = |r: &Row| {
        format!(
            "{:>9} {:<28} {:<20} {:<22} ({:>5.0},{:>5.0}) {:>6.0} m2 x{:>4.1} {:>5.0} m2/job cover {:.2} setback {:>4.1} dock {:<4} {}",
            r.id,
            r.name.chars().take(28).collect::<String>(),
            r.kind.chars().take(20).collect::<String>(),
            r.style.chars().take(22).collect::<String>(),
            r.pos.x,
            r.pos.y,
            r.area,
            r.ratio,
            r.per_job,
            r.cover,
            r.setback,
            r.dock.map_or("-", |ok| if ok { "ok" } else { "ODD" }),
            r.shape.join(",")
        )
    };
    if let Some(p) = at {
        rows.iter().for_each(|r| println!("{}", line(r)));
        if std::env::var("HEIGHTS").is_ok() {
            // Where the graded ground stands over the lot's pad (it shows
            // through the yard): a map of the lot, 3 m cells.
            for id in &ids {
                let b = &w.buildings[*id];
                let fp = w.footprint(*id).unwrap();
                let pad = w.lot_pad(b.lot);
                let (lo, hi) = nd_sim::geom::bounds(&fp.polygon);
                println!("pad {pad:.2}  (rows north to south, '#' ground > pad+0.05, '+' > pad+0.5, '.' at or below, ' ' off the lot)");
                let mut y = hi.y;
                while y >= lo.y {
                    let mut row = String::new();
                    let mut x = lo.x;
                    while x <= hi.x {
                        let q = Vec2::new(x, y);
                        row.push(if !point_in_polygon(q, &fp.polygon) { ' ' } else {
                            let h = w.ground_height(q) - pad;
                            if h > 0.5 { '+' } else if h > 0.05 { '#' } else { '.' }
                        });
                        x += 3.0;
                    }
                    println!("{y:>6.0} {row}");
                    y -= 3.0;
                }
            }
        }
        if let Ok(path) = std::env::var("DUMP") {
            dump(&w, p, &rules, &pal, &book, &path);
        }
        return;
    }
    let count = |f: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| f(r)).count();
    println!("day {} buildings measured {}", w.clock.day(), rows.len());
    println!("huge (shop/leisure site > 3x its use's lot): {}", count(&|r| r.huge));
    println!("filler (site > 1200 m2, building covers < 20%, shop/leisure): {}", count(&|r| r.filler));
    println!("set back > 12 m from the street (shop/leisure): {}", count(&|r| r.setback > 12.0 && r.kind != "Residential" && (r.huge || r.filler || r.ratio > 1.5)));
    println!("set back > 12 m from the street (all): {}", count(&|r| r.setback > 12.0));
    println!("docks drawn: {} (odd: {}, water uses: {})", count(&|r| r.dock.is_some()), count(&|r| r.dock == Some(false)), count(&|r| r.dock == Some(true)));
    println!("seam (lot floor over the yard by > 5 cm): {} (> 0.5 m: {})", count(&|r| r.seam > 0.05), count(&|r| r.seam > 0.5));
    // The size rule on the water: parcels the next cut would halve.
    let squat: Vec<&nd_sim::zoning::Parcel> = w.parcels.parcels.iter().filter(|p| p.water_frontage >= nd_sim::zoning::LotShore::MIN_FRONTAGE && p.area > nd_sim::zoning::MAX_PARCEL_AREA && nd_sim::zoning::shore_cap(&p.polygon) <= nd_sim::zoning::MAX_PARCEL_AREA).collect();
    println!(
        "squat shore parcels over {:.0} m2 (halved road-to-water at the next cut): {} ({} under a building, {} m2 in all)",
        nd_sim::zoning::MAX_PARCEL_AREA,
        squat.len(),
        squat.iter().filter(|p| p.lot.and_then(|l| w.lots.get(l)).is_some_and(|l| l.building.is_some())).count(),
        squat.iter().map(|p| p.area).sum::<f32>().round()
    );
    for f in ["sliver", "notch", "diagonal", "doubled", "crossing", "spill"] {
        println!("shape {f}: {}", count(&|r| r.shape.contains(&f)));
    }
    let mut v: Vec<&Row> = rows.iter().filter(|r| r.huge || r.filler).collect();
    v.sort_by(|a, b| b.ratio.total_cmp(&a.ratio));
    println!("\n-- top huge / filler --");
    v.iter().take(top).for_each(|r| println!("{}", line(r)));
    let mut v: Vec<&Row> = rows.iter().filter(|r| r.dock == Some(false)).collect();
    v.sort_by(|a, b| b.area.total_cmp(&a.area));
    println!("\n-- odd docks --");
    v.iter().take(top).for_each(|r| println!("{}", line(r)));
    let mut v: Vec<&Row> = rows.iter().filter(|r| r.seam > 0.05).collect();
    v.sort_by(|a, b| b.seam.total_cmp(&a.seam));
    println!("
-- seams (lot floor over the yard) --");
    v.iter().take(top).for_each(|r| println!("{} seam {:.2} m", line(r), r.seam));
    let mut v: Vec<&Row> = rows.iter().filter(|r| r.setback > 12.0).collect();
    v.sort_by(|a, b| b.setback.total_cmp(&a.setback));
    println!("\n-- set back --");
    v.iter().take(top).for_each(|r| println!("{}", line(r)));
    let mut v: Vec<&Row> = rows.iter().filter(|r| r.shape.iter().any(|s| *s == "diagonal" || *s == "crossing" || *s == "sliver" || *s == "spill")).collect();
    v.sort_by(|a, b| b.area.total_cmp(&a.area));
    println!("\n-- odd shapes --");
    v.iter().take(top).for_each(|r| println!("{}", line(r)));
}

/// `DUMP=file.json` with `x y`: the lots within 60 m (outline, building,
/// footprint) and every building's ground-level triangles by surface, for
/// a plot of what covers what.
fn dump(w: &World, p: Vec2, rules: &BuildingRules, pal: &Palette, book: &StyleBook, path: &str) {
    let pts = |v: &[Vec2]| v.iter().map(|q| format!("[{:.2},{:.2}]", q.x, q.y)).collect::<Vec<_>>().join(",");
    let mut out = String::from("{\"lots\":[");
    let mut first = true;
    for (lid, l) in w.lots.iter() {
        if l.center.distance(p) > 70.0 {
            continue;
        }
        if !first {
            out.push(',');
        }
        first = false;
        let name = l.building.and_then(|b| w.buildings.get(b)).map_or(String::new(), |b| b.name.replace('"', ""));
        out.push_str(&format!("{{\"id\":\"{:?}\",\"name\":\"{}\",\"poly\":[{}]}}", lid, name, pts(&l.polygon)));
    }
    out.push_str("],\"buildings\":[");
    let mut first = true;
    for (id, b) in w.buildings.iter() {
        let Some(fp) = w.footprint(id) else { continue };
        if fp.center.distance(p) > 70.0 || b.is_tenant() {
            continue;
        }
        let extras = Extras { powered: b.powered, park: fp.park.as_ref(), ..nd_procgen::style_extras(w, id, book.neighbours(id)) };
        let g = nd_procgen::generate_building(&fp, b.land_use, b.floors, b.tier, b.condition, b.seed, rules, pal, &extras);
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&format!("{{\"name\":\"{}\",\"fp\":[{}],\"corners\":[{}],\"ground\":{{", b.name.replace('"', ""), pts(&fp.polygon), pts(&fp.corners)));
        let mut fs = true;
        for (s, m) in g.iter() {
            let mut tris = Vec::new();
            for t in m.indices.chunks(3) {
                let v: Vec<[f32; 3]> = t.iter().map(|i| m.positions[*i as usize]).collect();
                if v.iter().all(|q| q[1] < 0.3) {
                    tris.push(format!("[[{:.2},{:.2}],[{:.2},{:.2}],[{:.2},{:.2}]]", v[0][0], v[0][2], v[1][0], v[1][2], v[2][0], v[2][2]));
                }
            }
            if tris.is_empty() {
                continue;
            }
            if !fs {
                out.push(',');
            }
            fs = false;
            out.push_str(&format!("\"{:?}\":[{}]", s, tris.join(",")));
        }
        out.push_str("}}");
    }
    out.push_str("]}");
    std::fs::write(path, out).unwrap();
}
