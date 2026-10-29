//! Retaining walls along cuttings and embankments.

use super::*;

// ── retaining walls ─────────────────────────────────────────────────────────

/// [review] Where a corner lot levelled to its front street stands over a
/// lower side street (`nd_sim::landscape::Retaining`): a board-formed
/// concrete wall along the lot's edge, its outer face on the edge, from
/// below the side street's pavement up to a coping just over the pad; and
/// the lot's ground as one level cap at the pad (the coarse terrain mesh
/// dips toward the street behind the wall, `keep_under_streets`). Absolute
/// heights: drawn as is, not lifted. Returns (walls, caps).
/// [r4-hillside] A terrace's wall this high over the drop is drawn as a
/// podium (piers and dark bays) rather than a blank wall.
pub const PODIUM_FROM: f32 = 3.5;

pub fn retaining_walls(world: &World, palette: &Palette) -> (MeshData, MeshData) {
    const THICK: f32 = 0.35;
    const COPING: f32 = 0.12;
    const FOOTING: f32 = 0.3;
    let mut walls = MeshData::default();
    let mut caps = MeshData::default();
    let concrete = rgba(mul(palette.concrete(0), 0.95));
    let ground = rgba(mul(palette.ground, 0.9));
    let mut capped: Vec<nd_sim::LotId> = Vec::new();
    let dark = rgba(mul(palette.concrete(0), 0.35));
    for r in &world.ground.retaining {
        let (a, b) = (r.a, r.b);
        let v = |p: Vec2, y: f32| glam::Vec3::new(p.x, y, p.y);
        if r.holds_back {
            // [r4-hillside] The hill held back above a terrace cut into the
            // slope: the face looks into the lot from its pad up to the
            // hill behind, a coping on top and buttresses every 4 m.
            let into = glam::Vec3::new(r.inward.x, 0.0, r.inward.y);
            let (ba, bb) = (a - r.inward * THICK, b - r.inward * THICK);
            let (ta, tb) = (r.foot_a + COPING, r.foot_b + COPING);
            let base = r.top - FOOTING;
            walls.quad_facing([v(a, base), v(b, base), v(b, tb), v(a, ta)], into, concrete);
            walls.quad_facing([v(a, ta), v(b, tb), v(bb, tb), v(ba, ta)], glam::Vec3::Y, concrete);
            let len = a.distance(b);
            let d = (b - a).normalize_or_zero();
            let n = (len / 4.0).floor() as i32;
            for k in 1..n {
                let p = a + d * (k as f32 * len / n as f32);
                let h = r.foot_a + (r.foot_b - r.foot_a) * (k as f32 / n as f32) - r.top;
                if h < 1.5 {
                    continue;
                }
                let c = p + r.inward * 0.25;
                walls.boxed(glam::Vec3::new(c.x, r.top + h * 0.45, c.y), glam::Vec3::new(0.45, h * 0.9, 0.5), d.y.atan2(d.x), concrete);
            }
            if !capped.contains(&r.lot) {
                capped.push(r.lot);
                if let Some(lot) = world.lots.get(r.lot) {
                    caps.ground_polygon(&lot.polygon, r.top + 0.01, ground);
                }
            }
            continue;
        }
        let (ia, ib) = (a + r.inward * THICK, b + r.inward * THICK);
        let top = r.top + COPING;
        let (fa, fb) = (r.foot_a.min(r.top) - FOOTING, r.foot_b.min(r.top) - FOOTING);
        let out = glam::Vec3::new(-r.inward.x, 0.0, -r.inward.y);
        // The face to the street, the coping, the lip above the pad behind.
        walls.quad_facing([v(a, fa), v(b, fb), v(b, top), v(a, top)], out, concrete);
        walls.quad_facing([v(a, top), v(b, top), v(ib, top), v(ia, top)], glam::Vec3::Y, concrete);
        walls.quad_facing([v(ia, r.top - 0.05), v(ib, r.top - 0.05), v(ib, top), v(ia, top)], -out, concrete);
        // The ends, where the wall meets the next lot or the corner.
        let along = (b - a).normalize_or_zero();
        let along3 = glam::Vec3::new(along.x, 0.0, along.y);
        walls.quad_facing([v(a, fa), v(ia, fa), v(ia, top), v(a, top)], -along3, concrete);
        walls.quad_facing([v(b, fb), v(ib, fb), v(ib, top), v(b, top)], along3, concrete);
        // [r4-hillside] A podium standing high over the drop (Hong Kong's
        // Mid-Levels): piers down its face every 4 m with dark bays between
        // them (the car park and stores under the lot), and a slab edge at
        // the top.
        let drop = r.top - r.foot_a.max(r.foot_b);
        if drop >= PODIUM_FROM {
            let len = a.distance(b);
            let n = (len / 4.0).floor().max(1.0) as i32;
            let o = -r.inward * 0.06;
            for k in 0..n {
                let (f0, f1) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
                let (p0, p1) = (a.lerp(b, f0 + 0.08 / n as f32 * 4.0), a.lerp(b, f1 - 0.08 / n as f32 * 4.0));
                let (g0, g1) = (fa + (fb - fa) * f0 + FOOTING + 0.4, fa + (fb - fa) * f1 + FOOTING + 0.4);
                let roof = r.top - 0.9;
                if roof - g0.max(g1) < 1.8 {
                    continue;
                }
                walls.quad_facing([v(p0 + o, g0), v(p1 + o, g1), v(p1 + o, roof), v(p0 + o, roof)], out, dark);
            }
            for k in 0..=n {
                let p = a.lerp(b, k as f32 / n as f32) - r.inward * 0.2;
                let g = fa + (fb - fa) * (k as f32 / n as f32);
                walls.boxed(glam::Vec3::new(p.x, (g + r.top) * 0.5, p.y), glam::Vec3::new(0.6, r.top - g, 0.6), along.y.atan2(along.x), concrete);
            }
            let (sa, sb) = (a - r.inward * 0.25, b - r.inward * 0.25);
            walls.quad_facing([v(sa, r.top - 0.7), v(sb, r.top - 0.7), v(sb, top), v(sa, top)], out, concrete);
        }
        if !capped.contains(&r.lot) {
            capped.push(r.lot);
            if let Some(lot) = world.lots.get(r.lot) {
                caps.ground_polygon(&lot.polygon, r.top + 0.01, ground);
            }
        }
    }
    (walls, caps)
}
