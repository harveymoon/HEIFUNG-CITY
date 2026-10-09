//! Colour helpers shared by every generator: linear RGB triples (`[f32; 3]`,
//! may exceed 1 for glow) and the RGBA vertex colours meshes carry.
//!
//! Palette lists are read through [`at`] (wrapping, a grey fallback for an
//! emptied list) or [`pick`] (random, the same fallback), never indexed.

use rand::seq::IndexedRandom;
use rand::Rng;

/// Neutral grey an emptied colour list falls back to.
pub const FALLBACK: [f32; 3] = [0.5, 0.5, 0.5];
/// Near-black for tyres, frames, vents and doors.
pub const DARK: [f32; 3] = [0.09, 0.09, 0.10];
/// Dark grey for posts, housings and machinery.
pub const CHARCOAL: [f32; 3] = [0.16, 0.16, 0.17];

/// An opaque vertex colour.
pub fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

/// A colour scaled by `k` (darker under 1, brighter or glowing over).
pub fn mul(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// `a` towards `b` by `t` (0 is `a`, 1 is `b`).
pub fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Entry `i` of a colour list, wrapping round, or the fallback grey when the
/// list is empty. Every fixed-index read of a palette list goes through
/// here so a hot-reloaded palette with a shortened list cannot panic.
pub fn at(list: &[[f32; 3]], i: usize) -> [f32; 3] {
    if list.is_empty() { FALLBACK } else { list[i % list.len()] }
}

/// A random entry of a colour list, or the fallback grey when it is empty.
pub fn pick(rng: &mut impl Rng, v: &[[f32; 3]]) -> [f32; 3] {
    *v.choose(rng).unwrap_or(&FALLBACK)
}
