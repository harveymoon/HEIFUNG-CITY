//! Stateless hashes for picks that must not draw from a random stream (a
//! choice that has to stay put when unrelated input changes), and the
//! named salts that keep their streams apart.

use glam::Vec2;

/// Salt of `nd_sim::fishing::hash`: the boats' names and the supertall
/// sky-garden plots draw from that variant.
pub const SALT_FISHING: u64 = 0xF15F_15F1;

/// Mix two words into one (a splitmix-style finaliser).
pub fn hash2(a: u64, b: u64) -> u64 {
    hash2_salted(a, b, 0)
}

/// [`hash2`] with a salt folded in before the finaliser.
pub fn hash2_salted(a: u64, b: u64, salt: u64) -> u64 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ salt;
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 29)
}

/// A rank in `0..1` from a point in space: the bits of its coordinates,
/// mixed. A window light's place in its building's lighting order.
pub fn rank01(p: [f32; 3]) -> f32 {
    let h = (p[0].to_bits() ^ p[1].to_bits().rotate_left(11) ^ p[2].to_bits().rotate_left(22)).wrapping_mul(0x9E37_79B1);
    ((h >> 8) as f32 / (1u32 << 24) as f32).min(0.999)
}

/// 0..1 from a ground position, the shader-style `fract(sin(x) * 43758)`.
/// Coarse (it repeats at city scale and depends on the platform's `sin`)
/// but kept for the climate trees it already places; prefer [`hash2`] on
/// quantised coordinates for anything new.
pub fn hash01_sin(w: Vec2) -> f32 {
    let x = (w.x * 12.9898 + w.y * 78.233).sin() * 43_758.547;
    x - x.floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_salted_variant_is_the_fishing_hash() {
        for (a, b) in [(0u64, 0u64), (1, 2), (u64::MAX, 77), (123_456_789, 987_654_321)] {
            assert_eq!(hash2_salted(a, b, SALT_FISHING), nd_sim::fishing::hash(a, b));
        }
    }
}
