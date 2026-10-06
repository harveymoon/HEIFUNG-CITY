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
pub const MAX_RIDER_TRIS: usize = 190;
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
}

/// The silhouette part of a key: the same shape in other colours.
pub fn shape_of(k: &PersonKey) -> (u8, u8, u8, bool, bool) {
    (k.style, k.outfit, k.build, k.child, k.ride)
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
    }
}

/// [tourism] A visitor's figure: their look with a backpack and camera.
pub fn tourist_key(look: &Appearance, age: u8, class: Tier) -> PersonKey {
    PersonKey { tourist: true, ..person_key(look, age, class, false) }
}

// â”€â”€ the emissive palette â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ the figure â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

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

    if !k.ride {
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
        // The bike: wheels, frame and bars.
        let dark = [0.03, 0.03, 0.035];
        let steel = [0.35, 0.36, 0.4];
        for x in [-0.55f32, 0.55] {
            b.part(Vec3::new(x - 0.34, 0.0, -0.03), Vec3::new(x + 0.34, 0.68, 0.03), dark, 0, true);
        }
        b.part(Vec3::new(-0.55, 0.57, -0.03), Vec3::new(0.55, 0.63, 0.03), steel, 0, true);
        b.part(Vec3::new(0.32, 0.6, -0.22), Vec3::new(0.38, 1.3, 0.22), steel, 0, true);
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

// â”€â”€ the figure cache â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    lent: std::collections::BTreeMap<(u8, u8, u8, bool, bool), Option<H>>,
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
                            out.push(PersonKey { style, outfit, build, child, skin: (style * 3 + outfit) % SKIN_STEPS, hair, fav, wear, ride, tourist });
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
            let budget = if k.ride { MAX_RIDER_TRIS } else if k.tourist { MAX_TOURIST_TRIS } else { MAX_TRIS };
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

    #[test]
    fn the_figure_matches_the_old_one_in_size() {
        let k = PersonKey { style: 0, outfit: 0, build: 1, child: false, skin: 3, hair: 0, fav: [0, 1], wear: 1, ride: false, tourist: false };
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
        let k = PersonKey { style: 0, outfit: 1, build: 1, child: false, skin: 3, hair: 0, fav: [8, 0], wear: 1, ride: false, tourist: false };
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
        let key = |i: u32| PersonKey { style: (i % 7) as u8, outfit: (i % 6) as u8, build: (i % 3) as u8, child: false, skin: ((i / 484) % 10) as u8, hair: (i % 8) as u8, fav: [(i % 22) as u8, ((i / 22) % 22) as u8], wear: 1, ride: false, tourist: false };
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
