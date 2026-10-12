//! What a citizen looks like: skin, hair, build, the two colours they like
//! and the clothes they wear in them. Chosen once, on arrival, and saved, so
//! the person in the pane is the figure on the street every time.
//!
//! Families resemble each other: children take their skin tone and hair
//! from their parents (a mix, with a little variation), a grandparent passes
//! theirs to a parent. Couples and housemates are unrelated. Everything is
//! drawn from the citizens' seeds, never from the world's generator, so a
//! save from before looks existed gets exactly the looks it would have had.

use crate::buildings::Tier;
use crate::citizens::{ADULT_AGE, RETIRE_AGE};
use crate::SimRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

/// A colour someone can like: a few neon shades that suit the district at
/// night and the everyday ones most wardrobes are made of.
pub struct Swatch {
    pub name: &'static str,
    /// sRGB, 0..255.
    pub srgb: [u8; 3],
    pub neon: bool,
}

const fn sw(name: &'static str, r: u8, g: u8, b: u8, neon: bool) -> Swatch {
    Swatch { name, srgb: [r, g, b], neon }
}

/// Every favourite colour, addressed by `Appearance::fav`. Append only:
/// saves store the index.
pub const COLORS: [Swatch; 22] = [
    sw("teal", 0, 190, 180, true),
    sw("magenta", 225, 0, 160, true),
    sw("cyan", 0, 215, 255, true),
    sw("amber", 255, 168, 0, true),
    sw("violet", 145, 60, 255, true),
    sw("acid green", 160, 255, 40, true),
    sw("hot pink", 255, 60, 150, true),
    sw("electric blue", 40, 90, 255, true),
    sw("black", 22, 22, 26, false),
    sw("charcoal", 55, 57, 62, false),
    sw("white", 232, 232, 226, false),
    sw("navy", 28, 38, 85, false),
    sw("olive", 95, 100, 50, false),
    sw("burgundy", 112, 22, 42, false),
    sw("tan", 190, 155, 110, false),
    sw("denim", 70, 100, 140, false),
    sw("grey", 130, 132, 136, false),
    sw("brown", 100, 65, 40, false),
    sw("red", 190, 30, 35, false),
    sw("mustard", 200, 160, 40, false),
    sw("forest green", 30, 85, 50, false),
    sw("cream", 235, 225, 200, false),
];

/// sRGB 0..255 to linear 0..1.
pub fn linear(c: [u8; 3]) -> [f32; 3] {
    let f = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(c[0]), f(c[1]), f(c[2])]
}

/// Skin from the palest (`0`) to the deepest (`1`), in sRGB.
pub fn skin_srgb(t: f32) -> [u8; 3] {
    const STOPS: [(f32, [f32; 3]); 7] = [
        (0.0, [250.0, 222.0, 200.0]),
        (0.18, [238.0, 196.0, 164.0]),
        (0.36, [222.0, 172.0, 130.0]),
        (0.52, [196.0, 136.0, 96.0]),
        (0.68, [160.0, 104.0, 70.0]),
        (0.84, [116.0, 74.0, 50.0]),
        (1.0, [72.0, 46.0, 34.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    let i = STOPS.iter().position(|s| s.0 >= t).unwrap_or(STOPS.len() - 1).max(1);
    let (a, b) = (STOPS[i - 1], STOPS[i]);
    let k = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    let c = |j: usize| (a.1[j] + (b.1[j] - a.1[j]) * k).round() as u8;
    [c(0), c(1), c(2)]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HairColor {
    Black,
    DarkBrown,
    Brown,
    Auburn,
    Ginger,
    Blonde,
}

impl HairColor {
    pub const ALL: [HairColor; 6] = [HairColor::Black, HairColor::DarkBrown, HairColor::Brown, HairColor::Auburn, HairColor::Ginger, HairColor::Blonde];

    pub fn label(self) -> &'static str {
        match self {
            HairColor::Black => "black",
            HairColor::DarkBrown => "dark brown",
            HairColor::Brown => "brown",
            HairColor::Auburn => "auburn",
            HairColor::Ginger => "ginger",
            HairColor::Blonde => "blonde",
        }
    }

    pub fn srgb(self) -> [u8; 3] {
        match self {
            HairColor::Black => [26, 22, 20],
            HairColor::DarkBrown => [58, 38, 26],
            HairColor::Brown => [108, 72, 44],
            HairColor::Auburn => [128, 56, 32],
            HairColor::Ginger => [188, 98, 46],
            HairColor::Blonde => [218, 186, 118],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HairTexture {
    Straight,
    Wavy,
    Curly,
    Coily,
}

impl HairTexture {
    pub fn label(self) -> &'static str {
        match self {
            HairTexture::Straight => "straight",
            HairTexture::Wavy => "wavy",
            HairTexture::Curly => "curly",
            HairTexture::Coily => "coily",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HairStyle {
    Short,
    Long,
    Bun,
    Buzz,
    Bald,
    Mohawk,
    Afro,
}

impl HairStyle {
    pub const ALL: [HairStyle; 7] = [HairStyle::Short, HairStyle::Long, HairStyle::Bun, HairStyle::Buzz, HairStyle::Bald, HairStyle::Mohawk, HairStyle::Afro];

    pub fn index(self) -> u8 {
        HairStyle::ALL.iter().position(|s| *s == self).unwrap_or(0) as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Build {
    Slim,
    Average,
    Broad,
}

impl Build {
    pub fn label(self) -> &'static str {
        match self {
            Build::Slim => "slim",
            Build::Average => "medium build",
            Build::Broad => "broad",
        }
    }
}

/// The cut of the clothes; the colours are the favourite two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Outfit {
    Tee,
    Hoodie,
    WorkJacket,
    Suit,
    Dress,
    Coat,
}

impl Outfit {
    pub const ALL: [Outfit; 6] = [Outfit::Tee, Outfit::Hoodie, Outfit::WorkJacket, Outfit::Suit, Outfit::Dress, Outfit::Coat];

    pub fn index(self) -> u8 {
        Outfit::ALL.iter().position(|s| *s == self).unwrap_or(0) as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Outfit::Tee => "T-shirt and trousers",
            Outfit::Hoodie => "hoodie",
            Outfit::WorkJacket => "work jacket",
            Outfit::Suit => "suit",
            Outfit::Dress => "dress",
            Outfit::Coat => "long coat",
        }
    }
}

/// The inherited part of a look: what a child gets from their parents.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Genes {
    /// `0` palest to `1` deepest (`skin_srgb`).
    pub skin: f32,
    pub hair: HairColor,
    pub texture: HairTexture,
}

/// A citizen's look. Age-dependent details (greying, a child's size) are
/// worked out from the age when drawn (`stature`, `shown_hair`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub genes: Genes,
    pub style: HairStyle,
    /// Dyed hair: an index into `COLORS` (always a neon one).
    pub dye: Option<u8>,
    /// Age the hair turns grey (white eighteen years later).
    pub grey_at: u8,
    /// Grown-up height relative to average, about `0.9..1.1`.
    pub height: f32,
    pub build: Build,
    /// The two favourite colours, indices into `COLORS`: the main one and
    /// the accent.
    pub fav: [u8; 2],
    pub outfit: Outfit,
}

/// What the hair looks like right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShownHair {
    Natural(HairColor),
    Grey,
    White,
    Dyed(u8),
}

impl ShownHair {
    pub fn srgb(self) -> [u8; 3] {
        match self {
            ShownHair::Natural(c) => c.srgb(),
            ShownHair::Grey => [150, 150, 152],
            ShownHair::White => [222, 222, 218],
            ShownHair::Dyed(i) => COLORS[(i as usize).min(COLORS.len() - 1)].srgb,
        }
    }

    pub fn label(self) -> String {
        match self {
            ShownHair::Natural(c) => c.label().to_string(),
            ShownHair::Grey => "grey".into(),
            ShownHair::White => "white".into(),
            ShownHair::Dyed(i) => format!("dyed {}", COLORS[(i as usize).min(COLORS.len() - 1)].name),
        }
    }
}

const GENE_SALT: u64 = 0x5EED_0F_6E4E5;
const LOOK_SALT: u64 = 0x100C_5A1E_D00D;
const KIN_SALT: u64 = 0x0A11_E4ED_4A57;

/// The five personality draws (`nd_minds::Personality::from_seed`):
/// sociability, ambition, hedonism, caution, curiosity.
use crate::citizens::traits_of as traits;

fn pick<T: Copy>(rng: &mut SimRng, items: &[(T, f32)]) -> T {
    let total: f32 = items.iter().map(|i| i.1.max(0.0)).sum();
    let mut r = rng.random::<f32>() * total;
    for &(t, w) in items {
        let w = w.max(0.0);
        if r < w {
            return t;
        }
        r -= w;
    }
    items.last().expect("something to pick").0
}

impl Genes {
    /// Someone's own genes, unrelated to anyone in the city.
    pub fn of_seed(seed: u64) -> Self {
        let mut rng = SimRng::seed_from_u64(seed ^ GENE_SALT);
        let skin: f32 = rng.random();
        use HairColor::*;
        let hair = if skin > 0.55 {
            pick(&mut rng, &[(Black, 0.72), (DarkBrown, 0.26), (Auburn, 0.02)])
        } else if skin > 0.3 {
            pick(&mut rng, &[(Black, 0.38), (DarkBrown, 0.38), (Brown, 0.18), (Auburn, 0.04), (Blonde, 0.02)])
        } else {
            pick(&mut rng, &[(Black, 0.08), (DarkBrown, 0.22), (Brown, 0.28), (Blonde, 0.26), (Auburn, 0.08), (Ginger, 0.08)])
        };
        use HairTexture::*;
        let texture = if skin > 0.6 { pick(&mut rng, &[(Coily, 0.55), (Curly, 0.3), (Wavy, 0.1), (Straight, 0.05)]) } else { pick(&mut rng, &[(Straight, 0.45), (Wavy, 0.35), (Curly, 0.15), (Coily, 0.05)]) };
        Genes { skin, hair, texture }
    }

    /// A child of `a` and `b`: skin between theirs, hair from one of them.
    pub fn child_of(a: Genes, b: Genes, seed: u64) -> Self {
        let mut rng = SimRng::seed_from_u64(seed ^ KIN_SALT);
        let k = rng.random_range(0.3..0.7f32);
        let skin = (a.skin + (b.skin - a.skin) * k + rng.random_range(-0.05..0.05f32)).clamp(0.0, 1.0);
        // Darker hair wins more often.
        let dark = |h: HairColor| matches!(h, HairColor::Black | HairColor::DarkBrown);
        let p_a = match (dark(a.hair), dark(b.hair)) {
            (true, false) => 0.75,
            (false, true) => 0.25,
            _ => 0.5,
        };
        let hair = if rng.random_bool(p_a) { a.hair } else { b.hair };
        let texture = if rng.random_bool(0.5) { a.texture } else { b.texture };
        Genes { skin, hair, texture }
    }

    /// A close relative of `g` (a parent's parent): much the same.
    pub fn kin_of(g: Genes, seed: u64) -> Self {
        let mut rng = SimRng::seed_from_u64(seed ^ KIN_SALT);
        Genes { skin: (g.skin + rng.random_range(-0.06..0.06f32)).clamp(0.0, 1.0), ..g }
    }
}

impl Appearance {
    /// A look for someone with no relatives in town.
    pub fn of_seed(seed: u64, age: u8, class: Tier) -> Self {
        Self::with_genes(seed, age, class, Genes::of_seed(seed))
    }

    /// The rest of a look (hair cut, build, taste, clothes) for someone
    /// with these genes, by age, class and personality.
    pub fn with_genes(seed: u64, age: u8, class: Tier, genes: Genes) -> Self {
        let t = traits(seed);
        let (ambition, hedonism, caution) = (t[1], t[2], t[3]);
        let mut rng = SimRng::seed_from_u64(seed ^ LOOK_SALT);
        let child = age < ADULT_AGE;
        let old = age >= RETIRE_AGE;
        let young = (16..36).contains(&age);
        let curly = matches!(genes.texture, HairTexture::Curly | HairTexture::Coily);
        let coily = genes.texture == HairTexture::Coily;
        use HairStyle::*;
        let style = if child {
            pick(&mut rng, &[(Short, 0.4), (Long, if coily { 0.12 } else { 0.3 }), (Bun, 0.14), (Buzz, 0.12), (Afro, if curly { 0.25 } else { 0.0 })])
        } else {
            let bald = if age > 30 { ((age as f32 - 30.0) / 40.0).min(1.0) * 0.28 } else { 0.0 };
            let mohawk = if young && hedonism > 0.65 { 0.3 } else { 0.0 };
            pick(
                &mut rng,
                &[
                    (Short, if old { 0.4 } else { 0.3 }),
                    (Long, if old { 0.08 } else { 0.22 }),
                    (Bun, 0.12),
                    (Buzz, if old { 0.08 } else { 0.12 }),
                    (Bald, bald),
                    (Mohawk, mohawk),
                    (Afro, if curly && !old { 0.18 } else { 0.0 }),
                ],
            )
        };
        let neon: Vec<u8> = (0..COLORS.len() as u8).filter(|&i| COLORS[i as usize].neon).collect();
        let dye_chance = match style {
            Mohawk => 0.7,
            Bald | Buzz => 0.0,
            _ if (ADULT_AGE..40).contains(&age) && hedonism > 0.6 => 0.15 + (hedonism - 0.6) * 1.2,
            _ => 0.0,
        };
        let dye = rng.random_bool(dye_chance.clamp(0.0, 1.0) as f64).then(|| neon[rng.random_range(0..neon.len())]);
        let grey_at = rng.random_range(44..74u8);
        let height = 0.9 + rng.random_range(0.0..0.1f32) + rng.random_range(0.0..0.1f32);
        let build = pick(&mut rng, &[(Build::Slim, 0.3), (Build::Average, 0.45), (Build::Broad, 0.25)]);
        // Taste: the young and the pleasure-seeking like neon; the careful
        // and the old mostly do not.
        let neon_weight = (0.25 + hedonism * 0.6 + if young { 0.25 } else { 0.0 } - caution * 0.2 - if old { 0.2 } else { 0.0 }).max(0.05);
        let weights: Vec<(u8, f32)> = (0..COLORS.len() as u8).map(|i| (i, if COLORS[i as usize].neon { neon_weight / 8.0 * 3.0 } else { 1.0 / 14.0 * 2.0 })).collect();
        let first = pick(&mut rng, &weights);
        let rest: Vec<(u8, f32)> = weights.iter().copied().filter(|w| w.0 != first).collect();
        let second = pick(&mut rng, &rest);
        use Outfit::*;
        let outfit = if child {
            pick(&mut rng, &[(Tee, 0.4), (Hoodie, 0.4), (Dress, 0.2)])
        } else if old {
            pick(&mut rng, &[(Coat, 0.45), (Dress, 0.2), (Suit, if class == Tier::High { 0.3 } else { 0.1 }), (Tee, 0.15)])
        } else {
            let street = if young { hedonism * 0.3 } else { 0.0 };
            match class {
                Tier::Low => pick(&mut rng, &[(WorkJacket, 0.35), (Hoodie, 0.28 + street), (Tee, 0.2), (Coat, 0.1), (Dress, 0.07)]),
                Tier::Mid => pick(&mut rng, &[(Coat, 0.25), (Tee, 0.2), (Hoodie, 0.18 + street), (Dress, 0.15), (WorkJacket, 0.1), (Suit, 0.05 + ambition * 0.15)]),
                Tier::High => pick(&mut rng, &[(Suit, 0.25 + ambition * 0.3), (Coat, 0.3), (Dress, 0.2), (Tee, 0.08), (Hoodie, street * 0.5)]),
            }
        };
        Appearance { genes, style, dye, grey_at, height, build, fav: [first, second], outfit }
    }

    /// Overall size at this age, `1` an average adult.
    pub fn stature(&self, age: u8) -> f32 {
        if age < ADULT_AGE {
            let k = (age as f32 / (ADULT_AGE - 1) as f32).clamp(0.0, 1.0).powf(0.85);
            0.42 + (self.height - 0.42) * k
        } else if age >= RETIRE_AGE + 8 {
            self.height * 0.97
        } else {
            self.height
        }
    }

    pub fn shown_hair(&self, age: u8) -> ShownHair {
        if let Some(d) = self.dye {
            return ShownHair::Dyed(d);
        }
        if age >= self.grey_at.saturating_add(18) {
            ShownHair::White
        } else if age >= self.grey_at {
            ShownHair::Grey
        } else {
            ShownHair::Natural(self.genes.hair)
        }
    }

    pub fn skin_label(&self) -> &'static str {
        match self.genes.skin {
            s if s < 0.15 => "pale",
            s if s < 0.33 => "fair",
            s if s < 0.5 => "olive",
            s if s < 0.68 => "light brown",
            s if s < 0.84 => "brown",
            _ => "dark",
        }
    }

    pub fn hair_label(&self, age: u8) -> String {
        let colour = self.shown_hair(age).label();
        match self.style {
            HairStyle::Bald => "bald".into(),
            HairStyle::Short => format!("short {} {colour} hair", self.genes.texture.label()),
            HairStyle::Long => format!("long {} {colour} hair", self.genes.texture.label()),
            HairStyle::Bun => format!("{colour} hair in a bun"),
            HairStyle::Buzz => format!("{} buzz cut", crate::names::a(colour)),
            HairStyle::Mohawk => format!("{} mohawk", crate::names::a(colour)),
            HairStyle::Afro => format!("{} afro", crate::names::a(colour)),
        }
    }

    /// "dark skin, short curly black hair, wears teal and amber, worn hoodie".
    pub fn describe(&self, age: u8, class: Tier) -> String {
        let [a, b] = self.fav.map(|i| COLORS[(i as usize).min(COLORS.len() - 1)].name);
        let wear = match (class, self.outfit) {
            (_, _) if age < ADULT_AGE => "",
            (Tier::Low, _) => "worn ",
            (Tier::High, Outfit::Suit) => "sharp ",
            (Tier::High, _) => "tailored ",
            _ => "",
        };
        let size = if age < ADULT_AGE {
            "child"
        } else if self.height > 1.06 {
            "tall"
        } else if self.height < 0.94 {
            "short"
        } else {
            "average height"
        };
        format!("{} skin, {}, {size}, {}, wears {a} and {b}, {wear}{}", self.skin_label(), self.hair_label(age), self.build.label(), self.outfit.label())
    }
}

/// Looks for the members of one household, in order, by `(seed, age)`.
/// With children in the house, the first two working-age adults are their
/// parents (one parent alone gets an absent partner from their own seed),
/// other adults are the first parent's parents, and the children inherit.
/// Without children nobody is related: couples and housemates.
pub fn family_looks(members: &[(u64, u8)], class: Tier) -> Vec<Appearance> {
    let has_children = members.iter().any(|m| m.1 < ADULT_AGE) && members.iter().any(|m| m.1 >= ADULT_AGE);
    if !has_children {
        return members.iter().map(|&(seed, age)| Appearance::of_seed(seed, age, class)).collect();
    }
    let mut parents: Vec<usize> = (0..members.len()).filter(|&i| (ADULT_AGE..RETIRE_AGE).contains(&members[i].1)).take(2).collect();
    if parents.is_empty() {
        parents = (0..members.len()).filter(|&i| members[i].1 >= ADULT_AGE).take(2).collect();
    }
    let p0 = Genes::of_seed(members[parents[0]].0);
    let p1 = match parents.get(1) {
        Some(&i) => Genes::of_seed(members[i].0),
        None => Genes::of_seed(members[parents[0]].0 ^ KIN_SALT.rotate_left(17)),
    };
    members
        .iter()
        .enumerate()
        .map(|(i, &(seed, age))| {
            let genes = if i == parents[0] {
                p0
            } else if parents.get(1) == Some(&i) {
                p1
            } else if age < ADULT_AGE {
                Genes::child_of(p0, p1, seed)
            } else {
                Genes::kin_of(p0, seed)
            };
            Appearance::with_genes(seed, age, class, genes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_are_deterministic_from_the_seed() {
        for seed in [1u64, 42, 9_999_999, u64::MAX] {
            assert_eq!(Appearance::of_seed(seed, 30, Tier::Mid), Appearance::of_seed(seed, 30, Tier::Mid));
            let fam = [(seed, 40), (seed ^ 7, 38), (seed ^ 9, 8), (seed ^ 11, 5)];
            assert_eq!(family_looks(&fam, Tier::Low), family_looks(&fam, Tier::Low));
        }
    }

    #[test]
    fn looks_are_varied_and_sensible() {
        let looks: Vec<(u8, Appearance)> = (0..2000u64).map(|s| {
            let age = (s % 90) as u8;
            (age, Appearance::of_seed(s.wrapping_mul(0x9E37_79B9_7F4A_7C15), age, Tier::ALL[(s % 3) as usize]))
        }).collect();
        for style in HairStyle::ALL {
            assert!(looks.iter().any(|l| l.1.style == style), "{style:?} never appears");
        }
        for o in Outfit::ALL {
            assert!(looks.iter().any(|l| l.1.outfit == o), "{o:?} never appears");
        }
        assert!(looks.iter().any(|l| l.1.genes.skin < 0.15) && looks.iter().any(|l| l.1.genes.skin > 0.85));
        for (age, l) in &looks {
            assert_ne!(l.fav[0], l.fav[1]);
            assert!((l.fav[0] as usize) < COLORS.len() && (l.fav[1] as usize) < COLORS.len());
            if *age < ADULT_AGE {
                assert!(l.style != HairStyle::Bald && l.style != HairStyle::Mohawk && l.dye.is_none(), "{age}: {l:?}");
                assert!(l.stature(*age) < l.height + 1e-3);
                assert_ne!(l.shown_hair(*age), ShownHair::White);
            }
            if l.style == HairStyle::Mohawk {
                assert!(*age < 36);
            }
            assert!(l.stature(*age) > 0.4 && l.stature(*age) < 1.15);
        }
        let old_grey = looks.iter().filter(|(a, l)| *a >= 85 && l.dye.is_none()).all(|(a, l)| matches!(l.shown_hair(*a), ShownHair::Grey | ShownHair::White));
        assert!(old_grey, "everyone over 85 has gone grey");
        let d = Appearance::of_seed(5, 30, Tier::Low).describe(30, Tier::Low);
        assert!(d.contains("skin") && d.contains("wears"), "{d}");
    }

    #[test]
    fn siblings_resemble_each_other_more_than_strangers() {
        let mut sib_skin = 0.0;
        let mut sib_hair = 0;
        let mut str_skin = 0.0;
        let mut str_hair = 0;
        let n = 400;
        for f in 0..n as u64 {
            let s = f.wrapping_mul(0x2545_F491_4F6C_DD1D);
            let fam = family_looks(&[(s, 40), (s ^ 1, 41), (s ^ 2, 9), (s ^ 3, 6)], Tier::Mid);
            sib_skin += (fam[2].genes.skin - fam[3].genes.skin).abs();
            sib_hair += usize::from(fam[2].genes.hair == fam[3].genes.hair);
            let a = Appearance::of_seed(s ^ 0x55, 9, Tier::Mid);
            let b = Appearance::of_seed(s ^ 0x77, 6, Tier::Mid);
            str_skin += (a.genes.skin - b.genes.skin).abs();
            str_hair += usize::from(a.genes.hair == b.genes.hair);
            // A child's skin lies near the span of the parents'.
            let (lo, hi) = (fam[0].genes.skin.min(fam[1].genes.skin), fam[0].genes.skin.max(fam[1].genes.skin));
            assert!(fam[2].genes.skin >= lo - 0.06 && fam[2].genes.skin <= hi + 0.06);
            assert!(fam[2].genes.hair == fam[0].genes.hair || fam[2].genes.hair == fam[1].genes.hair);
        }
        let (sib_skin, str_skin) = (sib_skin / n as f32, str_skin / n as f32);
        assert!(sib_skin < str_skin * 0.5, "siblings {sib_skin} vs strangers {str_skin}");
        assert!(sib_hair > str_hair, "siblings share hair {sib_hair} vs strangers {str_hair}");
    }

    #[test]
    fn couples_and_housemates_are_unrelated() {
        let a = family_looks(&[(10, 30), (11, 31)], Tier::Mid);
        assert_eq!(a[0], Appearance::of_seed(10, 30, Tier::Mid));
        assert_eq!(a[1], Appearance::of_seed(11, 31, Tier::Mid));
        // A grandparent passes their colouring to the first parent.
        let f = family_looks(&[(20, 40), (21, 38), (22, 7), (23, 75)], Tier::Mid);
        assert_eq!(f[3].genes.hair, f[0].genes.hair);
        assert!((f[3].genes.skin - f[0].genes.skin).abs() <= 0.061);
        // A single parent's child still inherits.
        let s = family_looks(&[(30, 33), (31, 4)], Tier::Low);
        assert!((s[1].genes.skin - s[0].genes.skin).abs() < 0.8);
    }

    #[test]
    fn appearance_round_trips_through_ron() {
        let a = Appearance::of_seed(77, 22, Tier::High);
        let back: Appearance = ron::from_str(&ron::to_string(&a).unwrap()).unwrap();
        assert_eq!(a, back);
    }
}
