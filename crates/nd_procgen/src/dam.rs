//! [plant-groups] The hydro plant's dam: a concrete arch bowed upstream
//! across the river beside the plant, stepped like a gravity-arch dam, with
//! a spillway chute down its middle, intake towers on the plant's side
//! feeding a penstock to the turbine hall, and the neon art direction: a
//! cyan light line along the crest parapets, lit lamp posts, magenta lights
//! on the intake towers and glowing edges down the spillway.

use crate::buildings::parts::{mul, rgba};
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::landscape::{BANK_HEIGHT, WATER_LEVEL};

/// Crest height above the water, metres.
pub const DAM_CREST: f32 = 7.0;
const CYAN: [f32; 3] = [0.25, 1.5, 2.0];
const MAGENTA: [f32; 3] = [1.9, 0.25, 1.4];
const FOAM: [f32; 3] = [0.62, 0.7, 0.74];
const WHITEWATER: [f32; 3] = [0.4, 0.52, 0.58];
/// About the colour the river is drawn in, for foam to fade into.
const RIVER: [f32; 3] = [0.26, 0.37, 0.43];

/// Points along the arch, `n + 1` of them from one abutment to the other:
/// the centreline of the crest, bowed upstream by `bow` at the middle.
pub fn arch(c: Vec2, across: Vec2, upstream: Vec2, half: f32, bow: f32, n: usize) -> Vec<Vec2> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32 * 2.0 - 1.0;
            c + across * (t * half) + upstream * (bow * (1.0 - t * t))
        })
        .collect()
}

/// Draw the dam for a hydro plant on `lot_center`, across river centre
/// point `c` of a river `width` wide flowing along `flow` (unit).
pub fn hydro_dam(g: &mut Geometry, c: Vec2, flow: Vec2, width: f32, lot_center: Vec2, ground: &dyn Fn(Vec2) -> f32, concrete: [f32; 3], steel: [f32; 3]) {
    let across = Vec2::new(-flow.y, flow.x);
    let upstream = -flow;
    // Into the banks on both sides, so the abutments sit in the ground.
    let half = width * 0.5 + 6.0;
    let bow = (half * 0.32).clamp(3.0, 22.0);
    let n = ((half * 2.0) / 2.5).ceil().clamp(10.0, 48.0) as usize;
    let crest = arch(c, across, upstream, half, bow, n);
    let top = WATER_LEVEL + DAM_CREST;
    let face = mul(concrete, 1.05);
    let darker = mul(concrete, 0.8);
    // The body: three tiers stepping in as it rises, so the downstream face
    // reads as a battered curve.
    for (w, y0, y1, col) in [(7.5, WATER_LEVEL - 3.0, WATER_LEVEL + 2.2, darker), (5.4, WATER_LEVEL + 2.2, top - 2.2, concrete), (3.6, top - 2.2, top, face)] {
        g.part(Surface::Concrete).path_box(&crest, 0.0, w, y0, y1, rgba(col));
    }
    // A thin line of light along the ledge where the lower tier steps in.
    for side in [-1.0f32, 1.0] {
        g.part(Surface::Glow).path_box(&crest, side * 2.72, 0.12, WATER_LEVEL + 2.2, WATER_LEVEL + 2.32, rgba(mul(CYAN, 0.8)));
    }
    // The crest road's parapets, and the neon line along each.
    for side in [-1.0f32, 1.0] {
        g.part(Surface::Concrete).path_box(&crest, side * 1.6, 0.35, top, top + 1.0, rgba(face));
        g.part(Surface::Glow).path_box(&crest, side * 1.6, 0.4, top + 1.0, top + 1.12, rgba(CYAN));
    }
    // Lamp posts along the crest.
    let mut i = 2;
    while i + 2 <= n {
        let p = crest[i];
        let d = (crest[i + 1] - crest[i - 1]).normalize_or_zero();
        let out = Vec2::new(-d.y, d.x) * 1.4;
        for s in [-1.0f32, 1.0] {
            let q = p + out * s;
            g.part(Surface::Metal).cylinder(q, 0.08, 0.08, top + 1.0, top + 4.2, 6, rgba(steel));
            g.part(Surface::Glow).boxed(Vec3::new(q.x, top + 4.3, q.y), Vec3::new(0.32, 0.18, 0.32), 0.0, rgba([2.4, 2.0, 1.4]));
        }
        i += 4;
    }
    // Abutment blocks where the arch meets the banks.
    for &p in [crest[0], crest[n]].iter() {
        let y = ground(p).max(BANK_HEIGHT);
        g.part(Surface::Concrete).cylinder(p, 4.2, 3.6, y - 2.0, top + 1.4, 8, rgba(darker));
        // A band of light round the head, its caps hidden in the block.
        g.part(Surface::Glow).cylinder(p, 3.66, 3.66, top + 0.6, top + 0.8, 8, rgba(CYAN));
    }
    // The spillway: the middle third of the arch, a chute down the
    // downstream face into a stilling basin of white water.
    let mid = n / 2;
    let span = (n / 6).max(2);
    let (sa, sb) = (crest[mid - span], crest[mid + span]);
    let sw = sa.distance(sb);
    let sm = crest[mid];
    let fwd = flow;
    let lip = sm + fwd * 2.0;
    let foot = sm + fwd * (DAM_CREST * 1.6 + 4.0);
    let chute_w = sw.max(6.0);
    // The chute's floor, and the water racing down it in streaks.
    g.part(Surface::Concrete).slab(Vec3::new(lip.x, top - 0.6, lip.y), Vec3::new(foot.x, WATER_LEVEL + 0.2, foot.y), chute_w, 0.8, rgba(darker));
    let lanes = ((chute_w - 1.2) / 1.1).floor().max(3.0) as i32;
    let lane_w = (chute_w - 1.2) / lanes as f32;
    let side0 = Vec2::new(-fwd.y, fwd.x);
    for k in 0..lanes {
        let o = side0 * ((k as f32 + 0.5) * lane_w - (chute_w - 1.2) * 0.5);
        let col = if k % 3 == 1 { FOAM } else if k % 2 == 0 { WHITEWATER } else { mul(WHITEWATER, 0.9) };
        let (a, b) = (lip + o, foot + o);
        g.part(Surface::Matte).slab(Vec3::new(a.x, top - 0.45, a.y), Vec3::new(b.x, WATER_LEVEL + 0.35, b.y), lane_w * 0.96, 0.1, rgba(col));
    }
    // Training walls either side of the chute, edged in light.
    let side = Vec2::new(-fwd.y, fwd.x);
    for s in [-1.0f32, 1.0] {
        let o = side * (chute_w * 0.5 + 0.3) * s;
        let (a, b) = (lip + o, foot + o);
        g.part(Surface::Concrete).slab(Vec3::new(a.x, top + 0.6, a.y), Vec3::new(b.x, WATER_LEVEL + 1.4, b.y), 0.6, 1.8, rgba(face));
        g.part(Surface::Glow).slab(Vec3::new(a.x, top + 0.72, a.y), Vec3::new(b.x, WATER_LEVEL + 1.52, b.y), 0.7, 0.12, rgba(CYAN));
    }
    // Radial gates over the spillway crest: steel arcs between piers.
    let gates = 3;
    for k in 0..=gates {
        let t = k as f32 / gates as f32;
        let p = sa.lerp(sb, t);
        let q = Vec2::new(p.x, p.y) + fwd * 0.6;
        g.part(Surface::Concrete).boxed(Vec3::new(q.x, top + 1.8, q.y), Vec3::new(1.2, 3.6, 4.0), fwd.y.atan2(fwd.x), rgba(face));
        if k < gates {
            let m = sa.lerp(sb, (k as f32 + 0.5) / gates as f32) - fwd * 0.8;
            let d = (sb - sa).normalize_or_zero();
            let len = sw / gates as f32 - 1.4;
            g.part(Surface::Metal).pipe(m, d, len, 0.9, top + 1.4, 8, rgba(mul(steel, 0.85)));
            g.part(Surface::Glow).pipe(m, d, len, 0.12, top + 2.45, 4, rgba(MAGENTA));
        }
    }
    // Foam boiling up in the stilling basin and breaking up downstream:
    // patches that thin out and drift apart with distance.
    for k in 0..26 {
        let h = (k as f32 * 2.399).sin() * 43_758.547;
        let u = h - h.floor();
        let v = (h * 7.13) - (h * 7.13).floor();
        let along = 2.0 + (k as f32 / 26.0).powf(1.4) * 34.0;
        let spread = chute_w * 0.5 + along * 0.35;
        let p = foot + fwd * along + side * ((u * 2.0 - 1.0) * spread);
        let r = (3.4 - along * 0.07).max(0.9) * (0.7 + v * 0.5);
        // White at the foot, fading into the river's colour downstream.
        let f = (along / 36.0).clamp(0.0, 1.0);
        let base = mul(FOAM, 0.85);
        let col = [base[0] + (RIVER[0] - base[0]) * f, base[1] + (RIVER[1] - base[1]) * f, base[2] + (RIVER[2] - base[2]) * f];
        let ring: Vec<Vec2> = (0..10)
            .map(|i| {
                let a = i as f32 / 10.0 * std::f32::consts::TAU + u * 3.0;
                p + (fwd * (a.cos() * 1.5) + side * a.sin()) * r * (0.9 + 0.1 * (a * 2.0 + v * 5.0).sin())
            })
            .collect();
        g.part(Surface::Matte).ground_polygon(&ring, WATER_LEVEL + 0.06 + k as f32 * 0.002, rgba(col));
    }
    // Intake towers on the plant's side, in the reservoir upstream of the
    // arch, with a penstock from each under the bank to the turbine hall.
    let plant_side = if (lot_center - c).dot(across) >= 0.0 { 1.0 } else { -1.0 };
    for (j, t) in [0.42f32, 0.66].into_iter().enumerate() {
        let k = ((0.5 + plant_side * t * 0.5) * n as f32).round() as usize;
        let k = k.min(n);
        let base = crest[k] + upstream * 6.5;
        let r = 2.4 - j as f32 * 0.3;
        let h = top + 3.5 + j as f32 * 1.5;
        g.part(Surface::Concrete).cylinder(base, r + 0.3, r, WATER_LEVEL - 3.0, h, 12, rgba(face));
        g.part(Surface::Metal).cylinder(base, r * 0.75, r * 0.6, h, h + 2.0, 10, rgba(mul(steel, 0.9)));
        g.part(Surface::Glow).cylinder(base, r + 0.05, r + 0.05, h - 0.9, h - 0.7, 12, rgba(MAGENTA));
        g.part(Surface::Glow).boxed(Vec3::new(base.x, h + 2.3, base.y), Vec3::new(0.4, 0.4, 0.4), 0.0, rgba([2.6, 0.3, 0.3]));
        // A footbridge from the tower to the crest.
        g.part(Surface::Metal).slab(Vec3::new(base.x, top + 0.4, base.y), Vec3::new(crest[k].x, top + 0.4, crest[k].y), 1.2, 0.3, rgba(steel));
        // The penstock: down the bank and across to the hall.
        let bank = c + across * plant_side * (width * 0.5 + 2.0) + upstream * (2.0 + j as f32 * 3.0);
        let to = bank + (lot_center - bank).normalize_or_zero() * 10.0;
        let yb = ground(bank).max(BANK_HEIGHT) + 1.2;
        let yt = ground(to).max(BANK_HEIGHT) + 1.2;
        g.part(Surface::Metal).tube(Vec3::new(base.x, WATER_LEVEL + 1.0, base.y), Vec3::new(bank.x, yb, bank.y), 1.0, 10, rgba(mul(steel, 0.75)));
        g.part(Surface::Metal).tube(Vec3::new(bank.x, yb, bank.y), Vec3::new(to.x, yt, to.y), 1.0, 10, rgba(mul(steel, 0.75)));
        g.part(Surface::Glow).tube(Vec3::new(bank.x, yb + 1.02, bank.y), Vec3::new(to.x, yt + 1.02, to.y), 0.06, 4, rgba(CYAN));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arch_bows_upstream_and_ends_on_the_banks() {
        let c = Vec2::new(10.0, 5.0);
        let pts = arch(c, Vec2::X, -Vec2::Y, 30.0, 8.0, 20);
        assert_eq!(pts.len(), 21);
        assert!((pts[0] - Vec2::new(-20.0, 5.0)).length() < 1e-4);
        assert!((pts[20] - Vec2::new(40.0, 5.0)).length() < 1e-4);
        assert!((pts[10] - Vec2::new(10.0, -3.0)).length() < 1e-4, "the middle is furthest upstream");
        let mut g = Geometry::default();
        hydro_dam(&mut g, c, Vec2::Y, 40.0, Vec2::new(60.0, 0.0), &|_| BANK_HEIGHT, [0.5, 0.5, 0.5], [0.4, 0.4, 0.45]);
        assert!(g.iter().any(|(s, m)| s == Surface::Glow && !m.positions.is_empty()), "neon on the dam");
        assert!(g.iter().any(|(s, m)| s == Surface::Concrete && m.positions.len() > 100), "a concrete arch");
    }
}
