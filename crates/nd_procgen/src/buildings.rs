//! Building grammar. Takes a lot, land use, floors, tier, condition and a seed
//! and returns two meshes: a lit body and an unlit glow layer (windows, signs,
//! lamps, LED screens).
//!
//! Three very different architectures by tier:
//! - low: stacked shipping containers with walkways, tarps and ladders
//! - middle: brutalist concrete with cantilevered slabs, fins, recessed window
//!   bands and LED advert screens
//! - high: faceted crystal glass towers with lit floor bands, or walled
//!   fortress compounds for the rich
//!
//! Industrial lots get sheds with chimneys and pipes; civic lots get brutalist
//! monuments on pilotis; parks get trees and lamps.

use crate::mesh::MeshData;
use crate::palette::Palette;
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use nd_sim::{LandUse, Lot, Tier};
use rand::seq::IndexedRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LandUseRules {
    pub floor_height: f32,
    /// Inset of the footprint from the lot edges.
    pub inset: f32,
    pub window_spacing: f32,
    pub window_size: [f32; 2],
    pub sign_chance: f32,
    pub vertical_sign_chance: f32,
    pub roof_antenna_chance: f32,
    pub setback_chance: f32,
}

/// Look of a quality tier, applied on top of the land-use rules.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TierStyle {
    pub wall_colors: Vec<[f32; 3]>,
    pub roof_color: [f32; 3],
    pub trim_color: [f32; 3],
    /// Probability a window is lit at night.
    pub lit_chance: f32,
    pub window_colors: Vec<[f32; 3]>,
    /// Stains, patches and rust streaks per facade.
    pub grime: f32,
    /// Air conditioners, dishes, water tanks.
    pub clutter: f32,
    /// Glass facades get vertical mullions.
    pub mullions: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildingRules {
    pub residential: LandUseRules,
    pub commercial: LandUseRules,
    pub industrial: LandUseRules,
    pub entertainment: LandUseRules,
    pub civic: LandUseRules,
    pub park: LandUseRules,
    pub tiers: [TierStyle; 3],
    /// Chance a high-tier home is a walled compound instead of a tower.
    pub fortress_chance: f32,
    /// Chance a middle-tier residential building carries an LED advert.
    pub advert_chance_residential: f32,
}

impl BuildingRules {
    pub fn for_land_use(&self, lu: LandUse) -> &LandUseRules {
        match lu {
            LandUse::Residential => &self.residential,
            LandUse::Commercial => &self.commercial,
            LandUse::Industrial => &self.industrial,
            LandUse::Entertainment => &self.entertainment,
            LandUse::Civic => &self.civic,
            LandUse::Park => &self.park,
        }
    }

    pub fn style(&self, tier: Tier) -> &TierStyle {
        &self.tiers[tier.index()]
    }
}

impl Default for BuildingRules {
    fn default() -> Self {
        let base = LandUseRules {
            floor_height: 3.1,
            inset: 1.5,
            window_spacing: 2.6,
            window_size: [1.1, 1.4],
            sign_chance: 0.05,
            vertical_sign_chance: 0.0,
            roof_antenna_chance: 0.35,
            setback_chance: 0.3,
        };
        Self {
            residential: base.clone(),
            commercial: LandUseRules { floor_height: 3.5, window_spacing: 2.2, window_size: [1.5, 1.8], sign_chance: 0.6, vertical_sign_chance: 0.12, ..base.clone() },
            industrial: LandUseRules {
                floor_height: 5.0,
                inset: 2.5,
                window_spacing: 4.0,
                window_size: [2.5, 0.9],
                sign_chance: 0.3,
                roof_antenna_chance: 0.9,
                setback_chance: 0.0,
                ..base.clone()
            },
            entertainment: LandUseRules { floor_height: 3.6, sign_chance: 0.9, vertical_sign_chance: 0.5, ..base.clone() },
            civic: LandUseRules { floor_height: 4.2, inset: 3.0, window_spacing: 3.0, window_size: [1.6, 2.4], sign_chance: 0.3, setback_chance: 0.1, ..base.clone() },
            park: LandUseRules { inset: 2.0, sign_chance: 0.0, roof_antenna_chance: 0.0, ..base },
            tiers: [
                TierStyle {
                    wall_colors: vec![[0.30, 0.27, 0.23], [0.26, 0.25, 0.24], [0.33, 0.30, 0.26], [0.24, 0.23, 0.22]],
                    roof_color: [0.14, 0.13, 0.12],
                    trim_color: [0.20, 0.17, 0.14],
                    lit_chance: 0.35,
                    window_colors: vec![[1.3, 0.85, 0.45], [1.1, 0.95, 0.75], [0.7, 0.8, 0.9]],
                    grime: 0.9,
                    clutter: 0.8,
                    mullions: false,
                },
                TierStyle {
                    wall_colors: vec![[0.46, 0.45, 0.44], [0.40, 0.41, 0.43], [0.50, 0.47, 0.43], [0.36, 0.38, 0.40]],
                    roof_color: [0.22, 0.22, 0.23],
                    trim_color: [0.30, 0.30, 0.31],
                    lit_chance: 0.45,
                    window_colors: vec![[1.2, 1.1, 0.9], [1.0, 1.05, 1.15], [1.3, 0.95, 0.6]],
                    grime: 0.3,
                    clutter: 0.35,
                    mullions: false,
                },
                TierStyle {
                    wall_colors: vec![[0.30, 0.36, 0.44], [0.26, 0.32, 0.40], [0.36, 0.40, 0.46]],
                    roof_color: [0.26, 0.28, 0.32],
                    trim_color: [0.62, 0.64, 0.68],
                    lit_chance: 0.55,
                    window_colors: vec![[1.0, 1.15, 1.4], [1.2, 1.2, 1.25], [0.9, 1.0, 1.3]],
                    grime: 0.0,
                    clutter: 0.1,
                    mullions: true,
                },
            ],
            fortress_chance: 0.4,
            advert_chance_residential: 0.45,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BuildingGeometry {
    pub solid: MeshData,
    pub glow: MeshData,
}

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

fn scale(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Lot-aligned coordinate frame: `u` runs along the frontage, `v` inward from
/// the road.
struct Frame {
    center: Vec2,
    along: Vec2,
    inward: Vec2,
    width: f32,
    depth: f32,
}

impl Frame {
    fn from_lot(lot: &Lot) -> Self {
        let along = (lot.corners[1] - lot.corners[0]).normalize_or_zero();
        let inward = -lot.front_dir;
        Self { center: lot.center, along, inward, width: lot.width(), depth: lot.depth() }
    }

    fn yaw(&self) -> f32 {
        self.along.y.atan2(self.along.x)
    }

    fn to_world(&self, u: f32, v: f32) -> Vec2 {
        self.center + self.along * u + self.inward * v
    }

    fn at(&self, u: f32, y: f32, v: f32) -> Vec3 {
        let p = self.to_world(u, v);
        Vec3::new(p.x, y, p.y)
    }

    /// Box in lot coordinates: centre (u, y, v) and size (along, up, inward).
    fn boxed(&self, mesh: &mut MeshData, u: f32, y: f32, v: f32, size: Vec3, color: [f32; 4]) {
        let p = self.to_world(u, v);
        mesh.boxed(Vec3::new(p.x, y, p.y), size, self.yaw(), color);
    }
}

struct Ctx<'a> {
    frame: Frame,
    rules: &'a LandUseRules,
    style: &'a TierStyle,
    palette: &'a Palette,
    land_use: LandUse,
    tier: Tier,
    floors: u8,
    /// 0 = pristine, 1 = ruined.
    wear: f32,
    fw: f32,
    fd: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn generate_building(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    tier: Tier,
    condition: f32,
    seed: u64,
    rules: &BuildingRules,
    palette: &Palette,
) -> BuildingGeometry {
    let mut g = BuildingGeometry::default();
    let mut rng = rng_for(seed, 11);
    let frame = Frame::from_lot(lot);
    let r = rules.for_land_use(land_use);
    let ctx = Ctx {
        fw: (frame.width - 2.0 * r.inset).max(4.0),
        fd: (frame.depth - 2.0 * r.inset).max(4.0),
        frame,
        rules: r,
        style: rules.style(tier),
        palette,
        land_use,
        tier,
        floors: floors.max(1),
        wear: 1.0 - condition,
    };

    match land_use {
        LandUse::Park => park(&mut g, &ctx, &mut rng),
        LandUse::Industrial => industrial(&mut g, &ctx, &mut rng),
        LandUse::Civic => brutalist(&mut g, &ctx, &mut rng, true, false),
        _ => match tier {
            Tier::Low => containers(&mut g, &ctx, &mut rng),
            Tier::Mid => {
                let advert = matches!(land_use, LandUse::Commercial | LandUse::Entertainment) || rng.random_bool(rules.advert_chance_residential as f64);
                brutalist(&mut g, &ctx, &mut rng, false, advert)
            }
            Tier::High => {
                if land_use == LandUse::Residential && rng.random_bool(rules.fortress_chance as f64) {
                    fortress(&mut g, &ctx, &mut rng)
                } else {
                    crystal(&mut g, &ctx, &mut rng)
                }
            }
        },
    }
    g
}

// ── low tier: the Stacks ─────────────────────────────────────────────────────
//
// Shipping containers and old trailers piled up the way people would do it:
// dropped on the lot at mixed orientations, each upper unit resting on units
// below (most of its footprint must be supported), overhanging corners propped
// on steel jacks, planks between neighbours, an external zigzag stair, and
// wires that run from a utility pole on the lot to the units. Grime scales
// with wear.

#[derive(Clone, Copy)]
struct Unit {
    /// Centre in lot coordinates (u along frontage, v inward).
    u: f32,
    v: f32,
    /// Rotation relative to the lot frame.
    yaw: f32,
    len: f32,
    wid: f32,
    h: f32,
    level: i32,
    /// Bottom height.
    y0: f32,
    trailer: bool,
    color: [f32; 3],
}

impl Unit {
    fn local_to_lot(&self, lu: f32, lv: f32) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(self.u + lu * c - lv * s, self.v + lu * s + lv * c)
    }

    fn corners(&self) -> [Vec2; 4] {
        let (hl, hw) = (self.len * 0.5, self.wid * 0.5);
        [self.local_to_lot(-hl, -hw), self.local_to_lot(hl, -hw), self.local_to_lot(hl, hw), self.local_to_lot(-hl, hw)]
    }

    fn contains(&self, p: Vec2) -> bool {
        let (s, c) = self.yaw.sin_cos();
        let d = p - Vec2::new(self.u, self.v);
        let lu = d.x * c + d.y * s;
        let lv = -d.x * s + d.y * c;
        lu.abs() <= self.len * 0.5 && lv.abs() <= self.wid * 0.5
    }

    fn top(&self) -> f32 {
        self.y0 + self.h
    }
}

/// Separating-axis overlap test for two rotated rectangles.
fn units_overlap(a: &Unit, b: &Unit, margin: f32) -> bool {
    let ca = a.corners();
    let cb = b.corners();
    let axes = [
        ca[1] - ca[0],
        ca[3] - ca[0],
        cb[1] - cb[0],
        cb[3] - cb[0],
    ];
    for axis in axes {
        let axis = axis.normalize_or_zero();
        if axis == Vec2::ZERO {
            continue;
        }
        let proj = |cs: &[Vec2; 4]| {
            let v: Vec<f32> = cs.iter().map(|p| p.dot(axis)).collect();
            (v.iter().cloned().fold(f32::MAX, f32::min), v.iter().cloned().fold(f32::MIN, f32::max))
        };
        let (a0, a1) = proj(&ca);
        let (b0, b1) = proj(&cb);
        if a1 + margin < b0 || b1 + margin < a0 {
            return false;
        }
    }
    true
}

fn containers(g: &mut BuildingGeometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let yaw0 = f.yaw();
    let steel = scale(pal.steel, 0.45 - 0.15 * c.wear);
    let lit_chance = c.style.lit_chance * (1.0 - 0.8 * c.wear);
    let (hw, hd) = (c.fw * 0.5 + 0.6, c.fd * 0.5 + 0.6);

    // ── placement ────────────────────────────────────────────────────────────
    let mut units: Vec<Unit> = Vec::new();
    let ground_target = ((c.fw * c.fd / 26.0) as i32).clamp(2, 10);
    let levels = ((c.floors as f32 * 1.3) as i32).clamp(2, 8);
    let mut prev_count = ground_target;
    for level in 0..levels {
        let target = if level == 0 { ground_target } else { ((prev_count as f32 * rng.random_range(0.6..0.9)).round() as i32).max(1) };
        let mut placed = 0;
        let mut attempts = 0;
        while placed < target && attempts < 80 {
            attempts += 1;
            let trailer = rng.random_bool(0.2);
            let (len, wid, h) = if trailer { (rng.random_range(6.5..8.5), 2.7, 2.9) } else if rng.random_bool(0.3) { (12.1, 2.44, 2.6) } else { (6.05, 2.44, 2.6) };
            let yaw = match rng.random_range(0..10) {
                0..=5 => 0.0,
                6..=8 => std::f32::consts::FRAC_PI_2,
                _ => rng.random_range(-0.45..0.45),
            };
            let (u, v, y0) = if level == 0 {
                (rng.random_range(-hw + len * 0.5..hw - len * 0.5).clamp(-hw, hw), rng.random_range(-hd + wid * 0.5..hd - wid * 0.5).clamp(-hd, hd), 0.0)
            } else {
                let below: Vec<&Unit> = units.iter().filter(|x| x.level == level - 1).collect();
                let Some(support) = below.choose(rng) else { break };
                let reach = support.len.max(len) * 0.45;
                let off = Vec2::new(rng.random_range(-reach..reach), rng.random_range(-2.0..2.0));
                let off = if support.yaw.abs() > 0.5 { Vec2::new(off.y, off.x) } else { off };
                (support.u + off.x, support.v + off.y, support.top())
            };
            let color = if trailer { scale([0.62, 0.60, 0.55], rng.random_range(0.6..0.95)) } else { *pal.container_colors.choose(rng).unwrap() };
            let cand = Unit { u, v, yaw, len, wid, h, level, y0, trailer, color };
            // Stay roughly inside the lot.
            if cand.corners().iter().any(|p| p.x.abs() > hw + 1.0 || p.y.abs() > hd + 1.0) {
                continue;
            }
            // No overlap with units on the same level.
            if units.iter().any(|o| o.level == level && units_overlap(o, &cand, 0.35)) {
                continue;
            }
            // Upper units need most of their footprint supported.
            if level > 0 {
                let mut inside = 0;
                let samples = 15;
                for i in 0..5 {
                    for j in 0..3 {
                        let lu = (i as f32 + 0.5) / 5.0 * len - len * 0.5;
                        let lv = (j as f32 + 0.5) / 3.0 * wid - wid * 0.5;
                        let p = cand.local_to_lot(lu, lv);
                        if units.iter().any(|o| o.level == level - 1 && o.contains(p)) {
                            inside += 1;
                        }
                    }
                }
                if (inside as f32) < samples as f32 * 0.55 {
                    continue;
                }
            }
            units.push(cand);
            placed += 1;
        }
        if placed == 0 {
            break;
        }
        prev_count = placed;
    }

    // ── draw units ───────────────────────────────────────────────────────────
    for unit in &units {
        let col = scale(unit.color, (0.6 + rng.random_range(0.0..0.3)) * (1.0 - 0.45 * c.wear));
        let center = unit.local_to_lot(0.0, 0.0);
        let world = f.to_world(center.x, center.y);
        let yaw = yaw0 + unit.yaw;
        let yc = unit.y0 + unit.h * 0.5;
        g.solid.boxed(Vec3::new(world.x, yc, world.y), Vec3::new(unit.len, unit.h, unit.wid), yaw, rgba(col));
        let place = |mesh: &mut MeshData, lu: f32, y: f32, lv: f32, size: Vec3, color: [f32; 4]| {
            let p = unit.local_to_lot(lu, lv);
            let w = f.to_world(p.x, p.y);
            mesh.boxed(Vec3::new(w.x, y, w.y), size, yaw, color);
        };
        if unit.trailer {
            let stripe = scale(*pal.container_colors.choose(rng).unwrap(), 0.8);
            for side in [-1.0f32, 1.0] {
                place(&mut g.solid, 0.0, yc - 0.3, side * (unit.wid * 0.5 + 0.02), Vec3::new(unit.len * 0.96, 0.35, 0.04), rgba(stripe));
            }
            place(&mut g.solid, 0.0, unit.top() + 0.05, 0.0, Vec3::new(unit.len * 1.02, 0.1, unit.wid * 1.02), rgba(scale(col, 0.7)));
        } else {
            let ribs = (unit.len / 1.5) as i32;
            for k in 0..ribs {
                let lu = (k as f32 + 0.5) / ribs as f32 * unit.len - unit.len * 0.5;
                for side in [-1.0f32, 1.0] {
                    place(&mut g.solid, lu, yc, side * (unit.wid * 0.5 + 0.02), Vec3::new(0.18, unit.h * 0.9, 0.04), rgba(scale(col, 0.72)));
                }
            }
            let door = if rng.random_bool(0.5) { -1.0 } else { 1.0 };
            place(&mut g.solid, door * (unit.len * 0.5 + 0.02), yc, 0.0, Vec3::new(0.04, unit.h * 0.92, unit.wid * 0.92), rgba(scale(col, 0.55)));
        }
        // A cut-in window with light on one long side, plywood, tarp, rust.
        let side = if rng.random_bool(0.5) { -1.0 } else { 1.0 };
        if rng.random_bool(lit_chance as f64) {
            let wc = if rng.random_bool(0.3) { [0.5, 0.75, 1.1] } else { *c.style.window_colors.choose(rng).unwrap() };
            place(&mut g.glow, rng.random_range(-unit.len * 0.3..unit.len * 0.3), yc + 0.3, side * (unit.wid * 0.5 + 0.05), Vec3::new(0.9, 0.7, 0.06), rgba(scale(wc, 0.9)));
        }
        if rng.random_bool((0.25 + 0.4 * c.wear) as f64) {
            place(&mut g.solid, rng.random_range(-unit.len * 0.3..unit.len * 0.3), yc + rng.random_range(-0.5..0.5), -side * (unit.wid * 0.5 + 0.05), Vec3::new(rng.random_range(0.6..1.4), rng.random_range(0.6..1.2), 0.05), rgba([0.36, 0.26, 0.14]));
        }
        if rng.random_bool(0.25) {
            let tc = scale(*pal.tarp_colors.choose(rng).unwrap(), 0.8);
            place(&mut g.solid, rng.random_range(-unit.len * 0.25..unit.len * 0.25), yc + unit.h * 0.15, side * (unit.wid * 0.5 + 0.1), Vec3::new(rng.random_range(2.0..4.0), unit.h * 0.75, 0.08), rgba(tc));
        }
        for _ in 0..rng.random_range(0..=(1.0 + 2.0 * c.wear) as u32 + 1) {
            let s2 = if rng.random_bool(0.5) { -1.0 } else { 1.0 };
            place(&mut g.solid, rng.random_range(-unit.len * 0.4..unit.len * 0.4), yc + rng.random_range(-0.8..0.8), s2 * (unit.wid * 0.5 + 0.06), Vec3::new(rng.random_range(0.3..1.4), rng.random_range(0.3..1.3), 0.04), rgba(crate::palette::RUST_STAIN));
        }
        // Roof clutter on the topmost units: tarps, dishes, AC, a water drum.
        let exposed = !units.iter().any(|o| o.level == unit.level + 1 && o.contains(center));
        if exposed {
            if rng.random_bool(0.45) {
                let tc = scale(*pal.tarp_colors.choose(rng).unwrap(), 0.85);
                place(&mut g.solid, rng.random_range(-unit.len * 0.2..unit.len * 0.2), unit.top() + 0.08, 0.0, Vec3::new(rng.random_range(2.5..unit.len.max(3.0)), 0.1, unit.wid * 0.9), rgba(tc));
            }
            if rng.random_bool(0.4) {
                let lu = rng.random_range(-unit.len * 0.35..unit.len * 0.35);
                place(&mut g.solid, lu, unit.top() + 0.5, 0.0, Vec3::new(0.1, 1.0, 0.1), rgba(steel));
                place(&mut g.solid, lu, unit.top() + 1.0, -0.3, Vec3::new(0.9, 0.9, 0.12), rgba(scale(pal.steel, 1.1)));
            }
            if rng.random_bool(0.35) {
                place(&mut g.solid, rng.random_range(-unit.len * 0.35..unit.len * 0.35), unit.top() + 0.45, rng.random_range(-0.6..0.6), Vec3::new(0.9, 0.9, 0.9), rgba(if rng.random_bool(0.5) { pal.rust } else { [0.2, 0.3, 0.6] }));
            }
        }
        if rng.random_bool(0.25) {
            place(&mut g.solid, rng.random_range(-unit.len * 0.3..unit.len * 0.3), yc - 0.6, side * (unit.wid * 0.5 + 0.35), Vec3::new(0.8, 0.5, 0.6), rgba(if rng.random_bool(0.4) { pal.rust } else { [0.5, 0.5, 0.5] }));
        }
    }

    // ── jacks under unsupported corners ─────────────────────────────────────
    for unit in units.iter().filter(|x| x.level > 0) {
        for corner in unit.corners() {
            let inset = corner + (Vec2::new(unit.u, unit.v) - corner).normalize_or_zero() * 0.35;
            let supported = units.iter().any(|o| o.level == unit.level - 1 && o.contains(inset));
            if supported {
                continue;
            }
            // Rest on the highest unit below that covers the point, else the ground.
            let base = units.iter().filter(|o| o.level < unit.level && o.contains(inset)).map(|o| o.top()).fold(0.0, f32::max);
            let w = f.to_world(inset.x, inset.y);
            let h = unit.y0 - base;
            if h > 0.3 {
                g.solid.boxed(Vec3::new(w.x, base + h * 0.5, w.y), Vec3::new(0.22, h, 0.22), yaw0, rgba(if rng.random_bool(0.4) { pal.rust } else { steel }));
            }
        }
    }

    // ── planks between neighbours on the same level ──────────────────────────
    for i in 0..units.len() {
        for j in (i + 1)..units.len() {
            let (a, b) = (units[i], units[j]);
            if a.level != b.level || a.level == 0 {
                continue;
            }
            let ca = Vec2::new(a.u, a.v);
            let cb = Vec2::new(b.u, b.v);
            let dist = ca.distance(cb);
            let reach = (a.len.max(a.wid) + b.len.max(b.wid)) * 0.5;
            if dist > reach + 2.2 || dist < reach * 0.6 {
                continue;
            }
            let mid = (ca + cb) * 0.5;
            if a.contains(mid) || b.contains(mid) {
                continue;
            }
            let w = f.to_world(mid.x, mid.y);
            let d = cb - ca;
            let span = (dist - reach * 0.7).max(1.0);
            g.solid.boxed(Vec3::new(w.x, a.y0 + 0.08, w.y), Vec3::new(span, 0.12, 0.9), yaw0 + d.y.atan2(d.x), rgba([0.36, 0.26, 0.14]));
        }
    }

    // ── external zigzag stair up one side of the pile ───────────────────────
    let top_level = units.iter().map(|x| x.level).max().unwrap_or(0);
    if top_level >= 1 {
        let su = -hw + 1.2;
        let sv = rng.random_range(-hd * 0.5..hd * 0.5);
        let run = 3.4;
        let mut y = 0.0;
        for l in 0..top_level {
            let rise = 2.6;
            let steps = 8;
            let dir = if l % 2 == 0 { 1.0 } else { -1.0 };
            for s in 0..steps {
                let t = (s as f32 + 0.5) / steps as f32;
                f.boxed(&mut g.solid, su, y + t * rise, sv + dir * (t * run - run * 0.5), Vec3::new(0.9, 0.16, 0.42), rgba(steel));
            }
            f.boxed(&mut g.solid, su - 0.5, y + rise * 0.5 + 0.5, sv, Vec3::new(0.06, rise + 1.0, run), rgba(steel));
            f.boxed(&mut g.solid, su, y + rise + 0.08, sv + dir * run * 0.5, Vec3::new(1.0, 0.16, 1.0), rgba(steel));
            y += rise;
        }
        f.boxed(&mut g.solid, su, y * 0.5, sv + run * 0.55, Vec3::new(0.15, y, 0.15), rgba(steel));
        f.boxed(&mut g.solid, su, y * 0.5, sv - run * 0.55, Vec3::new(0.15, y, 0.15), rgba(steel));
    }

    // ── utility pole and wires that reach real units ─────────────────────────
    let pole = Vec2::new(hw - 1.0, -hd - 1.2);
    let pole_h = 9.0;
    let pw = f.to_world(pole.x, pole.y);
    g.solid.boxed(Vec3::new(pw.x, pole_h * 0.5, pw.y), Vec3::new(0.3, pole_h, 0.3), yaw0, rgba([0.28, 0.22, 0.16]));
    f.boxed(&mut g.solid, pole.x, pole_h - 0.6, pole.y, Vec3::new(1.8, 0.12, 0.12), rgba([0.28, 0.22, 0.16]));
    f.boxed(&mut g.glow, pole.x, pole_h - 1.6, pole.y - 0.5, Vec3::new(0.5, 0.25, 0.4), rgba(pal.street_lamp));
    let pole_top = Vec3::new(pw.x, pole_h - 0.6, pw.y);
    let upper: Vec<&Unit> = units.iter().filter(|x| x.level >= 1).collect();
    for unit in upper.choose_multiple(rng, 3) {
        let corner = unit.corners()[rng.random_range(0..4)];
        let w = f.to_world(corner.x, corner.y);
        g.solid.beam(pole_top, Vec3::new(w.x, unit.top() + 0.1, w.y), 0.05, rgba([0.05, 0.05, 0.05]));
    }
    for _ in 0..rng.random_range(2..5) {
        let (Some(a), Some(b)) = (upper.choose(rng), upper.choose(rng)) else { break };
        if std::ptr::eq(*a, *b) {
            continue;
        }
        let pa = f.to_world(a.u, a.v);
        let pb = f.to_world(b.u, b.v);
        if pa.distance(pb) < 3.0 {
            continue;
        }
        let from = Vec3::new(pa.x, a.top() + 0.1, pa.y);
        let to = Vec3::new(pb.x, b.top() + 0.1, pb.y);
        g.solid.beam(from, to, 0.04, rgba([0.05, 0.05, 0.05]));
        // Laundry on some lines.
        if rng.random_bool(0.5) {
            for k in 1..4 {
                let t = k as f32 / 4.0;
                let p = from.lerp(to, t);
                let tc = *pal.tarp_colors.choose(rng).unwrap();
                g.solid.boxed(Vec3::new(p.x, p.y - 0.4, p.z), Vec3::new(0.5, 0.7, 0.04), yaw0 + rng.random_range(-0.5..0.5), rgba(scale(tc, 1.4)));
            }
        }
    }

    // ── street level: stalls for shops, a drum fire, junk ────────────────────
    if matches!(c.land_use, LandUse::Commercial | LandUse::Entertainment) {
        let sv = -hd - 1.0;
        for k in 0..((c.fw / 5.0) as i32).max(1) {
            let su = -hw + 2.5 + k as f32 * 5.0;
            f.boxed(&mut g.solid, su, 2.4, sv, Vec3::new(4.2, 0.1, 2.2), rgba(scale(*pal.tarp_colors.choose(rng).unwrap(), 0.9)));
            f.boxed(&mut g.solid, su, 0.5, sv, Vec3::new(3.6, 1.0, 1.2), rgba([0.3, 0.22, 0.14]));
            let sc = *pal.sign_colors.choose(rng).unwrap();
            f.boxed(&mut g.glow, su, 2.1, sv - 1.0, Vec3::new(3.0, 0.3, 0.08), rgba(scale(sc, 0.9 - 0.5 * c.wear)));
        }
    }
    if rng.random_bool(0.4) {
        let bu = rng.random_range(-hw * 0.8..hw * 0.8);
        f.boxed(&mut g.solid, bu, 0.45, -hd - 1.2, Vec3::new(0.6, 0.9, 0.6), rgba(pal.rust));
        f.boxed(&mut g.glow, bu, 1.0, -hd - 1.2, Vec3::new(0.3, 0.2, 0.3), rgba([2.0, 0.9, 0.3]));
    }
}

// ── middle tier: brutalist concrete ──────────────────────────────────────────

fn brutalist(g: &mut BuildingGeometry, c: &Ctx, rng: &mut impl Rng, monumental: bool, advert: bool) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.rules.floor_height;
    let concrete = *pal.concrete.choose(rng).unwrap();
    let concrete = scale(concrete, 1.0 - 0.3 * c.wear);
    let dark = scale(concrete, 0.55);
    let lit_chance = c.style.lit_chance * (1.0 - 0.8 * c.wear);

    // Stacked slabs with alternating overhangs.
    let floors = c.floors as i32;
    let slab_count = if floors >= 6 { rng.random_range(2..=3) } else if floors >= 3 { rng.random_range(1..=2) } else { 1 };
    let mut remaining = floors;
    let mut y = 0.0;
    let mut slabs: Vec<(f32, f32, f32, f32, f32, f32)> = Vec::new(); // (u, v, w, d, y0, h)
    for i in 0..slab_count {
        let fl = if i == slab_count - 1 { remaining } else { (remaining / (slab_count - i)).max(1) };
        remaining -= fl;
        let h = fl as f32 * fh;
        let w = c.fw * rng.random_range(0.8..1.0);
        let d = c.fd * rng.random_range(0.75..1.0);
        let u = if i == 0 { 0.0 } else { rng.random_range(-1.8..1.8) };
        let v = if i == 0 { 0.0 } else { rng.random_range(-1.2..1.2) };
        slabs.push((u, v, w, d, y, h));
        y += h;
    }
    let ground_h = if monumental { fh * 1.6 } else { fh };

    for (si, &(u, v, w, d, y0, h)) in slabs.iter().enumerate() {
        if si == 0 && monumental {
            // Pilotis: the mass floats on columns above an open plaza.
            f.boxed(&mut g.solid, u, y0 + ground_h + (h - ground_h) * 0.5, v, Vec3::new(w, h - ground_h, d), rgba(concrete));
            for k in 0..((w / 4.0) as i32).max(2) {
                for side in [-1.0f32, 1.0] {
                    let cu = u - w * 0.5 + 2.0 + k as f32 * 4.0;
                    f.boxed(&mut g.solid, cu, y0 + ground_h * 0.5, v + side * (d * 0.5 - 1.0), Vec3::new(0.9, ground_h, 0.9), rgba(concrete));
                }
            }
            f.boxed(&mut g.solid, u, 0.15, v, Vec3::new(w + 4.0, 0.3, d + 4.0), rgba(scale(concrete, 0.8)));
            f.boxed(&mut g.solid, u, 0.45, v, Vec3::new(w + 2.0, 0.3, d + 2.0), rgba(scale(concrete, 0.85)));
        } else {
            f.boxed(&mut g.solid, u, y0 + h * 0.5, v, Vec3::new(w, h, d), rgba(concrete));
        }
        // Slab edge line between floors reads as the concrete floor plates.
        let first = if si == 0 && monumental { 1 } else { 0 };
        let fl = (h / fh).round() as i32;
        for k in first..fl {
            let fy = y0 + k as f32 * fh;
            f.boxed(&mut g.solid, u, fy + fh * 0.95, v, Vec3::new(w + 0.5, 0.35, d + 0.5), rgba(scale(concrete, 1.08)));
            // Recessed window band on the front and back, lit segments inside.
            for side in [-1.0f32, 1.0] {
                let bv = v + side * (d * 0.5 + 0.06);
                f.boxed(&mut g.solid, u, fy + fh * 0.5, bv, Vec3::new(w * 0.92, fh * 0.45, 0.1), rgba(dark));
                let count = ((w * 0.92) / c.rules.window_spacing).floor().max(1.0) as i32;
                for wi in 0..count {
                    if !rng.random_bool(lit_chance as f64) {
                        continue;
                    }
                    let wu = u - w * 0.46 + (wi as f32 + 0.5) * (w * 0.92 / count as f32);
                    let wc = *c.style.window_colors.choose(rng).unwrap();
                    f.boxed(&mut g.glow, wu, fy + fh * 0.5, bv + side * 0.03, Vec3::new(c.rules.window_spacing * 0.6, fh * 0.32, 0.05), rgba(scale(wc, rng.random_range(0.6..1.0))));
                }
            }
            // Side bands, narrower.
            for side in [-1.0f32, 1.0] {
                let bu = u + side * (w * 0.5 + 0.06);
                f.boxed(&mut g.solid, bu, fy + fh * 0.5, v, Vec3::new(0.1, fh * 0.4, d * 0.85), rgba(dark));
                if rng.random_bool((lit_chance * 0.7) as f64) {
                    let wc = *c.style.window_colors.choose(rng).unwrap();
                    f.boxed(&mut g.glow, bu + side * 0.03, fy + fh * 0.5, v + rng.random_range(-d * 0.3..d * 0.3), Vec3::new(0.05, fh * 0.3, 1.6), rgba(wc));
                }
            }
        }
        // Vertical fins on the front.
        let fins = ((w / 3.2) as i32).max(2);
        for k in 0..=fins {
            let fu = u - w * 0.5 + k as f32 * (w / fins as f32);
            f.boxed(&mut g.solid, fu, y0 + h * 0.5, v - d * 0.5 - 0.3, Vec3::new(0.35, h, 0.6), rgba(scale(concrete, 1.05)));
        }
        // Grime streaks under the slab edges.
        let streaks = ((c.style.grime + c.wear) * rng.random_range(0.0..4.0)) as i32;
        for _ in 0..streaks {
            let su = u + rng.random_range(-w * 0.45..w * 0.45);
            let sh = rng.random_range(1.0..h.max(1.5));
            f.boxed(&mut g.solid, su, y0 + h - sh * 0.5, v - d * 0.5 - 0.08, Vec3::new(rng.random_range(0.3..1.2), sh, 0.05), rgba(crate::palette::RUST_STAIN));
        }
    }

    let top = y;
    // Roof gear sits inside the top slab's footprint so nothing floats.
    let (tu, tv, tw, td, _, _) = *slabs.last().unwrap();
    f.boxed(&mut g.solid, tu + rng.random_range(-tw * 0.2..tw * 0.2), top + 1.4, tv + rng.random_range(-td * 0.2..td * 0.2), Vec3::new(tw * 0.3, 2.8, td * 0.3), rgba(scale(concrete, 0.9)));
    for _ in 0..rng.random_range(1..4) {
        let s = rng.random_range(0.8..1.8);
        f.boxed(&mut g.solid, tu + rng.random_range(-tw * 0.4..tw * 0.4), top + s * 0.5, tv + rng.random_range(-td * 0.4..td * 0.4), Vec3::new(s, s, s), rgba(pal.steel));
    }
    if rng.random_bool(c.rules.roof_antenna_chance as f64) {
        let ah = rng.random_range(4.0..10.0);
        let au = tu + rng.random_range(-tw * 0.3..tw * 0.3);
        f.boxed(&mut g.solid, au, top + ah * 0.5, tv, Vec3::new(0.3, ah, 0.3), rgba(pal.steel));
        f.boxed(&mut g.glow, au, top + ah + 0.15, tv, Vec3::new(0.35, 0.35, 0.35), rgba(pal.aviation_light));
    }

    // LED advert screens: one on the front facade, maybe one on the roof.
    if advert {
        let (u0, _, w0, d0, _, h0) = slabs[0];
        let sw = (w0 * rng.random_range(0.35..0.6)).min(14.0);
        let sh = (sw * 0.56).min(h0 * 0.6);
        let su = u0 + rng.random_range(-(w0 * 0.5 - sw * 0.5)..(w0 * 0.5 - sw * 0.5)).clamp(-w0 * 0.4, w0 * 0.4);
        let sy = (h0 * 0.55).max(sh * 0.5 + fh);
        led_screen(g, f, rng, pal, su, sy, -d0 * 0.5 - 0.3, sw, sh, c.wear);
        if rng.random_bool(0.5) {
            let rw = (tw * 0.7).min(16.0);
            let rh = rw * 0.4;
            for side in [-1.0f32, 1.0] {
                f.boxed(&mut g.solid, tu + side * rw * 0.45, top + rh * 0.55, tv, Vec3::new(0.3, rh * 1.1 + 0.5, 0.3), rgba(pal.steel));
            }
            led_screen(g, f, rng, pal, tu, top + rh * 0.6 + 0.5, tv - 0.3, rw, rh, c.wear);
        }
    }
    // Entrance canopy and lamp.
    let (u0, _, _, d0, _, _) = slabs[0];
    f.boxed(&mut g.solid, u0, 2.9, -d0 * 0.5 - 1.0, Vec3::new(4.0, 0.3, 2.0), rgba(scale(concrete, 0.9)));
    f.boxed(&mut g.glow, u0, 2.7, -d0 * 0.5 - 1.0, Vec3::new(1.6, 0.08, 0.8), rgba(scale(pal.lamp, 0.8)));
}

/// A framed LED billboard: a grid of brightly lit cells in two colours plus a
/// dim frame. Dead pixels appear with wear.
#[allow(clippy::too_many_arguments)]
fn led_screen(g: &mut BuildingGeometry, f: &Frame, rng: &mut impl Rng, pal: &Palette, u: f32, y: f32, v: f32, w: f32, h: f32, wear: f32) {
    f.boxed(&mut g.solid, u, y, v + 0.18, Vec3::new(w + 0.4, h + 0.4, 0.36), rgba([0.08, 0.08, 0.09]));
    let a = *pal.led_colors.choose(rng).unwrap();
    let b = *pal.led_colors.choose(rng).unwrap();
    let cols = 5;
    let rows = 3;
    let cw = w / cols as f32;
    let ch = h / rows as f32;
    for i in 0..cols {
        for j in 0..rows {
            if rng.random_bool((wear * 0.5) as f64) {
                continue; // dead pixels
            }
            let t = (i as f32 / cols as f32 + j as f32 / rows as f32) * 0.5 + rng.random_range(-0.2..0.2);
            let t = t.clamp(0.0, 1.0);
            let col = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
            let k = rng.random_range(0.6..1.1);
            let cu = u - w * 0.5 + (i as f32 + 0.5) * cw;
            let cy = y - h * 0.5 + (j as f32 + 0.5) * ch;
            f.boxed(&mut g.glow, cu, cy, v, Vec3::new(cw * 0.96, ch * 0.96, 0.05), rgba(scale(col, k)));
        }
    }
}

// ── high tier: crystal towers ────────────────────────────────────────────────

fn crystal(g: &mut BuildingGeometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.rules.floor_height;
    let height = c.floors as f32 * fh;
    let lit_chance = c.style.lit_chance * (1.0 - 0.8 * c.wear);
    let shards = rng.random_range(1..=3);
    let max_r = (c.fw.min(c.fd) * 0.5).max(3.0);
    let mut shard_centers: Vec<(Vec2, Vec2, f32)> = Vec::new(); // (base, top, height)

    // Podium shared by the shards.
    let podium_h = fh * 1.2;
    f.boxed(&mut g.solid, 0.0, podium_h * 0.5, 0.0, Vec3::new(c.fw, podium_h, c.fd), rgba(scale(pal.stone, 1.4)));
    f.boxed(&mut g.glow, 0.0, podium_h * 0.5, -c.fd * 0.5 - 0.05, Vec3::new(c.fw * 0.8, podium_h * 0.5, 0.05), rgba(scale(c.style.window_colors[0], 0.8)));

    for s in 0..shards {
        let main = s == 0;
        let r = if main { max_r * rng.random_range(0.7..0.95) } else { max_r * rng.random_range(0.35..0.6) };
        let h = if main { height } else { height * rng.random_range(0.35..0.8) };
        let sides = *[4usize, 5, 6, 6, 7, 8].choose(rng).unwrap();
        let rot = rng.random_range(0.0..std::f32::consts::TAU);
        let cu = if main { 0.0 } else { rng.random_range(-c.fw * 0.3..c.fw * 0.3) };
        let cv = if main { 0.0 } else { rng.random_range(-c.fd * 0.3..c.fd * 0.3) };
        let center = f.to_world(cu, cv);
        let taper = rng.random_range(0.25..0.75);
        let lean = Vec2::new(rng.random_range(-0.12..0.12), rng.random_range(-0.12..0.12)) * h;
        let top_center = center + lean.clamp_length_max(r * 0.8);
        let bottom = MeshData::ngon(center, r, sides, rot, podium_h);
        let top = MeshData::ngon(top_center, r * taper, sides, rot + rng.random_range(-0.4..0.4), podium_h + h);
        let glass = *pal.glass.choose(rng).unwrap();
        g.solid.frustum(&bottom, &top, rgba(scale(glass, 1.0 - 0.3 * c.wear)));
        shard_centers.push((center, top_center, h));

        // Lit floor bands: thin rings slightly proud of the facets.
        let floors_here = (h / fh) as i32;
        for k in 1..floors_here {
            if !rng.random_bool(lit_chance as f64) {
                continue;
            }
            let t0 = k as f32 / floors_here as f32;
            let t1 = t0 + 0.35 / floors_here as f32;
            let ring = |t: f32| -> Vec<Vec3> {
                bottom
                    .iter()
                    .zip(top.iter())
                    .map(|(b, tp)| {
                        let p = b.lerp(*tp, t);
                        let cc = Vec3::new(center.x, 0.0, center.y).lerp(Vec3::new(top_center.x, 0.0, top_center.y), t);
                        let out = Vec3::new(p.x - cc.x, 0.0, p.z - cc.z) * 0.02;
                        p + out
                    })
                    .collect()
            };
            let wc = *c.style.window_colors.choose(rng).unwrap();
            g.glow.frustum(&ring(t0), &ring(t1), rgba(scale(wc, rng.random_range(0.5..0.9))));
        }
        // Spire or crown light.
        let crown = top.iter().fold(Vec3::ZERO, |a, p| a + *p) / top.len() as f32;
        if main {
            let spire = rng.random_range(4.0..12.0);
            g.solid.frustum(&MeshData::ngon(top_center, r * taper * 0.5, sides, rot, podium_h + h), &MeshData::ngon(top_center, 0.3, sides, rot, podium_h + h + spire), rgba(pal.steel));
            g.glow.boxed(Vec3::new(crown.x, podium_h + h + spire + 0.15, crown.z), Vec3::new(0.5, 0.5, 0.5), 0.0, rgba(pal.aviation_light));
        } else {
            g.glow.boxed(Vec3::new(crown.x, podium_h + h + 0.15, crown.z), Vec3::new(0.4, 0.4, 0.4), 0.0, rgba(pal.aviation_light));
        }
    }
    // Skybridge between the first two shards when there are several.
    if shard_centers.len() >= 2 && rng.random_bool(0.6) {
        let (b0, t0, h0) = shard_centers[0];
        let (b1, t1, h1) = shard_centers[1];
        let y = podium_h + h0.min(h1) * rng.random_range(0.3..0.6);
        let p0 = b0.lerp(t0, (y - podium_h) / h0);
        let p1 = b1.lerp(t1, (y - podium_h) / h1);
        let mid = (p0 + p1) * 0.5;
        let d = p1 - p0;
        g.solid.boxed(Vec3::new(mid.x, y, mid.y), Vec3::new(d.length() + 2.0, 2.2, 2.5), d.y.atan2(d.x), rgba(pal.steel));
    }
    // Entrance lamp and a few bollard lights along the frontage.
    for k in 0..4 {
        let u = -c.fw * 0.375 + k as f32 * c.fw * 0.25;
        f.boxed(&mut g.glow, u, 0.45, -c.fd * 0.5 + 0.8, Vec3::new(0.25, 0.9, 0.25), rgba(scale(pal.lamp, 0.7)));
    }
}

// ── high tier: fortress compounds ────────────────────────────────────────────

fn fortress(g: &mut BuildingGeometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let stone = scale(pal.stone, 1.0 + rng.random_range(0.0..0.4));
    let wall_h = rng.random_range(6.0..9.5);
    let wall_t = 1.4;
    let hw = c.fw * 0.5;
    let hd = c.fd * 0.5;
    let lit_chance = c.style.lit_chance * (1.0 - 0.8 * c.wear);

    // Perimeter walls with a gate gap on the street side.
    let gate_w = 5.0;
    f.boxed(&mut g.solid, -(hw + gate_w * 0.5) * 0.5, wall_h * 0.5, -hd, Vec3::new(hw - gate_w * 0.5, wall_h, wall_t), rgba(stone));
    f.boxed(&mut g.solid, (hw + gate_w * 0.5) * 0.5, wall_h * 0.5, -hd, Vec3::new(hw - gate_w * 0.5, wall_h, wall_t), rgba(stone));
    f.boxed(&mut g.solid, 0.0, wall_h * 0.5, hd, Vec3::new(c.fw, wall_h, wall_t), rgba(stone));
    f.boxed(&mut g.solid, -hw, wall_h * 0.5, 0.0, Vec3::new(wall_t, wall_h, c.fd), rgba(stone));
    f.boxed(&mut g.solid, hw, wall_h * 0.5, 0.0, Vec3::new(wall_t, wall_h, c.fd), rgba(stone));
    // Crenellations.
    let step = 2.0;
    let mut u = -hw + 1.0;
    while u < hw {
        if u.abs() > gate_w * 0.5 + 0.5 {
            f.boxed(&mut g.solid, u, wall_h + 0.5, -hd, Vec3::new(1.0, 1.0, wall_t), rgba(stone));
        }
        f.boxed(&mut g.solid, u, wall_h + 0.5, hd, Vec3::new(1.0, 1.0, wall_t), rgba(stone));
        u += step;
    }
    // Gatehouse over the gap with a warning light and a barrier arm.
    f.boxed(&mut g.solid, 0.0, wall_h + 1.0, -hd, Vec3::new(gate_w + 2.0, 2.4, wall_t + 1.5), rgba(scale(stone, 0.9)));
    f.boxed(&mut g.glow, 0.0, wall_h + 2.4, -hd - 0.9, Vec3::new(0.6, 0.3, 0.3), rgba(pal.aviation_light));
    f.boxed(&mut g.solid, 0.0, 4.9, -hd, Vec3::new(gate_w + 0.6, 0.6, wall_t), rgba(scale(stone, 0.9)));
    f.boxed(&mut g.glow, 0.0, 4.55, -hd, Vec3::new(gate_w * 0.8, 0.1, 0.4), rgba(scale(pal.lamp, 1.1)));
    // Corner towers.
    for (su, sv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let center = f.to_world(su * hw, sv * hd);
        let th = wall_h + rng.random_range(4.0..9.0);
        g.solid.frustum(&MeshData::ngon(center, 2.6, 8, 0.0, 0.0), &MeshData::ngon(center, 2.2, 8, 0.0, th), rgba(stone));
        g.solid.frustum(&MeshData::ngon(center, 2.6, 8, 0.0, th), &MeshData::ngon(center, 0.6, 8, 0.0, th + 2.5), rgba(scale(stone, 0.8)));
        for k in 0..3 {
            if rng.random_bool(lit_chance as f64) {
                let wc = *c.style.window_colors.choose(rng).unwrap();
                let dir = Vec2::new(-su, -sv).normalize();
                let wy = wall_h + 1.5 + k as f32 * 1.6;
                let radius = 2.6 - 0.4 * (wy / th).clamp(0.0, 1.0);
                let p = center + dir * (radius - 0.1);
                g.glow.boxed(Vec3::new(p.x, wy, p.y), Vec3::new(0.3, 1.0, 0.3), 0.0, rgba(wc));
            }
        }
    }
    // Keep: a tall tapered core with a lantern top, plus an annex.
    let keep_h = (c.floors as f32 * c.rules.floor_height).max(wall_h + 10.0);
    let keep_c = f.to_world(0.0, hd * 0.2);
    let kr = (hw.min(hd) * 0.55).max(4.0);
    g.solid.frustum(&MeshData::rect(keep_c, kr, kr * 0.8, f.yaw(), 0.0), &MeshData::rect(keep_c, kr * 0.72, kr * 0.6, f.yaw(), keep_h), rgba(scale(stone, 1.15)));
    g.solid.frustum(&MeshData::rect(keep_c, kr * 0.8, kr * 0.66, f.yaw(), keep_h), &MeshData::rect(keep_c, kr * 0.5, kr * 0.4, f.yaw(), keep_h + 3.0), rgba(scale(stone, 0.9)));
    g.glow.frustum(&MeshData::rect(keep_c, kr * 0.74, kr * 0.62, f.yaw(), keep_h - 1.2), &MeshData::rect(keep_c, kr * 0.73, kr * 0.61, f.yaw(), keep_h - 0.4), rgba(scale(c.style.window_colors[0], 0.9)));
    // Slit windows up the keep.
    let fl = (keep_h / c.rules.floor_height) as i32;
    for k in 1..fl {
        for side in [-1.0f32, 1.0] {
            if !rng.random_bool(lit_chance as f64) {
                continue;
            }
            let t = k as f32 / fl as f32;
            let half = kr * 0.8 * (1.0 - 0.25 * t);
            let wc = *c.style.window_colors.choose(rng).unwrap();
            f.boxed(&mut g.glow, rng.random_range(-kr * 0.5..kr * 0.5), k as f32 * c.rules.floor_height + 1.2, hd * 0.2 + side * (half + 0.05), Vec3::new(0.45, 1.6, 0.08), rgba(wc));
        }
    }
    // Annex, garden and courtyard lamps.
    f.boxed(&mut g.solid, -hw * 0.5, 4.0, -hd * 0.35, Vec3::new(hw * 0.7, 8.0, hd * 0.5), rgba(scale(stone, 1.05)));
    f.boxed(&mut g.solid, hw * 0.55, 0.3, -hd * 0.3, Vec3::new(hw * 0.6, 0.6, hd * 0.6), rgba(pal.foliage));
    for (lu, lv) in [(-hw * 0.6, -hd * 0.7), (hw * 0.6, -hd * 0.7), (hw * 0.6, hd * 0.7)] {
        f.boxed(&mut g.solid, lu, 1.5, lv, Vec3::new(0.15, 3.0, 0.15), rgba(pal.steel));
        f.boxed(&mut g.glow, lu, 3.1, lv, Vec3::new(0.35, 0.25, 0.35), rgba(pal.lamp));
    }
}

// ── industrial sheds ─────────────────────────────────────────────────────────

fn industrial(g: &mut BuildingGeometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.rules.floor_height;
    let h = c.floors as f32 * fh;
    let wall = scale(*c.style.wall_colors.choose(rng).unwrap(), 1.0 - 0.3 * c.wear);
    f.boxed(&mut g.solid, 0.0, h * 0.5, 0.0, Vec3::new(c.fw, h, c.fd), rgba(wall));
    // Saw-tooth roof lights.
    let teeth = ((c.fd / 4.0) as i32).max(1);
    for k in 0..teeth {
        let v = -c.fd * 0.5 + (k as f32 + 0.5) * (c.fd / teeth as f32);
        f.boxed(&mut g.solid, 0.0, h + 0.9, v, Vec3::new(c.fw * 0.9, 1.8, 1.6), rgba(scale(wall, 0.9)));
        if rng.random_bool((0.5 * (1.0 - c.wear)) as f64) {
            f.boxed(&mut g.glow, 0.0, h + 0.9, v - 0.85, Vec3::new(c.fw * 0.85, 1.2, 0.05), rgba(scale(c.style.window_colors[0], 0.7)));
        }
    }
    // Roll-up doors and a loading dock on the street side.
    let doors = ((c.fw / 7.0) as i32).max(1);
    for k in 0..doors {
        let u = -c.fw * 0.5 + (k as f32 + 0.5) * (c.fw / doors as f32);
        f.boxed(&mut g.solid, u, 2.2, -c.fd * 0.5 - 0.05, Vec3::new(4.0, 4.4, 0.08), rgba(scale(pal.steel, 0.6)));
        f.boxed(&mut g.glow, u, 4.8, -c.fd * 0.5 - 0.2, Vec3::new(0.6, 0.2, 0.4), rgba(pal.street_lamp));
    }
    f.boxed(&mut g.solid, 0.0, 0.6, -c.fd * 0.5 - 1.5, Vec3::new(c.fw * 0.8, 1.2, 3.0), rgba(scale(wall, 0.8)));
    // Chimneys, tanks and pipes.
    for _ in 0..rng.random_range(1..=3) {
        let u = rng.random_range(-c.fw * 0.4..c.fw * 0.4);
        let v = rng.random_range(-c.fd * 0.3..c.fd * 0.3);
        let ch = rng.random_range(6.0..16.0);
        let center = f.to_world(u, v);
        g.solid.frustum(&MeshData::ngon(center, 1.2, 8, 0.0, h), &MeshData::ngon(center, 0.9, 8, 0.0, h + ch), rgba(scale(pal.steel, 0.7)));
        g.glow.boxed(Vec3::new(center.x, h + ch + 0.3, center.y), Vec3::new(0.4, 0.4, 0.4), 0.0, rgba(pal.aviation_light));
    }
    for _ in 0..rng.random_range(0..3) {
        let center = f.to_world(rng.random_range(-c.fw * 0.4..c.fw * 0.4), c.fd * 0.5 + 2.0);
        let th = rng.random_range(4.0..8.0);
        g.solid.frustum(&MeshData::ngon(center, 1.8, 10, 0.0, 0.0), &MeshData::ngon(center, 1.8, 10, 0.0, th), rgba(pal.rust));
    }
    for k in 0..rng.random_range(1..4) {
        let py = h * (0.3 + 0.2 * k as f32);
        f.boxed(&mut g.solid, 0.0, py, c.fd * 0.5 + 0.17, Vec3::new(c.fw * 0.9, 0.35, 0.35), rgba(scale(pal.steel, 0.8)));
    }
    if rng.random_bool(c.rules.sign_chance as f64) {
        let sc = *pal.sign_colors.choose(rng).unwrap();
        f.boxed(&mut g.glow, -c.fw * 0.25, h - 1.0, -c.fd * 0.5 - 0.12, Vec3::new(c.fw * 0.35, 1.0, 0.1), rgba(scale(sc, 0.8)));
    }
}

// ── parks ────────────────────────────────────────────────────────────────────

fn park(g: &mut BuildingGeometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let condition = 1.0 - c.wear;
    let trees = rng.random_range(3..8);
    for _ in 0..trees {
        let p = f.to_world(rng.random_range(-f.width * 0.4..f.width * 0.4), rng.random_range(-f.depth * 0.4..f.depth * 0.4));
        let h = rng.random_range(3.0..7.0);
        g.solid.boxed(Vec3::new(p.x, h * 0.5, p.y), Vec3::new(0.5, h, 0.5), 0.0, rgba([0.16, 0.13, 0.10]));
        let cw = rng.random_range(2.5..5.0);
        let col = scale(pal.foliage, 0.7 + 0.3 * condition);
        g.solid.frustum(&MeshData::ngon(p, cw * 0.5, 7, 0.0, h), &MeshData::ngon(p, cw * 0.15, 7, 0.4, h + cw * 0.9), rgba(col));
    }
    for i in 0..2 {
        let u = (i as f32 + 0.5) / 2.0 * f.width - f.width * 0.5;
        f.boxed(&mut g.solid, u, 2.0, -f.depth * 0.5 + 1.5, Vec3::new(0.2, 4.0, 0.2), rgba([0.25, 0.25, 0.27]));
        f.boxed(&mut g.glow, u, 4.2, -f.depth * 0.5 + 1.5, Vec3::new(0.6, 0.3, 0.6), rgba(pal.lamp));
    }
    // Benches and a path.
    f.boxed(&mut g.solid, 0.0, 0.05, 0.0, Vec3::new(f.width * 0.7, 0.1, 1.6), rgba(scale(pal.stone, 1.6)));
    for side in [-1.0f32, 1.0] {
        f.boxed(&mut g.solid, side * f.width * 0.2, 0.45, 1.3, Vec3::new(1.8, 0.1, 0.5), rgba([0.35, 0.25, 0.15]));
    }
}
