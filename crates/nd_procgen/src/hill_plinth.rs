//! [r5-hillside] Storeys stepping down the hill under a building.
//!
//! A deep terraced lot on a steep hillside stands on two pads
//! (`nd_sim::landscape::PadStep`): the front one at its street's height,
//! where the building's door is, and behind a step a back one down the
//! hill. The building is drawn on its front pad; over the back pad its
//! back part goes on down to the lower ground in plain storeys of board-
//! marked concrete with rows of windows (lit at night like the building's
//! own), floor bands and a shuttered service door at the bottom: the Hong
//! Kong Mid-Levels look, a street door at the top and floors stepping down
//! the hill behind it.

use crate::buildings::WINDOW_LIGHT_MARK;
use crate::color::{mul, rgba};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::landscape::PadStep;

/// Height of one storey of the plinth.
pub const PLINTH_STOREY: f32 = 3.2;

/// Surfaces a building's walls are drawn in: its footprint is read off
/// their lowest vertices.
const MASS: [Surface; 10] = [Surface::Concrete, Surface::Plaster, Surface::Brick, Surface::Stone, Surface::Panel, Surface::Glass, Surface::WornConcrete, Surface::WornPlaster, Surface::WornBrick, Surface::WornPanel];

/// The storeys under `building` (drawn on its front pad `pad`, heights
/// relative to that pad) where it stands over `step`'s back pad lower down:
/// empty if the back pad is not lower or the building does not reach over
/// it. `dynamic_lights` marks the windows for the renderer's switching.
pub fn hill_plinth(building: &Geometry, step: &PadStep, pad: f32, seed: u64, palette: &Palette, dynamic_lights: bool) -> Geometry {
    let mut g = Geometry::default();
    let drop = pad - step.back;
    if drop < 1.0 {
        return g;
    }
    let len = step.a.distance(step.b);
    let u = (step.b - step.a).normalize_or_zero();
    let d = step.back_dir;
    // The building's ground floor over the back part, in the step's frame.
    let (mut u0, mut u1, mut dmax) = (f32::MAX, f32::MIN, f32::MIN);
    for s in MASS {
        for p in &building.get(s).positions {
            if p[1] > 0.8 {
                continue;
            }
            let q = Vec2::new(p[0], p[2]) - step.a;
            let dd = q.dot(d);
            if dd <= 0.3 {
                continue;
            }
            let uu = q.dot(u);
            u0 = u0.min(uu);
            u1 = u1.max(uu);
            dmax = dmax.max(dd);
        }
    }
    let (u0, u1) = (u0.max(0.0), u1.min(len));
    if u1 - u0 < 3.0 || dmax < 2.0 {
        return g;
    }
    let at = |uu: f32, dd: f32| step.a + u * uu + d * dd;
    let v = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
    let base = -drop - 0.3;
    let top = 0.05;
    let wall = rgba(mul(palette.concrete(0), 0.62));
    let band = rgba(mul(palette.concrete(0), 0.48));
    // Three faces out to the hill (the fourth stands against the step's
    // wall): (start, end, outward).
    let back = (at(u1, dmax), at(u0, dmax), d);
    let left = (at(u0, dmax), at(u0, -0.2), -u);
    let right = (at(u1, -0.2), at(u1, dmax), u);
    let faces = [back, left, right];
    for &(a, b, out) in &faces {
        let o3 = Vec3::new(out.x, 0.0, out.y);
        g.part(Surface::WornConcrete).quad_facing([v(a, base), v(b, base), v(b, top), v(a, top)], o3, wall);
    }
    // The underside is the hill; the top is the building's floor.
    let storeys = ((drop / PLINTH_STOREY).round() as usize).max(1);
    let h = drop / storeys as f32;
    let mut k = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut next = || {
        k ^= k >> 29;
        k = k.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        (k >> 40) as f32 / (1u64 << 24) as f32
    };
    for (fi, &(a, b, out)) in faces.iter().enumerate() {
        let o3 = Vec3::new(out.x, 0.0, out.y);
        let flen = a.distance(b);
        let along = (b - a).normalize_or_zero();
        // Floor bands, a little proud of the face.
        for s in 1..storeys {
            let y = -drop + h * s as f32;
            g.part(Surface::Concrete).beam(v(a + out * 0.08, y), v(b + out * 0.08, y), 0.22, band);
        }
        // Windows: a row to each storey, every 2.6 m, some dark, a few
        // shuttered; the bottom storey of the back face has a service door.
        let n = (flen / 2.6).floor() as usize;
        if n == 0 {
            continue;
        }
        let step_t = flen / n as f32;
        for s in 0..storeys {
            let y0 = -drop + h * s as f32;
            let (wy0, wy1) = (y0 + h * 0.32, y0 + h * 0.32 + (h * 0.48).min(1.6));
            for j in 0..n {
                let t = step_t * (j as f32 + 0.5);
                let c = a + along * t;
                if fi == 0 && s == 0 && j == n / 2 {
                    // The service door: a roll shutter and a lamp over it.
                    let (p0, p1) = (c - along * 1.4, c + along * 1.4);
                    let o = out * 0.05;
                    g.part(Surface::Metal).quad_facing([v(p0 + o, y0 + 0.02), v(p1 + o, y0 + 0.02), v(p1 + o, y0 + 2.6), v(p0 + o, y0 + 2.6)], o3, rgba(mul(palette.steel, 0.55)));
                    let lamp = c + out * 0.25;
                    g.part(Surface::Glow).boxed(v(lamp, y0 + 2.95), Vec3::new(0.5, 0.12, 0.2), along.y.atan2(along.x), rgba(palette.window_cold));
                    continue;
                }
                let (p0, p1) = (c - along * 0.65, c + along * 0.65);
                let o = out * 0.04;
                let quad = [v(p0 + o, wy0), v(p1 + o, wy0), v(p1 + o, wy1), v(p0 + o, wy1)];
                let roll = next();
                if roll < 0.14 {
                    g.part(Surface::Metal).quad_facing(quad, o3, rgba(mul(palette.steel, 0.45)));
                } else if roll < 0.62 || !dynamic_lights {
                    g.part(Surface::Glass).quad_facing(quad, o3, rgba(palette.glass_dark));
                } else {
                    let col = if next() < 0.7 { palette.window_warm } else { palette.window_cold };
                    let glow = g.part(Surface::Glow);
                    let from = glow.colors.len();
                    glow.quad_facing(quad, o3, rgba(col));
                    let rank = crate::hash::rank01(quad[0].to_array());
                    for c in &mut glow.colors[from..] {
                        c[3] = WINDOW_LIGHT_MARK + rank;
                    }
                    // Dark glass just behind, for when the light is off.
                    let behind = quad.map(|q| q - o3 * 0.02);
                    g.part(Surface::Glass).quad_facing(behind, o3, rgba(palette.glass_dark));
                }
                // A sill under each opening.
                g.part(Surface::Concrete).beam(v(p0 + out * 0.08, wy0 - 0.06), v(p1 + out * 0.08, wy0 - 0.06), 0.1, band);
            }
        }
    }
    // A drainpipe down a back corner and stains of run-off under the
    // floor bands: grit.
    let pipe = at(u0 + 0.3, dmax) + d * 0.12;
    g.part(Surface::Metal).beam(v(pipe, base + 0.3), v(pipe, top), 0.12, rgba(mul(palette.steel, 0.5)));
    g
}

/// [r5-hillside] Entrance steps for a building whose pad stands off its
/// street's height (`street` and `pad` absolute): one flight along the
/// lot's front edge (`front`, from the corner to the corner), centred on
/// it, from the pavement's height to the pad, with a steel rail on the
/// open side and a lamp at the top. Up to a pad above the street the
/// flight stands at the back of the pavement against the lot's front wall;
/// down to one below, just inside the lot. Empty under 0.6 m (the door is
/// at street level) or past 8 m (the street door is the building's own).
/// Absolute heights.
pub fn entrance_stair(front: [Vec2; 2], front_dir: Vec2, street: f32, pad: f32, palette: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let rise = pad - street;
    if rise.abs() < 0.6 || rise.abs() > 8.0 {
        return g;
    }
    let len = front[0].distance(front[1]);
    let run = rise.abs() / 0.62;
    if run + 1.0 > len {
        return g;
    }
    let along = (front[1] - front[0]) / len;
    let out = front_dir.normalize_or_zero();
    // Out on the pavement going up, inside the lot going down.
    let side = if rise > 0.0 { out * 0.8 } else { -out * 0.8 };
    let mid = (front[0] + front[1]) * 0.5 + side;
    let (lo_end, hi_end) = (mid - along * (run * 0.5), mid + along * (run * 0.5));
    let (zlo, zhi) = (street.min(pad), street.max(pad));
    // Going up the flight climbs from `lo_end` (street) to `hi_end` (pad);
    // going down from `hi_end` (street) to `lo_end` (pad): the low end is
    // `lo_end` either way.
    let steps = (rise.abs() / 0.17).ceil().max(2.0) as usize;
    let tread = run / steps as f32;
    let yaw = along.y.atan2(along.x);
    let c = rgba(mul(palette.concrete(0), 0.8));
    for k in 0..steps {
        let t = tread * (k as f32 + 0.5);
        let q = lo_end + along * t;
        let top = zlo + rise.abs() * (k as f32 + 1.0) / steps as f32;
        // Solid under each step, down to the low end's level.
        g.part(Surface::WornConcrete).boxed(Vec3::new(q.x, (top + zlo - 0.3) * 0.5, q.y), Vec3::new(tread + 0.01, top - zlo + 0.3, 1.3), yaw, c);
    }
    // The rail on the open side (away from the wall), with posts.
    let open = side.normalize_or_zero() * 0.6;
    let v = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
    let steel = rgba(palette.steel);
    let (r0, r1) = (lo_end + open, hi_end + open);
    g.part(Surface::Metal).beam(v(r0, zlo + 1.0), v(r1, zhi + 1.0), 0.06, steel);
    let posts = (run / 1.5).ceil().max(1.0) as usize;
    for k in 0..=posts {
        let f = k as f32 / posts as f32;
        let q = r0.lerp(r1, f);
        let z = zlo + (zhi - zlo) * f;
        g.part(Surface::Metal).beam(v(q, z), v(q, z + 1.0), 0.05, steel);
    }
    // A lamp over the top of the flight.
    g.part(Surface::Glow).boxed(v(hi_end + open * 0.5, zhi + 2.4), Vec3::new(0.35, 0.1, 0.18), yaw, rgba(palette.window_warm));
    g.part(Surface::Metal).beam(v(hi_end + open, zhi), v(hi_end + open, zhi + 2.45), 0.06, steel);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_building(lo: Vec2, hi: Vec2, h: f32) -> Geometry {
        let mut g = Geometry::default();
        let c = (lo + hi) * 0.5;
        g.part(Surface::Concrete).boxed(Vec3::new(c.x, h * 0.5, c.y), Vec3::new(hi.x - lo.x, h, hi.y - lo.y), 0.0, [1.0; 4]);
        g
    }

    /// A building over a back pad 9 m lower gets three storeys under its
    /// back part, down to the lower pad, with windows; none over a back pad
    /// that is higher.
    #[test]
    fn storeys_step_down_to_the_back_pad() {
        let pal = Palette::default();
        // The lot runs back along +y from its street at y = 0; the step
        // crosses it at y = 15.
        let b = box_building(Vec2::new(2.0, 2.0), Vec2::new(18.0, 26.0), 12.0);
        let step = PadStep { a: Vec2::new(0.0, 15.0), b: Vec2::new(20.0, 15.0), back_dir: Vec2::Y, back: 41.0 };
        let g = hill_plinth(&b, &step, 50.0, 7, &pal, true);
        let ys: Vec<f32> = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).collect();
        let lowest = ys.iter().copied().fold(f32::MAX, f32::min);
        assert!(lowest <= -9.0 && lowest > -10.0, "down to the back pad: {lowest}");
        for (_, m) in g.iter() {
            for p in &m.positions {
                assert!(p[2] >= 14.5 && p[2] <= 26.5 && p[0] >= 1.5 && p[0] <= 18.5, "under the building's back part: {p:?}");
            }
        }
        let lit = g.get(Surface::Glow).colors.iter().filter(|c| c[3] >= WINDOW_LIGHT_MARK).count();
        assert!(lit > 0, "lit windows");
        assert!(g.triangle_count() < 1500, "{} triangles", g.triangle_count());
        let up = PadStep { back: 58.0, ..step };
        assert!(hill_plinth(&b, &up, 50.0, 7, &pal, true).triangle_count() == 0);
        // A building on the front part alone has nothing to step down.
        let front_only = box_building(Vec2::new(2.0, 2.0), Vec2::new(18.0, 14.0), 12.0);
        assert!(hill_plinth(&front_only, &step, 50.0, 7, &pal, true).triangle_count() == 0);
    }

    /// Entrance steps climb from the street to a pad above it, along the
    /// front, all within the frontage; none at street level.
    #[test]
    fn entrance_steps_climb_from_the_street_to_the_pad() {
        let pal = Palette::default();
        let front = [Vec2::new(0.0, 0.0), Vec2::new(16.0, 0.0)];
        let g = entrance_stair(front, -Vec2::Y, 20.0, 23.0, &pal);
        let ys: Vec<f32> = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).collect();
        assert!(ys.iter().copied().fold(f32::MAX, f32::min) >= 19.6);
        assert!(ys.iter().copied().fold(f32::MIN, f32::max) > 23.0);
        for (_, m) in g.iter() {
            for p in &m.positions {
                assert!(p[0] > 0.0 && p[0] < 16.0 && p[2] < 0.1 && p[2] > -2.0, "on the pavement along the front: {p:?}");
            }
        }
        assert!(entrance_stair(front, -Vec2::Y, 20.0, 20.3, &pal).triangle_count() == 0);
        let down = entrance_stair(front, -Vec2::Y, 20.0, 17.0, &pal);
        assert!(down.iter().all(|(_, m)| m.positions.iter().all(|p| p[2] > -0.1)), "inside the lot going down");
    }
}
