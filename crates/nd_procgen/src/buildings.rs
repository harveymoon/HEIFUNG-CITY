//! Building grammar. Takes a lot, land use, floors, tier, condition and a seed
//! and returns two meshes: a lit body and an unlit glow layer (windows, signs,
//! lamps) whose vertex colours exceed 1.0 so bloom makes them shine softly.
//!
//! The look is worn near-future rather than neon: stained concrete and rusted
//! units on cheap buildings, plain mid-rise in the middle, glass and steel at
//! the top.

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
                    lit_chance: 0.32,
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

struct Frame {
    center: Vec2,
    /// Unit vector along the lot frontage.
    along: Vec2,
    /// Unit vector from the road into the lot.
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

    /// Local (u along frontage, v inward) to world XZ.
    fn to_world(&self, u: f32, v: f32) -> Vec2 {
        self.center + self.along * u + self.inward * v
    }

    fn shifted(&self, center: Vec2) -> Frame {
        Frame { center, along: self.along, inward: self.inward, width: self.width, depth: self.depth }
    }
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
    let r = rules.for_land_use(land_use);
    let style = rules.style(tier);
    let mut rng = rng_for(seed, 11);
    let frame = Frame::from_lot(lot);

    if land_use == LandUse::Park {
        generate_park(&mut g, &frame, &mut rng, palette, condition);
        return g;
    }

    // Worn buildings are darker and dirtier; derelict ones mostly dark.
    let wear = 1.0 - condition;
    let wall = scale(*style.wall_colors.choose(&mut rng).unwrap_or(&[0.4, 0.4, 0.4]), 1.0 - 0.35 * wear);
    let fw = (frame.width - 2.0 * r.inset).max(4.0);
    let fd = (frame.depth - 2.0 * r.inset).max(4.0);
    let fh = r.floor_height;
    let yaw = frame.yaw();
    let lit_chance = style.lit_chance * (1.0 - 0.8 * wear);

    let setback = floors >= 5 && rng.random_bool(r.setback_chance as f64);
    let lower_floors = if setback { (floors / 2).max(2) } else { floors };
    let lower_h = lower_floors as f32 * fh;
    let c = frame.to_world(0.0, 0.0);
    g.solid.boxed(Vec3::new(c.x, lower_h * 0.5, c.y), Vec3::new(fw, lower_h, fd), yaw, rgba(wall));
    g.solid.boxed(Vec3::new(c.x, lower_h + 0.15, c.y), Vec3::new(fw + 0.4, 0.3, fd + 0.4), yaw, rgba(style.roof_color));
    facade_details(&mut g, &frame, &mut rng, r, style, lit_chance, wear, 0.0, fw, fd, 0, lower_floors);

    let mut top_h = lower_h;
    if setback {
        let upper = floors - lower_floors;
        let uw = fw * 0.65;
        let ud = fd * 0.65;
        let uh = upper as f32 * fh;
        let uc = frame.to_world(0.0, (fd - ud) * 0.5 * rng.random_range(-0.8..0.8));
        g.solid.boxed(Vec3::new(uc.x, lower_h + uh * 0.5, uc.y), Vec3::new(uw, uh, ud), yaw, rgba(wall));
        g.solid.boxed(Vec3::new(uc.x, lower_h + uh + 0.15, uc.y), Vec3::new(uw + 0.3, 0.3, ud + 0.3), yaw, rgba(style.roof_color));
        let sub = frame.shifted(uc);
        facade_details(&mut g, &sub, &mut rng, r, style, lit_chance, wear, lower_h, uw, ud, lower_floors, floors);
        top_h = lower_h + uh;
    }

    // Roof: antenna with a dim aviation light, mechanical units, water tanks.
    if rng.random_bool(r.roof_antenna_chance as f64) {
        let ah = rng.random_range(3.0..9.0);
        let ac = frame.to_world(rng.random_range(-fw * 0.3..fw * 0.3), rng.random_range(-fd * 0.3..fd * 0.3));
        g.solid.boxed(Vec3::new(ac.x, top_h + ah * 0.5, ac.y), Vec3::new(0.3, ah, 0.3), yaw, rgba(style.trim_color));
        g.glow.boxed(Vec3::new(ac.x, top_h + ah + 0.2, ac.y), Vec3::new(0.35, 0.35, 0.35), yaw, rgba(palette.aviation_light));
    }
    let units = rng.random_range(0..=(1.0 + style.clutter * 3.0) as u32);
    for _ in 0..units {
        let uc = frame.to_world(rng.random_range(-fw * 0.35..fw * 0.35), rng.random_range(-fd * 0.35..fd * 0.35));
        let s = rng.random_range(1.0..2.6);
        let col = if rng.random_bool(0.4) { palette.rust } else { scale(style.trim_color, 1.1) };
        g.solid.boxed(Vec3::new(uc.x, top_h + s * 0.5, uc.y), Vec3::new(s, s, s * 1.3), yaw, rgba(col));
    }
    if tier == Tier::High && rng.random_bool(0.5) {
        // Roof garden strip.
        let gc = frame.to_world(0.0, fd * 0.2);
        g.solid.boxed(Vec3::new(gc.x, top_h + 0.5, gc.y), Vec3::new(fw * 0.6, 0.6, fd * 0.3), yaw, rgba(palette.foliage));
    }

    // Signs: muted lit boards, mostly on shops and venues.
    if rng.random_bool(r.sign_chance as f64) {
        let col = *palette.sign_colors.choose(&mut rng).unwrap();
        let y = fh + 0.4;
        let sw = fw * rng.random_range(0.35..0.8);
        let sc = frame.to_world(0.0, -fd * 0.5 - 0.15);
        g.solid.boxed(Vec3::new(sc.x, y, sc.y), Vec3::new(sw + 0.3, 0.8, 0.25), yaw, rgba(style.trim_color));
        g.glow.boxed(Vec3::new(sc.x, y, sc.y), Vec3::new(sw, 0.5, 0.3), yaw, rgba(scale(col, 1.0 - 0.6 * wear)));
    }
    if rng.random_bool(r.vertical_sign_chance as f64) {
        let col = *palette.sign_colors.choose(&mut rng).unwrap();
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let vh = (floors as f32 * fh * rng.random_range(0.25..0.5)).max(3.0);
        let sc = frame.to_world(side * (fw * 0.5 + 0.6), -fd * 0.5 + 1.0);
        let y = (floors as f32 * fh * 0.55).max(vh * 0.5 + 2.0);
        g.solid.boxed(Vec3::new(sc.x, y, sc.y), Vec3::new(1.3, vh, 0.5), yaw, rgba(style.trim_color));
        g.glow.boxed(Vec3::new(sc.x, y, sc.y), Vec3::new(0.9, vh - 0.6, 0.62), yaw, rgba(col));
    }

    // Entrance: a canopy and a lamp over the door.
    let ec = frame.to_world(0.0, -fd * 0.5 - 0.6);
    g.solid.boxed(Vec3::new(ec.x, 2.6, ec.y), Vec3::new(3.0, 0.2, 1.2), yaw, rgba(style.trim_color));
    g.glow.boxed(Vec3::new(ec.x, 2.45, ec.y), Vec3::new(1.2, 0.08, 0.6), yaw, rgba(scale(palette.lamp, 0.8)));

    g
}

/// Windows, grime, air conditioners and mullions on every facade.
#[allow(clippy::too_many_arguments)]
fn facade_details(
    g: &mut BuildingGeometry,
    frame: &Frame,
    rng: &mut impl Rng,
    r: &LandUseRules,
    style: &TierStyle,
    lit_chance: f32,
    wear: f32,
    base_y: f32,
    fw: f32,
    fd: f32,
    first_floor: u8,
    end_floor: u8,
) {
    let yaw = frame.yaw();
    let (ww, wh) = (r.window_size[0], r.window_size[1]);
    let faces: [(f32, f32, bool); 4] = [(0.0, -fd * 0.5, true), (0.0, fd * 0.5, true), (-fw * 0.5, 0.0, false), (fw * 0.5, 0.0, false)];
    let height = (end_floor - first_floor) as f32 * r.floor_height;
    for (ou, ov, front_back) in faces {
        let span = if front_back { fw } else { fd };
        let count = ((span - 1.0) / r.window_spacing).floor().max(0.0) as i32;
        if count == 0 {
            continue;
        }
        let start = -(count as f32 - 1.0) * r.window_spacing * 0.5;
        for floor in first_floor..end_floor {
            let y = base_y + (floor - first_floor) as f32 * r.floor_height + r.floor_height * 0.55;
            for k in 0..count {
                let along = start + k as f32 * r.window_spacing;
                let (u, v) = if front_back { (ou + along, ov) } else { (ou, ov + along) };
                let p = frame.to_world(u, v);
                let size = if front_back { Vec3::new(ww, wh, 0.12) } else { Vec3::new(0.12, wh, ww) };
                let boarded = wear > 0.6 && rng.random_bool(0.5);
                if boarded {
                    g.solid.boxed(Vec3::new(p.x, y, p.y), size, yaw, rgba(scale(style.trim_color, 0.7)));
                } else if rng.random_bool(lit_chance as f64) {
                    let col = *style.window_colors.choose(rng).unwrap();
                    let k = rng.random_range(0.6..1.1);
                    g.glow.boxed(Vec3::new(p.x, y, p.y), size, yaw, rgba(scale(col, k)));
                } else {
                    // Dark glass still reads as a window during the day.
                    g.solid.boxed(Vec3::new(p.x, y, p.y), size, yaw, [0.08, 0.09, 0.12, 1.0]);
                }
                // Air conditioners under some windows on cheap buildings.
                if rng.random_bool((style.clutter * 0.25) as f64) {
                    let size = if front_back { Vec3::new(0.8, 0.5, 0.5) } else { Vec3::new(0.5, 0.5, 0.8) };
                    let off = if front_back { frame.inward * (if ov < 0.0 { -0.25 } else { 0.25 }) } else { frame.along * (if ou < 0.0 { -0.25 } else { 0.25 }) };
                    let col = if rng.random_bool(0.3) { crate::palette::RUST } else { [0.55, 0.55, 0.56] };
                    g.solid.boxed(Vec3::new(p.x + off.x, y - wh * 0.5 - 0.3, p.y + off.y), size, yaw, rgba(col));
                }
            }
        }
        // Grime: dark streaks and patches hugging the facade.
        let streaks = ((style.grime + wear) * rng.random_range(0.0..3.0)) as u32;
        for _ in 0..streaks {
            let along = rng.random_range(-span * 0.45..span * 0.45);
            let (u, v) = if front_back { (ou + along, ov) } else { (ou, ov + along) };
            let p = frame.to_world(u, v);
            let out = if front_back { frame.inward * (if ov < 0.0 { -0.08 } else { 0.08 }) } else { frame.along * (if ou < 0.0 { -0.08 } else { 0.08 }) };
            let h = rng.random_range(1.0..(height * 0.6).max(1.5));
            let w = rng.random_range(0.4..2.5);
            let y = base_y + rng.random_range(0.0..(height - h).max(0.1)) + h * 0.5;
            let size = if front_back { Vec3::new(w, h, 0.06) } else { Vec3::new(0.06, h, w) };
            let col = if rng.random_bool(0.35) { crate::palette::RUST_STAIN } else { [0.09, 0.08, 0.07, 1.0][..3].try_into().unwrap() };
            g.solid.boxed(Vec3::new(p.x + out.x, y, p.y + out.y), size, yaw, rgba(col));
        }
        // Mullions on glass towers.
        if style.mullions {
            for k in 0..=count {
                let along = start - r.window_spacing * 0.5 + k as f32 * r.window_spacing;
                let (u, v) = if front_back { (ou + along, ov) } else { (ou, ov + along) };
                let p = frame.to_world(u, v);
                let out = if front_back { frame.inward * (if ov < 0.0 { -0.1 } else { 0.1 }) } else { frame.along * (if ou < 0.0 { -0.1 } else { 0.1 }) };
                let size = if front_back { Vec3::new(0.18, height, 0.2) } else { Vec3::new(0.2, height, 0.18) };
                g.solid.boxed(Vec3::new(p.x + out.x, base_y + height * 0.5, p.y + out.y), size, yaw, rgba(style.trim_color));
            }
        }
    }
}

fn generate_park(g: &mut BuildingGeometry, frame: &Frame, rng: &mut impl Rng, palette: &Palette, condition: f32) {
    let yaw = frame.yaw();
    let trees = rng.random_range(3..8);
    for _ in 0..trees {
        let p = frame.to_world(rng.random_range(-frame.width * 0.4..frame.width * 0.4), rng.random_range(-frame.depth * 0.4..frame.depth * 0.4));
        let h = rng.random_range(3.0..7.0);
        g.solid.boxed(Vec3::new(p.x, h * 0.5, p.y), Vec3::new(0.5, h, 0.5), yaw, rgba([0.16, 0.13, 0.10]));
        let cw = rng.random_range(2.5..5.0);
        let col = scale(palette.foliage, 0.7 + 0.3 * condition);
        g.solid.boxed(Vec3::new(p.x, h + cw * 0.3, p.y), Vec3::new(cw, cw * 0.8, cw), yaw, rgba(col));
    }
    let lamps = 2;
    for i in 0..lamps {
        let u = (i as f32 + 0.5) / lamps as f32 * frame.width - frame.width * 0.5;
        let p = frame.to_world(u, -frame.depth * 0.5 + 1.5);
        g.solid.boxed(Vec3::new(p.x, 2.0, p.y), Vec3::new(0.2, 4.0, 0.2), yaw, rgba([0.25, 0.25, 0.27]));
        g.glow.boxed(Vec3::new(p.x, 4.2, p.y), Vec3::new(0.6, 0.3, 0.6), yaw, rgba(palette.lamp));
    }
}
