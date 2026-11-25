//! [r17-logos] Company logos: a vector mark per company, generated from
//! its brand seed and drawn as smooth flat meshes of light.
//!
//! A [`Mark`] is what identifies a company's look: a seed, its colour and
//! an accent, its initial and a shape hint. Mega-corps take theirs from
//! their brand's colour, shape and monogram ([`Mark::corp`]), so a lorry's
//! livery (which carries only those) wears the very mark the HQ crown
//! does; chains and local businesses from their trading name
//! ([`Mark::local`], branch numbers dropped, so every branch matches).
//!
//! [`design`] turns a mark into a [`Logo`]: a few primitives (filled
//! polygons and stroked polylines) in the unit square, one of fourteen
//! families (a neon outline round the initial, a solid badge with the
//! initial cut out, an orbit, a starburst, a striped sun, chevrons, linked
//! rings, a pinwheel, a big letterform, a dot grid, waves, an isometric
//! cube, a split disc, broadcast arcs) with its proportions, angles and
//! counts from the seed. [`tessellate`] makes indexed 2D triangles, shared
//! by the 3D signs ([`emit`]), the UI and the contact sheet
//! ([`rasterize`]); wordmarks use the stroke font in [`crate::logo_font`].

use crate::logo_font;
use crate::mesh::{triangulate, MeshData};
use glam::{Vec2, Vec3};
use nd_sim::corp::{Brand, LogoShape, BRAND_COLOURS};
use nd_sim::hash::{mix, unit};
use std::f32::consts::{PI, TAU};

/// What a part of a logo is painted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ink {
    /// The brand colour, lit.
    Brand,
    /// The brand colour, deep (a shaded face).
    Deep,
    /// The second colour.
    Accent,
    /// Warm white light.
    Light,
    /// The dark of the board (a cut-out).
    Dark,
}

/// One primitive in the unit square (x right, y up, about -1..1).
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    /// A filled simple polygon (either winding).
    Fill { pts: Vec<Vec2>, ink: Ink },
    /// A polyline drawn as a band `w` wide.
    Stroke { pts: Vec<Vec2>, closed: bool, w: f32, ink: Ink },
}

impl Prim {
    pub fn ink(&self) -> Ink {
        match self {
            Prim::Fill { ink, .. } | Prim::Stroke { ink, .. } => *ink,
        }
    }
}

/// The look families a mark can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Outline,
    Badge,
    Orbit,
    Burst,
    Stripes,
    Chevrons,
    Rings,
    Blades,
    Letter,
    Dots,
    Waves,
    Cube,
    Split,
    Arcs,
}

impl Family {
    pub const ALL: [Family; 14] = [
        Family::Outline,
        Family::Badge,
        Family::Orbit,
        Family::Burst,
        Family::Stripes,
        Family::Chevrons,
        Family::Rings,
        Family::Blades,
        Family::Letter,
        Family::Dots,
        Family::Waves,
        Family::Cube,
        Family::Split,
        Family::Arcs,
    ];
}

/// A designed logo: its primitives, back to front.
#[derive(Clone, Debug, PartialEq)]
pub struct Logo {
    pub family: Family,
    pub prims: Vec<Prim>,
}

/// A company's visual identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub seed: u64,
    /// Linear RGB, 0..1.
    pub colour: [f32; 3],
    pub accent: [f32; 3],
    pub monogram: char,
    pub shape: LogoShape,
}

fn brand_rgb(i: u8) -> [f32; 3] {
    Brand { colour: i, ..Default::default() }.rgb_f32()
}

/// The accent for a brand colour: another of the brand colours, far
/// enough round the wheel, or white.
fn accent_for(colour: u8, h: u64) -> [f32; 3] {
    let n = BRAND_COLOURS.len() as u64;
    match h % 5 {
        0 => [0.86, 0.86, 0.82],
        k => brand_rgb(((colour as u64 + 2 + k * 2) % (n - 1)) as u8),
    }
}

impl Mark {
    /// A mega-corp's (or a university's) mark from its brand: the same
    /// colour, shape and initial give the same mark wherever it is drawn.
    pub fn corp(colour: u8, shape: LogoShape, monogram: char) -> Mark {
        let seed = mix(0xC0_4B_7A_11, (colour as u64) << 16 | (shape as u64) << 8 | monogram as u64);
        Mark { seed, colour: brand_rgb(colour), accent: accent_for(colour, mix(seed, 7)), monogram: monogram.to_ascii_uppercase(), shape }
    }

    /// A mark from a brand record.
    pub fn of_brand(b: &Brand) -> Mark {
        Mark::corp(b.colour, b.shape, b.monogram)
    }

    /// A chain's or a local business's mark from its trading name: the
    /// branch number ("Rocket Burger #3") and a leading "The" do not count.
    pub fn local(name: &str) -> Mark {
        let base = trading_name(name);
        let seed = nd_sim::hash::fnv_bytes(base.bytes().map(|b| b.to_ascii_lowercase()));
        let n = BRAND_COLOURS.len() as u64;
        let colour = (mix(seed, 1) % n) as u8;
        let shape = LogoShape::CORP[(mix(seed, 2) % LogoShape::CORP.len() as u64) as usize];
        let monogram = base.chars().find(|c| c.is_ascii_alphanumeric()).map_or('X', |c| c.to_ascii_uppercase());
        Mark { seed: mix(seed, 0x10CA1), colour: brand_rgb(colour), accent: accent_for(colour, mix(seed, 3)), monogram, shape }
    }
}

/// A name without its branch number or a leading "The ".
pub fn trading_name(name: &str) -> &str {
    let n = name.trim();
    let n = match n.rfind(" #") {
        Some(i) if n[i + 2..].chars().all(|c| c.is_ascii_digit()) => &n[..i],
        _ => n,
    };
    n.strip_prefix("The ").unwrap_or(n)
}

// ── shapes ────────────────────────────────────────────────────────────────────

fn ngon(n: usize, r: f32, rot: f32) -> Vec<Vec2> {
    (0..n).map(|k| Vec2::from_angle(rot + TAU * k as f32 / n as f32) * r).collect()
}

fn arc(c: Vec2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Vec2> {
    (0..=n).map(|k| c + Vec2::from_angle(a0 + (a1 - a0) * k as f32 / n as f32) * r).collect()
}

fn rounded_square(r: f32, round: f32) -> Vec<Vec2> {
    let mut out = Vec::new();
    let c = r - round;
    for (k, q) in [Vec2::new(c, -c), Vec2::new(c, c), Vec2::new(-c, c), Vec2::new(-c, -c)].into_iter().enumerate() {
        let a0 = -PI * 0.5 + k as f32 * PI * 0.5;
        out.extend(arc(q, round, a0, a0 + PI * 0.5, 3));
    }
    out
}

/// The outline of a shape hint, radius about `r`, counter-clockwise.
pub fn shape_outline(shape: LogoShape, r: f32) -> Vec<Vec2> {
    match shape {
        LogoShape::Circle | LogoShape::Ring => ngon(26, r, 0.0),
        LogoShape::Hexagon => ngon(6, r, PI / 6.0),
        LogoShape::Diamond => ngon(4, r, PI / 2.0),
        LogoShape::Triangle => ngon(3, r * 1.08, PI / 2.0).into_iter().map(|p| p - Vec2::new(0.0, r * 0.18)).collect(),
        LogoShape::Square => rounded_square(r * 0.86, r * 0.2),
        LogoShape::Chevron => vec![Vec2::new(-r, r * 0.2), Vec2::new(0.0, -r * 0.8), Vec2::new(r, r * 0.2), Vec2::new(r, r * 0.8), Vec2::new(0.0, -r * 0.2), Vec2::new(-r, r * 0.8)],
        LogoShape::Shield => {
            let mut v = vec![Vec2::new(-r * 0.85, r * 0.9), Vec2::new(-r * 0.85, -r * 0.1)];
            v.extend(arc(Vec2::new(0.0, -r * 0.1), r * 0.85, PI, PI * 1.5, 5).into_iter().skip(1).map(|p| Vec2::new(p.x, -r * 0.1 + (p.y + r * 0.1) * 1.05)));
            v.extend(arc(Vec2::new(0.0, -r * 0.1), r * 0.85, PI * 1.5, TAU, 5).into_iter().skip(1).map(|p| Vec2::new(p.x, -r * 0.1 + (p.y + r * 0.1) * 1.05)));
            v.push(Vec2::new(r * 0.85, r * 0.9));
            v
        }
    }
}

/// The initial as strokes, centred on `at`, `h` tall.
fn letter(c: char, at: Vec2, h: f32, slant: f32, w: f32, ink: Ink) -> Vec<Prim> {
    let k = h / logo_font::GLYPH_H;
    logo_font::text_lines(&c.to_string(), slant)
        .into_iter()
        .map(|l| Prim::Stroke { pts: l.into_iter().map(|p| at + Vec2::new((p.x - logo_font::GLYPH_W * 0.5 - logo_font::GLYPH_H * 0.5 * slant) * k, (p.y - logo_font::GLYPH_H * 0.5) * k)).collect(), closed: false, w, ink })
        .collect()
}

fn rot(p: Vec2, a: f32) -> Vec2 {
    Vec2::from_angle(a).rotate(p)
}

/// A seeded number in `lo..hi`.
fn r(seed: u64, salt: u64, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * unit(mix(seed, salt))
}

fn pick<T: Copy>(seed: u64, salt: u64, of: &[T]) -> T {
    of[(mix(seed, salt) % of.len() as u64) as usize]
}

/// The family a mark is drawn in.
pub fn family(m: &Mark) -> Family {
    // A crest stays a crest.
    if m.shape == LogoShape::Shield {
        return if mix(m.seed, 40) % 2 == 0 { Family::Badge } else { Family::Outline };
    }
    Family::ALL[(mix(m.seed, 41) % Family::ALL.len() as u64) as usize]
}

/// Design the logo for a mark.
pub fn design(m: &Mark) -> Logo {
    let s = m.seed;
    let fam = family(m);
    let mut p: Vec<Prim> = Vec::new();
    let initial = |at: Vec2, h: f32, w: f32, ink: Ink| letter(m.monogram, at, h, if mix(s, 50) % 3 == 0 { 0.18 } else { 0.0 }, w, ink);
    match fam {
        Family::Outline => {
            let w = r(s, 1, 0.08, 0.13);
            p.push(Prim::Stroke { pts: shape_outline(m.shape, 0.86), closed: true, w, ink: Ink::Brand });
            if mix(s, 2) % 2 == 0 {
                p.push(Prim::Stroke { pts: shape_outline(m.shape, 0.66), closed: true, w: w * 0.45, ink: Ink::Accent });
                p.extend(initial(Vec2::ZERO, 0.62, w * 0.9, Ink::Light));
            } else {
                p.extend(initial(Vec2::ZERO, 0.86, w, Ink::Light));
            }
        }
        Family::Badge => {
            p.push(Prim::Fill { pts: shape_outline(m.shape, 0.9), ink: Ink::Brand });
            if mix(s, 3) % 2 == 0 {
                p.push(Prim::Stroke { pts: shape_outline(m.shape, 0.98), closed: true, w: 0.04, ink: Ink::Light });
            }
            p.extend(initial(Vec2::new(0.0, if m.shape == LogoShape::Triangle { -0.12 } else { 0.0 }), if m.shape == LogoShape::Triangle { 0.6 } else { 0.92 }, r(s, 4, 0.14, 0.2), Ink::Dark));
        }
        Family::Orbit => {
            let core = r(s, 5, 0.26, 0.42);
            p.push(Prim::Fill { pts: ngon(20, core, 0.0), ink: Ink::Brand });
            let tilt = r(s, 6, -0.9, 0.9);
            let ell = |a: f32, b: f32, t: f32| -> Vec<Vec2> { (0..28).map(|k| rot(Vec2::new((TAU * k as f32 / 28.0).cos() * a, (TAU * k as f32 / 28.0).sin() * b), t)).collect() };
            let b = r(s, 7, 0.22, 0.42);
            p.push(Prim::Stroke { pts: ell(0.92, b, tilt), closed: true, w: 0.07, ink: Ink::Accent });
            if mix(s, 8) % 2 == 0 {
                p.push(Prim::Stroke { pts: ell(0.92, b, tilt + PI * 0.5), closed: true, w: 0.05, ink: Ink::Brand });
            } else {
                let at = rot(Vec2::new(0.92, 0.0), tilt);
                p.push(Prim::Fill { pts: ngon(12, 0.1, 0.0).into_iter().map(|q| q + at).collect(), ink: Ink::Light });
            }
        }
        Family::Burst => {
            let n = 5 + (mix(s, 9) % 8) as usize;
            let inner = r(s, 10, 0.32, 0.62);
            let a0 = r(s, 11, 0.0, PI);
            let pts: Vec<Vec2> = (0..n * 2).map(|k| Vec2::from_angle(a0 + PI * k as f32 / n as f32) * if k % 2 == 0 { 0.95 } else { 0.95 * inner }).collect();
            p.push(Prim::Fill { pts, ink: Ink::Brand });
            p.push(Prim::Fill { pts: ngon(14, 0.95 * inner * 0.55, 0.0), ink: if mix(s, 12) % 2 == 0 { Ink::Light } else { Ink::Dark } });
        }
        Family::Stripes => {
            let n = 4 + (mix(s, 13) % 4) as usize;
            let rr = 0.92;
            let gap0 = r(s, 14, 0.03, 0.06);
            // Bands from the top down; the gaps widen towards the bottom.
            let mut y = rr;
            let unit_h = (2.0 * rr - gap0 * (n as f32 * (n as f32 - 1.0) * 0.5) * 0.6) / n as f32;
            for k in 0..n {
                let y1 = y;
                let y0 = (y - unit_h).max(-rr);
                let band = chord_band(rr, y0, y1);
                let ink = if k == n - 1 && mix(s, 15) % 2 == 0 { Ink::Accent } else { Ink::Brand };
                if band.len() >= 3 {
                    p.push(Prim::Fill { pts: band, ink });
                }
                y = y0 - gap0 * (k as f32 + 1.0) * 0.6;
                if y <= -rr {
                    break;
                }
            }
        }
        Family::Chevrons => {
            let n = 2 + (mix(s, 16) % 2) as usize;
            let dir = pick(s, 17, &[0.0f32, PI * 0.5, PI, -PI * 0.5]);
            let w = r(s, 18, 0.16, 0.24);
            let gap = if n == 2 { 0.5 } else { 0.42 };
            let total = (n as f32 - 1.0) * gap + 0.6;
            for k in 0..n {
                let y = -total * 0.5 + k as f32 * gap;
                let pts = vec![Vec2::new(-0.8, y), Vec2::new(0.0, y + 0.6), Vec2::new(0.8, y)].into_iter().map(|q| rot(q, dir)).collect();
                p.push(Prim::Stroke { pts, closed: false, w, ink: if k % 2 == 1 { Ink::Accent } else { Ink::Brand } });
            }
        }
        Family::Rings => {
            let n = 2 + (mix(s, 19) % 2) as usize;
            let rr = if n == 2 { 0.5 } else { 0.42 };
            let a0 = r(s, 20, 0.0, TAU);
            let inks = [Ink::Brand, Ink::Accent, Ink::Light];
            for k in 0..n {
                let c = if n == 2 { Vec2::new(if k == 0 { -0.36 } else { 0.36 }, 0.0) } else { Vec2::from_angle(a0 + TAU * k as f32 / 3.0) * 0.42 };
                p.push(Prim::Stroke { pts: ngon(24, rr, 0.0).into_iter().map(|q| q + c).collect(), closed: true, w: r(s, 21, 0.08, 0.12), ink: inks[k] });
            }
        }
        Family::Blades => {
            let n = 3 + (mix(s, 22) % 4) as usize;
            let a0 = r(s, 23, 0.0, TAU);
            let sweep = r(s, 24, 0.25, 0.6);
            for k in 0..n {
                let a = a0 + TAU * k as f32 / n as f32;
                let blade = vec![Vec2::new(0.12, 0.02), Vec2::new(0.95, 0.0), Vec2::new(0.95 * (sweep * 1.4).cos(), 0.95 * (sweep * 1.4).sin())].into_iter().map(|q| rot(q, a)).collect();
                p.push(Prim::Fill { pts: blade, ink: if k % 2 == 1 && mix(s, 25) % 2 == 0 { Ink::Accent } else { Ink::Brand } });
            }
        }
        Family::Letter => {
            let w = r(s, 26, 0.2, 0.28);
            p.extend(letter(m.monogram, Vec2::new(0.0, 0.08), 1.5, r(s, 27, 0.0, 0.25), w, Ink::Brand));
            match mix(s, 28) % 3 {
                0 => p.push(Prim::Stroke { pts: vec![Vec2::new(-0.85, -0.86), Vec2::new(0.85, -0.86)], closed: false, w: 0.1, ink: Ink::Accent }),
                1 => p.push(Prim::Fill { pts: ngon(14, 0.13, 0.0).into_iter().map(|q| q + Vec2::new(0.78, -0.72)).collect(), ink: Ink::Accent }),
                _ => p.push(Prim::Stroke { pts: vec![Vec2::new(-0.9, -0.5), Vec2::new(0.9, 0.5)], closed: false, w: 0.08, ink: Ink::Accent }),
            }
        }
        Family::Dots => {
            // A halftone: a grid of round dots inside the shape hint, growing
            // across it along a seeded direction (a designed mark, not
            // pixels), the biggest in the accent.
            let n = 5 + (mix(s, 29) % 2) as usize;
            let step = 1.8 / (n as f32 - 1.0);
            let dir = Vec2::from_angle(r(s, 30, 0.0, TAU));
            let shape = if m.shape == LogoShape::Square || m.shape == LogoShape::Chevron { LogoShape::Circle } else { m.shape };
            let mut dots: Vec<(Vec2, f32)> = Vec::new();
            for i in 0..n {
                for j in 0..n {
                    let c = Vec2::new(-0.9 + step * j as f32, 0.9 - step * i as f32);
                    if !shape.covers(c.x / 0.95, c.y / 0.95) {
                        continue;
                    }
                    let t = (c.dot(dir) / 1.3 * 0.5 + 0.5).clamp(0.0, 1.0);
                    dots.push((c, step * 0.5 * (0.3 + 0.62 * t)));
                }
            }
            let biggest = dots.iter().enumerate().max_by(|a, b| a.1 .1.total_cmp(&b.1 .1)).map_or(0, |(k, _)| k);
            for (k, (c, rad)) in dots.into_iter().enumerate() {
                p.push(Prim::Fill { pts: ngon(8, rad, 0.0).into_iter().map(|q| q + c).collect(), ink: if k == biggest { Ink::Accent } else { Ink::Brand } });
            }
        }
        Family::Waves => {
            let n = 2 + (mix(s, 34) % 2) as usize;
            let amp = r(s, 35, 0.1, 0.2);
            let cycles = r(s, 36, 1.0, 2.0);
            for k in 0..n {
                let y = 0.45 - k as f32 * 0.9 / (n as f32 - 1.0).max(1.0);
                let pts = (0..=20).map(|i| {
                    let x = -0.85 + 1.7 * i as f32 / 20.0;
                    Vec2::new(x, y + (x * PI * cycles + k as f32 * 0.6).sin() * amp)
                });
                p.push(Prim::Stroke { pts: pts.collect(), closed: false, w: r(s, 37, 0.1, 0.16), ink: if k == 1 { Ink::Accent } else { Ink::Brand } });
            }
            if mix(s, 38) % 2 == 0 {
                p.push(Prim::Stroke { pts: ngon(26, 0.98, 0.0), closed: true, w: 0.05, ink: Ink::Light });
            }
        }
        Family::Cube => {
            let a = 0.9;
            let c = Vec2::ZERO;
            let up = Vec2::new(0.0, a);
            let r30 = Vec2::new(a * 0.866, a * 0.5);
            let l30 = Vec2::new(-a * 0.866, a * 0.5);
            let top = vec![c, c + r30, c + up, c + l30];
            let left = vec![c, c + l30, c + l30 - up, c - up];
            let right = vec![c, c - up, c + r30 - up, c + r30];
            p.push(Prim::Fill { pts: top, ink: Ink::Accent });
            p.push(Prim::Fill { pts: left, ink: Ink::Brand });
            p.push(Prim::Fill { pts: right, ink: Ink::Deep });
            if mix(s, 39) % 2 == 0 {
                p.extend(letter(m.monogram, Vec2::new(-a * 0.42, -a * 0.28), 0.5, 0.0, 0.09, Ink::Light));
            }
        }
        Family::Split => {
            let a = r(s, 42, -0.8, 0.8);
            let gap = r(s, 43, 0.06, 0.12);
            let n = Vec2::from_angle(a + PI * 0.5);
            let half = |side: f32| -> Vec<Vec2> { arc(Vec2::ZERO, 0.92, a + if side > 0.0 { 0.0 } else { PI }, a + if side > 0.0 { PI } else { TAU }, 18).into_iter().map(|q| q + n * gap * side).collect() };
            p.push(Prim::Fill { pts: half(1.0), ink: Ink::Brand });
            p.push(Prim::Fill { pts: half(-1.0), ink: Ink::Accent });
        }
        Family::Arcs => {
            let a = r(s, 44, 0.0, TAU);
            let spread = r(s, 45, 0.6, 1.1);
            let base = Vec2::from_angle(a + PI) * 0.55;
            p.push(Prim::Fill { pts: ngon(14, 0.15, 0.0).into_iter().map(|q| q + base).collect(), ink: Ink::Accent });
            for k in 0..3 {
                let rr = 0.42 + k as f32 * 0.34;
                p.push(Prim::Stroke { pts: arc(base, rr, a - spread * 0.5, a + spread * 0.5, 10), closed: false, w: 0.12, ink: Ink::Brand });
            }
        }
    }
    Logo { family: fam, prims: p }
}

/// The part of the disc of radius `r` between heights `y0` and `y1`.
fn chord_band(r: f32, y0: f32, y1: f32) -> Vec<Vec2> {
    let y0 = y0.max(-r);
    let y1 = y1.min(r);
    if y1 <= y0 {
        return Vec::new();
    }
    let x = |y: f32| (r * r - y * y).max(0.0).sqrt();
    let n = 4;
    let mut out = Vec::new();
    for k in 0..=n {
        let y = y0 + (y1 - y0) * k as f32 / n as f32;
        out.push(Vec2::new(x(y), y));
    }
    for k in (0..=n).rev() {
        let y = y0 + (y1 - y0) * k as f32 / n as f32;
        out.push(Vec2::new(-x(y), y));
    }
    out.dedup_by(|a, b| a.distance(*b) < 1e-5);
    out
}

// ── wordmarks ─────────────────────────────────────────────────────────────────

/// How a company's name is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Type {
    pub slant: f32,
    /// Stroke width as a share of the letter height.
    pub weight: f32,
    /// An underline (swoosh) in the accent.
    pub underline: bool,
}

/// The type a mark's wordmark is set in.
pub fn type_of(m: &Mark) -> Type {
    Type { slant: if mix(m.seed, 60) % 3 == 0 { 0.2 } else { 0.0 }, weight: r(m.seed, 61, 0.1, 0.16), underline: mix(m.seed, 62) % 3 == 0 }
}

/// `text` as stroke primitives, letters `h` tall, laid from `(x0, y0)` (its
/// baseline's left end), in `ink`. Returns the primitives and the width.
pub fn text_prims(text: &str, x0: f32, y0: f32, h: f32, ty: Type, ink: Ink) -> (Vec<Prim>, f32) {
    let k = h / logo_font::GLYPH_H;
    let w = h * ty.weight;
    let prims = logo_font::text_lines(text, ty.slant).into_iter().map(|l| Prim::Stroke { pts: l.into_iter().map(|p| Vec2::new(x0, y0) + p * k).collect(), closed: false, w, ink }).collect();
    (prims, logo_font::text_width(text) * k + ty.slant * h)
}

/// Width of `text` at letter height `h`.
pub fn text_w(text: &str, h: f32, ty: Type) -> f32 {
    logo_font::text_width(text) * h / logo_font::GLYPH_H + ty.slant * h
}

/// [r17-logos] One line of a sign `w` x `h` metres centred at `centre`
/// (read along `right`, facing `normal`): the mark (when given) at its left
/// end, `text` beside it as large as fits, in `ink`.
#[allow(clippy::too_many_arguments)]
pub fn sign_line(m: &mut MeshData, centre: Vec3, right: Vec3, normal: Vec3, w: f32, h: f32, mark: Option<&Mark>, text: &str, paint: &Paint, ink: Ink) {
    let ty = match mark {
        Some(mk) => Type { underline: false, ..type_of(mk) },
        None => Type { slant: 0.0, weight: 0.14, underline: false },
    };
    let side = if mark.is_some() { h * 0.9 } else { 0.0 };
    let gap = if mark.is_some() { h * 0.25 } else { 0.0 };
    let room = (w - side - gap).max(0.0);
    let th = (room / text_w(text, 1.0, ty).max(0.01)).min(h * 0.72);
    let tw = text_w(text, th, ty);
    let total = side + gap + if th >= h * 0.25 { tw } else { 0.0 };
    let x0 = -total * 0.5;
    let mut t = Tess::default();
    if let Some(mk) = mark {
        let mut lt = tessellate(mk);
        lt.place(side * 0.5, Vec2::new(x0 + side * 0.5, 0.0));
        t.add_tess(lt);
    }
    if th >= h * 0.25 {
        let (p, _) = text_prims(text, x0 + side + gap, -th * 0.5, th, ty, ink);
        t.add(&p);
    }
    emit(m, &t, &Place { centre, right: right.normalize_or_zero(), up: Vec3::Y, normal, lift: (h * 0.01).clamp(0.003, 0.01) }, paint, 1.0);
}

// ── tessellation ──────────────────────────────────────────────────────────────

/// Indexed flat triangles: each vertex its point, ink and the paint layer
/// (the primitive's order, drawn a little proud of the one before).
#[derive(Clone, Debug, Default)]
pub struct Tess {
    pub pts: Vec<Vec2>,
    pub inks: Vec<Ink>,
    pub layers: Vec<u16>,
    pub tris: Vec<[u32; 3]>,
}

impl Tess {
    pub fn triangles(&self) -> usize {
        self.tris.len()
    }

    fn vertex(&mut self, p: Vec2, ink: Ink, layer: u16) -> u32 {
        self.pts.push(p);
        self.inks.push(ink);
        self.layers.push(layer);
        (self.pts.len() - 1) as u32
    }

    /// Add primitives (each its own layer, after those already in).
    pub fn add(&mut self, prims: &[Prim]) {
        let base_layer = self.layers.iter().copied().max().map_or(0, |l| l + 1);
        for (i, prim) in prims.iter().enumerate() {
            let layer = base_layer + i as u16;
            match prim {
                Prim::Fill { pts, ink } => self.fill(pts, *ink, layer),
                Prim::Stroke { pts, closed, w, ink } => self.stroke(pts, *closed, *w, *ink, layer),
            }
        }
    }

    fn fill(&mut self, pts: &[Vec2], ink: Ink, layer: u16) {
        if pts.len() < 3 {
            return;
        }
        let base = self.pts.len() as u32;
        for p in pts {
            self.vertex(*p, ink, layer);
        }
        for t in triangulate(pts) {
            self.tris.push([base + t[0] as u32, base + t[1] as u32, base + t[2] as u32]);
        }
    }

    /// A band `w` wide along the polyline, mitred at gentle turns and
    /// broken (each side squared off past the corner) at sharp ones.
    fn stroke(&mut self, pts: &[Vec2], closed: bool, w: f32, ink: Ink, layer: u16) {
        let mut pts: Vec<Vec2> = pts.to_vec();
        pts.dedup_by(|a, b| a.distance(*b) < 1e-5);
        if closed && pts.len() > 2 && pts[0].distance(pts[pts.len() - 1]) < 1e-5 {
            pts.pop();
        }
        let n = pts.len();
        if n < 2 {
            return;
        }
        let hw = w * 0.5;
        let seg = |i: usize| -> Vec2 { (pts[(i + 1) % n] - pts[i]).normalize_or_zero() };
        let segs = if closed { n } else { n - 1 };
        // Runs between sharp corners.
        let sharp = |i: usize| -> bool {
            // corner at vertex i between seg i-1 and seg i
            let (a, b) = (seg((i + segs - 1) % segs), seg(i % segs));
            a.dot(b) < -0.2
        };
        let mut runs: Vec<Vec<Vec2>> = Vec::new();
        if closed && !(0..n).any(sharp) {
            // Closed and smooth: one strip round, mitred everywhere.
            let mut left = Vec::with_capacity(n);
            let mut right = Vec::with_capacity(n);
            for i in 0..n {
                let (a, b) = (seg((i + n - 1) % n), seg(i));
                let (o, len) = miter(a, b, hw);
                left.push(pts[i] + o * len);
                right.push(pts[i] - o * len);
            }
            let base = self.pts.len() as u32;
            for i in 0..n {
                self.vertex(left[i], ink, layer);
                self.vertex(right[i], ink, layer);
            }
            for i in 0..n as u32 {
                let j = (i + 1) % n as u32;
                let (l0, r0, l1, r1) = (base + i * 2, base + i * 2 + 1, base + j * 2, base + j * 2 + 1);
                self.tris.push([l0, r0, r1]);
                self.tris.push([l0, r1, l1]);
            }
            return;
        }
        // Open, or closed with sharp corners: split at sharp corners.
        let mut cur = vec![pts[0]];
        for i in 1..=segs {
            let p = pts[i % n];
            cur.push(p);
            let at_end = i == segs;
            if !at_end && sharp(i % n) {
                runs.push(std::mem::replace(&mut cur, vec![p]));
            }
        }
        runs.push(cur);
        for run in runs {
            self.open_run(&run, hw, ink, layer);
        }
    }

    fn open_run(&mut self, pts: &[Vec2], hw: f32, ink: Ink, layer: u16) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        let d = |i: usize| (pts[i + 1] - pts[i]).normalize_or_zero();
        let base = self.pts.len() as u32;
        for i in 0..n {
            let (p, off, len) = if i == 0 {
                let a = d(0);
                (pts[0] - a * hw, a.perp(), hw)
            } else if i == n - 1 {
                let a = d(n - 2);
                (pts[n - 1] + a * hw, a.perp(), hw)
            } else {
                let (o, len) = miter(d(i - 1), d(i), hw);
                (pts[i], o, len)
            };
            self.vertex(p + off * len, ink, layer);
            self.vertex(p - off * len, ink, layer);
        }
        for i in 0..(n - 1) as u32 {
            let (l0, r0, l1, r1) = (base + i * 2, base + i * 2 + 1, base + i * 2 + 2, base + i * 2 + 3);
            self.tris.push([l0, r0, r1]);
            self.tris.push([l0, r1, l1]);
        }
    }

    /// Append another tessellation as the next layers.
    pub fn add_tess(&mut self, src: Tess) {
        let base_layer = self.layers.iter().copied().max().map_or(0, |l| l + 1);
        let base = self.pts.len() as u32;
        self.pts.extend(src.pts);
        self.inks.extend(src.inks);
        self.layers.extend(src.layers.into_iter().map(|l| l + base_layer));
        self.tris.extend(src.tris.into_iter().map(|tri| tri.map(|i| i + base)));
    }

    /// Scale and move every point (`p * k + at`).
    pub fn place(&mut self, k: f32, at: Vec2) {
        for p in &mut self.pts {
            *p = *p * k + at;
        }
    }
}

/// The miter at a corner from direction `a` into `b`: the unit offset and
/// its length for a band of half-width `hw` (capped).
fn miter(a: Vec2, b: Vec2, hw: f32) -> (Vec2, f32) {
    let (na, nb) = (a.perp(), b.perp());
    let m = (na + nb).normalize_or_zero();
    if m == Vec2::ZERO {
        return (nb, hw);
    }
    let c = m.dot(nb).max(0.35);
    (m, hw / c)
}

/// The logo of `m` tessellated in the unit square.
pub fn tessellate(m: &Mark) -> Tess {
    let mut t = Tess::default();
    t.add(&design(m).prims);
    t
}

// ── drawing ───────────────────────────────────────────────────────────────────

/// The lit colours of the inks for a mark (linear, unlit-glow scale).
#[derive(Clone, Copy, Debug)]
pub struct Paint {
    pub brand: [f32; 3],
    pub deep: [f32; 3],
    pub accent: [f32; 3],
    pub light: [f32; 3],
    pub dark: [f32; 3],
}

impl Paint {
    /// The mark lit at `k` (about 2-2.6 for neon signs).
    pub fn lit(m: &Mark, k: f32) -> Paint {
        let s = |c: [f32; 3], f: f32| [c[0] * f, c[1] * f, c[2] * f];
        Paint { brand: s(m.colour, k), deep: s(m.colour, k * 0.45), accent: s(m.accent, k * 0.95), light: [k * 0.95, k * 0.93, k * 0.88], dark: [0.02, 0.02, 0.025] }
    }

    pub fn of(&self, ink: Ink) -> [f32; 3] {
        match ink {
            Ink::Brand => self.brand,
            Ink::Deep => self.deep,
            Ink::Accent => self.accent,
            Ink::Light => self.light,
            Ink::Dark => self.dark,
        }
    }
}

/// Where a flat design goes in 3D: its unit square's centre, the reader's
/// right and up (scaled to half the side) and the facing normal; each
/// paint layer stands `lift` metres proud of the one before.
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub centre: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub normal: Vec3,
    pub lift: f32,
}

/// Write `t` into `m`: shared vertices, flat normals, colours from `paint`
/// with vertex alpha `alpha` (a glow mark, or 1).
pub fn emit(m: &mut MeshData, t: &Tess, place: &Place, paint: &Paint, alpha: f32) {
    let base = m.positions.len() as u32;
    let n = place.normal.normalize_or_zero();
    let tan = place.right.normalize_or_zero();
    let has_aux = !m.aux.is_empty();
    for i in 0..t.pts.len() {
        let p = t.pts[i];
        let pos = place.centre + place.right * p.x + place.up * p.y + n * (place.lift * t.layers[i] as f32);
        m.positions.push(pos.to_array());
        m.normals.push(n.to_array());
        let c = paint.of(t.inks[i]);
        m.colors.push([c[0], c[1], c[2], alpha]);
        m.uvs.push([p.x, p.y]);
        m.tangents.push([tan.x, tan.y, tan.z, 1.0]);
    }
    if has_aux {
        m.aux.resize(m.positions.len(), [0.0; 2]);
    }
    // Wound to face the normal.
    let face = place.right.cross(place.up).dot(n) >= 0.0;
    let at = |i: u32| Vec3::from(m.positions[(i + base) as usize]);
    for tri in &t.tris {
        let [a, b, c] = tri.map(|i| i + base);
        // A sliver (a tiny letter's joint, three points on a line) is left out.
        if (at(tri[1]) - at(tri[0])).cross(at(tri[2]) - at(tri[0])).length() < 4e-6 {
            continue;
        }
        let (pa, pb, pc) = (t.pts[tri[0] as usize], t.pts[tri[1] as usize], t.pts[tri[2] as usize]);
        let ccw = (pb - pa).perp_dot(pc - pa) >= 0.0;
        if ccw == face {
            m.indices.extend_from_slice(&[a, b, c]);
        } else {
            m.indices.extend_from_slice(&[a, c, b]);
        }
    }
}

/// Paint `t` (unit square, y up) into an RGB image `size` pixels square at
/// `(ox, oy)` of a `stride`-wide buffer, over what is there: for the
/// contact sheet and the tests. Four samples a pixel, each taking the last
/// triangle over it (paint order), so a fan has no seams.
pub fn rasterize(buf: &mut [[f32; 3]], stride: usize, ox: usize, oy: usize, size: usize, t: &Tess, paint: &Paint) {
    const SS: usize = 2;
    let n = size * SS;
    let mut sub: Vec<Option<[f32; 3]>> = vec![None; n * n];
    let s = n as f32 * 0.5;
    let to_px = |p: Vec2| Vec2::new(s + p.x * s * 0.95, s - p.y * s * 0.95);
    for tri in &t.tris {
        let [a, b, c] = tri.map(|i| to_px(t.pts[i as usize]));
        let col = paint.of(t.inks[tri[0] as usize]);
        let area = (b - a).perp_dot(c - a);
        if area.abs() < 1e-9 {
            continue;
        }
        let lo = a.min(b).min(c).floor().max(Vec2::ZERO);
        let hi = a.max(b).max(c).ceil().min(Vec2::splat(n as f32));
        for y in lo.y as usize..hi.y as usize {
            for x in lo.x as usize..hi.x as usize {
                let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w0 = (b - a).perp_dot(q - a) / area;
                let w1 = (c - b).perp_dot(q - b) / area;
                let w2 = (a - c).perp_dot(q - c) / area;
                if w0 >= -1e-6 && w1 >= -1e-6 && w2 >= -1e-6 {
                    sub[y * n + x] = Some(col);
                }
            }
        }
    }
    let rows = buf.len() / stride;
    for y in 0..size {
        for x in 0..size {
            let (bx, by) = (ox + x, oy + y);
            if bx >= stride || by >= rows {
                continue;
            }
            let px = &mut buf[by * stride + bx];
            let base = *px;
            let mut acc = [0.0f32; 3];
            for sy in 0..SS {
                for sx in 0..SS {
                    let c = sub[(y * SS + sy) * n + x * SS + sx].unwrap_or(base);
                    for k in 0..3 {
                        acc[k] += c[k] / (SS * SS) as f32;
                    }
                }
            }
            *px = acc;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks() -> Vec<Mark> {
        let mut v = Vec::new();
        for c in 0..BRAND_COLOURS.len() as u8 {
            for s in LogoShape::CORP {
                v.push(Mark::corp(c, s, (b'A' + (c * 7 + s as u8) % 26) as char));
            }
        }
        v
    }

    #[test]
    fn every_mark_designs_a_drawable_logo_in_the_square() {
        for m in marks().into_iter().chain(["Rocket Burger", "Lucky Noodle Bar", "Okafor Builders", "Pie Hard"].map(Mark::local)) {
            let t = tessellate(&m);
            assert!(t.triangles() >= 3, "{:?} {:?}", design(&m).family, m);
            assert!(t.triangles() < 900, "{:?} {} tris", design(&m).family, t.triangles());
            for p in &t.pts {
                assert!(p.x.abs() < 1.25 && p.y.abs() < 1.25 && p.is_finite(), "{:?} {p}", design(&m).family);
            }
            for tri in &t.tris {
                assert!(tri.iter().all(|i| (*i as usize) < t.pts.len()));
            }
        }
    }

    #[test]
    fn marks_are_stable_and_varied() {
        let all = marks();
        assert_eq!(design(&all[3]), design(&all[3]));
        let fams: std::collections::HashSet<Family> = all.iter().map(family).collect();
        assert!(fams.len() >= 12, "{} families used of 14", fams.len());
        // Two corps rarely share a design: compare family and colour.
        let mut same = 0;
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                if design(a) == design(b) && a.colour == b.colour {
                    same += 1;
                }
            }
        }
        assert_eq!(same, 0);
    }

    #[test]
    fn a_chain_keeps_one_mark_across_branches() {
        assert_eq!(Mark::local("Rocket Burger #3"), Mark::local("Rocket Burger"));
        assert_eq!(Mark::local("The Greasy Spoon"), Mark::local("Greasy Spoon"));
        assert_ne!(Mark::local("Rocket Burger"), Mark::local("Zap Burger"));
        assert_eq!(Mark::local("Rocket Burger").monogram, 'R');
        assert_eq!(trading_name("Pie Hard #12"), "Pie Hard");
        assert_eq!(trading_name("Route #9 Diner"), "Route #9 Diner");
    }

    #[test]
    fn the_corp_mark_follows_only_colour_shape_and_initial() {
        let b = Brand { name: "Vorion Dynamics".into(), colour: 4, shape: LogoShape::Hexagon, monogram: 'V' };
        assert_eq!(Mark::of_brand(&b), Mark::corp(4, LogoShape::Hexagon, 'V'));
    }

    #[test]
    fn strokes_cover_their_line() {
        let mut t = Tess::default();
        t.add(&[Prim::Stroke { pts: vec![Vec2::new(-0.5, 0.0), Vec2::new(0.5, 0.0)], closed: false, w: 0.2, ink: Ink::Brand }]);
        let mut buf = vec![[0.0f32; 3]; 64 * 64];
        let paint = Paint { brand: [1.0; 3], deep: [0.0; 3], accent: [0.0; 3], light: [0.0; 3], dark: [0.0; 3] };
        rasterize(&mut buf, 64, 0, 0, 64, &t, &paint);
        assert!(buf[32 * 64 + 32][0] > 0.9, "the middle of the line is lit");
        assert!(buf[10 * 64 + 32][0] < 0.1, "off the line is not");
    }

    #[test]
    fn emitted_triangles_face_the_normal() {
        let m = Mark::corp(2, LogoShape::Circle, 'Q');
        let t = tessellate(&m);
        let mut mesh = MeshData::default();
        let place = Place { centre: Vec3::ZERO, right: Vec3::X, up: Vec3::Y, normal: Vec3::Z, lift: 0.01 };
        emit(&mut mesh, &t, &place, &Paint::lit(&m, 2.0), 1.0);
        assert!(mesh.indices.len() <= t.tris.len() * 3 && mesh.indices.len() * 10 >= t.tris.len() * 27);
        for tri in mesh.indices.chunks(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(mesh.positions[i as usize]));
            let n = (b - a).cross(c - a);
            assert!(n.z >= -1e-6, "faces +z");
        }
    }
}
