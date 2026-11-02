//! The Stacks: shipping containers and old trailers piled into homes the
//! way people would do it: dropped on the parcel at mixed orientations, each
//! upper unit resting on the units below (most of its footprint must be
//! carried), overhanging corners propped on steel jacks, planks bridging
//! neighbouring roofs, and wires strung from a utility pole and between
//! masts on the roofs.
//!
//! [r9-stacks-look] Everything that people climb or that hangs is planned
//! against the pile before it is drawn ([`plan`]), so it attaches to what is
//! there: an upper unit's door opens onto a roof at its floor, or onto a
//! landing hung off its own wall whose flight runs down beside the pile to
//! the ground or a lower roof (a ladder where no flight fits); a door that
//! nothing reaches is not cut. Flights and landings keep clear of the boxes
//! and stand on stringers and posts; every wire runs between two anchors (the
//! pole's crossarm, a mast on an open roof) over clear air.

use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rail::BoxKind;
use rand::seq::{IndexedRandom, SliceRandom};
use rand::Rng;

/// Rise over run of a flight (a steep site stair, about 38 degrees).
pub(crate) const FLIGHT_SLOPE: f32 = 0.78;
/// Width of a flight, and how far a landing stands out from its wall.
pub(crate) const WALK_W: f32 = 0.9;
pub(crate) const LANDING_D: f32 = 1.0;
/// Half the length of a landing along its wall.
pub(crate) const LANDING_HALF: f32 = 0.8;
/// Clear height people need over a step, a landing or a roof they walk.
pub(crate) const HEADROOM: f32 = 2.0;
/// Height of the masts wires are tied to on a roof.
pub(crate) const MAST_H: f32 = 0.7;
/// Longest drop a flight takes (two boxes and a bit); a ladder beyond.
const MAX_FLIGHT_DROP: f32 = 6.0;
const MAX_LADDER: f32 = 7.5;
/// Most landings one pile hangs (the triangle budget).
const MAX_LANDINGS: usize = 6;

/// What a unit is built from: what can stand or be fixed on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell {
    /// A box or trailer: walls to cut doors in, a roof to walk.
    Box,
    /// An open-top box: walls, but a tarp for a roof.
    OpenTop,
    /// An ISO tank: a barrel in a frame, the pile's water store.
    Tank,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Unit {
    pub u: f32,
    pub v: f32,
    pub yaw: f32,
    pub len: f32,
    pub wid: f32,
    pub h: f32,
    pub level: i32,
    pub y0: f32,
    pub color: [f32; 3],
    /// [K34] The real box it is (`Ctx::boxes`), if any.
    pub face: Option<usize>,
    pub shell: Shell,
}

impl Unit {
    pub fn local_to_lot(&self, lu: f32, lv: f32) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(self.u + lu * c - lv * s, self.v + lu * s + lv * c)
    }

    pub fn along(&self) -> Vec2 {
        Vec2::new(self.yaw.cos(), self.yaw.sin())
    }

    pub fn across(&self) -> Vec2 {
        Vec2::new(-self.yaw.sin(), self.yaw.cos())
    }

    pub fn corners(&self) -> [Vec2; 4] {
        let (hl, hw) = (self.len * 0.5, self.wid * 0.5);
        [self.local_to_lot(-hl, -hw), self.local_to_lot(hl, -hw), self.local_to_lot(hl, hw), self.local_to_lot(-hl, hw)]
    }

    pub fn to_unit(&self, p: Vec2) -> Vec2 {
        let d = p - Vec2::new(self.u, self.v);
        Vec2::new(d.dot(self.along()), d.dot(self.across()))
    }

    pub fn contains(&self, p: Vec2) -> bool {
        self.contains_m(p, 0.0)
    }

    /// Whether `p` lies on the footprint grown by `m` (shrunk if negative).
    pub fn contains_m(&self, p: Vec2, m: f32) -> bool {
        let l = self.to_unit(p);
        l.x.abs() <= self.len * 0.5 + m && l.y.abs() <= self.wid * 0.5 + m
    }

    pub fn top(&self) -> f32 {
        self.y0 + self.h
    }

    /// Whether people can walk its roof (a tank's barrel or an open top's
    /// tarp they cannot).
    pub fn walk_roof(&self) -> bool {
        self.shell == Shell::Box
    }
}

fn units_overlap(a: &Unit, b: &Unit, margin: f32) -> bool {
    let ca = a.corners();
    let cb = b.corners();
    for axis in [ca[1] - ca[0], ca[3] - ca[0], cb[1] - cb[0], cb[3] - cb[0]] {
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

// ── the plan ─────────────────────────────────────────────────────────────────

/// What a door opens onto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Onto {
    Ground,
    /// The roof of a lower unit, at the door's floor.
    Roof,
    /// A landing hung off the unit's wall (index into `StackPlan::landings`).
    Landing(usize),
}

/// A door in a long wall of a unit: `lu` along it, on `side` (+1 or -1
/// across).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Door {
    pub unit: usize,
    pub lu: f32,
    pub side: f32,
    /// What it opens onto (read by the checks).
    #[cfg_attr(not(test), allow(dead_code))]
    pub onto: Onto,
}

/// A steel landing outside an upper unit's door, against its wall at its
/// floor, `LANDING_D` deep, from `lu0` to `lu1` along the wall.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Landing {
    pub unit: usize,
    pub lu0: f32,
    pub lu1: f32,
    pub side: f32,
    pub y: f32,
}

impl Landing {
    /// A point on the landing: `lu` along the wall, `out` from it.
    pub fn at(&self, units: &[Unit], lu: f32, out: f32) -> Vec2 {
        let u = &units[self.unit];
        u.local_to_lot(lu, self.side * (u.wid * 0.5 + out))
    }

    /// The landing's middle line, wall end to wall end, at its height.
    fn line(&self, units: &[Unit]) -> (Vec3, Vec3) {
        let a = self.at(units, self.lu0, LANDING_D * 0.5);
        let b = self.at(units, self.lu1, LANDING_D * 0.5);
        (Vec3::new(a.x, self.y, a.y), Vec3::new(b.x, self.y, b.y))
    }

    /// Whether lot-local `p` is over the landing.
    #[cfg(test)]
    pub fn covers(&self, units: &[Unit], p: Vec2, m: f32) -> bool {
        let u = &units[self.unit];
        let l = u.to_unit(p);
        let out = l.y * self.side - u.wid * 0.5;
        l.x >= self.lu0.min(self.lu1) - m && l.x <= self.lu0.max(self.lu1) + m && out >= -m && out <= LANDING_D + m
    }
}

/// A straight flight of steps, lot-local `(u, y, v)` of its centre line:
/// `top` at a landing's end, `foot` on the ground or a roof. `out` is the
/// direction away from the wall it runs beside.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flight {
    pub top: Vec3,
    pub foot: Vec3,
    pub out: Vec2,
    pub landing: usize,
}

/// A ladder up to a landing's outer edge: `foot` on the ground or a roof,
/// rails up to `top` (over the landing's deck, to hold on to), facing `out`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ladder {
    pub foot: Vec3,
    pub top: f32,
    pub out: Vec2,
    pub landing: usize,
}

/// A plank bridge from roof to roof, lot-local `(u, y, v)` of its ends,
/// each resting on a roof.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Plank {
    pub a: Vec3,
    pub b: Vec3,
    /// The two roofs it rests on (read by the checks).
    #[cfg_attr(not(test), allow(dead_code))]
    pub units: (usize, usize),
}

/// A steel mast on a roof that wires are tied to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mast {
    pub at: Vec2,
    /// The unit it stands on (read by the checks).
    #[cfg_attr(not(test), allow(dead_code))]
    pub unit: usize,
    pub y0: f32,
}

impl Mast {
    pub fn head(&self) -> Vec3 {
        Vec3::new(self.at.x, self.y0 + MAST_H - 0.05, self.at.y)
    }
}

/// What a wire is tied to at one end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Anchor {
    /// One end of the utility pole's crossarm (-1 or +1).
    Pole(f32),
    Mast(usize),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Wire {
    pub a: Anchor,
    pub b: Anchor,
    pub sag: f32,
    pub washing: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pole {
    pub at: Vec2,
    pub h: f32,
}

impl Pole {
    /// Where a wire is tied on the crossarm (lot-local `(u, y, v)`).
    pub fn arm(&self, end: f32) -> Vec3 {
        Vec3::new(self.at.x + end * 0.8, self.h - 0.55, self.at.y)
    }
}

/// The pile and everything that attaches to it, in lot-local coordinates.
#[derive(Clone, Debug, Default)]
pub(crate) struct StackPlan {
    pub units: Vec<Unit>,
    pub doors: Vec<Door>,
    pub landings: Vec<Landing>,
    pub flights: Vec<Flight>,
    pub ladders: Vec<Ladder>,
    pub planks: Vec<Plank>,
    pub masts: Vec<Mast>,
    pub wires: Vec<Wire>,
    pub pole: Option<Pole>,
}

impl StackPlan {
    pub fn anchor(&self, a: Anchor) -> Vec3 {
        match a {
            Anchor::Pole(end) => self.pole.map_or(Vec3::ZERO, |p| p.arm(end)),
            Anchor::Mast(i) => self.masts[i].head(),
        }
    }

    /// Whether anything of a unit is at `p` between heights `lo` and `hi`
    /// (footprint grown by `m`).
    pub fn solid(&self, p: Vec2, lo: f32, hi: f32, m: f32) -> bool {
        self.units.iter().any(|u| u.y0 < hi && u.top() > lo && u.contains_m(p, m))
    }

    /// The highest roof at `p` at or below `y`, or the ground.
    pub fn surface(&self, p: Vec2, y: f32) -> f32 {
        self.units.iter().filter(|u| u.top() <= y + 0.03 && u.contains(p)).map(|u| u.top()).fold(0.0, f32::max)
    }

    /// The unit whose roof is the walkable surface at `p`, height `y`.
    pub fn roof_at(&self, p: Vec2, y: f32) -> Option<usize> {
        self.units.iter().position(|u| (u.top() - y).abs() < 0.03 && u.contains_m(p, -0.05))
    }

    /// Whether people can stand at `p` at height `y`: on the lot, on the
    /// ground or a walkable roof at that height, with headroom over it.
    pub fn walkable(&self, c: &Ctx, p: Vec2, y: f32) -> bool {
        if !c.inside_local(p) || self.solid(p, y + 0.03, y + HEADROOM, 0.05) {
            return false;
        }
        if y < 0.03 {
            return self.surface(p, 0.0) < 0.03;
        }
        self.roof_at(p, y).is_some_and(|i| self.units[i].walk_roof()) && (self.surface(p, y) - y).abs() < 0.03
    }

    /// Whether the stairs, landings, ladders, planks or masts already
    /// planned take the space at `p` between `lo` and `hi`.
    pub fn taken(&self, p: Vec2, lo: f32, hi: f32) -> bool {
        self.taken_by(p, lo, hi, true)
    }

    /// [`StackPlan::taken`], the masts left out (`masts` false) for the
    /// wires tied to them.
    pub fn taken_by(&self, p: Vec2, lo: f32, hi: f32, masts: bool) -> bool {
        let seg = |a: Vec3, b: Vec3, half: f32, down: f32, up: f32| {
            let (a2, b2) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
            let d = b2 - a2;
            let l2 = d.length_squared();
            let t = if l2 < 1e-6 { 0.0 } else { ((p - a2).dot(d) / l2).clamp(0.0, 1.0) };
            let y = a.y + (b.y - a.y) * t;
            p.distance(a2 + d * t) <= half && hi > y - down && lo < y + up
        };
        self.landings.iter().any(|l| {
            let (a, b) = l.line(&self.units);
            seg(a, b, LANDING_D * 0.5 + 0.15, 0.3, HEADROOM)
        }) || self.flights.iter().any(|f| seg(f.top, f.foot, WALK_W * 0.5 + 0.15, 0.4, HEADROOM))
            || self.ladders.iter().any(|l| seg(l.foot, Vec3::new(l.foot.x, l.top, l.foot.z), 0.5, 0.1, HEADROOM.min(l.top - l.foot.y + 0.5)))
            || self.planks.iter().any(|k| seg(k.a, k.b, 0.45, 0.15, 0.5))
            || (masts && self.masts.iter().any(|m| p.distance(m.at) < 0.35 && hi > m.y0 && lo < m.y0 + MAST_H))
            || self.doors.iter().any(|d| {
                let u = &self.units[d.unit];
                let q = u.local_to_lot(d.lu, d.side * (u.wid * 0.5 + 0.45));
                p.distance(q) < 0.6 && hi > u.y0 && lo < u.y0 + HEADROOM
            })
    }
}

/// The pile and all that attaches to it, for drawing ([`stacks`]) and the
/// checks.
pub(crate) fn plan(c: &Ctx, rng: &mut impl Rng) -> StackPlan {
    let (lo, hi) = bounds(&c.lot_poly);
    let mut p = StackPlan { units: place_units(c, rng, lo, hi), ..StackPlan::default() };
    plan_doors(c, rng, &mut p);
    plan_planks(c, &mut p);
    plan_wires(c, rng, &mut p, lo, hi);
    p
}

/// The Stacks: shipping containers and trailers piled into homes, tied
/// together with landings, stairs, planks, wires and props.
pub fn stacks(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let (lo, hi) = bounds(&c.lot_poly);
    stack_alleys(g, c, lo, hi);
    let p = plan(c, rng);
    for i in 0..p.units.len() {
        dress_unit(g, c, rng, &p, i);
    }
    prop_units(g, c, rng, &p.units);
    draw_planks(g, c, &p);
    draw_access(g, c, &p);
    draw_utilities(g, c, rng, &p);
    stack_street(g, c, rng);
}

/// The ground and the trodden alleys through the middle of the lot.
fn stack_alleys(g: &mut Geometry, c: &Ctx, lo: Vec2, hi: Vec2) {
    let f = &c.frame;
    let pal = c.palette;
    ground(g, f, Dirt, &c.lot_poly, 0.03, pal.dirt);
    // Alleys: trodden paving through the middle of the lot both ways.
    // Laid in 2 m pieces so they stop at the edge of odd-shaped lots.
    let mut v = lo.y + 1.0;
    while v < hi.y {
        if poly_inside(c, &rect(0.0, v, 1.1, 1.0)) {
            f.flat(g, Paving, 0.0, 0.05, v, 2.2, 2.0, 0.0, mul(pal.paving, 0.8));
        }
        v += 2.0;
    }
    let mut u = lo.x + 1.0;
    while u < hi.x {
        if poly_inside(c, &rect(u, 0.0, 1.0, 1.1)) {
            f.flat(g, Paving, u, 0.05, 0.0, 2.0, 2.2, 0.0, mul(pal.paving, 0.8));
        }
        u += 2.0;
    }
}

/// The units, level by level: the ground ones scattered clear of the
/// alleys, each upper one resting on the highest roofs under it, most of
/// its footprint carried at that height.
fn place_units(c: &Ctx, rng: &mut impl Rng, lo: Vec2, hi: Vec2) -> Vec<Unit> {
    let pal = c.palette;
    let area = nd_sim::zoning::polygon_area(&c.lot_poly).abs();
    let mut units: Vec<Unit> = Vec::new();
    // [K34] A stack developer's home is piled from the boxes it bought, in
    // order; it uses each once and no trailers.
    let real = !c.boxes.is_empty();
    let mut next = 0usize;
    let ground_target = ((area / 40.0) as i32).clamp(2, 10);
    let levels = ((c.floors as f32 * 1.3) as i32).clamp(2, 8);
    let mut prev_count = ground_target;
    for level in 0..levels {
        let target = if level == 0 { ground_target } else { ((prev_count as f32 * rng.random_range(0.6..0.9)).round() as i32).max(1) };
        let mut placed = 0;
        let mut attempts = 0;
        while placed < target && attempts < if real { 160 } else { 90 } {
            attempts += 1;
            if real && next >= c.boxes.len() {
                break;
            }
            let trailer = rng.random_bool(0.2);
            let (len, wid, h) = if real {
                let forty = c.boxes[next].forty && c.boxes[next].kind != BoxKind::Tank;
                (if forty { CONTAINER_40 } else { CONTAINER_20 }, CONTAINER_W, CONTAINER_H)
            } else if trailer {
                (rng.random_range(6.5..8.5), 2.7, 2.9)
            } else if rng.random_bool(0.3) {
                (CONTAINER_40, CONTAINER_W, CONTAINER_H)
            } else {
                (CONTAINER_20, CONTAINER_W, CONTAINER_H)
            };
            let trailer = trailer && !real;
            let yaw = match rng.random_range(0..10) {
                0..=5 => 0.0,
                6..=8 => std::f32::consts::FRAC_PI_2,
                _ => rng.random_range(-0.45..0.45),
            };
            let (u, v) = if level == 0 {
                (rng.random_range(lo.x..hi.x), rng.random_range(lo.y..hi.y))
            } else {
                let below: Vec<&Unit> = units.iter().filter(|x| x.level == level - 1).collect();
                let Some(support) = below.choose(rng) else { break };
                let reach = support.len.max(len) * 0.45;
                let off = Vec2::new(rng.random_range(-reach..reach), rng.random_range(-2.0..2.0));
                let off = if support.yaw.abs() > 0.5 { Vec2::new(off.y, off.x) } else { off };
                (support.u + off.x, support.v + off.y)
            };
            let color = if trailer { mul([0.68, 0.66, 0.60], rng.random_range(0.6..0.95)) } else { pick(rng, &pal.container_colors) };
            let face = real.then_some(next);
            let color = face.map_or(color, |i| SHIPPING_LINES[c.boxes[i].line as usize % SHIPPING_LINES.len()].body);
            let shell = match face.map(|i| c.boxes[i].kind) {
                Some(BoxKind::Tank) => Shell::Tank,
                Some(BoxKind::OpenTop) => Shell::OpenTop,
                _ => Shell::Box,
            };
            let mut cand = Unit { u, v, yaw, len, wid, h, level, y0: 0.0, color, face, shell };
            // Stay inside the parcel (an overhang of half a metre is fine).
            if cand.corners().iter().any(|p| !c.inside_local(*p) && distance_to_boundary(*p, &c.lot_poly) > 0.6) {
                continue;
            }
            // Keep the alleys clear: a cross through the middle of the pile
            // that people walk and that joins the neighbours' alleys.
            if level == 0 && cand.corners().iter().any(|p| p.x.abs() < 1.3 || p.y.abs() < 1.3) && (cand.corners().iter().any(|p| p.x < -1.3) && cand.corners().iter().any(|p| p.x > 1.3) || cand.corners().iter().any(|p| p.y < -1.3) && cand.corners().iter().any(|p| p.y > 1.3)) {
                continue;
            }
            if units.iter().any(|o| o.level == level && units_overlap(o, &cand, 0.35)) {
                continue;
            }
            if level > 0 {
                // It rests on the highest roof anywhere under it (a trailer
                // stands taller than a box: a unit over both sits on the
                // trailer, never through it), and most of it must be
                // carried at that height.
                cand.y0 = units.iter().filter(|o| o.level < level && units_overlap(o, &cand, -0.01)).map(|o| o.top()).fold(0.0, f32::max);
                if cand.y0 <= 0.0 {
                    continue;
                }
                let mut carried = 0;
                for i in 0..5 {
                    for j in 0..3 {
                        let lu = (i as f32 + 0.5) / 5.0 * len - len * 0.5;
                        let lv = (j as f32 + 0.5) / 3.0 * wid - wid * 0.5;
                        let p = cand.local_to_lot(lu, lv);
                        if units.iter().any(|o| o.level < level && (o.top() - cand.y0).abs() < 0.05 && o.contains(p)) {
                            carried += 1;
                        }
                    }
                }
                if (carried as f32) < 15.0 * 0.55 {
                    continue;
                }
            }
            units.push(cand);
            placed += 1;
            if real {
                next += 1;
            }
        }
        if placed == 0 {
            break;
        }
        prev_count = placed;
    }
    units
}

/// Doors, and how people reach the upper ones: out onto a roof at the
/// door's floor, or a landing on the wall with a flight (or a ladder) down
/// to the ground or a lower roof. A door nothing reaches is left out.
fn plan_doors(c: &Ctx, rng: &mut impl Rng, p: &mut StackPlan) {
    let mut order: Vec<usize> = (0..p.units.len()).collect();
    order.sort_by_key(|&i| p.units[i].level);
    for i in order {
        let u = p.units[i];
        if u.shell == Shell::Tank || u.len < 5.0 || !coin(rng, if u.level == 0 { 0.7 } else { 0.8 }) {
            continue;
        }
        let first = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        let end = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        let flight_first = rng.random_bool(0.5);
        'sides: for side in [first, -first] {
            // Near either end, or mid-wall on a long box.
            let spots = [end * (u.len * 0.5 - 1.0), -end * (u.len * 0.5 - 1.0), if u.len > 9.0 { end * 1.2 } else { f32::NAN }];
            for lu in spots.into_iter().filter(|x| x.is_finite()) {
                let sill = u.local_to_lot(lu, side * (u.wid * 0.5 + 0.25));
                let step = u.local_to_lot(lu, side * (u.wid * 0.5 + 0.7));
                // Something to step out to, and room for the door to open.
                if !c.inside_local(step) || p.solid(sill, u.y0 + 0.1, u.y0 + HEADROOM, 0.02) || p.solid(step, u.y0 + 0.1, u.y0 + HEADROOM, 0.05) || p.taken(step, u.y0 + 0.1, u.y0 + HEADROOM) {
                    continue;
                }
                if u.level == 0 {
                    if p.walkable(c, step, 0.0) {
                        p.doors.push(Door { unit: i, lu, side, onto: Onto::Ground });
                        break 'sides;
                    }
                    continue;
                }
                // Straight out onto a lower box's roof (a ledge of it is enough).
                let wide = [u.local_to_lot(lu - 0.45, side * (u.wid * 0.5 + 0.7)), u.local_to_lot(lu + 0.45, side * (u.wid * 0.5 + 0.7))];
                if p.walkable(c, step, u.y0) && wide.iter().all(|q| p.walkable(c, *q, u.y0)) {
                    p.doors.push(Door { unit: i, lu, side, onto: Onto::Roof });
                    break 'sides;
                }
                // A big pile hangs a handful of landings; the rest of its
                // upper units are reached across the roofs and planks.
                if p.landings.len() >= MAX_LANDINGS {
                    continue;
                }
                if let Some(onto) = hang_landing(c, p, i, lu, side, flight_first) {
                    p.doors.push(Door { unit: i, lu, side, onto });
                    break 'sides;
                }
            }
        }
    }
}

/// A landing on unit `i`'s wall round a door at `lu` on `side`, with the
/// flight or ladder that reaches it; `None` (nothing planned) when neither
/// fits.
fn hang_landing(c: &Ctx, p: &mut StackPlan, i: usize, lu: f32, side: f32, flight_first: bool) -> Option<Onto> {
    let u = p.units[i];
    let y = u.y0;
    let landing = Landing { unit: i, lu0: lu - LANDING_HALF, lu1: lu + LANDING_HALF, side, y };
    // The deck, its rail and the tie rods need open air: nothing of a box
    // beside or just under it, and nothing else planned there.
    for a in [-1.0f32, 0.0, 1.0] {
        for out in [0.12f32, 0.5, 0.95] {
            let q = landing.at(&p.units, lu + a * LANDING_HALF, out);
            if !c.inside_local(q) || p.units.iter().any(|o| o.contains_m(q, 0.05) && o.top() > y - 0.3) || p.taken(q, y - 0.3, y + HEADROOM) {
                return None;
            }
        }
    }
    let li = p.landings.len();
    // Lower roofs a flight could land on, highest first, then the ground.
    let mut bases: Vec<f32> = p.units.iter().filter(|o| o.walk_roof() && o.top() < y - 1.2).map(|o| o.top()).collect();
    bases.sort_by(|a, b| b.total_cmp(a));
    bases.dedup_by(|a, b| (*a - *b).abs() < 0.03);
    bases.push(0.0);
    let out = u.across() * side;
    let mid = u.wid * 0.5 + LANDING_D * 0.5;
    let try_flight = |p: &StackPlan| -> Option<Flight> {
        for dir in [-lu.signum(), lu.signum()] {
            let top2 = u.local_to_lot(lu + dir * LANDING_HALF, side * mid);
            let along = u.along() * dir;
            for &base in &bases {
                let drop = y - base;
                if drop > MAX_FLIGHT_DROP {
                    continue;
                }
                let run = drop / FLIGHT_SLOPE;
                let foot2 = top2 + along * run;
                if flight_clear(c, p, top2, foot2, out, y, base) {
                    return Some(Flight { top: Vec3::new(top2.x, y, top2.y), foot: Vec3::new(foot2.x, base, foot2.y), out, landing: li });
                }
            }
        }
        None
    };
    let try_ladder = |p: &StackPlan| -> Option<Ladder> {
        let q = landing.at(&p.units, lu, LANDING_D + 0.15);
        let base = p.surface(q, y - 0.5);
        if y - base > MAX_LADDER || y - base < 1.0 || !p.walkable(c, q, base) {
            return None;
        }
        // A clear column from its foot to over the deck, and room to climb.
        let clear = (0..=6).all(|k| {
            let h = base + (y + 1.0 - base) * k as f32 / 6.0;
            !p.solid(q, h, h + 0.2, 0.3) && !p.solid(q + out * 0.5, h, h + 0.2, 0.1) && c.inside_local(q + out * 0.5)
        });
        let roomy = p.walkable(c, q + out * 0.6, base) && !p.taken(q, base, y) && !p.taken(q + out * 0.6, base, base + HEADROOM);
        (clear && roomy).then_some(Ladder { foot: Vec3::new(q.x, base, q.y), top: y + 1.0, out, landing: li })
    };
    let (flight, ladder) = if flight_first {
        let f = try_flight(&*p);
        (f, if f.is_none() { try_ladder(&*p) } else { None })
    } else {
        let l = try_ladder(&*p).filter(|l| y - l.foot.y > 3.2);
        (if l.is_none() { try_flight(&*p) } else { None }, l)
    };
    let ladder = if flight.is_none() && ladder.is_none() { try_ladder(&*p) } else { ladder };
    if flight.is_none() && ladder.is_none() {
        return None;
    }
    p.landings.push(landing);
    p.flights.extend(flight);
    p.ladders.extend(ladder);
    Some(Onto::Landing(li))
}

/// Whether a flight from `top2` (at `y`) down to `foot2` (at `base`) beside
/// a wall, `out` away from it, is open: on the lot, nothing of a box at or
/// over any step, nothing else planned in the way, and its foot on ground
/// or a roof with room to step off.
fn flight_clear(c: &Ctx, p: &StackPlan, top2: Vec2, foot2: Vec2, out: Vec2, y: f32, base: f32) -> bool {
    let run = top2.distance(foot2);
    let n = (run / 0.3).ceil().max(2.0) as usize;
    for k in 0..=n {
        let t = k as f32 / n as f32;
        let cpt = top2.lerp(foot2, t);
        let h = y + (base - y) * t;
        for off in [-0.4f32, 0.0, 0.4] {
            let q = cpt + out * off;
            if !c.inside_local(q) || p.units.iter().any(|o| o.top() > h + 0.03 && o.contains_m(q, 0.04)) || p.taken(q, h - 0.2, h + HEADROOM) {
                return false;
            }
        }
    }
    let ahead = (foot2 - top2).normalize_or_zero();
    [foot2 + ahead * 0.35, foot2 + ahead * 0.8, foot2 + ahead * 0.5 + out * 0.4, foot2 + ahead * 0.5 - out * 0.4].iter().all(|q| p.walkable(c, *q, base) && !p.taken(*q, base, base + HEADROOM))
}

/// Plank bridges between neighbouring roofs at one height, a short step
/// apart, each end resting on its roof.
fn plan_planks(c: &Ctx, p: &mut StackPlan) {
    let n = p.units.len();
    for i in 0..n {
        for j in (i + 1)..n {
            if p.planks.len() >= 5 {
                return;
            }
            let (a, b) = (p.units[i], p.units[j]);
            if !a.walk_roof() || !b.walk_roof() || (a.top() - b.top()).abs() > 0.03 {
                continue;
            }
            let (ca, cb) = (Vec2::new(a.u, a.v), Vec2::new(b.u, b.v));
            let d = (cb - ca).normalize_or_zero();
            let dist = ca.distance(cb);
            if d == Vec2::ZERO || dist > 16.0 {
                continue;
            }
            // How far along the line of centres each roof reaches.
            let reach = |u: &Unit| {
                let l = Vec2::new(d.dot(u.along()).abs(), d.dot(u.across()).abs());
                let tx = if l.x > 1e-4 { u.len * 0.5 / l.x } else { f32::MAX };
                let ty = if l.y > 1e-4 { u.wid * 0.5 / l.y } else { f32::MAX };
                tx.min(ty)
            };
            let (ra, rb) = (reach(&a), reach(&b));
            let gap = dist - ra - rb;
            if !(0.4..=2.4).contains(&gap) {
                continue;
            }
            let y = a.top();
            let pa = ca + d * (ra - 0.45);
            let pb = cb - d * (rb - 0.45);
            // Both ends on open roof, the gap open, the run on the lot.
            let ok = [pa, pb, pa + d.perp() * 0.3, pb - d.perp() * 0.3].iter().all(|q| p.walkable(c, *q, y))
                && (1..8).all(|k| {
                    let q = pa.lerp(pb, k as f32 / 8.0);
                    c.inside_local(q) && !p.units.iter().any(|o| o.top() > y + 0.03 && o.contains_m(q, 0.4)) && !p.taken(q, y, y + 0.6)
                });
            if ok {
                p.planks.push(Plank { a: Vec3::new(pa.x, y, pa.y), b: Vec3::new(pb.x, y, pb.y), units: (i, j) });
            }
        }
    }
}

/// Whether a wire from `a` to `b` sagging `sag` (as `MeshData::wire` hangs
/// it) runs over clear air: on the lot, clear of every box and of what
/// people walk; `drop` more below it for washing.
fn wire_clear(c: &Ctx, p: &StackPlan, a: Vec3, b: Vec3, sag: f32, drop: f32) -> bool {
    let len = a.distance(b);
    let steep = ((b - a).y / len.max(1e-3)).abs();
    let sag = if steep > 1e-3 { sag.min(0.8 * len / (4.0 * steep)) } else { sag };
    let n = ((len / 0.25) as usize).clamp(16, 120);
    (1..n).all(|k| {
        let t = k as f32 / n as f32;
        let q = a.lerp(b, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t));
        let q2 = Vec2::new(q.x, q.z);
        c.inside_local(q2) && !p.solid(q2, q.y - 0.12 - drop, q.y + 0.12, 0.12) && !p.taken_by(q2, q.y - drop, q.y, false)
    })
}

/// A mast on an open corner of unit `i`'s roof (made if not there yet).
fn mast_at(p: &mut StackPlan, i: usize, at: Vec2) -> usize {
    if let Some(k) = p.masts.iter().position(|m| m.at.distance(at) < 0.05) {
        return k;
    }
    p.masts.push(Mast { at, unit: i, y0: p.units[i].top() });
    p.masts.len() - 1
}

/// The utility pole and the wires: from the pole's crossarm to masts on
/// open roof corners, and between masts (with washing on some).
fn plan_wires(c: &Ctx, rng: &mut impl Rng, p: &mut StackPlan, lo: Vec2, hi: Vec2) {
    let pole_h = 9.0;
    // The pole stands on open ground near a corner of the lot, clear of the
    // boxes and of the stairs.
    let anchor = interior_anchor(&c.lot_poly);
    let starts = [Vec2::new(hi.x - 1.0, lo.y + 0.8), Vec2::new(lo.x + 1.0, lo.y + 0.8), Vec2::new(hi.x - 1.0, hi.y - 0.8), Vec2::new(lo.x + 1.0, hi.y - 0.8)];
    'find: for s in starts {
        let mut q = s;
        for _ in 0..10 {
            let free = c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.3 && !p.units.iter().any(|u| u.y0 < pole_h && u.contains_m(q, 0.6)) && !p.taken(q, 0.0, pole_h) && [-1.0f32, 1.0].iter().all(|e| c.inside_local(q + Vec2::new(e * 0.9, 0.0)) && !p.solid(q + Vec2::new(e * 0.9, 0.0), pole_h - 1.0, pole_h, 0.2));
            if free {
                p.pole = Some(Pole { at: q, h: pole_h });
                break 'find;
            }
            q = q.lerp(anchor, 0.2);
        }
    }
    // Open roof corners: nothing resting over them, nothing planned there.
    let mut corners: Vec<(usize, Vec2)> = Vec::new();
    for (i, u) in p.units.iter().enumerate() {
        if u.shell == Shell::Tank {
            continue;
        }
        for k in u.corners() {
            let q = toward(k, Vec2::new(u.u, u.v), 0.25);
            if !p.units.iter().any(|o| o.y0 > u.top() - 0.05 && o.contains_m(q, 0.3)) && !p.taken(q, u.top(), u.top() + MAST_H + 0.3) && c.inside_local(q) {
                corners.push((i, q));
            }
        }
    }
    if let Some(pole) = p.pole {
        // Service drops to a few units, upper ones first.
        let mut cand: Vec<(usize, Vec2)> = corners.iter().copied().filter(|(i, _)| p.units[*i].top() + MAST_H < pole.h - 0.8).collect();
        cand.shuffle(rng);
        cand.sort_by_key(|(i, _)| -p.units[*i].level);
        let mut done: Vec<usize> = Vec::new();
        for (i, q) in cand {
            if done.len() >= 3 {
                break;
            }
            if done.contains(&i) {
                continue;
            }
            let end = if q.x >= pole.at.x { 1.0 } else { -1.0 };
            let a = pole.arm(end);
            let b = Vec3::new(q.x, p.units[i].top() + MAST_H - 0.05, q.y);
            let d = Vec2::new(a.x, a.z).distance(q);
            if !(2.5..=26.0).contains(&d) || !wire_clear(c, p, a, b, 0.5, 0.0) {
                continue;
            }
            let m = mast_at(p, i, q);
            p.wires.push(Wire { a: Anchor::Pole(end), b: Anchor::Mast(m), sag: 0.5, washing: false });
            done.push(i);
        }
    }
    // Lines between roofs, some with washing.
    let mut tries = rng.random_range(4..9);
    let mut lines = 0;
    while tries > 0 && lines < 4 && corners.len() >= 2 {
        tries -= 1;
        let (Some(&(ia, qa)), Some(&(ib, qb))) = (corners.choose(rng), corners.choose(rng)) else { break };
        if ia == ib || !(3.0..=13.0).contains(&qa.distance(qb)) {
            continue;
        }
        let a = Vec3::new(qa.x, p.units[ia].top() + MAST_H - 0.05, qa.y);
        let b = Vec3::new(qb.x, p.units[ib].top() + MAST_H - 0.05, qb.y);
        if (a.y - b.y).abs() > 3.0 {
            continue;
        }
        let washing = rng.random_bool(0.5);
        if !wire_clear(c, p, a, b, 0.4, if washing { 0.8 } else { 0.0 }) {
            continue;
        }
        let (ma, mb) = (mast_at(p, ia, qa), mast_at(p, ib, qb));
        p.wires.push(Wire { a: Anchor::Mast(ma), b: Anchor::Mast(mb), sag: 0.4, washing });
        lines += 1;
    }
}

// ── drawing ──────────────────────────────────────────────────────────────────

/// One unit: its shell, tags, window, door, boarding, tarps and roof gear,
/// each on a face or roof that is open where it goes.
fn dress_unit(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: &StackPlan, i: usize) {
    let unit = &p.units[i];
    let f = &c.frame;
    let pal = c.palette;
    let lit_chance = c.lit_chance();
    // Each unit jitters and fades its own paint in `container_unit`.
    let col = mul(unit.color, 1.0 - 0.3 * c.wear);
    let yc = unit.y0 + unit.h * 0.5;
    match unit.face {
        // [K34] The very box bought: its line, wear and number.
        Some(k) => real_box(g, c, unit.u, unit.v, unit.yaw, unit.y0, &c.boxes[k], true),
        None => container_unit(g, c, rng, unit.u, unit.v, unit.yaw, unit.len, unit.wid, unit.h, unit.y0, col),
    }
    if unit.shell == Shell::Tank {
        // A barrel in a frame: no walls to cut or hang things on.
        return;
    }
    let door = p.doors.iter().find(|d| d.unit == i).copied();
    let side = door.map_or_else(|| if rng.random_bool(0.5) { -1.0 } else { 1.0 }, |d| d.side);
    // Whether a wall face is open `out` from the wall at `lu`, between two
    // heights: no box against it, nothing planned in front of it.
    let open = |lu: f32, s: f32, out: f32, lo: f32, hi: f32| {
        let q = unit.local_to_lot(lu, s * (unit.wid * 0.5 + out));
        c.inside_local(q) && !p.solid(q, lo, hi, 0.05) && !p.taken(q, lo, hi)
    };
    // Tags along a long side of the lower units, where a spray can reaches.
    if unit.level <= 1 && rng.random_bool(0.55) {
        let s = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let ga = unit.local_to_lot(s * unit.len * 0.5, s * unit.wid * 0.5);
        let gb = unit.local_to_lot(-s * unit.len * 0.5, s * unit.wid * 0.5);
        graffiti(g, c, rng, &Face::from_edge(ga, gb), unit.y0 + 0.15, unit.y0 + unit.h - 0.15, 1.6);
    }
    let place = |g: &mut Geometry, s, lu: f32, y: f32, lv: f32, size: Vec3, color: [f32; 3]| {
        let q = unit.local_to_lot(lu, lv);
        f.bx_yaw(g, s, q.x, y, q.y, size, color, unit.yaw);
    };
    // Openings cut in the long side: a window and the door, as decals
    // proud of the corrugation (the frame, then the pane or leaf).
    let along = f.dir3(unit.along());
    let out = f.dir3(unit.across()) * side;
    let opening = |g: &mut Geometry, s, lu: f32, y: f32, d: f32, w: f32, h: f32, color: [f32; 3]| {
        g.part(s).decal(f.at2(unit.local_to_lot(lu, side * (unit.wid * 0.5 + d)), y), along, Vec3::Y, w, h, out, rgba(color));
    };
    // The window at the other end of the wall from the door, where the
    // wall looks out at something other than the next box.
    let away = door.map_or_else(|| if rng.random_bool(0.5) { -1.0 } else { 1.0 }, |d| -d.lu.signum());
    let win_u = away * rng.random_range(0.3..(unit.len * 0.3).max(0.4));
    if coin(rng, lit_chance) && open(win_u, side, 0.6, yc - 0.2, yc + 0.8) {
        let wc = if rng.random_bool(0.3) { [0.5, 0.75, 1.1] } else { pick(rng, &c.style.window_colors) };
        opening(g, Matte, win_u, yc + 0.3, 0.05, 1.1, 0.9, DARK);
        window_light(g, c, |g| opening(g, Glow, win_u, yc + 0.3, 0.06, 0.9, 0.7, mul(wc, 0.9)));
    }
    if let Some(d) = door {
        let dh = (unit.h - 0.6).min(2.0);
        let door_col = if rng.random_bool(0.5) { mul(col, 0.7) } else { mul(pick(rng, &pal.tarp_colors), 0.6) };
        opening(g, Matte, d.lu, unit.y0 + 0.16 + dh * 0.5 + 0.03, 0.05, 1.0, dh + 0.06, DARK);
        opening(g, Matte, d.lu, unit.y0 + 0.16 + dh * 0.5, 0.06, 0.82, dh, door_col);
    }
    // Boarding and a tarp on the back wall (the door's wall keeps clear).
    if coin(rng, 0.25 + 0.4 * c.wear) {
        let lu = rng.random_range(-unit.len * 0.3..unit.len * 0.3);
        let y = yc + rng.random_range(-0.5..0.5);
        let size = Vec3::new(rng.random_range(0.6..1.4), rng.random_range(0.6..1.2), 0.05);
        if open(lu, -side, 0.3, y - 0.6, y + 0.6) {
            place(g, Wood, lu, y, -side * (unit.wid * 0.5 + 0.05), size, pal.plywood);
        }
    }
    if rng.random_bool(0.25) {
        let tc = mul(pick(rng, &pal.tarp_colors), 0.8);
        let lu = rng.random_range(-unit.len * 0.25..unit.len * 0.25);
        let tl = rng.random_range(2.0..(unit.len - 2.0 * lu.abs()).clamp(2.1, 4.0));
        let y = yc + unit.h * 0.15;
        if [lu - tl * 0.5, lu, lu + tl * 0.5].iter().all(|&t| open(t, -side, 0.3, y - unit.h * 0.38, y + unit.h * 0.38)) {
            place(g, Tarp, lu, y, -side * (unit.wid * 0.5 + 0.1), Vec3::new(tl, unit.h * 0.75, 0.08), tc);
        }
    }
    // Roof gear, each piece on open roof: nothing resting over it, no
    // landing, flight foot, plank or mast where it stands.
    if unit.shell == Shell::Box {
        let top = unit.top();
        let roof_open = |pts: &[(f32, f32)], hi: f32| {
            pts.iter().all(|&(lu, lv)| {
                let q = unit.local_to_lot(lu, lv);
                !p.units.iter().any(|o| o.y0 > top - 0.05 && o.y0 < top + hi && o.contains_m(q, 0.1)) && !p.taken(q, top, top + hi)
            })
        };
        if rng.random_bool(0.45) {
            let tc = mul(pick(rng, &pal.tarp_colors), 0.85);
            // A sheet no longer than the roof it lies on.
            let lu = rng.random_range(-unit.len * 0.2..unit.len * 0.2);
            let tl = rng.random_range(2.5..(unit.len - 2.0 * lu.abs()).max(2.6));
            let (a, b, w) = (lu - tl * 0.5, lu + tl * 0.5, unit.wid * 0.45);
            if roof_open(&[(a, -w), (a, w), (b, -w), (b, w), (lu, 0.0)], 0.3) {
                place(g, Tarp, lu, top + 0.08, 0.0, Vec3::new(tl, 0.1, unit.wid * 0.9), tc);
            }
        }
        if rng.random_bool(0.4) {
            let lu = rng.random_range(-unit.len * 0.35..unit.len * 0.35);
            if roof_open(&[(lu, 0.0), (lu - 0.7, 0.0), (lu + 0.7, 0.0), (lu, 0.7), (lu, -0.7)], 1.6) {
                dish(g, f, unit.local_to_lot(lu, 0.0), top, 0.7, pal.steel);
            }
        }
        if rng.random_bool(0.35) {
            let (lu, lv) = (rng.random_range(-unit.len * 0.35..unit.len * 0.35), rng.random_range(-0.6..0.6));
            if roof_open(&[(lu - 0.45, lv - 0.45), (lu + 0.45, lv - 0.45), (lu - 0.45, lv + 0.45), (lu + 0.45, lv + 0.45)], 1.0) {
                place(g, Metal, lu, top + 0.45, lv, Vec3::new(0.9, 0.9, 0.9), if rng.random_bool(0.5) { c.palette.rust } else { [0.2, 0.3, 0.6] });
            }
        }
    }
    // An air conditioner under the window, hard against the wall.
    if rng.random_bool(0.25) {
        let lv = side * (unit.wid * 0.5 + 0.3);
        if open(win_u, side, 0.3, yc - 0.9, yc - 0.3) && open(win_u, side, 0.65, yc - 0.9, yc - 0.3) {
            place(g, Metal, win_u, yc - 0.6, lv, Vec3::new(0.8, 0.5, 0.6), if rng.random_bool(0.4) { c.palette.rust } else { [0.5, 0.5, 0.5] });
        }
    }
}

/// Rusted I-beam props (and cribbing) under the upper units' unsupported
/// corners, tied into frames or raked.
fn prop_units(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, units: &[Unit]) {
    let f = &c.frame;
    let pal = c.palette;
    // Rusted I-beam props under unsupported corners; timber cribbing where
    // the gap is too short for a beam. Two tall props at one end of a unit
    // are tied into a frame (a header and, higher up, a cross brace); a lone
    // tall one gets a raking strut.
    let mut placed: Vec<(Vec2, f32)> = Vec::new();
    for unit in units.iter().filter(|x| x.level > 0) {
        let mut props: [Option<(Vec2, f32)>; 4] = [None; 4];
        for (ci, corner) in unit.corners().into_iter().enumerate() {
            let inset = toward(corner, Vec2::new(unit.u, unit.v), 0.35);
            // Carried there by a roof at its floor.
            if units.iter().any(|o| o.level < unit.level && (o.top() - unit.y0).abs() < 0.05 && o.contains(inset)) {
                continue;
            }
            let base = units.iter().filter(|o| o.level < unit.level && o.top() < unit.y0 - 0.05 && o.contains(inset)).map(|o| o.top()).fold(0.0, f32::max);
            let h = unit.y0 - base;
            // Neighbouring units' corners share a prop.
            if placed.iter().any(|(q, y)| q.distance(inset) < 0.7 && (y - unit.y0).abs() < 0.1) {
                continue;
            }
            if h > 0.6 {
                ibeam_support(g, c, rng, inset, unit.yaw, base, unit.y0, base <= 0.0);
                props[ci] = Some((inset, base));
                placed.push((inset, unit.y0));
            } else if h > 0.05 {
                f.bx_yaw(g, Wood, inset.x, base + h * 0.5, inset.y, Vec3::new(0.5, h, 0.3), mul(pal.wood, 0.7), unit.yaw);
            }
        }
        let brace = mul(pal.rust, 0.8);
        // Corners 0 and 3 are one end of the unit, 1 and 2 the other.
        for (i, j) in [(0usize, 3usize), (1, 2)] {
            match (props[i], props[j]) {
                (Some((a, ba)), Some((b, bb))) if (ba - bb).abs() < 0.1 && unit.y0 - ba > 2.0 => {
                    // The plane of the frame, for the braces' faces.
                    let n = f.dir3((b - a).perp()).normalize_or_zero();
                    let top = unit.y0 - 0.12;
                    let m = g.part(Rust);
                    flat_bar(m, f.at2(a, top), f.at2(b, top), 0.2, n, brace);
                    if unit.y0 - ba > 3.2 {
                        flat_bar(m, f.at2(a, ba + 0.5), f.at2(b, top - 0.3), 0.08, n, brace);
                        flat_bar(m, f.at2(b, bb + 0.5), f.at2(a, top - 0.3), 0.08, n, brace);
                    }
                }
                _ => {
                    for (q, base) in [props[i], props[j]].into_iter().flatten() {
                        if unit.y0 - base > 3.2 {
                            // Raking strut from two-thirds up to the ground (or roof)
                            // a metre and a half in under the unit.
                            let foot = toward(q, Vec2::new(unit.u, unit.v), 1.5);
                            if units.iter().filter(|o| o.level < unit.level && o.top() < unit.y0 - 0.05 && o.contains(foot)).map(|o| o.top()).fold(0.0, f32::max) == base {
                                let n = f.dir3((foot - q).perp()).normalize_or_zero();
                                flat_bar(g.part(Rust), f.at2(q, base + (unit.y0 - base) * 0.66), f.at2(foot, base + 0.05), 0.12, n, brace);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Plank bridges: two scaffold boards side by side on a pair of bearers,
/// one resting on each roof.
fn draw_planks(g: &mut Geometry, c: &Ctx, p: &StackPlan) {
    let f = &c.frame;
    let pal = c.palette;
    for k in &p.planks {
        let (a, b) = (Vec2::new(k.a.x, k.a.z), Vec2::new(k.b.x, k.b.z));
        let d = (b - a).normalize_or_zero();
        let yaw = d.y.atan2(d.x);
        let span = a.distance(b);
        let mid = (a + b) * 0.5;
        for (s, shade) in [(-0.17f32, 1.0f32), (0.17, 0.85)] {
            let q = mid + d.perp() * s;
            f.bx_yaw(g, Wood, q.x, k.a.y + 0.11, q.y, Vec3::new(span, 0.05, 0.3), mul(pal.wood, shade), yaw);
        }
        for q in [a, b] {
            f.bx_yaw(g, Wood, q.x, k.a.y + 0.045, q.y, Vec3::new(0.12, 0.08, 0.8), mul(pal.wood, 0.6), yaw);
        }
    }
}

/// A thin steel rod from `a` to `b`: a three-sided section without end
/// caps (six triangles), for rails, posts and ladder stiles.
fn rod(m: &mut crate::mesh::MeshData, a: Vec3, b: Vec3, t: f32, color: [f32; 3]) {
    let d = (b - a).normalize_or_zero();
    if d == Vec3::ZERO {
        return;
    }
    let helper = if d.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    let p1 = d.cross(helper).normalize() * t * 0.6;
    let p2 = d.cross(p1).normalize() * t * 0.6;
    let ring = |c: Vec3| [c + p1, c + p2 * 0.87 - p1 * 0.5, c - p2 * 0.87 - p1 * 0.5];
    m.frustum_capped(&ring(a), &ring(b), false, false, rgba(color));
}

/// A flat plate from `a` to `b`, `width` across, lying level (a tread).
fn plate(m: &mut crate::mesh::MeshData, a: Vec3, b: Vec3, width: f32, color: [f32; 3]) {
    flat_bar(m, a, b, width, Vec3::Y, color);
}

/// The landings, flights and ladders.
fn draw_access(g: &mut Geometry, c: &Ctx, p: &StackPlan) {
    let f = &c.frame;
    let steel = mul(c.palette.steel, 0.45 - 0.15 * c.wear);
    let (s, color) = outdoor_steel(c, steel);
    let rail = if s == Rust { Rust } else { Matte };
    let w3 = |q: Vec2, y: f32| f.at2(q, y);
    for (li, l) in p.landings.iter().enumerate() {
        let u = &p.units[l.unit];
        let mid = (l.lu0 + l.lu1) * 0.5;
        let len = (l.lu1 - l.lu0).abs();
        // The deck: chequer plate on a channel frame, its top just over
        // the door's sill.
        let deck = l.at(&p.units, mid, LANDING_D * 0.5 + 0.02);
        f.bx_yaw(g, s, deck.x, l.y + 0.02, deck.y, Vec3::new(len, 0.1, LANDING_D - 0.04), color, u.yaw);
        // Tie rods from the outer corners up to brackets on the wall.
        let n = f.dir3(u.along());
        for e in [l.lu0 + 0.1, l.lu1 - 0.1] {
            flat_bar(g.part(rail), w3(l.at(&p.units, e, LANDING_D - 0.05), l.y + 0.07), w3(l.at(&p.units, e, 0.02), l.y + 1.85), 0.05, n, color);
        }
        // Rails on the open edges: the front, and the end the stair does
        // not leave from (both ends with a ladder).
        let flight = p.flights.iter().find(|fl| fl.landing == li);
        let stair_end = flight.map(|fl| {
            let t = u.to_unit(Vec2::new(fl.top.x, fl.top.z)).x;
            if (t - l.lu0).abs() < (t - l.lu1).abs() { l.lu0 } else { l.lu1 }
        });
        let ladder = p.ladders.iter().any(|ld| ld.landing == li);
        let (r0, r1) = (l.at(&p.units, l.lu0, LANDING_D - 0.04), l.at(&p.units, l.lu1, LANDING_D - 0.04));
        if ladder {
            // A gap in the front rail where the ladder comes up.
            for (a, b) in [(l.lu0, mid - 0.3), (mid + 0.3, l.lu1)] {
                let (qa, qb) = (l.at(&p.units, a, LANDING_D - 0.04), l.at(&p.units, b, LANDING_D - 0.04));
                rod(g.part(rail), w3(qa, l.y + 1.0), w3(qb, l.y + 1.0), 0.05, color);
            }
        } else {
            rod(g.part(rail), w3(r0, l.y + 1.0), w3(r1, l.y + 1.0), 0.05, color);
            rod(g.part(rail), w3(r0, l.y + 0.5), w3(r1, l.y + 0.5), 0.035, color);
        }
        for (e, q) in [(l.lu0, r0), (l.lu1, r1)] {
            rod(g.part(rail), w3(q, l.y + 0.07), w3(q, l.y + 1.03), 0.05, color);
            if stair_end != Some(e) {
                rod(g.part(rail), w3(q, l.y + 1.0), w3(l.at(&p.units, e, 0.03), l.y + 1.0), 0.05, color);
            }
        }
    }
    for fl in &p.flights {
        let (t2, f2) = (Vec2::new(fl.top.x, fl.top.z), Vec2::new(fl.foot.x, fl.foot.z));
        let drop = fl.top.y - fl.foot.y;
        let ahead = (f2 - t2).normalize_or_zero();
        // Treads: open steel plates, a steep 25 cm a step.
        let steps = (drop / 0.25).round().max(2.0) as usize;
        let rise = drop / steps as f32;
        let depth = t2.distance(f2) / steps as f32 + 0.04;
        for k in 1..steps {
            let q = t2.lerp(f2, k as f32 / steps as f32);
            let y = fl.top.y - rise * k as f32;
            let half = fl.out * (WALK_W * 0.5 - 0.04);
            plate(g.part(s), w3(q - half, y), w3(q + half, y), depth, color);
        }
        // Stringers: steel channels from the landing to the foot, each a
        // plate on edge.
        let side_n = f.dir3(fl.out);
        for o in [-0.5f32, 0.5] {
            let off = fl.out * o * WALK_W;
            flat_bar(g.part(s), w3(t2 + off, fl.top.y - 0.08), w3(f2 + off + ahead * 0.15, fl.foot.y + 0.1), 0.2, side_n, mul(color, 0.85));
            // Handrail on posts at both ends.
            let (ta, fa) = (t2 + off * 1.05, f2 + off * 1.05 - ahead * 0.1);
            rod(g.part(rail), w3(ta, fl.top.y + 1.0), w3(fa, fl.foot.y + 1.0), 0.05, color);
            rod(g.part(rail), w3(fa, fl.foot.y + 0.04), w3(fa, fl.foot.y + 1.03), 0.05, color);
        }
        // A foot plate where it lands, and posts under a long flight down
        // to whatever is under them.
        let posts: &[f32] = if drop > 4.0 { &[0.33, 0.66] } else if drop > 1.8 { &[0.5] } else { &[] };
        for &t in posts {
            let y = fl.top.y - drop * t - 0.12;
            for o in [-0.5f32, 0.5] {
                let q = t2.lerp(f2, t) + fl.out * o * WALK_W;
                let base = p.surface(q, y);
                if y - base > 0.2 {
                    rod(g.part(s), w3(q, base), w3(q, y), 0.09, mul(color, 0.8));
                }
            }
        }
    }
    for ld in &p.ladders {
        let q = Vec2::new(ld.foot.x, ld.foot.z);
        let along = ld.out.perp();
        let n = f.dir3(ld.out);
        let m = g.part(rail);
        for o in [-0.22f32, 0.22] {
            rod(m, w3(q + along * o, ld.foot.y), w3(q + along * o, ld.top), 0.05, color);
        }
        let rungs = ((ld.top - 1.0 - ld.foot.y) / 0.3) as i32;
        for k in 1..=rungs {
            let y = ld.foot.y + k as f32 * 0.3;
            flat_bar(m, w3(q - along * 0.22, y), w3(q + along * 0.22, y), 0.04, n, color);
        }
        // Brackets to the landing's edge.
        for o in [-0.22f32, 0.22] {
            let a = q + along * o;
            rod(m, w3(a, ld.top - 1.0), w3(a - ld.out * 0.17, ld.top - 1.0), 0.04, color);
        }
    }
}

/// The utility pole, the masts on the roofs and the wires between them,
/// washing on some.
fn draw_utilities(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: &StackPlan) {
    let f = &c.frame;
    let pal = c.palette;
    if let Some(pole) = p.pole {
        let (q, h) = (pole.at, pole.h);
        f.bx(g, Wood, q.x, h * 0.5, q.y, Vec3::new(0.3, h, 0.3), pal.bark);
        f.bx(g, Wood, q.x, h - 0.6, q.y, Vec3::new(1.8, 0.12, 0.12), pal.bark);
        // The lamp on the pole's side, under its arm.
        f.bx(g, Glow, q.x, h - 1.6, q.y - 0.35, Vec3::new(0.5, 0.25, 0.4), pal.street_lamp);
        // Insulators where the wires are tied.
        for e in [-1.0f32, 1.0] {
            let a = pole.arm(e);
            f.bx(g, Matte, a.x, a.y - 0.02, a.z, Vec3::new(0.08, 0.12, 0.08), mul(pal.steel, 0.5));
        }
    }
    let steel = mul(pal.steel, 0.5);
    for m in &p.masts {
        rod(g.part(Matte), f.at2(m.at, m.y0), f.at2(m.at, m.y0 + MAST_H), 0.07, steel);
        // Its foot plate, bolted to the roof.
        let d = f.dir3(Vec2::X) * 0.13;
        plate(g.part(Matte), f.at2(m.at, m.y0 + 0.015) - d, f.at2(m.at, m.y0 + 0.015) + d, 0.26, mul(steel, 0.8));
    }
    for w in &p.wires {
        let (a, b) = (p.anchor(w.a), p.anchor(w.b));
        let (fa, fb) = (f.at(a.x, a.y, a.z), f.at(b.x, b.y, b.z));
        g.part(Matte).wire(fa, fb, w.sag, if w.washing { 0.03 } else { 0.05 }, rgba(DARK));
        if w.washing {
            let d = fb - fa;
            let yaw = d.z.atan2(d.x);
            for k in 1..4 {
                let t = k as f32 / 4.0;
                let q = fa.lerp(fb, t) - Vec3::Y * (w.sag * 4.0 * t * (1.0 - t));
                let tc = pick(rng, &pal.tarp_colors);
                g.part(Tarp).boxed(Vec3::new(q.x, q.y - 0.37, q.z), Vec3::new(0.5, 0.7, 0.04), yaw + rng.random_range(-0.15..0.15), rgba(mul(tc, 1.4)));
            }
        }
    }
}

/// Street level: a drum fire and a wreck.
fn stack_street(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    // Street level: a drum fire and junk.
    let front = c.front_face(&c.lot_poly);
    if rng.random_bool(0.5) {
        drum_fire(g, f, pal, front.at(rng.random_range(-front.len * 0.3..front.len * 0.3), -1.6));
    }
    if rng.random_bool(0.4) {
        let q = front.at(rng.random_range(-front.len * 0.3..front.len * 0.3), -4.0);
        let yaw = front.yaw() + rng.random_range(-0.3..0.3);
        if vehicle_fits(c, q, yaw, Vehicle::Wreck) {
            let col = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, q, 0.0, yaw, Vehicle::Wreck, col);
        }
    }
}
