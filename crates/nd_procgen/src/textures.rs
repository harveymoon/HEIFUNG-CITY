//! Seamless procedural textures for every [`Surface`]: a colour map in sRGB
//! (mostly grey so vertex colours set the hue) and a tangent-space normal map
//! derived from a height field. Everything tiles because the noise lattice
//! wraps at the texture edge and all patterns are aligned to the tile.

use crate::surface::Surface;

pub struct Texture {
    pub size: usize,
    /// RGBA8, sRGB-encoded, `size * size * 4` bytes.
    pub color: Vec<u8>,
    /// RGBA8 linear tangent-space normals.
    pub normal: Vec<u8>,
}

fn hash(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x8da6_b343) ^ (iy as u32).wrapping_mul(0xd8163841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise on a lattice with `period` cells that wraps seamlessly.
fn vnoise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let fx = smooth(x - xi as f32);
    let fy = smooth(y - yi as f32);
    let w = |ix: i32, iy: i32| hash(ix.rem_euclid(period), iy.rem_euclid(period), seed);
    let a = w(xi, yi);
    let b = w(xi + 1, yi);
    let c = w(xi, yi + 1);
    let d = w(xi + 1, yi + 1);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    top + (bot - top) * fy
}

/// Value noise with separate wrapping periods across and up: stretched
/// features (drips, runs) that still tile seamlessly.
fn vnoise2(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let fx = smooth(x - xi as f32);
    let fy = smooth(y - yi as f32);
    let w = |ix: i32, iy: i32| hash(ix.rem_euclid(px), iy.rem_euclid(py), seed);
    let top = w(xi, yi) + (w(xi + 1, yi) - w(xi, yi)) * fx;
    let bot = w(xi, yi + 1) + (w(xi + 1, yi + 1) - w(xi, yi + 1)) * fx;
    top + (bot - top) * fy
}

/// Fractal noise in 0..1 over a unit tile, seamless.
fn fbm(u: f32, v: f32, base_period: i32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut total = 0.0;
    let mut period = base_period;
    for o in 0..octaves {
        sum += vnoise(u * period as f32, v * period as f32, period, seed + o * 31) * amp;
        total += amp;
        amp *= 0.5;
        period *= 2;
    }
    sum / total
}

/// Pixel-level white noise (hashed by cell, no interpolation).
fn grain(px: i32, py: i32, seed: u32) -> f32 {
    hash(px, py, seed)
}

/// Shortest wrapped distance between two tile coordinates in 0..1.
fn wrap_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(1.0);
    d.min(1.0 - d)
}

struct Sample {
    /// Brightness multiplier (0..1), applied to the vertex colour.
    value: f32,
    /// Hue shift toward warm (+) or cool (-), small.
    tint: f32,
    /// Height in arbitrary units for the normal map.
    height: f32,
}

fn sample(surface: Surface, u: f32, v: f32, px: i32, py: i32) -> Sample {
    match surface {
        Surface::Concrete => {
            // 3 m tile: formwork boards every 0.25 m horizontally, tie holes on a 1 m grid.
            let g = fbm(u, v, 8, 4, 1);
            let board = ((v * 12.0).fract() - 0.5).abs();
            let seam = if board > 0.47 { 0.78 } else { 1.0 };
            let board_tone = 0.96 + 0.08 * (hash((v * 12.0).floor() as i32, 0, 7) - 0.5);
            let streak = fbm(u * 1.0, v * 0.15, 4, 3, 3);
            let streak_dark = 1.0 - 0.14 * (streak - 0.5).max(0.0) * 2.0;
            let hole = {
                let du = wrap_dist(u * 3.0 % 1.0, 0.5) ;
                let dv = wrap_dist(v * 3.0 % 1.0, 0.5);
                if du * du + dv * dv < 0.0012 { 0.55 } else { 1.0 }
            };
            let value = (0.78 + 0.22 * g) * seam * board_tone * streak_dark * hole;
            Sample { value, tint: 0.0, height: g * 0.4 + if seam < 1.0 { -0.6 } else { 0.0 } + if hole < 1.0 { -1.0 } else { 0.0 } }
        }
        Surface::Plaster => {
            let g = fbm(u, v, 6, 4, 11);
            let crack_field = fbm(u, v, 3, 3, 13);
            // Hairlines only where a second field allows, so the contour
            // breaks into short cracks instead of closed loops.
            let crack = if (crack_field - 0.5).abs() < 0.004 && fbm(u, v, 5, 2, 19) > 0.55 { 0.6 } else { 1.0 };
            let blotch = fbm(u, v, 2, 2, 17);
            let value = (0.84 + 0.12 * g) * crack * (0.94 + 0.08 * blotch);
            Sample { value, tint: 0.0, height: g * 0.2 + if crack < 1.0 { -0.8 } else { 0.0 } }
        }
        Surface::Brick => {
            // 1 m tile: 12 courses, 4 bricks per course, running bond. An even
            // count keeps the half-brick offset alternating across the tile
            // seam; 13 put two same-offset courses together at every repeat.
            let courses = 12.0;
            let per = 4.0;
            let row = (v * courses).floor();
            let offset = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
            let bu = u * per + offset;
            let col = bu.floor();
            let fu = bu - col;
            let fv = v * courses - row;
            let mortar_w = 0.10;
            let mortar_h = 0.16;
            let in_mortar = fu < mortar_w || fv < mortar_h;
            let id = hash(col.rem_euclid(per) as i32, row as i32, 23);
            let g = grain(px, py, 29);
            if in_mortar {
                Sample { value: 0.72 + 0.1 * g, tint: 0.0, height: -1.0 }
            } else {
                let face = fbm(u, v, 32, 2, 31);
                Sample { value: (0.74 + 0.26 * id) * (0.9 + 0.1 * face) * (0.95 + 0.1 * g), tint: 0.08 * (id - 0.5), height: 0.3 * face + 0.1 * g }
            }
        }
        Surface::Stone => {
            // 3 m tile: 6 rows of blocks of varying length.
            let rows = 6.0;
            let row = (v * rows).floor();
            let row_jitter = hash(row as i32, 0, 41);
            let fv = v * rows - row;
            let blocks = 3.0 + (row_jitter * 2.0).floor();
            let bu = u * blocks + row_jitter;
            let col = bu.floor();
            let fu = bu - col;
            let gap_u = 0.05;
            let gap_v = 0.08;
            let in_gap = fu < gap_u || fv < gap_v;
            let id = hash(col.rem_euclid(blocks) as i32, row as i32, 43);
            let face = fbm(u, v, 12, 3, 47);
            if in_gap {
                Sample { value: 0.5 + 0.1 * face, tint: 0.0, height: -1.5 }
            } else {
                Sample { value: (0.8 + 0.2 * id) * (0.9 + 0.12 * face), tint: 0.0, height: 0.6 * face + 0.4 * id }
            }
        }
        Surface::Panel => {
            // 2.4 m tile: 2 x 4 panels, dark seams, a fixing in each corner.
            let pu = u * 2.0;
            let pv = v * 4.0;
            let fu = pu.fract();
            let fv = pv.fract();
            let seam = fu < 0.012 || fv < 0.024;
            let id = hash(pu.floor() as i32, pv.floor() as i32, 53);
            let g = fbm(u, v, 16, 2, 59);
            let fix = {
                let du = wrap_dist(fu, 0.04).min(wrap_dist(fu, 0.96));
                let dv = wrap_dist(fv, 0.08).min(wrap_dist(fv, 0.92));
                du < 0.012 && dv < 0.024
            };
            if seam {
                Sample { value: 0.45, tint: 0.0, height: -1.0 }
            } else if fix {
                Sample { value: 0.7, tint: 0.0, height: -0.4 }
            } else {
                Sample { value: (0.86 + 0.1 * id) * (0.96 + 0.06 * g), tint: 0.0, height: 0.1 * g }
            }
        }
        Surface::Metal => {
            // 1 m tile: 13 vertical corrugations plus streaks.
            let rib = (u * 13.0 * std::f32::consts::TAU).sin();
            let shade = 0.82 + 0.18 * (rib * 0.5 + 0.5);
            let streak = fbm(u * 0.3, v, 6, 3, 61);
            let g = grain(px, py, 67);
            let value = shade * (0.9 + 0.1 * streak) * (0.97 + 0.05 * g);
            Sample { value, tint: 0.0, height: rib * 0.8 }
        }
        Surface::Container => {
            // 5.6 m tile: 20 trapezoidal corrugations at the ISO pitch of
            // 0.28 m (outer flat, slope, inner flat, slope). Everything else
            // is low frequency over the whole tile: paint mottling a metre
            // or two across, runs of grime down the valleys, the odd
            // panel ripple, so no pattern repeats within one 6 m unit.
            const RIBS: f32 = 20.0;
            let x = (u * RIBS).fract();
            let (prof, slope) = if x < 0.3 {
                (1.0, false)
            } else if x < 0.48 {
                (1.0 - smooth((x - 0.3) / 0.18), true)
            } else if x < 0.8 {
                (0.0, false)
            } else if x < 0.98 {
                (smooth((x - 0.8) / 0.18), true)
            } else {
                (1.0, false)
            };
            // Baked occlusion: inner flats a touch darker than the outer
            // faces, the slopes darker still.
            let shade = 0.9 + 0.08 * prof - if slope { 0.05 } else { 0.0 };
            let mottle = fbm(u, v, 3, 4, 601);
            let blot = fbm(u, v, 2, 3, 613);
            // Grime runs down the valleys: per-rib strength, stretched up the wall.
            let rib = (u * RIBS).floor() as i32;
            let run = vnoise2(u * RIBS, v * 3.0, RIBS as i32, 3, 607) * hash(rib.rem_euclid(RIBS as i32), 0, 609);
            let run_dark = 1.0 - 0.12 * ((run - 0.25) * 2.0).clamp(0.0, 1.0) * (1.0 - 0.6 * prof);
            // Panel ripple: shallow waves in the sheet that catch the light.
            let ripple = fbm(u, v, 6, 2, 617);
            let g = grain(px, py, 619);
            let value = shade * (0.86 + 0.16 * mottle) * (0.95 + 0.08 * blot) * run_dark * (0.97 + 0.05 * g);
            Sample { value: value.min(1.0), tint: 0.03 * (blot - 0.5), height: prof * 1.0 + 0.5 * ripple }
        }
        Surface::Rust => {
            let blotch = fbm(u, v, 3, 5, 71);
            let fine = fbm(u, v, 24, 2, 73);
            let pit = grain(px, py, 79);
            let t = ((blotch - 0.35) * 2.2).clamp(0.0, 1.0);
            let value = (0.45 + 0.5 * t) * (0.85 + 0.15 * fine) * if pit > 0.97 { 0.6 } else { 1.0 };
            Sample { value, tint: 0.12 * (1.0 - t), height: 0.6 * fine + 0.4 * blotch + if pit > 0.97 { -0.6 } else { 0.0 } }
        }
        Surface::Glass => {
            // 1.5 m tile: mullion at the tile edge, slight reflective gradient.
            let m = 0.025;
            let mullion = u < m || v < m || u > 1.0 - m || v > 1.0 - m;
            let gradient = 0.85 + 0.15 * ((u + v) * 0.5);
            let smear = fbm(u, v, 2, 2, 83);
            if mullion {
                Sample { value: 0.25, tint: 0.0, height: 0.4 }
            } else {
                Sample { value: gradient * (0.96 + 0.06 * smear), tint: 0.0, height: 0.0 }
            }
        }
        Surface::Wood => {
            // 1 m tile: 6 planks across v, grain along u.
            let planks = 6.0;
            let row = (v * planks).floor();
            let fv = v * planks - row;
            let id = hash(row as i32, 0, 89);
            let grain_v = vnoise(u * 40.0, (v * planks) * 3.0, 40, 97);
            let seam = fv < 0.05;
            let knots = fbm(u, v, 10, 2, 101);
            if seam {
                Sample { value: 0.5, tint: 0.0, height: -0.8 }
            } else {
                Sample { value: (0.78 + 0.2 * id) * (0.88 + 0.14 * grain_v) * (0.95 + 0.08 * knots), tint: 0.05, height: 0.3 * grain_v }
            }
        }
        Surface::Tarp => {
            let weave = ((px % 3 == 0) as i32 + (py % 3 == 0) as i32) as f32 * 0.03;
            let wrinkle = fbm(u, v, 3, 3, 103);
            let value = (0.92 - weave) * (0.88 + 0.16 * wrinkle);
            Sample { value, tint: 0.0, height: wrinkle * 0.8 }
        }
        Surface::Asphalt => {
            let g = grain(px, py, 107);
            let patch = fbm(u, v, 3, 3, 109);
            let crack_field = fbm(u, v, 2, 3, 113);
            let crack = if (crack_field - 0.5).abs() < 0.004 { 0.6 } else { 1.0 };
            let value = (0.72 + 0.25 * g) * (0.9 + 0.14 * patch) * crack;
            Sample { value, tint: 0.0, height: 0.5 * g + if crack < 1.0 { -0.7 } else { 0.0 } }
        }
        Surface::Paving => {
            // 2 m tile: 4 x 4 slabs.
            let su = u * 4.0;
            let sv = v * 4.0;
            let fu = su.fract();
            let fv = sv.fract();
            let seam = fu < 0.03 || fv < 0.03;
            let id = hash(su.floor() as i32, sv.floor() as i32, 127);
            let g = fbm(u, v, 24, 2, 131);
            let chip = grain(px, py, 137) > 0.985;
            if seam {
                Sample { value: 0.55, tint: 0.0, height: -0.9 }
            } else {
                Sample { value: (0.82 + 0.14 * id) * (0.92 + 0.1 * g) * if chip { 0.75 } else { 1.0 }, tint: 0.0, height: 0.2 * g + if chip { -0.5 } else { 0.0 } }
            }
        }
        Surface::Dirt => {
            let g = fbm(u, v, 4, 5, 139);
            let pebble = grain(px, py, 149) > 0.96;
            let value = (0.7 + 0.3 * g) * if pebble { 1.15 } else { 1.0 };
            Sample { value: value.min(1.0), tint: 0.04, height: g * 0.8 + if pebble { 0.8 } else { 0.0 } }
        }
        Surface::Foliage => {
            let leaf = fbm(u, v, 12, 3, 151);
            let g = grain(px, py, 157);
            let value = 0.55 + 0.4 * leaf + 0.1 * g;
            Sample { value: value.min(1.0), tint: 0.03 * (leaf - 0.5), height: leaf }
        }
        Surface::Matte => {
            let g = grain(px, py, 163);
            Sample { value: 0.9 + 0.08 * g, tint: 0.0, height: 0.1 * g }
        }
        Surface::Glow | Surface::Paint => Sample { value: 1.0, tint: 0.0, height: 0.0 },
        // Weathered surfaces are generated through `generate_wear`.
        other => sample(other.base(), u, v, px, py),
    }
}

/// How weathered the `Worn*` surfaces are drawn.
pub const WORN: f32 = 0.85;

/// A thin dark line where `field` crosses one half: the hairline crack
/// pattern, widening with `wear`.
fn crack_line(field: f32, width: f32) -> f32 {
    let d = (field - 0.5).abs();
    if d < width { 1.0 - d / width } else { 0.0 }
}

/// Grit laid over a clean sample: water stains and soot blotches, drip
/// streaks running down (v is up a wall), cracks, worn-through paint and,
/// on steel, rust bleeding from the fixings. Ground surfaces get crazing,
/// chipped slabs and patched asphalt instead of drips. Strength `wear`,
/// 0 leaves the sample untouched.
fn grit(surface: Surface, s: Sample, u: f32, v: f32, px: i32, py: i32, wear: f32) -> Sample {
    if wear <= 0.0 {
        return s;
    }
    let Sample { mut value, mut tint, mut height } = s;
    let ground = matches!(surface, Surface::Paving | Surface::Asphalt);
    // Broad stains: damp patches and soot, a little warm.
    let stain = fbm(u, v, 3, 4, 401);
    let st = ((stain - 0.45) * 2.5).clamp(0.0, 1.0) * wear;
    value *= 1.0 - 0.32 * st;
    tint += 0.05 * st;
    // Fine dirt in the grain everywhere.
    let dirt = grain(px, py, 409);
    value *= 1.0 - 0.10 * wear * dirt;
    if !ground {
        // Drip streaks: stretched noise along v, darkest under the
        // stain bands, as rain carries dirt down a facade.
        let drip = fbm(u * 6.0, v * 0.5, 6, 3, 419);
        let dr = ((drip - 0.55) * 4.0).clamp(0.0, 1.0) * wear;
        value *= 1.0 - 0.28 * dr;
        height -= 0.15 * dr;
    }
    // Cracks: hairlines along a noise contour, broken into short runs by a
    // second field so they read as cracks, not a marbled map.
    let w = 0.002 + 0.004 * wear;
    let mask = fbm(u, v, 4, 2, 437) > 0.56;
    let c1 = if mask { crack_line(fbm(u, v, 6, 3, 431), w) } else { 0.0 };
    let c2 = if !mask { crack_line(fbm(u, v, 8, 2, 433), w * 0.7) * 0.6 } else { 0.0 };
    // Steel sheet does not crack like render: no hairlines on containers.
    let crack = if surface == Surface::Container { 0.0 } else { c1.max(c2) * wear };
    value *= 1.0 - 0.35 * crack;
    height -= 1.0 * crack;
    match surface {
        Surface::Container => {
            // Paint chipped down to rust in small, scattered spots, clustered
            // where a broad mask allows (not primer blotches all over).
            let chip = fbm(u, v, 48, 2, 443);
            let cluster = fbm(u, v, 3, 2, 449);
            if cluster > 0.52 && chip > 0.8 - 0.04 * wear {
                value *= 0.8;
                tint += 0.1;
                height -= 0.2;
            }
        }
        Surface::Panel | Surface::Metal | Surface::Plaster => {
            // Worn paint: blotches where the topcoat has gone, a lighter,
            // cooler primer showing through.
            let chip = fbm(u, v, 8, 3, 443);
            if chip > 0.68 - 0.12 * wear {
                value = value * 0.8 + 0.12;
                tint -= 0.03;
                height -= 0.3;
            }
        }
        _ => {}
    }
    if matches!(surface, Surface::Metal | Surface::Panel | Surface::Container) {
        // Rust bleeding down from fixings in vertical runs.
        let run = fbm(u * 10.0, v * 0.6, 10, 2, 457);
        let r = ((run - 0.62) * 5.0).clamp(0.0, 1.0) * wear;
        value *= 1.0 - 0.2 * r;
        tint += 0.18 * r;
    }
    if matches!(surface, Surface::Brick | Surface::Concrete) {
        // Efflorescence: pale salt blooms.
        let salt = fbm(u, v, 4, 3, 461);
        if salt > 0.74 {
            value = value * 0.85 + 0.12 * wear;
        }
    }
    if ground {
        // Crazing in a coarse cell pattern, chipped edges and dark spots
        // (gum, oil) on paving; patched repairs on asphalt.
        let cu = u * 6.0;
        let cv = v * 6.0;
        let (iu, iv) = (cu.floor() as i32, cv.floor() as i32);
        let jitter = hash(iu.rem_euclid(6), iv.rem_euclid(6), 467);
        let edge = (cu.fract() - 0.5).abs().max((cv.fract() - 0.5).abs());
        if jitter < 0.5 * wear && edge > 0.47 {
            value *= 0.6;
            height -= 1.0;
        }
        if grain(px, py, 479) > 1.0 - 0.012 * wear {
            value *= 0.45;
        }
        if surface == Surface::Asphalt {
            let patch = fbm(u, v, 2, 2, 487);
            if patch > 0.66 {
                value *= 0.82;
                height += 0.2;
            }
        }
    }
    Sample { value: value.clamp(0.0, 1.0), tint, height }
}

/// Texture size the bump strengths in `Surface::bump` were tuned at.
const REFERENCE_SIZE: f32 = 256.0;

/// Generate the colour and normal textures for a surface. `None` for glow,
/// which stays a flat emissive colour.
pub fn generate(surface: Surface, size: usize) -> Option<Texture> {
    if surface.is_worn() {
        return generate_wear(surface.base(), size, WORN);
    }
    generate_wear(surface, size, 0.0)
}

/// The texture of a surface weathered by `wear` in `0..1`: 0 is the clean
/// texture, byte for byte; more adds stains, cracks, streaks and worn paint.
pub fn generate_wear(surface: Surface, size: usize, wear: f32) -> Option<Texture> {
    if surface.is_glow() {
        return None;
    }
    let surface = surface.base();
    let wear = if wear.is_finite() { wear.clamp(0.0, 1.0) } else { 0.0 };
    let mut heights = vec![0.0f32; size * size];
    let mut color = vec![0u8; size * size * 4];
    for py in 0..size {
        for px in 0..size {
            let u = (px as f32 + 0.5) / size as f32;
            let v = (py as f32 + 0.5) / size as f32;
            let s = grit(surface, sample(surface, u, v, px as i32, py as i32), u, v, px as i32, py as i32, wear);
            heights[py * size + px] = s.height;
            let r = (s.value * (1.0 + s.tint)).clamp(0.0, 1.0);
            let g = s.value.clamp(0.0, 1.0);
            let b = (s.value * (1.0 - s.tint)).clamp(0.0, 1.0);
            let i = (py * size + px) * 4;
            color[i] = (r * 255.0) as u8;
            color[i + 1] = (g * 255.0) as u8;
            color[i + 2] = (b * 255.0) as u8;
            color[i + 3] = 255;
        }
    }
    // The height field is a function of (u, v), so the finite difference
    // between neighbouring pixels shrinks as the texture grows; scaling by
    // the size keeps the normal map's slope the same at any resolution.
    let strength = surface.bump() * (1.0 + 0.25 * wear) * 0.5 * (size as f32 / REFERENCE_SIZE);
    let mut normal = vec![0u8; size * size * 4];
    let at = |x: i32, y: i32| heights[(y.rem_euclid(size as i32) as usize) * size + x.rem_euclid(size as i32) as usize];
    for py in 0..size as i32 {
        for px in 0..size as i32 {
            let dx = (at(px + 1, py) - at(px - 1, py)) * strength;
            let dy = (at(px, py + 1) - at(px, py - 1)) * strength;
            let n = glam::Vec3::new(-dx, -dy, 1.0).normalize();
            let i = ((py as usize) * size + px as usize) * 4;
            normal[i] = ((n.x * 0.5 + 0.5) * 255.0) as u8;
            normal[i + 1] = ((n.y * 0.5 + 0.5) * 255.0) as u8;
            normal[i + 2] = ((n.z * 0.5 + 0.5) * 255.0) as u8;
            normal[i + 3] = 255;
        }
    }
    Some(Texture { size, color, normal })
}

/// [look] A metallic-roughness map for a surface's texture (RGBA8, linear;
/// green is the perceptual roughness, blue scales the metalness, as Bevy's
/// `metallic_roughness_texture` reads them), so a material's sheen varies
/// over it instead of being one number: grime, stains and cracks (the
/// darker texels) are rougher, clean and worn-smooth spots glossier;
/// on metal the darkest marks (rust, scorching) also lose their metalness.
/// The material's own roughness is the map's average, so the look from a
/// distance is unchanged.
pub fn roughness_map(surface: Surface, tex: &Texture) -> Vec<u8> {
    let n = tex.size * tex.size;
    let lum = |i: usize| {
        let p = &tex.color[i * 4..i * 4 + 3];
        (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0
    };
    let mean = (0..n).map(lum).sum::<f32>() / n.max(1) as f32;
    let base = surface.roughness();
    // How strongly the texture's light and dark modulate the sheen.
    let k = match surface.base() {
        Surface::Glass => 0.15,
        Surface::Metal | Surface::Container | Surface::Panel => 0.9,
        Surface::Asphalt | Surface::Paving | Surface::Concrete => 0.7,
        _ => 0.5,
    };
    let metal = surface.base() == Surface::Metal || surface.base() == Surface::Container || surface.base() == Surface::Rust;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let d = mean - lum(i);
        let r = (base + d * k * 1.5).clamp(0.04, 1.0);
        let m = if metal { (1.0 - (d * 2.5).max(0.0)).clamp(0.3, 1.0) } else { 1.0 };
        out[i * 4] = 255;
        out[i * 4 + 1] = (r * 255.0).round() as u8;
        out[i * 4 + 2] = (m * 255.0).round() as u8;
        out[i * 4 + 3] = 255;
    }
    out
}

/// Box-filtered mip chain for a square RGBA8 image, concatenated from the
/// full-size level down to 1x1. Returns the data and the level count.
pub fn mip_chain(size: usize, rgba: &[u8]) -> (Vec<u8>, u32) {
    let mut out = rgba.to_vec();
    let mut prev = rgba.to_vec();
    let mut s = size;
    let mut levels = 1;
    while s > 1 {
        let ns = s / 2;
        let mut next = vec![0u8; ns * ns * 4];
        for y in 0..ns {
            for x in 0..ns {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        sum += prev[((2 * y + dy) * s + 2 * x + dx) * 4 + c] as u32;
                    }
                    next[(y * ns + x) * 4 + c] = (sum / 4) as u8;
                }
            }
        }
        out.extend_from_slice(&next);
        prev = next;
        s = ns;
        levels += 1;
    }
    (out, levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The roughness map varies over the texture but averages out near the
    /// surface's own roughness, so the material reads the same from afar.
    #[test]
    fn roughness_map_varies_round_the_surface_roughness() {
        for s in [Surface::Asphalt, Surface::Brick, Surface::Metal, Surface::Glass] {
            let t = generate(s, 64).unwrap();
            let m = roughness_map(s, &t);
            assert_eq!(m.len(), t.color.len());
            let g: Vec<f32> = m.chunks(4).map(|p| p[1] as f32 / 255.0).collect();
            let avg = g.iter().sum::<f32>() / g.len() as f32;
            let (lo, hi) = g.iter().fold((1.0f32, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)));
            assert!((avg - s.roughness()).abs() < 0.08, "{s:?}: average {avg} vs {}", s.roughness());
            assert!(hi > lo, "{s:?}: no variation");
        }
    }

    /// The slope the normal map encodes should not depend on how many
    /// pixels the texture has.
    #[test]
    fn normal_strength_is_independent_of_size() {
        let small = generate(Surface::Metal, 64).unwrap();
        let large = generate(Surface::Metal, 256).unwrap();
        let spread = |t: &Texture| {
            let (mut lo, mut hi) = (255u8, 0u8);
            for px in t.normal.chunks(4) {
                lo = lo.min(px[0]);
                hi = hi.max(px[0]);
            }
            hi as f32 - lo as f32
        };
        let (a, b) = (spread(&small), spread(&large));
        assert!((a - b).abs() < a.max(b) * 0.25, "normal x range differs: 64px {a}, 256px {b}");
    }

    fn mean(t: &Texture) -> f32 {
        t.color.chunks(4).map(|p| p[1] as f32).sum::<f32>() / (t.size * t.size) as f32
    }

    fn dark(t: &Texture, below: f32) -> usize {
        t.color.chunks(4).filter(|p| (p[1] as f32) < below).count()
    }

    /// Wear 0 is the clean texture; weathering darkens it and adds dark
    /// marks (stains, cracks, streaks), deterministically.
    #[test]
    fn wear_adds_grit() {
        for (base, worn) in Surface::WEATHERED {
            let clean = generate(base, 64).unwrap();
            let zero = generate_wear(base, 64, 0.0).unwrap();
            assert!(clean.color == zero.color && clean.normal == zero.normal, "{base:?}: wear 0 must be the clean texture");
            let a = generate(worn, 64).unwrap();
            let b = generate(worn, 64).unwrap();
            assert!(a.color == b.color, "{worn:?} is not deterministic");
            let (m0, m1) = (mean(&clean), mean(&a));
            let (d0, d1) = (dark(&clean, m0 * 0.7), dark(&a, m0 * 0.7));
            assert!(m1 < m0 * 0.97, "{worn:?} should be darker: {m1} vs {m0}");
            assert!(d1 > d0, "{worn:?} should have more dark marks: {d1} vs {d0}");
            let half = generate_wear(base, 64, 0.4).unwrap();
            let mh = mean(&half);
            assert!(mh <= m0 && mh >= m1, "{base:?}: wear should be monotone ({m0} {mh} {m1})");
        }
    }

    #[test]
    fn textures_tile_and_have_mips() {
        for s in Surface::ALL {
            let Some(t) = generate(s, 32) else { continue };
            assert_eq!(t.color.len(), 32 * 32 * 4);
            let (data, levels) = mip_chain(32, &t.color);
            assert_eq!(levels, 6);
            assert_eq!(data.len(), (32 * 32 + 16 * 16 + 8 * 8 + 4 * 4 + 2 * 2 + 1) * 4);
        }
    }
}
