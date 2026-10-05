//! Streets: carriageway, kerbs and pavements, markings and crossings,
//! junction pads, lamps, signal posts, bins, benches, shelters, bollards,
//! utility poles with sagging wires, and trees where the land is worth it.

use crate::buildings::parts::{bench_world, bin_world, bollard_world, lamp_post_world, mul, rgba, tree_world, CHARCOAL, DARK};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::land_value::LandValue;
use nd_sim::roads::{RoadNetwork, LANE_WIDTH, SHOULDER};
use nd_sim::traffic::{LaneId, LaneKind, LaneNetwork};
use nd_sim::zoning::SIDEWALK;
use nd_sim::rng_for;
use rand::Rng;

pub struct StreetInput<'a> {
    pub roads: &'a RoadNetwork,
    pub net: &'a LaneNetwork,
    pub land_value: &'a LandValue,
    pub palette: &'a Palette,
    pub seed: u64,
}

/// Where a signal lamp hangs over the end of a lane, for the renderer to
/// swap its colour at runtime.
pub struct SignalPost {
    pub lane: LaneId,
    pub lamp: Vec3,
    pub heading: f32,
}

const KERB_H: f32 = 0.16;
const ASPHALT_Y: f32 = 0.05;
const MARK_Y: f32 = 0.085;

pub fn generate_street(inp: &StreetInput) -> Geometry {
    let mut g = Geometry::default();
    let pal = inp.palette;
    let roads = inp.roads;
    let net = inp.net;
    let asphalt_c = rgba(pal.asphalt);
    let edge_c = rgba(pal.lane_marker);
    let centre_c = rgba(pal.centre_line);
    let divider_c = rgba(pal.lane_divider);
    let paving_c = rgba(pal.paving);
    let kerb_c = rgba(pal.curb);
    let node_r = |n: nd_sim::NodeId| net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0);

    for (sid, seg) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        let len = a.distance(b);
        if len < 0.5 {
            continue;
        }
        let d = (b - a) / len;
        let right = Vec2::new(-d.y, d.x);
        let hw = seg.half_width();
        let mut rng = rng_for(inp.seed ^ (sid.0 as u64 * 7919), 3);
        g.part(Asphalt).ground_strip(&[a, b], hw * 2.0, ASPHALT_Y, asphalt_c);

        // Edge lines and lane boundaries.
        let e = right * (hw - SHOULDER * 0.5);
        g.part(Glow).ground_strip(&[a + e, b + e], 0.18, MARK_Y, edge_c);
        g.part(Glow).ground_strip(&[a - e, b - e], 0.18, MARK_Y, edge_c);
        let total = seg.lanes() as f32;
        for k in 1..seg.lanes() as u32 {
            let off = (k as f32 - total * 0.5) * LANE_WIDTH;
            let p = right * off;
            let is_centre = seg.lanes_fwd > 0 && seg.lanes_bwd > 0 && k == seg.lanes_bwd as u32;
            if is_centre {
                g.part(Glow).ground_strip(&[a + p + right * 0.18, b + p + right * 0.18], 0.14, MARK_Y, centre_c);
                g.part(Glow).ground_strip(&[a + p - right * 0.18, b + p - right * 0.18], 0.14, MARK_Y, centre_c);
            } else {
                dashes(g.part(Glow), a + p, b + p, 0.16, MARK_Y, 3.0, 4.0, divider_c);
            }
        }

        // Kerbs and pavements between the junction pads.
        let ra = node_r(seg.a);
        let rb = node_r(seg.b);
        let t0 = ra + 0.6;
        let t1 = len - rb - 0.6;
        if t1 - t0 < 1.5 {
            continue;
        }
        let pa = a + d * t0;
        let pb = a + d * t1;
        for side in [-1.0f32, 1.0] {
            g.part(Concrete).strip_box(pa, pb, side * (hw + 0.15), 0.3, 0.0, KERB_H, kerb_c);
            let pw = SIDEWALK - 0.3;
            g.part(Paving).strip_box(pa, pb, side * (hw + 0.3 + pw * 0.5), pw, 0.0, KERB_H - 0.01, paving_c);
        }

        // Furniture along the pavement.
        let lv_mid = inp.land_value.at((a + b) * 0.5);
        let lamp_off = hw + 0.95;
        let back_off = hw + SIDEWALK - 0.7;
        let mut t = t0 + 6.0;
        let mut side = 1.0;
        let mut lamp_ts: Vec<(f32, f32)> = Vec::new();
        while t < t1 - 4.0 {
            let p = a + d * t + right * side * lamp_off;
            lamp_post_world(&mut g, p, 7.0, pal.street_lamp);
            lamp_ts.push((t, side));
            t += 28.0;
            side = -side;
        }
        // Trees on valuable streets, in planters at the back of the pavement.
        if lv_mid > 0.42 {
            let mut t = t0 + 4.0;
            while t < t1 - 3.0 {
                for side in [-1.0f32, 1.0] {
                    if lamp_ts.iter().any(|(lt, ls)| *ls == side && (lt - t).abs() < 3.0) {
                        continue;
                    }
                    let p = a + d * t + right * side * back_off;
                    g.part(Concrete).boxed(Vec3::new(p.x, KERB_H + 0.2, p.y), Vec3::new(1.3, 0.4, 1.3), d.y.atan2(d.x), rgba(mul(pal.paving, 1.1)));
                    tree_world(&mut g, p, rng.random_range(3.0..4.5), rng.random_range(2.8..4.0), pal.bark, mul(pal.foliage, rng.random_range(0.85..1.15)));
                }
                t += 13.0 + rng.random_range(0.0..3.0);
            }
        }
        // Utility poles with sagging wires on cheap streets.
        if lv_mid < 0.4 {
            let mut prev: Option<Vec3> = None;
            let mut t = t0 + 2.0;
            while t < t1 - 1.0 {
                let p = a + d * t - right * back_off;
                g.part(Wood).cylinder(p, 0.16, 0.12, 0.0, 8.5, 6, rgba(pal.bark));
                g.part(Wood).boxed(Vec3::new(p.x, 8.0, p.y), Vec3::new(0.1, 0.1, 1.6), d.y.atan2(d.x), rgba(pal.bark));
                let top = Vec3::new(p.x, 8.2, p.y);
                if let Some(q) = prev {
                    for dz in [-0.6f32, 0.0, 0.6] {
                        let off = Vec3::new(right.x, 0.0, right.y) * dz;
                        g.part(Matte).wire(q + off, top + off, 0.8, 0.035, rgba(DARK));
                    }
                }
                if rng.random_bool(0.3) {
                    g.part(Metal).boxed(Vec3::new(p.x, 7.0, p.y) + Vec3::new(right.x, 0.0, right.y) * 0.35, Vec3::new(0.5, 0.9, 0.5), 0.0, rgba(mul(pal.steel, 0.6)));
                }
                prev = Some(top);
                t += 32.0;
            }
        }
        // Bins, benches, kiosks and shelters.
        let mut t = t0 + 10.0 + rng.random_range(0.0..8.0);
        while t < t1 - 6.0 {
            let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
            let p = a + d * t + right * side * (hw + 1.2);
            match rng.random_range(0..6) {
                0 | 1 => bin_world(&mut g, p, mul(pal.metal_paint[0], 0.9)),
                2 if lv_mid > 0.35 => {
                    let bp = a + d * t + right * side * back_off;
                    bench_world(&mut g, bp, d.y.atan2(d.x) + if side > 0.0 { std::f32::consts::PI } else { 0.0 }, pal.wood, CHARCOAL);
                }
                3 if lv_mid < 0.55 => {
                    // vending / phone kiosk with a lit screen
                    let kp = a + d * t + right * side * (hw + 1.4);
                    let yaw = d.y.atan2(d.x);
                    g.part(Metal).boxed(Vec3::new(kp.x, 1.2, kp.y), Vec3::new(1.0, 2.4, 0.9), yaw, rgba(mul(pal.metal_paint[2], 1.0)));
                    let face = kp - right * side * 0.47;
                    g.part(Glow).boxed(Vec3::new(face.x, 1.5, face.y), Vec3::new(0.6, 0.8, 0.04), yaw, rgba(mul(pal.led_colors[1], 0.5)));
                }
                4 if len > 80.0 => shelter(&mut g, pal, a + d * t + right * side * (hw + SIDEWALK * 0.55), d, right * side),
                _ => {}
            }
            t += 22.0 + rng.random_range(0.0..20.0);
        }
        // Bollards where the pavement meets a junction.
        for (p, _) in [(pa, 1.0f32), (pb, -1.0)] {
            for side in [-1.0f32, 1.0] {
                for k in 0..2 {
                    let q = p + right * side * (hw + 0.9 + k as f32 * 1.1);
                    bollard_world(&mut g, q, mul(pal.steel, 0.7));
                }
            }
        }
    }

    // Junction pads, crossings and stop lines.
    for (i, node) in roads.nodes.iter().enumerate() {
        let alive: Vec<_> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive).collect();
        if alive.is_empty() {
            continue;
        }
        let r = net.node_radius.get(i).copied().unwrap_or(4.0) + 0.3;
        let poly: Vec<Vec2> = (0..20)
            .map(|k| {
                let a = k as f32 / 20.0 * std::f32::consts::TAU;
                node.pos + Vec2::new(a.cos(), a.sin()) * r
            })
            .collect();
        g.part(Asphalt).ground_polygon(&poly, ASPHALT_Y + 0.01, asphalt_c);
        if alive.len() < 3 {
            continue;
        }
        for sid in alive {
            let seg = roads.segment(sid);
            let (a, b) = roads.segment_points(sid);
            let (far, _) = if seg.a.0 as usize == i { (b, a) } else { (a, b) };
            let d = (far - node.pos).normalize_or_zero();
            let right = Vec2::new(-d.y, d.x);
            let hw = seg.half_width();
            // Zebra crossing just outside the pad.
            let c0 = node.pos + d * (r + 0.8);
            let c1 = node.pos + d * (r + 3.2);
            let mut s = -hw + SHOULDER + 0.4;
            while s < hw - SHOULDER - 0.2 {
                g.part(Glow).ground_strip(&[c0 + right * s, c1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
                s += 1.0;
            }
            // Stop line across the approaching lanes (on the left when looking out).
            let incoming = if seg.a.0 as usize == i { seg.lanes_bwd } else { seg.lanes_fwd } as f32;
            if incoming > 0.0 {
                let w = incoming * LANE_WIDTH;
                let centre = node.pos + d * (r + 3.7) - right * (w * 0.5);
                g.part(Glow).ground_strip(&[centre - right * w * 0.5, centre + right * w * 0.5], 0.4, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 1.1)));
            }
        }
    }

    // Signal posts: pole at the kerb, arm over the approach, one head per lane.
    for post in signal_groups(net) {
        let d = Vec2::new(post.heading.cos(), post.heading.sin());
        let right = Vec2::new(-d.y, d.x);
        let base = post.pole;
        let pole_h = 5.6;
        g.part(Matte).cylinder(base, 0.16, 0.12, 0.0, pole_h, 8, rgba(CHARCOAL));
        let arm_end = post.leftmost;
        let a = Vec3::new(base.x, pole_h - 0.2, base.y);
        let b = Vec3::new(arm_end.x, pole_h - 0.2, arm_end.y);
        g.part(Matte).beam(a, b, 0.18, rgba(CHARCOAL));
        for lamp in &post.heads {
            // Head hangs below the arm, facing oncoming traffic.
            let head = Vec3::new(lamp.x, 4.75, lamp.y);
            g.part(Matte).boxed(head, Vec3::new(0.35, 1.1, 0.4), -post.heading, rgba(DARK));
            g.part(Matte).beam(Vec3::new(lamp.x, pole_h - 0.2, lamp.y), head + Vec3::Y * 0.55, 0.08, rgba(CHARCOAL));
            let _ = right;
        }
    }
    g
}

/// Bus shelter: two posts, a roof, a back panel and a lit advert.
fn shelter(g: &mut Geometry, pal: &Palette, p: Vec2, d: Vec2, out: Vec2) {
    let yaw = d.y.atan2(d.x);
    for s in [-1.6f32, 1.6] {
        let q = p + d * s;
        g.part(Matte).boxed(Vec3::new(q.x, 1.3, q.y), Vec3::new(0.1, 2.6, 0.1), yaw, rgba(CHARCOAL));
    }
    g.part(Panel).boxed(Vec3::new(p.x, 2.65, p.y), Vec3::new(3.8, 0.12, 1.8), yaw, rgba(mul(pal.steel, 0.9)));
    let back = p + out * 0.8;
    g.part(Glass).boxed(Vec3::new(back.x, 1.4, back.y), Vec3::new(3.6, 2.2, 0.05), yaw, rgba(pal.glass_dark));
    let ad = p + out * 0.78 + d * 1.0;
    g.part(Glow).boxed(Vec3::new(ad.x, 1.5, ad.y), Vec3::new(1.2, 1.6, 0.04), yaw, rgba(mul(pal.led_colors[2], 0.45)));
    g.part(Glow).boxed(Vec3::new(p.x, 2.57, p.y), Vec3::new(2.0, 0.05, 0.4), yaw, rgba(mul(pal.lamp, 0.6)));
    let seat = p + out * 0.4;
    g.part(Wood).boxed(Vec3::new(seat.x, 0.5, seat.y), Vec3::new(2.4, 0.08, 0.4), yaw, rgba(pal.wood));
}

struct SignalGroup {
    pole: Vec2,
    leftmost: Vec2,
    heads: Vec<Vec2>,
    heading: f32,
}

/// Approaches to signalised junctions grouped by segment and direction.
fn signal_groups(net: &LaneNetwork) -> Vec<SignalGroup> {
    use std::collections::HashMap;
    let mut groups: HashMap<(u32, bool), Vec<(u8, Vec2, Vec2)>> = HashMap::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        for &(conn, _) in &inter.connectors {
            for lane in net.lanes.iter() {
                if !lane.next.contains(&conn) || lane.is_connector() {
                    continue;
                }
                if let LaneKind::Road { segment, forward, index } = lane.kind {
                    let (p, d) = lane.point_at(lane.length - 0.6);
                    let entry = groups.entry((segment.0, forward)).or_default();
                    if !entry.iter().any(|(i, _, _)| *i == index) {
                        entry.push((index, p, d));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for (_, lanes) in groups {
        let Some((_, _, d)) = lanes.first() else { continue };
        let d = *d;
        let right = Vec2::new(-d.y, d.x);
        // Highest index is the rightmost lane.
        let rightmost = lanes.iter().max_by_key(|(i, _, _)| *i).unwrap();
        let leftmost = lanes.iter().min_by_key(|(i, _, _)| *i).unwrap();
        let pole = rightmost.1 + right * (LANE_WIDTH * 0.5 + SHOULDER + 0.45);
        out.push(SignalGroup { pole, leftmost: leftmost.1 - right * (LANE_WIDTH * 0.2), heads: lanes.iter().map(|(_, p, _)| *p).collect(), heading: d.y.atan2(d.x) });
    }
    out
}

/// Lamp positions for the renderer, one per lane feeding a signal.
pub fn signal_posts(net: &LaneNetwork) -> Vec<SignalPost> {
    let mut out = Vec::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        for &(conn, _) in &inter.connectors {
            for (lid, lane) in net.lanes.iter().enumerate() {
                if lane.next.contains(&conn) && !lane.is_connector() && seen.insert(lid) {
                    let (p, d) = lane.point_at(lane.length - 0.6);
                    out.push(SignalPost { lane: lid as LaneId, lamp: Vec3::new(p.x - d.x * 0.24, 4.95, p.y - d.y * 0.24), heading: d.y.atan2(d.x) });
                }
            }
        }
    }
    out
}

fn dashes(mesh: &mut crate::mesh::MeshData, a: Vec2, b: Vec2, width: f32, y: f32, dash: f32, gap: f32, color: [f32; 4]) {
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let d = (b - a) / len;
    let mut t = gap * 0.5;
    while t + dash <= len {
        mesh.ground_strip(&[a + d * t, a + d * (t + dash)], width, y, color);
        t += dash + gap;
    }
}
