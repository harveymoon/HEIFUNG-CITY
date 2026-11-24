//! Stable hashes: the one home of the splitmix and FNV mixes the sim uses
//! for seeds, names and decisions that must not draw from (and so shift)
//! a random stream. [review] These were once copied into a dozen modules;
//! every function here is bit-identical to the copies it replaced, so
//! generated names, seeds and saved keys do not change. Never change the
//! output of one: saves and generated content depend on them.
//!
//! [r16-tidy] Round 16 brought the remaining local copies here too (the
//! renderer's looks, the airport timetable, pressure seeds, the landscape's
//! value noise, the lattice hashes of the textures and the foot wear, the
//! FNV digests of the save fingerprint and the walk routes). Most were
//! small variants of one another (another shift, one round fewer, a 32-bit
//! cousin): each variant is kept under its own name, bit for bit, because
//! a sim fingerprint, a golden test or a saved or drawn look reads it.
//! Not here on purpose: the WGSL shader hashes (they run on the GPU) and
//! the `DefaultHasher` digests the renderer uses only to see whether a
//! chunk changed within one run.

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
    let x = step31(a.wrapping_mul(PHI) ^ b.wrapping_mul(K2) ^ salt);
    x ^ (x >> 29)
}

/// The second odd constant the two-word mixes multiply `b` by.
pub const K2: u64 = 0xC2B2_AE3D_27D4_EB4F;
/// splitmix64's first multiplier.
pub const M1: u64 = 0xBF58_476D_1CE4_E5B9;
/// The 32-bit golden ratio (some sort keys multiply by it, not [`PHI`]).
pub const PHI32: u32 = 0x9E37_79B9;

/// One xor-shift-multiply round: `(x ^ x >> 31)·M1`.
#[inline]
pub fn step31(x: u64) -> u64 {
    (x ^ (x >> 31)).wrapping_mul(M1)
}

/// One xor-shift-multiply round with a 29-bit shift: `(x ^ x >> 29)·M1`
/// (the fire scene's crew looks, the hill plinths' window stream).
#[inline]
pub fn step29(x: u64) -> u64 {
    (x ^ (x >> 29)).wrapping_mul(M1)
}

/// The quickest mix: `(a ^ b·k)·M1`, one multiply, no finaliser. Its low
/// bits are poor: take the high ones (`>> 40`, `>> 58`) or use it as a
/// sort key. [r16-tidy] District names, a homeless sleeper's pose roll,
/// the renderer's grime and haze, the transit layer's mover keys.
#[inline]
pub fn quick_by(a: u64, b: u64, k: u64) -> u64 {
    (a ^ b.wrapping_mul(k)).wrapping_mul(M1)
}

/// [`quick_by`] with `k` = [`PHI`].
#[inline]
pub fn quick(a: u64, b: u64) -> u64 {
    quick_by(a, b, PHI)
}

/// `0..1` from [`quick`].
#[inline]
pub fn quick01(a: u64, b: u64) -> f32 {
    unit(quick(a, b))
}

/// `0..1` from [`mix`]: the props', yards' and household yards' `h01`.
#[inline]
pub fn mix01(a: u64, b: u64) -> f32 {
    unit(mix(a, b))
}

/// `0..1` from [`mix2`]: the lot memory's ghost signs.
#[inline]
pub fn mix2_01(a: u64, b: u64) -> f32 {
    unit(mix2(a, b))
}

/// `0..1` in steps of 1/10,000 from the low digits of a hash.
#[inline]
pub fn per_10k(h: u64) -> f32 {
    (h % 10_000) as f32 / 10_000.0
}

/// `0..1` in 1/10,000 steps from a premixed word: one [`step29`] round and
/// an xor-shift by 32. [r16-tidy] A car's weave phase (`x` = its id·φ ^
/// salt·D6E8…), the gulls over a tip (`a·φ ^ b·C2B2…`).
#[inline]
pub fn tenk01(x: u64) -> f32 {
    let x = step29(x);
    per_10k(x ^ (x >> 32))
}

/// `0..1` from one word: `(h·φ ^ h >> 29)·M1`, top 24 bits (the towers'
/// `unit`; note the shift is of `h`, not of the product).
#[inline]
pub fn spread01(h: u64) -> f32 {
    unit((h.wrapping_mul(PHI) ^ (h >> 29)).wrapping_mul(M1))
}

/// MurmurHash3's 64-bit finaliser (`fmix64`).
#[inline]
pub fn fmix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^ (x >> 33)
}

/// [`fmix64`] of `a ^ b·φ`: the airport timetable's mix.
#[inline]
pub fn mix_fmix(a: u64, b: u64) -> u64 {
    fmix64(a ^ b.wrapping_mul(PHI))
}

/// The pressure seeds' mix: `fmix64`'s constants with shifts 33, 29 and 32
/// (a variant, kept: which lots a pressure opens depends on it).
#[inline]
pub fn mix_pressure(key: u64, salt: u64) -> u64 {
    let mut x = key ^ salt.wrapping_mul(PHI);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 29;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^ (x >> 32)
}

/// A grid point's word: `seed ^ x·φ ^ y·C2B2…` (coordinates sign-extended).
#[inline]
pub fn lattice(seed: u64, x: i64, y: i64) -> u64 {
    seed ^ (x as u64).wrapping_mul(PHI) ^ (y as u64).wrapping_mul(K2)
}

/// `0..1` at a grid point, [`fmix64`] of [`lattice`]: the landscape's
/// value noise.
#[inline]
pub fn lattice01(seed: u64, x: i64, y: i64) -> f32 {
    unit(fmix64(lattice(seed, x, y)))
}

/// `0..1` at a grid point with only the first round of [`fmix64`] (shift,
/// multiply, shift): the city generator's zoning picks.
#[inline]
pub fn lattice01_short(seed: u64, x: i64, y: i64) -> f32 {
    let mut h = lattice(seed, x, y);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    unit(h)
}

/// A 32-bit grid point's word: `x·8DA6… ^ y·D816… ^ salt·CB1A…`.
#[inline]
pub fn lattice32(x: i32, y: i32, salt: u32) -> u32 {
    (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ salt.wrapping_mul(0xCB1A_B31F)
}

/// A 32-bit finaliser (shifts 15, 12, 15; the "lowbias32" family).
#[inline]
pub fn fin32(mut h: u32) -> u32 {
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// MurmurHash2's 32-bit mixing step (shift 13, multiply, shift 15).
#[inline]
pub fn fin32_m2(mut h: u32) -> u32 {
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^ (h >> 15)
}

/// `0..1` from the low 24 bits of a 32-bit hash.
#[inline]
pub fn low24_01(h: u32) -> f32 {
    (h & 0x00FF_FFFF) as f32 / (1u32 << 24) as f32
}

/// `0..1` from the top 24 bits of a 32-bit hash.
#[inline]
pub fn top24_01(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
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

/// [r16-tidy] An FNV-1a word digest fed a word at a time ([`fnv_words`]
/// for code that cannot gather its words into one iterator).
#[derive(Clone, Copy, Debug)]
pub struct Fnv(pub u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(FNV_OFFSET)
    }
}

impl Fnv {
    /// Fold in one whole word.
    #[inline]
    pub fn word(&mut self, w: u64) {
        self.0 = (self.0 ^ w).wrapping_mul(FNV_PRIME);
    }
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

    /// [r16-tidy] The round-16 variants against longhand copies of the
    /// local functions they replaced (file named on each).
    #[test]
    fn round16_variants_are_pinned() {
        // render/fire_scene.rs hash01
        let fire01 = |a: u64| {
            let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            x ^= x >> 31;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^= x >> 29;
            (x % 10_000) as f32 / 10_000.0
        };
        // fire_scene.rs hash
        let crew = |a: u64| {
            let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            x ^= x >> 29;
            x.wrapping_mul(0xBF58_476D_1CE4_E5B9)
        };
        // render/grime.rs, render/haze.rs hash01
        let grime = |a: u64, b: u64| {
            let h = (a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            ((h >> 40) as f32) / (1u64 << 24) as f32
        };
        // render/gulls.rs hash01
        let gulls = |a: u64, b: u64| {
            let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
            x ^= x >> 29;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^= x >> 32;
            (x % 10_000) as f32 / 10_000.0
        };
        // world/driving car_hash
        let car = |id: u64, salt: u64| {
            let x = id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xD6E8_FEB8_6659_FD93);
            let x = (x ^ (x >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            ((x ^ (x >> 32)) % 10_000) as f32 / 10_000.0
        };
        // render/lights.rs building_hash
        let light = |slot: u64| {
            let mut h = slot.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            h ^= h >> 31;
            h.wrapping_mul(0xBF58_476D_1CE4_E5B9) >> 17
        };
        // towers.rs unit
        let tower = |h: u64| {
            let x = h.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (h >> 29);
            let x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            ((x >> 40) as f32) / ((1u64 << 24) as f32)
        };
        // airport.rs mix
        let air = |a: u64, b: u64| {
            let mut x = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            x ^= x >> 33;
            x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
            x ^= x >> 33;
            x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
            x ^ (x >> 33)
        };
        // world/pressure.rs mix
        let pressure = |key: u64, salt: u64| {
            let mut x = key ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            x ^= x >> 33;
            x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
            x ^= x >> 29;
            x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
            x ^ (x >> 32)
        };
        // landscape/noise.rs hash2
        let land = |seed: u64, x: i32, y: i32| {
            let mut h = seed ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
            h ^= h >> 33;
            h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
            h ^= h >> 33;
            h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
            h ^= h >> 33;
            (h >> 40) as f32 / (1u64 << 24) as f32
        };
        // citygen/mod.rs hash01 (on rounded coordinates)
        let city = |seed: u64, x: i64, y: i64| {
            let mut h = seed ^ ((x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)) ^ ((y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
            h ^= h >> 33;
            h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
            h ^= h >> 33;
            (h >> 40) as f32 / (1u64 << 24) as f32
        };
        // render/climate.rs hash
        let climate = |k: u32| {
            let mut x = k.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
            x ^= x >> 15;
            x = x.wrapping_mul(0x2C1B_3C6D);
            x ^= x >> 12;
            x = x.wrapping_mul(0x297A_2D39);
            x ^= x >> 15;
            (x >> 8) as f32 / (1u32 << 24) as f32
        };
        // nd_procgen textures.rs hash
        let tex = |ix: i32, iy: i32, seed: u32| {
            let mut h = (ix as u32).wrapping_mul(0x8da6_b343) ^ (iy as u32).wrapping_mul(0xd8163841) ^ seed.wrapping_mul(0xcb1a_b31f);
            h ^= h >> 15;
            h = h.wrapping_mul(0x2c1b_3c6d);
            h ^= h >> 12;
            h = h.wrapping_mul(0x297a_2d39);
            h ^= h >> 15;
            (h & 0x00ff_ffff) as f32 / 16_777_216.0
        };
        // render/foot_wear.rs hash2
        let wear = |x: i32, y: i32, salt: u32| {
            let mut h = (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ salt.wrapping_mul(0xCB1A_B31F);
            h ^= h >> 13;
            h = h.wrapping_mul(0x5BD1_E995);
            h ^= h >> 15;
            (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
        };
        let mut s = 0x0F1E_2D3C_4B5A_6978u64;
        for _ in 0..2000 {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let (a, b) = (s, s.rotate_left(23) ^ 0x3C);
            let (xi, yi) = ((s >> 7) as i32 % 5000, (s >> 29) as i32 % 5000);
            assert_eq!(per_10k(mix2(a, 0)).to_bits(), fire01(a).to_bits());
            assert_eq!(step29(a.wrapping_mul(PHI)), crew(a));
            assert_eq!(quick01(a, b).to_bits(), grime(a, b).to_bits());
            assert_eq!(tenk01(a.wrapping_mul(PHI) ^ b.wrapping_mul(K2)).to_bits(), gulls(a, b).to_bits());
            assert_eq!(tenk01(a.wrapping_mul(PHI) ^ b.wrapping_mul(0xD6E8_FEB8_6659_FD93)).to_bits(), car(a, b).to_bits());
            assert_eq!(step31(a.wrapping_mul(PHI)) >> 17, light(a));
            assert_eq!(spread01(a).to_bits(), tower(a).to_bits());
            assert_eq!(mix_fmix(a, b), air(a, b));
            assert_eq!(mix_pressure(a, b), pressure(a, b));
            assert_eq!(lattice01(a, xi as i64, yi as i64).to_bits(), land(a, xi, yi).to_bits());
            assert_eq!(lattice01_short(a, xi as i64, -(yi as i64)).to_bits(), city(a, xi as i64, -(yi as i64)).to_bits());
            assert_eq!(top24_01(fin32((a as u32).wrapping_mul(PHI32) ^ 0x85EB_CA6B)).to_bits(), climate(a as u32).to_bits());
            assert_eq!(low24_01(fin32(lattice32(xi, -yi, b as u32))).to_bits(), tex(xi, -yi, b as u32).to_bits());
            assert_eq!(low24_01(fin32_m2(lattice32(-xi, yi, b as u32))).to_bits(), wear(-xi, yi, b as u32).to_bits());
            let mut f = Fnv::default();
            f.word(a);
            f.word(b);
            assert_eq!(f.0, fnv_words([a, b]));
        }
    }
}
