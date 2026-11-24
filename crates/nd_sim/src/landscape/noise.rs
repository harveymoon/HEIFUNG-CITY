//! Value noise for the terrain: a lattice hash, smooth noise and fbm.

use super::*;

pub(super) fn hash2(seed: u64, x: i32, y: i32) -> f32 {
    crate::hash::lattice01(seed, x as i64, y as i64)
}

/// Smooth value noise in 0..1.
pub(super) fn value_noise(seed: u64, p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let a = hash2(seed, x, y);
    let b = hash2(seed, x + 1, y);
    let c = hash2(seed, x, y + 1);
    let d = hash2(seed, x + 1, y + 1);
    let top = a + (b - a) * u.x;
    let bottom = c + (d - c) * u.x;
    top + (bottom - top) * u.y
}

/// Fractal noise in 0..1.
pub fn fbm(seed: u64, p: Vec2, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q) = (0.0, 1.0, 0.0, p);
    for o in 0..octaves {
        sum += value_noise(seed.wrapping_add(o as u64 * 1013), q) * amp;
        norm += amp;
        amp *= 0.5;
        q = q * 2.03 + Vec2::new(17.3, -9.1);
    }
    sum / norm
}
