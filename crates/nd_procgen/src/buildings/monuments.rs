//! [pt-visual] Monuments the player places (F40: the "special" monument
//! read as an office block with a helipad). Each is a monument first, in the
//! neon city's way: a stone or bronze form a kilometre can read, lit by thin
//! neon lines after dark. Obelisks with glowing pyramidions, colossal statues
//! holding a torch, triumphal arches facing the road, memorial plazas with a
//! wall of names and a field of stelae, and an upright halo ring round a
//! floating core. The tier sets the scale and the menu; the seed picks one.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::Tier;
use rand::Rng;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Design {
    Statue,
    Memorial,
    Obelisk,
    Arch,
    Halo,
    Crystal,
}

/// The menu for a tier.
pub(crate) fn designs(tier: Tier) -> &'static [Design] {
    match tier {
        Tier::Low => &[Design::Statue, Design::Memorial, Design::Obelisk],
        Tier::Mid => &[Design::Obelisk, Design::Arch, Design::Statue, Design::Memorial],
        Tier::High => &[Design::Halo, Design::Statue, Design::Obelisk, Design::Arch, Design::Crystal],
    }
}

/// A frame at the monument's centre: `t` along the road, `d` towards it.
fn centre_face(m: Vec2, front: &Face) -> Face {
    Face::new(m - front.tangent, m + front.tangent, front.out)
}

/// Rectangle `hw` along and `hd` across, centred `u`, `d` on the face,
/// counter-clockwise as the solids want it.
fn rect_on(fc: &Face, u: f32, d: f32, hw: f32, hd: f32) -> Vec<Vec2> {
    ccw(vec![fc.at(u - hw, d - hd), fc.at(u + hw, d - hd), fc.at(u + hw, d + hd), fc.at(u - hw, d + hd)])
}

fn ccw(mut p: Vec<Vec2>) -> Vec<Vec2> {
    if nd_sim::zoning::polygon_area(&p) < 0.0 {
        p.reverse();
    }
    p
}

pub fn landmark(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pal = c.palette;
    let _plaza = yard(g, c, 0.4, Stone, mul(pal.paving, 1.25));
    let center = interior_anchor(&c.poly);
    // The room the monument has: the clear distance to the lot's edge.
    let room = distance_to_boundary(center, &c.poly).max(2.0);
    let front = c.front_face(&c.poly);
    let fc = centre_face(center, &front);
    let menu = designs(c.tier);
    let design = menu[rng.random_range(0..menu.len())];
    let neon = pick(rng, &pal.glow_colors);
    let stone = mul(pal.stone, 1.5);
    let scale = match c.tier {
        Tier::Low => 0.7,
        Tier::Mid => 1.0,
        Tier::High => 1.5,
    };
    // Monument footprints stay inside `reach` of the centre.
    let reach = room * 0.85;
    let keep = match design {
        Design::Statue => statue_on_plinth(g, c, &fc, reach, scale, stone, neon),
        Design::Memorial => memorial(g, c, rng, &fc, reach, scale, stone, neon),
        Design::Obelisk => obelisk(g, c, &fc, reach, scale, stone, neon),
        Design::Arch => arch(g, c, &fc, reach, scale, stone, neon),
        Design::Halo => halo(g, c, &fc, reach, scale, stone, neon),
        Design::Crystal => crystal(g, c, rng, &fc, reach, stone),
    };
    // Lamps along the front, trees and benches round the plaza, clear of
    // the monument's own footprint.
    for k in 0..3 {
        let t = -front.len * 0.35 + k as f32 * front.len * 0.35;
        let p = front.at(t, -1.5);
        if p.distance(center) > keep + 1.0 {
            lamp_post_in(g, c, p, 4.5, pal.lamp);
        }
    }
    let f = &c.frame;
    for k in 0..6 {
        let a = k as f32 / 6.0 * TAU + 0.5;
        let p = center + Vec2::new(a.cos(), a.sin()) * (keep + 3.0).max(room * 0.8);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.8 {
            if k % 2 == 0 {
                tree_in(g, f, pal, p, 4.0, 2.8, pal.foliage);
            } else {
                bench_world(g, f.to_world(p.x, p.y), f.yaw() + a + std::f32::consts::FRAC_PI_2, pal.wood, CHARCOAL);
            }
        }
    }
}

/// Stepped square plinth up to `top`; returns its half width.
fn plinth_steps(g: &mut Geometry, c: &Ctx, fc: &Face, half: f32, steps: usize, top: f32, stone: [f32; 3]) -> f32 {
    let f = &c.frame;
    let rise = top / steps as f32;
    for k in 0..steps {
        let hw = half * (1.0 - 0.18 * k as f32);
        extrude(g, f, Stone, &rect_on(fc, 0.0, 0.0, hw, hw), k as f32 * rise, (k + 1) as f32 * rise, mul(stone, 1.0 + 0.04 * k as f32));
    }
    half
}

/// A thin glowing line round a rectangle at height `y`, just proud of it.
fn neon_ring(g: &mut Geometry, c: &Ctx, fc: &Face, hw: f32, hd: f32, y: f32, color: [f32; 3]) {
    let f = &c.frame;
    let r = rect_on(fc, 0.0, 0.0, hw + 0.06, hd + 0.06);
    extrude(g, f, Glow, &r, y - 0.07, y + 0.07, color);
}

/// A colossal figure in a long coat, one arm raised with a torch, a lit
/// visor across the face. `h` tall, standing on `y0`, `u`/`d` on the face.
#[allow(clippy::too_many_arguments)]
fn figure(g: &mut Geometry, c: &Ctx, fc: &Face, u: f32, d: f32, y0: f32, h: f32, metal: [f32; 3], neon: [f32; 3]) {
    let f = &c.frame;
    let b = |g: &mut Geometry, s: Surface, du: f32, y: f32, dd: f32, size: Vec3, col: [f32; 3]| fc.bx(g, f, s, u + du, y0 + y, d + dd, size, col);
    // Legs and boots.
    for s in [-1.0f32, 1.0] {
        b(g, Metal, s * 0.08 * h, 0.23 * h, 0.0, Vec3::new(0.11 * h, 0.46 * h, 0.13 * h), metal);
        b(g, Metal, s * 0.08 * h, 0.03 * h, 0.02 * h, Vec3::new(0.13 * h, 0.06 * h, 0.18 * h), mul(metal, 0.8));
    }
    // Coat: a tapered body from the knees to the shoulders.
    let lo = rect_on(fc, u, d, 0.15 * h, 0.08 * h);
    let hi = rect_on(fc, u, d, 0.2 * h, 0.09 * h);
    loft(g, f, Metal, &lo, &hi, y0 + 0.3 * h, y0 + 0.8 * h, metal);
    // Shoulders, neck, head with the visor.
    b(g, Metal, 0.0, 0.82 * h, 0.0, Vec3::new(0.44 * h, 0.05 * h, 0.17 * h), mul(metal, 0.9));
    b(g, Metal, 0.0, 0.86 * h, 0.0, Vec3::new(0.07 * h, 0.04 * h, 0.07 * h), metal);
    b(g, Metal, 0.0, 0.93 * h, 0.0, Vec3::new(0.12 * h, 0.12 * h, 0.12 * h), metal);
    b(g, Glow, 0.0, 0.94 * h, 0.061 * h + 0.03, Vec3::new(0.11 * h, 0.022 * h, 0.02), neon);
    // One arm down, one raised with a torch.
    b(g, Metal, -0.25 * h, 0.62 * h, 0.0, Vec3::new(0.07 * h, 0.34 * h, 0.08 * h), metal);
    b(g, Metal, 0.25 * h, 0.98 * h, 0.0, Vec3::new(0.07 * h, 0.34 * h, 0.08 * h), metal);
    b(g, Metal, 0.25 * h, 1.17 * h, 0.0, Vec3::new(0.09 * h, 0.05 * h, 0.09 * h), mul(metal, 0.8));
    b(g, Glow, 0.25 * h, 1.24 * h, 0.0, Vec3::new(0.07 * h, 0.1 * h, 0.07 * h), [2.6, 1.3, 0.35]);
}

#[allow(clippy::too_many_arguments)]
fn statue_on_plinth(g: &mut Geometry, c: &Ctx, fc: &Face, reach: f32, scale: f32, stone: [f32; 3], neon: [f32; 3]) -> f32 {
    let pal = c.palette;
    let h = (reach * 1.5).clamp(5.0, 12.0) * scale;
    let half = (h * 0.32).min(reach);
    let top = (h * 0.28).clamp(1.5, 6.0);
    plinth_steps(g, c, fc, half, 3, top, stone);
    neon_ring(g, c, fc, half * 0.64, half * 0.64, top - 0.2, neon);
    // An inscription on the plinth's road face.
    let f = &c.frame;
    fc.decal(g, f, Glow, 0.0, top * 0.45, half * 0.82 + 0.06, half * 1.0, 0.25, mul(neon, 0.8));
    let metal = if c.tier == Tier::High { mul(pal.steel, 0.6) } else { pal.bronze };
    figure(g, c, fc, 0.0, 0.0, top, h, metal, neon);
    half
}

#[allow(clippy::too_many_arguments)]
fn memorial(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, fc: &Face, reach: f32, scale: f32, stone: [f32; 3], neon: [f32; 3]) -> f32 {
    let f = &c.frame;
    let granite = [0.05, 0.05, 0.06];
    let r = reach.min(14.0 * scale.max(1.0));
    // The wall of names across the back, dark granite, the names lit.
    let wall_len = (r * 1.5).max(4.0);
    let wall_d = -r * 0.55;
    let wall_h = (2.6 * scale).max(2.0);
    fc.bx(g, f, Stone, 0.0, wall_h * 0.5, wall_d, Vec3::new(wall_len, wall_h, 0.8), granite);
    fc.bx(g, f, Stone, 0.0, wall_h + 0.1, wall_d, Vec3::new(wall_len + 0.4, 0.2, 1.0), mul(stone, 0.9));
    let rows = (wall_h / 0.35) as i32 - 1;
    for k in 0..rows {
        let y = 0.45 + k as f32 * 0.32;
        let mut u = -wall_len * 0.45;
        while u < wall_len * 0.45 {
            let w = rng.random_range(0.5..1.4);
            fc.decal(g, f, Glow, u + w * 0.5, y, wall_d + 0.46, w, 0.06, mul(neon, rng.random_range(0.4..0.8)));
            u += w + 0.35;
        }
    }
    // The eternal flame in a stone bowl in front of it.
    let flame = fc.at(0.0, r * 0.25);
    let fw = f.to_world(flame.x, flame.y);
    g.part(Stone).cylinder(fw, 0.9, 1.3, 0.0, 0.9, 12, rgba(stone));
    g.part(Glow).cylinder(fw, 0.5, 0.05, 0.9, 2.0, 6, rgba([2.6, 1.0, 0.25]));
    // A field of stelae either side: dark blocks of uneven height.
    let mut u = -r * 0.9;
    while u <= r * 0.9 {
        let mut d = wall_d + 1.6;
        while d <= r * 0.7 {
            let p = fc.at(u, d);
            let clear = p.distance(flame) > 2.2;
            if clear && c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.2 {
                let h = rng.random_range(0.4..2.4) * scale.max(0.8);
                fc.bx(g, f, Concrete, u, h * 0.5, d, Vec3::new(0.9, h, 1.6), mul(granite, 3.0));
            }
            d += 2.4;
        }
        u += 1.6;
    }
    r
}

#[allow(clippy::too_many_arguments)]
fn obelisk(g: &mut Geometry, c: &Ctx, fc: &Face, reach: f32, scale: f32, stone: [f32; 3], neon: [f32; 3]) -> f32 {
    let f = &c.frame;
    let pal = c.palette;
    let h = (reach * 3.0).clamp(12.0, 40.0) * scale;
    let w = (h * 0.075).min(reach * 0.45);
    let base = plinth_steps(g, c, fc, (w * 1.9).min(reach), 3, 1.6, stone);
    let y0 = 1.6;
    let shaft_top = y0 + h * 0.9;
    loft(g, f, Stone, &rect_on(fc, 0.0, 0.0, w * 0.5, w * 0.5), &rect_on(fc, 0.0, 0.0, w * 0.32, w * 0.32), y0, shaft_top, mul(stone, 1.1));
    // A glowing pyramidion and neon bands up the shaft.
    loft(g, f, Glow, &rect_on(fc, 0.0, 0.0, w * 0.32, w * 0.32), &rect_on(fc, 0.0, 0.0, w * 0.02, w * 0.02), shaft_top, y0 + h, neon);
    for k in 1..4 {
        let t = k as f32 * 0.22;
        let hw = w * (0.5 - 0.18 * t) + 0.05;
        neon_ring(g, c, fc, hw - 0.06, hw - 0.06, y0 + h * 0.9 * t, mul(neon, 0.8));
    }
    aviation_light(g, pal, f.at2(fc.mid(), y0 + h + 0.3), 0.3);
    // Uplights at the plinth's corners.
    for (su, sd) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        fc.bx(g, f, Glow, su * base * 0.8, 1.65, sd * base * 0.8, Vec3::new(0.3, 0.1, 0.3), mul(pal.lamp, 0.9));
    }
    base
}

#[allow(clippy::too_many_arguments)]
fn arch(g: &mut Geometry, c: &Ctx, fc: &Face, reach: f32, scale: f32, stone: [f32; 3], neon: [f32; 3]) -> f32 {
    let f = &c.frame;
    let pal = c.palette;
    let w = (reach * 1.8).clamp(10.0, 26.0 * scale).min(reach * 1.9);
    let pier = w * 0.2;
    let h = w * 0.95;
    let open_h = h * 0.66;
    let depth = (w * 0.3).min(reach * 0.9);
    for s in [-1.0f32, 1.0] {
        let u = s * (w * 0.5 - pier * 0.5);
        fc.bx(g, f, Stone, u, open_h * 0.5, 0.0, Vec3::new(pier, open_h, depth), stone);
        // Neon down the inner face of each pier.
        fc.bx(g, f, Glow, u - s * (pier * 0.5 + 0.04), open_h * 0.5, 0.0, Vec3::new(0.08, open_h, depth * 0.5), neon);
        // A relief panel on each pier's road face.
        fc.decal(g, f, Glow, u, open_h * 0.55, depth * 0.5 + 0.06, pier * 0.4, open_h * 0.4, mul(pal.lamp, 0.35));
    }
    // The attic over the opening, its soffit lit, an inscription across it.
    fc.bx(g, f, Stone, 0.0, (open_h + h) * 0.5, 0.0, Vec3::new(w, h - open_h, depth), mul(stone, 1.05));
    fc.bx(g, f, Stone, 0.0, h + 0.3, 0.0, Vec3::new(w + 0.6, 0.6, depth + 0.6), mul(stone, 1.1));
    fc.bx(g, f, Glow, 0.0, open_h - 0.04, 0.0, Vec3::new(w - 2.0 * pier, 0.08, depth * 0.5), neon);
    fc.decal(g, f, Glow, 0.0, (open_h + h) * 0.5, depth * 0.5 + 0.06, w * 0.6, 0.4, mul(pal.window_cold, 0.9));
    fc.reversed().decal(g, f, Glow, 0.0, (open_h + h) * 0.5, depth * 0.5 + 0.06, w * 0.6, 0.4, mul(pal.window_cold, 0.9));
    // A figure on the attic.
    figure(g, c, fc, 0.0, 0.0, h + 0.6, (h * 0.45).min(11.0), pal.bronze, neon);
    w * 0.5
}

#[allow(clippy::too_many_arguments)]
fn halo(g: &mut Geometry, c: &Ctx, fc: &Face, reach: f32, scale: f32, stone: [f32; 3], neon: [f32; 3]) -> f32 {
    let f = &c.frame;
    let pal = c.palette;
    let r1 = (reach * 0.9).clamp(4.0, 12.0 * scale);
    let r0 = r1 * 0.82;
    let th = (r1 * 0.08).max(0.4);
    let base_h = 1.2;
    let cy = base_h + r1 + 0.6;
    // A low stepped pedestal the ring stands in.
    let half = (th * 3.0).max(1.5).min(reach);
    plinth_steps(g, c, fc, half.max(r1 * 0.35).min(reach), 2, base_h, stone);
    let up = Vec3::Y;
    let along = f.dir3(fc.tangent);
    let out = f.dir3(fc.out);
    let centre = f.at2(fc.mid(), cy);
    let pt = |r: f32, a: f32, d: f32| centre + along * (r * a.cos()) + up * (r * a.sin()) + out * d;
    let n = 32;
    let body = rgba(mul(pal.steel, 0.55));
    let glow = rgba(neon);
    for k in 0..n {
        let a0 = k as f32 / n as f32 * TAU;
        let a1 = (k + 1) as f32 / n as f32 * TAU;
        let m = Surface::Metal;
        // Front, back, outer and inner rims of the ring.
        g.part(m).quad_facing([pt(r0, a0, th), pt(r1, a0, th), pt(r1, a1, th), pt(r0, a1, th)], out, body);
        g.part(m).quad_facing([pt(r0, a0, -th), pt(r0, a1, -th), pt(r1, a1, -th), pt(r1, a0, -th)], -out, body);
        let ra = along * ((a0 + a1) * 0.5).cos() + up * ((a0 + a1) * 0.5).sin();
        g.part(m).quad_facing([pt(r1, a0, -th), pt(r1, a1, -th), pt(r1, a1, th), pt(r1, a0, th)], ra, body);
        g.part(m).quad_facing([pt(r0, a0, th), pt(r0, a1, th), pt(r0, a1, -th), pt(r0, a0, -th)], -ra, body);
        // A neon line on both faces, near the inner edge.
        let (g0, g1) = (r0 + (r1 - r0) * 0.15, r0 + (r1 - r0) * 0.4);
        g.part(Glow).quad_facing([pt(g0, a0, th + 0.03), pt(g1, a0, th + 0.03), pt(g1, a1, th + 0.03), pt(g0, a1, th + 0.03)], out, glow);
        g.part(Glow).quad_facing([pt(g0, a0, -th - 0.03), pt(g0, a1, -th - 0.03), pt(g1, a1, -th - 0.03), pt(g1, a0, -th - 0.03)], -out, glow);
    }
    // A floating core: a faceted crystal held in the ring.
    let core = r0 * 0.28;
    let cw = f.to_world(fc.mid().x, fc.mid().y);
    g.part(Glass).frustum(&crate::mesh::MeshData::ngon(cw, core * 0.05, 6, 0.0, cy - core), &crate::mesh::MeshData::ngon(cw, core * 0.6, 6, 0.0, cy), rgba(mul(pick_glass(pal), 1.3)));
    g.part(Glass).frustum(&crate::mesh::MeshData::ngon(cw, core * 0.6, 6, 0.0, cy), &crate::mesh::MeshData::ngon(cw, core * 0.05, 6, 0.0, cy + core), rgba(mul(pick_glass(pal), 1.3)));
    g.part(Glow).cylinder(cw, core * 0.18, core * 0.18, cy - 0.15, cy + 0.15, 6, rgba(neon));
    r1
}

fn pick_glass(pal: &crate::palette::Palette) -> [f32; 3] {
    pal.glass.first().copied().unwrap_or(pal.glass_dark)
}

/// The faceted glass sculpture of the first landmarks, kept as one design.
fn crystal(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, fc: &Face, reach: f32, stone: [f32; 3]) -> f32 {
    let f = &c.frame;
    let pal = c.palette;
    let center = fc.mid();
    let r = (reach * 0.6).clamp(1.5, 14.0);
    extrude(g, f, Stone, &ngon(center.x, center.y, (r * 1.25).min(reach), 8, 0.2), 0.0, 1.0, stone);
    extrude(g, f, Stone, &ngon(center.x, center.y, (r * 1.1).min(reach), 8, 0.2), 1.0, 2.0, mul(stone, 1.05));
    let girdle = ngon(center.x, center.y, r, 8, 0.0);
    let waist = r * 0.9;
    let glass = mul(pick(rng, &pal.glass), 1.2);
    loft(g, f, Glass, &ngon(center.x, center.y, r * 0.55, 8, 0.39), &girdle, 2.0, 2.0 + waist * 0.6, glass);
    loft(g, f, Glass, &girdle, &ngon(center.x, center.y, r * 0.45, 8, 0.39), 2.0 + waist * 0.6, 2.0 + waist * 1.1, mul(glass, 1.1));
    let top = 2.0 + waist * 1.1;
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU + 0.39;
        let p = center + Vec2::new(a.cos(), a.sin()) * r * 0.45;
        f.bx(g, Glow, p.x, top + 0.6, p.y, Vec3::new(0.5, 1.2, 0.5), mul(pick(rng, &pal.led_colors), 1.0));
    }
    extrude(g, f, Glow, &ngon(center.x, center.y, r * 1.02, 8, 0.0), 2.0 + waist * 0.6 - 0.15, 2.0 + waist * 0.6 + 0.15, mul(pal.window_cold, 1.1));
    (r * 1.25).min(reach)
}
