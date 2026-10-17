//! Procedural vehicles: one parametric generator for everything on wheels,
//! from a beat-up hatchback to a bubble pod bus, an Akira bike or a pavement
//! delivery robot.
//!
//! A vehicle is described by [`nd_sim::VehicleModel`] (body, class, wear,
//! tuning, livery, paint) and drawn in its own frame: +X forward, +Y up, Z
//! across, origin on the ground under its centre. Bodies are side
//! silhouettes extruded across the width (flat sides keep panels, windows
//! and signs simple decals), with a narrower glass greenhouse on top, so a
//! handful of numbers per body type gives distinct shapes: a hatch's
//! upright tail, a saloon's boot, a coupe's fastback, a cyber-truck's single
//! wedge. Class bends the numbers: cheap cars sit taller and boxier with
//! black bumpers, rust, odd panels and a dead lamp; expensive ones lower and
//! raked with chrome and full-width light bars. Tuned cars get a spoiler,
//! wide wheels, neon side strips and underglow.
//!
//! Output is three meshes: the lit `body` (vertex-coloured), `lights`
//! (headlamps, tail lamps, lit signs: drawn with the shared glow material,
//! which is dimmed by day) and `fx` (underglow and light trails: additive,
//! night only). Every variant stays under [`TRI_BUDGET`] triangles; the
//! renderer caches one mesh set per variant, not per car.

use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::vehicle::{BodyType, Livery, VehicleClass, VehicleModel, PAINTS};
use rand::{Rng, SeedableRng};

/// Most triangles any variant may have (body, lights and fx together).
pub const TRI_BUDGET: usize = 600;

/// The meshes of one vehicle variant, in its own frame.
#[derive(Clone, Debug, Default)]
pub struct VehicleMesh {
    pub body: MeshData,
    pub lights: MeshData,
    pub fx: MeshData,
}

impl VehicleMesh {
    pub fn triangles(&self) -> usize {
        (self.body.indices.len() + self.lights.indices.len() + self.fx.indices.len()) / 3
    }
}

/// Paint colours (linear), indexed by `VehicleModel::paint`.
pub const PAINT: [[f32; 3]; PAINTS as usize] = [
    [0.025, 0.025, 0.03],
    [0.11, 0.12, 0.14],
    [0.60, 0.62, 0.66],
    [0.78, 0.78, 0.76],
    [0.04, 0.08, 0.24],
    [0.42, 0.03, 0.05],
    [0.03, 0.28, 0.30],
    [0.72, 0.40, 0.04],
];

/// Neon accents (HDR) per paint: underglow, transit stripes, side strips.
pub const NEON: [[f32; 3]; PAINTS as usize] = [
    [0.1, 1.8, 2.4],
    [2.4, 0.2, 1.8],
    [0.6, 2.4, 0.3],
    [1.2, 0.3, 2.6],
    [2.6, 0.4, 1.0],
    [0.2, 2.2, 1.8],
    [2.6, 1.0, 0.1],
    [0.3, 0.8, 2.8],
];

const GLASS: [f32; 3] = [0.05, 0.07, 0.10];
const TYRE: [f32; 3] = [0.035, 0.035, 0.04];
const TRIM: [f32; 3] = [0.06, 0.06, 0.07];
const CHROME: [f32; 3] = [0.85, 0.86, 0.9];
const RUST: [f32; 3] = [0.30, 0.12, 0.05];
const PRIMER: [f32; 3] = [0.36, 0.34, 0.31];
const HEAD: [f32; 3] = [2.8, 2.8, 2.5];
const TAIL: [f32; 3] = [2.4, 0.12, 0.12];
const AMBER: [f32; 3] = [2.4, 1.1, 0.1];
const RIDER: [f32; 3] = [0.10, 0.10, 0.12];
/// [taxis] Cab yellow: saturated enough to read as a cab from the top-down camera.
const TAXI_YELLOW: [f32; 3] = [1.0, 0.55, 0.0];

/// A number per livery for the variant's seed.
fn livery_code(l: Livery) -> u64 {
    match l {
        Livery::Corp(c, s) => 100 + c as u64 * 256 + s as u64,
        Livery::None => 0,
        Livery::Pizza => 1,
        Livery::Parcels => 2,
        Livery::Grocery => 3,
        Livery::Construction => 4,
        Livery::Utility => 5,
        Livery::Transit => 6,
        Livery::Taxi => 7,
    }
}

/// [megacorp] A brand colour, linear.
fn brand_rgb(c: u8) -> [f32; 3] {
    let b = nd_sim::corp::Brand { colour: c, ..Default::default() };
    b.rgb_f32()
}

/// [megacorp] The corp's mark (shape and monogram packed in `mark`, see
/// `World::corp_livery`) lit on both sides of the body, read the right way
/// round from each side, on a dark panel.
fn corp_logo(b: &mut B, x: f32, y: f32, hw: f32, size: f32, colour: u8, mark: u8) {
    use nd_sim::corp::{LogoPixel, LogoShape, LOGO_N};
    let shape = [LogoShape::Circle, LogoShape::Ring, LogoShape::Hexagon, LogoShape::Diamond, LogoShape::Triangle, LogoShape::Square, LogoShape::Chevron, LogoShape::Shield][(mark & 7) as usize];
    let monogram = (b'A' + (mark >> 3).min(25)) as char;
    let bmp = nd_sim::corp::Brand { colour, shape, monogram, name: String::new() }.logo_bitmap();
    let lit = mul(brand_rgb(colour), 2.6);
    let px = size / LOGO_N as f32;
    let half = (LOGO_N as f32 - 1.0) * 0.5;
    for s in [-1.0f32, 1.0] {
        b.body.decal(Vec3::new(x, y, s * (hw + 0.018)), Vec3::X, Vec3::Y, size + px, size + px, Vec3::Z * s, c4([0.02, 0.02, 0.025]));
        for (r, row) in bmp.iter().enumerate() {
            for (c, p) in row.iter().enumerate() {
                let col = match p {
                    LogoPixel::Shape => lit,
                    LogoPixel::LetterAlone => [2.6, 2.6, 2.5],
                    _ => continue,
                };
                // Viewer's right is +x on the +z side, -x on the -z side.
                let px_x = x + s * (c as f32 - half) * px;
                let px_y = y + (half - r as f32) * px;
                b.lights.decal(Vec3::new(px_x, px_y, s * (hw + 0.03)), Vec3::X, Vec3::Y, px * 1.02, px * 1.02, Vec3::Z * s, c4(col));
            }
        }
    }
}

/// Livery colours: body, accent, lit sign.
pub fn livery_colors(l: Livery, paint: u8, pal: &Palette) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    let neon = at(&pal.vehicle_neon, paint as usize);
    Some(match l {
        Livery::None => return None,
        Livery::Pizza => ([0.55, 0.04, 0.03], [0.9, 0.62, 0.05], [2.6, 1.3, 0.2]),
        Livery::Parcels => ([0.26, 0.15, 0.07], [0.9, 0.48, 0.05], [2.4, 1.0, 0.1]),
        Livery::Grocery => ([0.07, 0.32, 0.11], [0.85, 0.85, 0.8], [0.4, 2.4, 0.6]),
        Livery::Construction => ([0.85, 0.6, 0.04], [0.04, 0.04, 0.04], AMBER),
        Livery::Utility => ([0.82, 0.82, 0.8], [0.85, 0.3, 0.03], [2.4, 0.8, 0.1]),
        Livery::Transit => ([0.82, 0.84, 0.86], [neon[0] * 0.3, neon[1] * 0.3, neon[2] * 0.3], neon),
        // [taxis] Neon yellow, a black checker band, a hot lit roof sign.
        Livery::Taxi => (TAXI_YELLOW, [0.02, 0.02, 0.025], [3.2, 2.6, 0.5]),
        // [megacorp] A dark body tinted with the brand, the brand as accent
        // and lit sign.
        Livery::Corp(c, _) => {
            let k = brand_rgb(c);
            (mix(k, [0.05, 0.05, 0.06], 0.6), k, mul(k, 2.4))
        }
    })
}

/// Bumper-to-bumper length of a body, for parking and spacing.
pub fn body_length(b: BodyType) -> f32 {
    match b {
        BodyType::Hatchback => 3.9,
        BodyType::Sedan => 4.6,
        BodyType::Coupe => 4.6,
        BodyType::Suv => 4.8,
        BodyType::Pickup => 5.3,
        BodyType::CyberTruck => 5.7,
        BodyType::BoxVan => 6.4,
        BodyType::DeliveryVan => 5.4,
        BodyType::PanelTruck => 7.6,
        BodyType::Semi => 9.6,
        BodyType::GarbageTruck => 8.0,
        BodyType::BubbleBus => 11.6,
        BodyType::ArticulatedBus => 13.6,
        BodyType::Motorcycle => 2.1,
        BodyType::AkiraBike => 2.9,
        BodyType::Scooter => 1.7,
        BodyType::DeliveryBot => 0.8,
        BodyType::AirTaxi => 6.8,
        BodyType::Helicopter => 11.2,
        BodyType::TowTruck => 6.4,
        BodyType::HeavyTow => 9.8,
    }
}

/// The variant a model is drawn as. Everything that changes the mesh is in
/// the key; the renderer caches by it.
pub fn variant_key(m: &VehicleModel) -> VehicleModel {
    let mut k = *m;
    k.wear = k.wear.min(2);
    k.paint %= PAINTS;
    if k.body == BodyType::DeliveryBot || k.body == BodyType::AkiraBike || k.body.flies() {
        k.tuned = false;
    }
    k
}

/// Every body in every class, plus wear, tuning and liveries: the set the
/// tests check and the renderer can warm its cache with.
pub fn catalogue() -> Vec<VehicleModel> {
    let mut out = Vec::new();
    for body in BodyType::ALL {
        for class in [VehicleClass::Cheap, VehicleClass::Mid, VehicleClass::Expensive] {
            for (wear, tuned) in [(0u8, false), (2, false), (0, true)] {
                out.push(VehicleModel { class, body, wear, tuned, livery: Livery::None, paint: (body as u8 + wear) % PAINTS });
            }
        }
        for livery in Livery::ALL {
            out.push(VehicleModel { class: VehicleClass::Mid, body, wear: 1, tuned: false, livery, paint: 3 });
        }
    }
    out
}

/// Build a variant in the default palette's paints (see
/// [`vehicle_mesh_with`]).
pub fn vehicle_mesh(model: &VehicleModel) -> VehicleMesh {
    vehicle_mesh_with(model, crate::palette::default_palette())
}

/// Build a variant, its paint and neon from the palette.
pub fn vehicle_mesh_with(model: &VehicleModel, pal: &Palette) -> VehicleMesh {
    let m = variant_key(model);
    let paint = at(&pal.vehicle_paint, m.paint as usize);
    build(&m, paint, true, pal)
}

// ── builder ─────────────────────────────────────────────────────────────────

use crate::color::{at, mix, mul, rgba as c4};

struct B<'a> {
    m: &'a VehicleModel,
    pal: &'a Palette,
    body: MeshData,
    lights: MeshData,
    fx: MeshData,
    rng: rand_chacha::ChaCha8Rng,
    detail: bool,
}

impl B<'_> {
    /// A side silhouette `(x, y)` extruded from `-hw` to `hw` across.
    fn ext(&mut self, pts: &[(f32, f32)], hw: f32, col: [f32; 3]) {
        ext_into(&mut self.body, pts, hw, col);
    }

    fn bx(&mut self, c: (f32, f32, f32), s: (f32, f32, f32), col: [f32; 3]) {
        self.body.boxed(Vec3::new(c.0, c.1, c.2), Vec3::new(s.0, s.1, s.2), 0.0, c4(col));
    }

    /// The same panel on both flanks at `|z| = hw`.
    fn sides(&mut self, x: f32, y: f32, hw: f32, w: f32, h: f32, col: [f32; 3]) {
        for s in [-1.0f32, 1.0] {
            self.body.decal(Vec3::new(x, y, s * (hw + 0.012)), Vec3::X, Vec3::Y, w, h, Vec3::Z * s, c4(col));
        }
    }

    fn lit_sides(&mut self, x: f32, y: f32, hw: f32, w: f32, h: f32, col: [f32; 3]) {
        for s in [-1.0f32, 1.0] {
            self.lights.decal(Vec3::new(x, y, s * (hw + 0.02)), Vec3::X, Vec3::Y, w, h, Vec3::Z * s, c4(col));
        }
    }

    /// A lamp facing forward (`dir` +1) or back (-1) at `x`.
    fn lamp(&mut self, x: f32, y: f32, z: f32, w: f32, h: f32, dir: f32, col: [f32; 3]) {
        self.lights.decal(Vec3::new(x + dir * 0.02, y, z), Vec3::Z, Vec3::Y, w, h, Vec3::X * dir, c4(col));
    }

    /// A pair of lamps, mirrored across.
    fn lamps(&mut self, x: f32, y: f32, z: f32, w: f32, h: f32, dir: f32, col: [f32; 3]) {
        self.lamp(x, y, z, w, h, dir, col);
        self.lamp(x, y, -z, w, h, dir, col);
    }

    /// A pair of wheels on an axle at `x`, tyre outer faces at `|z| = hw`.
    fn axle(&mut self, x: f32, hw: f32, r: f32, w: f32) {
        if !self.detail {
            // Parked at low detail: one dark block per axle.
            self.body.boxed(Vec3::new(x, r * 0.9, 0.0), Vec3::new(r * 1.8, r * 1.8, hw * 2.0), 0.0, c4(TYRE));
            return;
        }
        let sides = 8;
        for s in [-1.0f32, 1.0] {
            let z = s * (hw - w * 0.5);
            self.body.pipe(Vec2::new(x, z), Vec2::Y, w, r, r, sides, c4(TYRE));
            let hub = if self.m.class == VehicleClass::Cheap && self.m.wear >= 2 { mul(TRIM, 1.5) } else if self.m.class == VehicleClass::Expensive || self.m.tuned { CHROME } else { [0.4, 0.4, 0.42] };
            self.body.decal(Vec3::new(x, r, s * (hw + 0.006)), Vec3::X, Vec3::Y, r * 1.1, r * 1.1, Vec3::Z * s, c4(hub));
        }
    }

    /// Underglow pad (fx): a bright core and a dim spill.
    fn underglow(&mut self, len: f32, hw: f32, col: [f32; 3]) {
        let y = 0.07;
        for (k, s) in [(1.0f32, 1.0f32), (0.35, 1.5)] {
            let (l, w) = (len * 0.5 * s.min(1.2), hw * s);
            self.fx.quad_facing([Vec3::new(-l, y, -w), Vec3::new(l, y, -w), Vec3::new(l, y, w), Vec3::new(-l, y, w)], Vec3::Y, c4(mul(col, k)));
        }
    }

    fn wear_marks(&mut self, x0: f32, x1: f32, y0: f32, y1: f32, hw: f32) {
        if self.m.wear == 0 || !self.detail {
            return;
        }
        let n = if self.m.wear >= 2 { 4 } else { 1 };
        for _ in 0..n {
            let x = self.rng.random_range(x0..x1);
            let y = self.rng.random_range(y0..y1);
            let side = if self.rng.random_bool(0.5) { 1.0 } else { -1.0 };
            let w = self.rng.random_range(0.2..0.6);
            let h = self.rng.random_range(0.1..0.3);
            self.body.decal(Vec3::new(x, y, side * (hw + 0.014)), Vec3::X, Vec3::Y, w, h, Vec3::Z * side, c4(RUST));
        }
    }
}

/// Extrude a side profile across the width.
fn ext_into(mesh: &mut MeshData, pts: &[(f32, f32)], hw: f32, col: [f32; 3]) {
    let a: Vec<Vec3> = pts.iter().map(|&(x, y)| Vec3::new(x, y, -hw)).collect();
    let b: Vec<Vec3> = pts.iter().map(|&(x, y)| Vec3::new(x, y, hw)).collect();
    mesh.frustum(&a, &b, c4(col));
}

/// A quad on a sloped face from `(x0, y0)` to `(x1, y1)` (a windscreen),
/// `hw` across, lifted along the face normal.
fn slope_quad(mesh: &mut MeshData, p0: (f32, f32), p1: (f32, f32), hw: f32, col: [f32; 3]) {
    let d = Vec2::new(p1.0 - p0.0, p1.1 - p0.1);
    let n = Vec2::new(d.y, -d.x).normalize_or_zero();
    // Face the outside: away from the body's centre line, which sits behind
    // and below a windscreen.
    let out = if n.x + n.y < 0.0 { -n } else { n };
    let o = out * 0.015;
    let a = Vec3::new(p0.0 + o.x, p0.1 + o.y, -hw);
    let b = Vec3::new(p1.0 + o.x, p1.1 + o.y, -hw);
    let c = Vec3::new(p1.0 + o.x, p1.1 + o.y, hw);
    let e = Vec3::new(p0.0 + o.x, p0.1 + o.y, hw);
    mesh.quad_facing([a, b, c, e], Vec3::new(out.x, out.y, 0.0), c4(col));
}

fn build(m: &VehicleModel, paint: [f32; 3], detail: bool, pal: &Palette) -> VehicleMesh {
    let seed = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in [m.body as u64, m.class as u64, m.wear as u64, m.tuned as u64, livery_code(m.livery), m.paint as u64] {
            h = (h ^ v).wrapping_mul(0x100_0000_01b3);
        }
        h
    };
    let mut b = B { m, pal, body: MeshData::default(), lights: MeshData::default(), fx: MeshData::default(), rng: rand_chacha::ChaCha8Rng::seed_from_u64(seed), detail };
    // Body colour: livery first, else paint, dulled by wear.
    let livery = livery_colors(m.livery, m.paint, pal);
    let mut col = livery.map(|l| l.0).unwrap_or(paint);
    if m.wear >= 2 {
        col = mix(col, [0.3, 0.29, 0.27], 0.35);
    } else if m.wear == 1 {
        col = mix(col, [0.3, 0.29, 0.27], 0.12);
    }
    match m.body {
        BodyType::Hatchback | BodyType::Sedan | BodyType::Coupe | BodyType::Suv | BodyType::Pickup => car(&mut b, col),
        BodyType::CyberTruck => cyber_truck(&mut b, col),
        BodyType::DeliveryVan => delivery_van(&mut b, col),
        BodyType::BoxVan | BodyType::PanelTruck => box_truck(&mut b, col),
        BodyType::Semi => semi(&mut b, col),
        BodyType::GarbageTruck => garbage_truck(&mut b, col),
        BodyType::BubbleBus => bubble_bus(&mut b, col, 0.0, 11.6, true),
        BodyType::ArticulatedBus => articulated_bus(&mut b, col),
        BodyType::Motorcycle | BodyType::Scooter => bike(&mut b, col),
        BodyType::AkiraBike => akira(&mut b),
        BodyType::DeliveryBot => bot(&mut b, col),
        BodyType::AirTaxi => air_taxi(&mut b, col),
        BodyType::Helicopter => helicopter(&mut b, col),
        BodyType::TowTruck => tow_truck(&mut b, col, false),
        BodyType::HeavyTow => tow_truck(&mut b, col, true),
    }
    if let Some((_, accent, sign)) = livery {
        livery_marks(&mut b, accent, sign);
    }
    VehicleMesh { body: b.body, lights: b.lights, fx: b.fx }
}

// ── cars ────────────────────────────────────────────────────────────────────

fn car(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let k = match m.class {
        VehicleClass::Cheap => 0.0,
        VehicleClass::Mid => 0.5,
        VehicleClass::Expensive => 1.0,
    };
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let mut hw = match m.body {
        BodyType::Hatchback => 0.84,
        BodyType::Suv | BodyType::Pickup => 0.97,
        BodyType::Coupe => 0.93,
        _ => 0.89,
    };
    // Cheap: taller and boxier; expensive: lower and raked.
    let tall = matches!(m.body, BodyType::Suv | BodyType::Pickup);
    let sill = if tall { 0.42 } else { 0.28 - 0.04 * k };
    let belt = if tall { 1.12 } else { 0.92 - 0.12 * k } + if m.body == BodyType::Coupe { -0.08 } else { 0.0 };
    let hood = belt - 0.06 - 0.08 * k;
    let roof = match m.body {
        BodyType::Suv | BodyType::Pickup => 1.82,
        BodyType::Coupe => 1.22,
        _ => 1.5 - 0.12 * k,
    };
    let nose = 0.05 + 0.35 * k; // how far the hood line sweeps back
    let (cab_f, roof_f) = match m.body {
        BodyType::Pickup => (1.05, 0.35),
        _ => (fx - 1.55 - 0.1 * k, fx - 2.45 - 0.35 * k + if m.body == BodyType::Hatchback { 0.45 } else { 0.0 }),
    };
    let (cab_r, roof_r) = match m.body {
        BodyType::Hatchback => (rx + 0.08, rx + 0.28 + 0.1 * k),
        BodyType::Sedan => (rx + 0.85 - 0.1 * k, rx + 1.35 + 0.05 * k),
        BodyType::Coupe => (rx + 0.45, rx + 1.6),
        BodyType::Suv => (rx + 0.06, rx + 0.18),
        _ => (-0.75, -0.65), // pickup cab
    };
    let rear_top = if matches!(m.body, BodyType::Sedan | BodyType::Coupe) { belt - 0.02 } else { belt };
    if m.wear >= 2 {
        hw -= 0.02;
    }
    // Lower body.
    let lower = [(rx, sill), (fx, sill), (fx + 0.03 - 0.1 * k, sill + (hood - sill) * 0.5), (fx - nose, hood), (cab_f, belt), (rx + 0.1, rear_top), (rx - 0.02, sill + (belt - sill) * 0.55)];
    b.ext(&lower, hw, col);
    // Greenhouse and roof.
    let ghw = hw - 0.1 - 0.04 * k;
    b.ext(&[(cab_r, belt), (cab_f, belt), (roof_f, roof), (roof_r, roof)], ghw, GLASS);
    if b.detail {
        b.ext(&[(roof_r - 0.03, roof - 0.02), (roof_f + 0.03, roof - 0.02), (roof_f - 0.02, roof + 0.05), (roof_r + 0.02, roof + 0.05)], ghw + 0.015, col);
    }
    // B-pillar.
    if b.detail {
        let px = (cab_f + cab_r) * 0.5 - 0.1;
        b.sides(px, (belt + roof) * 0.5, ghw, 0.09, roof - belt, col);
    }
    // Pickup bed: rails and a tailgate.
    if m.body == BodyType::Pickup {
        b.bx(((rx + cab_r) * 0.5, belt + 0.18, hw - 0.04), (cab_r - rx - 0.1, 0.36, 0.08), col);
        b.bx(((rx + cab_r) * 0.5, belt + 0.18, -(hw - 0.04)), (cab_r - rx - 0.1, 0.36, 0.08), col);
        b.bx((rx + 0.04, belt + 0.18, 0.0), (0.08, 0.36, hw * 2.0 - 0.1), col);
        b.bx(((rx + cab_r) * 0.5, belt + 0.02, 0.0), (cab_r - rx - 0.2, 0.04, hw * 2.0 - 0.2), TRIM);
    }
    // Wheels.
    let r = if tall { 0.4 } else { 0.32 + 0.03 * k };
    let (ww, whw) = if m.tuned { (0.34, hw + 0.08) } else { (0.24, hw + 0.01) };
    let ax = len * 0.5 - 0.85 - if tall { 0.1 } else { 0.0 };
    b.axle(ax, whw, r, ww);
    b.axle(-ax + 0.05, whw, r, ww);
    // Bumpers: black plastic on cheap cars, body colour otherwise.
    let bumper = if m.class == VehicleClass::Cheap { TRIM } else { mul(col, 0.9) };
    let by = sill + 0.12;
    if b.detail {
        b.bx((fx + 0.04, by, 0.0), (0.12, 0.2, hw * 2.0 - 0.04), bumper);
        b.bx((rx - 0.04, by, 0.0), (0.12, 0.2, hw * 2.0 - 0.04), bumper);
    }
    // Lamps.
    let ly = sill + (hood - sill) * 0.55;
    let front = fx + 0.03 - 0.1 * k;
    match m.class {
        VehicleClass::Expensive => {
            b.lamp(front - 0.02, ly + 0.05, 0.0, hw * 1.7, 0.06, 1.0, HEAD);
            b.lamp(rx - 0.02, ly + 0.08, 0.0, hw * 1.85, 0.08, -1.0, TAIL);
            b.sides(0.0, belt - 0.05, hw, len * 0.75, 0.035, CHROME);
        }
        VehicleClass::Mid => {
            b.lamps(front, ly, hw - 0.22, 0.34, 0.14, 1.0, HEAD);
            b.lamps(rx - 0.02, ly + 0.05, hw - 0.18, 0.3, 0.14, -1.0, TAIL);
        }
        VehicleClass::Cheap => {
            // Round-ish old lamps; a beat-up one has a dead headlamp.
            b.lamp(front, ly, hw - 0.25, 0.26, 0.2, 1.0, HEAD);
            let dead = m.wear >= 2;
            b.lamp(front, ly, -(hw - 0.25), 0.26, 0.2, 1.0, if dead { [0.05, 0.05, 0.05] } else { HEAD });
            b.lamps(rx - 0.02, ly, hw - 0.2, 0.22, 0.22, -1.0, TAIL);
        }
    }
    // Beat-up: a primer door, rust round the arches, dents.
    if m.class == VehicleClass::Cheap && m.wear >= 1 && b.detail {
        let door = if b.rng.random_bool(0.5) { PRIMER } else { { let n = b.pal.vehicle_paint.len().max(1); mul(at(&b.pal.vehicle_paint, b.rng.random_range(0..n)), 0.85) } };
        let side = if b.rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let dx = (cab_f + cab_r) * 0.5 + 0.35;
        b.body.decal(Vec3::new(dx, (sill + belt) * 0.5 + 0.04, side * (hw + 0.012)), Vec3::X, Vec3::Y, 0.9, belt - sill - 0.12, Vec3::Z * side, c4(door));
    }
    b.wear_marks(rx + 0.2, fx - 0.2, sill + 0.05, belt - 0.1, hw);
    // Tuned: spoiler, neon strips, underglow.
    if m.tuned {
        let sy = rear_top + 0.32;
        b.bx((rx + 0.25, sy, 0.0), (0.32, 0.05, hw * 2.0 - 0.1), TRIM);
        for z in [hw - 0.2, -(hw - 0.2)] {
            b.bx((rx + 0.3, (rear_top + sy) * 0.5, z), (0.08, sy - rear_top, 0.05), TRIM);
        }
        let neon = at(&b.pal.vehicle_neon, m.paint as usize);
        b.lit_sides(0.0, sill + 0.04, hw, len * 0.7, 0.04, neon);
        b.underglow(len * 0.85, hw * 1.05, neon);
    }
}

fn cyber_truck(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let steel = if m.livery == Livery::None && m.paint <= 3 { [0.55, 0.57, 0.6] } else { col };
    let hw = 1.0;
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    // One wedge: low nose, apex over the front seats, a long flat slope to
    // the tailgate.
    b.ext(&[(rx, 0.5), (fx, 0.5), (fx + 0.02, 1.0), (0.25, 1.95), (rx, 1.3)], hw, steel);
    // Glass: windscreen on the front slope, side band.
    slope_quad(&mut b.body, (1.6, 1.43), (0.35, 1.9), hw - 0.1, GLASS);
    b.sides(-0.2, 1.55, hw, 2.4, 0.32, GLASS);
    // Black arch flares and a skirt.
    b.ext(&[(rx + 0.1, 0.35), (fx - 0.1, 0.35), (fx - 0.1, 0.62), (rx + 0.1, 0.62)], hw + 0.02, TRIM);
    let r = 0.44;
    b.axle(1.85, hw + 0.06, r, 0.3);
    b.axle(-1.85, hw + 0.06, r, 0.3);
    // Full-width light bars.
    b.lamp(fx + 0.02, 0.98, 0.0, hw * 1.9, 0.05, 1.0, HEAD);
    b.lamp(rx, 1.22, 0.0, hw * 1.9, 0.06, -1.0, TAIL);
    if m.tuned {
        let neon = at(&b.pal.vehicle_neon, m.paint as usize);
        b.underglow(len * 0.85, hw * 1.05, neon);
        b.lit_sides(0.0, 0.66, hw + 0.02, len * 0.6, 0.04, neon);
    }
    b.wear_marks(rx + 0.3, fx - 0.3, 0.7, 1.2, hw);
}

// ── vans and lorries ────────────────────────────────────────────────────────

fn delivery_van(b: &mut B, col: [f32; 3]) {
    let len = body_length(BodyType::DeliveryVan);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = 1.0;
    b.ext(&[(rx, 0.38), (fx, 0.38), (fx + 0.04, 0.95), (fx - 0.4, 1.2), (fx - 1.15, 2.45), (rx, 2.5)], hw, col);
    slope_quad(&mut b.body, (fx - 0.42, 1.24), (fx - 1.1, 2.38), hw - 0.08, GLASS);
    b.sides(fx - 1.25, 1.75, hw, 0.7, 0.6, GLASS);
    b.axle(fx - 0.9, hw + 0.02, 0.38, 0.26);
    b.axle(rx + 1.0, hw + 0.02, 0.38, 0.26);
    b.bx((fx + 0.05, 0.5, 0.0), (0.12, 0.22, hw * 2.0), TRIM);
    b.lamps(fx + 0.02, 0.9, hw - 0.2, 0.32, 0.14, 1.0, HEAD);
    b.lamps(rx, 0.9, hw - 0.1, 0.12, 0.5, -1.0, TAIL);
    // Rear doors split line.
    b.lamp(rx - 0.005, 1.45, 0.0, 0.03, 1.8, -1.0, mul(col, 0.4));
    b.wear_marks(rx + 0.3, fx - 0.5, 0.5, 2.0, hw);
}

/// Box van (cab and a box) and panel truck (longer, ribbed, roller door).
fn box_truck(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let panel = m.body == BodyType::PanelTruck;
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = if panel { 1.2 } else { 1.05 };
    let cab_len = 1.8;
    let cab_col = if m.livery == Livery::None { mix(col, [0.8, 0.8, 0.82], 0.5) } else { col };
    // Cab.
    let cr = fx - cab_len;
    b.ext(&[(cr, 0.55), (fx, 0.55), (fx + 0.03, 1.15), (fx - 0.25, 1.45), (fx - 0.55, 2.4), (cr, 2.45)], hw - 0.05, cab_col);
    slope_quad(&mut b.body, (fx - 0.27, 1.5), (fx - 0.53, 2.33), hw - 0.12, GLASS);
    b.sides(fx - 0.95, 1.85, hw - 0.05, 0.7, 0.6, GLASS);
    // Box.
    let top = if panel { 3.5 } else { 3.0 };
    let box_col = if m.livery == Livery::None { [0.78, 0.78, 0.76] } else { col };
    b.ext(&[(rx, 0.75), (cr - 0.08, 0.75), (cr - 0.08, top), (rx, top)], hw, box_col);
    // Chassis rail.
    b.bx(((rx + fx) * 0.5, 0.6, 0.0), (len - 0.4, 0.25, hw * 1.4), TRIM);
    if panel && b.detail {
        for i in 1..6 {
            let x = rx + (cr - rx) * i as f32 / 6.0;
            b.sides(x, (0.75 + top) * 0.5, hw, 0.05, top - 0.85, mul(box_col, 0.8));
        }
        // Roller door slats.
        b.lamp(rx - 0.005, (0.75 + top) * 0.5, 0.0, hw * 1.8, top - 0.9, -1.0, mul(box_col, 0.35));
    }
    let r = 0.48;
    b.axle(fx - 0.9, hw + 0.02, r, 0.3);
    b.axle(rx + 1.1, hw + 0.02, r, 0.3);
    if panel {
        b.axle(rx + 2.2, hw + 0.02, r, 0.3);
    }
    b.lamps(fx + 0.02, 1.0, hw - 0.25, 0.3, 0.16, 1.0, HEAD);
    b.lamps(rx, 0.95, hw - 0.15, 0.25, 0.15, -1.0, TAIL);
    b.lamps(rx, top - 0.1, hw - 0.1, 0.08, 0.08, -1.0, AMBER);
    b.wear_marks(rx + 0.3, cr - 0.3, 1.0, top - 0.3, hw);
}

fn semi(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let len = body_length(BodyType::Semi);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = 1.22;
    // Cab-over tractor, flat face.
    let cr = fx - 2.2;
    let cab_col = if m.livery == Livery::None { col } else { livery_colors(m.livery, m.paint, b.pal).map_or(col, |l| l.0) };
    b.ext(&[(cr, 0.7), (fx, 0.7), (fx + 0.05, 2.6), (fx - 0.3, 3.15), (cr, 3.2)], hw - 0.05, cab_col);
    b.body.decal(Vec3::new(fx + 0.07, 2.05, 0.0), Vec3::Z, Vec3::Y, hw * 1.7, 0.8, Vec3::X, c4(GLASS));
    b.sides(fx - 0.6, 2.2, hw - 0.05, 0.6, 0.55, GLASS);
    // Container on a skeletal trailer. [r4] (K33) The town's standard
    // twenty-foot box (`CONTAINER_20` by `CONTAINER_W` by `CONTAINER_H`),
    // in a shipping line's colours and mark.
    let cfront = (rx + crate::CONTAINER_20).min(cr - 0.35);
    let line = &crate::SHIPPING_LINES[b.rng.random_range(0..crate::SHIPPING_LINES.len())];
    let ccol = line.body;
    let (cb, ct) = (1.15, 1.15 + crate::CONTAINER_H);
    b.ext(&[(rx, cb), (cfront, cb), (cfront, ct), (rx, ct)], crate::CONTAINER_W * 0.5, ccol);
    if b.detail {
        for i in 1..8 {
            let x = rx + (cfront - rx) * i as f32 / 8.0;
            b.sides(x, (cb + ct) * 0.5, crate::CONTAINER_W * 0.5, 0.06, crate::CONTAINER_H - 0.1, mul(ccol, 0.7));
        }
        if line.band {
            b.sides((rx + cfront) * 0.5, ct - 0.55, crate::CONTAINER_W * 0.5 + 0.004, cfront - rx - 0.6, 0.36, line.mark);
        } else {
            b.sides(rx + 1.4, (cb + ct) * 0.5 + 0.2, crate::CONTAINER_W * 0.5 + 0.004, 1.8, 0.9, line.mark);
        }
        b.lamp(rx - 0.005, (cb + ct) * 0.5, 0.0, 0.04, crate::CONTAINER_H - 0.1, -1.0, mul(ccol, 0.5));
    }
    b.bx(((rx + cfront) * 0.5 + 0.5, 0.95, 0.0), (cfront - rx, 0.3, hw * 1.2), TRIM);
    let r = 0.5;
    b.axle(fx - 0.75, hw, r, 0.3);
    b.axle(cr - 0.2, hw, r, 0.3);
    b.axle(rx + 1.0, hw, r, 0.3);
    b.axle(rx + 2.1, hw, r, 0.3);
    b.lamps(fx + 0.05, 1.0, hw - 0.3, 0.32, 0.16, 1.0, HEAD);
    b.lamps(fx + 0.02, 3.2, hw - 0.3, 0.1, 0.06, 1.0, AMBER);
    b.lamps(rx, 1.2, hw - 0.2, 0.3, 0.14, -1.0, TAIL);
    b.wear_marks(rx + 0.4, cfront - 0.4, 1.4, 3.4, hw);
}

/// [tow] A wrecker: a conventional cab, a low flat deck with tool lockers,
/// a boom raked up over the back with its cable and hook, and the wheel
/// lift (an L-arm a car's front wheels sit in) low behind; an amber light
/// bar on the cab roof. The heavy one is longer, on three axles, with a
/// longer rotator boom and outriggers folded at the back.
fn tow_truck(b: &mut B, col: [f32; 3], heavy: bool) {
    let m = *b.m;
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = if heavy { 1.25 } else { 1.05 };
    let cab_len = if heavy { 2.3 } else { 1.9 };
    let cr = fx - cab_len;
    let (sill, roof) = if heavy { (0.75, 3.0) } else { (0.6, 2.35) };
    // Cab with a short bonnet.
    b.ext(&[(cr, sill), (fx, sill), (fx + 0.04, sill + 0.55), (fx - 0.55, sill + 0.75), (fx - 0.85, roof - 0.05), (cr + 0.05, roof)], hw - 0.05, col);
    slope_quad(&mut b.body, (fx - 0.58, sill + 0.8), (fx - 0.84, roof - 0.12), hw - 0.12, GLASS);
    b.sides(cr + (fx - cr) * 0.45, (sill + roof) * 0.5 + 0.35, hw - 0.05, 0.7, 0.55, GLASS);
    // Amber light bar across the roof.
    let bar = roof + 0.09;
    b.bx((cr + 0.6, roof + 0.04, 0.0), (0.3, 0.06, hw * 1.6), TRIM);
    b.lights.boxed(Vec3::new(cr + 0.6, bar, 0.0), Vec3::new(0.24, 0.12, hw * 1.5), 0.0, c4(AMBER));
    // Chassis and the deck behind the cab.
    let deck = sill + 0.35;
    b.bx(((rx + cr) * 0.5, sill + 0.1, 0.0), (cr - rx, 0.3, hw * 1.2), TRIM);
    b.bx(((rx + cr) * 0.5 + 0.2, deck, 0.0), (cr - rx - 0.4, 0.12, hw * 2.0 - 0.1), mul(col, 0.8));
    // Tool lockers down both sides.
    let lockers = if heavy { 2.6 } else { 1.4 };
    for s in [-1.0f32, 1.0] {
        b.bx((cr - 0.15 - lockers * 0.5, deck + 0.35, s * (hw - 0.22)), (lockers, 0.6, 0.42), col);
    }
    // The boom: raked up from behind the cab to over the back, cable and
    // hook hanging off its end.
    let boom = 0.18;
    let (bx0, by0) = (cr - 0.3, deck + 0.1);
    let (bx1, by1) = (rx + if heavy { 0.6 } else { 0.35 }, if heavy { 3.3 } else { 2.55 });
    b.ext(&[(bx0, by0), (bx0 + 0.45, by0), (bx1 + 0.2, by1 - 0.15), (bx1, by1)], boom, [0.12, 0.12, 0.13]);
    b.bx((bx1 - 0.05, (by1 + deck) * 0.5, 0.0), (0.04, by1 - deck - 0.2, 0.04), CHROME);
    b.bx((bx1 - 0.05, deck + 0.12, 0.0), (0.16, 0.2, 0.1), [0.85, 0.5, 0.05]);
    // The wheel lift: an arm low out the back with its cross bar.
    b.bx((rx + 0.25, 0.4, 0.0), (0.7, 0.12, 0.2), TRIM);
    b.bx((rx - 0.05, 0.4, 0.0), (0.16, 0.14, hw * 1.5), [0.85, 0.5, 0.05]);
    if heavy {
        // Outriggers folded at the rear corners.
        for s in [-1.0f32, 1.0] {
            b.bx((rx + 0.5, sill + 0.15, s * (hw - 0.1)), (0.5, 0.5, 0.2), [0.85, 0.5, 0.05]);
        }
    }
    // Chevrons on the back.
    if b.detail {
        for i in 0..4 {
            b.lamp(rx - 0.01, sill + 0.2 + i as f32 * 0.12, 0.0, hw * 1.6, 0.05, -1.0, if i % 2 == 0 { AMBER } else { [0.05, 0.05, 0.05] });
        }
    }
    let r = if heavy { 0.52 } else { 0.42 };
    b.axle(fx - 0.85, hw, r, 0.3);
    b.axle(rx + 1.2, hw, r, 0.34);
    if heavy {
        b.axle(rx + 2.4, hw, r, 0.34);
    }
    b.lamps(fx + 0.04, sill + 0.35, hw - 0.25, 0.3, 0.16, 1.0, HEAD);
    b.lamps(rx, sill + 0.15, hw - 0.15, 0.22, 0.14, -1.0, TAIL);
    b.wear_marks(rx + 0.3, cr - 0.2, sill + 0.2, deck + 0.6, hw);
}

fn garbage_truck(b: &mut B, col: [f32; 3]) {
    let len = body_length(BodyType::GarbageTruck);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let hw = 1.2;
    let cr = fx - 1.9;
    b.ext(&[(cr, 0.6), (fx, 0.6), (fx + 0.05, 2.2), (fx - 0.25, 2.75), (cr, 2.8)], hw - 0.05, [0.82, 0.82, 0.8]);
    b.body.decal(Vec3::new(fx + 0.07, 1.85, 0.0), Vec3::Z, Vec3::Y, hw * 1.7, 0.7, Vec3::X, c4(GLASS));
    // Compactor body with a rounded top and a hopper.
    let body_col = mix(col, [0.15, 0.32, 0.12], if b.m.livery == Livery::Utility { 0.0 } else { 0.6 });
    b.ext(&[(rx + 0.9, 0.8), (cr - 0.1, 0.8), (cr - 0.1, 3.0), (cr - 0.5, 3.35), (rx + 1.3, 3.35), (rx + 0.9, 3.0)], hw, body_col);
    b.ext(&[(rx, 0.7), (rx + 0.9, 0.7), (rx + 0.9, 3.2), (rx + 0.3, 2.9), (rx, 2.0)], hw - 0.05, mul(body_col, 0.7));
    b.bx(((rx + fx) * 0.5, 0.6, 0.0), (len - 0.4, 0.25, hw * 1.4), TRIM);
    let r = 0.5;
    b.axle(fx - 0.8, hw, r, 0.3);
    b.axle(rx + 1.6, hw, r, 0.3);
    b.axle(rx + 2.7, hw, r, 0.3);
    b.lamps(fx + 0.05, 1.0, hw - 0.3, 0.3, 0.16, 1.0, HEAD);
    b.lamps(fx - 0.6, 2.85, 0.4, 0.2, 0.12, 1.0, AMBER);
    b.lamps(rx, 1.0, hw - 0.2, 0.25, 0.25, -1.0, TAIL);
    b.wear_marks(rx + 1.0, cr - 0.3, 1.0, 3.0, hw);
}

// ── buses ───────────────────────────────────────────────────────────────────

/// A rounded driverless pod: glass band all round, lidar puck, no cab.
/// Drawn centred at `x0`, `len` long; `ends` rounds both ends (an
/// articulated section has a flat inner end).
fn bubble_bus(b: &mut B, col: [f32; 3], x0: f32, len: f32, ends: bool) {
    let m = *b.m;
    let hw = 1.27;
    let h = len * 0.5;
    let (fx, rx) = (x0 + h, x0 - h);
    let round = |x_end: f32, s: f32| -> Vec<(f32, f32)> {
        // Profile corner from the floor up to the roof at one end; s is +1
        // for the front, -1 for the back.
        vec![(x_end + s * 0.05, 0.75), (x_end + s * 0.02, 1.8), (x_end - s * 0.3, 2.6), (x_end - s * 0.9, 3.05)]
    };
    let mut prof = vec![(rx + 0.2, 0.32), (fx - 0.2, 0.32)];
    if ends || x0 > 0.0 {
        prof.extend(round(fx, 1.0));
    } else {
        prof.extend([(fx, 0.75), (fx, 3.05)]);
    }
    let mut back: Vec<(f32, f32)> = if ends || x0 < 0.0 { round(rx, -1.0) } else { vec![(rx, 0.75), (rx, 3.05)] };
    back.reverse();
    prof.extend(back);
    b.ext(&prof, hw, col);
    // Wraparound glass band, a hair outside the body.
    let gb = |x_end: f32, s: f32, rounded: bool| -> Vec<(f32, f32)> {
        if rounded {
            vec![(x_end + s * 0.04, 1.2), (x_end + s * 0.035, 1.8), (x_end - s * 0.28, 2.55)]
        } else {
            vec![(x_end, 1.2), (x_end, 2.55)]
        }
    };
    let mut band = gb(fx, 1.0, ends || x0 > 0.0);
    let mut bb = gb(rx, -1.0, ends || x0 < 0.0);
    bb.reverse();
    band.extend(bb);
    ext_into(&mut b.body, &band, hw + 0.015, GLASS);
    // Roof crown and the route-colour skirt stripe.
    b.ext(&[(rx + 1.2, 3.0), (fx - 1.2, 3.0), (fx - 1.6, 3.22), (rx + 1.6, 3.22)], hw - 0.3, mul(col, 0.95));
    let accent = livery_colors(Livery::Transit, m.paint, b.pal).map_or(at(&b.pal.vehicle_neon, 0), |l| l.2);
    b.sides(x0, 0.9, hw, len - 1.2, 0.12, mul(accent, 0.35));
    b.lit_sides(x0, 2.62, hw, len - 1.4, 0.05, accent);
    // Wheels behind skirts.
    let r = 0.45;
    b.axle(fx - 1.8, hw - 0.04, r, 0.3);
    b.axle(rx + 1.8, hw - 0.04, r, 0.3);
    if ends || x0 > 0.0 {
        // Lidar puck, LED face, destination strip.
        let px = fx - 1.1;
        b.body.cylinder(Vec2::new(px, 0.0), 0.28, 0.24, 3.0, 3.4, 8, c4([0.08, 0.08, 0.1]));
        b.lights.cylinder(Vec2::new(px, 0.0), 0.25, 0.25, 3.25, 3.3, 8, c4(accent));
        b.lamp(fx + 0.05, 0.95, 0.0, hw * 1.5, 0.08, 1.0, HEAD);
        b.lamp(fx - 0.3, 2.7, 0.0, hw * 1.1, 0.14, 1.0, accent);
    }
    if ends || x0 < 0.0 {
        b.lamp(rx - 0.05, 0.95, 0.0, hw * 1.5, 0.1, -1.0, TAIL);
    }
}

fn articulated_bus(b: &mut B, col: [f32; 3]) {
    let len = body_length(BodyType::ArticulatedBus);
    let sec = (len - 0.7) * 0.5;
    bubble_bus(b, col, len * 0.5 - sec * 0.5, sec, false);
    bubble_bus(b, col, -len * 0.5 + sec * 0.5, sec, false);
    // Bellows.
    b.ext(&[(-0.4, 0.5), (0.4, 0.5), (0.4, 2.95), (-0.4, 2.95)], 1.18, [0.08, 0.08, 0.09]);
    if b.detail {
        for x in [-0.2f32, 0.0, 0.2] {
            b.sides(x, 1.7, 1.18, 0.04, 2.3, [0.18, 0.18, 0.2]);
        }
    }
}

// ── two wheels and robots ───────────────────────────────────────────────────

/// A rider sitting at `x`, leaning `lean` (0 upright, 1 flat on the tank).
fn rider(b: &mut B, x: f32, seat: f32, lean: f32, helmet: [f32; 3]) {
    let jacket = RIDER;
    // Legs (thigh forward to the pegs), torso, arms, helmet.
    b.bx((x + 0.18, seat + 0.02, 0.0), (0.5, 0.16, 0.34), mul(jacket, 0.8));
    let th = 0.62 - 0.25 * lean;
    let tx = x - 0.05 + 0.25 * lean;
    b.bx((tx, seat + 0.1 + th * 0.5, 0.0), (0.3 + 0.2 * lean, th, 0.38), jacket);
    b.bx((tx + 0.3 + 0.1 * lean, seat + th * 0.85, 0.0), (0.5, 0.09, 0.48), jacket);
    let hy = seat + 0.15 + th + 0.12;
    b.bx((tx + 0.12 + 0.25 * lean, hy, 0.0), (0.28, 0.26, 0.25), helmet);
    if b.detail {
        b.body.decal(Vec3::new(tx + 0.27 + 0.25 * lean, hy + 0.02, 0.0), Vec3::Z, Vec3::Y, 0.2, 0.08, Vec3::X, c4(GLASS));
    }
}

fn bike(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let scooter = m.body == BodyType::Scooter;
    let len = body_length(m.body);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let r = if scooter { 0.22 } else { 0.32 };
    let wheel = |b: &mut B, x: f32| {
        // 14 sides: an 8-sided wheel reads as a nut close up. [round-wheels]
        b.body.pipe(Vec2::new(x, 0.0), Vec2::Y, 0.12, r, r, 14, c4(TYRE));
    };
    wheel(b, fx - r);
    wheel(b, rx + r);
    if scooter {
        // Floorboard, apron, seat, handlebar column.
        b.bx((-0.1, 0.32, 0.0), (0.8, 0.08, 0.3), col);
        b.ext(&[(fx - 0.5, 0.28), (fx - 0.3, 0.3), (fx - 0.42, 1.0), (fx - 0.55, 1.0)], 0.2, col);
        b.ext(&[(rx + 0.1, 0.3), (rx + 0.8, 0.3), (rx + 0.75, 0.62), (rx + 0.2, 0.66)], 0.18, col);
        b.bx((fx - 0.45, 1.05, 0.0), (0.06, 0.06, 0.6), TRIM);
        let helmet = if m.livery == Livery::None { mix(col, [0.8, 0.8, 0.8], 0.4) } else { livery_colors(m.livery, m.paint, b.pal).map_or(col, |l| l.1) };
        rider(b, rx + 0.5, 0.66, 0.0, helmet);
        b.lamp(fx - 0.28, 0.85, 0.0, 0.14, 0.1, 1.0, HEAD);
        b.lamp(rx + 0.08, 0.55, 0.0, 0.14, 0.08, -1.0, TAIL);
        // A delivery top box.
        if m.livery != Livery::None {
            b.bx((rx + 0.25, 0.95, 0.0), (0.42, 0.4, 0.42), livery_colors(m.livery, m.paint, b.pal).map_or(col, |l| l.0));
        }
    } else {
        // Tank, seat and tail; engine block; forks; bars.
        b.ext(&[(-0.75, 0.62), (0.4, 0.58), (0.62, 0.92), (0.25, 1.02), (-0.35, 0.86), (-0.85, 0.9)], 0.15, col);
        b.bx((0.0, 0.42, 0.0), (0.55, 0.32, 0.26), [0.18, 0.18, 0.2]);
        b.body.tube(Vec3::new(fx - r, r, 0.0), Vec3::new(0.55, 1.0, 0.0), 0.04, 4, c4(CHROME));
        b.bx((0.55, 1.05, 0.0), (0.05, 0.05, 0.62), TRIM);
        let helmet = if m.class == VehicleClass::Cheap { [0.3, 0.3, 0.3] } else { mix(col, [1.0, 1.0, 1.0], 0.2) };
        rider(b, -0.35, 0.86, 0.35, helmet);
        b.lamp(0.66, 0.9, 0.0, 0.16, 0.14, 1.0, HEAD);
        b.lamp(-0.86, 0.86, 0.0, 0.14, 0.06, -1.0, TAIL);
        if m.tuned {
            let neon = at(&b.pal.vehicle_neon, m.paint as usize);
            b.underglow(1.5, 0.3, neon);
        }
    }
    if m.wear >= 2 && b.detail {
        b.body.decal(Vec3::new(-0.1, 0.75, 0.16), Vec3::X, Vec3::Y, 0.3, 0.1, Vec3::Z, c4(RUST));
    }
}

/// Kaneda's bike, more or less: long, low, red, a rider lying flat.
fn akira(b: &mut B) {
    let red = [0.62, 0.02, 0.02];
    let len = body_length(BodyType::AkiraBike);
    let (fx, rx) = (len * 0.5, -len * 0.5);
    let r = 0.36;
    for x in [fx - 0.45, rx + 0.42] {
        b.body.pipe(Vec2::new(x, 0.0), Vec2::Y, 0.22, r, r, 10, c4(TYRE));
        for s in [-1.0f32, 1.0] {
            b.body.decal(Vec3::new(x, r, s * 0.115), Vec3::X, Vec3::Y, 0.5, 0.5, Vec3::Z * s, c4(red));
        }
    }
    // Faired body: nose cone to a high tail.
    b.ext(&[(rx + 0.05, 0.3), (fx - 0.1, 0.32), (fx + 0.02, 0.55), (fx - 0.35, 0.92), (0.15, 0.82), (-0.6, 0.72), (rx + 0.25, 0.95), (rx - 0.02, 0.6)], 0.27, red);
    // Windscreen bubble and white trim.
    slope_quad(&mut b.body, (fx - 0.33, 0.92), (fx - 0.75, 1.08), 0.16, GLASS);
    b.sides(0.2, 0.5, 0.27, 1.6, 0.06, [0.85, 0.85, 0.82]);
    b.sides(-0.3, 0.62, 0.27, 0.5, 0.14, [0.85, 0.85, 0.82]);
    rider(b, -0.55, 0.72, 0.85, [0.75, 0.75, 0.72]);
    // Twin headlamps, a long tail lamp, glowing rim strips.
    b.lamps(fx + 0.02, 0.58, 0.1, 0.1, 0.08, 1.0, HEAD);
    b.lamp(rx - 0.02, 0.72, 0.0, 0.4, 0.06, -1.0, TAIL);
    b.lit_sides(0.0, 0.34, 0.27, len * 0.75, 0.04, [2.6, 0.15, 0.1]);
    b.underglow(len * 0.8, 0.35, [2.0, 0.08, 0.06]);
}

/// Pavement delivery robot: a rounded white box on six little wheels, a
/// lid stripe, LED eyes and a tall flag.
fn bot(b: &mut B, col: [f32; 3]) {
    let m = *b.m;
    let shell = if m.livery == Livery::None { [0.82, 0.82, 0.8] } else { mix(col, [0.85, 0.85, 0.85], 0.35) };
    let accent = livery_colors(m.livery, m.paint, b.pal).map_or(at(&b.pal.vehicle_neon, m.paint as usize), |l| l.2);
    let hw = 0.27;
    b.ext(&[(-0.4, 0.14), (0.4, 0.14), (0.42, 0.3), (0.36, 0.5), (0.25, 0.56), (-0.36, 0.56), (-0.42, 0.42)], hw, shell);
    b.ext(&[(-0.34, 0.56), (0.24, 0.56), (0.2, 0.6), (-0.3, 0.6)], hw - 0.04, mul(shell, 0.85));
    for x in [-0.28f32, 0.0, 0.28] {
        for s in [-1.0f32, 1.0] {
            b.body.pipe(Vec2::new(x, s * (hw - 0.01)), Vec2::Y, 0.07, 0.1, 0.1, 6, c4(TYRE));
        }
    }
    // Flag on a whip.
    b.bx((-0.3, 0.6 + 0.55, 0.2), (0.015, 1.1, 0.015), [0.7, 0.7, 0.7]);
    b.body.decal(Vec3::new(-0.2, 1.55, 0.2), Vec3::X, Vec3::Y, 0.2, 0.14, Vec3::Z, c4([0.95, 0.35, 0.05]));
    b.body.decal(Vec3::new(-0.2, 1.55, 0.2), Vec3::X, Vec3::Y, 0.2, 0.14, -Vec3::Z, c4([0.95, 0.35, 0.05]));
    b.lights.boxed(Vec3::new(-0.3, 1.72, 0.2), Vec3::splat(0.05), 0.0, c4(AMBER));
    // Eyes and a status stripe.
    b.lamps(0.425, 0.38, 0.1, 0.1, 0.05, 1.0, [0.6, 2.2, 2.6]);
    b.lamp(-0.425, 0.32, 0.0, 0.3, 0.04, -1.0, TAIL);
    b.lit_sides(0.0, 0.47, hw, 0.6, 0.03, accent);
    b.fx.quad_facing([Vec3::new(-0.5, 0.05, -0.38), Vec3::new(0.5, 0.05, -0.38), Vec3::new(0.5, 0.05, 0.38), Vec3::new(-0.5, 0.05, 0.38)], Vec3::Y, c4(mul(accent, 0.5)));
}

// ── aircraft ────────────────────────────────────────────────────────────────

const NAV_RED: [f32; 3] = [2.8, 0.1, 0.08];
const NAV_GREEN: [f32; 3] = [0.1, 2.6, 0.5];
const STROBE: [f32; 3] = [3.0, 3.0, 3.0];

/// Four-fan eVTOL taxi: a teardrop cabin with a wraparound canopy, two
/// skids, arms out to four ducted fans with lit rims, nav lights (red one
/// side, green the other, a white strobe aft) and a glow underneath at
/// night. Drawn on its skids; the renderer lifts it.
fn air_taxi(b: &mut B, col: [f32; 3]) {
    let hw = 0.8;
    let accent = at(&b.pal.vehicle_neon, b.m.paint as usize);
    b.ext(&[(-1.7, 0.55), (1.3, 0.55), (1.9, 0.95), (1.75, 1.35), (0.6, 1.85), (-1.1, 1.8), (-1.75, 1.2)], hw, col);
    // Canopy: glass across the front and down both sides.
    slope_quad(&mut b.body, (1.75, 1.35), (0.6, 1.85), hw * 0.85, GLASS);
    b.sides(0.2, 1.4, hw, 1.9, 0.5, GLASS);
    b.lit_sides(-0.2, 0.85, hw, 2.6, 0.06, accent);
    // Skids on struts.
    for z in [-0.7f32, 0.7] {
        b.bx((0.0, 0.04, z), (2.8, 0.08, 0.1), TRIM);
        for x in [-0.8f32, 0.8] {
            b.bx((x, 0.3, z * 0.9), (0.08, 0.5, 0.08), TRIM);
        }
    }
    // Arms to the fans, fans in ducts.
    for (x, z) in [(2.25f32, 2.1f32), (2.25, -2.1), (-2.25, 2.1), (-2.25, -2.1)] {
        let mid = (x * 0.5, 1.55, z * 0.5);
        let len = (x * x + z * z).sqrt();
        b.body.boxed(Vec3::new(mid.0, mid.1, mid.2), Vec3::new(len, 0.12, 0.18), z.atan2(x), c4(mul(col, 0.7)));
        b.body.cylinder(Vec2::new(x, z), 0.95, 0.95, 1.45, 1.75, 10, c4(mul(col, 0.8)));
        b.body.cylinder(Vec2::new(x, z), 0.85, 0.85, 1.56, 1.6, 10, c4([0.05, 0.05, 0.06]));
        b.lights.boxed(Vec3::new(x + 0.95 * x.signum() * 0.7, 1.75, z + 0.95 * z.signum() * 0.7), Vec3::splat(0.1), 0.0, c4(accent));
    }
    // Nav lights and a landing lamp.
    b.lights.boxed(Vec3::new(0.0, 1.2, -hw - 0.03), Vec3::new(0.25, 0.1, 0.06), 0.0, c4(NAV_RED));
    b.lights.boxed(Vec3::new(0.0, 1.2, hw + 0.03), Vec3::new(0.25, 0.1, 0.06), 0.0, c4(NAV_GREEN));
    b.lights.boxed(Vec3::new(-1.78, 1.3, 0.0), Vec3::new(0.06, 0.12, 0.12), 0.0, c4(STROBE));
    b.lamp(1.92, 0.95, 0.0, 0.5, 0.12, 1.0, HEAD);
    b.fx.quad_facing([Vec3::new(-1.6, 0.5, -1.0), Vec3::new(1.4, 0.5, -1.0), Vec3::new(1.4, 0.5, 1.0), Vec3::new(-1.6, 0.5, 1.0)], -Vec3::Y, c4(mul(accent, 0.6)));
}

/// A light helicopter: cabin, tail boom, fin and tail rotor, skids, nav
/// lights. The main rotor is [`rotor_mesh`], spun by the renderer.
fn helicopter(b: &mut B, col: [f32; 3]) {
    let hw = 0.95;
    b.ext(&[(-1.2, 0.55), (1.9, 0.55), (2.7, 1.1), (2.5, 1.9), (1.2, 2.5), (-1.4, 2.35), (-2.0, 1.5)], hw, col);
    slope_quad(&mut b.body, (2.5, 1.9), (1.2, 2.5), hw * 0.85, GLASS);
    b.sides(1.2, 1.6, hw, 1.6, 0.7, GLASS);
    b.sides(-0.6, 1.6, hw, 0.9, 0.7, GLASS);
    // A stripe: red for an air ambulance (white paint), white for the police.
    let stripe = if b.m.paint == 3 { [0.8, 0.05, 0.04] } else { [0.85, 0.85, 0.85] };
    b.sides(0.2, 1.05, hw, 3.4, 0.22, stripe);
    // Tail boom, fin, tail rotor.
    b.bx((-3.6, 1.95, 0.0), (3.4, 0.38, 0.32), col);
    b.bx((-5.15, 2.55, 0.0), (0.7, 1.3, 0.1), col);
    b.body.pipe(Vec2::new(-5.15, 0.2), Vec2::Y, 0.06, 0.6, 2.6, 8, c4([0.08, 0.08, 0.09]));
    // Mast and hub (the blades turn apart).
    b.body.cylinder(Vec2::ZERO, 0.22, 0.18, 2.4, 3.0, 8, c4(TRIM));
    // Skids.
    for z in [-0.85f32, 0.85] {
        b.bx((0.3, 0.04, z), (3.4, 0.08, 0.1), TRIM);
        for x in [-0.6f32, 1.2] {
            b.bx((x, 0.32, z * 0.85), (0.08, 0.55, 0.08), TRIM);
        }
    }
    // Searchlight under the nose (police), nav lights, strobe on the fin.
    b.bx((2.1, 0.45, 0.0), (0.4, 0.3, 0.4), [0.1, 0.1, 0.11]);
    b.lamp(2.32, 0.45, 0.0, 0.3, 0.3, 1.0, HEAD);
    b.lights.boxed(Vec3::new(0.2, 1.3, -hw - 0.03), Vec3::new(0.25, 0.1, 0.06), 0.0, c4(NAV_RED));
    b.lights.boxed(Vec3::new(0.2, 1.3, hw + 0.03), Vec3::new(0.25, 0.1, 0.06), 0.0, c4(NAV_GREEN));
    b.lights.boxed(Vec3::new(-5.15, 3.25, 0.0), Vec3::splat(0.14), 0.0, c4(STROBE));
    b.lights.boxed(Vec3::new(-0.4, 2.45, 0.0), Vec3::new(0.3, 0.1, 0.2), 0.0, c4(NAV_RED));
}

/// A helicopter's main rotor in its own frame (hub at the origin, blades
/// in the horizontal plane), for the renderer to spin about Y: four
/// blades and a hub.
pub fn rotor_mesh() -> MeshData {
    let mut m = MeshData::default();
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        let (s, c) = a.sin_cos();
        m.boxed(Vec3::new(c * 2.5, 0.0, s * 2.5), Vec3::new(4.9, 0.05, 0.28), a, c4([0.06, 0.06, 0.07]));
    }
    m.boxed(Vec3::ZERO, Vec3::new(0.5, 0.2, 0.5), 0.0, c4(TRIM));
    m
}

// ── fleet liveries ──────────────────────────────────────────────────────────

/// Stripes and a lit sign panel for a business fleet, placed for the body.
fn livery_marks(b: &mut B, accent: [f32; 3], sign: [f32; 3]) {
    let m = *b.m;
    if m.livery == Livery::Transit || m.body.is_bus() || m.body == BodyType::DeliveryBot {
        return;
    }
    let len = body_length(m.body);
    match m.body {
        BodyType::DeliveryVan | BodyType::BoxVan | BodyType::PanelTruck | BodyType::GarbageTruck => {
            let (hw, x, y, w, h) = match m.body {
                BodyType::DeliveryVan => (1.0, -0.6, 1.6, 2.6, 0.9),
                BodyType::BoxVan => (1.05, -1.2, 1.9, 3.4, 1.4),
                BodyType::PanelTruck => (1.2, -1.0, 2.2, 4.4, 1.6),
                _ => (1.2, -0.8, 2.1, 3.0, 1.3),
            };
            // A stripe along the bottom and a lit logo panel.
            b.sides(x, y - h * 0.5 - 0.2, hw, w + 0.6, 0.18, accent);
            if m.livery == Livery::Construction {
                for i in 0..5 {
                    b.lamp(-len * 0.5 - 0.01, 0.7 + i as f32 * 0.12, 0.0, 1.8, 0.05, -1.0, if i % 2 == 0 { AMBER } else { [0.05, 0.05, 0.05] });
                }
            }
            if let Livery::Corp(colour, mark) = m.livery {
                // [megacorp] The corp's logo, big on the box.
                corp_logo(b, x, y, hw, (h * 0.85).min(w * 0.6), colour, mark);
            } else {
                b.sides(x, y, hw, w * 0.5, h * 0.6, accent);
                b.lit_sides(x, y, hw + 0.005, w * 0.42, h * 0.45, sign);
                // Brand emblem: a dark square inside the lit panel.
                b.lit_sides(x, y, hw + 0.01, h * 0.25, h * 0.25, mul(accent, 0.2));
            }
        }
        BodyType::Hatchback | BodyType::Sedan | BodyType::Suv | BodyType::Coupe if m.livery == Livery::Taxi => taxi_marks(b, accent, sign),
        BodyType::Hatchback | BodyType::Sedan | BodyType::Suv | BodyType::Pickup | BodyType::Coupe | BodyType::CyberTruck => {
            // Door decal and a lit roof topper.
            b.sides(0.0, 0.7, if matches!(m.body, BodyType::Suv | BodyType::Pickup | BodyType::CyberTruck) { 0.97 } else { 0.89 }, 1.2, 0.3, accent);
            let roof = match m.body {
                BodyType::Suv | BodyType::Pickup => 1.87,
                BodyType::CyberTruck => 1.95,
                BodyType::Coupe => 1.27,
                _ => 1.55,
            };
            b.lights.boxed(Vec3::new(-0.4, roof + 0.13, 0.0), Vec3::new(0.5, 0.22, 0.12), 0.0, c4(sign));
        }
        BodyType::Scooter | BodyType::Motorcycle => {
            b.lit_sides(-len * 0.5 + 0.25, 0.95, 0.21, 0.3, 0.2, sign);
        }
        BodyType::Semi => {
            b.lit_sides(len * 0.5 - 1.1, 2.75, 1.17, 0.9, 0.3, sign);
            // [megacorp] The logo on the trailer.
            if let Livery::Corp(colour, mark) = m.livery {
                corp_logo(b, -1.4, 2.3, 1.25, 1.9, colour, mark);
            }
        }
        _ => {}
    }
    if m.livery == Livery::Construction || m.livery == Livery::Utility {
        // Beacon on the roof of the cab.
        let (x, y) = match m.body {
            BodyType::PanelTruck | BodyType::BoxVan => (len * 0.5 - 1.0, 2.5),
            BodyType::Semi => (len * 0.5 - 1.0, 3.2),
            BodyType::GarbageTruck => (len * 0.5 - 1.0, 2.85),
            BodyType::Pickup => (0.0, 1.9),
            _ => (0.0, 2.55),
        };
        b.lights.boxed(Vec3::new(x, y + 0.08, 0.0), Vec3::new(0.18, 0.14, 0.5), 0.0, c4(AMBER));
    }
}

/// [taxis] A cab: a checker band along both sides, a wide lit TAXI sign on
/// the roof (two bright faces, readable from above and from the street), a
/// neon sill strip and underglow in the cab's accent so the fleet glows at
/// night: the cyberpunk cab.
fn taxi_marks(b: &mut B, dark: [f32; 3], sign: [f32; 3]) {
    let m = *b.m;
    let len = body_length(m.body);
    let tall = matches!(m.body, BodyType::Suv);
    let hw = match m.body {
        BodyType::Hatchback => 0.84,
        BodyType::Suv => 0.97,
        BodyType::Coupe => 0.93,
        _ => 0.89,
    };
    let roof = match m.body {
        BodyType::Suv => 1.87,
        BodyType::Coupe => 1.27,
        _ => 1.5,
    };
    // Checker band: alternating dark squares along the doors.
    let y = if tall { 0.95 } else { 0.72 };
    let sq = 0.16;
    let n = ((len * 0.62) / sq) as i32;
    let x0 = -(n as f32) * sq * 0.5;
    for i in 0..n {
        for row in 0..2 {
            if (i + row) % 2 == 0 {
                let x = x0 + (i as f32 + 0.5) * sq;
                b.sides(x, y + row as f32 * sq, hw + 0.004, sq, sq, dark);
            }
        }
    }
    // Roof sign: a lit box across the roof, its top lit too for the
    // camera overhead.
    if b.detail {
        b.bx((-0.25, roof + 0.03, 0.0), (0.9, 0.06, 0.7), dark);
    }
    b.lights.boxed(Vec3::new(-0.25, roof + 0.2, 0.0), Vec3::new(0.75, 0.26, 0.6), 0.0, c4(sign));
    // Neon sill strip and underglow in the cab's accent.
    let neon = at(&b.pal.vehicle_neon, m.paint as usize);
    b.lit_sides(0.0, if tall { 0.46 } else { 0.32 }, hw, len * 0.7, 0.05, neon);
    b.underglow(len * 0.85, hw * 1.05, neon);
}

// ── parked vehicles in building dressing ────────────────────────────────────

/// The vehicle kinds building generators park on their lots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parked {
    Car,
    Van,
    Truck,
    Wreck,
    Bus,
    /// [economy] Always a garbage truck (trash company yards).
    GarbageTruck,
    /// [economy] Always a box-bodied lorry (reefers at fish docks).
    Lorry,
}

/// Draw a parked vehicle into building geometry: at world-plane point `at`
/// (x, z), height `y`, facing `yaw` (world). Low detail (no hubs, decals or
/// lamps) since parking decks hold dozens. The body goes on the metal
/// surface (a wreck's on rust); glass on glass.
#[allow(clippy::too_many_arguments)]
pub fn draw_parked(g: &mut Geometry, pal: &Palette, rng: &mut impl Rng, at: Vec2, y: f32, yaw: f32, kind: Parked, color: [f32; 3]) {
    let (body, class, wear, livery) = match kind {
        Parked::Car => {
            let b = [BodyType::Sedan, BodyType::Hatchback, BodyType::Suv, BodyType::Coupe, BodyType::Pickup, BodyType::CyberTruck][rng.random_range(0..6usize).min(if rng.random_bool(0.85) { 4 } else { 5 })];
            let class = [VehicleClass::Cheap, VehicleClass::Mid, VehicleClass::Mid, VehicleClass::Expensive][rng.random_range(0..4usize)];
            (b, class, rng.random_range(0..2u8), Livery::None)
        }
        Parked::Van => (if rng.random_bool(0.6) { BodyType::DeliveryVan } else { BodyType::BoxVan }, VehicleClass::Mid, 1, Livery::None),
        Parked::Truck => (if rng.random_bool(0.5) { BodyType::PanelTruck } else { BodyType::GarbageTruck }, VehicleClass::Mid, 1, Livery::None),
        Parked::GarbageTruck => (BodyType::GarbageTruck, VehicleClass::Mid, 1, Livery::None),
        Parked::Lorry => (BodyType::PanelTruck, VehicleClass::Mid, 1, Livery::None),
        Parked::Bus => (BodyType::BubbleBus, VehicleClass::Expensive, 0, Livery::Transit),
        Parked::Wreck => (if rng.random_bool(0.5) { BodyType::Sedan } else { BodyType::Hatchback }, VehicleClass::Cheap, 2, Livery::None),
    };
    let model = VehicleModel { class, body, wear, tuned: false, livery, paint: rng.random_range(0..PAINTS) };
    let paint = if kind == Parked::Wreck { mix(color, pal.rust, 0.5) } else { color };
    let v = build(&model, paint, false, pal);
    let surface = if kind == Parked::Wreck { Surface::Rust } else { Surface::Metal };
    let (s, c) = yaw.sin_cos();
    let xf = |p: [f32; 3]| -> [f32; 3] { [at.x + p[0] * c - p[2] * s, y + p[1], at.y + p[0] * s + p[2] * c] };
    let rot = |n: [f32; 3]| -> [f32; 3] { [n[0] * c - n[2] * s, n[1], n[0] * s + n[2] * c] };
    // Split glass-coloured triangles onto the glass surface so they read as
    // windows; the rest is paint.
    let src = &v.body;
    for tri in src.indices.chunks(3) {
        let col = src.colors[tri[0] as usize];
        let is_glass = (col[0] - GLASS[0]).abs() < 1e-3 && (col[2] - GLASS[2]).abs() < 1e-3;
        let dst = g.part(if is_glass { Surface::Glass } else { surface });
        let base = dst.positions.len() as u32;
        for &i in tri {
            let i = i as usize;
            dst.positions.push(xf(src.positions[i]));
            dst.normals.push(rot(src.normals[i]));
            dst.colors.push(if is_glass { c4(pal.car_glass) } else { src.colors[i] });
            dst.uvs.push(src.uvs[i]);
            let t = src.tangents.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 1.0]);
            let tr = rot([t[0], t[1], t[2]]);
            dst.tangents.push([tr[0], tr[1], tr[2], t[3]]);
        }
        dst.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_is_finite_and_under_budget() {
        let mut worst = (0, String::new());
        for m in catalogue() {
            let v = vehicle_mesh(&m);
            let t = v.triangles();
            assert!(t > 0, "{m:?} is empty");
            assert!(t <= TRI_BUDGET, "{m:?} has {t} triangles");
            if t > worst.0 {
                worst = (t, format!("{m:?}"));
            }
            for mesh in [&v.body, &v.lights, &v.fx] {
                assert!(mesh.positions.iter().flatten().all(|x| x.is_finite()), "{m:?} has a non-finite vertex");
                assert!(mesh.normals.iter().flatten().all(|x| x.is_finite()), "{m:?} has a non-finite normal");
                assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.positions.len()));
                assert_eq!(mesh.positions.len(), mesh.colors.len());
            }
            // Stands on the ground and within its length.
            let len = body_length(m.body);
            let (lo, hi) = v.body.positions.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
            assert!(lo >= -0.01 && lo < 0.2, "{m:?} floats or sinks: lowest point {lo}");
            assert!(hi < 4.2, "{m:?} is {hi} m tall");
            let reach = v.body.positions.iter().map(|p| p[0].abs()).fold(0.0, f32::max);
            assert!(reach <= len * 0.5 + 0.2, "{m:?} reaches {reach} m, length {len}");
        }
        eprintln!("heaviest variant: {} triangles, {}", worst.0, worst.1);
    }

    #[test]
    fn triangle_counts_per_body() {
        for body in BodyType::ALL {
            let n = |class, tuned, livery| vehicle_mesh(&VehicleModel { body, class, tuned, livery, wear: 2, paint: 1 }).triangles();
            eprintln!("{:16} cheap {:4} mid {:4} exp {:4} tuned {:4} livery {:4}", body.label(), n(VehicleClass::Cheap, false, Livery::None), n(VehicleClass::Mid, false, Livery::None), n(VehicleClass::Expensive, false, Livery::None), n(VehicleClass::Expensive, true, Livery::None), n(VehicleClass::Mid, false, Livery::Pizza));
        }
    }

    #[test]
    fn generation_is_deterministic() {
        for m in catalogue().into_iter().step_by(7) {
            let a = vehicle_mesh(&m);
            let b = vehicle_mesh(&m);
            assert_eq!(a.body.positions, b.body.positions);
            assert_eq!(a.body.colors, b.body.colors);
            assert_eq!(a.lights.positions, b.lights.positions);
        }
    }

    #[test]
    fn silhouettes_differ() {
        let shape = |body: BodyType| {
            let v = vehicle_mesh(&VehicleModel { body, ..Default::default() });
            let hi = v.body.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
            let reach = v.body.positions.iter().map(|p| p[0]).fold(0.0, f32::max);
            ((hi * 10.0).round() as i32, (reach * 10.0).round() as i32, v.body.positions.len())
        };
        let shapes: Vec<_> = BodyType::ALL.iter().map(|&b| shape(b)).collect();
        for i in 0..shapes.len() {
            for j in i + 1..shapes.len() {
                assert_ne!(shapes[i], shapes[j], "{:?} and {:?} look the same", BodyType::ALL[i], BodyType::ALL[j]);
            }
        }
    }

    #[test]
    fn tuned_cars_glow_underneath_and_beaters_have_rust() {
        let tuned = vehicle_mesh(&VehicleModel { body: BodyType::Coupe, class: VehicleClass::Expensive, tuned: true, ..Default::default() });
        assert!(!tuned.fx.is_empty());
        let plain = vehicle_mesh(&VehicleModel { body: BodyType::Coupe, class: VehicleClass::Expensive, ..Default::default() });
        assert!(plain.fx.is_empty());
        let beater = vehicle_mesh(&VehicleModel { body: BodyType::Sedan, class: VehicleClass::Cheap, wear: 2, ..Default::default() });
        assert!(beater.body.colors.iter().any(|c| (c[0] - RUST[0]).abs() < 1e-4 && (c[1] - RUST[1]).abs() < 1e-4));
        let akira = vehicle_mesh(&VehicleModel { body: BodyType::AkiraBike, ..Default::default() });
        assert!(!akira.fx.is_empty());
    }

    #[test]
    fn liveries_paint_the_fleet() {
        let pizza = vehicle_mesh(&VehicleModel { body: BodyType::DeliveryVan, livery: Livery::Pizza, ..Default::default() });
        let plain = vehicle_mesh(&VehicleModel { body: BodyType::DeliveryVan, ..Default::default() });
        assert!(pizza.lights.indices.len() > plain.lights.indices.len(), "a lit sign panel");
        let red = livery_colors(Livery::Pizza, 0, &Palette::default()).unwrap().0;
        assert!(pizza.body.colors.iter().any(|c| (c[0] - red[0]).abs() < 0.05 && (c[1] - red[1]).abs() < 0.05));
    }
}
