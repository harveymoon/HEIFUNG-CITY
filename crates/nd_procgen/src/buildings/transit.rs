//! Bus depot and bus stops. The depot is a big shed with roller doors, a
//! yard of parked buses, a fuel island, a wash and an office. Stops are a
//! pole with a sign and, on busier ground, the shelter.

use super::parts::*;
use super::Ctx;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

pub fn bus_depot(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yard = offset_poly(&c.lot_poly, 0.5);
    ground(g, f, Asphalt, &yard, 0.03, mul(pal.asphalt, 1.25));
    let poly = &c.poly;
    let (lo, hi) = bounds(poly);
    let w = hi.x - lo.x;
    let d = hi.y - lo.y;
    // Shed across the back half, doors facing the yard.
    let shed = fit_inside(c, &rect((lo.x + hi.x) * 0.5, lo.y + d * 0.7, w * 0.44, d * 0.26));
    let h = 7.5;
    let panel = mul(pick(rng, &pal.panel), 0.9);
    extrude(g, f, Panel, &shed, 0.0, h, panel);
    cornice(g, c, &shed, h - 0.4, 0.8, 0.3, Panel, mul(panel, 0.8));
    let front = c.front_face(&shed);
    let doors = ((front.len / 5.0).floor() as i32).clamp(1, 5);
    for k in 0..doors {
        let t = -front.len * 0.5 + (k as f32 + 0.5) * front.len / doors as f32;
        roller_door(g, c, &front, t, 4.0, 5.0, mul(pal.metal_paint[0], 0.9));
    }
    sign_board(g, c, rng, &front, 0.0, h - 1.2, (front.len * 0.5).min(10.0), 0.9);
    for face in c.faces(&shed) {
        if !face.is_front && face.len > 6.0 {
            band_windows(g, c, rng, &face, h * 0.55, 1, 3.5);
            grime(g, c, rng, &face, 0.0, h, 2.0);
        }
    }
    roof_gear(g, c, rng, &shed, h, 0.3);
    // Bus bays in the front half, nose-in, a few empty.
    let bay_w = 3.6;
    let n = ((w * 0.9) / bay_w).floor() as i32;
    let bay_v = lo.y + d * 0.3;
    let mut parked = 0;
    for k in 0..n {
        let u = lo.x + w * 0.05 + (k as f32 + 0.5) * bay_w;
        let p = Vec2::new(u, bay_v);
        if !c.inside(p + Vec2::new(0.0, 6.5)) || !c.inside(p - Vec2::new(0.0, 6.5)) {
            continue;
        }
        f.bx(g, Matte, u - bay_w * 0.5, 0.05, bay_v, Vec3::new(0.12, 0.01, 13.0), mul(pal.lane_marker, 0.7));
        if rng.random_bool(0.6) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, rng, p, std::f32::consts::FRAC_PI_2, Vehicle::Bus, col);
            parked += 1;
        }
    }
    let _ = parked;
    // Fuel island and wash arch by one side, office by the gate.
    let fuel = Vec2::new(hi.x - 6.0, lo.y + d * 0.5);
    if c.inside(fuel + Vec2::new(3.0, 3.0)) && c.inside(fuel - Vec2::new(3.0, 3.0)) {
        f.bx(g, Concrete, fuel.x, 0.1, fuel.y, Vec3::new(2.0, 0.2, 7.0), mul(pal.concrete[0], 1.1));
        for k in 0..2 {
            f.bx(g, Metal, fuel.x, 0.9, fuel.y - 2.0 + k as f32 * 4.0, Vec3::new(0.8, 1.6, 0.6), mul(pal.metal_paint[1], 0.9));
        }
        for s in [-1.0f32, 1.0] {
            f.bx(g, Matte, fuel.x + s * 1.6, 2.4, fuel.y, Vec3::new(0.2, 4.8, 0.2), mul(pal.steel, 0.7));
        }
        f.bx(g, Panel, fuel.x, 4.9, fuel.y, Vec3::new(5.0, 0.3, 9.0), mul(pal.panel[0], 0.85));
        f.bx(g, Glow, fuel.x, 4.7, fuel.y, Vec3::new(4.0, 0.05, 8.0), mul(pal.lamp, 0.8));
    }
    let wash = Vec2::new(lo.x + 5.0, lo.y + d * 0.5);
    if c.inside(wash + Vec2::new(2.5, 7.0)) && c.inside(wash - Vec2::new(2.5, 7.0)) {
        for s in [-1.0f32, 1.0] {
            f.bx(g, Metal, wash.x + s * 2.0, 2.2, wash.y, Vec3::new(0.3, 4.4, 6.0), mul(pal.metal_paint[2 % pal.metal_paint.len()], 0.9));
        }
        f.bx(g, Metal, wash.x, 4.5, wash.y, Vec3::new(4.3, 0.3, 6.0), mul(pal.steel, 0.8));
        for k in 0..3 {
            f.bx(g, Matte, wash.x, 2.0, wash.y - 2.0 + k as f32 * 2.0, Vec3::new(3.4, 2.6, 0.6), [0.2, 0.3, 0.6]);
        }
    }
    let gate = c.front_face(&yard);
    let office = gate.at(gate.len * 0.3, -4.0);
    if c.inside(office) {
        let ob = fit_inside(c, &rect(office.x, office.y, 3.0, 2.2));
        extrude(g, f, Panel, &ob, 0.0, 3.2, mul(panel, 0.95));
        curtain_lights(g, c, rng, &ob, 1.0, 1, 1.8);
        extrude(g, f, Panel, &offset_poly(&ob, -0.2), 3.2, 3.5, mul(pal.steel, 0.9));
    }
    let faces = c.faces(&yard);
    let front_idx = faces.iter().position(|x| x.a.distance(gate.a) < 0.1);
    fence_poly(g, c, &yard, 2.2, front_idx, false);
    floodlight(g, c, gate.at(-gate.len * 0.4, -1.5), 9.0);
    barrier(g, c, &gate, -gate.len * 0.1, -1.0, 3.5);
}

/// Stops for the renderer: (kerb point, travel direction, outward normal).
/// Every stop gets a pole and a sign; stops get a shelter too.
pub fn bus_stops(stops: &[(Vec2, Vec2, Vec2)], pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    for &(p, d, out) in stops {
        let yaw = d.y.atan2(d.x);
        let pole = p + out * 0.9;
        g.part(Matte).boxed(Vec3::new(pole.x, 1.4, pole.y), Vec3::new(0.08, 2.8, 0.08), yaw, rgba(mul(pal.steel, 0.6)));
        g.part(Panel).boxed(Vec3::new(pole.x, 2.6, pole.y), Vec3::new(0.5, 0.5, 0.04), yaw, rgba([0.9, 0.85, 0.2]));
        g.part(Matte).boxed(Vec3::new(pole.x, 2.6, pole.y), Vec3::new(0.3, 0.3, 0.05), yaw, rgba([0.1, 0.1, 0.12]));
        // Kerb marking.
        let mark = p + out * 0.1;
        g.part(Matte).boxed(Vec3::new(mark.x, 0.055, mark.y), Vec3::new(12.0, 0.01, 0.12), yaw, rgba(mul(pal.lane_marker, 0.9)));
        crate::street::shelter(&mut g, pal, p + out * 2.4 - d * 1.5, d, out);
    }
    g
}
