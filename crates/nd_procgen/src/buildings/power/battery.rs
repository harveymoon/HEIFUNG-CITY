//! Grid batteries: containerised modules with status lights.

use super::*;

// ── grid batteries ─────────────────────────────────────────────────────────

/// A battery module: a container-sized cabinet, along the frame x by deep
/// by high.
pub(super) const MODULE: Vec3 = Vec3::new(6.0, 2.9, 2.4);
/// Module spacing along a row and between rows (an aisle to open the
/// doors into).
pub(super) const MODULE_PITCH: f32 = 6.8;
pub(super) const MODULE_ROW: f32 = 5.4;
/// Status lights on every module's front: one per fifth of charge.
pub const BATTERY_LIGHTS: usize = 5;

/// What a battery's status lights show: lit lights for the share of charge
/// (one per fifth, rounded up, so a sliver of charge still shows one) and
/// their colour for what it is doing.
pub fn battery_lights(level: f32, flow_mw: f32) -> (usize, [f32; 3]) {
    let lit = ((level.clamp(0.0, 1.0) * BATTERY_LIGHTS as f32) - 1e-3).ceil().max(0.0) as usize;
    let col = if flow_mw < -0.05 {
        // Charging: cold cyan.
        [0.15, 1.1, 1.6]
    } else if flow_mw > 0.05 {
        // Delivering: amber.
        [1.7, 0.85, 0.15]
    } else {
        // Holding: green.
        [0.3, 1.4, 0.45]
    };
    (lit.min(BATTERY_LIGHTS), col)
}

/// A grid battery: rows of white container-sized battery modules on
/// plinths, each with fans on the roof, door seams and a row of status
/// lights on its front showing the charge (cyan while charging, amber
/// while delivering, green holding); inverter skids at the row ends, a
/// control cabin at the gate and a fence. How many modules: by the kind,
/// as many as fit.
pub fn battery(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let (kind, level, flow) = c.battery.unwrap_or((nd_sim::BatteryKind::Medium, 0.6, 0.0));
    let most = match kind {
        nd_sim::BatteryKind::Small => 2,
        nd_sim::BatteryKind::Medium => 8,
        nd_sim::BatteryKind::Large => 36,
    };
    let yard = yard(g, c, 0.5, Paving, mul(pal.concrete(0), 0.75));
    let inner = offset_poly(&yard, 1.2);
    let (lo, hi) = bounds(&inner);
    let (lit, light) = battery_lights(level, flow);
    let body = [0.78, 0.8, 0.82];
    let half = Vec2::new(MODULE.x * 0.5 + 0.2, MODULE.z * 0.5 + 0.2);
    let fits = |p: Vec2| {
        let r = rect(p.x, p.y, half.x, half.y);
        r.iter().all(|q| nd_sim::zoning::point_in_polygon(*q, &inner)) && distance_to_boundary(p, &inner) >= half.y
    };
    let mut placed = 0;
    let mut v = lo.y + 6.0 + half.y;
    let mut rows: Vec<(f32, f32)> = Vec::new();
    while v < hi.y - half.y && placed < most {
        let mut u = lo.x + half.x;
        let mut row_end: Option<f32> = None;
        while u < hi.x - half.x && placed < most {
            let p = Vec2::new(u, v);
            if fits(p) {
                // Plinth, cabinet, a dark band of louvres along the top.
                f.bx(g, Concrete, p.x, 0.15, p.y, Vec3::new(MODULE.x + 0.3, 0.3, MODULE.z + 0.3), mul(pal.concrete(0), 0.9));
                f.bx(g, Panel, p.x, 0.3 + MODULE.y * 0.5, p.y, MODULE, body);
                for s in [-1.0f32, 1.0] {
                    // Two roof fans.
                    f.bx(g, Matte, p.x + s * 1.5, 0.3 + MODULE.y + 0.05, p.y, Vec3::new(1.1, 0.1, 1.1), MATTE_BLACK);
                }
                let face = crate::buildings::Face::new(Vec2::new(p.x - MODULE.x * 0.5, p.y - MODULE.z * 0.5), Vec2::new(p.x + MODULE.x * 0.5, p.y - MODULE.z * 0.5), Vec2::new(0.0, -1.0));
                for t in [-1.5f32, 0.0, 1.5] {
                    decal(g, f, Matte, &face, t - 0.75, 1.5, 0.02, 0.04, 2.3, [0.3, 0.32, 0.34]);
                }
                decal(g, f, Matte, &face, 0.0, 0.3 + MODULE.y - 0.25, 0.02, MODULE.x - 0.3, 0.22, [0.12, 0.13, 0.15]);
                // A thin light line under the louvres, in the status colour,
                // and the status lights near the right-hand end, on both
                // long faces so the yard reads from either side.
                let back = crate::buildings::Face::new(Vec2::new(p.x - MODULE.x * 0.5, p.y + MODULE.z * 0.5), Vec2::new(p.x + MODULE.x * 0.5, p.y + MODULE.z * 0.5), Vec2::new(0.0, 1.0));
                for side in [&face, &back] {
                    decal(g, f, Glow, side, 0.0, 0.3 + MODULE.y - 0.42, 0.025, MODULE.x - 0.6, 0.05, mul(light, 0.45));
                    for k in 0..BATTERY_LIGHTS {
                        let (s, col) = if k < lit { (Glow, light) } else { (Matte, [0.06, 0.07, 0.08]) };
                        decal(g, f, s, side, 1.7 + k as f32 * 0.24, 1.9, 0.03, 0.16, 0.16, col);
                    }
                }
                placed += 1;
                row_end = Some(u + half.x);
            }
            u += MODULE_PITCH;
        }
        if let Some(e) = row_end {
            rows.push((e, v));
        }
        v += MODULE_ROW;
    }
    // An inverter skid at the end of each row where it fits: a grey box
    // with radiator fins.
    for (e, v) in rows {
        let p = Vec2::new(e + 1.4, v);
        if poly_inside(c, &rect(p.x, p.y, 1.1, 1.0)) && nd_sim::zoning::point_in_polygon(p, &inner) {
            f.bx(g, Metal, p.x, 1.0, p.y, Vec3::new(1.6, 2.0, 1.4), mul(pal.metal_paint(1), 0.8));
            for k in 0..3 {
                f.bx(g, Metal, p.x - 0.5 + k as f32 * 0.5, 1.0, p.y - 0.75, Vec3::new(0.06, 1.6, 0.2), mul(pal.steel, 0.6));
            }
        }
    }
    let front = c.front_face(&yard);
    let cab = front.at(front.len * 0.3, -3.0);
    if c.inside_local(cab) {
        let cb = fit_inside(c, &rect(cab.x, cab.y, 2.0, 1.3));
        extrude(g, f, Panel, &cb, 0.0, 2.7, mul(GUNMETAL, 1.6));
        let (blo, bhi) = bounds(&cb);
        let face = crate::buildings::Face::new(Vec2::new(blo.x, blo.y), Vec2::new(bhi.x, blo.y), Vec2::new(0.0, -1.0));
        if face.len > 1.5 {
            decal(g, f, Glow, &face, 0.0, 2.35, 0.03, face.len * 0.8, 0.07, mul(light, 0.6));
        }
    }
    fence_lot(g, c, &yard, 2.2, false);
    sign_board(g, c, rng, &front, -front.len * 0.3, 2.6, (front.len * 0.3).min(6.0), 0.6);
}
