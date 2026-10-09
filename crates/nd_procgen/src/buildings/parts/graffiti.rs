//! Graffiti on walls.

#[allow(unused_imports)]
use super::*;

// ── graffiti ─────────────────────────────────────────────────────────────────

/// Spray paint on a face between two heights: bubble-letter pieces with an
/// outline, quick tag strokes and the odd throw-up with drips, all decals a
/// few centimetres proud of the wall at arm's reach. `amount` scales
/// how much of the face is covered; wear adds more.
pub fn graffiti(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, y1: f32, amount: f32) {
    let a = c.frame.to_world(face.a.x, face.a.y);
    let b = c.frame.to_world(face.b.x, face.b.y);
    graffiti_world(g, rng, a, b, y0, y1, amount, c.wear, c.palette);
}

/// Spray colours: dimmed LED hues, awning paints, chalk white and black.
pub(super) fn spray_colour(rng: &mut impl Rng, pal: &Palette) -> [f32; 3] {
    match rng.random_range(0..7) {
        0..=2 => mul(pick(rng, &pal.led_colors), 0.32),
        3 | 4 => mul(pick(rng, &pal.awning_colors), 1.25),
        5 => pal.chalk,
        _ => DARK,
    }
}

/// World-space graffiti on the wall from `a` to `b`; the right-hand side of
/// the a→b direction is the painted side, as with [`Face::from_edge`].
#[allow(clippy::too_many_arguments)]
pub fn graffiti_world(g: &mut Geometry, rng: &mut impl Rng, a: Vec2, b: Vec2, y0: f32, y1: f32, amount: f32, wear: f32, pal: &Palette) {
    let len = a.distance(b);
    let span = y1 - y0;
    if len < 2.5 || span < 0.8 {
        return;
    }
    let tangent = (b - a) / len;
    let out = Vec2::new(tangent.y, -tangent.x);
    let along3 = Vec3::new(tangent.x, 0.0, tangent.y);
    let out3 = Vec3::new(out.x, 0.0, out.y);
    let mid = (a + b) * 0.5;
    let pieces = ((len / 7.0) * amount * (0.4 + wear) * rng.random_range(0.6..1.4)).round().clamp(0.0, 6.0) as i32;
    // Paint depths: their own layers between the grime (0.02) and the
    // window frames (0.03), off the whole-centimetre grid the other decals
    // use. Pieces are kept apart, so two never share a layer where they
    // overlap.
    use crate::depth::{GRAFFITI_FILL as FILL, GRAFFITI_OUTLINE as OUTLINE, GRAFFITI_STROKE as STROKE};
    let mut taken: Vec<(f32, f32, f32, f32)> = Vec::new();
    for _ in 0..pieces {
        let w = rng.random_range(1.4f32..3.6).min(len * 0.8);
        let h = rng.random_range(0.6f32..1.3).min(span * 0.8);
        let room = len * 0.5 - w * 0.5 - 0.3;
        let t = if room > 0.05 { rng.random_range(-room..room) } else { 0.0 };
        // Painted from the ground: the piece sits between knee and head height.
        let top = (span - h - 0.2).clamp(0.31, 2.4);
        let yc = y0 + h * 0.5 + rng.random_range(0.3..top);
        let col = spray_colour(rng, pal);
        let outline = if rng.random_bool(0.6) { DARK } else { pal.chalk };
        // Its extent with the outline and the drips under it.
        let (t0, t1, b0, b1) = (t - w * 0.5 - 0.1, t + w * 0.5 + 0.1, yc - h * 0.5 - 0.8, yc + h * 0.5 + 0.1);
        let clash = taken.iter().any(|&(u0, u1, v0, v1)| t0 < u1 && u0 < t1 && b0 < v1 && v0 < b1);
        let kind = rng.random_range(0..5);
        if clash {
            continue;
        }
        taken.push((t0, t1, b0, b1));
        // Every stroke is a decal: paint has no back.
        let place = |g: &mut Geometry, dt: f32, y: f32, d: f32, size: Vec3, c: [f32; 3]| {
            let p = mid + tangent * (t + dt) + out * d;
            g.part(Matte).decal(Vec3::new(p.x, y, p.y), along3, Vec3::Y, size.x, size.y, out3, rgba(c));
        };
        match kind {
            0 | 1 => {
                // Bubble letters: fat blocks in two colours with an outline behind.
                let n = ((w / 0.65) as i32).clamp(2, 6);
                let lw = w / n as f32;
                let second = spray_colour(rng, pal);
                for i in 0..n {
                    let lt = -w * 0.5 + (i as f32 + 0.5) * lw;
                    let lh = h * rng.random_range(0.7..1.0);
                    let dy = rng.random_range(-0.08..0.08);
                    place(g, lt, yc + dy, OUTLINE, Vec3::new(lw * 0.98, lh + 0.14, 0.015), outline);
                    place(g, lt, yc + dy, FILL, Vec3::new(lw * 0.8, lh, 0.015), if i % 2 == 0 { col } else { second });
                }
            }
            2 | 3 => {
                // Tag: a few quick strokes and a flick or two.
                for _ in 0..rng.random_range(3..6) {
                    let sw = rng.random_range(0.5..w);
                    let slack = (w - sw) * 0.5;
                    let st = if slack > 0.01 { rng.random_range(-slack..slack) } else { 0.0 };
                    let sy = yc + rng.random_range(-h * 0.45..h * 0.45);
                    place(g, st, sy, STROKE, Vec3::new(sw, rng.random_range(0.07..0.16), 0.012), col);
                }
                for _ in 0..rng.random_range(1..3) {
                    let st = rng.random_range(-w * 0.45..w * 0.45);
                    place(g, st, yc, STROKE, Vec3::new(rng.random_range(0.07..0.14), h * rng.random_range(0.5..1.0), 0.012), col);
                }
            }
            _ => {
                // Throw-up: one filled block with an outline and drips below.
                place(g, 0.0, yc, OUTLINE, Vec3::new(w + 0.16, h + 0.16, 0.015), outline);
                place(g, 0.0, yc, FILL, Vec3::new(w, h, 0.015), col);
                let under = yc - h * 0.5;
                for _ in 0..rng.random_range(1..4) {
                    let dt = rng.random_range(-w * 0.45..w * 0.45);
                    let dl = rng.random_range(0.2f32..0.7).min(under - y0 - 0.05).max(0.05);
                    place(g, dt, under - dl * 0.5, STROKE, Vec3::new(0.05, dl, 0.012), col);
                }
            }
        }
    }
}
