//! Street-level detail for the first-person walk camera: things only worth
//! drawing within a stone's throw of the eye. Shop signs and lightboxes at
//! eye level, vending machines, street food carts with steam, steam vents
//! and manholes, cables strung across the street, puddles catching the
//! neon, parked scooters, clutter, cracks and stains on the slabs.
//!
//! Everything is laid out per road segment from a seed of its own, then
//! culled to a radius around the walker, so stepping along a street keeps
//! the same carts where they were.

use crate::buildings::parts::{mul, pick, rgba, CHARCOAL, DARK};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::roads::{RoadKind, SHOULDER};
use nd_sim::world::World;
use nd_sim::zoning::SIDEWALK;
use nd_sim::{rng_for, LandUse};
use rand::Rng;
use std::collections::HashMap;

/// Pavement top, matching the street generator's kerb.
const PAVE_Y: f32 = 0.155;
const ROAD_Y: f32 = 0.05;

/// The radius the app should draw walk detail in, and how far the walker
/// may stray before it is redrawn.
pub const WALK_RADIUS: f32 = 90.0;
pub const WALK_REDRAW: f32 = 25.0;

/// What fronts a stretch of pavement.
#[derive(Clone, Copy)]
struct Frontage {
    land_use: LandUse,
    built: bool,
    s0: f32,
    s1: f32,
    side: f32,
}

/// Street-level detail within `radius` of `centre` (sim coordinates).
pub fn walk_detail(world: &World, pal: &Palette, centre: Vec2, radius: f32) -> Geometry {
    let mut g = Geometry::default();
    let roads = &world.roads;
    // Which lots front each segment, and on which side.
    let mut fronts: HashMap<u32, Vec<Frontage>> = HashMap::new();
    for lot in world.lots.values() {
        if !lot.usable() {
            continue;
        }
        let Some(seg) = roads.try_segment(lot.front_segment) else { continue };
        if !seg.is_street() {
            continue;
        }
        let (a, b) = roads.segment_points(lot.front_segment);
        let d = (b - a).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        let side = if (lot.center - a).dot(right) >= 0.0 { 1.0 } else { -1.0 };
        let (s0, s1) = {
            let p = (lot.front_edge[0] - a).dot(d);
            let q = (lot.front_edge[1] - a).dot(d);
            (p.min(q), p.max(q))
        };
        let built = lot.building.and_then(|id| world.buildings.get(id)).is_some_and(|b| b.construction.is_none());
        fronts.entry(lot.front_segment.0).or_default().push(Frontage { land_use: lot.land_use, built, s0, s1, side });
    }
    let near = |p: Vec2| p.distance(centre) < radius;
    let heights: &[f32] = if world.relief { &world.road_z } else { &[] };
    let inner: &[Vec<(f32, f32)>] = if world.relief { &world.road_inner } else { &[] };
    for (sid, seg) in roads.alive_segments() {
        // Only streets with pavements: the highway, its ramps and a
        // roundabout's ring have hard shoulders (nobody walks them), and a
        // tunnel's bore has no shops, carts or cables strung overhead.
        if seg.kind != RoadKind::Street || seg.is_motorway() || seg.ring || seg.tunnel {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let len = a.distance(b);
        if len < 8.0 {
            continue;
        }
        // Skip segments wholly out of reach.
        let (_, closest) = nd_sim::roads::project_on_segment(centre, a, b);
        if closest.distance(centre) > radius + 4.0 {
            continue;
        }
        let d = (b - a) / len;
        let right = Vec2::new(-d.y, d.x);
        let yaw = d.y.atan2(d.x);
        let hw = seg.half_width();
        let node_r = |n: nd_sim::NodeId| world.traffic.net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0);
        let trim0 = if roads.street_degree(seg.a) >= 3 { node_r(seg.a) + 1.5 } else { 1.0 };
        let trim1 = if roads.street_degree(seg.b) >= 3 { node_r(seg.b) + 1.5 } else { 1.0 };
        let mut rng = rng_for(world.config.seed ^ (sid.0 as u64).wrapping_mul(0x9e37_79b9), 41);
        let at = |s: f32, side: f32, off: f32| a + d * s + right * side * off;
        let segment_fronts = fronts.get(&sid.0).map(|v| v.as_slice()).unwrap_or(&[]);
        let frontage = |s: f32, side: f32| segment_fronts.iter().find(|f| f.side == side && s >= f.s0 - 1.0 && s <= f.s1 + 1.0).copied();
        let lv_mid = world.land_value.at((a + b) * 0.5);
        // Everything below is drawn on a flat datum and then set on the
        // road's own height along the segment (`World::road_height`), so a
        // viaduct's pavement carries its clutter up with it and a street on
        // a hillside is not buried. The renderer does not lift this mesh.
        let from = crate::street::marks(&g);
        let seg_z = |p: Vec2| if world.relief { world.road_height(sid, nd_sim::roads::project_on_segment(p, a, b).0) } else { 0.0 };

        // Things on the pavement, both sides, every few metres.
        for side in [-1.0f32, 1.0] {
            let mut s = trim0 + rng.random_range(0.0..3.0);
            while s < len - trim1 {
                let step = rng.random_range(3.5..8.0);
                let p_back = at(s, side, hw + SIDEWALK - 0.45);
                let roll: f32 = rng.random();
                let front = frontage(s, side);
                let lu = front.map(|f| f.land_use);
                let built = front.is_some_and(|f| f.built);
                let lv = world.land_value.at(p_back);
                // Facing the road from the back of the pavement.
                let to_road = -right * side;
                // Nothing on another street's lanes (a sharp fork, a tight
                // corner): the pavement is laid out from this piece alone.
                let clear = |p: Vec2| !crate::street::on_carriageway(roads, heights, inner, p, seg_z(p));
                if near(p_back) && clear(p_back) && clear(at(s, side, hw + SIDEWALK * 0.55)) {
                    match (lu, built) {
                        (Some(LandUse::Commercial | LandUse::Entertainment), true) => {
                            if roll < 0.32 {
                                shop_sign(&mut g, pal, &mut rng, p_back, d, to_road);
                            } else if roll < 0.46 && lv < 0.75 {
                                food_cart(&mut g, pal, &mut rng, at(s, side, hw + SIDEWALK * 0.55), yaw, to_road);
                            } else if roll < 0.56 {
                                vending(&mut g, pal, &mut rng, p_back, yaw, to_road);
                            } else if roll < 0.66 {
                                clutter(&mut g, pal, &mut rng, p_back, yaw, lv);
                            }
                        }
                        (Some(LandUse::Residential), _) => {
                            if roll < 0.12 {
                                vending(&mut g, pal, &mut rng, p_back, yaw, to_road);
                            } else if roll < 0.32 {
                                clutter(&mut g, pal, &mut rng, p_back, yaw, lv);
                            } else if roll < 0.40 && lv > 0.5 {
                                planter(&mut g, pal, &mut rng, p_back);
                            }
                        }
                        (Some(LandUse::Industrial), _) => {
                            if roll < 0.3 {
                                clutter(&mut g, pal, &mut rng, p_back, yaw, 0.0);
                            }
                        }
                        _ => {
                            if roll < 0.08 {
                                clutter(&mut g, pal, &mut rng, p_back, yaw, lv);
                            }
                        }
                    }
                }
                // Scooters parked in the gutter, in little rows.
                if rng.random::<f32>() < 0.12 && lv < 0.8 {
                    let n = rng.random_range(1..=4);
                    for k in 0..n {
                        let p = at(s + k as f32 * 0.9, side, hw - SHOULDER * 0.5);
                        if near(p) && s + k as f32 * 0.9 < len - trim1 {
                            scooter(&mut g, pal, &mut rng, p, yaw + std::f32::consts::FRAC_PI_2 * 0.85 * side);
                        }
                    }
                }
                // Puddles in the gutter and on sunken slabs: dark glass
                // with a smear of sign colour in it.
                let wet = 0.08 + 0.25 * (1.0 - lv).clamp(0.0, 1.0);
                if rng.random::<f32>() < wet {
                    let on_road = rng.random_bool(0.6);
                    let p = if on_road { at(s, side, hw - rng.random_range(0.3..1.2)) } else { at(s, side, hw + rng.random_range(0.8..SIDEWALK - 0.6)) };
                    if near(p) {
                        puddle(&mut g, pal, &mut rng, p, yaw, if on_road { ROAD_Y + 0.025 } else { PAVE_Y + 0.012 });
                    }
                }
                // Cracks, stains and gum on the slabs: dense where cheap.
                let crack_n = ((1.0 - lv) * 3.0).round() as i32;
                for _ in 0..crack_n {
                    let p = at(s + rng.random_range(-2.0..2.0), side, hw + rng.random_range(0.5..SIDEWALK - 0.2));
                    if near(p) {
                        slab_mark(&mut g, pal, &mut rng, p, yaw, lv);
                    }
                }
                s += step;
            }
        }
        // Steam from vents and manholes in the carriageway.
        let mut s = trim0 + rng.random_range(5.0..25.0);
        while s < len - trim1 {
            let lane = rng.random_range(-(hw - SHOULDER - 0.8).max(0.1)..(hw - SHOULDER - 0.8).max(0.2));
            let p = at(s, 1.0, lane);
            if near(p) {
                let venting = rng.random::<f32>() < 0.45 + 0.3 * (1.0 - lv_mid).clamp(0.0, 1.0);
                manhole(&mut g, pal, p, venting, &mut rng);
            }
            s += rng.random_range(25.0..55.0);
        }
        // Cables across the street on cheap blocks, a banner or lanterns now and then.
        if lv_mid < 0.65 {
            let mut s = trim0 + rng.random_range(3.0..12.0);
            while s < len - trim1 {
                let pa = at(s, -1.0, hw + SIDEWALK - 0.2);
                let pb = at(s + rng.random_range(-3.0..3.0), 1.0, hw + SIDEWALK - 0.2);
                if near((pa + pb) * 0.5) {
                    let ya = rng.random_range(6.0..8.5);
                    let yb = rng.random_range(6.0..8.5);
                    let wa = Vec3::new(pa.x, ya, pa.y);
                    let wb = Vec3::new(pb.x, yb, pb.y);
                    for k in 0..rng.random_range(1..=3) {
                        let off = Vec3::Y * (k as f32 * 0.25);
                        g.part(Matte).wire(wa + off, wb + off, rng.random_range(0.6..1.4), 0.03, rgba(DARK));
                    }
                    for (p, y) in [(pa, ya), (pb, yb)] {
                        g.part(Matte).boxed(Vec3::new(p.x, y - 0.1, p.y), Vec3::new(0.15, 0.4, 0.15), yaw, rgba(CHARCOAL));
                    }
                    match rng.random_range(0..4) {
                        0 => {
                            // Lanterns hung along the cable.
                            let col = if rng.random_bool(0.6) { [2.0, 0.35, 0.18] } else { pal.lamp };
                            for i in 1..6 {
                                let t = i as f32 / 6.0;
                                let q = wa.lerp(wb, t) - Vec3::Y * (4.0 * t * (1.0 - t) * 1.0 + 0.5);
                                g.part(Glow).boxed(q, Vec3::new(0.35, 0.5, 0.35), 0.0, rgba(col));
                            }
                        }
                        1 => {
                            // A cloth banner over the middle of the road, lettered in light.
                            let m = wa.lerp(wb, 0.5) - Vec3::Y * 1.6;
                            let across = Vec3::new(right.x, 0.0, right.y);
                            let along = Vec3::new(d.x, 0.0, d.y);
                            g.part(Tarp).decal(m, across, Vec3::Y, 4.0, 1.0, along, rgba(mul(pick(&mut rng, &pal.awning_colors), 0.9)));
                            g.part(Tarp).decal(m, -across, Vec3::Y, 4.0, 1.0, -along, rgba(mul(pick(&mut rng, &pal.awning_colors), 0.9)));
                            let sc = mul(pick(&mut rng, &pal.sign_colors), 0.8);
                            g.part(Glow).decal(m + along * 0.02, across, Vec3::Y, 3.0, 0.5, along, rgba(sc));
                            g.part(Glow).decal(m - along * 0.02, -across, Vec3::Y, 3.0, 0.5, -along, rgba(sc));
                        }
                        _ => {}
                    }
                }
                s += rng.random_range(10.0..26.0);
            }
        }
        if world.relief {
            crate::street::lift_since(&mut g, &from, |p| world.road_height(sid, nd_sim::roads::project_on_segment(p, a, b).0));
        }
    }
    g
}

/// Eye-level signage for a shop: an A-frame board, a standing lightbox or a
/// blade sign on a short pole, all lit.
fn shop_sign(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2, d: Vec2, to_road: Vec2) {
    let sc = pick(rng, &pal.sign_colors);
    let out = Vec3::new(to_road.x, 0.0, to_road.y);
    let along = Vec3::new(d.x, 0.0, d.y);
    let yaw = d.y.atan2(d.x);
    let base = Vec3::new(p.x, PAVE_Y, p.y);
    match rng.random_range(0..3) {
        0 => {
            // A-frame: two boards leaning together, a lit menu on the street face.
            let c = base + out * 0.35 + Vec3::Y * 0.5;
            g.part(Matte).boxed(c, Vec3::new(0.7, 1.0, 0.45), yaw, rgba(DARK));
            g.part(Glow).decal(c + out * 0.24, along, Vec3::Y, 0.55, 0.75, out, rgba(mul(sc, 0.8)));
            g.part(Glow).decal(c - out * 0.24, -along, Vec3::Y, 0.55, 0.75, -out, rgba(mul(sc, 0.6)));
        }
        1 => {
            // Standing lightbox, two lit faces along the pavement.
            let c = base + Vec3::Y * 1.0;
            g.part(Matte).boxed(c, Vec3::new(0.25, 2.0, 0.6), yaw, rgba(CHARCOAL));
            for s in [1.0f32, -1.0] {
                g.part(Glow).decal(c + along * (0.13 * s), out * s, Vec3::Y, 0.5, 1.7, along * s, rgba(mul(sc, 0.9)));
            }
        }
        _ => {
            // Blade sign on a pole at head height, lit both sides.
            g.part(Matte).cylinder(p, 0.05, 0.05, PAVE_Y, PAVE_Y + 3.2, 6, rgba(CHARCOAL));
            let c = base + Vec3::Y * 2.6 + out * 0.6;
            g.part(Matte).boxed(c, Vec3::new(0.12, 0.9, 1.2), yaw, rgba(DARK));
            for s in [1.0f32, -1.0] {
                g.part(Glow).decal(c + along * (0.07 * s), out * s, Vec3::Y, 1.0, 0.7, along * s, rgba(mul(sc, 1.0)));
            }
        }
    }
}

/// Vending machine against the back of the pavement, lit front to the road.
fn vending(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2, yaw: f32, to_road: Vec2) {
    let out = Vec3::new(to_road.x, 0.0, to_road.y);
    let along = Vec3::new(-to_road.y, 0.0, to_road.x);
    let n = rng.random_range(1..=2);
    for k in 0..n {
        let c = Vec3::new(p.x, PAVE_Y + 0.95, p.y) + along * (k as f32 * 1.0) - out * 0.05;
        let body = pick(rng, &pal.metal_paint);
        g.part(Metal).boxed(c, Vec3::new(0.95, 1.9, 0.75), yaw, rgba(body));
        g.part(Glow).decal(c + out * 0.39 + Vec3::Y * 0.25, along, Vec3::Y, 0.75, 0.9, out, rgba(mul(pick(rng, &pal.led_colors), 0.55)));
        g.part(Matte).decal(c + out * 0.39 - Vec3::Y * 0.55, along, Vec3::Y, 0.6, 0.25, out, rgba(DARK));
    }
}

/// Food cart: a steel cart on wheels with a striped awning, a lantern, a
/// steaming pot and stools.
fn food_cart(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2, yaw: f32, to_road: Vec2) {
    let out = Vec3::new(to_road.x, 0.0, to_road.y);
    let along = Vec3::new(-to_road.y, 0.0, to_road.x);
    let base = Vec3::new(p.x, PAVE_Y, p.y);
    g.part(Metal).boxed(base + Vec3::Y * 0.6, Vec3::new(1.8, 0.9, 0.9), yaw, rgba(mul(pal.steel, 1.1)));
    for s in [-0.7f32, 0.7] {
        g.part(Matte).boxed(base + along * s + Vec3::Y * 0.2, Vec3::new(0.4, 0.4, 0.1), yaw, rgba(DARK));
    }
    // Poles and awning.
    for s in [-0.85f32, 0.85] {
        g.part(Matte).boxed(base + along * s + Vec3::Y * 1.5, Vec3::new(0.05, 1.8, 0.05), yaw, rgba(CHARCOAL));
    }
    g.part(Tarp).boxed(base + Vec3::Y * 2.35 + out * 0.2, Vec3::new(2.1, 0.08, 1.4), yaw, rgba(pick(rng, &pal.awning_colors)));
    g.part(Glow).decal(base + Vec3::Y * 2.2 + out * 0.91, along, Vec3::Y, 1.8, 0.22, out, rgba(mul(pick(rng, &pal.sign_colors), 0.9)));
    g.part(Glow).boxed(base + Vec3::Y * 2.0 + out * 0.5, Vec3::new(0.3, 0.4, 0.3), 0.0, rgba(if rng.random_bool(0.5) { [2.0, 0.4, 0.2] } else { pal.lamp }));
    // Pot and steam.
    let pot = base + Vec3::Y * 1.15 - along * 0.4;
    g.part(Metal).cylinder(Vec2::new(pot.x, pot.z), 0.25, 0.25, pot.y - 0.1, pot.y + 0.15, 8, rgba(mul(pal.steel, 0.8)));
    steam(g, pot + Vec3::Y * 0.2, 0.25, 3);
    // Stools on the pavement.
    for k in 0..rng.random_range(1..=3) {
        let q = base + out * 1.0 + along * (k as f32 * 0.6 - 0.6);
        g.part(Matte).boxed(q + Vec3::Y * 0.22, Vec3::new(0.3, 0.44, 0.3), 0.0, rgba([0.75, 0.16, 0.12]));
    }
}

/// Steam: a few thin, faint wisps curling up, unlit so they read as a
/// haze at night. Solid puffs looked like white boxes; wisps do not.
fn steam(g: &mut Geometry, at: Vec3, r: f32, puffs: i32) {
    for k in 0..puffs {
        let t = k as f32;
        let a = t * 2.1;
        let off = Vec3::new(a.cos(), 0.0, a.sin()) * r * 0.5;
        let c = at + off + Vec3::Y * (0.5 + t * 0.35);
        let h = 0.9 + 0.3 * t;
        let col = rgba(mul([0.16, 0.17, 0.19], 1.0 - 0.15 * t));
        for (along, out) in [(Vec3::X, Vec3::Z), (Vec3::Z, -Vec3::X)] {
            g.part(Glow).decal(c, along, Vec3::Y, 0.12 + 0.05 * t, h, out, col);
            g.part(Glow).decal(c, -along, Vec3::Y, 0.12 + 0.05 * t, h, -out, col);
        }
    }
}

/// Manhole cover in the carriageway, venting steam on cold nights.
fn manhole(g: &mut Geometry, pal: &Palette, p: Vec2, venting: bool, rng: &mut impl Rng) {
    let ring: Vec<Vec2> = (0..10).map(|k| p + Vec2::from_angle(k as f32 / 10.0 * std::f32::consts::TAU) * 0.45).collect();
    g.part(Metal).ground_polygon(&ring, ROAD_Y + 0.03, rgba(mul(pal.steel, 0.45)));
    if venting {
        steam(g, Vec3::new(p.x, ROAD_Y, p.y), rng.random_range(0.35..0.6), 4);
    }
}

/// A shallow puddle: dark glossy glass on the ground with a streak of
/// reflected sign colour.
fn puddle(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2, yaw: f32, y: f32) {
    let r = rng.random_range(0.4..1.3);
    let squash = rng.random_range(0.35..0.7);
    let rot = yaw + rng.random_range(-0.4..0.4);
    let poly: Vec<Vec2> = (0..9)
        .map(|k| {
            let a = k as f32 / 9.0 * std::f32::consts::TAU;
            let wob = 1.0 + 0.18 * ((a * 3.0 + r * 7.0).sin());
            let (x, z) = (a.cos() * r * wob, a.sin() * r * squash * wob);
            p + Vec2::new(x * rot.cos() - z * rot.sin(), x * rot.sin() + z * rot.cos())
        })
        .collect();
    g.part(Glass).ground_polygon(&poly, y, rgba(mul(pal.glass_dark, 0.6)));
    if rng.random_bool(0.7) {
        let sc = mul(pick(rng, &pal.sign_colors), rng.random_range(0.18..0.35));
        let along = Vec3::new(rot.cos(), 0.0, rot.sin());
        let across = Vec3::new(-rot.sin(), 0.0, rot.cos());
        g.part(Glow).decal(Vec3::new(p.x, y + 0.004, p.y), along, across, r * 1.1, r * squash * 0.35, Vec3::Y, rgba(sc));
    }
}

/// A crack, a stain or a blob of gum on the pavement slabs.
fn slab_mark(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2, yaw: f32, lv: f32) {
    let a = yaw + rng.random_range(-1.2..1.2);
    let along = Vec3::new(a.cos(), 0.0, a.sin());
    let across = Vec3::new(-a.sin(), 0.0, a.cos());
    let c = Vec3::new(p.x, PAVE_Y + 0.006, p.y);
    match rng.random_range(0..4) {
        0 | 1 => {
            // Crack with a branch.
            let l = rng.random_range(0.5..1.8);
            let col = rgba(mul(pal.paving, 0.3));
            g.part(Matte).decal(c, along, across, l, 0.035, Vec3::Y, col);
            let b = a + rng.random_range(0.5..1.1);
            g.part(Matte).decal(c + along * l * 0.3, Vec3::new(b.cos(), 0.0, b.sin()), Vec3::new(-b.sin(), 0.0, b.cos()), l * 0.45, 0.03, Vec3::Y, col);
        }
        2 => {
            // Oil or water stain.
            let r = rng.random_range(0.3..0.9);
            g.part(Matte).decal(c, along, across, r, r * 0.7, Vec3::Y, rgba(mul(pal.paving, 0.55 + 0.2 * lv)));
        }
        _ => {
            // Gum: tiny dark spots.
            for _ in 0..4 {
                let q = c + along * rng.random_range(-0.4..0.4) + across * rng.random_range(-0.4..0.4);
                g.part(Matte).decal(q, along, across, 0.05, 0.05, Vec3::Y, rgba(mul(pal.paving, 0.35)));
            }
        }
    }
}

/// Scooter on its stand: body, seat, wheels and bars.
fn scooter(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2, yaw: f32) {
    let dir = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let col = pick(rng, &pal.car_colors);
    let base = Vec3::new(p.x, ROAD_Y, p.y);
    g.part(Panel).boxed(base + Vec3::Y * 0.5, Vec3::new(1.2, 0.35, 0.32), yaw, rgba(col));
    g.part(Matte).boxed(base + Vec3::Y * 0.78 - dir * 0.15, Vec3::new(0.6, 0.12, 0.28), yaw, rgba(DARK));
    for s in [-0.6f32, 0.6] {
        g.part(Matte).boxed(base + dir * s + Vec3::Y * 0.25, Vec3::new(0.45, 0.45, 0.1), yaw, rgba(DARK));
    }
    g.part(Matte).boxed(base + dir * 0.55 + Vec3::Y * 0.95, Vec3::new(0.08, 0.55, 0.08), yaw, rgba(CHARCOAL));
    g.part(Matte).boxed(base + dir * 0.55 + Vec3::Y * 1.2, Vec3::new(0.06, 0.06, 0.6), yaw, rgba(CHARCOAL));
    g.part(Glow).boxed(base + dir * 0.62 + Vec3::Y * 0.8, Vec3::new(0.03, 0.07, 0.09), yaw, rgba(mul(pal.headlight, 0.12)));
}

/// Pavement clutter by how well off the street is: bags, boxes and crates,
/// a bicycle, cones, drums.
fn clutter(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2, yaw: f32, lv: f32) {
    let base = Vec3::new(p.x, PAVE_Y, p.y);
    let dir = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    match rng.random_range(0..5) {
        0 => {
            // Bin bags piled against the wall.
            for k in 0..rng.random_range(2..=5) {
                let q = base + dir * (k as f32 * 0.45 - 0.6) + Vec3::Y * 0.22;
                g.part(Tarp).boxed(q, Vec3::new(0.5, 0.45, 0.45), yaw + k as f32, rgba([0.06, 0.06, 0.07]));
            }
        }
        1 => {
            // Cardboard and crates.
            for k in 0..rng.random_range(1..=3) {
                let s = rng.random_range(0.35..0.6);
                g.part(Wood).boxed(base + dir * (k as f32 * 0.55) + Vec3::Y * (s * 0.5), Vec3::splat(s), yaw + rng.random_range(-0.3..0.3), rgba(mul(pal.plywood, 0.85)));
            }
            if rng.random_bool(0.5) {
                g.part(Tarp).boxed(base + Vec3::Y * 0.75, Vec3::new(0.4, 0.3, 0.4), yaw, rgba(mul(pick(rng, &pal.tarp_colors), 0.9)));
            }
        }
        2 => {
            // A bicycle leaning on the wall.
            for s in [-0.5f32, 0.5] {
                g.part(Matte).boxed(base + dir * s + Vec3::Y * 0.35, Vec3::new(0.66, 0.66, 0.04), yaw, rgba(DARK));
            }
            g.part(Matte).beam(base + Vec3::Y * 0.4 - dir * 0.5, base + Vec3::Y * 0.85 + dir * 0.35, 0.05, rgba(pick(rng, &pal.car_colors)));
        }
        3 if lv < 0.5 => {
            // Rusted drum with a fire in it, or traffic cones.
            if rng.random_bool(0.4) {
                g.part(Rust).cylinder(p, 0.3, 0.3, PAVE_Y, PAVE_Y + 0.9, 10, rgba(pal.rust));
                g.part(Glow).boxed(base + Vec3::Y * 1.0, Vec3::new(0.3, 0.25, 0.3), 0.0, rgba([2.0, 0.9, 0.3]));
            } else {
                for k in 0..3 {
                    let q = p + Vec2::new(dir.x, dir.z) * (k as f32 * 0.6);
                    g.part(Matte).cylinder(q, 0.16, 0.03, PAVE_Y, PAVE_Y + 0.6, 6, rgba([1.0, 0.35, 0.05]));
                }
            }
        }
        _ => {
            // Plastic crates of bottles.
            let col = pick(rng, &pal.awning_colors);
            for k in 0..rng.random_range(1..=4) {
                g.part(Matte).boxed(base + Vec3::Y * (0.15 + 0.3 * k as f32), Vec3::new(0.5, 0.3, 0.35), yaw, rgba(col));
            }
        }
    }
}

/// A planter with a shrub, for the better streets.
fn planter(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, p: Vec2) {
    g.part(Concrete).cylinder(p, 0.45, 0.38, PAVE_Y, PAVE_Y + 0.6, 8, rgba(mul(pal.paving, 1.1)));
    g.part(Foliage).cylinder(p, 0.45, 0.2, PAVE_Y + 0.6, PAVE_Y + rng.random_range(1.1..1.6), 7, rgba(mul(pal.foliage, rng.random_range(0.85..1.15))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::world::WorldConfig;

    fn town() -> World {
        let mut w = World::new(WorldConfig::default());
        for i in -1..=1 {
            let c = i as f32 * 120.0;
            w.add_road(&[Vec2::new(-200.0, c), Vec2::new(200.0, c)], 1, 1);
            w.add_road(&[Vec2::new(c, -200.0), Vec2::new(c, 200.0)], 1, 1);
        }
        let sq = |x: f32, y: f32| vec![Vec2::new(x, y), Vec2::new(x + 110.0, y), Vec2::new(x + 110.0, y + 110.0), Vec2::new(x, y + 110.0)];
        w.add_zone(sq(5.0, 5.0), LandUse::Commercial);
        w.add_zone(sq(-115.0, 5.0), LandUse::Residential);
        w.add_zone(sq(5.0, -115.0), LandUse::Entertainment);
        w.add_zone(sq(-115.0, -115.0), LandUse::Industrial);
        let lots: Vec<_> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
        for id in lots {
            w.build_on(id);
        }
        let ids: Vec<_> = w.buildings.keys().collect();
        for id in ids {
            w.finish_construction(id);
        }
        w
    }

    #[test]
    fn walk_detail_is_local_bounded_and_stable() {
        let w = town();
        let pal = Palette::default();
        let here = Vec2::new(10.0, 3.0);
        let a = walk_detail(&w, &pal, here, WALK_RADIUS);
        let b = walk_detail(&w, &pal, here, WALK_RADIUS);
        assert!(a == b, "walk detail must be deterministic");
        let tris = a.triangle_count();
        assert!(tris > 500, "a commercial crossing should have street life: {tris} triangles");
        assert!(tris < 60_000, "walk detail over budget: {tris} triangles");
        // Set on the streets' heights (this town stands on the hills).
        let (lo, hi) = w.road_z.iter().fold((f32::MAX, f32::MIN), |(lo, hi), z| (lo.min(*z), hi.max(*z)));
        for (s, m) in a.iter() {
            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{s:?} has NaN");
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{s:?} bad index");
            for p in &m.positions {
                assert!(Vec2::new(p[0], p[2]).distance(here) < WALK_RADIUS + 25.0, "{s:?} drawn far from the walker");
                assert!(p[1] > lo - 0.05 && p[1] < hi + 12.0, "{s:?} at height {} (streets {lo}..{hi})", p[1]);
            }
        }
        // Walking along keeps what was already there: the overlap of two
        // nearby views has the same detail.
        let near = walk_detail(&w, &pal, here + Vec2::new(4.0, 0.0), WALK_RADIUS);
        let small_a = walk_detail(&w, &pal, here, 20.0).triangle_count();
        let small_b = walk_detail(&w, &pal, here + Vec2::new(0.01, 0.0), 20.0).triangle_count();
        assert_eq!(small_a, small_b);
        assert!(near.triangle_count() > 0);
        // Nothing far from any street.
        let empty = walk_detail(&w, &pal, Vec2::new(5000.0, 5000.0), WALK_RADIUS);
        assert_eq!(empty.triangle_count(), 0);
    }
}
