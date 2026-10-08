//! [social] What the city's crews and clubs make, as props: a street-art
//! crew's mural (a free-standing painted wall at a lot's front, its face
//! to the street, bands and shapes in the crew's colours with a few neon
//! strokes) and a community garden on a vacant lot (raised beds of greens
//! and flowers, a shed, a water butt, a hand-painted sign).
//!
//! Laid out in world coordinates from a seed, so each keeps its look.

use crate::buildings::parts::{mul, pick, rgba};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use rand::Rng;

/// Height of a lot's ground.
const LOT_Y: f32 = 0.04;

/// The paint: neon and street colours (linear).
const PAINT: [[f32; 3]; 10] = [
    [0.95, 0.12, 0.45],
    [0.1, 0.75, 0.95],
    [0.98, 0.75, 0.08],
    [0.35, 0.95, 0.25],
    [0.6, 0.2, 0.95],
    [0.98, 0.35, 0.08],
    [0.05, 0.25, 0.75],
    [0.9, 0.9, 0.85],
    [0.08, 0.08, 0.1],
    [0.85, 0.15, 0.12],
];

/// A painted wall `width` x `height` standing at `pos` (on the ground),
/// running along `along`, its painted face towards `facing`. `quality`
/// (`-1..1`) is art or a tag: good murals get big shapes and neon, poor
/// ones a scrawl on grey.
pub fn mural_wall(pos: Vec2, along: Vec2, facing: Vec2, width: f32, height: f32, quality: f32, seed: u64) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 0x3A1A);
    let along = along.try_normalize().unwrap_or(Vec2::X);
    let mut out = facing.try_normalize().unwrap_or(Vec2::new(-along.y, along.x));
    out = (out - along * out.dot(along)).try_normalize().unwrap_or(Vec2::new(-along.y, along.x));
    let yaw = along.y.atan2(along.x);
    let thick = 0.3;
    let a3 = Vec3::new(along.x, 0.0, along.y);
    let o3 = Vec3::new(out.x, 0.0, out.y);
    let up = Vec3::Y;
    let base = Vec3::new(pos.x, LOT_Y, pos.y);
    // The wall itself, with a coping on top.
    g.part(WornConcrete).boxed(base + up * (height * 0.5), Vec3::new(width, height, thick), yaw, rgba([0.42, 0.41, 0.4]));
    g.part(Concrete).boxed(base + up * (height + 0.06), Vec3::new(width + 0.1, 0.12, thick + 0.12), yaw, rgba([0.5, 0.5, 0.48]));
    let face = base + o3 * (thick * 0.5 + 0.01);
    let at = |u: f32, v: f32| face + a3 * u + up * v;
    let palette: Vec<[f32; 3]> = (0..3).map(|_| pick(&mut rng, &PAINT)).collect();
    if quality >= -0.2 {
        // A background wash, then bands and blocks.
        g.part(Matte).decal(at(0.0, height * 0.5), a3, up, width * 0.96, height * 0.92, o3, rgba(mul(palette[0], 0.55)));
        let bands = rng.random_range(3..6);
        for k in 0..bands {
            let w = rng.random_range(0.6..(width * 0.35).max(0.8));
            let h = rng.random_range(0.6..height * 0.8);
            let u = rng.random_range(-width * 0.45 + w * 0.5..(width * 0.45 - w * 0.5).max(-width * 0.45 + w * 0.5 + 0.01));
            let v = rng.random_range(0.2 + h * 0.5..(height - 0.2 - h * 0.5).max(0.21 + h * 0.5));
            let off = o3 * (0.005 * (k + 1) as f32);
            g.part(Matte).decal(at(u, v) + off, a3, up, w, h, o3, rgba(palette[1 + k % 2]));
        }
        // A big round shape: a sun, a face, an eye.
        let r = (height * 0.32).min(width * 0.2);
        let c = at(rng.random_range(-width * 0.25..width * 0.25), height * 0.55) + o3 * 0.04;
        let ring: Vec<Vec3> = (0..12).map(|k| c + (a3 * (k as f32 / 12.0 * std::f32::consts::TAU).cos() + up * (k as f32 / 12.0 * std::f32::consts::TAU).sin()) * r).collect();
        g.part(Matte).cap(&ring, o3, rgba(pick(&mut rng, &PAINT)));
        // Neon strokes on the good ones.
        if quality > 0.15 {
            for k in 0..3 {
                let v = 0.4 + k as f32 * (height - 0.8) / 3.0;
                g.part(Glow).decal(at(0.0, v) + o3 * 0.05, a3, up, width * rng.random_range(0.4..0.85), 0.07, o3, rgba(mul(palette[k % 3], 1.6)));
            }
        }
    } else {
        // A tag: scrawls on the grey.
        for _ in 0..5 {
            let w = rng.random_range(0.8..2.2);
            let u = rng.random_range(-width * 0.4..width * 0.4);
            let v = rng.random_range(0.5..height * 0.7);
            g.part(Matte).decal(at(u, v), a3, up, w, 0.25, o3, rgba(pick(&mut rng, &PAINT)));
        }
    }
    g
}

/// A community garden on a lot around `center`: rows of raised beds square
/// to `front` (the way to the street), a shed and a sign.
pub fn garden(center: Vec2, front: Vec2, size: f32, seed: u64) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 0x6A4D);
    let out = front.try_normalize().unwrap_or(Vec2::Y);
    let along = Vec2::new(-out.y, out.x);
    let yaw = along.y.atan2(along.x);
    let at = |u: f32, v: f32, h: f32| {
        let p = center + along * u + out * v;
        Vec3::new(p.x, LOT_Y + h, p.y)
    };
    let span = (size * 0.38).clamp(4.0, 14.0);
    let rows = ((span * 2.0 / 2.4) as i32).clamp(2, 6);
    let beds = ((span * 2.0 / 3.4) as i32).clamp(2, 5);
    let greens = [[0.08, 0.35, 0.06], [0.15, 0.45, 0.08], [0.05, 0.28, 0.1], [0.25, 0.4, 0.05]];
    let flowers = [[0.9, 0.2, 0.35], [0.95, 0.75, 0.1], [0.6, 0.2, 0.85], [0.95, 0.45, 0.1]];
    for r in 0..rows {
        let v = -span + 1.2 + r as f32 * 2.4;
        for b in 0..beds {
            let u = -span + 1.7 + b as f32 * 3.4;
            g.part(Wood).boxed_open(at(u, v, 0.2), Vec3::new(2.6, 0.4, 1.2), yaw, rgba([0.35, 0.22, 0.12]));
            g.part(Dirt).decal(at(u, v, 0.41), Vec3::new(along.x, 0.0, along.y), Vec3::new(out.x, 0.0, out.y), 2.4, 1.0, Vec3::Y, rgba([0.18, 0.12, 0.07]));
            // Plants: little tufts, now and then flowers.
            let n = rng.random_range(3..6);
            for k in 0..n {
                let du = -1.0 + k as f32 * 2.0 / (n - 1).max(1) as f32;
                let h = rng.random_range(0.2..0.6);
                let col = if rng.random_bool(0.25) { pick(&mut rng, &flowers) } else { pick(&mut rng, &greens) };
                g.part(Foliage).boxed(at(u + du, v + rng.random_range(-0.25..0.25), 0.4 + h * 0.5), Vec3::new(0.35, h, 0.35), yaw + k as f32, rgba(col));
            }
        }
    }
    // A shed, a water butt, a sign by the street.
    g.part(Wood).boxed(at(span - 1.2, span - 1.2, 1.1), Vec3::new(2.0, 2.2, 1.6), yaw, rgba([0.22, 0.32, 0.25]));
    g.part(Matte).cylinder(center + along * (span - 2.6) + out * (span - 0.8), 0.35, 0.35, LOT_Y, LOT_Y + 0.9, 8, rgba([0.1, 0.2, 0.12]));
    g.part(Wood).boxed(at(-span + 0.6, span + 0.4, 0.6), Vec3::new(0.1, 1.2, 0.1), yaw, rgba([0.3, 0.2, 0.1]));
    g.part(Matte).decal(at(-span + 0.6, span + 0.46, 1.25), Vec3::new(along.x, 0.0, along.y), Vec3::Y, 1.2, 0.55, Vec3::new(out.x, 0.0, out.y), rgba([0.9, 0.85, 0.6]));
    g
}
