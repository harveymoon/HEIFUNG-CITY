//! Building grammar. Takes a lot, a land use, a floor count and a seed and
//! returns two meshes: a lit body and an unlit glow layer (windows, signs,
//! strips) whose vertex colours exceed 1.0 so bloom makes them shine.

use crate::mesh::MeshData;
use crate::palette::Palette;
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use nd_sim::{LandUse, Lot};
use rand::seq::IndexedRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LandUseRules {
    pub floor_height: f32,
    /// Inset of the footprint from the lot edges.
    pub inset: f32,
    pub wall_colors: Vec<[f32; 3]>,
    pub roof_color: [f32; 3],
    pub window_spacing: f32,
    pub window_size: [f32; 2],
    /// Probability a window is lit.
    pub lit_chance: f32,
    pub warm_window_chance: f32,
    pub sign_chance: f32,
    pub vertical_sign_chance: f32,
    pub roof_antenna_chance: f32,
    pub setback_chance: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildingRules {
    pub residential: LandUseRules,
    pub commercial: LandUseRules,
    pub industrial: LandUseRules,
    pub entertainment: LandUseRules,
    pub civic: LandUseRules,
    pub park: LandUseRules,
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
}

impl Default for BuildingRules {
    fn default() -> Self {
        let base = LandUseRules {
            floor_height: 3.2,
            inset: 1.5,
            wall_colors: vec![[0.22, 0.22, 0.27], [0.28, 0.25, 0.30], [0.18, 0.20, 0.26]],
            roof_color: [0.12, 0.13, 0.17],
            window_spacing: 2.4,
            window_size: [1.2, 1.6],
            lit_chance: 0.45,
            warm_window_chance: 0.6,
            sign_chance: 0.3,
            vertical_sign_chance: 0.1,
            roof_antenna_chance: 0.4,
            setback_chance: 0.3,
        };
        Self {
            residential: LandUseRules { ..base.clone() },
            commercial: LandUseRules {
                floor_height: 3.6,
                window_spacing: 2.0,
                window_size: [1.5, 2.0],
                lit_chance: 0.7,
                warm_window_chance: 0.35,
                sign_chance: 0.9,
                vertical_sign_chance: 0.5,
                wall_colors: vec![[0.25, 0.26, 0.32], [0.30, 0.28, 0.36]],
                ..base.clone()
            },
            industrial: LandUseRules {
                floor_height: 5.0,
                inset: 2.5,
                window_spacing: 4.0,
                window_size: [2.5, 1.0],
                lit_chance: 0.4,
                warm_window_chance: 0.9,
                sign_chance: 0.5,
                vertical_sign_chance: 0.0,
                roof_antenna_chance: 0.9,
                wall_colors: vec![[0.30, 0.26, 0.22], [0.22, 0.22, 0.22]],
                ..base.clone()
            },
            entertainment: LandUseRules {
                floor_height: 3.8,
                lit_chance: 0.8,
                warm_window_chance: 0.2,
                sign_chance: 1.0,
                vertical_sign_chance: 0.8,
                wall_colors: vec![[0.30, 0.15, 0.30], [0.20, 0.18, 0.32]],
                ..base.clone()
            },
            civic: LandUseRules {
                floor_height: 4.2,
                inset: 3.0,
                window_spacing: 3.0,
                window_size: [1.6, 2.4],
                lit_chance: 0.5,
                warm_window_chance: 0.1,
                sign_chance: 0.6,
                vertical_sign_chance: 0.0,
                wall_colors: vec![[0.42, 0.42, 0.47], [0.36, 0.37, 0.42]],
                ..base.clone()
            },
            park: LandUseRules { inset: 2.0, sign_chance: 0.0, vertical_sign_chance: 0.0, ..base },
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
        // Rotation that maps local +X to `along` in the XZ plane.
        self.along.y.atan2(self.along.x)
    }

    /// Local (u along frontage, v inward) to world XZ.
    fn to_world(&self, u: f32, v: f32) -> Vec2 {
        self.center + self.along * u + self.inward * v
    }
}

pub fn generate_building(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    seed: u64,
    rules: &BuildingRules,
    palette: &Palette,
) -> BuildingGeometry {
    let mut g = BuildingGeometry::default();
    let r = rules.for_land_use(land_use);
    let mut rng = rng_for(seed, 11);
    let frame = Frame::from_lot(lot);

    if land_use == LandUse::Park {
        generate_park(&mut g, &frame, &mut rng, palette);
        return g;
    }

    let wall = *r.wall_colors.choose(&mut rng).unwrap_or(&[0.05, 0.05, 0.07]);
    let fw = (frame.width - 2.0 * r.inset).max(4.0);
    let fd = (frame.depth - 2.0 * r.inset).max(4.0);
    let fh = r.floor_height;
    let yaw = frame.yaw();

    // Body, possibly with a setback tower on top.
    let setback = floors >= 5 && rng.random_bool(r.setback_chance as f64);
    let lower_floors = if setback { (floors / 2).max(2) } else { floors };
    let lower_h = lower_floors as f32 * fh;
    let c = frame.to_world(0.0, 0.0);
    g.solid.boxed(Vec3::new(c.x, lower_h * 0.5, c.y), Vec3::new(fw, lower_h, fd), yaw, rgba(wall));
    g.solid.boxed(Vec3::new(c.x, lower_h + 0.15, c.y), Vec3::new(fw + 0.4, 0.3, fd + 0.4), yaw, rgba(r.roof_color));
    windows(&mut g.glow, &frame, &mut rng, r, palette, 0.0, fw, fd, 0, lower_floors);

    let mut top_h = lower_h;
    if setback {
        let upper = floors - lower_floors;
        let uw = fw * 0.65;
        let ud = fd * 0.65;
        let uh = upper as f32 * fh;
        let uc = frame.to_world(0.0, (fd - ud) * 0.5 * rng.random_range(-0.8..0.8));
        g.solid.boxed(Vec3::new(uc.x, lower_h + uh * 0.5, uc.y), Vec3::new(uw, uh, ud), yaw, rgba(wall));
        g.solid.boxed(Vec3::new(uc.x, lower_h + uh + 0.15, uc.y), Vec3::new(uw + 0.3, 0.3, ud + 0.3), yaw, rgba(r.roof_color));
        let sub = Frame { center: uc, ..frame_copy(&frame) };
        windows(&mut g.glow, &sub, &mut rng, r, palette, lower_h, uw, ud, lower_floors, floors);
        top_h = lower_h + uh;
    }

    // Roof clutter.
    if rng.random_bool(r.roof_antenna_chance as f64) {
        let ah = rng.random_range(3.0..9.0);
        let ac = frame.to_world(rng.random_range(-fw * 0.3..fw * 0.3), rng.random_range(-fd * 0.3..fd * 0.3));
        g.solid.boxed(Vec3::new(ac.x, top_h + ah * 0.5, ac.y), Vec3::new(0.4, ah, 0.4), yaw, rgba([0.2, 0.2, 0.24]));
        g.glow.boxed(Vec3::new(ac.x, top_h + ah + 0.3, ac.y), Vec3::new(0.5, 0.5, 0.5), yaw, [3.5, 0.3, 0.2, 1.0]);
    }
    let units = rng.random_range(0..3);
    for _ in 0..units {
        let uc = frame.to_world(rng.random_range(-fw * 0.35..fw * 0.35), rng.random_range(-fd * 0.35..fd * 0.35));
        let s = rng.random_range(1.0..2.5);
        g.solid.boxed(Vec3::new(uc.x, top_h + s * 0.5, uc.y), Vec3::new(s, s, s * 1.3), yaw, rgba(scale(wall, 1.4)));
    }

    // Signs: a horizontal glowing strip over the entrance and maybe a vertical
    // neon board hanging off the front corner.
    if rng.random_bool(r.sign_chance as f64) {
        let col = *palette.glow_colors.choose(&mut rng).unwrap();
        let y = fh + 0.4;
        let sw = fw * rng.random_range(0.4..0.9);
        let sc = frame.to_world(0.0, -fd * 0.5 - 0.15);
        g.glow.boxed(Vec3::new(sc.x, y, sc.y), Vec3::new(sw, 0.5, 0.2), yaw, rgba(col));
    }
    if rng.random_bool(r.vertical_sign_chance as f64) {
        let col = *palette.glow_colors.choose(&mut rng).unwrap();
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let vh = (floors as f32 * fh * rng.random_range(0.3..0.7)).max(4.0);
        let sc = frame.to_world(side * (fw * 0.5 + 0.6), -fd * 0.5 + 1.0);
        let y = (floors as f32 * fh * 0.55).max(vh * 0.5 + 2.0);
        g.solid.boxed(Vec3::new(sc.x, y, sc.y), Vec3::new(1.4, vh, 0.5), yaw, rgba([0.12, 0.12, 0.14]));
        g.glow.boxed(Vec3::new(sc.x, y, sc.y), Vec3::new(1.0, vh - 0.6, 0.62), yaw, rgba(col));
    }

    // Entrance glow strip at ground level.
    let ec = frame.to_world(0.0, -fd * 0.5 - 0.05);
    let col = *palette.glow_colors.choose(&mut rng).unwrap();
    g.glow.boxed(Vec3::new(ec.x, 0.15, ec.y), Vec3::new(fw * 0.5, 0.1, 0.12), yaw, rgba(scale(col, 0.6)));

    g
}

fn frame_copy(f: &Frame) -> Frame {
    Frame { center: f.center, along: f.along, inward: f.inward, width: f.width, depth: f.depth }
}

#[allow(clippy::too_many_arguments)]
fn windows(
    glow: &mut MeshData,
    frame: &Frame,
    rng: &mut impl Rng,
    r: &LandUseRules,
    palette: &Palette,
    base_y: f32,
    fw: f32,
    fd: f32,
    first_floor: u8,
    end_floor: u8,
) {
    let yaw = frame.yaw();
    let (ww, wh) = (r.window_size[0], r.window_size[1]);
    let faces: [(f32, f32, bool); 4] = [
        // (offset along, offset inward, is_front_back)
        (0.0, -fd * 0.5, true),
        (0.0, fd * 0.5, true),
        (-fw * 0.5, 0.0, false),
        (fw * 0.5, 0.0, false),
    ];
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
                if !rng.random_bool(r.lit_chance as f64) {
                    continue;
                }
                let along = start + k as f32 * r.window_spacing;
                let (u, v) = if front_back { (ou + along, ov) } else { (ou, ov + along) };
                let p = frame.to_world(u, v);
                let col = if rng.random_bool(r.warm_window_chance as f64) {
                    palette.window_warm
                } else {
                    palette.window_cold
                };
                let k = rng.random_range(0.5..1.2);
                let size = if front_back { Vec3::new(ww, wh, 0.12) } else { Vec3::new(0.12, wh, ww) };
                glow.boxed(Vec3::new(p.x, y, p.y), size, yaw, rgba(scale(col, k)));
            }
        }
    }
}

fn generate_park(g: &mut BuildingGeometry, frame: &Frame, rng: &mut impl Rng, palette: &Palette) {
    let yaw = frame.yaw();
    let trees = rng.random_range(3..8);
    for _ in 0..trees {
        let p = frame.to_world(
            rng.random_range(-frame.width * 0.4..frame.width * 0.4),
            rng.random_range(-frame.depth * 0.4..frame.depth * 0.4),
        );
        let h = rng.random_range(3.0..7.0);
        g.solid.boxed(Vec3::new(p.x, h * 0.5, p.y), Vec3::new(0.5, h, 0.5), yaw, rgba([0.08, 0.05, 0.12]));
        let cw = rng.random_range(2.5..5.0);
        g.glow.boxed(Vec3::new(p.x, h + cw * 0.3, p.y), Vec3::new(cw, cw * 0.8, cw), yaw, [0.1, 1.6, 1.2, 1.0]);
    }
    // Lamp posts along the front.
    let lamps = 2;
    for i in 0..lamps {
        let u = (i as f32 + 0.5) / lamps as f32 * frame.width - frame.width * 0.5;
        let p = frame.to_world(u, -frame.depth * 0.5 + 1.5);
        g.solid.boxed(Vec3::new(p.x, 2.0, p.y), Vec3::new(0.2, 4.0, 0.2), yaw, rgba([0.1, 0.1, 0.12]));
        g.glow.boxed(Vec3::new(p.x, 4.2, p.y), Vec3::new(0.6, 0.3, 0.6), yaw, rgba(palette.window_warm));
    }
}
