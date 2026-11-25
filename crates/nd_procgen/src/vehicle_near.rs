//! [r17-cars] The near level of detail for vehicles, and the paint and part
//! tags every vehicle mesh carries for the car paint shader.
//!
//! The far model (`vehicles.rs`) extrudes a side silhouette across the
//! width: flat sides, cheap, read from a distance. Near the camera a car is
//! drawn instead as a **loft**: a ring of cross-section points at each of a
//! few dozen stations along its length, joined into a smooth skin with
//! shared normals inside each smoothing group and crisp creases between
//! them (the shoulder line, the window sill, the cowl, the bumpers). The
//! ring gives the body tumblehome (the glass leans in over the shoulder),
//! a crowned roof and bonnet, a rounded nose and tail in plan, and wheel
//! arches cut by raising the lower rows over each axle, with flares. Glass,
//! pillars, bumpers and two-tone roofs are colours of whole cells; door
//! lines, stripes, rust and liveries are thin patches laid on the skin
//! (`Loft::side_quad`, `Loft::top_rect`), so they follow its curves.
//! Wheels are turned tyres with a rim, spokes or a hubcap and a brake disc.
//!
//! How much detail the near model carries is a step of a ladder
//! ([`NearDetail`]): more stations, more sides on the wheels, spokes, then
//! the small parts (wipers, handles, fins, exhausts, calipers). The
//! renderer picks the step; the frame-cost measurements that chose it are
//! in `todo.md` (r17-cars).
//!
//! Every vehicle mesh (far and near) is tagged per vertex for the paint
//! shader (`nd_app/src/render/car_paint.rs`): `uv.x` the kind of part
//! (paint, glass, tyre, chrome, black trim, alloy, rough, gloss black),
//! `uv.y` the paint finish (solid, gloss, metallic, pearl, matte wrap,
//! worn) plus a fraction for the dirt at that height, and the second UV set
//! a position on the body for the paint's flakes and the rain's beads.

use super::*;

/// One step of the near model's detail ladder, 1 (lightest) to
/// [`NearDetail::MAX`]; 0 means no near model (the far one everywhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NearDetail(pub u8);

impl NearDetail {
    pub const MAX: u8 = 4;
    fn s(self) -> usize {
        (self.0.clamp(1, Self::MAX) - 1) as usize
    }
    /// Most triangles a near variant of this step may have.
    pub fn budget(self) -> usize {
        [2400, 3000, 3900, 4800][self.s()]
    }
    fn arch_steps(self) -> usize {
        [3, 4, 6, 8][self.s()]
    }
    fn roof_steps(self) -> usize {
        [1, 2, 3, 4][self.s()]
    }
    fn screen_steps(self) -> usize {
        [1, 1, 2, 3][self.s()]
    }
    fn wheel_sides(self) -> usize {
        [10, 14, 18, 24][self.s()]
    }
    fn spokes(self) -> bool {
        self.0 >= 2
    }
    /// Handles, wipers, fins, exhausts, the bonnet's lines.
    fn extras(self) -> bool {
        self.0 >= 3
    }
    /// Brake calipers, tread grooves, the fine trim.
    fn fine(self) -> bool {
        self.0 >= 4
    }
}

// ── part colours (each its own exact value: the tag pass matches them) ──────

/// Gloss black: floating-roof pillars, piano-black trim.
pub(super) const GLOSS_BLACK: [f32; 3] = [0.011, 0.011, 0.013];
/// A painted alloy wheel.
pub(super) const ALLOY: [f32; 3] = [0.52, 0.53, 0.56];
/// A dark alloy (tuned, sports).
pub(super) const DARK_ALLOY: [f32; 3] = [0.075, 0.075, 0.08];
/// Plastic hubcap / steel wheel grey.
pub(super) const STEEL: [f32; 3] = [0.30, 0.30, 0.31];
/// Number plates.
pub(super) const PLATE: [f32; 3] = [0.74, 0.74, 0.68];
/// Grilles and intakes.
pub(super) const GRILLE: [f32; 3] = [0.018, 0.018, 0.02];
/// The floor and the wheel wells.
pub(super) const UNDER: [f32; 3] = [0.02, 0.02, 0.021];
/// Mirror glass.
pub(super) const MIRROR: [f32; 3] = [0.35, 0.37, 0.40];
/// Brake discs.
pub(super) const DISC: [f32; 3] = [0.22, 0.21, 0.2];

// ── kinds and finishes ──────────────────────────────────────────────────────

/// Kinds of part, `uv.x` of every body vertex.
pub mod kind {
    pub const PAINT: u8 = 0;
    pub const GLASS: u8 = 1;
    pub const TYRE: u8 = 2;
    pub const CHROME: u8 = 3;
    pub const TRIM: u8 = 4;
    pub const ALLOY: u8 = 5;
    pub const ROUGH: u8 = 6;
    pub const GLOSS_BLACK: u8 = 7;
}

/// Paint finishes, the whole part of `uv.y` on paint.
pub mod finish {
    pub const SOLID: u8 = 0;
    pub const GLOSS: u8 = 1;
    pub const METALLIC: u8 = 2;
    pub const PEARL: u8 = 3;
    pub const MATTE: u8 = 4;
    pub const WORN: u8 = 5;
}

fn same(a: [f32; 4], b: [f32; 3]) -> bool {
    (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5 && (a[2] - b[2]).abs() < 1e-5
}

/// The kind of part a vertex colour is.
pub fn part_kind(c: [f32; 4], pal: &Palette) -> u8 {
    if same(c, GLASS) || same(c, pal.car_glass) {
        kind::GLASS
    } else if same(c, TYRE) {
        kind::TYRE
    } else if same(c, CHROME) || same(c, MIRROR) {
        kind::CHROME
    } else if same(c, TRIM) || same(c, GRILLE) || same(c, UNDER) || same(c, mul(TRIM, 1.5)) || same(c, mul(TRIM, 1.6)) {
        kind::TRIM
    } else if same(c, ALLOY) || same(c, DARK_ALLOY) || same(c, STEEL) || same(c, [0.4, 0.4, 0.42]) || same(c, DISC) {
        kind::ALLOY
    } else if same(c, RUST) || same(c, PRIMER) || same(c, PLATE) {
        kind::ROUGH
    } else if same(c, GLOSS_BLACK) {
        kind::GLOSS_BLACK
    } else {
        kind::PAINT
    }
}

/// A stable number per variant for its paint choices.
fn look_hash(m: &VehicleModel) -> u64 {
    let mut h = nd_sim::hash::Fnv::default();
    for v in [m.body as u64, m.class as u64, m.paint as u64, livery_code(m.livery), m.tuned as u64, 7_177] {
        h.word(v);
    }
    nd_sim::hash::step31(h.0)
}

/// The paint finish of a variant: worn beaters dull, fleets gloss, the
/// rest by class (cheap mostly solid, dearer cars metallic and pearl),
/// tuned cars matte wraps, pearl and metallic.
pub fn paint_finish(m: &VehicleModel) -> u8 {
    let h = look_hash(m) % 100;
    let car = matches!(m.body, BodyType::Hatchback | BodyType::Sedan | BodyType::Coupe | BodyType::Suv | BodyType::Pickup | BodyType::CyberTruck | BodyType::Motorcycle | BodyType::Scooter);
    if m.wear >= 2 && m.class == VehicleClass::Cheap {
        return finish::WORN;
    }
    if m.livery != Livery::None {
        return finish::GLOSS;
    }
    if !car {
        return if m.class == VehicleClass::Cheap || m.wear >= 2 { finish::SOLID } else { finish::GLOSS };
    }
    if m.tuned {
        return if h < 30 { finish::MATTE } else if h < 65 { finish::PEARL } else { finish::METALLIC };
    }
    match m.class {
        VehicleClass::Cheap => {
            if h < 30 {
                finish::GLOSS
            } else {
                finish::SOLID
            }
        }
        VehicleClass::Mid => {
            if h < 40 {
                finish::GLOSS
            } else if h < 85 {
                finish::METALLIC
            } else {
                finish::PEARL
            }
        }
        VehicleClass::Expensive => {
            if h < 20 {
                finish::GLOSS
            } else if h < 62 {
                finish::METALLIC
            } else {
                finish::PEARL
            }
        }
    }
}

/// How dirty a variant is overall (0..1): its wear, more on cheap cars.
pub fn dirt_of(m: &VehicleModel) -> f32 {
    let w: f32 = [0.08, 0.38, 0.75][m.wear.min(2) as usize];
    let c = match m.class {
        VehicleClass::Cheap => 0.12,
        VehicleClass::Mid => 0.04,
        VehicleClass::Expensive => 0.0,
    };
    (w + c).min(0.9)
}

/// Tag a body mesh for the paint shader: `uv` = (kind, finish + dirt
/// here), the second set a position on the body (flakes, beads). Dirt
/// gathers low down: full at the sills, a quarter on the roof.
pub fn tag_parts(mesh: &mut MeshData, model: &VehicleModel, pal: &Palette) {
    let fin = paint_finish(model) as f32;
    let dirt = dirt_of(model);
    let top = mesh.positions.iter().map(|p| p[1]).fold(0.5f32, f32::max);
    mesh.uvs.resize(mesh.positions.len(), [0.0, 0.0]);
    mesh.aux.clear();
    for i in 0..mesh.positions.len() {
        let p = mesh.positions[i];
        let k = part_kind(mesh.colors[i], pal);
        let rel = (p[1] / top).clamp(0.0, 1.0);
        let low = 1.0 - smooth(0.12, 0.6, rel);
        let d = (dirt * (0.25 + 0.75 * low)).clamp(0.0, 0.95);
        mesh.uvs[i] = [k as f32, fin + d];
        mesh.aux.push([p[0] + 0.71 * p[2], p[1] + 0.53 * p[2]]);
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A piecewise-linear profile through `(x, value)` keys (any order; keys
/// out of order after sorting by x are dropped), held flat past its ends.
fn profile(keys: &[(f32, f32)], x: f32) -> f32 {
    let mut k: Vec<(f32, f32)> = keys.to_vec();
    k.sort_by(|a, b| a.0.total_cmp(&b.0));
    if x <= k[0].0 {
        return k[0].1;
    }
    for w in k.windows(2) {
        if x <= w[1].0 {
            let span = w[1].0 - w[0].0;
            return if span < 1e-5 { w[1].1 } else { lerp(w[0].1, w[1].1, (x - w[0].0) / span) };
        }
    }
    k[k.len() - 1].1
}

// ── raw vertices ────────────────────────────────────────────────────────────

fn vtx(m: &mut MeshData, p: Vec3, n: Vec3, c: [f32; 3]) -> u32 {
    let i = m.positions.len() as u32;
    m.positions.push(p.to_array());
    m.normals.push(n.to_array());
    m.colors.push(c4(c));
    m.uvs.push([0.0, 0.0]);
    m.tangents.push([1.0, 0.0, 0.0, 1.0]);
    i
}

/// A quad with its own normals, wound `a b c d` counter-clockwise seen
/// from outside unless `flip`.
fn quad_n(m: &mut MeshData, p: [Vec3; 4], n: [Vec3; 4], c: [f32; 3], flip: bool) {
    let i: Vec<u32> = (0..4).map(|k| vtx(m, p[k], n[k], c)).collect();
    if flip {
        m.indices.extend_from_slice(&[i[0], i[2], i[1], i[0], i[3], i[2]]);
    } else {
        m.indices.extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
    }
}

/// A flat polygon (convex or star-shaped from its first point's
/// neighbours: lamp shapes) at `x`, facing `dir` (+1 forward), from
/// `(z, y)` points.
fn flat_poly_x(m: &mut MeshData, x: f32, pts: &[(f32, f32)], dir: f32, c: [f32; 3]) {
    let ring: Vec<Vec3> = pts.iter().map(|&(z, y)| Vec3::new(x, y, z)).collect();
    m.cap(&ring, Vec3::X * dir, c4(c));
}

/// A rounded-corner rectangle outline in a plane, `n` points per corner.
fn round_rect(c0: f32, c1: f32, hw: f32, hh: f32, r: f32, n: usize) -> Vec<(f32, f32)> {
    let r = r.min(hw).min(hh);
    let mut out = Vec::new();
    for (k, (cx, cy)) in [(hw - r, hh - r), (-(hw - r), hh - r), (-(hw - r), -(hh - r)), (hw - r, -(hh - r))].into_iter().enumerate() {
        for i in 0..=n {
            let a = (k as f32 + i as f32 / n.max(1) as f32) * std::f32::consts::FRAC_PI_2;
            out.push((c0 + cx + r * a.cos(), c1 + cy + r * a.sin()));
        }
    }
    out
}

// ── the loft ────────────────────────────────────────────────────────────────

/// Points per half ring, from the floor's centre round the right side to
/// the roof's centre.
pub(super) const R: usize = 10;
const BANDS: usize = R - 1;

/// Bands of the ring (between rows `b` and `b + 1`).
pub(super) mod band {
    pub const FLOOR: usize = 0;
    pub const ROCKER: usize = 1;
    pub const LOWER: usize = 2;
    pub const GLASS: usize = 5;
    pub const PILLAR: usize = 6;
    pub const ROOF_OUT: usize = 7;
    pub const ROOF_IN: usize = 8;
}

/// A lofted skin: rings of `(z, y)` points (right side) at stations along
/// x, mirrored for the left.
pub(super) struct Loft {
    pub xs: Vec<f32>,
    pub rings: Vec<[Vec2; R]>,
    /// Smoothing group of each band: normals are shared only within one.
    pub vgroup: [u8; BANDS],
    /// Smoothing group of each column of cells (between stations).
    pub ugroup: Vec<u16>,
    /// Each cell's colour (column, band).
    pub cols: Vec<[[f32; 3]; BANDS]>,
}

impl Loft {
    fn n(&self) -> usize {
        self.xs.len()
    }

    fn p(&self, i: usize, j: usize) -> Vec3 {
        let q = self.rings[i][j];
        Vec3::new(self.xs[i], q.y, q.x)
    }

    /// Area-weighted face normal of a cell, right side.
    fn face(&self, i: usize, b: usize) -> Vec3 {
        let (a, bb, c, d) = (self.p(i, b), self.p(i + 1, b), self.p(i + 1, b + 1), self.p(i, b + 1));
        (c - a).cross(d - bb)
    }

    /// The normal at ring point `(ci, cj)` as seen by cell `(i, b)`: the
    /// cells round it in the same smoothing groups.
    fn vnormal(&self, ci: usize, cj: usize, i: usize, b: usize) -> Vec3 {
        let mut n = Vec3::ZERO;
        for ii in [ci.wrapping_sub(1), ci] {
            if ii >= self.n() - 1 || self.ugroup[ii] != self.ugroup[i] {
                continue;
            }
            for bb in [cj.wrapping_sub(1), cj] {
                if bb >= BANDS || self.vgroup[bb] != self.vgroup[b] {
                    continue;
                }
                n += self.face(ii, bb);
            }
        }
        if n.length_squared() < 1e-14 {
            n = self.face(i, b);
        }
        // On the centre line the two halves meet: no sideways lean.
        if self.rings[ci][cj].x.abs() < 1e-4 {
            n.z = 0.0;
        }
        let n = n.normalize_or_zero();
        if n == Vec3::ZERO { Vec3::Y } else { n }
    }

    /// The skin, both halves, each cell in its colour.
    fn shell(&self, m: &mut MeshData) {
        for i in 0..self.n() - 1 {
            // The floor (band 0) is under the car: never seen.
            for b in 1..BANDS {
                if self.face(i, b).length_squared() < 1e-10 {
                    continue;
                }
                let idx = [(i, b), (i + 1, b), (i + 1, b + 1), (i, b + 1)];
                let p = idx.map(|(ci, cj)| self.p(ci, cj));
                let n = idx.map(|(ci, cj)| self.vnormal(ci, cj, i, b));
                let col = self.cols[i][b];
                for s in [1.0f32, -1.0] {
                    let mz = |v: Vec3| Vec3::new(v.x, v.y, v.z * s);
                    quad_n(m, p.map(mz), n.map(mz), col, s < 0.0);
                }
            }
        }
    }

    /// The full ring at station `i` (right side up, left side back down).
    fn full_ring(&self, i: usize) -> Vec<Vec3> {
        let mut ring: Vec<Vec3> = (0..R).map(|j| self.p(i, j)).collect();
        for j in (1..R - 1).rev() {
            let q = self.p(i, j);
            ring.push(Vec3::new(q.x, q.y, -q.z));
        }
        ring
    }

    /// Close both ends (flat faces).
    fn caps(&self, m: &mut MeshData, rear: [f32; 3], front: [f32; 3]) {
        m.cap(&self.full_ring(0), -Vec3::X, c4(rear));
        m.cap(&self.full_ring(self.n() - 1), Vec3::X, c4(front));
    }

    /// Position and normal on the right-side skin at fractional station
    /// `u` and row `v`.
    fn eval(&self, u: f32, v: f32) -> (Vec3, Vec3) {
        let i = (u.max(0.0).floor() as usize).min(self.n() - 2);
        let b = (v.max(0.0).floor() as usize).min(BANDS - 1);
        let (fu, fv) = ((u - i as f32).clamp(0.0, 1.0), (v - b as f32).clamp(0.0, 1.0));
        let bl = |a: Vec3, bb: Vec3, c: Vec3, d: Vec3| (a * (1.0 - fu) + bb * fu) * (1.0 - fv) + (d * (1.0 - fu) + c * fu) * fv;
        let p = bl(self.p(i, b), self.p(i + 1, b), self.p(i + 1, b + 1), self.p(i, b + 1));
        let n = bl(self.vnormal(i, b, i, b), self.vnormal(i + 1, b, i, b), self.vnormal(i + 1, b + 1, i, b), self.vnormal(i, b + 1, i, b)).normalize_or_zero();
        (p, n)
    }

    /// Station coordinate of `x`.
    fn u_of_x(&self, x: f32) -> f32 {
        let n = self.n();
        if x <= self.xs[0] {
            return 0.0;
        }
        for i in 0..n - 1 {
            if x <= self.xs[i + 1] {
                let span = (self.xs[i + 1] - self.xs[i]).max(1e-5);
                return i as f32 + (x - self.xs[i]) / span;
            }
        }
        (n - 1) as f32
    }

    /// The ring at fractional station `u`.
    fn ring_at(&self, u: f32) -> [Vec2; R] {
        let i = (u.max(0.0).floor() as usize).min(self.n() - 2);
        let f = (u - i as f32).clamp(0.0, 1.0);
        let mut r = [Vec2::ZERO; R];
        for (j, out) in r.iter_mut().enumerate() {
            *out = self.rings[i][j].lerp(self.rings[i + 1][j], f);
        }
        r
    }

    /// Row coordinate where the side rows (1..=6) reach height `y`.
    fn v_of_y(&self, u: f32, y: f32) -> f32 {
        let r = self.ring_at(u);
        if y <= r[1].y {
            return 1.0;
        }
        for j in 1..6 {
            if y <= r[j + 1].y {
                let span = (r[j + 1].y - r[j].y).max(1e-5);
                return j as f32 + (y - r[j].y) / span;
            }
        }
        6.0
    }

    /// Row coordinate where the top rows (5..=9) reach `z` from the centre.
    fn v_of_z(&self, u: f32, z: f32) -> f32 {
        let r = self.ring_at(u);
        if z >= r[5].x {
            return 5.0;
        }
        for j in 5..9 {
            if z >= r[j + 1].x {
                let span = (r[j].x - r[j + 1].x).max(1e-5);
                return j as f32 + (r[j].x - z) / span;
            }
        }
        9.0
    }

    /// Cuts of `[a, b]` at the whole numbers between.
    fn cuts(a: f32, b: f32) -> Vec<f32> {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let mut c = vec![a];
        let mut k = a.floor() + 1.0;
        while k < b - 1e-3 {
            c.push(k);
            k += 1.0;
        }
        c.push(b);
        c
    }

    /// A patch laid on the side skin over the side-view quad `q` (x, y
    /// corners, counter-clockwise from the bottom left seen from the right),
    /// on side `s` (+1 right), lifted `lift` off it.
    pub fn side_quad(&self, m: &mut MeshData, q: [(f32, f32); 4], s: f32, lift: f32, c: [f32; 3]) {
        let (x0, x1) = (q.iter().map(|p| p.0).fold(f32::MAX, f32::min), q.iter().map(|p| p.0).fold(f32::MIN, f32::max));
        let su = Self::cuts(self.u_of_x(x0), self.u_of_x(x1));
        // Rows crossed in the middle, for the vertical cuts.
        let ymid = (q[0].1 + q[1].1 + q[2].1 + q[3].1) * 0.25;
        let um = self.u_of_x((x0 + x1) * 0.5);
        let rows = (self.v_of_y(um, q[2].1.max(q[3].1)) - self.v_of_y(um, q[0].1.min(q[1].1))).abs().ceil().max(1.0) as usize;
        let _ = ymid;
        let at = |sx: f32, ty: f32| -> (f32, f32) {
            // Bilinear in the quad: sx along the bottom/top, ty up.
            let b = (q[0].0 + (q[1].0 - q[0].0) * sx, q[0].1 + (q[1].1 - q[0].1) * sx);
            let t = (q[3].0 + (q[2].0 - q[3].0) * sx, q[3].1 + (q[2].1 - q[3].1) * sx);
            (b.0 + (t.0 - b.0) * ty, b.1 + (t.1 - b.1) * ty)
        };
        let point = |x: f32, y: f32| -> (Vec3, Vec3) {
            let u = self.u_of_x(x);
            let (p, n) = self.eval(u, self.v_of_y(u, y));
            let p = p + n * lift;
            (Vec3::new(x, p.y, p.z * s), Vec3::new(n.x, n.y, n.z * s))
        };
        // Param cuts along: the station crossings mapped back to sx (the
        // quad's bottom edge spans x0..x1 for the rectangles used here).
        let sxs: Vec<f32> = su.iter().map(|&u| {
            let x = self.x_of_u(u);
            ((x - x0) / (x1 - x0).max(1e-5)).clamp(0.0, 1.0)
        }).collect();
        for w in sxs.windows(2) {
            for k in 0..rows {
                let (t0, t1) = (k as f32 / rows as f32, (k + 1) as f32 / rows as f32);
                let corners = [at(w[0], t0), at(w[1], t0), at(w[1], t1), at(w[0], t1)];
                let pn = corners.map(|(x, y)| point(x, y));
                quad_n(m, pn.map(|v| v.0), pn.map(|v| v.1), c, s < 0.0);
            }
        }
    }

    fn x_of_u(&self, u: f32) -> f32 {
        let i = (u.max(0.0).floor() as usize).min(self.n() - 2);
        let f = (u - i as f32).clamp(0.0, 1.0);
        lerp(self.xs[i], self.xs[i + 1], f)
    }

    /// An axis-aligned side patch, both sides.
    pub fn side_rect(&self, m: &mut MeshData, x0: f32, x1: f32, y0: f32, y1: f32, lift: f32, c: [f32; 3]) {
        for s in [1.0f32, -1.0] {
            self.side_quad(m, [(x0, y0), (x1, y0), (x1, y1), (x0, y1)], s, lift, c);
        }
    }

    /// A patch on the top (bonnet, roof, boot) from `x0..x1` and `z0..z1`
    /// across (right side; negative z mirrors), lifted `lift`.
    pub fn top_rect(&self, m: &mut MeshData, x0: f32, x1: f32, z0: f32, z1: f32, lift: f32, c: [f32; 3]) {
        let su = Self::cuts(self.u_of_x(x0), self.u_of_x(x1));
        let s = if z0 + z1 < 0.0 { -1.0 } else { 1.0 };
        let (za, zb) = (z0.abs().min(z1.abs()), z0.abs().max(z1.abs()));
        let point = |u: f32, z: f32| -> (Vec3, Vec3) {
            let (p, n) = self.eval(u, self.v_of_z(u, z));
            let p = p + n * lift;
            (Vec3::new(p.x, p.y, p.z * s), Vec3::new(n.x, n.y, n.z * s))
        };
        for w in su.windows(2) {
            // Corners counter-clockwise seen from above on the right: the
            // outer edge (zb) first.
            let pn = [point(w[0], zb), point(w[1], zb), point(w[1], za), point(w[0], za)];
            quad_n(m, pn.map(|v| v.0), pn.map(|v| v.1), c, s < 0.0);
        }
    }

    /// A patch over whole-cell coordinates (`u0..u1`, `v0..v1`), both sides.
    pub fn patch(&self, m: &mut MeshData, u0: f32, u1: f32, v0: f32, v1: f32, lift: f32, c: [f32; 3]) {
        let (su, sv) = (Self::cuts(u0, u1), Self::cuts(v0, v1));
        for wu in su.windows(2) {
            for wv in sv.windows(2) {
                let pn = [(wu[0], wv[0]), (wu[1], wv[0]), (wu[1], wv[1]), (wu[0], wv[1])].map(|(u, v)| {
                    let (p, n) = self.eval(u, v);
                    (p + n * lift, n)
                });
                for s in [1.0f32, -1.0] {
                    let mz = |v: Vec3| Vec3::new(v.x, v.y, v.z * s);
                    quad_n(m, pn.map(|v| mz(v.0)), pn.map(|v| mz(v.1)), c, s < 0.0);
                }
            }
        }
    }

    /// Height of the skin's side rows at `x` and the outermost z there.
    pub fn side_at(&self, x: f32, y: f32) -> Vec3 {
        let u = self.u_of_x(x);
        self.eval(u, self.v_of_y(u, y)).0
    }
}

// ── wheels ──────────────────────────────────────────────────────────────────

/// How a wheel's face looks.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Rim {
    /// A plain plastic hubcap or a steel wheel.
    Cap,
    /// Alloy spokes in a colour.
    Spokes([f32; 3], usize),
    /// Many fine spokes (expensive).
    Mesh([f32; 3]),
}

/// A near wheel round the axle at `(x, r)`, its tyre's outer face at
/// `z_out` (signed: +z the right side), `w` wide.
#[allow(clippy::too_many_arguments)]
pub(super) fn wheel(m: &mut MeshData, d: NearDetail, x: f32, r: f32, z_out: f32, w: f32, rim: Rim, caliper: Option<[f32; 3]>) {
    let s = z_out.signum();
    let n = d.wheel_sides();
    let rr = r * 0.66;
    // Profile from the inner tread edge round to the hub: (radius, depth
    // in from the outer face), and each band's colour. The inner face
    // nobody sees is left open.
    let mut prof: Vec<(f32, f32, [f32; 3])> = vec![
        (r - 0.025, w - 0.01, TYRE),
        (r, w - 0.05, TYRE),
        (r, 0.05, TYRE),
        (r - 0.03, 0.005, TYRE),
        ((r + rr) * 0.5 + 0.01, -0.005, TYRE),
        (rr + 0.012, 0.012, TYRE),
    ];
    let face = match rim {
        Rim::Cap => STEEL,
        Rim::Spokes(c, _) | Rim::Mesh(c) => c,
    };
    match rim {
        Rim::Cap => {
            prof.push((rr, 0.018, face));
            prof.push((rr * 0.82, 0.03, face));
            prof.push((rr * 0.3, 0.015, face));
            prof.push((0.0, 0.01, face));
        }
        Rim::Spokes(..) | Rim::Mesh(_) => {
            prof.push((rr, 0.015, face));
            prof.push((rr * 0.94, 0.04, face));
            // The barrel's inside, dark, down to the disc.
            prof.push((rr * 0.9, 0.12, UNDER));
            prof.push((rr * 0.75, 0.125, DISC));
            prof.push((rr * 0.3, 0.125, DISC));
        }
    }
    let ang = |k: usize| (k % n) as f32 / n as f32 * std::f32::consts::TAU;
    let at = |rad: f32, depth: f32, a: f32| Vec3::new(x + rad * a.cos(), r + rad * a.sin(), z_out - s * depth);
    for p in prof.windows(2) {
        let (r0, d0, c) = (p[0].0, p[0].1, p[1].2);
        let (r1, d1) = (p[1].0, p[1].1);
        // Normal in the (radial, axial-out) plane, perpendicular to the band.
        let t = Vec2::new(r1 - r0, -(d1 - d0));
        let pn = Vec2::new(t.y, -t.x).normalize_or_zero();
        // Outward: away from the axle, or out of the face.
        let pn = if pn.x * (r0 + r1) * 0.5 + pn.y * 0.3 < 0.0 { -pn } else { pn };
        if (r0 - r1).abs() < 1e-5 && (d0 - d1).abs() < 1e-5 {
            continue;
        }
        for k in 0..n {
            let (a0, a1) = (ang(k), ang(k + 1));
            let nr = |a: f32| Vec3::new(pn.x * a.cos(), pn.x * a.sin(), pn.y * s).normalize_or_zero();
            let q = [at(r0, d0, a0), at(r0, d0, a1), at(r1, d1, a1), at(r1, d1, a0)];
            let nn = [nr(a0), nr(a1), nr(a1), nr(a0)];
            // Wound to face along the normal.
            let fwd = (q[1] - q[0]).cross(q[3] - q[0]).dot(nn[0]) >= 0.0;
            quad_n(m, q, nn, c, !fwd);
        }
    }
    let spoked = match rim {
        Rim::Spokes(c, k) => Some((c, k)),
        Rim::Mesh(c) => Some((c, if d.fine() { 10 } else { 7 })),
        Rim::Cap => None,
    };
    if let Some((c, count)) = spoked {
        if d.spokes() {
            let out = Vec3::Z * s;
            for i in 0..count {
                let a = i as f32 / count as f32 * std::f32::consts::TAU + 0.3;
                let (dir, side) = (Vec3::new(a.cos(), a.sin(), 0.0), Vec3::new(-a.sin(), a.cos(), 0.0));
                let hub = Vec3::new(x, r, z_out - s * 0.045);
                let (w0, w1) = if count > 6 { (0.022, 0.014) } else { (0.04, 0.026) };
                let (p0, p1) = (hub + dir * rr * 0.25, hub + dir * rr * 0.93 - out * 0.0);
                let q = [p0 - side * w0, p1 - side * w1, p1 + side * w1, p0 + side * w0];
                m.quad_facing(q, out, c4(c));
                if d.extras() {
                    // The spoke's two sides, back to the disc.
                    for (e0, e1) in [(q[0], q[1]), (q[3], q[2])] {
                        let back = -out * 0.06;
                        let mid = (e0 + e1) * 0.5 - hub;
                        m.quad_facing([e0, e1, e1 + back, e0 + back], (mid - dir * mid.dot(dir)).normalize_or_zero(), c4(mul(c, 0.7)));
                    }
                }
            }
            // The centre cap.
            let hub = Vec3::new(x, r, z_out - s * 0.04);
            let ring: Vec<Vec3> = (0..6).map(|k| hub + Vec3::new((k as f32 / 6.0 * std::f32::consts::TAU).cos() * rr * 0.27, (k as f32 / 6.0 * std::f32::consts::TAU).sin() * rr * 0.27, 0.0)).collect();
            m.cap(&ring, out, c4(mul(c, 0.8)));
        } else {
            // No spokes at this step: the face as a dish.
            let hub = Vec3::new(x, r, z_out - s * 0.05);
            let ring: Vec<Vec3> = (0..n).map(|k| hub + Vec3::new(ang(k).cos() * rr * 0.92, ang(k).sin() * rr * 0.92, 0.0)).collect();
            m.cap(&ring, Vec3::Z * s, c4(mul(c, 0.85)));
        }
    }
    if let (Some(cc), true) = (caliper, d.fine()) {
        // A brake caliper behind the spokes, at the top rear of the disc.
        m.boxed(Vec3::new(x - rr * 0.45, r + rr * 0.45, z_out - s * 0.11), Vec3::new(0.12, 0.12, 0.05), 0.0, c4(cc));
    }
}

// ── cars ────────────────────────────────────────────────────────────────────

/// The near car: hatch, saloon, coupe, SUV and pickup, on the same
/// dimensions as the far one ([`CarDims`]).
pub(super) fn car_near(b: &mut B, col: [f32; 3], d: NearDetail) {
    let m = *b.m;
    let dm = CarDims::of(&m);
    let CarDims { len, fx, rx, hw, k, tall, sill, belt, hood, roof, cab_f, roof_f, cab_r, roof_r, rear_top: rt, r, ww, whw, ax_f, ax_r, .. } = dm;
    let h = look_hash(&m);
    let boot = cab_r - rx > 0.4;
    let pickup = m.body == BodyType::Pickup;
    let coupe = m.body == BodyType::Coupe;
    // ── profiles ──
    let mut top_keys: Vec<(f32, f32)> = Vec::new();
    if boot && !pickup {
        top_keys.extend([(rx, rt - 0.16), (rx + 0.05, rt - 0.045), (rx + 0.16, rt - 0.005), (cab_r, rt + 0.02)]);
    } else if pickup {
        top_keys.extend([(rx, rt - 0.06), (rx + 0.04, rt + 0.02), (cab_r - 0.05, rt + 0.03), (cab_r, rt + 0.06)]);
    } else {
        top_keys.extend([(rx, rt - 0.06), (cab_r, rt + 0.02)]);
    }
    top_keys.extend([(roof_r, roof - 0.025), (roof_f, roof)]);
    let cowl = belt + 0.05;
    top_keys.push((cab_f, cowl));
    let bonnet_x = fx - 0.2 - 0.25 * k;
    if bonnet_x < cab_f - 0.15 {
        top_keys.push((bonnet_x, hood));
    }
    top_keys.extend([(fx - 0.06 - 0.1 * k, hood - 0.035 - 0.03 * k), (fx, hood - 0.08 - 0.12 * k)]);
    // Keep only keys strictly forward of the one before.
    top_keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    top_keys.dedup_by(|a, b| a.0 - b.0 < 0.02);
    let top_at = |x: f32| profile(&top_keys, x);
    let belt_keys = [(rx, rt - 0.05), (cab_r.max(rx + 0.2), rt), (cab_f, belt), (fx, hood - 0.12)];
    let belt_at = |x: f32| profile(&belt_keys, x).min(top_at(x) - 0.035);
    let g_at = |x: f32| -> f32 {
        if x >= cab_f || x <= cab_r {
            0.0
        } else if x >= roof_f {
            (cab_f - x) / (cab_f - roof_f).max(1e-3)
        } else if x <= roof_r {
            (x - cab_r) / (roof_r - cab_r).max(1e-3)
        } else {
            1.0
        }
    };
    let plan = |x: f32| profile(&[(rx, 0.87), (rx + 0.1, 0.96), (rx + 0.35, 1.0), (fx - 0.4, 1.0), (fx - 0.12, 0.955), (fx, 0.86)], x);
    let sill_at = |x: f32| profile(&[(rx, sill + 0.08), (rx + 0.28, sill), (fx - 0.32, sill), (fx, sill + 0.07)], x);
    let flare = if m.tuned { 0.07 } else if tall { 0.05 } else if coupe { 0.04 } else { 0.02 + 0.015 * k };
    let gap = 0.06 + if tall { 0.03 } else { 0.0 };
    let axles = [(ax_f, r), (ax_r, r)];
    let tumble = if tall { 0.12 } else { 0.16 + 0.05 * k };
    let (crown_d, crown_c) = (0.035, 0.045);
    // ── stations ──
    let mut xs = vec![rx, rx + 0.05, rx + 0.16, rx + 0.32, cab_r, roof_r, roof_f, cab_f, bonnet_x, fx - 0.32, fx - 0.06 - 0.1 * k, fx - 0.025, fx];
    xs.push(rx + 0.015);
    for i in 1..=d.roof_steps() {
        xs.push(lerp(roof_r, roof_f, i as f32 / (d.roof_steps() + 1) as f32));
    }
    for i in 1..=d.screen_steps() {
        xs.push(lerp(roof_f, cab_f, i as f32 / (d.screen_steps() + 1) as f32));
        if cab_r < roof_r - 0.25 {
            xs.push(lerp(cab_r, roof_r, i as f32 / (d.screen_steps() + 1) as f32));
        }
    }
    let bpx = (cab_f + cab_r.max(roof_r - 0.3)) * 0.5 - 0.1;
    let two_door = coupe || m.body == BodyType::Hatchback && m.class == VehicleClass::Cheap;
    let pillar = if pickup { (cab_r - 0.02, roof_r + 0.04) } else if two_door { (roof_r + 0.12, roof_r) } else { (bpx - 0.05, bpx + 0.05) };
    if !pickup && !two_door {
        xs.extend([pillar.0, pillar.1]);
    }
    if pickup {
        xs.extend([cab_r - 0.08, cab_r + 0.02]);
    }
    for &(ax, rr) in &axles {
        let ra = rr + gap;
        let n = d.arch_steps();
        for i in 0..=n {
            xs.push(ax + ra * (std::f32::consts::PI * i as f32 / n as f32).cos());
        }
        xs.push(ax + ra + 0.12);
        xs.push(ax - ra - 0.12);
    }
    xs.retain(|x| (rx..=fx).contains(x));
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.03 && *a != fx);
    if xs.last() != Some(&fx) {
        xs.push(fx);
    }
    // ── rings ──
    let mut rings = Vec::with_capacity(xs.len());
    for &x in &xs {
        let t = top_at(x);
        let bl = belt_at(x);
        let g = g_at(x);
        let w = hw * plan(x);
        let sl = sill_at(x);
        let mut ylow = sl;
        let mut fl: f32 = 0.0;
        for &(ax, rr) in &axles {
            let ra = rr + gap;
            let dx = (x - ax).abs();
            if dx < ra {
                ylow = ylow.max(rr + (ra * ra - dx * dx).sqrt());
            }
            fl = fl.max(flare * smooth(0.0, 1.0, 1.0 - dx / (ra + 0.14)));
        }
        let ylow = ylow.min(bl - 0.17);
        let (wl, wm, wb) = (w - 0.05, w, w - 0.035 - 0.015 * k);
        let wg = wb - tumble;
        let ymid = lerp(sl, bl, 0.5).max(ylow + 0.08).min(bl - 0.09);
        let deck = [Vec2::new(wb - 0.12, lerp(bl, t, 0.72)), Vec2::new(wb * 0.62, lerp(bl, t, 0.94)), Vec2::new(wb * 0.3, t + crown_d * 0.65), Vec2::new(0.0, t + crown_d)];
        let cabin = [Vec2::new(wg, t - 0.09), Vec2::new(wg - 0.06, t - 0.018), Vec2::new(wg * 0.5, t + crown_c * 0.72), Vec2::new(0.0, t + crown_c)];
        let mut ring = [
            Vec2::new(0.0, ylow),
            Vec2::new(wl - 0.1, ylow),
            Vec2::new(wl + fl, ylow + 0.07),
            Vec2::new(wm + fl * 0.8, ymid),
            Vec2::new(wb + fl * 0.2, bl - 0.045),
            Vec2::new(wb - 0.025, bl + 0.004),
            Vec2::ZERO,
            Vec2::ZERO,
            Vec2::ZERO,
            Vec2::ZERO,
        ];
        for j in 0..4 {
            ring[6 + j] = deck[j].lerp(cabin[j], g);
        }
        rings.push(ring);
    }
    // ── groups and colours ──
    let n = xs.len();
    let crease_x = [cab_f, fx - 0.025, rx + 0.015, if boot && !pickup { cab_r } else { f32::NAN }];
    let mut ugroup = Vec::with_capacity(n - 1);
    let mut g = 0u16;
    for i in 0..n - 1 {
        if i > 0 && crease_x.iter().any(|&c| (xs[i] - c).abs() < 1e-4) {
            g += 1;
        }
        ugroup.push(g);
    }
    let vgroup = [0, 1, 1, 1, 2, 3, 4, 4, 4];
    let pillar_col = match m.class {
        VehicleClass::Expensive => GLOSS_BLACK,
        VehicleClass::Mid => TRIM,
        VehicleClass::Cheap => col,
    };
    let cheap = m.class == VehicleClass::Cheap;
    let bumper = if cheap || tall { TRIM } else { col };
    // Two-tone: a gloss-black or contrasting roof on some dear and tuned cars.
    let tone2 = if m.livery != Livery::None {
        None
    } else if m.tuned && h % 3 == 0 {
        Some(if luma(col) > 0.3 { GLOSS_BLACK } else { [0.82, 0.82, 0.8] })
    } else if m.class == VehicleClass::Expensive && h % 4 == 1 {
        Some(GLOSS_BLACK)
    } else {
        None
    };
    let sunroof = m.class == VehicleClass::Expensive && d.extras() && h % 3 != 0 && !pickup;
    let mut cols = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let xm = (xs[i] + xs[i + 1]) * 0.5;
        let g = g_at(xm);
        let screen = xm < cab_f && xm > roof_f;
        let back = xm > cab_r && xm < roof_r && !pickup;
        let roofz = xm <= roof_f && xm >= roof_r;
        let in_pillar = xm > pillar.0.min(pillar.1) && xm < pillar.0.max(pillar.1);
        let end = xm > fx - 0.3 || xm < rx + 0.3;
        let arch = axles.iter().any(|&(ax, rr)| (xm - ax).abs() < rr + gap);
        let mut c = [col; BANDS];
        c[band::FLOOR] = UNDER;
        if end {
            c[band::ROCKER] = bumper;
            c[band::LOWER] = bumper;
        } else if arch && (tall || m.tuned) {
            c[band::ROCKER] = TRIM;
        } else if tall || cheap {
            c[band::ROCKER] = TRIM;
        }
        // Side glass, pillars, screens, roof.
        if g > 0.05 && !(screen && xm > cab_f - 0.12) {
            c[band::GLASS] = if in_pillar { pillar_col } else if back && boot && g < 0.6 { col } else { GLASS };
        }
        if screen || back {
            c[band::ROOF_OUT] = GLASS;
            c[band::ROOF_IN] = GLASS;
            c[band::PILLAR] = if m.class == VehicleClass::Expensive { GLOSS_BLACK } else { col };
        }
        if roofz {
            if let Some(t2) = tone2 {
                c[band::PILLAR] = t2;
                c[band::ROOF_OUT] = t2;
                c[band::ROOF_IN] = t2;
            }
            if sunroof && xm < roof_f - 0.15 && xm > roof_r + 0.35 {
                c[band::ROOF_IN] = GLASS;
            }
        }
        if roofz && tone2.is_none() && m.class == VehicleClass::Expensive {
            c[band::PILLAR] = col;
        }
        if pickup && xm < cab_r - 0.04 && xm > rx + 0.05 {
            // The bed's liner.
            c[band::PILLAR] = col;
            c[band::ROOF_OUT] = TRIM;
            c[band::ROOF_IN] = TRIM;
        }
        cols.push(c);
    }
    let loft = Loft { xs: xs.clone(), rings: rings.clone(), vgroup, ugroup, cols };
    loft.shell(&mut b.body);
    loft.caps(&mut b.body, col, col);
    // ── wheels ──
    let rim = match (m.class, m.tuned) {
        (_, true) => Rim::Spokes(if h % 2 == 0 { DARK_ALLOY } else { CHROME }, 5),
        (VehicleClass::Cheap, _) => Rim::Cap,
        (VehicleClass::Mid, _) => Rim::Spokes(ALLOY, if h % 2 == 0 { 5 } else { 6 }),
        (VehicleClass::Expensive, _) => Rim::Mesh(if h % 3 == 0 { DARK_ALLOY } else { ALLOY }),
    };
    let caliper = (m.tuned || m.class == VehicleClass::Expensive).then(|| if m.tuned { at(&b.pal.vehicle_neon, m.paint as usize).map(|v| (v * 0.35).min(0.9)) } else { [0.6, 0.06, 0.04] });
    for &(ax, _) in &axles {
        for s in [1.0f32, -1.0] {
            wheel(&mut b.body, d, ax, r, s * whw, ww, rim, caliper);
        }
    }
    // ── the face: grille, lamps, plate ──
    let fr = rings[n - 1];
    let fw = fr[3].x; // half width of the nose
    let (ftop, fbot) = (fr[5].y, fr[1].y);
    let fxo = fx + 0.004;
    let gy = lerp(fbot, ftop, 0.42);
    let gh = (ftop - fbot) * 0.32;
    let gw = fw * if m.class == VehicleClass::Expensive { 0.42 } else { 0.5 };
    if m.class == VehicleClass::Expensive {
        flat_poly_x(&mut b.body, fxo - 0.001, &round_rect(0.0, gy, gw + 0.03, gh * 0.5 + 0.03, 0.04, 2), 1.0, CHROME);
    }
    let grille: Vec<(f32, f32)> = if m.tuned { vec![(-gw * 1.2, gy - gh * 0.5), (gw * 1.2, gy - gh * 0.5), (gw * 0.9, gy + gh * 0.5), (-gw * 0.9, gy + gh * 0.5)] } else { round_rect(0.0, gy, gw, gh * 0.5, 0.03, 2) };
    flat_poly_x(&mut b.body, fxo, &grille, 1.0, GRILLE);
    if d.extras() && !m.tuned && m.class != VehicleClass::Cheap {
        for i in 0..3 {
            let y = gy - gh * 0.3 + i as f32 * gh * 0.3;
            b.body.decal(Vec3::new(fxo + 0.003, y, 0.0), Vec3::Z, Vec3::Y, gw * 1.9, 0.012, Vec3::X, c4(CHROME));
        }
        // The badge.
        b.body.decal(Vec3::new(fxo + 0.005, gy + gh * 0.1, 0.0), Vec3::Z, Vec3::Y, 0.07, 0.05, Vec3::X, c4(CHROME));
    }
    // Lower intake and plate.
    flat_poly_x(&mut b.body, fxo, &round_rect(0.0, fbot + 0.1, fw * 0.7, 0.045, 0.02, 1), 1.0, GRILLE);
    b.body.decal(Vec3::new(fxo + 0.003, fbot + 0.19, 0.0), Vec3::Z, Vec3::Y, 0.5, 0.11, Vec3::X, c4(PLATE));
    // Headlamps: a shape per class on the face, wrapping round the corner.
    let lamp_y = lerp(fbot, ftop, 0.78);
    let lz = fw * 0.62;
    let fl_x = fx + 0.008;
    let lamp_u = (n - 2) as f32;
    match m.class {
        VehicleClass::Cheap => {
            let dead = m.wear >= 2;
            for s in [1.0f32, -1.0] {
                let ring: Vec<(f32, f32)> = (0..10).map(|i| {
                    let a = i as f32 / 10.0 * std::f32::consts::TAU;
                    (s * lz + 0.085 * a.cos(), lamp_y - 0.01 + 0.075 * a.sin())
                }).collect();
                flat_poly_x(&mut b.lights, fl_x, &ring, 1.0, if dead && s < 0.0 { [0.05, 0.05, 0.05] } else { HEAD });
                b.lamp(fx, lamp_y - 0.11, s * (lz + 0.05), 0.09, 0.04, 1.0, AMBER);
            }
        }
        VehicleClass::Mid => {
            for s in [1.0f32, -1.0] {
                let pts = [(s * (lz - 0.17), lamp_y - 0.055), (s * (lz + 0.13), lamp_y - 0.035), (s * (lz + 0.15), lamp_y + 0.05), (s * (lz - 0.15), lamp_y + 0.035)];
                flat_poly_x(&mut b.lights, fl_x, &pts, 1.0, HEAD);
                b.lamp(fx, lamp_y + 0.055, s * lz, 0.28, 0.018, 1.0, [2.0, 2.3, 2.6]);
            }
            loft.patch(&mut b.lights, lamp_u, lamp_u + 0.9, 3.55, 4.4, 0.006, HEAD);
        }
        VehicleClass::Expensive => {
            b.lamp(fx, lamp_y + 0.02, 0.0, fw * 1.5, 0.035, 1.0, HEAD);
            for s in [1.0f32, -1.0] {
                // Vertical DRL fangs.
                b.lamp(fx, lamp_y - 0.06, s * (fw * 0.72), 0.025, 0.13, 1.0, [2.2, 2.4, 2.7]);
            }
            loft.patch(&mut b.lights, lamp_u + 0.2, lamp_u + 0.95, 3.85, 4.3, 0.006, HEAD);
        }
    }
    // ── the tail ──
    let tr = rings[0];
    let (ttop, tbot, tw) = (tr[5].y, tr[1].y, tr[3].x);
    let rxo = rx - 0.008;
    let ty = lerp(tbot, ttop, 0.72);
    match m.class {
        VehicleClass::Expensive => {
            b.lamp(rx, ty + 0.02, 0.0, tw * 1.75, 0.05, -1.0, TAIL);
            loft.patch(&mut b.lights, 0.0, 1.0, 3.7, 4.35, 0.006, TAIL);
        }
        VehicleClass::Mid => {
            for s in [1.0f32, -1.0] {
                let pts = [(s * (tw * 0.55), ty - 0.07), (s * (tw * 0.95), ty - 0.07), (s * (tw * 0.95), ty + 0.07), (s * (tw * 0.75), ty + 0.07), (s * (tw * 0.75), ty - 0.01), (s * (tw * 0.55), ty - 0.01)];
                flat_poly_x(&mut b.lights, rxo, &pts, -1.0, TAIL);
            }
            loft.patch(&mut b.lights, 0.0, 1.0, 3.4, 4.3, 0.006, TAIL);
        }
        VehicleClass::Cheap => {
            b.lamps(rx, ty - 0.02, tw * 0.75, 0.2, 0.18, -1.0, TAIL);
            b.lamps(rx, ty - 0.13, tw * 0.75, 0.2, 0.04, -1.0, AMBER);
        }
    }
    b.body.decal(Vec3::new(rxo - 0.002, lerp(tbot, ttop, 0.35), 0.0), Vec3::Z, Vec3::Y, 0.5, 0.11, -Vec3::X, c4(PLATE));
    if cheap || tall {
        // Black bumper faces.
        b.body.decal(Vec3::new(rxo, tbot + 0.07, 0.0), Vec3::Z, Vec3::Y, tw * 1.9, 0.12, -Vec3::X, c4(TRIM));
        b.body.decal(Vec3::new(fxo, fbot + 0.035, 0.0), Vec3::Z, Vec3::Y, fw * 1.9, 0.06, Vec3::X, c4(TRIM));
    }
    // Third brake lamp.
    if !pickup && m.class != VehicleClass::Cheap {
        let rr_x = if boot { cab_r + 0.04 } else { roof_r - 0.02 };
        b.lamp(rr_x, top_at(rr_x) + 0.03, 0.0, 0.3, 0.025, -1.0, TAIL);
    }
    // ── the sides: door lines, handles, mirrors ──
    let seam = mul(col, 0.3);
    let side_lo = |x: f32| loft.side_at(x, 0.0).y + 0.05;
    let door_lines: Vec<f32> = if pickup {
        vec![cab_f - 0.08, (cab_f + cab_r) * 0.5 + 0.1, cab_r + 0.03]
    } else if two_door {
        vec![cab_f - 0.08, (cab_f - 1.3).max(roof_r + 0.1)]
    } else {
        vec![cab_f - 0.08, bpx, (cab_r + 0.05).max(roof_r - 0.15)]
    };
    for &x in &door_lines {
        let y0 = side_lo(x);
        let y1 = belt_at(x) - 0.012;
        if y1 > y0 + 0.1 {
            loft.side_rect(&mut b.body, x - 0.006, x + 0.006, y0, y1, 0.003, seam);
        }
    }
    if d.extras() {
        // Handles, a little ahead of each door's back edge.
        for w in door_lines.windows(2) {
            let x = w[1] + 0.2;
            let y = belt_at(x) - 0.1;
            loft.side_rect(&mut b.body, x - 0.08, x + 0.08, y - 0.017, y + 0.017, 0.006, if m.class == VehicleClass::Expensive { CHROME } else { mul(col, 0.8) });
        }
        // Bonnet shut lines and the boot lid.
        let (bz0, bz1) = (rings[n - 2][5].x - 0.1, rings[n - 2][5].x - 0.088);
        for s in [1.0f32, -1.0] {
            loft.top_rect(&mut b.body, cab_f + 0.04, fx - 0.08, s * bz0, s * bz1, 0.003, seam);
        }
        if boot && !pickup {
            loft.top_rect(&mut b.body, cab_r - 0.07, cab_r - 0.058, -0.6, 0.6, 0.003, seam);
        }
        // Wipers at the foot of the screen.
        let wy = cowl + 0.035;
        for z in [0.35f32, -0.25] {
            b.body.beam(Vec3::new(cab_f - 0.06, wy, z - 0.3), Vec3::new(cab_f - 0.1, wy + 0.02, z + 0.25), 0.018, c4(TRIM));
        }
        // A shark-fin aerial on the roof.
        if !pickup && m.class != VehicleClass::Cheap {
            b.bx((roof_r + 0.15, roof + crown_c + 0.03, 0.0), (0.16, 0.06, 0.05), GLOSS_BLACK);
        }
        // Exhaust tips under the back.
        let tips: &[f32] = if m.tuned || m.class == VehicleClass::Expensive { &[0.45, -0.45] } else { &[-0.45] };
        for &z in tips {
            let rad = if m.tuned { 0.055 } else { 0.035 };
            b.body.tube(Vec3::new(rx + 0.12, tbot + 0.02, z), Vec3::new(rx - 0.03, tbot + 0.02, z), rad, 8, c4(CHROME));
        }
    }
    // Mirrors on the doors, by the screen pillar.
    let mx = cab_f - 0.18;
    let my = belt_at(mx) + 0.08;
    let mz = rings[(xs.iter().position(|&x| x >= mx).unwrap_or(n - 1)).min(n - 1)][5].x + 0.12;
    let mcol = if m.class == VehicleClass::Expensive { GLOSS_BLACK } else if cheap { TRIM } else { col };
    for s in [1.0f32, -1.0] {
        b.body.boxed(Vec3::new(mx, my, s * mz), Vec3::new(0.11, 0.1, 0.19), 0.0, c4(mcol));
        b.body.decal(Vec3::new(mx - 0.057, my, s * mz), Vec3::Z, Vec3::Y, 0.17, 0.08, -Vec3::X, c4(MIRROR));
        b.body.boxed(Vec3::new(mx + 0.02, my - 0.05, s * (mz - 0.1)), Vec3::new(0.06, 0.03, 0.08), 0.0, c4(mcol));
    }
    // ── the kit by kind ──
    if pickup {
        // Bed rails and the tailgate's top.
        let bx0 = rx + 0.04;
        let bx1 = cab_r - 0.06;
        let yb = top_at((bx0 + bx1) * 0.5) + 0.06;
        let zr = rings[2][5].x - 0.03;
        for s in [1.0f32, -1.0] {
            b.bx(((bx0 + bx1) * 0.5, yb, s * zr), (bx1 - bx0, 0.12, 0.07), col);
        }
        b.bx((bx0 + 0.03, yb, 0.0), (0.07, 0.12, zr * 2.0), col);
        b.bx((bx1 - 0.02, yb + 0.05, 0.0), (0.06, 0.22, zr * 2.0), col);
    }
    if m.body == BodyType::Suv {
        // Roof rails.
        let z = rings[n / 2][7].x - 0.04;
        for s in [1.0f32, -1.0] {
            b.bx(((roof_r + roof_f) * 0.5, roof + crown_c + 0.04, s * z), (roof_f - roof_r - 0.1, 0.035, 0.04), if m.class == VehicleClass::Expensive { CHROME } else { TRIM });
        }
        // A skid plate under the nose.
        b.body.decal(Vec3::new(fxo + 0.004, fbot + 0.03, 0.0), Vec3::Z, Vec3::Y, fw * 1.1, 0.07, Vec3::X, c4(ALLOY));
    }
    if m.class == VehicleClass::Expensive && d.extras() && coupe {
        // Side vents behind the front wheels.
        let x = ax_f - r - gap - 0.18;
        let y = lerp(sill, belt, 0.55);
        loft.side_quad(&mut b.body, [(x - 0.12, y - 0.06), (x + 0.1, y - 0.06), (x + 0.14, y + 0.06), (x - 0.08, y + 0.06)], 1.0, 0.004, GRILLE);
        loft.side_quad(&mut b.body, [(x - 0.12, y - 0.06), (x + 0.1, y - 0.06), (x + 0.14, y + 0.06), (x - 0.08, y + 0.06)], -1.0, 0.004, GRILLE);
    }
    // ── beat-up: a mismatched door, rust round the arches ──
    if cheap && m.wear >= 1 && b.detail {
        let door = if b.rng.random_bool(0.5) { PRIMER } else { let k = b.pal.vehicle_paint.len().max(1); mul(at(&b.pal.vehicle_paint, b.rng.random_range(0..k)), 0.85) };
        let s = if b.rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let (x0, x1) = (door_lines[0] + 0.012, door_lines[1] - 0.012);
        let (y0, y1) = (side_lo((x0 + x1) * 0.5) + 0.01, belt_at((x0 + x1) * 0.5) - 0.02);
        loft.side_quad(&mut b.body, [(x0, y0), (x1, y0), (x1, y1), (x0, y1)], s, 0.002, door);
    }
    if m.wear >= 1 && b.detail {
        let n_rust = if m.wear >= 2 { 4 } else { 1 };
        for _ in 0..n_rust {
            let (ax, rr) = axles[b.rng.random_range(0..2usize)];
            let s = if b.rng.random_bool(0.5) { 1.0 } else { -1.0 };
            let x = ax + b.rng.random_range(-0.35..0.35f32);
            let y = (rr * 2.0 + gap).min(belt - 0.2) + b.rng.random_range(-0.04..0.06f32);
            let (w, hh) = (b.rng.random_range(0.12..0.3f32), b.rng.random_range(0.05..0.12f32));
            loft.side_quad(&mut b.body, [(x - w, y - hh), (x + w, y - hh), (x + w * 0.8, y + hh), (x - w * 0.9, y + hh)], s, 0.004, RUST);
        }
    }
    // ── tuned: wraps, stripes, spoiler, neon ──
    let neon = at(&b.pal.vehicle_neon, m.paint as usize);
    if m.tuned {
        let wrap = if luma(col) > 0.25 { GLOSS_BLACK } else { mix(neon.map(|v| v.min(1.0)), [1.0, 1.0, 1.0], 0.2) };
        match h % 3 {
            0 => {
                // Twin racing stripes nose to tail.
                for (z0, z1) in [(0.07f32, 0.2f32), (-0.2, -0.07)] {
                    loft.top_rect(&mut b.body, rx + 0.02, fx - 0.03, z0, z1, 0.004, wrap);
                }
            }
            1 => {
                // A slash down each flank.
                let y0 = lerp(sill, belt, 0.25);
                let y1 = belt - 0.06;
                for s in [1.0f32, -1.0] {
                    loft.side_quad(&mut b.body, [(rx + 0.4, y0), (rx + 1.5, y0), (fx - 0.6, y1), (fx - 1.6, y1)], s, 0.004, wrap);
                }
            }
            _ => {
                // A wide band along the doors and a number on them.
                let y = lerp(sill, belt, 0.55);
                loft.side_rect(&mut b.body, rx + 0.35, fx - 0.35, y - 0.07, y + 0.05, 0.004, wrap);
                loft.side_rect(&mut b.body, bpx - 0.2, bpx + 0.2, y - 0.2, y + 0.18, 0.006, [0.9, 0.9, 0.88]);
            }
        }
        // Wing spoiler on stands over the tail.
        let sx = rx + 0.25;
        // Over the boot; on a hatch or SUV a lip at the roof's edge.
        let sy = (top_at(sx) + 0.3).min(roof + crown_c + 0.08);
        b.bx((sx, sy, 0.0), (0.32, 0.04, hw * 2.0 - 0.1), GLOSS_BLACK);
        for s in [1.0f32, -1.0] {
            b.bx((sx + 0.05, (top_at(sx) + sy) * 0.5, s * (hw - 0.25)), (0.08, sy - top_at(sx), 0.04), GLOSS_BLACK);
            b.bx((sx, sy + 0.03, s * (hw - 0.04)), (0.36, 0.1, 0.02), GLOSS_BLACK);
        }
        // Neon along the sills (wrapping the skin) and under the car.
        loft.side_rect(&mut b.lights, rx + 0.4, fx - 0.4, sill + 0.08, sill + 0.115, 0.008, neon);
        b.underglow(len * 0.85, hw * 1.05, neon);
        // Lit rings round the rims.
        if d.extras() {
            for &(ax, _) in &axles {
                for s in [1.0f32, -1.0] {
                    let z = s * (whw + 0.002);
                    let rr = r * 0.66 + 0.004;
                    let nseg = d.wheel_sides();
                    for i in 0..nseg {
                        let (a0, a1) = (i as f32 / nseg as f32 * std::f32::consts::TAU, (i + 1) as f32 / nseg as f32 * std::f32::consts::TAU);
                        let p = |a: f32, rad: f32| Vec3::new(ax + rad * a.cos(), r + rad * a.sin(), z);
                        b.lights.quad_facing([p(a0, rr), p(a1, rr), p(a1, rr + 0.018), p(a0, rr + 0.018)], Vec3::Z * s, c4(mul(neon, 0.7)));
                    }
                }
            }
        }
    }
    let _ = (ww, roof_f, len);
    b.loft = Some(loft);
}

fn luma(c: [f32; 3]) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

/// Mirrors on a van, lorry or bus cab: arms out from the cab's sides at
/// `x`, glass facing back.
pub(super) fn cab_mirrors(b: &mut B, x: f32, y: f32, hw: f32, tall: f32) {
    for s in [1.0f32, -1.0] {
        let z = s * (hw + 0.16);
        b.body.boxed(Vec3::new(x, y, z), Vec3::new(0.08, tall, 0.2), 0.0, c4(TRIM));
        b.body.decal(Vec3::new(x - 0.042, y, z), Vec3::Z, Vec3::Y, 0.17, tall - 0.04, -Vec3::X, c4(MIRROR));
        b.body.beam(Vec3::new(x, y + tall * 0.3, s * (hw - 0.02)), Vec3::new(x, y + tall * 0.3, z), 0.03, c4(TRIM));
    }
}

/// A grille and plate on a flat cab face at `x`.
pub(super) fn cab_face(b: &mut B, x: f32, y: f32, hw: f32, h: f32) {
    b.body.decal(Vec3::new(x + 0.006, y, 0.0), Vec3::Z, Vec3::Y, hw * 1.1, h, Vec3::X, c4(GRILLE));
    if b.near.is_some_and(|d| d.extras()) {
        for i in 0..4 {
            let yy = y - h * 0.36 + i as f32 * h * 0.24;
            b.body.decal(Vec3::new(x + 0.01, yy, 0.0), Vec3::Z, Vec3::Y, hw * 1.05, 0.02, Vec3::X, c4(CHROME));
        }
    }
    b.body.decal(Vec3::new(x + 0.008, y - h * 0.5 - 0.16, 0.0), Vec3::Z, Vec3::Y, 0.5, 0.11, Vec3::X, c4(PLATE));
}
