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
pub fn retaining_walls(world: &World, palette: &Palette) -> (MeshData, MeshData) {
    const THICK: f32 = 0.35;
    const COPING: f32 = 0.12;
    const FOOTING: f32 = 0.3;
    let mut walls = MeshData::default();
    let mut caps = MeshData::default();
    let concrete = rgba(mul(palette.concrete(0), 0.95));
    let ground = rgba(mul(palette.ground, 0.9));
    let mut capped: Vec<nd_sim::LotId> = Vec::new();
    for r in &world.ground.retaining {
        let (a, b) = (r.a, r.b);
        let (ia, ib) = (a + r.inward * THICK, b + r.inward * THICK);
        let top = r.top + COPING;
        let (fa, fb) = (r.foot_a.min(r.top) - FOOTING, r.foot_b.min(r.top) - FOOTING);
        let v = |p: Vec2, y: f32| glam::Vec3::new(p.x, y, p.y);
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
        if !capped.contains(&r.lot) {
            capped.push(r.lot);
            if let Some(lot) = world.lots.get(r.lot) {
                caps.ground_polygon(&lot.polygon, r.top + 0.01, ground);
            }
        }
    }
    (walls, caps)
}
