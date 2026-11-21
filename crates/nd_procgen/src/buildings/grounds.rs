//! [r12-odd-lots] A small building on a big site (Harvey's flag on Null
//! Pointer Karaoke: "a strange building to put on a huge plot of land ...
//! most of the land a gray tile pattern"). A shop or a place to go out
//! drawn on a site several times what its use wants paved the whole site
//! as its yard and stood a small block on it, often in the middle where an
//! odd outline left room: a car park's worth of grey tiles round a bar.
//!
//! Now such a building is drawn on a plot of its use's size at the street
//! ([`front_plot`]: the frontage window round the way in, a use's depth
//! deep), and the rest of the site is laid out as grounds that fit
//! ([`draw`]): a garden with trees, a path back from the building and
//! benches along it; on the water a promenade along the shore with a rail,
//! benches facing the water and lamps; on cheap land rough grass waiting
//! for the next building. Only where the style left the site mostly bare
//! ([`MAX_COVER`]): a cinema or a tower that fills its site keeps it.
//! Everything comes from the building's seed: the same from every camera.

use super::parts::*;
use super::{Extras, Frame, Style};
use crate::palette::Palette;
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::zoning::{clip_halfplane, point_in_polygon, polygon_area, polygon_centroid, LotShore};
use nd_sim::{LandUse, Lot, Tier};
use rand::Rng;

/// A site at least this many times its use's lot area is roomy.
pub const ROOMY: f32 = 2.5;
/// The plot the building keeps, in its use's lot areas.
pub const FRONT_AREA: f32 = 1.6;
/// A drawn building covering less of a roomy site than this (the hull of
/// everything over 2.5 m) left it mostly yard: it moves to the front plot.
pub const MAX_COVER: f32 = 0.2;

/// Uses whose small buildings move to the street on a roomy site: shops
/// and places to go out.
pub fn applies(land_use: LandUse) -> bool {
    land_use.is_commercial()
}

/// Could `style` move to a front plot on a roomy site? Not what fills its
/// site by its nature or has its own ground: a merged form (a mall, a mega
/// hotel), a landmark, a car park, a plant, a station or a dock, a tower of
/// the league or an arcology, a campus annex, and not a trade that works
/// the water (a beach resort's wings run down to the sand, a boat rental's
/// pontoon off its shore).
pub fn candidate(style: Style, extras: &Extras) -> bool {
    !(extras.form.is_some()
        || extras.service.is_some()
        || extras.plant.is_some()
        || extras.parking.is_some()
        || extras.battery.is_some()
        || extras.depot
        || extras.rail.is_some()
        || extras.port.is_some()
        || extras.metro
        || extras.cable
        || extras.tower.is_some()
        || extras.megablock.is_some()
        || extras.annex.is_some()
        || extras.pier.is_some()
        || extras.boxes_of.is_some()
        || extras.business.is_some_and(nd_sim::shore::uses_water)
        || style.is_attraction()
        || style.is_mall()
        || matches!(style, Style::Beach | Style::Garden | Style::Plaza | Style::ScrubLot | Style::ParkingLot | Style::CarPark | Style::WaterfrontHotel | Style::TruckLot))
}

/// Is `lot` roomy for `land_use` ([`ROOMY`])?
pub fn roomy(lot: &Lot, land_use: LandUse) -> bool {
    applies(land_use) && lot.area() > land_use.lot_area() * ROOMY
}

/// How much of `lot` the drawn building covers: the hull of everything
/// drawn over 2.5 m (walls, roofs, signs on poles), over the lot's area.
pub fn cover(g: &Geometry, lot: &Lot) -> f32 {
    let mut pts: Vec<Vec2> = Vec::new();
    for (_, m) in g.iter() {
        pts.extend(m.positions.iter().filter(|p| p[1] > 2.5).map(|p| Vec2::new(p[0], p[2])));
    }
    let a = lot.area();
    if a <= 0.0 {
        return 1.0;
    }
    (hull_area(&pts) / a).min(1.0)
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    p.distance(nd_sim::geom::project_on_segment(p, a, b).1)
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

/// The plot a building of `land_use` keeps on a roomy `lot`: the lot cut
/// to a window of the frontage round its way in (two of the use's widths)
/// and a use's depth from the street, deeper on a narrow window so it
/// holds [`FRONT_AREA`] lot areas. Its frame runs along the lot's, so the
/// building still faces the street; it is off the water (no pier of its
/// own). `None` when the cut leaves too little or no frontage.
pub fn front_plot(lot: &Lot, land_use: LandUse) -> Option<Lot> {
    let f = Frame::from_lot(lot);
    let (hw, hd) = (f.width * 0.5, f.depth * 0.5);
    let want = land_use.lot_area();
    let ww = (land_use.lot_width() * 2.0).min(f.width);
    let mid = f.to_local(lot.entrance()).x;
    let u0 = (mid - ww * 0.5).clamp(-hw, hw - ww);
    let u1 = u0 + ww;
    let d = (want * FRONT_AREA / ww).clamp(land_use.lot_depth(), (f.depth * 0.6).max(land_use.lot_depth()));
    if d >= f.depth - 4.0 {
        return None;
    }
    let mut poly = lot.polygon.clone();
    poly = clip_halfplane(&poly, f.to_world(0.0, -hd + d), -f.inward);
    poly = clip_halfplane(&poly, f.to_world(u0, 0.0), f.along);
    poly = clip_halfplane(&poly, f.to_world(u1, 0.0), -f.along);
    if poly.len() < 3 {
        return None;
    }
    if polygon_area(&poly) < 0.0 {
        poly.reverse();
    }
    let area = polygon_area(&poly);
    if area < want * 0.6 {
        return None;
    }
    // The front edge, kept to the window.
    let keep = |p: Vec2| {
        let l = f.to_local(p);
        f.to_world(l.x.clamp(u0, u1), l.y)
    };
    let front_edge = [keep(lot.front_edge[0]), keep(lot.front_edge[1])];
    if front_edge[0].distance(front_edge[1]) < 4.0 {
        return None;
    }
    Some(Lot {
        corners: [f.to_world(u0, -hd), f.to_world(u1, -hd), f.to_world(u1, -hd + d), f.to_world(u0, -hd + d)],
        center: polygon_centroid(&poly),
        polygon: poly,
        front_edge,
        park: None,
        shore: LotShore::default(),
        ..lot.clone()
    })
}

/// The grounds round the building's front plot on `lot`: a garden (rough
/// grass on cheap land), a path back from the building, benches and trees,
/// and on the water a promenade along the shore. `seaward` is the way the
/// water lies (zero inland).
#[allow(clippy::too_many_arguments)]
pub fn draw(g: &mut Geometry, lot: &Lot, front: &Lot, tier: Tier, wear: f32, seed: u64, pal: &Palette, seaward: Vec2) {
    let mut rng = nd_sim::rng_for(seed, 0x0DD1_075);
    let w = Frame::world();
    let site = clean_poly(&lot.polygon, 0.3);
    if site.len() < 3 {
        return;
    }
    let rough = tier == Tier::Low;
    let grass = if rough { mix(pal.foliage_dry, pal.foliage, 0.35) } else { mul(pal.foliage, 0.9 - 0.2 * wear) };
    let f = Frame::from_lot(lot);
    let fr = Frame::from_lot(front);
    // The lawn: the site round the front plot, behind it and either side
    // of it, at a yard's height (over the lot's gravel, never under the
    // front plot's own yard, which would fight it for the same height).
    let [fl, frr, _, bl] = front.corners;
    let behind = clip_halfplane(&site, bl, f.inward);
    let before = clip_halfplane(&site, bl, -f.inward);
    for piece in [behind, clip_halfplane(&before, fl, -f.along), clip_halfplane(&before, frr, f.along)] {
        if piece.len() >= 3 && polygon_area(&piece).abs() > 1.0 {
            ground(g, &w, Foliage, &piece, 0.03, grass);
        }
    }
    let back_mid = fr.to_world(0.0, fr.depth * 0.5);
    let near_front = |p: Vec2, m: f32| point_in_polygon(p, &front.polygon) || distance_to_boundary(p, &front.polygon) < m && point_in_polygon(p, &offset_poly(&front.polygon, -m));
    // The way down the site: from the back of the front plot, inward, to
    // the water or the far side.
    let dir = if seaward != Vec2::ZERO && seaward.dot(f.inward) > 0.3 { seaward.normalize_or(f.inward) } else { f.inward };
    let mut len = 0.0;
    while len < 300.0 && point_in_polygon(back_mid + dir * (len + 1.0), &site) {
        len += 1.0;
    }
    let paving = mul(pal.paving, 0.95 - 0.25 * wear);
    let path_w = if rough { 0.0 } else { 2.4 };
    let path: Vec<Vec2> = if len > 6.0 && path_w > 0.0 {
        let a = back_mid - dir * 0.5;
        let b = back_mid + dir * (len - 0.5);
        let s = dir.perp() * (path_w * 0.5);
        let quad = vec![a - s, a + s, b + s, b - s];
        let quad = if polygon_area(&quad) < 0.0 { quad.into_iter().rev().collect() } else { quad };
        ground(g, &w, Paving, &quad, 0.035, paving);
        vec![a, b]
    } else {
        Vec::new()
    };
    let on_path = |p: Vec2, m: f32| path.len() == 2 && seg_dist(p, path[0], path[1]) < path_w * 0.5 + m;
    let wood = mul(pal.wood, 0.8 - 0.2 * wear);
    let steel = mul(pal.steel, 0.8);
    // Benches beside the path, facing it.
    if path.len() == 2 {
        let n = ((len - 4.0) / 11.0).floor() as i32;
        for k in 0..n.max(0) {
            let t = 6.0 + k as f32 * 11.0;
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let p = back_mid + dir * t + dir.perp() * side * (path_w * 0.5 + 0.9);
            if point_in_polygon(p, &site) && distance_to_boundary(p, &site) > 1.0 {
                let yaw = (dir.perp() * -side).y.atan2((dir.perp() * -side).x) - std::f32::consts::FRAC_PI_2;
                g.lod(Lod::Mid, |g| bench_world(g, p, yaw, wood, steel));
            }
        }
    }
    // On the water: a promenade along every edge facing it, with a rail,
    // benches looking out and lamps.
    let mut prom: Vec<(Vec2, Vec2)> = Vec::new();
    if seaward != Vec2::ZERO {
        let n = site.len();
        for i in 0..n {
            let (a, b) = (site[i], site[(i + 1) % n]);
            let e = b - a;
            let l = e.length();
            // Outward normal of a counter-clockwise outline.
            let out = Vec2::new(e.y, -e.x) / l.max(1e-3);
            if l < 4.0 || out.dot(seaward) < 0.6 || near_front(a.lerp(b, 0.5), 2.0) {
                continue;
            }
            let inn = -out * 3.2;
            let strip = vec![a, b, b + inn, a + inn];
            let strip = if polygon_area(&strip) < 0.0 { strip.into_iter().rev().collect() } else { strip };
            ground(g, &w, Paving, &strip, 0.04, mul(paving, 1.05));
            prom.push((a, b));
        }
        for (a, b) in &prom {
            let d = (*b - *a).normalize_or_zero();
            let out = Vec2::new(d.y, -d.x);
            let l = a.distance(*b);
            let (ra, rb) = (*a + d * 0.3 - out * 0.15, *b - d * 0.3 - out * 0.15);
            g.lod(Lod::Mid, |g| {
                g.part(Metal).beam(Vec3::new(ra.x, 1.0, ra.y), Vec3::new(rb.x, 1.0, rb.y), 0.04, rgba(steel));
                let posts = (l / 2.5).floor() as i32;
                for k in 0..=posts.max(1) {
                    let p = ra.lerp(rb, k as f32 / posts.max(1) as f32);
                    g.part(Metal).beam(Vec3::new(p.x, 0.0, p.y), Vec3::new(p.x, 1.0, p.y), 0.035, rgba(steel));
                }
            });
            let yaw = out.y.atan2(out.x) - std::f32::consts::FRAC_PI_2;
            let mut t = 5.0;
            let mut k = 0;
            while t < l - 3.0 {
                let p = *a + d * t - out * 2.2;
                if k % 2 == 0 {
                    g.lod(Lod::Mid, |g| bench_world(g, p, yaw, wood, steel));
                } else if !rough {
                    lamp_post_world(g, p - out * 0.4, 3.6, [1.9, 1.5, 0.9]);
                }
                t += 7.0;
                k += 1;
            }
        }
    }
    let near_prom = |p: Vec2| prom.iter().any(|(a, b)| seg_dist(p, *a, *b) < 4.5);
    // Trees over the garden, clear of the building's plot, the path and
    // the promenade; fewer and scrubbier on rough ground.
    let rest = (lot.area() - front.area()).max(0.0);
    let n = ((rest / if rough { 260.0 } else { 140.0 }) as i32).clamp(1, 28);
    let leaf = if rough { mix(pal.foliage_dry, pal.foliage, 0.5) } else { pal.foliage };
    let mut placed: Vec<Vec2> = Vec::new();
    for _ in 0..n * 3 {
        if placed.len() as i32 >= n {
            break;
        }
        let Some(p) = random_in_poly(&mut rng, &site, 2.0) else { break };
        if near_front(p, 3.5) || on_path(p, 1.6) || near_prom(p) || placed.iter().any(|q| q.distance(p) < 5.0) {
            continue;
        }
        let h = rng.random_range(4.0..7.0) * if rough { 0.7 } else { 1.0 };
        crate::climate::climate_tree(g, pal, p, h, h * 0.42, mul(leaf, rng.random_range(0.85..1.1)));
        placed.push(p);
    }
    if rough {
        // Weeds over ground waiting for its next building.
        let lc = Frame::from_lot(lot);
        for _ in 0..(rest / 90.0).min(30.0) as i32 {
            let Some(p) = random_in_poly(&mut rng, &site, 1.0) else { break };
            if near_front(p, 1.0) {
                continue;
            }
            let q = lc.to_local(p);
            lc.bx(g, Foliage, q.x, 0.25, q.y, Vec3::new(0.8, 0.5, 0.8), mul(pal.foliage_dry, 0.8));
        }
    }
}
