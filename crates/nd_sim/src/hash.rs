//! Stable hashes: the one home of the splitmix and FNV mixes the sim uses
//! for seeds, names and decisions that must not draw from (and so shift)
//! a random stream. [review] These were once copied into a dozen modules;
//! every function here is bit-identical to the copies it replaced, so
//! generated names, seeds and saved keys do not change. Never change the
//! output of one: saves and generated content depend on them.

/// The golden-ratio increment of splitmix64.
pub const PHI: u64 = 0x9E37_79B9_7F4A_7C15;
/// FNV-1a 64-bit offset basis.
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64-bit prime.
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// The splitmix64 finaliser.
#[inline]
pub fn finish(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A stable 64-bit mix of two numbers (splitmix64 of `a ^ b·φ`), for seeds
/// from seeds.
#[inline]
pub fn mix(a: u64, b: u64) -> u64 {
    finish(a ^ b.wrapping_mul(PHI))
}

/// The shorter two-round mix the waterfront and fishing code use
/// (`a·φ ^ b·C2B2…`, then one multiply); `salt` is xored in first.
#[inline]
pub fn mix2_salted(a: u64, b: u64, salt: u64) -> u64 {
    let mut x = a.wrapping_mul(PHI) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ salt;
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 29)
}

/// [`mix2_salted`] with no salt.
#[inline]
pub fn mix2(a: u64, b: u64) -> u64 {
    mix2_salted(a, b, 0)
}

/// `0..1` from the top 24 bits of a hash.
#[inline]
pub fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// A stable `0..1` from three numbers, for decisions that must not draw
/// from (and so shift) the world's random stream.
pub fn hash01(a: u64, b: u64, c: u64) -> f32 {
    unit(finish(a.wrapping_mul(PHI) ^ b.rotate_left(21).wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ c.rotate_left(42).wrapping_mul(0x94D0_49BB_1331_11EB)))
}

/// FNV-1a over bytes.
pub fn fnv_bytes(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(FNV_OFFSET, |h, b| (h ^ b as u64).wrapping_mul(FNV_PRIME))
}

/// FNV-1a with whole 64-bit words xored in at a time (not bytes), as the
/// parcel cutter seeds its pieces.
pub fn fnv_words(words: impl IntoIterator<Item = u64>) -> u64 {
    words.into_iter().fold(FNV_OFFSET, |h, w| (h ^ w).wrapping_mul(FNV_PRIME))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values the copies this module replaced gave, pinned.
    #[test]
    fn outputs_are_pinned() {
        // Each variant against a longhand copy of its old body.
        let old_mix = |a: u64, b: u64| {
            let mut h = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            h ^= h >> 30;
            h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            h ^= h >> 27;
            h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
            h ^ (h >> 31)
        };
        let old_shore = |a: u64, b: u64| {
            let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
            x ^= x >> 31;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^ (x >> 29)
        };
        let old_fish = |a: u64, b: u64| {
            let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ 0xF15F_15F1;
            x ^= x >> 31;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^ (x >> 29)
        };
        let old_hash01 = |a: u64, b: u64, c: u64| {
            let mut z = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.rotate_left(21).wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ c.rotate_left(42).wrapping_mul(0x94D0_49BB_1331_11EB);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 40) as f32 / (1u64 << 24) as f32
        };
        let mut s = 0x1234_5678_9ABC_DEF0u64;
        for _ in 0..1000 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let (a, b, c) = (s, s.rotate_left(17) ^ 0x55, s >> 7);
            assert_eq!(mix(a, b), old_mix(a, b));
            assert_eq!(mix2(a, b), old_shore(a, b));
            assert_eq!(mix2_salted(a, b, 0xF15F_15F1), old_fish(a, b));
            assert_eq!(hash01(a, b, c).to_bits(), old_hash01(a, b, c).to_bits());
        }
        assert_eq!(fnv_bytes(*b"abc"), "abc".bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)));
    }
}
