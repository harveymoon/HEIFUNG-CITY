//! [looks] Low-poly people: a figure built from a citizen's look (skin,
//! hair, build, outfit in their two favourite colours), in vertex colours.
//!
//! A figure is keyed by a small quantised [`PersonKey`] so the renderer can
//! share one mesh between everyone who looks alike and cache the rest; size
//! (a child, a tall adult) is not in the key, it is the entity's scale
//! ([`Appearance::stature`]).
//!
//! Night trim: every vertex's UV points at one cell of a small palette
//! texture ([`emissive_palette`]) that the renderer uses as the emissive
//! map of the one shared material. Skin and hair point at black, clothes at
//! a dim version of their colour, the trim (a waistband, a collar, a tie, a
//! reflective stripe) at the full favourite colour, so at night people read
//! as softly lit figures with a thin line of their colour, not as glow sticks.

use crate::MeshData;
use glam::Vec3;
use nd_sim::buildings::Tier;
use nd_sim::citizens::ADULT_AGE;
use nd_sim::looks::{linear, skin_srgb, Appearance, Build, HairColor, HairStyle, Outfit, ShownHair, COLORS};

/// Skin tones a figure is quantised to.
pub const SKIN_STEPS: u8 = 10;
/// Triangle budget for a standing figure (a rider has a bike as well).
pub const MAX_TRIS: usize = 150;
/// A rider's bike has round 16-sided wheels and a tube frame.
pub const MAX_RIDER_TRIS: usize = 440;
/// Segments round a bike wheel's tyre.
pub const WHEEL_SIDES: usize = 16;
/// [K18] A seated figure's limbs are round tubes.
pub const MAX_SEATED_TRIS: usize = 180;
/// A tourist carries a backpack and a camera as well.
pub const MAX_TOURIST_TRIS: usize = 180;

/// Everything a figure's mesh depends on, quantised. Ordered, so the
/// figure cache evicts and lends in the same order every run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PersonKey {
    pub style: u8,
    pub outfit: u8,
    pub build: u8,
    /// A child's proportions (bigger head).
    pub child: bool,
    pub skin: u8,
    /// `0..6` natural (`HairColor::ALL`), 6 grey, 7 white, `8 + i` dyed `COLORS[i]`.
    pub hair: u8,
    pub fav: [u8; 2],
    /// Wear of the clothes by class: 0 worn, 1 everyday, 2 crisp.
    pub wear: u8,
    /// On a bike.
    pub ride: bool,
    /// [tourism] A visitor: a backpack and a camera on the strap.
    pub tourist: bool,
    /// [K18] Sitting: 0 standing (or riding), [`SIT_BENCH`] on a bench or
    /// a seat, [`SIT_GROUND`] on the pavement or the kerb, knees up.
    pub sit: u8,
}

/// [`PersonKey::sit`]: on a bench, the seat 0.46 m up.
pub const SIT_BENCH: u8 = 1;
/// [`PersonKey::sit`]: on the ground (the kerb, the pavement), knees up.
pub const SIT_GROUND: u8 = 2;
/// Seat height of a bench figure, metres.
pub const BENCH_SEAT: f32 = 0.46;

/// The silhouette part of a key: the same shape in other colours.
pub fn shape_of(k: &PersonKey) -> (u8, u8, u8, bool, bool, u8) {
    (k.style, k.outfit, k.build, k.child, k.ride, k.sit)
}

pub fn person_key(look: &Appearance, age: u8, class: Tier, ride: bool) -> PersonKey {
    let hair = match look.shown_hair(age) {
        ShownHair::Natural(c) => HairColor::ALL.iter().position(|h| *h == c).unwrap_or(0) as u8,
        ShownHair::Grey => 6,
        ShownHair::White => 7,
        ShownHair::Dyed(i) => 8 + i.min(COLORS.len() as u8 - 1),
    };
    PersonKey {
        style: look.style.index(),
        outfit: look.outfit.index(),
        build: match look.build {
            Build::Slim => 0,
            Build::Average => 1,
            Build::Broad => 2,
        },
        child: age < 12,
        skin: ((look.genes.skin.clamp(0.0, 1.0) * SKIN_STEPS as f32) as u8).min(SKIN_STEPS - 1),
        hair,
        fav: look.fav.map(|f| f.min(COLORS.len() as u8 - 1)),
        wear: if age < ADULT_AGE {
            1
        } else {
            match class {
                Tier::Low => 0,
                Tier::Mid => 1,
                Tier::High => 2,
            }
        },
        ride,
        tourist: false,
        sit: 0,
    }
}

/// [tourism] A visitor's figure: their look with a backpack and camera.
pub fn tourist_key(look: &Appearance, age: u8, class: Tier) -> PersonKey {
    PersonKey { tourist: true, ..person_key(look, age, class, false) }
}

// ── the emissive palette ───────────────────────────────────────────────────

/// Cells in the emissive palette: black, each colour bright, each dim.
pub const EMISSIVE_CELLS: usize = 1 + 2 * COLORS.len();
const DIM: f32 = 0.11;

fn bright_cell(i: u8) -> usize {
    1 + i as usize
}

fn dim_cell(i: u8) -> usize {
    1 + COLORS.len() + i as usize
}

fn cell_uv(cell: usize) -> [f32; 2] {
    [(cell as f32 + 0.5) / EMISSIVE_CELLS as f32, 0.5]
}

/// The emissive palette, linear RGB, one entry per cell: what each cell
/// glows at full night strength. Bright cells are the colour pushed to full
/// intensity (black and greys become a soft white), so even a navy trim
/// reads after dark.
pub fn emissive_palette() -> Vec<[f32; 3]> {
    let mut out = vec![[0.0; 3]; EMISSIVE_CELLS];
    for (i, sw) in COLORS.iter().enumerate() {
        let c = linear(sw.srgb);
        let m = c[0].max(c[1]).max(c[2]);
        let bright = if m < 0.05 {
            [0.55, 0.57, 0.6]
        } else {
            let k = 1.0 / m;
            [c[0] * k, c[1] * k, c[2] * k]
        };
        out[bright_cell(i as u8)] = bright;
        out[dim_cell(i as u8)] = bright.map(|v| v * DIM);
    }
    out
}

// ── the figure ─────────────────────────────────────────────────────────────

use crate::color::rgba;

/// Clothes by wear: worn ones faded and a little dark.
fn worn(c: [f32; 3], wear: u8) -> [f32; 3] {
    match wear {
        0 => {
            let l = c[0] * 0.3 + c[1] * 0.55 + c[2] * 0.15;
            c.map(|v| (v + (l - v) * 0.35) * 0.8)
        }
        _ => c,
    }
}

struct Builder<'a> {
    m: &'a mut MeshData,
}

impl Builder<'_> {
    /// An axis-aligned box from `lo` to `hi`, its UVs on `cell`. `bottom`
    /// keeps the underside (things seen from below: arms, hair, a hood).
    fn part(&mut self, lo: Vec3, hi: Vec3, color: [f32; 3], cell: usize, bottom: bool) {
        let v0 = self.m.positions.len();
        let (c, s) = ((lo + hi) * 0.5, hi - lo);
        if bottom {
            self.m.boxed(c, s, 0.0, rgba(color));
        } else {
            self.m.boxed_open(c, s, 0.0, rgba(color));
        }
        self.paint(v0, cell);
    }

    /// A round tube from `a` to `b` (four sides, open ends: too thin for
    /// anyone to see in), its UVs on `cell`.
    fn tube(&mut self, a: Vec3, b: Vec3, r: f32, color: [f32; 3], cell: usize) {
        let v0 = self.m.positions.len();
        self.m.tube_capped(a, b, r, 4, false, false, rgba(color));
        self.paint(v0, cell);
    }

    /// A bike wheel in the XY plane centred on `c`: a tyre ring of
    /// `WHEEL_SIDES` segments (both flat faces and the tread) from radius
    /// `inner` to `outer`, `half_w` either side of z = `c.z`, and three
    /// thin spokes across the hub, seen from both sides.
    fn wheel(&mut self, c: Vec3, outer: f32, inner: f32, half_w: f32, color: [f32; 3]) {
        let v0 = self.m.positions.len();
        let col = rgba(color);
        let at = |i: usize, r: f32, z: f32| {
            let t = i as f32 / WHEEL_SIDES as f32 * std::f32::consts::TAU;
            Vec3::new(c.x + t.cos() * r, c.y + t.sin() * r, c.z + z)
        };
        for i in 0..WHEEL_SIDES {
            let j = i + 1;
            let mid = (i as f32 + 0.5) / WHEEL_SIDES as f32 * std::f32::consts::TAU;
            let radial = Vec3::new(mid.cos(), mid.sin(), 0.0);
            for s in [-1.0f32, 1.0] {
                let z = s * half_w;
                self.m.quad_facing([at(i, inner, z), at(j, inner, z), at(j, outer, z), at(i, outer, z)], Vec3::Z * s, col);
            }
            self.m.quad_facing([at(i, outer, -half_w), at(j, outer, -half_w), at(j, outer, half_w), at(i, outer, half_w)], radial, col);
        }
        for k in 0..3 {
            let t = k as f32 / 3.0 * std::f32::consts::PI + 0.3;
            let (d, w) = (Vec3::new(t.cos(), t.sin(), 0.0) * inner, Vec3::new(-t.sin(), t.cos(), 0.0) * 0.008);
            for s in [-1.0f32, 1.0] {
                let z = Vec3::Z * (s * 0.006);
                self.m.quad_facing([c - d - w + z, c + d - w + z, c + d + w + z, c - d + w + z], Vec3::Z * s, col);
            }
        }
        self.paint(v0, 0);
    }

    fn paint(&mut self, from: usize, cell: usize) {
        let uv = cell_uv(cell);
        for u in &mut self.m.uvs[from..] {
            *u = uv;
        }
    }
}

/// Head and hair on a head whose bottom centre is `at`.
fn head(b: &mut Builder, k: &PersonKey, at: Vec3, scale: f32, skin: [f32; 3]) {
    let hw = 0.22 * scale;
    let hd = 0.24 * scale;
    let hh = 0.27 * scale;
    let (hb, ht) = (at.y, at.y + hh);
    b.part(Vec3::new(at.x - hd / 2.0, hb, -hw / 2.0), Vec3::new(at.x + hd / 2.0, ht, hw / 2.0), skin, 0, false);
    let hair_rgb = match k.hair {
        h @ 0..=5 => linear(HairColor::ALL[h as usize].srgb()),
        6 => linear(ShownHair::Grey.srgb()),
        7 => linear(ShownHair::White.srgb()),
        d => linear(COLORS[(d - 8) as usize].srgb),
    };
    let dyed = k.hair >= 8;
    let cell = if dyed { dim_cell(k.hair - 8) } else { 0 };
    let back = at.x - hd / 2.0;
    let front = at.x + hd / 2.0;
    let w = |extra: f32| hw / 2.0 + extra * scale;
    let style = HairStyle::ALL[(k.style as usize).min(HairStyle::ALL.len() - 1)];
    match style {
        HairStyle::Short => {
            b.part(Vec3::new(back - 0.015 * scale, hb + hh * 0.45, -w(0.015)), Vec3::new(front - hd * 0.35, ht + 0.035 * scale, w(0.015)), hair_rgb, cell, true);
        }
        HairStyle::Long => {
            b.part(Vec3::new(back - 0.02 * scale, hb - 0.12 * scale, -w(0.02)), Vec3::new(front - hd * 0.4, ht + 0.035 * scale, w(0.02)), hair_rgb, cell, true);
        }
        HairStyle::Bun => {
            b.part(Vec3::new(back - 0.012 * scale, hb + hh * 0.5, -w(0.012)), Vec3::new(front - hd * 0.35, ht + 0.025 * scale, w(0.012)), hair_rgb, cell, true);
            let c = Vec3::new(back + hd * 0.15, ht + 0.03 * scale, 0.0);
            let r = 0.06 * scale;
            b.part(c - Vec3::new(r, 0.0, r), c + Vec3::new(r, 2.0 * r, r), hair_rgb, cell, true);
        }
        HairStyle::Buzz => {
            b.part(Vec3::new(back - 0.008 * scale, hb + hh * 0.6, -w(0.008)), Vec3::new(front - hd * 0.2, ht + 0.012 * scale, w(0.008)), hair_rgb, cell, false);
        }
        HairStyle::Bald => {}
        HairStyle::Mohawk => {
            let cell = if dyed { bright_cell(k.hair - 8) } else { 0 };
            b.part(Vec3::new(back - 0.01 * scale, ht - 0.01, -0.028 * scale), Vec3::new(front - 0.02 * scale, ht + 0.12 * scale, 0.028 * scale), hair_rgb, cell, true);
        }
        HairStyle::Afro => {
            b.part(Vec3::new(back - 0.06 * scale, hb + hh * 0.4, -w(0.07)), Vec3::new(front - hd * 0.25, ht + 0.1 * scale, w(0.07)), hair_rgb, cell, true);
        }
    }
}

/// The figure for `k`, standing (or riding) at the origin facing +X, an
/// average adult about 1.8 m tall: scale it by the citizen's stature.
pub fn person_mesh(k: &PersonKey) -> MeshData {
    let mut m = MeshData::default();
    let mut b = Builder { m: &mut m };
    let skin = linear(skin_srgb((k.skin as f32 + 0.5) / SKIN_STEPS as f32));
    let fav = k.fav.map(|f| f.min(COLORS.len() as u8 - 1));
    let main = worn(linear(COLORS[fav[0] as usize].srgb), k.wear);
    let accent = worn(linear(COLORS[fav[1] as usize].srgb), k.wear);
    // The trim glows in whichever favourite is a neon (the accent if both).
    let trim_idx = if COLORS[fav[1] as usize].neon || !COLORS[fav[0] as usize].neon { fav[1] } else { fav[0] };
    let trim = linear(COLORS[trim_idx as usize].srgb);
    let trim_cell = bright_cell(trim_idx);
    let (main_cell, accent_cell) = (dim_cell(fav[0]), dim_cell(fav[1]));
    let bw = match k.build {
        0 => 0.86,
        2 => 1.18,
        _ => 1.0,
    };
    let head_scale = if k.child { 1.28 } else { 1.0 };
    let outfit = Outfit::ALL[(k.outfit as usize).min(Outfit::ALL.len() - 1)];
    let (tw, td) = (0.44 * bw, 0.25 * bw);
    let bare_arms = matches!(outfit, Outfit::Tee | Outfit::Dress);
    let (sleeve, sleeve_cell) = if bare_arms { (skin, 0) } else { (main, main_cell) };
    let (legs, legs_cell) = match outfit {
        Outfit::Suit => (main, main_cell),
        Outfit::Dress => (skin, 0),
        _ => (accent, accent_cell),
    };

    if k.sit != 0 && !k.ride {
        seated(&mut b, k, bw, td, tw, head_scale, skin, (main, main_cell), (legs, legs_cell), (sleeve, sleeve_cell), (trim, trim_cell), outfit);
    } else if !k.ride {
        let hip = 0.86;
        let top = 1.46;
        // Legs.
        for s in [-1.0f32, 1.0] {
            let z = s * 0.085 * bw;
            b.part(Vec3::new(-0.085 * bw, 0.0, z - 0.075 * bw), Vec3::new(0.085 * bw, hip, z + 0.075 * bw), legs, legs_cell, false);
        }
        // Torso, by cut.
        match outfit {
            Outfit::Coat => b.part(Vec3::new(-td * 0.56, 0.48, -tw * 0.52), Vec3::new(td * 0.56, top + 0.01, tw * 0.52), main, main_cell, true),
            _ => b.part(Vec3::new(-td / 2.0, hip - 0.02, -tw / 2.0), Vec3::new(td / 2.0, top, tw / 2.0), main, main_cell, false),
        }
        if outfit == Outfit::Dress {
            let ring = |y: f32, hx: f32, hz: f32| vec![Vec3::new(-hx, y, -hz), Vec3::new(hx, y, -hz), Vec3::new(hx, y, hz), Vec3::new(-hx, y, hz)];
            let v0 = b.m.positions.len();
            b.m.frustum(&ring(0.44, 0.2 * bw, 0.27 * bw), &ring(0.9, 0.13 * bw, 0.22 * bw), rgba(main));
            b.paint(v0, main_cell);
        }
        // Arms.
        for s in [-1.0f32, 1.0] {
            let z = s * (tw / 2.0 + 0.055);
            b.part(Vec3::new(-0.06, 0.82, z - 0.05), Vec3::new(0.06, top - 0.02, z + 0.05), sleeve, sleeve_cell, true);
        }
        // Trim: the line of colour that shows at night.
        let band = |b: &mut Builder, y0: f32, y1: f32, grow: f32| b.part(Vec3::new(-td / 2.0 - grow, y0, -tw / 2.0 - grow), Vec3::new(td / 2.0 + grow, y1, tw / 2.0 + grow), trim, trim_cell, false);
        match outfit {
            Outfit::Tee => band(&mut b, hip - 0.02, hip + 0.03, 0.01),
            Outfit::Hoodie => {
                band(&mut b, hip - 0.02, hip + 0.04, 0.012);
                b.part(Vec3::new(-td / 2.0 - 0.07, top - 0.08, -0.15 * bw), Vec3::new(-td / 2.0 + 0.03, top + 0.1, 0.15 * bw), main, main_cell, true);
            }
            Outfit::WorkJacket => band(&mut b, 1.18, 1.24, 0.012),
            Outfit::Suit => b.part(Vec3::new(td / 2.0, 1.14, -0.035), Vec3::new(td / 2.0 + 0.012, top - 0.02, 0.035), trim, trim_cell, false),
            Outfit::Dress => band(&mut b, 0.88, 0.94, 0.012),
            Outfit::Coat => b.part(Vec3::new(-td * 0.56 - 0.012, top - 0.07, -tw * 0.52 - 0.012), Vec3::new(td * 0.56 + 0.012, top + 0.02, tw * 0.52 + 0.012), trim, trim_cell, false),
        }
        head(&mut b, k, Vec3::new(0.0, top + 0.03, 0.0), head_scale, skin);
        if k.tourist {
            // A daypack on the back in the accent colour with a reflective
            // band that catches the night, and a dark camera at the chest.
            // Daypacks come in loud outdoor colours whatever the wearer's
            // taste: that is what marks a visitor from above.
            const PACKS: [[f32; 3]; 5] = [[0.95, 0.35, 0.04], [0.95, 0.75, 0.05], [0.05, 0.6, 0.85], [0.85, 0.08, 0.1], [0.2, 0.75, 0.25]];
            let pack = PACKS[(k.fav[0] as usize + k.skin as usize) % PACKS.len()];
            let back = -td / 2.0;
            b.part(Vec3::new(back - 0.2, 0.9, -0.15 * bw), Vec3::new(back, 1.4, 0.15 * bw), pack, 0, true);
            b.part(Vec3::new(back - 0.21, 1.12, -0.152 * bw), Vec3::new(back - 0.19, 1.17, 0.152 * bw), trim, trim_cell, false);
            b.part(Vec3::new(td / 2.0, 1.08, -0.06), Vec3::new(td / 2.0 + 0.08, 1.17, 0.06), [0.03, 0.03, 0.035], 0, false);
        }
    } else {
        // The bike: round wheels (a 16-sided tyre ring with three spokes
        // across), a diamond frame of round tubes in the rider's accent
        // colour, a saddle, a stem and drop of bars. [round-wheels]
        let dark = [0.03, 0.03, 0.035];
        let steel = [0.35, 0.36, 0.4];
        for x in [-0.55f32, 0.55] {
            b.wheel(Vec3::new(x, 0.34, 0.0), 0.34, 0.29, 0.028, dark);
        }
        let (rear, front) = (Vec3::new(-0.55, 0.34, 0.0), Vec3::new(0.55, 0.34, 0.0));
        let bb = Vec3::new(-0.02, 0.3, 0.0);
        let seat = Vec3::new(-0.15, 0.86, 0.0);
        let head_top = Vec3::new(0.36, 0.98, 0.0);
        let head_low = Vec3::new(0.41, 0.8, 0.0);
        for (a, c) in [(bb, seat), (seat, head_top), (bb, head_low), (head_top, head_low), (bb, rear), (seat - Vec3::new(0.0, 0.05, 0.0), rear)] {
            b.tube(a, c, 0.022, accent, accent_cell);
        }
        // Fork, stem and the bars across.
        b.tube(head_low, front, 0.018, steel, 0);
        b.tube(head_top, Vec3::new(0.38, 1.24, 0.0), 0.02, steel, 0);
        b.tube(Vec3::new(0.38, 1.24, -0.24), Vec3::new(0.38, 1.24, 0.24), 0.018, dark, 0);
        b.part(Vec3::new(-0.27, 0.86, -0.06), Vec3::new(-0.03, 0.91, 0.06), dark, 0, true);
        // The rider, seated, leaning on the bars.
        for s in [-1.0f32, 1.0] {
            let z = s * 0.085 * bw;
            b.part(Vec3::new(-0.2, 0.9, z - 0.07 * bw), Vec3::new(0.2, 1.02, z + 0.07 * bw), legs, legs_cell, true);
            b.part(Vec3::new(0.12, 0.3, z - 0.06 * bw), Vec3::new(0.24, 0.92, z + 0.06 * bw), legs, legs_cell, true);
        }
        let top = 1.58;
        b.part(Vec3::new(-0.24 - td / 2.0 + 0.1, 0.98, -tw / 2.0), Vec3::new(-0.14 + td / 2.0, top, tw / 2.0), main, main_cell, false);
        for s in [-1.0f32, 1.0] {
            let z = s * (tw / 2.0 + 0.05);
            b.part(Vec3::new(-0.1, 1.3, z - 0.05), Vec3::new(0.36, 1.4, z + 0.05), sleeve, sleeve_cell, true);
        }
        b.part(Vec3::new(-0.24 - td / 2.0 + 0.09, 1.0, -tw / 2.0 - 0.01), Vec3::new(-0.14 + td / 2.0 + 0.01, 1.06, tw / 2.0 + 0.01), trim, trim_cell, false);
        head(&mut b, k, Vec3::new(-0.12, top + 0.03, 0.0), head_scale, skin);
    }
    m
}

/// [K18] A seated figure facing +X: on a bench (the seat [`BENCH_SEAT`]
/// up, thighs level, shins down to the ground) or on the ground (sat on
/// the kerb or the pavement, knees drawn up, forearms on them). Limbs are
/// round tubes along their bones; the torso and head as standing.
#[allow(clippy::too_many_arguments)]
fn seated(b: &mut Builder, k: &PersonKey, bw: f32, td: f32, tw: f32, head_scale: f32, skin: [f32; 3], main: ([f32; 3], usize), legs: ([f32; 3], usize), sleeve: ([f32; 3], usize), trim: ([f32; 3], usize), outfit: Outfit) {
    let bench = k.sit == SIT_BENCH;
    let leg_r = 0.075 * bw;
    // Hip, knee and foot in the figure's own (x, y); the torso's foot.
    let (hip, knee, foot, seat) = if bench {
        (Vec3::new(-0.05, BENCH_SEAT + 0.07, 0.0), Vec3::new(0.38, BENCH_SEAT + 0.07, 0.0), Vec3::new(0.42, 0.04, 0.0), BENCH_SEAT)
    } else {
        (Vec3::new(-0.04, 0.09, 0.0), Vec3::new(0.3, 0.44, 0.0), Vec3::new(0.46, 0.05, 0.0), 0.0)
    };
    for s in [-1.0f32, 1.0] {
        let z = Vec3::Z * (s * 0.09 * bw);
        b.tube(hip + z, knee + z, leg_r, legs.0, legs.1);
        b.tube(knee + z, foot + z, leg_r * 0.9, legs.0, legs.1);
        // The shoe.
        b.part(foot + z + Vec3::new(-0.06, -0.04, -0.05), foot + z + Vec3::new(0.12, 0.06, 0.05), [0.05, 0.05, 0.06], 0, true);
    }
    // Torso: upright on a bench, a little forward on the ground.
    let lean = if bench { 0.0 } else { 0.08 };
    let (y0, y1) = (seat + 0.04, seat + 0.64);
    b.part(Vec3::new(-td / 2.0 - 0.04, y0, -tw / 2.0), Vec3::new(td / 2.0 - 0.04 + lean, y1, tw / 2.0), main.0, main.1, true);
    // The trim band at the waist.
    b.part(Vec3::new(-td / 2.0 - 0.05, y0 + 0.02, -tw / 2.0 - 0.01), Vec3::new(td / 2.0 - 0.03 + lean, y0 + 0.07, tw / 2.0 + 0.01), trim.0, trim.1, false);
    // Arms: down to the lap on a bench, forearms on the knees on the ground.
    let shoulder = |s: f32| Vec3::new(-0.02 + lean, y1 - 0.05, s * (tw / 2.0 + 0.05));
    for s in [-1.0f32, 1.0] {
        let hand = if bench { Vec3::new(0.22, seat + 0.2, s * (tw / 2.0 + 0.02)) } else { knee + Vec3::new(-0.02, 0.04, s * 0.13) };
        let elbow = if bench { Vec3::new(0.02, seat + 0.3, s * (tw / 2.0 + 0.06)) } else { (shoulder(s) + hand) * 0.5 + Vec3::new(0.0, -0.06, 0.0) };
        b.tube(shoulder(s), elbow, 0.05, sleeve.0, sleeve.1);
        b.tube(elbow, hand, 0.045, sleeve.0, sleeve.1);
    }
    if outfit == Outfit::Coat {
        // The coat's skirt over the seat.
        b.part(Vec3::new(-td / 2.0 - 0.06, seat, -tw * 0.55), Vec3::new(td / 2.0 + 0.04, y0 + 0.2, tw * 0.55), main.0, main.1, true);
    }
    head(b, k, Vec3::new(-0.02 + lean * 1.5, y1 + 0.03, 0.0), head_scale, skin);
}

// ── [fire-scene] firefighters ──────────────────────────────────────────────

/// What a firefighter on scene is doing with their arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FirePose {
    /// Walking or standing by: arms at the sides.
    Stand,
    /// On a hose line or the ladder's nozzle: both hands forward on the
    /// branch pipe, braced.
    Nozzle,
    /// At the pump panel: one hand on the levers.
    Pump,
}

impl FirePose {
    pub const ALL: [FirePose; 3] = [FirePose::Stand, FirePose::Nozzle, FirePose::Pump];
}

/// Most triangles a firefighter takes (a few on screen at a time, so
/// richer than a passer-by).
pub const MAX_FIREFIGHTER_TRIS: usize = 280;

/// A firefighter in turnout gear: a dark coat and trousers banded with
/// reflective stripes (lit at night, on the acid-green trim cell), boots, a
/// breathing-air cylinder on the back and a yellow helmet with a brim. At
/// the origin facing +X, about 1.85 m tall: scale it by stature. `skin` is
/// a step of `SKIN_STEPS`.
pub fn firefighter_mesh(skin: u8, pose: FirePose) -> MeshData {
    let mut m = MeshData::default();
    let mut b = Builder { m: &mut m };
    let skin = linear(skin_srgb((skin.min(SKIN_STEPS - 1) as f32 + 0.5) / SKIN_STEPS as f32));
    let coat = [0.03, 0.032, 0.036];
    let coat_cell = dim_cell(9);
    let stripe = [0.75, 0.95, 0.08];
    // Acid green: the stripes glow at night like the real retroreflectors.
    let stripe_cell = bright_cell(5);
    let silver = [0.6, 0.62, 0.65];
    let boots = [0.012, 0.012, 0.014];
    let helmet = [0.95, 0.68, 0.04];
    let (tw, td) = (0.5, 0.3);
    let hip = 0.84;
    let top = 1.46;
    // Legs, boots and the trouser stripe.
    for s in [-1.0f32, 1.0] {
        let z = s * 0.1;
        let lean = if pose == FirePose::Nozzle { s * 0.06 } else { 0.0 };
        b.part(Vec3::new(-0.09 + lean, 0.16, z - 0.085), Vec3::new(0.09 + lean, hip, z + 0.085), coat, coat_cell, false);
        b.part(Vec3::new(-0.1 + lean, 0.0, z - 0.09), Vec3::new(0.13 + lean, 0.18, z + 0.09), boots, 0, false);
        b.part(Vec3::new(-0.095 + lean, 0.3, z - 0.09), Vec3::new(0.095 + lean, 0.36, z + 0.09), stripe, stripe_cell, false);
    }
    // The coat: long, over the hips.
    b.part(Vec3::new(-td / 2.0, 0.62, -tw / 2.0), Vec3::new(td / 2.0, top, tw / 2.0), coat, coat_cell, true);
    // Stripes round the hem and the chest, silver between yellow.
    let band = |b: &mut Builder, y0: f32, y1: f32, c: [f32; 3], cell: usize| b.part(Vec3::new(-td / 2.0 - 0.012, y0, -tw / 2.0 - 0.012), Vec3::new(td / 2.0 + 0.012, y1, tw / 2.0 + 0.012), c, cell, false);
    band(&mut b, 0.68, 0.75, stripe, stripe_cell);
    band(&mut b, 0.75, 0.78, silver, stripe_cell);
    band(&mut b, 1.17, 1.24, stripe, stripe_cell);
    // The air cylinder on the back.
    b.part(Vec3::new(-td / 2.0 - 0.17, 0.92, -0.09), Vec3::new(-td / 2.0, 1.42, 0.09), silver, 0, true);
    // Arms, by what they are doing; cuffs striped.
    for s in [-1.0f32, 1.0] {
        let z = s * (tw / 2.0 + 0.06);
        let forward = match pose {
            FirePose::Stand => false,
            FirePose::Nozzle => true,
            FirePose::Pump => s > 0.0,
        };
        if forward {
            // Upper arm down, forearm out in front at chest height.
            b.part(Vec3::new(-0.06, 1.1, z - 0.06), Vec3::new(0.08, top - 0.02, z + 0.06), coat, coat_cell, true);
            let zf = if pose == FirePose::Nozzle { z * 0.55 } else { z };
            b.part(Vec3::new(0.0, 1.06, zf - 0.06), Vec3::new(0.52, 1.18, zf + 0.06), coat, coat_cell, true);
            b.part(Vec3::new(0.4, 1.05, zf - 0.065), Vec3::new(0.46, 1.19, zf + 0.065), stripe, stripe_cell, false);
        } else {
            b.part(Vec3::new(-0.065, 0.78, z - 0.06), Vec3::new(0.065, top - 0.02, z + 0.06), coat, coat_cell, true);
            b.part(Vec3::new(-0.07, 0.84, z - 0.065), Vec3::new(0.07, 0.9, z + 0.065), stripe, stripe_cell, false);
        }
    }
    if pose == FirePose::Nozzle {
        // The branch pipe in the hands, and the hose falling away behind.
        b.part(Vec3::new(0.3, 1.08, -0.045), Vec3::new(0.95, 1.17, 0.045), silver, 0, true);
        b.part(Vec3::new(-0.1, 1.06, -0.06), Vec3::new(0.32, 1.16, 0.06), [0.75, 0.55, 0.12], 0, true);
    }
    // Collar up, head, then the helmet with its brim and a reflective band.
    b.part(Vec3::new(-td / 2.0, top - 0.04, -0.17), Vec3::new(td / 2.0 - 0.04, top + 0.08, 0.17), coat, coat_cell, false);
    let hb = top + 0.03;
    b.part(Vec3::new(-0.12, hb, -0.11), Vec3::new(0.12, hb + 0.25, 0.11), skin, 0, false);
    b.part(Vec3::new(-0.2, hb + 0.17, -0.18), Vec3::new(0.22, hb + 0.2, 0.18), helmet, 0, true);
    b.part(Vec3::new(-0.15, hb + 0.2, -0.14), Vec3::new(0.15, hb + 0.37, 0.14), helmet, 0, true);
    b.part(Vec3::new(-0.155, hb + 0.24, -0.145), Vec3::new(0.155, hb + 0.28, 0.145), silver, stripe_cell, false);
    m
}

// ── the figure cache ───────────────────────────────────────────────────────

/// Figures (meshes, or anything a renderer makes of one) by look, bounded
/// at `cap`. A look used this frame (`touch`, `get_or_build`) is never
/// evicted; when the cache is full of looks on screen, a newcomer borrows a
/// cached figure of the same silhouette (or gets none) instead of growing it.
pub struct FigureCache<H> {
    cap: usize,
    /// In key order (not a `HashMap`'s), so which look is evicted among
    /// equally old ones and which figure a newcomer borrows are the same
    /// every run.
    entries: std::collections::BTreeMap<PersonKey, (H, u64)>,
    frame: u64,
    /// What each silhouette borrowed this frame, so borrowers agree.
    lent: std::collections::BTreeMap<(u8, u8, u8, bool, bool, u8), Option<H>>,
    /// Figures built over the run.
    pub built: usize,
    /// Figures borrowed this frame.
    pub borrowed: usize,
}

impl<H: Clone> FigureCache<H> {
    pub fn new(cap: usize) -> Self {
        Self { cap: cap.max(1), entries: Default::default(), frame: 0, lent: Default::default(), built: 0, borrowed: 0 }
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.borrowed = 0;
        self.lent.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, key: &PersonKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Mark a look as on screen this frame.
    pub fn touch(&mut self, key: &PersonKey) {
        if let Some(e) = self.entries.get_mut(key) {
            e.1 = self.frame;
        }
    }

    pub fn get_or_build(&mut self, key: PersonKey, build: impl FnOnce(&PersonKey) -> H) -> Option<H> {
        let frame = self.frame;
        if let Some(e) = self.entries.get_mut(&key) {
            e.1 = frame;
            return Some(e.0.clone());
        }
        if self.entries.len() >= self.cap {
            let oldest = self.entries.iter().filter(|(_, e)| e.1 < frame).min_by_key(|(_, e)| e.1).map(|(k, _)| *k);
            match oldest {
                Some(k) => {
                    self.entries.remove(&k);
                }
                None => {
                    self.borrowed += 1;
                    let shape = shape_of(&key);
                    let entries = &self.entries;
                    return self.lent.entry(shape).or_insert_with(|| entries.iter().find(|(k, _)| shape_of(k) == shape).map(|(_, e)| e.0.clone())).clone();
                }
            }
        }
        let h = build(&key);
        self.built += 1;
        self.entries.insert(key, (h.clone(), frame));
        Some(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_key() -> impl Iterator<Item = PersonKey> {
        let mut out = Vec::new();
        for style in 0..HairStyle::ALL.len() as u8 {
            for outfit in 0..Outfit::ALL.len() as u8 {
                for build in 0..3 {
                    for (child, ride, tourist) in [(false, false, false), (true, false, false), (false, true, false), (true, true, false), (false, false, true)] {
                        for (hair, fav, wear) in [(0u8, [0u8, 8u8], 0u8), (7, [10, 11], 1), (8 + 4, [5, 3], 2)] {
                            out.push(PersonKey { style, outfit, build, child, skin: (style * 3 + outfit) % SKIN_STEPS, hair, fav, wear, ride, tourist, sit: 0 });
                            if !ride && !tourist {
                                for sit in [SIT_BENCH, SIT_GROUND] {
                                    out.push(PersonKey { style, outfit, build, child, skin: (style * 3 + outfit) % SKIN_STEPS, hair, fav, wear, ride, tourist, sit });
                                }
                            }
                        }
                    }
                }
            }
        }
        out.into_iter()
    }

    #[test]
    fn figures_are_finite_and_within_budget() {
        for k in every_key() {
            let m = person_mesh(&k);
            let tris = m.indices.len() / 3;
            let budget = if k.ride { MAX_RIDER_TRIS } else if k.tourist { MAX_TOURIST_TRIS } else if k.sit != 0 { MAX_SEATED_TRIS } else { MAX_TRIS };
            assert!(tris > 20 && tris <= budget, "{k:?}: {tris} triangles");
            assert_eq!(m.uvs.len(), m.positions.len());
            assert_eq!(m.colors.len(), m.positions.len());
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
            for p in &m.positions {
                assert!(p.iter().all(|v| v.is_finite()));
                assert!(p[1] >= -1e-4 && p[1] < 2.2, "{k:?}: {p:?}");
            }
            assert!(m.colors.iter().flatten().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0));
            for uv in &m.uvs {
                assert!(uv[0] > 0.0 && uv[0] < 1.0);
            }
        }
    }

    /// [K18] Seated figures sit at their seat with their feet on the
    /// ground, full size (not a standing figure squashed into the ground).
    #[test]
    fn seated_figures_sit_on_their_seat_above_the_ground() {
        for k in every_key().filter(|k| k.sit != 0) {
            let m = person_mesh(&k);
            let ys: Vec<f32> = m.positions.iter().map(|p| p[1]).collect();
            let (lo, hi) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
            assert!(lo >= -0.02 && lo < 0.1, "{k:?}: feet at {lo}");
            let seat = if k.sit == SIT_BENCH { BENCH_SEAT } else { 0.0 };
            let want = seat + if k.child { 0.95 } else { 0.9 };
            assert!(hi > want * 0.85 && hi < want + 0.35, "{k:?}: head top {hi} (seat {seat})");
        }
    }

    #[test]
    fn bike_wheels_are_round() {
        // [round-wheels] A box wheel touched the ground along its whole
        // width; a round one only right under the axle.
        for k in every_key().filter(|k| k.ride) {
            let m = person_mesh(&k);
            for p in m.positions.iter().filter(|p| p[1] < 0.02) {
                let off = (p[0].abs() - 0.55).abs();
                assert!(off < 0.07, "{k:?}: ground contact at {p:?}");
            }
            let rim = m.positions.iter().filter(|p| ((p[0].abs() - 0.55).hypot(p[1] - 0.34) - 0.34).abs() < 1e-3).count();
            assert!(rim >= 2 * WHEEL_SIDES * 4, "{k:?}: only {rim} rim vertices");
        }
    }

    #[test]
    fn firefighters_are_finite_lit_and_within_budget() {
        for skin in 0..SKIN_STEPS {
            for pose in FirePose::ALL {
                let m = firefighter_mesh(skin, pose);
                let tris = m.indices.len() / 3;
                assert!(tris > 40 && tris <= MAX_FIREFIGHTER_TRIS, "{pose:?}: {tris} triangles");
                assert_eq!(m.uvs.len(), m.positions.len());
                assert!(m.positions.iter().flatten().all(|v| v.is_finite()));
                let top = m.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
                assert!((1.8..2.0).contains(&top), "{top}");
                let lit = m.uvs.iter().filter(|uv| (uv[0] * EMISSIVE_CELLS as f32) as usize == bright_cell(5)).count();
                assert!(lit > 0, "the stripes glow");
            }
        }
    }

    #[test]
    fn the_figure_matches_the_old_one_in_size() {
        let k = PersonKey { style: 0, outfit: 0, build: 1, child: false, skin: 3, hair: 0, fav: [0, 1], wear: 1, ride: false, tourist: false, sit: 0 };
        let m = person_mesh(&k);
        let top = m.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
        // The old box figure stood 1.75 m (plus the 0.16 m lift).
        assert!((1.7..1.9).contains(&top), "{top}");
    }

    #[test]
    fn the_trim_is_lit_and_skin_is_not() {
        let pal = emissive_palette();
        assert_eq!(pal.len(), EMISSIVE_CELLS);
        assert_eq!(pal[0], [0.0; 3]);
        assert!(pal.iter().flatten().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0));
        let k = PersonKey { style: 0, outfit: 1, build: 1, child: false, skin: 3, hair: 0, fav: [8, 0], wear: 1, ride: false, tourist: false, sit: 0 };
        let m = person_mesh(&k);
        let lit = m.uvs.iter().filter(|uv| (uv[0] * EMISSIVE_CELLS as f32) as usize == bright_cell(0)).count();
        let dark = m.uvs.iter().filter(|uv| (uv[0] * EMISSIVE_CELLS as f32) as usize == 0).count();
        assert!(lit > 0 && dark > 0);
    }

    #[test]
    fn a_city_needs_a_bounded_number_of_figures() {
        // 3,000 people of all ages and classes: how many distinct meshes.
        let mut keys = std::collections::HashSet::new();
        let mut shapes = std::collections::HashSet::new();
        for i in 0..3000u64 {
            let seed = i.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xABCD;
            let age = (i * 37 % 88) as u8;
            let tier = Tier::ALL[(i % 3) as usize];
            let look = Appearance::of_seed(seed, age, tier);
            let k = person_key(&look, age, tier, i % 10 == 0);
            shapes.insert(shape_of(&k));
            keys.insert(k);
        }
        println!("3000 people: {} distinct figures, {} silhouettes", keys.len(), shapes.len());
        assert!(shapes.len() <= HairStyle::ALL.len() * Outfit::ALL.len() * 3 * 4);
        assert!(keys.len() <= 3000);
    }

    #[test]
    fn the_figure_cache_stays_bounded() {
        let key = |i: u32| PersonKey { style: (i % 7) as u8, outfit: (i % 6) as u8, build: (i % 3) as u8, child: false, skin: ((i / 484) % 10) as u8, hair: (i % 8) as u8, fav: [(i % 22) as u8, ((i / 22) % 22) as u8], wear: 1, ride: false, tourist: false, sit: 0 };
        let cap = 64;
        let mut c: FigureCache<u32> = FigureCache::new(cap);
        let mut n = 0u32;
        for _ in 0..40 {
            c.begin_frame();
            for _ in 0..30 {
                n += 1;
                assert!(c.get_or_build(key(n), |_| n).is_some());
                assert!(c.len() <= cap);
            }
        }
        assert_eq!(c.len(), cap);
        assert_eq!(c.built, 1200);
        // Looks on screen this frame are never evicted: with all of them in
        // use a newcomer borrows instead of building.
        c.begin_frame();
        let on_screen: Vec<PersonKey> = c.entries.keys().copied().collect();
        for k in &on_screen {
            c.touch(k);
        }
        let built = c.built;
        let _ = c.get_or_build(key(100_000), |_| 0);
        assert_eq!(c.built, built, "nothing built while every figure is in use");
        assert_eq!(c.borrowed, 1);
        assert!(on_screen.iter().all(|k| c.contains(k)));
        // Next frame the oldest look makes room again.
        c.begin_frame();
        assert_eq!(c.get_or_build(key(100_000), |_| 7), Some(7));
        assert_eq!(c.len(), cap);
    }
}
