//! Tunnel bores: the cross-section and the lined tube.

use super::*;

/// How far a tunnel's walls stand outside its carriageway (half width).
pub const TUNNEL_WALL: f32 = 0.8;


/// Spring line of a tunnel bore (`bore_shell`): the walls stand straight
/// up to here, the vault springs from it.
pub const BORE_SPRING: f32 = 2.6;
/// Lining bands round the inside of a bore, this far apart along it.
pub(super) const BORE_RIB_EVERY: f32 = 14.0;

/// The cross-section of a tunnel bore of carriageway half width `hw`, from
/// the right foot up the wall, over the vault and down the left wall:
/// (offset right of travel, height). A horseshoe: straight walls
/// `TUNNEL_WALL` outside the carriageway to [`BORE_SPRING`], then a half
/// ellipse whose rise grows with the width (a wide bore is not a slot).
pub(crate) fn bore_profile(hw: f32) -> Vec<(f32, f32)> {
    let r = hw + TUNNEL_WALL;
    let rise = (r * 0.75).clamp(2.6, 4.6);
    let n = 12;
    let mut v = vec![(r, 0.0), (r, BORE_SPRING)];
    for k in 1..n {
        let a = std::f32::consts::PI * k as f32 / n as f32;
        v.push((r * a.cos(), BORE_SPRING + rise * a.sin()));
    }
    v.push((-r, BORE_SPRING));
    v.push((-r, 0.0));
    v
}

/// The direction into the bore from a point (x, y) of its cross-section:
/// straight in across the walls, towards the spring line's centre in the vault.
pub(super) fn bore_inward(x: f32, y: f32) -> Vec2 {
    Vec2::new(-x, if y > BORE_SPRING { BORE_SPRING - y } else { 0.0 }).normalize_or_zero()
}

/// A tunnel bore as a tube along `walk`: a concrete lining, darker lining
/// bands every [`BORE_RIB_EVERY`] metres and a lamp line along each wall.
///
/// Every face looks *into* the tube and the materials cull back faces, so
/// whichever half of the tube is nearer the camera (its outside towards the
/// viewer) is never drawn: the near side is cut away from any angle and the
/// cars inside show, with the far wall and vault behind them. No per-frame
/// work: the cut comes from the winding alone.
pub fn bore_shell(g: &mut Geometry, walk: &[Vec2], hw: f32, lining: [f32; 4], rib: [f32; 4], lamp: [f32; 4]) {
    if walk.len() < 2 {
        return;
    }
    let profile = bore_profile(hw);
    let lines: Vec<Vec<Vec2>> = profile.iter().map(|&(x, _)| offset_path(walk, x)).collect();
    // A point of the section (profile index k) at walk point i.
    let at = |k: usize, i: usize| {
        let q = lines[k][i];
        Vec3::new(q.x, profile[k].1, q.y)
    };
    let inward3 = |right: Vec2, x: f32, y: f32| {
        let d = bore_inward(x, y);
        Vec3::new(right.x * d.x, d.y, right.y * d.x)
    };
    for i in 0..walk.len() - 1 {
        let d = (walk[i + 1] - walk[i]).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        for k in 0..profile.len() - 1 {
            let (mx, my) = ((profile[k].0 + profile[k + 1].0) * 0.5, (profile[k].1 + profile[k + 1].1) * 0.5);
            g.part(Concrete).quad_facing([at(k, i), at(k, i + 1), at(k + 1, i + 1), at(k + 1, i)], inward3(right, mx, my), lining);
        }
        // The invert under the carriageway and its kerbs.
        let (l, r) = (lines[profile.len() - 1][i], lines[0][i]);
        let (l1, r1) = (lines[profile.len() - 1][i + 1], lines[0][i + 1]);
        g.part(Concrete).quad_facing([Vec3::new(l.x, 0.01, l.y), Vec3::new(l1.x, 0.01, l1.y), Vec3::new(r1.x, 0.01, r1.y), Vec3::new(r.x, 0.01, r.y)], Vec3::Y, rib);
    }
    // Lamp lines high on each wall, a hand's breadth proud of the lining.
    for side in [-1.0f32, 1.0] {
        let x = side * (hw + TUNNEL_WALL - 0.06);
        let line = offset_path(walk, x);
        for i in 0..line.len() - 1 {
            let d = (walk[i + 1] - walk[i]).normalize_or_zero();
            let into = Vec3::new(d.y * side, 0.0, -d.x * side);
            let (a, b) = (line[i], line[i + 1]);
            let (y0, y1) = (BORE_SPRING - 0.45, BORE_SPRING - 0.2);
            g.part(Glow).quad_facing([Vec3::new(a.x, y0, a.y), Vec3::new(b.x, y0, b.y), Vec3::new(b.x, y1, b.y), Vec3::new(a.x, y1, a.y)], into, lamp);
        }
    }
    // Lining bands: a ring of quads standing a little proud of the lining.
    let line = Polyline::new(walk);
    let len = path_length(walk);
    let mut s = BORE_RIB_EVERY * 0.5;
    while s < len - 1.0 {
        let (p0, d0) = line.at((s - 0.35).max(0.0));
        let (p1, d1) = line.at((s + 0.35).min(len));
        let (r0, r1) = (Vec2::new(-d0.y, d0.x), Vec2::new(-d1.y, d1.x));
        let ring = |p: Vec2, right: Vec2, k: usize| {
            let (x, y) = profile[k];
            let inset = bore_inward(x, y) * 0.14;
            let q = p + right * (x + inset.x);
            Vec3::new(q.x, y + inset.y, q.y)
        };
        for k in 0..profile.len() - 1 {
            let (mx, my) = ((profile[k].0 + profile[k + 1].0) * 0.5, (profile[k].1 + profile[k + 1].1) * 0.5);
            g.part(Concrete).quad_facing([ring(p0, r0, k), ring(p1, r1, k), ring(p1, r1, k + 1), ring(p0, r0, k + 1)], inward3(r0, mx, my), rib);
        }
        s += BORE_RIB_EVERY;
    }
}
