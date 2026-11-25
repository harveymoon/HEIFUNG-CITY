//! [r17-people] The near figure: the detailed person drawn within a few
//! dozen metres of the camera (and so always in the walk, drive and follow
//! views), in place of the low-poly figure of [`crate::people`].
//!
//! It is built from the same [`PersonKey`] (skin, hair, build, outfit in
//! the two favourite colours, wear, riding, sitting) in the same vertex
//! colours and on the same emissive palette cells, so the night trim lights
//! the same places and the swap from the far figure is not noticed. On top
//! of the key it takes a few [`NearExtras`]: what the person carries (a
//! shoulder bag, a backpack, a tote, a briefcase, a phone), an umbrella or
//! the hood up in the rain, an older person's stoop.
//!
//! The body is a skeleton of joints (hips, knees, ankles, shoulders, elbows,
//! wrists) with smooth round limbs between them, an elliptical torso lofted
//! through hips, waist, chest and shoulders, a head shaped from chin to
//! crown with a face (eyes, brows, nose, mouth, ears), hair with volume by
//! style, hands and shoes, and clothes by cut: short sleeves, a hood, a work
//! jacket's reflective band, a suit's shirt and tie, a dress's flared skirt,
//! a coat's long skirt.
//!
//! **The walk.** Every vertex carries its limb in the second UV set
//! ([`rig_aux`]): which limb it belongs to and the joints it turns about.
//! The renderer's vertex shader (`nd_app` `render/near_people.rs`) swings
//! the legs and arms (knees bending in the swing, elbows with the arm
//! forward, the body dipping at the long stride) from a per-figure phase
//! and amplitude in the mesh tag ([`walk_tag`]), so one shared mesh walks
//! at everyone's own pace and the instancing is kept. A rider's legs follow
//! the pedals round (a two-bone reach from the hip to the pedal, the cranks
//! turning). [`pose_vertex`] is the same motion in Rust, for the tests.

use crate::color::rgba;
use crate::people::{bright_cell, cell_uv, dim_cell, worn, PersonKey, BENCH_SEAT, SIT_BENCH, SKIN_STEPS};
use crate::MeshData;
use glam::{Vec2, Vec3};
use nd_sim::looks::{linear, skin_srgb, HairColor, HairStyle, Outfit, ShownHair, COLORS};

/// Most triangles of a near figure on foot (standing, walking, sitting).
pub const MAX_NEAR_TRIS: usize = 1400;
/// Most triangles of a near rider with their bike.
pub const MAX_NEAR_RIDER_TRIS: usize = 2000;

/// What a near figure carries in their hand or on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Carry {
    None,
    /// A bag on a strap across the chest, at the hip.
    ShoulderBag,
    /// A backpack (a child's school bag).
    Backpack,
    /// A tote or shopping bag in the left hand.
    Tote,
    /// A briefcase in the left hand (suits).
    Briefcase,
    /// A phone held up in the left hand, its screen lit.
    Phone,
}

/// The near figure's own extras, on top of the [`PersonKey`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NearExtras {
    pub carry: Carry,
    /// An umbrella up in the right hand (heavy rain or snow).
    pub umbrella: bool,
    /// A hoodie's hood up (heavy rain or snow, no umbrella).
    pub hood_up: bool,
    /// Older: a stoop and a shorter stride.
    pub old: bool,
}

/// A near figure's whole look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NearKey {
    pub base: PersonKey,
    pub extras: NearExtras,
}

/// Age from which a figure stoops a little.
pub const OLD_AGE: u8 = 72;

/// The extras of a citizen with `seed`, age and the base key, in rain or
/// not. Only hashes of the seed: no draw from any generator.
pub fn near_extras(k: &PersonKey, seed: u64, age: u8, rain: bool) -> NearExtras {
    let h = crate::hash::hash2(seed, 0x6E65_6172_5F66_6967);
    let roll = (h % 100) as u8;
    let outfit = Outfit::ALL[(k.outfit as usize).min(Outfit::ALL.len() - 1)];
    let carry = if k.ride || k.sit != 0 && k.child {
        if k.ride && roll < 25 { Carry::Backpack } else { Carry::None }
    } else if k.tourist {
        if roll < 30 { Carry::Phone } else { Carry::None }
    } else if k.child {
        if roll < 45 { Carry::Backpack } else { Carry::None }
    } else {
        match roll {
            0..=14 => Carry::ShoulderBag,
            15..=26 => Carry::Backpack,
            27..=36 => {
                if outfit == Outfit::Suit {
                    Carry::Briefcase
                } else {
                    Carry::Tote
                }
            }
            37..=49 if k.sit == 0 => Carry::Phone,
            37..=49 => Carry::None,
            _ if outfit == Outfit::Suit && roll < 70 => Carry::Briefcase,
            _ => Carry::None,
        }
    };
    let wet = rain && !k.ride && k.sit == 0;
    let hood = outfit == Outfit::Hoodie;
    // Most put an umbrella up; a hoodie's hood does instead for most.
    let up = (h >> 8) % 100;
    let umbrella = wet && !k.tourist && if hood { up < 30 } else { up < 72 };
    let carry = if umbrella && carry == Carry::Phone { Carry::None } else { carry };
    NearExtras { carry, umbrella, hood_up: wet && hood && !umbrella, old: age >= OLD_AGE && !k.child }
}

// ── the rig ────────────────────────────────────────────────────────────────

/// Limb codes in the rig channel. `L` is the -Z side (the figure faces +X).
pub mod limb {
    pub const RIGID: u8 = 0;
    pub const THIGH_L: u8 = 1;
    pub const THIGH_R: u8 = 2;
    pub const SHIN_L: u8 = 3;
    pub const SHIN_R: u8 = 4;
    pub const UPPER_ARM_L: u8 = 5;
    pub const UPPER_ARM_R: u8 = 6;
    pub const FOREARM_L: u8 = 7;
    pub const FOREARM_R: u8 = 8;
    pub const RIDE_THIGH_L: u8 = 10;
    pub const RIDE_THIGH_R: u8 = 11;
    pub const RIDE_SHIN_L: u8 = 12;
    pub const RIDE_SHIN_R: u8 = 13;
    pub const CRANK: u8 = 14;
}

/// A rider's hip, the bike's bottom bracket and crank, the ankle above the
/// pedal and the leg's two lengths (the shader has the same numbers).
pub const RIDE_HIP: Vec2 = Vec2::new(-0.17, 0.97);
pub const RIDE_BB: Vec2 = Vec2::new(0.0, 0.29);
pub const RIDE_CRANK: f32 = 0.17;
pub const RIDE_ANKLE: Vec2 = Vec2::new(-0.05, 0.07);
pub const RIDE_THIGH: f32 = 0.45;
pub const RIDE_SHIN: f32 = 0.45;
/// The crank angle the mesh is built at (left foot; the right is opposite).
pub const RIDE_REST: f32 = 0.0;

/// How far the body dips at the long stride, metres.
pub const WALK_DIP: f32 = 0.05;

/// A joint pivot packed to whole centimetres: x in -1.28..1.27, y in 0..2.55.
fn pack(p: Vec2) -> u32 {
    let x = ((p.x + 1.28) * 100.0).round().clamp(0.0, 255.0) as u32;
    let y = (p.y * 100.0).round().clamp(0.0, 255.0) as u32;
    (x << 8) | y
}

fn unpack(v: u32) -> Vec2 {
    Vec2::new((v >> 8) as f32 * 0.01 - 1.28, (v & 255) as f32 * 0.01)
}

/// The rig channel of a vertex: its limb, the joint it turns about and
/// (a lower limb) the joint its upper limb turns about.
pub fn rig_aux(code: u8, own: Vec2, parent: Vec2) -> [f32; 2] {
    [(code as u32 + 32 * pack(parent)) as f32, pack(own) as f32]
}

/// The mesh tag of a figure walking at `phase` (0..1 of a stride cycle of
/// two steps, or of one turn of the cranks) with the swing `amp` (0 stood
/// still, 1 a full walk).
pub fn walk_tag(phase: f32, amp: f32) -> u32 {
    let p = (phase.rem_euclid(1.0) * 65536.0) as u32 & 0xFFFF;
    let a = (amp.clamp(0.0, 1.0) * 255.0).round() as u32;
    p | (a << 16)
}

fn rot2(p: Vec2, pivot: Vec2, a: f32) -> Vec2 {
    let d = p - pivot;
    let (s, c) = a.sin_cos();
    pivot + Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c)
}

/// Thigh and shin angles (from +X, radians) of a rider's leg with the pedal
/// at crank angle `phi`: a two-bone reach from the hip to the ankle over the
/// pedal, the knee forward.
pub fn ride_leg(phi: f32) -> (f32, f32) {
    let ankle = RIDE_BB + Vec2::new(phi.cos(), phi.sin()) * RIDE_CRANK + RIDE_ANKLE;
    let d = ankle - RIDE_HIP;
    let len = d.length().clamp(0.05, RIDE_THIGH + RIDE_SHIN - 1e-3);
    let base = d.y.atan2(d.x);
    let cos_a = ((RIDE_THIGH * RIDE_THIGH + len * len - RIDE_SHIN * RIDE_SHIN) / (2.0 * RIDE_THIGH * len)).clamp(-1.0, 1.0);
    let thigh = base + cos_a.acos();
    let knee = RIDE_HIP + Vec2::new(thigh.cos(), thigh.sin()) * RIDE_THIGH;
    let s = ankle - knee;
    (thigh, s.y.atan2(s.x))
}

/// One vertex of a near figure posed by its rig channel `aux` and the mesh
/// tag: the same motion as the shader. Returns the position and normal.
pub fn pose_vertex(p: Vec3, n: Vec3, aux: [f32; 2], tag: u32) -> (Vec3, Vec3) {
    use std::f32::consts::{PI, TAU};
    let a0 = aux[0].round() as u32;
    let code = (a0 & 31) as u8;
    let parent = unpack(a0 >> 5);
    let own = unpack(aux[1].round() as u32);
    let phase = (tag & 0xFFFF) as f32 / 65536.0;
    let amp = ((tag >> 16) & 255) as f32 / 255.0;
    let phi = phase * TAU;
    let (s, c) = phi.sin_cos();
    let side = |l: u8| if code == l { 1.0 } else { -1.0 };
    // (angle about own, angle about parent), applied own first.
    let (a_own, a_parent, bob) = match code {
        limb::THIGH_L | limb::THIGH_R => (amp * 0.40 * s * side(limb::THIGH_L), 0.0, true),
        limb::SHIN_L | limb::SHIN_R => {
            let sd = side(limb::SHIN_L);
            let knee = amp * (0.08 + 0.62 * (c * sd).max(0.0));
            (-knee, amp * 0.40 * s * sd, true)
        }
        limb::UPPER_ARM_L | limb::UPPER_ARM_R => (-amp * 0.32 * s * side(limb::UPPER_ARM_L), 0.0, true),
        limb::FOREARM_L | limb::FOREARM_R => {
            let sd = side(limb::FOREARM_L);
            let elbow = amp * (0.05 + 0.30 * (-s * sd).max(0.0));
            (elbow, -amp * 0.32 * s * sd, true)
        }
        limb::RIDE_THIGH_L | limb::RIDE_THIGH_R | limb::RIDE_SHIN_L | limb::RIDE_SHIN_R => {
            let right = code == limb::RIDE_THIGH_R || code == limb::RIDE_SHIN_R;
            let off = if right { PI } else { 0.0 };
            let (t0, s0) = ride_leg(RIDE_REST + off);
            let (t1, s1) = ride_leg(phi + RIDE_REST + off);
            let dt = t1 - t0;
            if code == limb::RIDE_THIGH_L || code == limb::RIDE_THIGH_R {
                (dt, 0.0, false)
            } else {
                ((s1 - s0) - dt, dt, false)
            }
        }
        limb::CRANK => (phi, 0.0, false),
        _ => (0.0, 0.0, true),
    };
    let mut q = Vec2::new(p.x, p.y);
    let mut m = Vec2::new(n.x, n.y);
    if code != limb::RIGID {
        q = rot2(q, own, a_own);
        m = rot2(m, Vec2::ZERO, a_own);
        if a_parent != 0.0 {
            q = rot2(q, parent, a_parent);
            m = rot2(m, Vec2::ZERO, a_parent);
        }
    }
    let dip = if bob { -amp * WALK_DIP * (0.5 - 0.5 * (2.0 * phi).cos()) } else { 0.0 };
    (Vec3::new(q.x, q.y + dip, p.z), Vec3::new(m.x, m.y, n.z))
}

// ── building blocks ────────────────────────────────────────────────────────

struct B<'a> {
    m: &'a mut MeshData,
}

impl B<'_> {
    fn mark(&self) -> usize {
        self.m.positions.len()
    }

    /// Paint everything since `v0` on `cell` and give it the rig `rig`.
    fn done(&mut self, v0: usize, cell: usize, rig: [f32; 2]) {
        let uv = cell_uv(cell);
        for u in &mut self.m.uvs[v0..] {
            *u = uv;
        }
        self.m.set_aux(v0, rig);
    }

    /// A smooth loft through `rings` (each ring a closed loop), capped.
    fn loft(&mut self, rings: &[Vec<Vec3>], color: [f32; 3], cell: usize, rig: [f32; 2]) {
        let v0 = self.mark();
        self.m.strip(rings, rgba(color));
        self.done(v0, cell, rig);
    }

    /// A round limb through `pts` (point, radius), `sides` round.
    fn limb(&mut self, pts: &[(Vec3, f32)], sides: usize, color: [f32; 3], cell: usize, rig: [f32; 2]) {
        if pts.len() < 2 {
            return;
        }
        let rings: Vec<Vec<Vec3>> = (0..pts.len())
            .map(|i| {
                let prev = pts[i.saturating_sub(1)].0;
                let next = pts[(i + 1).min(pts.len() - 1)].0;
                let dir = (next - prev).normalize_or(Vec3::Y);
                let (u, v) = frame(dir);
                ring(pts[i].0, u, v, pts[i].1, pts[i].1, sides)
            })
            .collect();
        self.loft(&rings, color, cell, rig);
    }

    fn quad(&mut self, c: [Vec3; 4], out: Vec3, color: [f32; 3], cell: usize, rig: [f32; 2]) {
        let v0 = self.mark();
        self.m.quad_facing(c, out, rgba(color));
        self.done(v0, cell, rig);
    }

    /// A quad seen from both sides (a fin, a canopy's underside).
    fn quad2(&mut self, c: [Vec3; 4], out: Vec3, color: [f32; 3], cell: usize, rig: [f32; 2]) {
        self.quad(c, out, color, cell, rig);
        self.quad(c, -out, color, cell, rig);
    }

    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, out: Vec3, color: [f32; 3], cell: usize, rig: [f32; 2]) {
        let v0 = self.mark();
        self.m.tri_facing(a, b, c, out, rgba(color));
        self.done(v0, cell, rig);
    }

    /// A box with rounded-off corners along `axis` (a bag, a shoe): an
    /// elliptical loft of `sides` from `a` to `b`, `half` its half sizes
    /// across (along `up`, then sideways).
    #[allow(clippy::too_many_arguments)]
    fn block(&mut self, a: Vec3, b: Vec3, up: Vec3, half: (f32, f32), sides: usize, color: [f32; 3], cell: usize, rig: [f32; 2]) {
        let dir = (b - a).normalize_or(Vec3::X);
        let side = dir.cross(up).normalize_or(Vec3::Z);
        let up = side.cross(dir).normalize_or(Vec3::Y);
        let r = |c: Vec3| ring(c, up, side, half.0, half.1, sides);
        self.loft(&[r(a), r(b)], color, cell, rig);
    }
}

/// Two axes across `dir`: the first in the XY plane where it can be (so a
/// limb's ring is front-back and sideways), the second sideways.
fn frame(dir: Vec3) -> (Vec3, Vec3) {
    let side = if dir.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let u = side.cross(dir).normalize_or(Vec3::X);
    let v = dir.cross(u).normalize_or(Vec3::Z);
    (u, v)
}

/// A closed ring of `n` round `c`: radius `ru` along `u`, `rv` along `v`.
fn ring(c: Vec3, u: Vec3, v: Vec3, ru: f32, rv: f32, n: usize) -> Vec<Vec3> {
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            c + u * (t.cos() * ru) + v * (t.sin() * rv)
        })
        .collect()
}

/// A horizontal ellipse ring: `rx` front-back, `rz` sideways.
fn hring(c: Vec3, rx: f32, rz: f32, n: usize) -> Vec<Vec3> {
    ring(c, Vec3::X, Vec3::Z, rx, rz, n)
}

/// The knee or elbow between `a` and `c` with bones `l1`, `l2`, bent
/// towards `hint` (a straight limb if they do not reach so far).
fn two_bone(a: Vec3, c: Vec3, l1: f32, l2: f32, hint: Vec3) -> Vec3 {
    let d = c - a;
    let len = d.length();
    if len < 1e-4 {
        return a + hint.normalize_or(Vec3::X) * l1;
    }
    let dir = d / len;
    if len >= l1 + l2 - 1e-4 {
        return a + dir * (len * l1 / (l1 + l2));
    }
    let x = (l1 * l1 - l2 * l2 + len * len) / (2.0 * len);
    let h = (l1 * l1 - x * x).max(0.0).sqrt();
    let bend = (hint - dir * hint.dot(dir)).normalize_or(Vec3::X);
    a + dir * x + bend * h
}

fn rot_lean(p: Vec2, lean: f32) -> Vec2 {
    // Forward lean: the top moves towards +X.
    let (s, c) = lean.sin_cos();
    Vec2::new(p.x * c + p.y * s, -p.x * s + p.y * c)
}

// ── colours ────────────────────────────────────────────────────────────────

fn hair_rgb(k: &PersonKey) -> [f32; 3] {
    match k.hair {
        h @ 0..=5 => linear(HairColor::ALL[h as usize].srgb()),
        6 => linear(ShownHair::Grey.srgb()),
        7 => linear(ShownHair::White.srgb()),
        d => linear(COLORS[(d - 8) as usize].srgb),
    }
}

fn mul(c: [f32; 3], k: f32) -> [f32; 3] {
    c.map(|v| (v * k).clamp(0.0, 1.0))
}

/// Everything the parts are coloured with.
struct Look {
    skin: [f32; 3],
    main: ([f32; 3], usize),
    accent: ([f32; 3], usize),
    trim: ([f32; 3], usize),
    legs: ([f32; 3], usize),
    sleeve: ([f32; 3], usize),
    hair: ([f32; 3], usize),
    outfit: Outfit,
    bw: f32,
    head: f32,
}

fn look_of(k: &PersonKey) -> Look {
    let skin = linear(skin_srgb((k.skin as f32 + 0.5) / SKIN_STEPS as f32));
    let fav = k.fav.map(|f| f.min(COLORS.len() as u8 - 1));
    let main = worn(linear(COLORS[fav[0] as usize].srgb), k.wear);
    let accent = worn(linear(COLORS[fav[1] as usize].srgb), k.wear);
    // As the far figure: the trim glows in whichever favourite is a neon.
    let trim_idx = if COLORS[fav[1] as usize].neon || !COLORS[fav[0] as usize].neon { fav[1] } else { fav[0] };
    let trim = (linear(COLORS[trim_idx as usize].srgb), bright_cell(trim_idx));
    let (main, accent) = ((main, dim_cell(fav[0])), (accent, dim_cell(fav[1])));
    let outfit = Outfit::ALL[(k.outfit as usize).min(Outfit::ALL.len() - 1)];
    let bare_arms = matches!(outfit, Outfit::Tee | Outfit::Dress);
    let sleeve = if bare_arms { (skin, 0) } else { main };
    let legs = match outfit {
        Outfit::Suit => main,
        Outfit::Dress => (skin, 0),
        _ => accent,
    };
    let dyed = k.hair >= 8;
    let hair = (hair_rgb(k), if dyed { dim_cell(k.hair - 8) } else { 0 });
    let bw = match k.build {
        0 => 0.86,
        2 => 1.18,
        _ => 1.0,
    };
    Look { skin, main, accent, trim, legs, sleeve, hair, outfit, bw, head: if k.child { 1.28 } else { 1.0 } }
}

// ── the body ───────────────────────────────────────────────────────────────

/// Torso rings in its own frame: (height above the crotch, half depth,
/// half width, forward offset), times the build (the last is the neck).
const TORSO: [(f32, f32, f32, f32); 7] = [
    (0.00, 0.100, 0.165, 0.0),
    (0.10, 0.105, 0.172, 0.0),
    (0.20, 0.095, 0.160, 0.004),
    (0.36, 0.115, 0.180, 0.012),
    (0.54, 0.105, 0.200, 0.0),
    (0.62, 0.080, 0.180, -0.006),
    (0.68, 0.045, 0.055, 0.0),
];

/// The torso's frame: the crotch at `origin` (x, y), leaning `lean`
/// forward.
#[derive(Clone, Copy)]
struct Torso {
    origin: Vec2,
    lean: f32,
    bw: f32,
    belly: f32,
}

impl Torso {
    fn at(&self, local: Vec2, z: f32) -> Vec3 {
        let p = self.origin + rot_lean(local, self.lean);
        Vec3::new(p.x, p.y, z)
    }

    /// The torso's half depth, half width and forward offset at `h`.
    fn section(&self, h: f32) -> (f32, f32, f32) {
        let i = TORSO.iter().position(|r| r.0 >= h).unwrap_or(TORSO.len() - 1).max(1);
        let (a, b) = (TORSO[i - 1], TORSO[i]);
        let t = ((h - a.0) / (b.0 - a.0).max(1e-4)).clamp(0.0, 1.0);
        let l = |x: f32, y: f32| x + (y - x) * t;
        let neck = i == TORSO.len() - 1;
        let k = if neck { 1.0 + (self.bw - 1.0) * (1.0 - t) } else { self.bw };
        let belly = if (0.12..0.42).contains(&h) { self.belly } else { 0.0 };
        (l(a.1, b.1) * k + belly, l(a.2, b.2) * k, l(a.3, b.3))
    }

    /// A ring of the torso at `h`, grown by `grow` (a band over it).
    fn ring(&self, h: f32, grow: f32, n: usize) -> Vec<Vec3> {
        let (hx, hz, off) = self.section(h);
        let c = self.at(Vec2::new(off, h), 0.0);
        let fwd = Vec3::new(self.lean.cos(), -self.lean.sin(), 0.0);
        ring(c, fwd, Vec3::Z, hx + grow, hz + grow, n)
    }

    fn rings(&self, hs: &[f32], grow: f32, n: usize) -> Vec<Vec<Vec3>> {
        hs.iter().map(|&h| self.ring(h, grow, n)).collect()
    }

    fn hip(&self, s: f32) -> Vec3 {
        self.at(Vec2::new(0.0, 0.08), s * 0.09 * self.bw)
    }

    fn shoulder(&self, s: f32) -> Vec3 {
        self.at(Vec2::new(-0.005, 0.58), s * (0.175 * self.bw + 0.02))
    }

    fn neck_top(&self) -> Vec3 {
        // The neck leans less than the back: the head is held up.
        let base = self.at(Vec2::new(0.0, 0.62), 0.0);
        base + Vec3::new(0.012 + self.lean.sin() * 0.04, 0.125, 0.0)
    }
}

/// The joints of a figure's limbs (L = -Z side first).
struct Pose {
    torso: Torso,
    hips: [Vec3; 2],
    knees: [Vec3; 2],
    ankles: [Vec3; 2],
    /// Which way each foot points (+X standing).
    toes: [Vec3; 2],
    shoulders: [Vec3; 2],
    elbows: [Vec3; 2],
    wrists: [Vec3; 2],
    /// The rig code of each part, or rigid.
    walk: bool,
    ride: bool,
}

const THIGH: f32 = 0.43;
const SHIN: f32 = 0.41;
const UPPER_ARM: f32 = 0.30;
const FOREARM: f32 = 0.26;

fn standing(l: &Look, x: &NearExtras) -> Pose {
    let lean = if x.old { 0.16 } else { 0.0 };
    let torso = Torso { origin: Vec2::new(0.0, 0.80), lean, bw: l.bw, belly: if l.bw > 1.1 { 0.03 } else { 0.0 } };
    let hips = [torso.hip(-1.0), torso.hip(1.0)];
    let ankles = [Vec3::new(0.0, 0.085, -0.085 * l.bw), Vec3::new(0.0, 0.085, 0.085 * l.bw)];
    let knees = [0, 1].map(|i| two_bone(hips[i], ankles[i], THIGH, SHIN, Vec3::X));
    let shoulders = [torso.shoulder(-1.0), torso.shoulder(1.0)];
    let mut wrists = [0, 1].map(|i| shoulders[i] + Vec3::new(0.03, -0.54, if i == 0 { -0.035 } else { 0.035 }));
    let mut elbows = [0, 1].map(|i| two_bone(shoulders[i], wrists[i], UPPER_ARM, FOREARM, Vec3::new(-1.0, 0.0, 0.0)));
    // An umbrella up in the right hand, a phone up in the left.
    if x.umbrella {
        wrists[1] = Vec3::new(0.24, 1.17, 0.12 * l.bw);
        elbows[1] = two_bone(shoulders[1], wrists[1], UPPER_ARM, FOREARM, Vec3::new(-0.2, -1.0, 0.6));
    }
    if x.carry == Carry::Phone {
        wrists[0] = Vec3::new(0.25, 1.16, -0.07);
        elbows[0] = two_bone(shoulders[0], wrists[0], UPPER_ARM, FOREARM, Vec3::new(-0.3, -1.0, -0.5));
    }
    Pose { torso, hips, knees, ankles, toes: [Vec3::X; 2], shoulders, elbows, wrists, walk: true, ride: false }
}

fn seated_pose(l: &Look, bench: bool) -> Pose {
    let (origin, lean) = if bench { (Vec2::new(-0.1, BENCH_SEAT + 0.0), -0.04) } else { (Vec2::new(-0.08, 0.02), 0.16) };
    let torso = Torso { origin, lean, bw: l.bw, belly: if l.bw > 1.1 { 0.03 } else { 0.0 } };
    let hips = [torso.hip(-1.0), torso.hip(1.0)];
    let z = |s: f32, k: f32| s * k * l.bw;
    let (knees, ankles) = if bench {
        ([-1.0, 1.0].map(|s| Vec3::new(0.34, BENCH_SEAT + 0.08, z(s, 0.1))), [-1.0, 1.0].map(|s| Vec3::new(0.40, 0.085, z(s, 0.1))))
    } else {
        ([-1.0, 1.0].map(|s| Vec3::new(0.24, 0.46, z(s, 0.12))), [-1.0, 1.0].map(|s| Vec3::new(0.40, 0.085, z(s, 0.1))))
    };
    let shoulders = [torso.shoulder(-1.0), torso.shoulder(1.0)];
    let wrists = if bench { [-1.0, 1.0].map(|s| Vec3::new(0.22, BENCH_SEAT + 0.17, z(s, 0.14))) } else { [0, 1].map(|i| knees[i] + Vec3::new(0.03, 0.05, if i == 0 { 0.06 } else { -0.06 })) };
    let elbows = [0, 1].map(|i| two_bone(shoulders[i], wrists[i], UPPER_ARM, FOREARM, Vec3::new(-0.6, -1.0, if i == 0 { -0.3 } else { 0.3 })));
    Pose { torso, hips, knees, ankles, toes: [Vec3::X; 2], shoulders, elbows, wrists, walk: false, ride: false }
}

fn riding_pose(l: &Look) -> Pose {
    let lean = 0.35;
    let hip_local = rot_lean(Vec2::new(0.0, 0.08), lean);
    let torso = Torso { origin: RIDE_HIP - hip_local, lean, bw: l.bw, belly: if l.bw > 1.1 { 0.03 } else { 0.0 } };
    let hips = [torso.hip(-1.0), torso.hip(1.0)];
    let mut knees = [Vec3::ZERO; 2];
    let mut ankles = [Vec3::ZERO; 2];
    for (i, off) in [0.0f32, std::f32::consts::PI].into_iter().enumerate() {
        let (t, s) = ride_leg(RIDE_REST + off);
        let k = RIDE_HIP + Vec2::new(t.cos(), t.sin()) * RIDE_THIGH;
        let a = k + Vec2::new(s.cos(), s.sin()) * RIDE_SHIN;
        knees[i] = Vec3::new(k.x, k.y, hips[i].z);
        ankles[i] = Vec3::new(a.x, a.y, hips[i].z * 1.25);
    }
    let shoulders = [torso.shoulder(-1.0), torso.shoulder(1.0)];
    let wrists = [-1.0f32, 1.0].map(|s| Vec3::new(0.33, 1.13, s * 0.22));
    let elbows = [0, 1].map(|i| two_bone(shoulders[i], wrists[i], UPPER_ARM, FOREARM, Vec3::new(-0.3, -1.0, if i == 0 { -0.8 } else { 0.8 })));
    Pose { torso, hips, knees, ankles, toes: [Vec3::X; 2], shoulders, elbows, wrists, walk: false, ride: true }
}

/// The rig channel of each part of a pose.
struct Rigs {
    thigh: [[f32; 2]; 2],
    shin: [[f32; 2]; 2],
    upper: [[f32; 2]; 2],
    fore: [[f32; 2]; 2],
}

fn rigs_of(p: &Pose, x: &NearExtras) -> Rigs {
    let v2 = |v: Vec3| Vec2::new(v.x, v.y);
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let mut r = Rigs { thigh: [none; 2], shin: [none; 2], upper: [none; 2], fore: [none; 2] };
    for i in 0..2 {
        if p.walk {
            r.thigh[i] = rig_aux(limb::THIGH_L + i as u8, v2(p.hips[i]), Vec2::ZERO);
            r.shin[i] = rig_aux(limb::SHIN_L + i as u8, v2(p.knees[i]), v2(p.hips[i]));
            let held = (i == 1 && x.umbrella) || (i == 0 && x.carry == Carry::Phone);
            if !held {
                r.upper[i] = rig_aux(limb::UPPER_ARM_L + i as u8, v2(p.shoulders[i]), Vec2::ZERO);
                r.fore[i] = rig_aux(limb::FOREARM_L + i as u8, v2(p.elbows[i]), v2(p.shoulders[i]));
            }
        } else if p.ride {
            r.thigh[i] = rig_aux(limb::RIDE_THIGH_L + i as u8, RIDE_HIP, Vec2::ZERO);
            r.shin[i] = rig_aux(limb::RIDE_SHIN_L + i as u8, v2(p.knees[i]), RIDE_HIP);
        }
    }
    r
}

/// The figure's body: torso, limbs, hands, shoes, head and hair, clothes.
fn body(b: &mut B, k: &PersonKey, x: &NearExtras, l: &Look, p: &Pose, r: &Rigs) {
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let t = &p.torso;
    let o = l.outfit;
    let seated = !p.walk && !p.ride;
    // Legs: thigh and shin, round, the calf a little full. A dress shows
    // the legs; seated or riding, its skirt lies over the thighs.
    let thigh_col = if o == Outfit::Dress && !p.walk { l.main } else { l.legs };
    for i in 0..2 {
        let (h, kn, a) = (p.hips[i], p.knees[i], p.ankles[i]);
        let top = h + (h - kn).normalize_or(Vec3::Y) * 0.05;
        b.limb(&[(top, 0.078 * l.bw), (h.lerp(kn, 0.45), 0.07 * l.bw), (kn, 0.054 * l.bw.sqrt())], 6, thigh_col.0, thigh_col.1, r.thigh[i]);
        b.limb(&[(kn, 0.053 * l.bw.sqrt()), (kn.lerp(a, 0.35), 0.056 * l.bw.sqrt()), (a, 0.036)], 6, l.legs.0, l.legs.1, r.shin[i]);
        shoe(b, k, l, a, p.toes[i], r.shin[i]);
    }
    // The torso: trousers (or the skirt's top) up to the waistband, the top
    // over it.
    let pelvis = if o == Outfit::Dress || o == Outfit::Coat { l.main } else { l.legs };
    b.loft(&t.rings(&[0.0, 0.06, 0.12], 0.0, 10), pelvis.0, pelvis.1, none);
    b.loft(&t.rings(&[0.12, 0.2, 0.36, 0.54, 0.62, 0.68], 0.0, 10), l.main.0, l.main.1, none);
    // Arms: sleeves (short on a tee), hands.
    for i in 0..2 {
        let (s, e, w) = (p.shoulders[i], p.elbows[i], p.wrists[i]);
        let s_in = s + (s - e).normalize_or(Vec3::Y) * 0.03;
        let ra = 0.05 * l.bw.sqrt();
        if o == Outfit::Tee {
            let mid = s.lerp(e, 0.45);
            b.limb(&[(s_in, ra + 0.01), (mid, ra + 0.006)], 6, l.main.0, l.main.1, r.upper[i]);
            b.limb(&[(mid, ra * 0.92), (e, 0.04)], 6, l.skin, 0, r.upper[i]);
        } else {
            b.limb(&[(s_in, ra), (s.lerp(e, 0.5), ra * 0.95), (e, 0.042)], 6, l.sleeve.0, l.sleeve.1, r.upper[i]);
        }
        b.limb(&[(e, 0.041), (e.lerp(w, 0.4), 0.039), (w, 0.03)], 6, l.sleeve.0, l.sleeve.1, r.fore[i]);
        // The hand: a flat mitt on from the wrist, the thumb along it.
        let dir = (w - e).normalize_or(-Vec3::Y);
        let gloved = o == Outfit::WorkJacket;
        let hand = if gloved { [0.06, 0.055, 0.05] } else { l.skin };
        let side = if i == 0 { -Vec3::Z } else { Vec3::Z };
        b.block(w, w + dir * 0.09, side, (0.017, 0.038), 6, hand, 0, r.fore[i]);
    }
    // Clothes by cut.
    let band = |b: &mut B, h0: f32, h1: f32, c: ([f32; 3], usize)| b.loft(&t.rings(&[h0, h1], 0.008, 10), c.0, c.1, none);
    match o {
        Outfit::Tee => band(b, 0.07, 0.12, l.trim),
        Outfit::Hoodie => {
            band(b, 0.06, 0.13, l.trim);
            // The front pocket, and the drawstrings in the trim.
            let (hx, _, off) = t.section(0.24);
            let f = t.at(Vec2::new(off + hx + 0.006, 0.24), 0.0);
            let up = Vec3::new(t.lean.sin(), t.lean.cos(), 0.0);
            b.quad([f + Vec3::new(0.0, 0.0, -0.1 * l.bw) - up * 0.06, f + Vec3::new(0.0, 0.0, 0.1 * l.bw) - up * 0.06, f + Vec3::new(0.0, 0.0, 0.08 * l.bw) + up * 0.06, f + Vec3::new(0.0, 0.0, -0.08 * l.bw) + up * 0.06], Vec3::X, mul(l.main.0, 0.8), l.main.1, none);
            let (hx, _, off) = t.section(0.5);
            for s in [-1.0f32, 1.0] {
                let a = t.at(Vec2::new(off + hx + 0.004, 0.58), s * 0.035);
                let c = t.at(Vec2::new(off + hx + 0.008, 0.42), s * 0.04);
                b.limb(&[(a, 0.006), (c, 0.006)], 3, l.trim.0, l.trim.1, none);
            }
            hood(b, l, p, x.hood_up);
        }
        Outfit::WorkJacket => {
            band(b, 0.38, 0.44, l.trim);
            band(b, 0.10, 0.16, (mul(l.main.0, 0.7), l.main.1));
            // The collar.
            b.loft(&t.rings(&[0.62, 0.68], 0.018, 10), l.main.0, l.main.1, none);
        }
        Outfit::Suit => {
            // The shirt's V and the tie (the trim) down it.
            let (hx, _, off) = t.section(0.5);
            let f = |h: f32, z: f32| t.at(Vec2::new(off + hx + 0.004, h), z);
            let shirt = [0.82, 0.82, 0.8];
            b.tri(f(0.62, -0.075), f(0.62, 0.075), f(0.32, 0.0), Vec3::X, shirt, 0, none);
            let (hx2, _, off2) = t.section(0.4);
            let g = |h: f32, z: f32| t.at(Vec2::new(off2.max(off) + hx2.max(hx) + 0.008, h), z);
            b.quad([g(0.34, -0.014), g(0.34, 0.014), g(0.6, 0.012), g(0.6, -0.012)], Vec3::X, l.trim.0, l.trim.1, none);
            band(b, 0.1, 0.13, (mul(l.main.0, 0.6), l.main.1));
        }
        Outfit::Dress => {
            band(b, 0.18, 0.23, l.trim);
            if p.walk {
                // The flared skirt, clear of the stride.
                let hem = 0.5 - t.origin.y;
                let rings: Vec<Vec<Vec3>> = [(0.2, 0.0), (0.0, 0.03), (-0.15, 0.08), (hem, 0.12)].iter().map(|&(h, g)| {
                    let (hx, hz, _) = t.section(h.max(0.0));
                    ring(t.at(Vec2::new(0.0, h), 0.0), Vec3::X, Vec3::Z, hx + g * 1.1 + 0.01, hz + g + 0.01, 10)
                }).collect();
                b.loft(&rings, l.main.0, l.main.1, none);
            }
        }
        Outfit::Coat => {
            band(b, 0.6, 0.69, l.trim);
            band(b, 0.19, 0.23, (mul(l.main.0, 0.65), l.main.1));
            if p.walk {
                let hem = 0.42 - t.origin.y;
                let rings: Vec<Vec<Vec3>> = [(0.18, 0.012), (0.0, 0.03), (hem, 0.075)].iter().map(|&(h, g)| {
                    let (hx, hz, _) = t.section(h.max(0.0));
                    ring(t.at(Vec2::new(0.0, h), 0.0), Vec3::X, Vec3::Z, hx + g * 1.2, hz + g, 10)
                }).collect();
                b.loft(&rings, l.main.0, l.main.1, none);
            } else if seated {
                // The coat's skirt over the seat.
                b.loft(&t.rings(&[0.0, 0.12], 0.03, 10), l.main.0, l.main.1, none);
            }
        }
    }
    // Neck and head.
    let neck0 = t.at(Vec2::new(0.0, 0.6), 0.0);
    let neck1 = t.neck_top();
    b.limb(&[(neck0, 0.045), (neck1, 0.042)], 6, l.skin, 0, none);
    head(b, k, l, neck1 - Vec3::new(0.0, 0.03, 0.0), x.hood_up);
    carry(b, k, l, p, x, r);
}

/// A shoe at the ankle `a`, pointing `toe`.
fn shoe(b: &mut B, k: &PersonKey, l: &Look, a: Vec3, toe: Vec3, rig: [f32; 2]) {
    let o = l.outfit;
    let c = match o {
        Outfit::Tee | Outfit::Hoodie => {
            if k.wear == 0 {
                [0.35, 0.33, 0.3]
            } else {
                [0.75, 0.74, 0.72]
            }
        }
        Outfit::WorkJacket => [0.16, 0.09, 0.04],
        Outfit::Suit => [0.025, 0.02, 0.02],
        Outfit::Dress => mul(l.accent.0, 0.6),
        Outfit::Coat => [0.04, 0.035, 0.03],
    };
    let toe = Vec3::new(toe.x, 0.0, toe.z).normalize_or(Vec3::X);
    let foot = Vec3::new(a.x, a.y.max(0.04), a.z);
    // Soles on the ground under an ankle 0.085 m up.
    let heel = foot - toe * 0.06 + Vec3::new(0.0, -0.05, 0.0);
    let mid = foot + toe * 0.07 + Vec3::new(0.0, -0.045, 0.0);
    let tip = foot + toe * 0.17 + Vec3::new(0.0, -0.063, 0.0);
    let side = toe.cross(Vec3::Y).normalize_or(Vec3::Z);
    let r = |p: Vec3, h: f32, w: f32| ring(p, Vec3::Y, side, h, w, 6);
    b.loft(&[r(heel, 0.035, 0.042), r(mid, 0.04, 0.05), r(tip, 0.022, 0.04)], c, 0, rig);
}

/// The head: chin to crown in elliptical rings, the face towards +X, and
/// the hair. `neck` is the bottom of the chin.
fn head(b: &mut B, k: &PersonKey, l: &Look, neck: Vec3, hood_up: bool) {
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let s = l.head;
    let hh = 0.25 * s;
    // (fraction of the height, half depth, half width, forward offset).
    const P: [(f32, f32, f32, f32); 8] = [(0.0, 0.03, 0.035, 0.035), (0.1, 0.07, 0.06, 0.02), (0.28, 0.095, 0.075, 0.01), (0.5, 0.105, 0.083, 0.0), (0.68, 0.106, 0.085, -0.006), (0.84, 0.092, 0.075, -0.01), (0.95, 0.058, 0.05, -0.012), (1.0, 0.012, 0.012, -0.012)];
    let at = |f: f32| -> (f32, f32, f32, f32) {
        let i = P.iter().position(|r| r.0 >= f).unwrap_or(P.len() - 1).max(1);
        let (a, c) = (P[i - 1], P[i]);
        let t = ((f - a.0) / (c.0 - a.0)).clamp(0.0, 1.0);
        let m = |x: f32, y: f32| x + (y - x) * t;
        (neck.y + f * hh, m(a.1, c.1) * s, m(a.2, c.2) * s, neck.x + m(a.3, c.3) * s)
    };
    let rings: Vec<Vec<Vec3>> = P.iter().map(|r| {
        let (y, rx, rz, xo) = at(r.0);
        hring(Vec3::new(xo, y, neck.z), rx, rz, 10)
    }).collect();
    b.loft(&rings, l.skin, 0, none);
    // The face: eyes and brows, a nose, the mouth, ears.
    let front = |f: f32, z: f32| {
        let (y, rx, rz, xo) = at(f);
        let q = (1.0 - (z / rz).powi(2)).max(0.0).sqrt();
        Vec3::new(xo + rx * q * 0.985 + 0.003, y, neck.z + z)
    };
    let dark = [0.02, 0.018, 0.02];
    let lips = mul(l.skin, 0.62);
    for zs in [-1.0f32, 1.0] {
        let z = zs * 0.034 * s;
        let c = front(0.56, z);
        let (w, h) = (0.012 * s, 0.007 * s);
        b.quad([c + Vec3::new(0.0, -h, -w), c + Vec3::new(0.0, -h, w), c + Vec3::new(0.0, h, w), c + Vec3::new(0.0, h, -w)], Vec3::X, dark, 0, none);
        let br = front(0.65, z) + Vec3::new(0.002, 0.0, 0.0);
        let (w, h) = (0.016 * s, 0.005 * s);
        b.quad([br + Vec3::new(0.0, -h, -w), br + Vec3::new(0.0, -h, w), br + Vec3::new(0.0, h, w), br + Vec3::new(0.0, h, -w)], Vec3::X, mul(l.hair.0, 0.8), 0, none);
        // The ear.
        let (y, _, rz, xo) = at(0.5);
        let e = Vec3::new(xo - 0.012 * s, y, neck.z + zs * (rz - 0.004));
        b.quad([e + Vec3::new(-0.018 * s, -0.03 * s, 0.0), e + Vec3::new(0.012 * s, -0.03 * s, 0.0), e + Vec3::new(0.012 * s, 0.025 * s, zs * 0.008), e + Vec3::new(-0.02 * s, 0.025 * s, zs * 0.008)], Vec3::Z * zs, mul(l.skin, 0.92), 0, none);
    }
    let top = front(0.55, 0.0);
    let base = front(0.38, 0.0);
    let tip = base + Vec3::new(0.022 * s, 0.006 * s, 0.0);
    let nl = base + Vec3::new(-0.002, 0.0, -0.014 * s);
    let nr = base + Vec3::new(-0.002, 0.0, 0.014 * s);
    let nose = mul(l.skin, 0.95);
    b.tri(top, tip, nl, Vec3::new(1.0, 0.3, -1.0), nose, 0, none);
    b.tri(top, nr, tip, Vec3::new(1.0, 0.3, 1.0), nose, 0, none);
    b.tri(nl, tip, nr, Vec3::new(0.3, -1.0, 0.0), mul(l.skin, 0.8), 0, none);
    let mo = front(0.25, 0.0) + Vec3::new(0.001, 0.0, 0.0);
    let (w, h) = (0.022 * s, 0.005 * s);
    b.quad([mo + Vec3::new(0.0, -h, -w), mo + Vec3::new(0.0, -h, w), mo + Vec3::new(0.0, h, w), mo + Vec3::new(0.0, h, -w)], Vec3::X, lips, 0, none);
    if !hood_up {
        hair(b, k, l, &at, neck);
    }
}

/// A cap of hair over the head from the fraction `low` up, `vol` out from
/// the scalp, swept back off the face below `line` (the hairline).
fn hair_shell(at: &dyn Fn(f32) -> (f32, f32, f32, f32), low: f32, line: f32, vol: f32, s: f32, z0: f32) -> Vec<Vec<Vec3>> {
    let mut fs: Vec<f32> = Vec::new();
    let n = 6;
    for i in 0..=n {
        fs.push(low + (1.0 - low) * i as f32 / n as f32);
    }
    fs.iter().map(|&f| {
        let (y, rx, rz, xo) = at(f.clamp(0.0, 1.0));
        let (y, rx, rz) = if f < 0.0 { (at(0.0).0 + f * 0.25 * s, 0.05 * s, 0.09 * s) } else { (y, rx, rz) };
        let back = if f >= line { 0.0 } else { ((line - f) / (line - low).max(1e-3)).powf(0.7) * (rx + vol) * 1.05 };
        let top = f >= 0.999;
        let (rx, rz) = if top { ((rx + vol) * 0.35, (rz + vol) * 0.35) } else { (rx + vol, rz + vol) };
        let yy = if top { y + vol * 0.9 } else { y };
        let xo = if f < 0.0 { at(0.0).3 - 0.1 * s } else { xo };
        hring(Vec3::new(xo - back, yy, z0), rx, rz, 10)
    }).collect()
}

fn hair(b: &mut B, k: &PersonKey, l: &Look, at: &dyn Fn(f32) -> (f32, f32, f32, f32), neck: Vec3) {
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let s = l.head;
    let (c, cell) = l.hair;
    let style = HairStyle::ALL[(k.style as usize).min(HairStyle::ALL.len() - 1)];
    let shell = |b: &mut B, low: f32, line: f32, vol: f32| {
        let rings = hair_shell(at, low, line, vol * s, s, neck.z);
        b.loft(&rings, c, cell, none);
    };
    match style {
        HairStyle::Short => shell(b, 0.42, 0.82, 0.014),
        HairStyle::Long => {
            shell(b, 0.4, 0.84, 0.016);
            // Down the back past the shoulders.
            let (y0, _, _, xo) = at(0.45);
            let rings: Vec<Vec<Vec3>> = [(0.0, 0.07, 0.085), (-0.18, 0.05, 0.1), (-0.36, 0.035, 0.1)].iter().map(|&(dy, rx, rz): &(f32, f32, f32)| hring(Vec3::new(xo - 0.085 * s - dy.abs() * 0.12, y0 + dy * s, neck.z), rx * s, rz * s, 8)).collect();
            b.loft(&rings, c, cell, none);
        }
        HairStyle::Bun => {
            shell(b, 0.45, 0.84, 0.009);
            let (y, rx, _, xo) = at(0.9);
            let ctr = Vec3::new(xo - rx * 0.8, y + 0.05 * s, neck.z);
            let r = 0.05 * s;
            let rings: Vec<Vec<Vec3>> = [(-0.8, 0.5), (-0.3, 0.95), (0.3, 0.95), (0.8, 0.5), (1.0, 0.1)].iter().map(|&(dy, k)| hring(ctr + Vec3::new(0.0, dy * r, 0.0), r * k, r * k, 6)).collect();
            b.loft(&rings, c, cell, none);
        }
        HairStyle::Buzz => shell(b, 0.5, 0.86, 0.005),
        HairStyle::Bald => {}
        HairStyle::Mohawk => {
            shell(b, 0.55, 0.9, 0.003);
            // The crest: a fin of spikes front to back, lit if dyed.
            let cell = if k.hair >= 8 { bright_cell(k.hair - 8) } else { cell };
            let (y, rx, _, xo) = at(0.9);
            let n = 5;
            for i in 0..n {
                let t0 = i as f32 / n as f32;
                let t1 = (i as f32 + 1.0) / n as f32;
                let x0 = xo + rx * (0.7 - 1.6 * t0);
                let x1 = xo + rx * (0.7 - 1.6 * t1);
                let base = |x: f32| Vec3::new(x, y + 0.02 * s * (1.0 - ((x - xo) / rx).powi(2)).max(0.0) + 0.01 * s, neck.z);
                let tip = Vec3::new((x0 + x1) * 0.5 - 0.02 * s, y + 0.13 * s, neck.z);
                b.quad2([base(x0), base(x1), tip, tip + Vec3::new(0.001, 0.0, 0.0)], Vec3::Z, c, cell, none);
            }
        }
        HairStyle::Afro => shell(b, 0.36, 0.74, 0.06),
    }
}

/// A hoodie's hood: up over the head (rain) or bunched behind the neck.
fn hood(b: &mut B, l: &Look, p: &Pose, up: bool) {
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let t = &p.torso;
    if up {
        // Built over the head in `head` instead of the hair (see there).
        let neck = t.neck_top() - Vec3::new(0.0, 0.03, 0.0);
        let s = l.head;
        let hh = 0.25 * s;
        let at = |f: f32| -> (f32, f32, f32, f32) {
            let rx = 0.105 * s * (1.0 - (2.0 * f - 1.0).powi(2) * 0.55).sqrt();
            let rz = 0.085 * s * (1.0 - (2.0 * f - 1.0).powi(2) * 0.55).sqrt();
            (neck.y + f * hh, rx, rz, neck.x - 0.005)
        };
        let rings = hair_shell(&at, 0.05, 0.97, 0.03 * s, s, neck.z);
        b.loft(&rings, l.main.0, l.main.1, none);
    } else {
        let c = t.at(Vec2::new(-0.1, 0.66), 0.0);
        let rings: Vec<Vec<Vec3>> = [(-0.05, 0.5), (0.0, 1.0), (0.05, 0.6)].iter().map(|&(dy, k)| hring(c + Vec3::new(0.0, dy, 0.0), 0.06 * k, 0.12 * k * l.bw.sqrt(), 8)).collect();
        b.loft(&rings, l.main.0, l.main.1, none);
    }
}

/// What the figure carries, and the umbrella.
fn carry(b: &mut B, k: &PersonKey, l: &Look, p: &Pose, x: &NearExtras, r: &Rigs) {
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let t = &p.torso;
    let strap = [0.05, 0.045, 0.045];
    let hand = |i: usize| p.wrists[i] + (p.wrists[i] - p.elbows[i]).normalize_or(-Vec3::Y) * 0.07;
    let back_pack = |b: &mut B, col: [f32; 3], big: f32| {
        let (hx, _, off) = t.section(0.42);
        let lo = t.at(Vec2::new(off - hx - 0.01, 0.24), 0.0);
        let hi = t.at(Vec2::new(off - hx - 0.01, 0.58), 0.0);
        let out = Vec3::new(-t.lean.cos(), t.lean.sin(), 0.0);
        let (d, w) = (0.07 * big, 0.14 * big * l.bw.sqrt());
        let rings: Vec<Vec<Vec3>> = [(lo, 0.7), (lo.lerp(hi, 0.15), 1.0), (lo.lerp(hi, 0.85), 1.0), (hi, 0.75)].iter().map(|&(c, k)| ring(c + out * d, out, Vec3::Z, d * k, w * k, 8)).collect();
        b.loft(&rings, col, 0, none);
        for s in [-1.0f32, 1.0] {
            let sh = p.shoulders[if s < 0.0 { 0 } else { 1 }] * Vec3::new(1.0, 1.0, 0.7);
            let fr = |h: f32| {
                let (hx, _, off) = t.section(h);
                t.at(Vec2::new(off + hx + 0.004, h), s * 0.1 * l.bw)
            };
            b.limb(&[(hi + Vec3::new(0.0, 0.0, s * 0.08), 0.012), (sh + Vec3::new(0.0, 0.035, 0.0), 0.014), (fr(0.48), 0.012), (fr(0.3), 0.012)], 4, strap, 0, none);
        }
    };
    if k.tourist {
        // As the far figure: a loud daypack with a reflective band, a
        // camera at the chest.
        const PACKS: [[f32; 3]; 5] = [[0.95, 0.35, 0.04], [0.95, 0.75, 0.05], [0.05, 0.6, 0.85], [0.85, 0.08, 0.1], [0.2, 0.75, 0.25]];
        let pack = PACKS[(k.fav[0] as usize + k.skin as usize) % PACKS.len()];
        back_pack(b, pack, 1.3);
        let (hx, _, off) = t.section(0.36);
        let band_c = t.at(Vec2::new(off - hx - 0.19, 0.4), 0.0);
        b.block(band_c - Vec3::new(0.0, 0.0, 0.16), band_c + Vec3::new(0.0, 0.0, 0.16), Vec3::Y, (0.025, 0.012), 4, l.trim.0, l.trim.1, none);
        let cam = t.at(Vec2::new(off + hx + 0.04, 0.34), 0.0);
        b.block(cam - Vec3::new(0.0, 0.0, 0.055), cam + Vec3::new(0.0, 0.0, 0.055), Vec3::Y, (0.04, 0.035), 4, [0.03, 0.03, 0.035], 0, none);
    }
    match x.carry {
        Carry::None => {}
        Carry::Backpack => back_pack(b, if k.child { l.trim.0 } else { mul(l.accent.0, 0.8) }, if k.child { 1.1 } else { 1.0 }),
        Carry::ShoulderBag => {
            let bag = if k.wear == 2 { [0.12, 0.06, 0.03] } else { mul(l.accent.0, 0.75) };
            let (_, hz, off) = t.section(0.1);
            let at = t.at(Vec2::new(off - 0.02, 0.06), hz + 0.05);
            // Sat on the ground: the bag rests beside them, not under it.
            let at = Vec3::new(at.x, at.y.max(0.125), at.z);
            b.block(at - Vec3::new(0.0, 0.11, 0.0), at + Vec3::new(0.0, 0.09, 0.0), Vec3::X, (0.11, 0.035), 6, bag, 0, none);
            let fr = |h: f32, z: f32| {
                let (hx, _, off) = t.section(h);
                t.at(Vec2::new(off + hx + 0.006, h), z)
            };
            let bk = |h: f32, z: f32| {
                let (hx, _, off) = t.section(h);
                t.at(Vec2::new(off - hx - 0.006, h), z)
            };
            let sh = p.shoulders[0] + Vec3::new(0.0, 0.05, 0.03);
            b.limb(&[(at + Vec3::new(0.02, 0.08, -0.02), 0.01), (fr(0.3, 0.06), 0.012), (fr(0.5, -0.07), 0.012), (sh, 0.013), (bk(0.5, -0.06), 0.012), (bk(0.25, 0.08), 0.012), (at + Vec3::new(-0.02, 0.08, -0.02), 0.01)], 4, strap, 0, none);
        }
        Carry::Tote | Carry::Briefcase => {
            let h = hand(0);
            let rig = r.fore[0];
            if x.carry == Carry::Tote {
                let col = if (k.skin + k.fav[0]) % 2 == 0 { [0.78, 0.72, 0.6] } else { mul(l.accent.0, 0.85) };
                b.limb(&[(h + Vec3::new(-0.05, -0.08, 0.0), 0.007), (h, 0.008), (h + Vec3::new(0.05, -0.08, 0.0), 0.007)], 3, col, 0, rig);
                b.block(h + Vec3::new(0.0, -0.4, 0.0), h + Vec3::new(0.0, -0.08, 0.0), Vec3::X, (0.16, 0.04), 4, col, 0, rig);
            } else {
                b.block(h + Vec3::new(0.0, -0.33, 0.0), h + Vec3::new(0.0, -0.04, 0.0), Vec3::X, (0.19, 0.045), 4, [0.09, 0.055, 0.03], 0, rig);
            }
        }
        Carry::Phone => {
            let h = hand(0) + Vec3::new(0.0, 0.01, 0.0);
            let face = Vec3::new(-0.5, 0.8, 0.2).normalize();
            let along = Vec3::new(0.85, 0.5, 0.0).normalize();
            let side = face.cross(along).normalize();
            let (a, w) = (along * 0.07, side * 0.035);
            b.block(h - a, h + a, face, (0.005, 0.036), 4, [0.02, 0.02, 0.025], 0, none);
            let c = h + face * 0.0065;
            // The lit screen.
            b.quad([c - a * 0.92 - w * 0.88, c + a * 0.92 - w * 0.88, c + a * 0.92 + w * 0.88, c - a * 0.92 + w * 0.88], face, [0.6, 0.8, 0.95], bright_cell(2), none);
        }
    }
    if x.umbrella {
        let h = hand(1);
        let top = Vec3::new(0.06, 2.1, 0.03);
        let shaft = (top - h).normalize();
        b.limb(&[(h - shaft * 0.12, 0.012), (top + shaft * 0.03, 0.008)], 4, l.trim.0, l.trim.1, none);
        // The canopy: eight panels in the main colour, seen from below too,
        // the rim in the trim.
        let n = 8;
        let rim_r = 0.5;
        let apex = top + Vec3::new(0.0, 0.02, 0.0);
        let rim: Vec<Vec3> = (0..n).map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            top + Vec3::new(a.cos() * rim_r, -0.17, a.sin() * rim_r)
        }).collect();
        for i in 0..n {
            let (a, c) = (rim[i], rim[(i + 1) % n]);
            let mid = (a + c) * 0.5 + Vec3::new(0.0, 0.03, 0.0);
            let out = (mid - top).normalize() + Vec3::Y;
            b.tri(apex, a, c, out, l.main.0, l.main.1, none);
            b.tri(apex, a, c, -out, mul(l.main.0, 0.7), l.main.1, none);
            let drop = Vec3::new(0.0, -0.025, 0.0);
            b.quad2([a, c, c + drop, a + drop], (mid - top) * Vec3::new(1.0, 0.0, 1.0), l.trim.0, l.trim.1, none);
        }
    }
}

// ── the bike ───────────────────────────────────────────────────────────────

/// Segments round a near bike wheel.
const NEAR_WHEEL_SIDES: usize = 16;

fn wheel(b: &mut B, c: Vec3, rubber: [f32; 3], rim: [f32; 3]) {
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let v0 = b.mark();
    let (outer, mid, inner, hw) = (0.34, 0.305, 0.285, 0.022);
    let at = |i: usize, r: f32, z: f32| {
        let t = i as f32 / NEAR_WHEEL_SIDES as f32 * std::f32::consts::TAU;
        Vec3::new(c.x + t.cos() * r, c.y + t.sin() * r, c.z + z)
    };
    let (cr, cm) = (rgba(rubber), rgba(rim));
    for i in 0..NEAR_WHEEL_SIDES {
        let j = (i + 1) % NEAR_WHEEL_SIDES;
        let t = (i as f32 + 0.5) / NEAR_WHEEL_SIDES as f32 * std::f32::consts::TAU;
        let radial = Vec3::new(t.cos(), t.sin(), 0.0);
        for s in [-1.0f32, 1.0] {
            let z = s * hw;
            b.m.quad_facing([at(i, mid, z), at(j, mid, z), at(j, outer, z * 0.8), at(i, outer, z * 0.8)], Vec3::Z * s + radial * 0.3, cr);
            b.m.quad_facing([at(i, inner, z * 0.5), at(j, inner, z * 0.5), at(j, mid, z), at(i, mid, z)], Vec3::Z * s, cm);
        }
        b.m.quad_facing([at(i, outer, -hw * 0.8), at(j, outer, -hw * 0.8), at(j, outer, hw * 0.8), at(i, outer, hw * 0.8)], radial, cr);
    }
    b.done(v0, 0, none);
    // Spokes, crossed, and the hub.
    for k in 0..6 {
        let t = k as f32 / 6.0 * std::f32::consts::PI;
        let d = Vec3::new(t.cos(), t.sin(), 0.0) * inner;
        let w = Vec3::new(-t.sin(), t.cos(), 0.0) * 0.004;
        b.quad2([c - d - w, c + d - w, c + d + w, c - d + w], Vec3::Z, rim, 0, none);
    }
    b.block(c - Vec3::new(0.0, 0.0, 0.035), c + Vec3::new(0.0, 0.0, 0.035), Vec3::Y, (0.022, 0.022), 6, rim, 0, none);
}

fn bike(b: &mut B, k: &PersonKey, l: &Look, seed_bits: u32) {
    let none = rig_aux(limb::RIGID, Vec2::ZERO, Vec2::ZERO);
    let dark = [0.03, 0.03, 0.035];
    let steel = [0.42, 0.43, 0.46];
    let frame_c = l.accent;
    for x in [-0.53f32, 0.53] {
        wheel(b, Vec3::new(x, 0.34, 0.0), dark, steel);
    }
    let rear = Vec3::new(-0.53, 0.34, 0.0);
    let front = Vec3::new(0.53, 0.34, 0.0);
    let bb = Vec3::new(RIDE_BB.x, RIDE_BB.y, 0.0);
    let seat = Vec3::new(-0.16, 0.86, 0.0);
    let head_top = Vec3::new(0.38, 0.96, 0.0);
    let head_low = Vec3::new(0.42, 0.8, 0.0);
    let tube = |b: &mut B, a: Vec3, c: Vec3, r: f32, col: ([f32; 3], usize)| b.limb(&[(a, r), (c, r)], 6, col.0, col.1, none);
    // The diamond and the stays (both sides of the rear wheel).
    tube(b, bb, seat, 0.02, frame_c);
    tube(b, seat - Vec3::new(0.01, 0.03, 0.0), head_top, 0.018, frame_c);
    tube(b, bb, head_low, 0.022, frame_c);
    tube(b, head_top, head_low, 0.024, frame_c);
    for s in [-1.0f32, 1.0] {
        let z = Vec3::Z * (s * 0.04);
        tube(b, bb + z * 0.5, rear + z, 0.011, frame_c);
        tube(b, seat - Vec3::new(0.0, 0.04, 0.0) + z * 0.3, rear + z, 0.01, frame_c);
        // The fork's legs.
        tube(b, head_low + z * 0.4, front + z, 0.012, (steel, 0));
    }
    // Seat post and saddle, stem and bars with grips.
    tube(b, seat, seat + Vec3::new(-0.02, 0.07, 0.0), 0.014, (steel, 0));
    b.block(Vec3::new(-0.28, 0.94, 0.0), Vec3::new(-0.06, 0.95, 0.0), Vec3::Y, (0.025, 0.06), 6, dark, 0, none);
    tube(b, head_top, Vec3::new(0.36, 1.12, 0.0), 0.016, (steel, 0));
    b.limb(&[(Vec3::new(0.3, 1.13, -0.26), 0.013), (Vec3::new(0.36, 1.12, -0.12), 0.012), (Vec3::new(0.36, 1.12, 0.12), 0.012), (Vec3::new(0.3, 1.13, 0.26), 0.013)], 5, dark, 0, none);
    // The chainring and the cranks with their pedals, turning together.
    let crank = rig_aux(limb::CRANK, RIDE_BB, Vec2::ZERO);
    b.block(bb + Vec3::new(0.0, 0.0, 0.03), bb + Vec3::new(0.0, 0.0, 0.045), Vec3::Y, (0.09, 0.09), 10, steel, 0, crank);
    for (i, z) in [(0usize, -1.0f32), (1, 1.0)] {
        let a = RIDE_REST + if i == 1 { std::f32::consts::PI } else { 0.0 };
        let p = bb + Vec3::new(a.cos(), a.sin(), 0.0) * RIDE_CRANK + Vec3::Z * (z * 0.06);
        b.limb(&[(bb + Vec3::Z * (z * 0.05), 0.012), (p, 0.012)], 4, steel, 0, crank);
        b.block(p + Vec3::new(-0.05, 0.0, z * 0.0), p + Vec3::new(0.05, 0.0, 0.0), Vec3::Y, (0.01, 0.045), 4, dark, 0, crank);
    }
    // Lights: white at the front, red at the back; a basket on some.
    let lamp = Vec3::new(0.46, 0.98, 0.0);
    b.block(lamp, lamp + Vec3::new(0.05, 0.0, 0.0), Vec3::Y, (0.022, 0.022), 6, [0.9, 0.9, 0.85], bright_cell(10), none);
    let tail = Vec3::new(-0.3, 0.88, 0.0);
    b.block(tail - Vec3::new(0.03, 0.0, 0.0), tail, Vec3::Y, (0.018, 0.02), 4, [0.8, 0.05, 0.05], bright_cell(18), none);
    if seed_bits % 3 == 0 && !k.child {
        let c = Vec3::new(0.6, 0.95, 0.0);
        let rings: Vec<Vec<Vec3>> = [(0.0, 0.11), (0.16, 0.13)].iter().map(|&(dy, r)| ring(c + Vec3::new(0.0, dy, 0.0), Vec3::X, Vec3::Z, r, r * 1.2, 4)).collect();
        b.loft(&rings, [0.45, 0.32, 0.15], 0, none);
    }
}

// ── the mesh ───────────────────────────────────────────────────────────────

/// The near figure for `key`, standing (walking), sitting or riding at the
/// origin facing +X: an average adult about 1.8 m tall, scaled by stature
/// as the far figure. Its second UV set is the rig (see the module doc).
pub fn near_person_mesh(key: &NearKey) -> MeshData {
    let k = &key.base;
    let x = &key.extras;
    let mut m = MeshData::default();
    let mut b = B { m: &mut m };
    let l = look_of(k);
    let pose = if k.ride {
        riding_pose(&l)
    } else if k.sit != 0 {
        seated_pose(&l, k.sit == SIT_BENCH)
    } else {
        standing(&l, x)
    };
    let rigs = rigs_of(&pose, x);
    body(&mut b, k, x, &l, &pose, &rigs);
    if k.ride {
        bike(&mut b, k, &l, k.fav[0] as u32 * 7 + k.skin as u32 + k.style as u32);
    }
    let n = m.positions.len();
    m.set_aux(n, [0.0, 0.0]);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::people::SIT_GROUND;

    fn keys() -> Vec<NearKey> {
        let mut out = Vec::new();
        for style in 0..HairStyle::ALL.len() as u8 {
            for outfit in 0..Outfit::ALL.len() as u8 {
                for build in 0..3u8 {
                    for (child, ride, tourist, sit) in [(false, false, false, 0), (true, false, false, 0), (false, true, false, 0), (false, false, true, 0), (false, false, false, SIT_BENCH), (false, false, false, SIT_GROUND), (true, true, false, 0)] {
                        let base = PersonKey { style, outfit, build, child, skin: (style + outfit * 3) % SKIN_STEPS, hair: if style % 2 == 0 { 2 } else { 8 + style }, fav: [outfit * 3 % 22, (style * 5 + 1) % 22], wear: build, ride, tourist, sit };
                        for (i, carry) in [Carry::None, Carry::ShoulderBag, Carry::Backpack, Carry::Tote, Carry::Briefcase, Carry::Phone].into_iter().enumerate() {
                            let umbrella = i % 2 == 1 && sit == 0 && !ride && carry != Carry::Phone;
                            let hood_up = !umbrella && i == 2 && sit == 0 && !ride;
                            out.push(NearKey { base, extras: NearExtras { carry, umbrella, hood_up, old: (style + build) % 3 == 0 } });
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn near_figures_are_finite_rigged_and_within_budget() {
        let mut most = (0, 0);
        for key in keys() {
            let m = near_person_mesh(&key);
            let tris = m.indices.len() / 3;
            let budget = if key.base.ride { MAX_NEAR_RIDER_TRIS } else { MAX_NEAR_TRIS };
            assert!(tris > 300 && tris <= budget, "{key:?}: {tris} triangles");
            if key.base.ride { most.1 = most.1.max(tris) } else { most.0 = most.0.max(tris) }
            assert_eq!(m.uvs.len(), m.positions.len());
            assert_eq!(m.colors.len(), m.positions.len());
            assert_eq!(m.aux.len(), m.positions.len(), "every vertex has its rig");
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
            for p in &m.positions {
                assert!(p.iter().all(|v| v.is_finite()));
                assert!(p[1] >= -0.02 && p[1] < 2.3, "{key:?}: {p:?}");
            }
            assert!(m.normals.iter().flatten().all(|v| v.is_finite()));
            assert!(m.colors.iter().flatten().all(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0));
            for uv in &m.uvs {
                assert!(uv[0] > 0.0 && uv[0] < 1.0);
            }
        }
        println!("most triangles: on foot {}, riding {}", most.0, most.1);
        let plain = |ride: bool, outfit: u8| NearKey { base: PersonKey { style: 0, outfit, build: 1, child: false, skin: 3, hair: 2, fav: [3, 9], wear: 1, ride, tourist: false, sit: 0 }, extras: NearExtras { carry: Carry::None, umbrella: false, hood_up: false, old: false } };
        for o in 0..6 {
            println!("outfit {o}: walking {} riding {}", near_person_mesh(&plain(false, o)).indices.len() / 3, near_person_mesh(&plain(true, o)).indices.len() / 3);
        }
    }

    /// The near figure stands as tall as the far one (the swap does not
    /// jump), and its colours are the far figure's: the same skin, the same
    /// trim cell lit.
    #[test]
    fn the_near_figure_matches_the_far_one() {
        for key in keys().into_iter().filter(|k| !k.base.ride && k.base.sit == 0 && !k.extras.umbrella && !k.extras.old && k.extras.carry == Carry::None) {
            let near = near_person_mesh(&key);
            let far = crate::people::person_mesh(&key.base);
            let top = |m: &MeshData| m.positions.iter().map(|p| p[1]).fold(0.0, f32::max);
            let (tn, tf) = (top(&near), top(&far));
            assert!((tn - tf).abs() < 0.16, "{key:?}: near {tn} far {tf}");
            let cells = |m: &MeshData| m.uvs.iter().map(|uv| (uv[0] * crate::people::EMISSIVE_CELLS as f32) as usize).collect::<std::collections::BTreeSet<_>>();
            let (cn, cf) = (cells(&near), cells(&far));
            assert!(cf.is_subset(&cn), "{key:?}: far cells {cf:?} near {cn:?}");
        }
    }

    #[test]
    fn the_rig_packs_its_joints() {
        for p in [Vec2::new(0.0, 0.88), Vec2::new(-0.17, 0.97), Vec2::new(0.42, 1.44), Vec2::new(-1.0, 0.0)] {
            let a = rig_aux(limb::SHIN_L, p, p);
            let a0 = a[0] as u32;
            assert_eq!(a0 & 31, limb::SHIN_L as u32);
            assert!((unpack(a0 >> 5) - p).length() < 0.006);
            assert!((unpack(a[1] as u32) - p).length() < 0.006);
        }
    }

    /// Walking, the feet stay about the ground: neither through it nor
    /// lifted high, the swing as long as a stride; stood still nothing moves.
    #[test]
    fn the_walk_keeps_the_feet_on_the_ground() {
        let key = NearKey { base: PersonKey { style: 0, outfit: 0, build: 1, child: false, skin: 3, hair: 0, fav: [0, 9], wear: 1, ride: false, tourist: false, sit: 0 }, extras: NearExtras { carry: Carry::None, umbrella: false, hood_up: false, old: false } };
        let m = near_person_mesh(&key);
        let still = walk_tag(0.3, 0.0);
        for (i, p) in m.positions.iter().enumerate() {
            let (q, _) = pose_vertex(Vec3::from(*p), Vec3::from(m.normals[i]), m.aux[i], still);
            assert!((q - Vec3::from(*p)).length() < 1e-4, "still is the rest pose");
        }
        let mut reach: f32 = 0.0;
        for step in 0..16 {
            let tag = walk_tag(step as f32 / 16.0, 1.0);
            let mut low = f32::MAX;
            for (i, p) in m.positions.iter().enumerate() {
                let (q, n) = pose_vertex(Vec3::from(*p), Vec3::from(m.normals[i]), m.aux[i], tag);
                assert!(q.is_finite() && n.is_finite());
                assert!((n.length() - Vec3::from(m.normals[i]).length()).abs() < 1e-3, "normals keep their length");
                low = low.min(q.y);
                if q.y < 0.15 {
                    reach = reach.max(q.x.abs());
                }
            }
            assert!(low > -0.05 && low < 0.05, "step {step}: the lowest point {low}");
        }
        assert!(reach > 0.3, "the feet swing out: {reach}");
    }

    /// Riding, the feet go round with the pedals at every turn of them.
    #[test]
    fn a_riders_feet_follow_the_pedals() {
        for step in 0..12 {
            let phi = step as f32 / 12.0 * std::f32::consts::TAU;
            for off in [0.0, std::f32::consts::PI] {
                let (t, s) = ride_leg(phi + off);
                let knee = RIDE_HIP + Vec2::new(t.cos(), t.sin()) * RIDE_THIGH;
                let ankle = knee + Vec2::new(s.cos(), s.sin()) * RIDE_SHIN;
                let want = RIDE_BB + Vec2::new((phi + off).cos(), (phi + off).sin()) * RIDE_CRANK + RIDE_ANKLE;
                assert!((ankle - want).length() < 0.01, "phi {phi}: {ankle} vs {want}");
                assert!(knee.x > RIDE_HIP.x, "the knee is forward");
            }
        }
        // The shin's rig moves the mesh's ankle with the pedal.
        let base = PersonKey { style: 0, outfit: 1, build: 1, child: false, skin: 3, hair: 0, fav: [0, 9], wear: 1, ride: true, tourist: false, sit: 0 };
        let key = NearKey { base, extras: NearExtras { carry: Carry::None, umbrella: false, hood_up: false, old: false } };
        let m = near_person_mesh(&key);
        let phase = 0.25;
        let tag = walk_tag(phase, 1.0);
        let pedal = RIDE_BB + Vec2::new((phase * std::f32::consts::TAU).cos(), (phase * std::f32::consts::TAU).sin()) * RIDE_CRANK;
        let shin_l: Vec<Vec3> = m.positions.iter().zip(&m.aux).filter(|(_, a)| (a[0].round() as u32 & 31) as u8 == limb::RIDE_SHIN_L).map(|(p, a)| pose_vertex(Vec3::from(*p), Vec3::Y, *a, tag).0).collect();
        assert!(!shin_l.is_empty());
        let lowest = shin_l.iter().fold(Vec3::splat(f32::MAX), |a, p| if p.y < a.y { *p } else { a });
        assert!((Vec2::new(lowest.x, lowest.y) - pedal).length() < 0.12, "the shoe at the pedal: {lowest} vs {pedal}");
    }

    #[test]
    fn extras_come_from_the_seed_alone() {
        let k = PersonKey { style: 0, outfit: 1, build: 1, child: false, skin: 3, hair: 0, fav: [0, 9], wear: 1, ride: false, tourist: false, sit: 0 };
        let mut umbrellas = 0;
        let mut carried = 0;
        for seed in 0..1000u64 {
            let a = near_extras(&k, seed, 30, true);
            assert_eq!(a, near_extras(&k, seed, 30, true));
            assert!(!(a.umbrella && a.hood_up));
            umbrellas += a.umbrella as u32;
            carried += (a.carry != Carry::None) as u32;
            let dry = near_extras(&k, seed, 30, false);
            assert!(!dry.umbrella && !dry.hood_up);
        }
        assert!(umbrellas > 100 && carried > 200, "{umbrellas} umbrellas, {carried} carrying");
    }
}
