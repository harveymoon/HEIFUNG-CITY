//! Power plants: a coal station with stacks and a coal yard, a field of
//! solar panels, a line of wind turbines, and a nuclear station with its
//! containment dome and cooling towers.

use super::parts::*;
use super::Ctx;
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

pub fn coal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.3));
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    // Boiler hall at the back, turbine hall in front of it.
    let hall = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.68, (hi.x - lo.x) * 0.42, (hi.y - lo.y) * 0.26));
    let h = 22.0;
    let panel = mul(pick(rng, &pal.panel), 0.75);
    extrude(g, f, Panel, &hall, 0.0, h, panel);
    cornice(g, c, &hall, h - 0.5, 1.0, 0.3, Panel, mul(panel, 0.8));
    for face in c.faces(&hall) {
        if face.len > 8.0 {
            band_windows(g, c, rng, &face, h * 0.6, 1, 4.0);
            grime(g, c, rng, &face, 0.0, h, 3.0);
        }
    }
    let turbine = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + (hi.y - lo.y) * 0.3, (hi.x - lo.x) * 0.38, (hi.y - lo.y) * 0.16));
    extrude(g, f, Metal, &turbine, 0.0, 11.0, mul(pick(rng, &pal.metal_paint), 0.9));
    weeds(g, c, rng, &yard, 4);
    // Two tall stacks with a glowing top and a plume.
    for k in 0..2 {
        let mut p = Vec2::new(lo.x + (hi.x - lo.x) * (0.3 + 0.4 * k as f32), hi.y - 6.0);
        let hc = poly_center(&hall);
        for _ in 0..6 {
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) >= 5.0 {
                break;
            }
            p = p.lerp(hc, 0.3);
        }
        if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 5.0 {
            continue;
        }
        chimney(g, f, p, 2.2, h, 42.0, mul(pal.concrete(0), 0.7), pal.aviation_light);
        let w = f.to_world(p.x, p.y);
        // The plume stays within the 5 m the stack was given from the edge.
        for s in 0..5 {
            let y = h + 42.0 + 2.0 + s as f32 * 3.5;
            let drift = s as f32 * 0.4;
            g.part(Matte).cylinder(w + Vec2::new(drift, drift * 0.4), 1.6 + s as f32 * 0.3, 1.8 + s as f32 * 0.3, y, y + 3.0, 8, rgba([0.32, 0.30, 0.28]));
        }
    }
    // Coal pile, conveyor up to the boiler hall, rail-side hopper.
    let pile = Vec2::new(lo.x + (hi.x - lo.x) * 0.2, lo.y + (hi.y - lo.y) * 0.45);
    let pile_r = if c.inside_local(pile) { (distance_to_boundary(pile, &c.lot_poly) - 0.5).min(7.0) } else { 0.0 };
    if pile_r >= 3.0 {
        let w = f.to_world(pile.x, pile.y);
        g.part(Matte).frustum(&MeshData::ngon(w, pile_r, 10, 0.0, 0.0), &MeshData::ngon(w, pile_r * 0.3, 10, 0.3, pile_r * 0.7), rgba([0.07, 0.07, 0.07]));
        let a = f.at(pile.x, 5.0, pile.y);
        let hc = poly_center(&hall);
        let b = f.at(hc.x, h + 1.0, hc.y);
        g.part(Metal).beam(a, b, 1.6, rgba(mul(pal.steel, 0.6)));
        for t in [0.3f32, 0.6] {
            let p = a.lerp(b, t);
            g.part(Matte).boxed(Vec3::new(p.x, p.y * 0.5, p.z), Vec3::new(0.5, p.y, 0.5), f.yaw(), rgba(mul(pal.steel, 0.5)));
        }
    }
    // Transformer yard: fenced, insulators, bus bars.
    let ty = Vec2::new(hi.x - 7.0, lo.y + 6.0);
    if poly_inside(c, &rect(ty.x, ty.y, 5.3, 4.3)) {
        let pen = rect(ty.x, ty.y, 5.0, 4.0);
        ground(g, f, Dirt, &pen, 0.05, pal.gravel);
        fence_poly(g, c, &pen, 2.0, None, true);
        for k in 0..3 {
            let p = ty + Vec2::new((k as f32 - 1.0) * 3.0, 0.0);
            f.bx(g, Metal, p.x, 1.2, p.y, Vec3::new(1.6, 2.4, 1.2), mul(pal.metal_paint(1), 0.9));
            f.bx(g, Matte, p.x, 3.3, p.y, Vec3::new(0.15, 1.8, 0.15), [0.6, 0.6, 0.55]);
        }
        f.bx(g, Matte, ty.x, 4.3, ty.y, Vec3::new(8.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 2.4, true);
    floodlight(g, c, front.at(-front.len * 0.4, -1.5), 10.0);
    floodlight(g, c, front.at(front.len * 0.4, -1.5), 10.0);
    sign_board(g, c, rng, &front, 0.0, 3.0, (front.len * 0.4).min(10.0), 0.9);
}

pub fn solar(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Dirt, pal.gravel);
    let inner = offset_poly(&yard, 3.0);
    let (lo, hi) = bounds(&inner);
    // Rows of tilted panels on posts, facing the front.
    let row_gap = 5.0;
    let mut v = lo.y + 2.0;
    while v < hi.y - 1.0 {
        let mut u = lo.x + 2.0;
        while u < hi.x - 1.0 {
            let p = Vec2::new(u, v);
            if nd_sim::zoning::point_in_polygon(p, &inner) && nd_sim::zoning::point_in_polygon(p + Vec2::new(3.0, 1.0), &inner) {
                f.bx(g, Matte, p.x, 0.9, p.y, Vec3::new(0.12, 1.8, 0.12), mul(pal.steel, 0.7));
                f.bx(g, Matte, p.x + 2.6, 0.9, p.y, Vec3::new(0.12, 1.8, 0.12), mul(pal.steel, 0.7));
                let w = f.to_world(p.x + 1.3, p.y);
                let yaw = f.yaw();
                let (s, co) = yaw.sin_cos();
                // Tilted panel: a thin box rotated about the frame's along axis.
                let tilt = 0.5f32;
                let (ts, tc) = tilt.sin_cos();
                let corners = |du: f32, dv: f32| {
                    let y = 1.8 + dv * ts;
                    let vv = dv * tc;
                    Vec3::new(w.x + du * co - vv * s, y, w.y + du * s + vv * co)
                };
                g.part(Glass).quad_facing([corners(-2.9, -1.0), corners(2.9, -1.0), corners(2.9, 1.0), corners(-2.9, 1.0)], Vec3::new(0.0, 1.0, 0.0), rgba([0.10, 0.14, 0.26]));
            }
            u += 6.4;
        }
        v += row_gap;
    }
    // Inverter cabin and a transformer by the gate.
    let front = c.front_face(&yard);
    let cab = front.at(front.len * 0.3, -3.5);
    if c.inside_local(cab) {
        let cb = fit_inside(c, &rect(cab.x, cab.y, 2.5, 1.8));
        extrude(g, f, Panel, &cb, 0.0, 2.8, mul(pick(rng, &pal.panel), 0.9));
        let tx = cab + Vec2::new(4.0, 0.0);
        if c.inside_local(tx) && distance_to_boundary(tx, &c.lot_poly) > 0.9 {
            f.bx(g, Metal, tx.x, 1.0, tx.y, Vec3::new(1.4, 2.0, 1.0), mul(pal.metal_paint(1), 0.9));
        }
    }
    fence_lot(g, c, &yard, 2.0, false);
    sign_board(g, c, rng, &front, -front.len * 0.3, 2.6, (front.len * 0.3).min(6.0), 0.6);
}

pub fn wind(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Foliage, mul(pal.foliage_dry, 1.1));
    weeds(g, c, rng, &yard, 10);
    let inner = offset_poly(&yard, 6.0);
    let (lo, hi) = bounds(&inner);
    let n = (((hi.x - lo.x) / 22.0).floor() as i32).clamp(1, 5);
    let white = [0.86, 0.87, 0.88];
    for k in 0..n {
        let u = if n == 1 { (lo.x + hi.x) * 0.5 } else { lo.x + (k as f32 + 0.5) * (hi.x - lo.x) / n as f32 };
        let p = Vec2::new(u, (lo.y + hi.y) * 0.5 + rng.random_range(-3.0..3.0));
        if !nd_sim::zoning::point_in_polygon(p, &inner) || !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 4.5 {
            continue;
        }
        let w = f.to_world(p.x, p.y);
        // Blades must clear the lot edge, so small lots get small turbines.
        let clearance = distance_to_boundary(p, &c.lot_poly) - 1.5;
        let hub = rng.random_range(38.0f32..52.0).min(clearance / 0.48).max(14.0);
        g.part(Concrete).cylinder(w, 3.0, 3.0, 0.0, 0.6, 12, rgba(mul(pal.concrete(0), 1.1)));
        g.part(Panel).cylinder(w, 1.6, 0.9, 0.6, hub, 12, rgba(white));
        // Nacelle facing the wind (the frame's inward direction) and three blades.
        let yaw = f.yaw();
        g.part(Panel).boxed(Vec3::new(w.x, hub + 0.2, w.y), Vec3::new(2.6, 2.4, 4.2), yaw, rgba(white));
        let (s, co) = yaw.sin_cos();
        let nose = Vec3::new(w.x - 2.4 * s * 0.0 + 2.3 * (-s), hub + 0.2, w.y + 2.3 * co);
        let axis = Vec3::new(-s, 0.0, co);
        let phase = rng.random_range(0.0..std::f32::consts::TAU);
        let blade_len = (hub * 0.48).min(clearance);
        for b in 0..3 {
            let a = phase + b as f32 * std::f32::consts::TAU / 3.0;
            // Blade direction in the rotor plane (perpendicular to axis).
            let up = Vec3::Y;
            let side = axis.cross(up).normalize();
            let dir = up * a.cos() + side * a.sin();
            let tip = nose + dir * blade_len;
            g.part(Panel).beam(nose, tip, 1.3, rgba(white));
        }
        g.part(Glow).boxed(Vec3::new(w.x, hub + 1.6, w.y), Vec3::splat(0.4), 0.0, rgba(pal.aviation_light));
    }
    let front = c.front_face(&yard);
    let cab = front.at(front.len * 0.3, -3.0);
    if c.inside_local(cab) {
        let cb = fit_inside(c, &rect(cab.x, cab.y, 2.0, 1.6));
        extrude(g, f, Panel, &cb, 0.0, 2.6, mul(pick(rng, &pal.panel), 0.9));
    }
    fence_lot(g, c, &yard, 1.6, false);
}

pub fn nuclear(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = yard(g, c, 0.5, Asphalt, mul(pal.asphalt, 1.4));
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let concrete = mul(pal.concrete(0), 1.15);
    // Containment: a cylinder with a dome.
    let cc = Vec2::new(lo.x + (hi.x - lo.x) * 0.3, lo.y + (hi.y - lo.y) * 0.6);
    let r = ((hi.x - lo.x).min(hi.y - lo.y) * 0.16).clamp(6.0, 14.0).min(distance_to_boundary(cc, &yard) - 0.5);
    if r >= 2.5 && c.inside_local(cc) {
        let w = f.to_world(cc.x, cc.y);
        g.part(Concrete).cylinder(w, r, r, 0.0, r * 1.8, 20, rgba(concrete));
        let mut prev = MeshData::ngon(w, r, 20, 0.0, r * 1.8);
        for k in 1..=5 {
            let t = k as f32 / 5.0;
            let a = t * std::f32::consts::FRAC_PI_2;
            // The last ring keeps a small radius so no side quad collapses.
            let ring = MeshData::ngon(w, (r * a.cos()).max(r * 0.05), 20, 0.0, r * 1.8 + r * 0.9 * a.sin());
            g.part(Concrete).frustum(&prev, &ring, rgba(mul(concrete, 1.0 - 0.05 * k as f32)));
            prev = ring;
        }
        g.part(Glow).boxed(Vec3::new(w.x, r * 2.75, w.y), Vec3::splat(0.4), 0.0, rgba(pal.aviation_light));
    }
    // Turbine and reactor service halls.
    let hall = fit_inside(c, &rect(lo.x + (hi.x - lo.x) * 0.7, lo.y + (hi.y - lo.y) * 0.6, (hi.x - lo.x) * 0.18, (hi.y - lo.y) * 0.3));
    let panel = mul(pick(rng, &pal.panel), 0.95);
    extrude(g, f, Panel, &hall, 0.0, 16.0, panel);
    cornice(g, c, &hall, 15.5, 1.0, 0.3, Panel, mul(panel, 0.8));
    for face in c.faces(&hall) {
        if face.len > 6.0 {
            band_windows(g, c, rng, &face, 10.0, 1, 4.0);
        }
    }
    // Cooling towers: hyperboloids from stacked frustums, with a steam plume.
    for k in 0..2 {
        let p = Vec2::new(lo.x + (hi.x - lo.x) * (0.25 + 0.5 * k as f32), lo.y + (hi.y - lo.y) * 0.18);
        if !c.inside_local(p) {
            continue;
        }
        let w = f.to_world(p.x, p.y);
        let rb = (r * 0.75).min(distance_to_boundary(p, &yard) - 1.5);
        if rb < 2.5 {
            continue;
        }
        let profile = [(0.0f32, 1.0f32), (0.3, 0.78), (0.6, 0.62), (0.8, 0.6), (1.0, 0.68)];
        let th = rb * 3.6;
        for seg in profile.windows(2) {
            let a = MeshData::ngon(w, rb * seg[0].1, 18, 0.0, th * seg[0].0);
            let b = MeshData::ngon(w, rb * seg[1].1, 18, 0.0, th * seg[1].0);
            g.part(Concrete).frustum(&a, &b, rgba(mul(concrete, 0.95)));
        }
        for s in 0..4 {
            let y = th + 1.0 + s as f32 * 4.0;
            g.part(Matte).cylinder(w + Vec2::new(s as f32 * 0.5, 0.0), rb * 0.5 + s as f32 * 0.4, rb * 0.55 + s as f32 * 0.4, y, y + 3.5, 10, rgba([0.9, 0.9, 0.92]));
        }
    }
    // Switchyard, double fence, floodlights, gatehouse.
    let sy = Vec2::new(hi.x - 8.0, hi.y - 7.0);
    if poly_inside(c, &rect(sy.x, sy.y, 6.3, 5.3)) {
        let pen = rect(sy.x, sy.y, 6.0, 5.0);
        ground(g, f, Dirt, &pen, 0.05, pal.gravel);
        fence_poly(g, c, &pen, 2.2, None, true);
        for k in 0..4 {
            let p = sy + Vec2::new((k as f32 - 1.5) * 3.0, 0.0);
            f.bx(g, Metal, p.x, 1.3, p.y, Vec3::new(1.8, 2.6, 1.3), mul(pal.metal_paint(1), 0.9));
            f.bx(g, Matte, p.x, 4.0, p.y, Vec3::new(0.15, 2.6, 0.15), [0.6, 0.6, 0.55]);
        }
        f.bx(g, Matte, sy.x, 5.3, sy.y, Vec3::new(11.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
        f.bx(g, Matte, sy.x, 5.3, sy.y + 1.2, Vec3::new(11.0, 0.08, 0.08), [0.5, 0.5, 0.5]);
    }
    let front = c.front_face(&yard);
    fence_lot(g, c, &yard, 3.0, true);
    fence_lot(g, c, &offset_poly(&yard, 1.5), 2.4, true);
    for p in [front.at(-front.len * 0.42, -2.0), front.at(front.len * 0.42, -2.0), Vec2::new(lo.x + 3.0, hi.y - 3.0), Vec2::new(hi.x - 3.0, lo.y + 3.0)] {
        if c.inside_local(p) {
            floodlight(g, c, p, 12.0);
        }
    }
    let gate = front.at(front.len * 0.3, -3.0);
    if c.inside_local(gate) {
        let house = fit_inside(c, &rect(gate.x, gate.y, 2.0, 1.6));
        extrude(g, f, Panel, &house, 0.0, 3.0, mul(panel, 0.9));
        curtain_lights(g, c, rng, &house, 1.0, 1, 1.6);
    }
    bollards(g, c, &front, 6, -1.4);
    sign_board(g, c, rng, &front, 0.0, 3.2, (front.len * 0.4).min(10.0), 1.0);
}
