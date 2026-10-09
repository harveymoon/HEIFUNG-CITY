//! Toll booths across a road: islands between the lanes with a kiosk on
//! each, a canopy on columns over the whole carriageway with neon edges,
//! lane lights and raised barrier arms, and rumble stripes before the line.
//! The plaza is the big version: the asphalt flares out either side to six
//! booth lanes splayed across, with fanned lane lines leading in and out.
//!
//! Drawn on the flat datum (the road surface at y = 0) in world
//! coordinates; the app lifts every vertex onto the road's height.

use crate::color::{mul, rgba, DARK};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};

/// What the renderer needs to draw one booth.
#[derive(Clone, Copy, Debug)]
pub struct TollDraw {
    /// On the road centre line.
    pub pos: Vec2,
    /// Along the road (unit).
    pub dir: Vec2,
    /// Lanes on the road under it (both directions).
    pub lanes: u8,
    /// Half the road's paved width there.
    pub half_width: f32,
    /// The big six-booth plaza.
    pub plaza: bool,
    /// For the neon colour.
    pub id: u32,
    /// Free width from the centre line to the left (-) and right (+)
    /// before another road (a highway's other carriageway): a plaza is
    /// shifted and squeezed to stay inside it.
    pub room: [f32; 2],
}

/// Width of one booth lane on a plaza.
pub const PLAZA_LANE: f32 = 4.0;
/// Booth lanes on a plaza.
pub const PLAZA_LANES: usize = 6;
/// The plaza's flare reaches this far along the road either side.
pub const PLAZA_FLARE: f32 = 22.0;
/// Booth lanes run straight this far either side of the line.
const PLAZA_STRAIGHT: f32 = 6.0;
const LANE: f32 = 3.5;

pub fn toll_booths(booths: &[TollDraw], pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    for b in booths {
        toll(&mut g, b, pal);
    }
    g
}

/// A booth line laid across its road: the frame its pieces are placed in
/// and its lanes.
struct TollFrame<'a> {
    b: &'a TollDraw,
    d: Vec2,
    right: Vec2,
    yaw: f32,
    /// Lanes through the booth line, and their width.
    n: usize,
    lane_w: f32,
    /// Half the booth lanes' width.
    half: f32,
    /// The booth line's centre across the road, from its centre line.
    c: f32,
    neon: [f32; 3],
    neon2: [f32; 3],
}

impl TollFrame<'_> {
    /// `u` along the road, `v` across it from the booth line's centre.
    fn at(&self, u: f32, v: f32) -> Vec2 {
        self.b.pos + self.d * u + self.right * (v + self.c)
    }

    fn p3(&self, u: f32, v: f32, y: f32) -> Vec3 {
        let p = self.at(u, v);
        Vec3::new(p.x, y, p.y)
    }
}

fn toll(g: &mut Geometry, b: &TollDraw, pal: &Palette) {
    let d = b.dir.normalize_or_zero();
    if d == Vec2::ZERO {
        return;
    }
    let neon = if pal.glow_colors.is_empty() { [1.0, 0.2, 0.8] } else { pal.glow(b.id as usize) };
    let neon2 = if pal.glow_colors.len() > 1 { pal.glow(b.id as usize + 3) } else { [0.2, 0.9, 1.0] };
    // Lanes through the booth line: the road's own, or the plaza's six.
    // A plaza squeezed between other roads narrows its lanes (to 3 m) and
    // shifts towards the side with room; `c` is the booth line's centre
    // across the road.
    let n = if b.plaza { PLAZA_LANES } else { b.lanes.max(1) as usize };
    let (lane_w, c) = if b.plaza {
        let (rl, rr) = (b.room[0].max(b.half_width), b.room[1].max(b.half_width));
        let lane_w = ((rl + rr - 3.0) / n as f32).clamp(3.0, PLAZA_LANE);
        let w = n as f32 * lane_w * 0.5 + 1.5;
        // Centre so that [c - w, c + w] fits in [-rl, rr] where it can.
        let c = if rl + rr >= 2.0 * w { (0.0f32).clamp(w - rl, rr - w) } else { (rr - rl) * 0.5 };
        (lane_w, c)
    } else {
        (LANE, 0.0)
    };
    let t = TollFrame { b, d, right: Vec2::new(-d.y, d.x), yaw: d.y.atan2(d.x), n, lane_w, half: n as f32 * lane_w * 0.5, c, neon, neon2 };
    if b.plaza {
        plaza_apron(g, &t, pal);
    }
    // Rumble stripes before the line, both ways.
    let stripe = rgba(mul(pal.lane_marker, 0.8));
    for sgn in [-1.0f32, 1.0] {
        for k in 0..4 {
            let u = sgn * (11.0 + k as f32 * 1.2);
            // Across the road's own width, where they are driven over.
            let v_half = if b.plaza { b.half_width - 1.0 } else { t.half };
            line(g, t.at(u, -v_half - c), t.at(u, v_half - c), 0.4, 0.09, stripe);
        }
    }
    islands(g, &t, pal);
    barriers(g, &t, pal);
    canopy(g, &t, pal);
}

/// A plaza's apron: the asphalt flares from the road's width to the
/// plaza's, runs straight through the booths and narrows again, kerbed
/// and lit, with lane lines fanning in to the booth lanes.
fn plaza_apron(g: &mut Geometry, t: &TollFrame, pal: &Palette) {
    let (b, d, n, lane_w, half, c) = (t.b, t.d, t.n, t.lane_w, t.half, t.c);
    let w = half + 1.5;
    let hw = b.half_width;
    let f = PLAZA_FLARE;
    let s = PLAZA_STRAIGHT;
    // The road's edges in the booth line's frame.
    let (el, er) = (-hw - c, hw - c);
    let apron = [t.at(-f, el), t.at(-s, -w), t.at(s, -w), t.at(f, el), t.at(f, er), t.at(s, w), t.at(-s, w), t.at(-f, er)];
    // Over the road's own lane marks (0.085), which it replaces, rather
    // than under them; its lines a layer above that.
    g.part(Asphalt).ground_polygon(&apron, 0.092, rgba(mul(pal.asphalt, 1.12)));
    // Kerbs along the flared edges, a strip of kerb lights on them.
    for (side, edge) in [(-1.0f32, el), (1.0, er)] {
        let pts = [t.at(-f, edge), t.at(-s, side * w), t.at(s, side * w), t.at(f, edge)];
        for k in 0..3 {
            let (a, e) = (pts[k], pts[k + 1]);
            let dd = (e - a).normalize_or_zero();
            let out = Vec2::new(-dd.y, dd.x) * side * (if (e - a).dot(d) > 0.0 { 1.0 } else { -1.0 });
            g.part(Concrete).strip_box(a + out * 0.2, e + out * 0.2, 0.0, 0.4, 0.0, 0.2, rgba(pal.curb));
            g.part(Glow).strip_box(a + out * 0.2, e + out * 0.2, 0.0, 0.12, 0.2, 0.26, rgba(mul(t.neon2, 0.9)));
        }
    }
    // Lane lines fanning in from the road's lanes to the booth lanes.
    let marker = rgba(mul(pal.lane_marker, 0.9));
    for k in 0..=n {
        let v = -half + k as f32 * lane_w;
        let tt = k as f32 / n as f32;
        let v_road = (el + 1.5) + tt * ((er - 1.5) - (el + 1.5));
        for sgn in [-1.0f32, 1.0] {
            let a = t.at(sgn * s, v);
            let e = t.at(sgn * f, v_road);
            line(g, a, e, 0.14, 0.1, marker);
        }
    }
    // Tall lamps at the four corners of the straight part.
    let steel = rgba(mul(pal.steel, 0.75));
    for (u, v) in [(-s, -w), (-s, w), (s, -w), (s, w)] {
        let q = t.at(u, v - v.signum() * 0.3);
        g.part(Metal).boxed(Vec3::new(q.x, 4.5, q.y), Vec3::new(0.25, 9.0, 0.25), t.yaw, steel);
        g.part(Glow).boxed(Vec3::new(q.x, 9.1, q.y), Vec3::new(0.9, 0.25, 0.9), t.yaw, rgba(mul(pal.lamp, 1.4)));
    }
}

/// Islands on every lane line, a kiosk on each, columns on the outer ones
/// and every other one between.
fn islands(g: &mut Geometry, t: &TollFrame, pal: &Palette) {
    let (b, d, right, yaw, n) = (t.b, t.d, t.right, t.yaw, t.n);
    let concrete = rgba(mul(pal.concrete(0), 1.15));
    let hazard = rgba(pal.hazard);
    let steel = rgba(mul(pal.steel, 0.75));
    let kiosk = rgba(mul(pal.panel(1), 1.05));
    let island_len = if b.plaza { 11.0 } else { 9.0 };
    for k in 0..=n {
        let v = -t.half + k as f32 * t.lane_w;
        let c = t.at(0.0, v);
        g.part(Concrete).boxed(Vec3::new(c.x, 0.15, c.y), Vec3::new(island_len, 0.3, 0.9), yaw, concrete);
        for sgn in [-1.0f32, 1.0] {
            let nose = t.at(sgn * (island_len * 0.5 + 0.25), v);
            g.part(Matte).boxed(Vec3::new(nose.x, 0.45, nose.y), Vec3::new(0.5, 0.9, 0.9), yaw, hazard);
            // A bollard light on the nose.
            g.part(Glow).boxed(Vec3::new(nose.x, 1.0, nose.y), Vec3::new(0.18, 0.2, 0.18), yaw, rgba(mul(pal.lamp, 1.2)));
        }
        // The kiosk: walls, a glass band lit from inside, a flat roof.
        let kc = t.at(0.6, v);
        g.part(Panel).boxed_open(Vec3::new(kc.x, 1.55, kc.y), Vec3::new(2.4, 2.5, 0.85), yaw, kiosk);
        for side in [-1.0f32, 1.0] {
            let w = t.at(0.6, v + side * 0.44);
            g.part(Glow).decal(Vec3::new(w.x, 1.95, w.y), Vec3::new(d.x, 0.0, d.y), Vec3::Y, 1.9, 0.8, Vec3::new(right.x * side, 0.0, right.y * side), rgba(mul(pal.window_warm, 1.1)));
        }
        g.part(Metal).boxed(Vec3::new(kc.x, 2.9, kc.y), Vec3::new(2.7, 0.2, 1.1), yaw, steel);
        let column = k == 0 || k == n || (b.plaza && k % 2 == 0) || (!b.plaza && n > 2 && k % 2 == 0);
        if column {
            for u in [-3.0f32, 3.0] {
                let p = t.at(u, v);
                g.part(Metal).boxed(Vec3::new(p.x, 2.9, p.y), Vec3::new(0.35, 5.8, 0.35), yaw, steel);
            }
        }
    }
}

/// Barrier arms, raised, one per lane on its right-hand island, and a
/// green lane light over each lane on the canopy.
fn barriers(g: &mut Geometry, t: &TollFrame, pal: &Palette) {
    let (d, right, lane_w) = (t.d, t.right, t.lane_w);
    for k in 0..t.n {
        let v = -t.half + k as f32 * lane_w;
        let foot = t.at(-2.6, v + 0.2);
        let tip = t.at(-2.6, v + 0.5 + 1.2);
        let a = Vec3::new(foot.x, 1.0, foot.y);
        let c = Vec3::new(tip.x, 1.0 + lane_w * 0.75, tip.y);
        g.part(Matte).beam(a, c, 0.12, rgba(pal.barrier_red));
        g.part(Matte).boxed(Vec3::new(foot.x, 0.6, foot.y), Vec3::new(0.35, 1.0, 0.35), t.yaw, rgba(DARK));
        // A lane light over the lane on the canopy: green, go.
        let l = t.at(0.0, v + lane_w * 0.5);
        for sgn in [-1.0f32, 1.0] {
            let face = Vec3::new(d.x * sgn, 0.0, d.y * sgn);
            let lp = Vec3::new(l.x, 5.15, l.y) + face * 4.6;
            g.part(Glow).decal(lp, Vec3::new(right.x, 0.0, right.y), Vec3::Y, 0.6, 0.6, face, rgba(mul(pal.signal_green, 1.4)));
        }
    }
}

/// The canopy over everything, its fascia striped in neon with a TOLL
/// board each way, and lit from below.
fn canopy(g: &mut Geometry, t: &TollFrame, pal: &Palette) {
    let (b, d, right) = (t.b, t.d, t.right);
    let span = t.half + 1.0;
    let depth = if b.plaza { 10.0 } else { 9.0 };
    let top = 5.8;
    let cc = t.at(0.0, 0.0);
    g.part(Panel).boxed(Vec3::new(cc.x, top + 0.35, cc.y), Vec3::new(depth, 0.7, span * 2.0), t.yaw, rgba(mul(pal.panel(0), 0.7)));
    for sgn in [-1.0f32, 1.0] {
        let face = Vec3::new(d.x * sgn, 0.0, d.y * sgn);
        let fc = t.p3(sgn * (depth * 0.5 + 0.02), 0.0, top + 0.35);
        g.part(Glow).decal(fc + Vec3::Y * 0.22, Vec3::new(right.x, 0.0, right.y), Vec3::Y, span * 2.0, 0.12, face, rgba(mul(t.neon, 1.6)));
        g.part(Glow).decal(fc - Vec3::Y * 0.22, Vec3::new(right.x, 0.0, right.y), Vec3::Y, span * 2.0, 0.08, face, rgba(mul(t.neon2, 1.3)));
        // A TOLL board in the middle of the fascia: a lit panel with a
        // darker band where the letters would be.
        let board = fc + face * 0.05 + Vec3::Y * 1.0;
        let bw = if b.plaza { 7.0 } else { 3.6 };
        g.part(Glow).decal(board, Vec3::new(right.x, 0.0, right.y), Vec3::Y, bw, 1.1, face, rgba(mul(t.neon, 1.1)));
        g.part(Matte).decal(board + face * 0.03, Vec3::new(right.x, 0.0, right.y), Vec3::Y, bw * 0.7, 0.45, face, rgba(DARK));
        for k in 0..4 {
            let off = (k as f32 - 1.5) * bw * 0.17;
            let lp = board + face * 0.06 + Vec3::new(right.x, 0.0, right.y) * off;
            g.part(Glow).decal(lp, Vec3::new(right.x, 0.0, right.y), Vec3::Y, bw * 0.1, 0.32, face, rgba(mul(pal.lamp, 1.5)));
        }
    }
    // Strip lights under the canopy, across the road.
    for u in [-2.5f32, 2.5] {
        let a = t.p3(u, 0.0, top - 0.02);
        g.part(Glow).decal(a, Vec3::new(right.x, 0.0, right.y), Vec3::new(d.x, 0.0, d.y), span * 1.9, 0.25, Vec3::NEG_Y, rgba(mul(pal.lamp, 1.3)));
    }
}

/// A painted line on the road from `a` to `b`.
fn line(g: &mut Geometry, a: Vec2, b: Vec2, width: f32, y: f32, color: [f32; 4]) {
    let dd = b - a;
    let len = dd.length();
    if len < 1e-3 {
        return;
    }
    let mid = (a + b) * 0.5;
    g.part(Matte).decal(Vec3::new(mid.x, y, mid.y), Vec3::new(dd.x, 0.0, dd.y), Vec3::new(-dd.y, 0.0, dd.x), len, width, Vec3::Y, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booths_and_plazas_stay_near_the_road_and_on_the_ground() {
        let pal = Palette::default();
        for plaza in [false, true] {
            let b = TollDraw { pos: Vec2::new(50.0, -20.0), dir: Vec2::new(0.6, 0.8), lanes: 2, half_width: 5.0, plaza, id: 3, room: [30.0, 30.0] };
            let g = toll_booths(&[b], &pal);
            let mut n = 0;
            for (_, m) in g.iter() {
                for p in &m.positions {
                    n += 1;
                    let q = Vec2::new(p[0], p[2]);
                    assert!(q.distance(b.pos) < 40.0, "within the booth's reach: {q}");
                    assert!(p[1] >= -0.01 && p[1] < 10.0, "on the road, under 10 m (plaza lamps): {}", p[1]);
                    assert!(p.iter().all(|x| x.is_finite()));
                }
            }
            assert!(n > 100, "something is drawn");
        }
        let small = toll_booths(&[TollDraw { pos: Vec2::ZERO, dir: Vec2::X, lanes: 2, half_width: 5.0, plaza: false, id: 1, room: [30.0, 30.0] }], &pal);
        let big = toll_booths(&[TollDraw { pos: Vec2::ZERO, dir: Vec2::X, lanes: 2, half_width: 5.0, plaza: true, id: 1, room: [30.0, 30.0] }], &pal);
        let count = |g: &Geometry| g.iter().map(|(_, m)| m.positions.len()).sum::<usize>();
        assert!(count(&big) > count(&small) * 2, "the plaza has six booths and an apron");
        // Squeezed against another carriageway on its left, the plaza keeps
        // to the right of the centre line (a little over the road's edge).
        let tight = toll_booths(&[TollDraw { pos: Vec2::ZERO, dir: Vec2::X, lanes: 3, half_width: 6.75, plaza: true, id: 1, room: [7.0, 40.0] }], &pal);
        let min_y = tight.iter().flat_map(|(_, m)| m.positions.iter()).map(|p| p[2]).fold(f32::MAX, f32::min);
        assert!(min_y > -7.5, "nothing over the other carriageway: {min_y}");
    }
}
