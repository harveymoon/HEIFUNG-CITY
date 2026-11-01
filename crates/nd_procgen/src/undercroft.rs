//! [r5-layered] The world under a raised road, drawn from a bay of the
//! simulation (`nd_sim::undercroft::Bay`): a worn concrete floor with oil
//! stains and puddles, sodium lamps hung from the deck's soffit and a
//! conduit along it, posters, graffiti and drip stains on the columns, and
//! what the bay is used for:
//!
//! - a **market**: rows of stalls under sagging tarps, goods on the
//!   counters (crates of produce, racks of clothes, phone cases), a bulb
//!   under every canopy, strings of bulbs from column to column, a hand
//!   cart; after hours the counters are sheeted over;
//! - **hawker stalls**: steel cart counters with a wok burner, gas
//!   bottles, a lit menu box over each, folding tables with plastic
//!   stools, bin bags;
//! - **workshops**: corrugated lock-ups built between the columns, as tall
//!   as the deck lets them be, their roller shutters up in the day (a
//!   strip light inside, a scooter or a car on blocks outside, tyres and
//!   drums), down at night;
//! - a **court**: a painted court, a hoop each end, fences along the sides
//!   and floodlights under the deck;
//! - **bare** ground: gravel, drums, a pallet stack, a dumped mattress, a
//!   run of chain-link fence, one weak lamp;
//! - a **camp** shelters under its deck (drawn by `society::camp`); the
//!   bay adds the floor and the stains.
//!
//! Everything is laid out in a frame on the bay (`u` along the deck, `v`
//! across it), from a seed, with y up from the ground; nothing stands
//! taller than `headroom`. Lamps after dark are `NIGHT_MARK` glow.

use crate::buildings::NIGHT_MARK;
use crate::color::{mix, mul, pick, rgba, DARK};
use crate::palette::Palette;
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use nd_sim::undercroft::UnderUse;
use rand::Rng;

/// The floor's height over the ground (over a lot's ground and a park's
/// lawn).
pub const FLOOR_Y: f32 = 0.07;

/// What a bay is drawn from.
#[derive(Clone, Debug)]
pub struct BayLook {
    pub pos: Vec2,
    pub dir: Vec2,
    pub len: f32,
    pub width: f32,
    /// Ground to the deck's soffit, metres.
    pub headroom: f32,
    pub use_: UnderUse,
    /// Stalls, lock-ups or hoops (`UnderUse::units`).
    pub units: u8,
    /// Open now (stalls out, shutters up).
    pub open: bool,
    /// On a park's ground.
    pub park: bool,
    /// Columns at the bay's ends: where each stands and the way its deck runs.
    pub columns: Vec<(Vec2, Vec2, f32)>,
    pub seed: u64,
}

#[derive(Clone, Copy)]
struct Frame {
    p: Vec2,
    along: Vec2,
    across: Vec2,
}

impl Frame {
    fn at(&self, u: f32, v: f32) -> Vec2 {
        self.p + self.along * u + self.across * v
    }
    fn v3(&self, u: f32, v: f32, y: f32) -> Vec3 {
        let q = self.at(u, v);
        Vec3::new(q.x, y, q.y)
    }
    fn yaw(&self) -> f32 {
        self.along.y.atan2(self.along.x)
    }
    fn rect(&self, u0: f32, u1: f32, v0: f32, v1: f32) -> Vec<Vec2> {
        vec![self.at(u0, v0), self.at(u1, v0), self.at(u1, v1), self.at(u0, v1)]
    }
    fn up3(&self) -> Vec3 {
        Vec3::new(self.along.x, 0.0, self.along.y)
    }
    fn side3(&self) -> Vec3 {
        Vec3::new(self.across.x, 0.0, self.across.y)
    }
}

/// Sodium light under the decks after dark.
const SODIUM: [f32; 3] = [2.6, 1.25, 0.32];
/// Cold strip light in a lock-up, a court's floodlight.
const STRIP: [f32; 3] = [1.9, 2.1, 2.2];
/// A warm bulb over a stall.
const BULB: [f32; 3] = [2.8, 1.9, 0.9];

fn night(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], NIGHT_MARK]
}

/// The lamps hung under a bay's deck (the centre under each), for the
/// renderer's real lights: y over the bay's ground.
pub fn bay_lamps(b: &BayLook) -> Vec<Vec3> {
    let f = Frame { p: b.pos, along: b.dir.normalize_or_zero(), across: b.dir.normalize_or_zero().perp() };
    let n = lamp_count(b);
    let y = b.headroom - 0.32;
    (0..n).map(|k| f.v3(((k as f32 + 0.5) / n as f32 - 0.5) * b.len, 0.0, y)).collect()
}

fn lamp_count(b: &BayLook) -> usize {
    match b.use_ {
        UnderUse::Bare | UnderUse::Shelter => 1,
        UnderUse::Court => ((b.len / 7.0) as usize).clamp(2, 3),
        _ => ((b.len / 6.5) as usize).clamp(1, 3),
    }
}

/// The footprints of the raised decks over (or near) a lot's outline,
/// world quads: every raised piece of way within reach, its deck's width.
pub fn deck_quads(world: &nd_sim::World, poly: &[Vec2]) -> Vec<[Vec2; 4]> {
    if poly.len() < 3 {
        return Vec::new();
    }
    let (lo, hi) = nd_sim::geom::bounds(poly);
    let roads = &world.roads;
    roads
        .segments_in_box(lo - Vec2::splat(20.0), hi + Vec2::splat(20.0))
        .filter(|&s| {
            let sg = roads.segment(s);
            sg.alive && sg.viaduct && !sg.tunnel && roads.node(sg.a).level.max(roads.node(sg.b).level) >= nd_sim::roads::VIADUCT_LEVEL
        })
        .map(|s| {
            let sg = roads.segment(s);
            let (a, b) = roads.segment_points(s);
            let hw = nd_sim::world::deck_half_width(sg.kind, sg.lanes_fwd, sg.lanes_bwd) + 0.5;
            let n = (b - a).normalize_or_zero().perp() * hw;
            [a - n, b - n, b + n, a + n]
        })
        .collect()
}

/// A bay's whole scene.
pub fn bay(b: &BayLook, pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let mut rng = rng_for(b.seed, 0xB4E);
    let along = b.dir.try_normalize().unwrap_or(Vec2::X);
    let f = Frame { p: b.pos, along, across: along.perp() };
    let (hl, hw) = (b.len * 0.5, b.width * 0.5);
    let head = b.headroom.max(2.6);
    floor(&mut g, &f, hl, hw, b, pal, &mut rng);
    soffit(&mut g, &f, hl, b, head, pal);
    for &(at, dir, w) in &b.columns {
        column_dressing(&mut g, at, dir, w, head, pal, &mut rng);
    }
    match b.use_ {
        UnderUse::Market => market(&mut g, &f, hl, hw, b, head, pal, &mut rng),
        UnderUse::Hawkers => hawkers(&mut g, &f, hl, hw, b, head, pal, &mut rng),
        UnderUse::Workshop => workshops(&mut g, &f, hl, hw, b, head, pal, &mut rng),
        UnderUse::Court => court(&mut g, &f, hl, hw, b, head, pal),
        UnderUse::Bare => bare(&mut g, &f, hl, hw, pal, &mut rng),
        UnderUse::Shelter => {}
    }
    g
}

/// Worn concrete, oil stains, puddles under the drips, litter.
fn floor(g: &mut Geometry, f: &Frame, hl: f32, hw: f32, b: &BayLook, pal: &Palette, rng: &mut impl Rng) {
    let slab = if b.park { mul(pick(rng, &pal.concrete), 0.75) } else { mul(pick(rng, &pal.concrete), 0.62) };
    g.part(WornConcrete).ground_polygon(&f.rect(-hl, hl, -hw, hw), FLOOR_Y, rgba(slab));
    // A kerb of darker concrete round a bay in a park.
    if b.park {
        for s in [-1.0f32, 1.0] {
            g.part(Concrete).boxed(f.v3(0.0, s * (hw - 0.1), FLOOR_Y + 0.06), Vec3::new(b.len, 0.12, 0.2), f.yaw(), rgba(mul(slab, 0.8)));
        }
    }
    let y = FLOOR_Y + 0.004;
    // Oil, rust and the damp where the deck drips.
    for _ in 0..rng.random_range(3..7) {
        let (u, v) = (rng.random_range(-hl * 0.85..hl * 0.85), rng.random_range(-hw * 0.8..hw * 0.8));
        let r = rng.random_range(0.4..1.4);
        let blob: Vec<Vec2> = (0..7).map(|k| f.at(u, v) + Vec2::from_angle(k as f32 / 7.0 * std::f32::consts::TAU + rng.random_range(-0.3..0.3)) * r * rng.random_range(0.6..1.2)).collect();
        let c = if rng.random_bool(0.3) { mul(pal.rust_stain, 0.8) } else { [0.05, 0.05, 0.055] };
        g.part(Matte).ground_polygon(&blob, y, rgba(c));
    }
    // Puddles along the drip line under the deck's edge.
    for s in [-1.0f32, 1.0] {
        if rng.random_bool(0.6) {
            let u = rng.random_range(-hl * 0.7..hl * 0.7);
            let v = s * (hw - rng.random_range(0.4..1.2));
            let pud: Vec<Vec2> = (0..8).map(|k| f.at(u, v) + Vec2::from_angle(k as f32 / 8.0 * std::f32::consts::TAU) * Vec2::new(rng.random_range(1.0..2.4), rng.random_range(0.3..0.7)).rotate(f.along)).collect();
            g.part(Matte).ground_polygon(&pud, y + 0.002, rgba([0.035, 0.045, 0.06]));
        }
    }
    // Litter: cups, paper, cans.
    g.lod(Lod::Near, |g| {
        for _ in 0..rng.random_range(4..10) {
            let p = f.v3(rng.random_range(-hl..hl), rng.random_range(-hw..hw), FLOOR_Y + 0.03);
            let c = pick(rng, &[[0.8, 0.78, 0.7], [0.7, 0.15, 0.1], [0.2, 0.4, 0.7], [0.85, 0.85, 0.85]]);
            g.part(Matte).boxed(p, Vec3::new(rng.random_range(0.08..0.3), 0.05, rng.random_range(0.08..0.25)), rng.random_range(0.0..3.0), rgba(c));
        }
    });
}

/// Lamps hung from the soffit, a conduit along it.
fn soffit(g: &mut Geometry, f: &Frame, hl: f32, b: &BayLook, head: f32, pal: &Palette) {
    let y = head - 0.05;
    let steel = rgba(mul(pal.steel, 0.55));
    g.lod(Lod::Mid, |g| g.part(Metal).beam(f.v3(-hl - 1.5, 0.35, y - 0.04), f.v3(hl + 1.5, 0.35, y - 0.04), 0.08, steel));
    let n = lamp_count(b);
    let colour = match b.use_ {
        UnderUse::Court | UnderUse::Workshop => STRIP,
        _ => SODIUM,
    };
    for k in 0..n {
        let u = ((k as f32 + 0.5) / n as f32 - 0.5) * b.len;
        // A drop rod, the housing, and the lit lens under it.
        g.part(Metal).boxed(f.v3(u, 0.0, y - 0.1), Vec3::new(0.05, 0.2, 0.05), 0.0, steel);
        g.part(Matte).boxed(f.v3(u, 0.0, y - 0.26), Vec3::new(0.7, 0.14, 0.32), f.yaw(), rgba(mul(pal.steel, 0.4)));
        g.part(Glow).boxed(f.v3(u, 0.0, y - 0.345), Vec3::new(0.6, 0.03, 0.24), f.yaw(), night(colour));
    }
}

/// Posters, graffiti and drip stains on a column at the bay's end, on the
/// faces that look along the deck.
fn column_dressing(g: &mut Geometry, at: Vec2, dir: Vec2, w: f32, head: f32, pal: &Palette, rng: &mut impl Rng) {
    let d = dir.try_normalize().unwrap_or(Vec2::X);
    let side = d.perp();
    let up = Vec3::Y;
    for s in [-1.0f32, 1.0] {
        let face = at + d * s * 0.81;
        let out = Vec3::new(d.x * s, 0.0, d.y * s);
        let across = Vec3::new(side.x, 0.0, side.y);
        // Drip stains down from the crosshead.
        for _ in 0..rng.random_range(1..4) {
            let o = rng.random_range(-w * 0.4..w * 0.4);
            let h = rng.random_range(1.5..head.min(6.0));
            let c = face + side * o;
            g.part(Matte).decal(Vec3::new(c.x, head - h * 0.5, c.y) + out * 0.01, across, up, rng.random_range(0.15..0.45), h, out, rgba([0.12, 0.12, 0.11]));
        }
        // Fly-posters, layered and torn.
        g.lod(Lod::Mid, |g| {
            for k in 0..rng.random_range(1..4) {
                let o = rng.random_range(-w * 0.3..w * 0.3);
                let c = face + side * o;
                let col = pick(rng, &[[0.85, 0.2, 0.25], [0.95, 0.8, 0.2], [0.2, 0.6, 0.85], [0.9, 0.9, 0.85], [0.15, 0.15, 0.18]]);
                g.part(Matte).decal(Vec3::new(c.x, rng.random_range(1.2..2.2), c.y) + out * (0.015 + k as f32 * 0.004), across, up, rng.random_range(0.4..0.7), rng.random_range(0.55..0.9), out, rgba(mul(col, 0.8)));
            }
        });
        // A tag sprayed low down.
        if rng.random_bool(0.7) {
            let col = pick(rng, &[[0.1, 0.75, 0.55], [0.85, 0.15, 0.6], [0.95, 0.55, 0.1], [0.4, 0.3, 0.9], [0.9, 0.9, 0.9]]);
            let c = face + side * rng.random_range(-w * 0.2..w * 0.2);
            g.part(Matte).decal(Vec3::new(c.x, rng.random_range(0.5..0.9), c.y) + out * 0.03, across, up, (w * 0.7).min(1.4), rng.random_range(0.35..0.6), out, rgba(col));
        }
    }
    let _ = pal;
}

/// A market: rows of stalls under tarps.
#[allow(clippy::too_many_arguments)]
fn market(g: &mut Geometry, f: &Frame, hl: f32, hw: f32, b: &BayLook, head: f32, pal: &Palette, rng: &mut impl Rng) {
    let rows: &[f32] = if b.width >= 8.0 { &[-1.0, 1.0] } else { &[-1.0] };
    let n = b.units.max(2) as usize;
    let stall_w = (b.len / n as f32 * (if rows.len() == 2 { 2.0 } else { 1.0 })).clamp(2.2, 3.4) * 0.92;
    let per_row = ((b.len - 1.0) / (stall_w + 0.3)).floor().max(1.0) as usize;
    let depth = 1.7f32.min(hw * 0.8);
    let canopy = (head - 0.4).clamp(2.1, 2.6);
    let wood = rgba(mul(pal.plywood, 0.75));
    let post = rgba(mul(pal.steel, 0.45));
    for &row in rows {
        // Stalls back onto the deck's edge, facing the aisle down the middle.
        let back = row * (hw - 0.3);
        let front = row * (hw - 0.3 - depth);
        for k in 0..per_row {
            let u = (k as f32 + 0.5) / per_row as f32 * (b.len - 1.0) - (b.len - 1.0) * 0.5;
            let (u0, u1) = (u - stall_w * 0.5, u + stall_w * 0.5);
            // Four posts.
            for (pu, pv) in [(u0, back), (u1, back), (u0, front), (u1, front)] {
                g.part(Matte).boxed(f.v3(pu, pv, canopy * 0.5), Vec3::new(0.05, canopy, 0.05), 0.0, post);
            }
            // The tarp, sloping to the back, sagging in the middle.
            let tarp = rgba(mul(pick(rng, &pal.tarp_colors), rng.random_range(0.6..0.95)));
            let (yb, yf) = (canopy - 0.25, canopy);
            let sag = 0.12;
            let mid = (u0 + u1) * 0.5;
            let m = g.part(Tarp);
            m.quad([f.v3(u0, back, yb), f.v3(mid, back, yb - sag), f.v3(mid, front - row * 0.25, yf - sag), f.v3(u0, front - row * 0.25, yf)], tarp);
            m.quad([f.v3(mid, back, yb - sag), f.v3(u1, back, yb), f.v3(u1, front - row * 0.25, yf), f.v3(mid, front - row * 0.25, yf - sag)], tarp);
            // The counter along the front.
            let cv = front + row * 0.35;
            g.part(Wood).boxed(f.v3(mid, cv, 0.45), Vec3::new(stall_w - 0.2, 0.9, 0.7), f.yaw(), wood);
            if b.open {
                // Goods: crates of produce, folded clothes, a rack hung up.
                let kind = rng.random_range(0..4);
                let goods: Vec<[f32; 3]> = match kind {
                    0 => vec![[0.9, 0.45, 0.08], [0.55, 0.75, 0.2], [0.75, 0.1, 0.12], [0.95, 0.8, 0.2]],
                    1 => vec![[0.2, 0.25, 0.5], [0.8, 0.8, 0.78], [0.6, 0.1, 0.25], [0.15, 0.15, 0.15]],
                    2 => vec![[0.1, 0.1, 0.12], [0.85, 0.3, 0.6], [0.2, 0.7, 0.8], [0.9, 0.9, 0.9]],
                    _ => vec![[0.55, 0.4, 0.25], [0.7, 0.62, 0.45], [0.35, 0.3, 0.25], [0.8, 0.5, 0.2]],
                };
                let crates = ((stall_w - 0.4) / 0.55).floor().max(1.0) as usize;
                g.lod(Lod::Mid, |g| {
                    for c in 0..crates {
                        let cu = u0 + 0.35 + c as f32 * 0.55;
                        let col = goods[rng.random_range(0..goods.len())];
                        g.part(Matte).boxed(f.v3(cu, cv, 0.97), Vec3::new(0.48, 0.14, 0.5), f.yaw(), rgba(col));
                    }
                    if kind == 1 || kind == 2 {
                        // A rail of hanging things under the canopy.
                        let rail_y = canopy - 0.45;
                        g.part(Metal).beam(f.v3(u0 + 0.15, cv + row * 0.2, rail_y), f.v3(u1 - 0.15, cv + row * 0.2, rail_y), 0.03, post);
                        for h in 0..(stall_w / 0.35) as usize {
                            let hu = u0 + 0.25 + h as f32 * 0.35;
                            let col = goods[rng.random_range(0..goods.len())];
                            g.part(Matte).boxed(f.v3(hu, cv + row * 0.2, rail_y - 0.4), Vec3::new(0.25, 0.7, 0.04), f.yaw(), rgba(mul(col, 0.85)));
                        }
                    }
                    // Crates stacked behind.
                    for c in 0..rng.random_range(0..3) {
                        g.part(Matte).boxed(f.v3(u0 + 0.4 + c as f32 * 0.6, back - row * 0.35, 0.18 + 0.36 * (c % 2) as f32), Vec3::new(0.5, 0.34, 0.4), f.yaw() + 0.1, rgba(pick(rng, &[[0.2, 0.45, 0.7], [0.75, 0.2, 0.15], [0.25, 0.5, 0.25]])));
                    }
                });
                // A bulb under the canopy, a hand-lettered sign lit at night.
                g.part(Glow).boxed(f.v3(mid, cv - row * 0.3, canopy - 0.35), Vec3::new(0.1, 0.12, 0.1), 0.0, night(BULB));
                if rng.random_bool(0.6) {
                    let sign = mul(pick(rng, &pal.sign_colors), 1.6);
                    g.part(Glow).boxed(f.v3(mid, front - row * 0.27, canopy + 0.12), Vec3::new(stall_w * 0.6, 0.22, 0.03), f.yaw(), night(sign));
                }
            } else {
                // Sheeted over and roped down for the night.
                let sheet = rgba(mul(pick(rng, &pal.tarp_colors), 0.45));
                g.part(Tarp).boxed(f.v3(mid, cv, 0.98), Vec3::new(stall_w - 0.1, 0.18, 0.8), f.yaw(), sheet);
            }
        }
    }
    // Strings of bulbs down the aisle from column to column.
    let y = canopy + 0.15;
    if b.open {
        g.lod(Lod::Mid, |g| {
            let (a, c) = (f.v3(-hl, 0.0, y + 0.2), f.v3(hl, 0.0, y + 0.2));
            g.part(Matte).wire(a, c, 0.35, 0.015, rgba(DARK));
        });
        let n = (b.len / 1.4) as usize;
        for k in 0..n {
            let t = (k as f32 + 0.5) / n as f32;
            let sag = 0.35 * 4.0 * t * (1.0 - t);
            let p = f.v3(-hl + t * b.len, 0.0, y + 0.2 - sag - 0.05);
            g.part(Glow).boxed(p, Vec3::new(0.07, 0.09, 0.07), 0.0, night(if k % 3 == 0 { [2.6, 0.8, 0.5] } else { BULB }));
        }
        // A hand cart and a stack of crates at the end.
        cart(g, f, hl - 1.0, 0.0, pal, rng);
    }
}

fn cart(g: &mut Geometry, f: &Frame, u: f32, v: f32, pal: &Palette, rng: &mut impl Rng) {
    let yaw = f.yaw() + rng.random_range(-0.5..0.5);
    g.part(Wood).boxed(f.v3(u, v, 0.55), Vec3::new(1.3, 0.12, 0.75), yaw, rgba(mul(pal.plywood, 0.6)));
    for s in [-1.0f32, 1.0] {
        let w = f.at(u, v + s * 0.42);
        g.part(Matte).cylinder(w, 0.25, 0.25, 0.05, 0.55, 8, rgba(DARK));
    }
    g.part(Matte).boxed(f.v3(u, v, 0.8), Vec3::new(1.0, 0.4, 0.6), yaw, rgba(pick(rng, &[[0.2, 0.45, 0.7], [0.75, 0.2, 0.15], [0.6, 0.6, 0.55]])));
}

/// Hawker stalls: cart counters, woks, menu boxes, tables and stools.
#[allow(clippy::too_many_arguments)]
fn hawkers(g: &mut Geometry, f: &Frame, hl: f32, hw: f32, b: &BayLook, head: f32, pal: &Palette, rng: &mut impl Rng) {
    let n = b.units.max(1) as usize;
    let steel = rgba(mul(pal.steel, 0.85));
    let back = -(hw - 0.9);
    for k in 0..n {
        let u = (k as f32 + 0.5) / n as f32 * (b.len - 1.5) - (b.len - 1.5) * 0.5;
        // The counter: steel, on wheels, a glass front.
        g.part(Metal).boxed(f.v3(u, back, 0.5), Vec3::new(2.2, 1.0, 0.85), f.yaw(), steel);
        g.part(Matte).boxed(f.v3(u, back + 0.44, 0.75), Vec3::new(2.0, 0.4, 0.02), f.yaw(), rgba(mul(pal.glass_dark, 1.4)));
        // Burner and wok, a gas bottle beside.
        g.part(Matte).cylinder(f.at(u - 0.5, back), 0.28, 0.32, 1.0, 1.12, 10, rgba(DARK));
        g.part(Metal).cylinder(f.at(u + 1.35, back - 0.1), 0.17, 0.17, 0.0, 0.65, 8, rgba(pick(rng, &[[0.7, 0.12, 0.1], [0.15, 0.3, 0.65], [0.85, 0.65, 0.1]])));
        // A canopy over it, a menu light box under that.
        let top = (head - 0.4).clamp(2.1, 2.5);
        for s in [-1.0f32, 1.0] {
            g.part(Matte).boxed(f.v3(u + s * 1.05, back - 0.35, top * 0.5), Vec3::new(0.05, top, 0.05), 0.0, rgba(mul(pal.steel, 0.4)));
        }
        let awn = rgba(mul(pick(rng, &pal.awning_colors), 0.8));
        g.part(Tarp).quad([f.v3(u - 1.2, back - 0.5, top), f.v3(u + 1.2, back - 0.5, top), f.v3(u + 1.2, back + 0.9, top - 0.25), f.v3(u - 1.2, back + 0.9, top - 0.25)], awn);
        if b.open {
            let sign = mul(pick(rng, &pal.sign_colors), 1.8);
            g.part(Glow).boxed(f.v3(u, back + 0.5, top - 0.45), Vec3::new(1.8, 0.38, 0.06), f.yaw(), night(sign));
            g.part(Glow).boxed(f.v3(u, back, top - 0.12), Vec3::new(0.12, 0.1, 0.12), 0.0, night(BULB));
            // Steam off the wok.
            g.lod(Lod::Mid, |g| {
                for s in 0..3 {
                    let q = f.v3(u - 0.5 + rng.random_range(-0.1..0.1), back, 1.4 + s as f32 * 0.25);
                    g.part(Matte).boxed(q, Vec3::new(0.3 - s as f32 * 0.05, 0.2, 0.3 - s as f32 * 0.05), rng.random_range(0.0..3.0), rgba(pal.steam));
                }
            });
        } else {
            // Shuttered: a steel lid down over the counter.
            g.part(Metal).boxed(f.v3(u, back, 1.05), Vec3::new(2.2, 0.08, 0.9), f.yaw(), rgba(mul(pal.steel, 0.6)));
        }
    }
    // Tables and stools in the aisle.
    let tables = ((b.len - 2.0) / 2.6).floor().max(1.0) as usize;
    let tv = (back + 1.8).min(hw - 0.8);
    let table = rgba(pick(rng, &[[0.85, 0.85, 0.82], [0.2, 0.5, 0.35], [0.75, 0.2, 0.15]]));
    for k in 0..tables {
        let u = (k as f32 + 0.5) / tables as f32 * (b.len - 2.0) - (b.len - 2.0) * 0.5;
        if !b.open && k % 2 == 1 {
            // Stacked away for the night.
            for s in 0..5 {
                g.part(Matte).cylinder(f.at(u, tv), 0.17, 0.2, s as f32 * 0.09, s as f32 * 0.09 + 0.08, 8, rgba(pal.stool));
            }
            continue;
        }
        g.part(Matte).boxed(f.v3(u, tv, 0.72), Vec3::new(0.85, 0.04, 0.85), f.yaw() + rng.random_range(-0.15..0.15), table);
        g.part(Matte).boxed(f.v3(u, tv, 0.36), Vec3::new(0.06, 0.72, 0.06), 0.0, rgba(mul(pal.steel, 0.4)));
        g.lod(Lod::Mid, |g| {
            for s in 0..4 {
                let a = s as f32 * std::f32::consts::FRAC_PI_2 + rng.random_range(-0.3..0.3);
                let q = f.at(u, tv) + Vec2::from_angle(a) * 0.68;
                g.part(Matte).cylinder(q, 0.16, 0.18, 0.0, 0.45, 8, rgba(pal.stool));
            }
        });
    }
    // Bin bags and a crate of empties by the columns.
    g.lod(Lod::Mid, |g| {
        for s in [-1.0f32, 1.0] {
            for k in 0..rng.random_range(1..4) {
                let q = f.at(s * (hl - 0.6) + rng.random_range(-0.3..0.3), -hw + 0.6 + k as f32 * 0.5);
                g.part(Matte).cylinder(q, 0.3, 0.2, 0.0, 0.55, 7, rgba([0.06, 0.06, 0.07]));
            }
        }
    });
}

/// Lock-up workshops between the columns.
#[allow(clippy::too_many_arguments)]
fn workshops(g: &mut Geometry, f: &Frame, hl: f32, hw: f32, b: &BayLook, head: f32, pal: &Palette, rng: &mut impl Rng) {
    let n = b.units.max(1) as usize;
    let h = (head - 0.45).clamp(2.4, 3.3);
    let deep = (hw * 0.95).clamp(2.6, 5.5);
    let unit = b.len / n as f32;
    let back = -hw + 0.1;
    let front = back + deep;
    for k in 0..n {
        let u = -hl + unit * (k as f32 + 0.5);
        let sheet = mul(pick(rng, &pal.metal_paint), rng.random_range(0.55..0.85));
        let w = unit - 0.15;
        // The box: back and sides of corrugated sheet, a roof.
        g.part(WornMetal).boxed(f.v3(u, back + 0.05, h * 0.5), Vec3::new(w, h, 0.1), f.yaw(), rgba(sheet));
        for s in [-1.0f32, 1.0] {
            g.part(WornMetal).boxed(f.v3(u + s * w * 0.5, (back + front) * 0.5, h * 0.5), Vec3::new(0.08, h, deep), f.yaw(), rgba(mul(sheet, 0.92)));
        }
        g.part(WornMetal).boxed(f.v3(u, (back + front) * 0.5, h + 0.04), Vec3::new(w + 0.1, 0.08, deep + 0.1), f.yaw(), rgba(mul(sheet, 0.8)));
        // A hand-painted sign over the door.
        let sign = mul(pick(rng, &pal.sign_colors), 0.9);
        g.part(Panel).decal(f.v3(u, front + 0.02, h - 0.3), f.up3(), Vec3::Y, w * 0.7, 0.4, f.side3(), rgba(sign));
        let door_h = h - 0.6;
        if b.open {
            // Shutter rolled up: the dark inside, a strip light, a bench.
            g.part(Matte).decal(f.v3(u, front - 0.02, door_h * 0.5), f.up3(), Vec3::Y, w - 0.3, door_h, f.side3(), rgba([0.05, 0.05, 0.055]));
            g.part(Metal).boxed(f.v3(u, front - 0.05, door_h + 0.12), Vec3::new(w - 0.25, 0.24, 0.24), f.yaw(), rgba(mul(sheet, 0.7)));
            g.part(Glow).boxed(f.v3(u, (back + front) * 0.5, h - 0.15), Vec3::new(1.2, 0.04, 0.08), f.yaw(), [STRIP[0], STRIP[1], STRIP[2], 1.0]);
            g.part(Wood).boxed(f.v3(u - w * 0.25, back + 0.5, 0.45), Vec3::new(w * 0.4, 0.9, 0.7), f.yaw(), rgba(mul(pal.plywood, 0.55)));
            // Out front: a scooter, or a car up on blocks; tyres and drums.
            g.lod(Lod::Mid, |g| {
                let fu = u + rng.random_range(-0.4..0.4);
                if rng.random_bool(0.4) && hw > 4.0 {
                    let body = mul(pick(rng, &pal.car_colors), 0.7);
                    let cv = front + 1.3;
                    g.part(Panel).boxed(f.v3(fu, cv, 0.85), Vec3::new(1.8, 0.7, 4.0), f.yaw(), rgba(body));
                    g.part(Panel).boxed(f.v3(fu, cv - 0.2, 1.4), Vec3::new(1.6, 0.5, 2.0), f.yaw(), rgba(mul(body, 0.85)));
                    for (du, dv) in [(-0.7, -1.4), (0.7, -1.4), (-0.7, 1.4), (0.7, 1.4)] {
                        g.part(Concrete).boxed(f.v3(fu + du, cv + dv, 0.25), Vec3::new(0.35, 0.5, 0.35), f.yaw(), rgba(mul(pal.concrete[0], 0.7)));
                    }
                } else {
                    let body = pick(rng, &pal.car_colors);
                    let cv = front + 0.8;
                    g.part(Panel).boxed(f.v3(fu, cv, 0.55), Vec3::new(1.6, 0.45, 0.35), f.yaw(), rgba(body));
                    for s in [-1.0f32, 1.0] {
                        g.part(Matte).pipe(f.at(fu + s * 0.65, cv), f.across, 0.1, 0.3, 0.3, 8, rgba(DARK));
                    }
                }
                for t in 0..rng.random_range(2..5) {
                    g.part(Matte).cylinder(f.at(u + w * 0.4, front + 0.4), 0.33, 0.33, t as f32 * 0.22, t as f32 * 0.22 + 0.2, 10, rgba([0.04, 0.04, 0.045]));
                }
                g.part(Rust).cylinder(f.at(u - w * 0.4, front + 0.4), 0.3, 0.3, 0.0, 0.88, 8, rgba(mix(pal.rust, [0.15, 0.3, 0.5], 0.3)));
            });
            g.part(Glow).boxed(f.v3(u + w * 0.3, front + 0.1, door_h + 0.35), Vec3::new(0.25, 0.18, 0.12), f.yaw(), night(BULB));
        } else {
            // Shutter down, padlocked.
            g.part(Metal).decal(f.v3(u, front + 0.01, door_h * 0.5), f.up3(), Vec3::Y, w - 0.3, door_h, f.side3(), rgba(mul(sheet, 0.75)));
        }
    }
}

/// A painted ball court with a hoop each end, fenced along its sides.
fn court(g: &mut Geometry, f: &Frame, hl: f32, hw: f32, b: &BayLook, head: f32, pal: &Palette) {
    let (cl, cw) = (hl - 0.6, hw - 0.7);
    let y = FLOOR_Y + 0.006;
    g.part(Matte).ground_polygon(&f.rect(-cl, cl, -cw, cw), y, rgba([0.2, 0.32, 0.36]));
    let line = rgba([0.85, 0.85, 0.8]);
    let lw = 0.07;
    let m = g.part(Paint);
    for (u0, u1, v0, v1) in [(-cl, cl, -cw, -cw + lw), (-cl, cl, cw - lw, cw), (-cl, -cl + lw, -cw, cw), (cl - lw, cl, -cw, cw), (-lw * 0.5, lw * 0.5, -cw, cw)] {
        m.ground_polygon(&f.rect(u0, u1, v0, v1), y + 0.002, line);
    }
    // The keys under the hoops.
    for s in [-1.0f32, 1.0] {
        let u0 = s * cl;
        let u1 = s * (cl - 4.0f32.min(cl * 0.4));
        let (a, c) = (u0.min(u1), u0.max(u1));
        m.ground_polygon(&f.rect(a, c, -1.8f32.min(cw), 1.8f32.min(cw)), y + 0.001, rgba([0.5, 0.18, 0.12]));
    }
    // Hoops: a post, the backboard, the rim.
    let rim = 3.05f32.min(head - 0.6);
    for s in [-1.0f32, 1.0] {
        let u = s * (cl + 0.2);
        g.part(Matte).boxed(f.v3(u, 0.0, (rim + 0.6) * 0.5), Vec3::new(0.14, rim + 0.6, 0.14), 0.0, rgba(mul(pal.steel, 0.5)));
        g.part(Matte).boxed(f.v3(u - s * 0.6, 0.0, rim + 0.15), Vec3::new(1.1, 0.12, 0.12), f.yaw(), rgba(mul(pal.steel, 0.5)));
        g.part(Panel).boxed(f.v3(u - s * 1.15, 0.0, rim + 0.35), Vec3::new(0.05, 1.05, 1.8), f.yaw(), rgba([0.85, 0.85, 0.82]));
        g.part(Metal).pipe(f.at(u - s * 1.45, 0.0), f.along, 0.45, 0.02, rim, 10, rgba([0.9, 0.35, 0.1]));
    }
    // Fences along the long sides, under the deck's edges.
    let fh = (head - 0.5).min(3.0);
    for s in [-1.0f32, 1.0] {
        chain_link(g, f, -hl, hl, s * (hw - 0.15), fh, pal);
    }
    let _ = b;
}

/// A run of chain-link from `u0` to `u1` at `v`, `h` tall: posts, a top
/// and a bottom rail, and the mesh as a few thin wires (a sheet read as a
/// solid wall from under the deck).
#[allow(clippy::too_many_arguments)]
fn chain_link(g: &mut Geometry, f: &Frame, u0: f32, u1: f32, v: f32, h: f32, pal: &Palette) {
    let steel = rgba(mul(pal.steel, 0.55));
    let posts = ((u1 - u0) / 2.5).ceil().max(1.0) as usize;
    for k in 0..=posts {
        let u = u0 + (u1 - u0) * k as f32 / posts as f32;
        g.part(Metal).boxed(f.v3(u, v, h * 0.5), Vec3::new(0.06, h, 0.06), 0.0, steel);
    }
    for y in [0.15, h - 0.05] {
        g.part(Metal).beam(f.v3(u0, v, y), f.v3(u1, v, y), 0.045, steel);
    }
    g.lod(Lod::Mid, |g| {
        let wire = rgba(mul(pal.fence_mesh, 0.9));
        let n = (h / 0.35) as usize;
        for k in 1..n {
            let y = 0.15 + (h - 0.2) * k as f32 / n as f32;
            g.part(Matte).beam(f.v3(u0, v, y), f.v3(u1, v, y), 0.012, wire);
        }
    });
}

/// Bare ground: gravel, dumped things, a run of fence.
fn bare(g: &mut Geometry, f: &Frame, hl: f32, hw: f32, pal: &Palette, rng: &mut impl Rng) {
    for _ in 0..rng.random_range(2..4) {
        let (u, v) = (rng.random_range(-hl * 0.7..hl * 0.7), rng.random_range(-hw * 0.6..hw * 0.6));
        let r = rng.random_range(1.2..2.8);
        let patch: Vec<Vec2> = (0..9).map(|k| f.at(u, v) + Vec2::from_angle(k as f32 / 9.0 * std::f32::consts::TAU) * r * rng.random_range(0.7..1.15)).collect();
        g.part(Dirt).ground_polygon(&patch, FLOOR_Y + 0.008, rgba(mul(pal.gravel, 0.75)));
    }
    // Drums, a pallet stack, maybe a mattress or a burnt-out cart.
    for _ in 0..rng.random_range(1..4) {
        let q = f.at(rng.random_range(-hl * 0.8..hl * 0.8), rng.random_range(-hw * 0.7..hw * 0.7));
        g.part(Rust).cylinder(q, 0.3, 0.3, FLOOR_Y, 0.9, 8, rgba(mul(mix(pal.rust, pick(rng, &[[0.15, 0.3, 0.5], [0.5, 0.15, 0.1], [0.2, 0.35, 0.2]]), 0.35), 0.8)));
    }
    let p = f.at(rng.random_range(-hl * 0.6..hl * 0.6), -hw * 0.6);
    for k in 0..rng.random_range(2..6) {
        g.part(Wood).boxed(Vec3::new(p.x, 0.08 + k as f32 * 0.15, p.y), Vec3::new(1.2, 0.13, 1.0), f.yaw() + k as f32 * 0.05, rgba(mul(pal.plywood, 0.5)));
    }
    if rng.random_bool(0.5) {
        let q = f.at(rng.random_range(-hl * 0.5..hl * 0.5), hw * 0.4);
        g.part(Matte).boxed(Vec3::new(q.x, 0.14, q.y), Vec3::new(1.9, 0.18, 0.9), f.yaw() + rng.random_range(-0.6..0.6), rgba([0.42, 0.38, 0.32]));
    }
    // A run of chain-link along one edge.
    if rng.random_bool(0.6) {
        let s = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let (u0, u1) = (-hl * rng.random_range(0.3..0.9), hl * rng.random_range(0.3..0.9));
        chain_link(g, f, u0, u1, s * (hw - 0.2), 2.2, pal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(u: UnderUse, headroom: f32, width: f32, open: bool) -> BayLook {
        BayLook { pos: Vec2::new(10.0, -4.0), dir: Vec2::new(0.6, 0.8), len: 18.0, width, headroom, use_: u, units: u.units(18.0), open, park: false, columns: vec![(Vec2::new(10.0, -4.0) - Vec2::new(0.6, 0.8) * 11.0, Vec2::new(0.6, 0.8), 2.0)], seed: 7 }
    }

    /// [r5-layered] Nothing in a bay stands taller than the headroom under
    /// its deck, open or shut, on a low deck and a high one, a narrow walkway
    /// and a wide street; and the lamps hang just under the soffit.
    #[test]
    fn every_use_fits_under_its_deck() {
        let pal = Palette::default();
        for u in UnderUse::ALL {
            for headroom in [3.0f32, 4.6, 9.0] {
                if u == UnderUse::Court && headroom < 4.5 {
                    continue;
                }
                for width in [4.4f32, 11.2] {
                    for open in [true, false] {
                        let b = look(u, headroom, width, open);
                        let g = bay(&b, &pal);
                        let top = g.parts.iter().chain(g.detail.iter().flatten()).flat_map(|p| p.positions.iter()).map(|p| p[1]).fold(0.0f32, f32::max);
                        assert!(top <= headroom.max(2.6) + 1e-3, "{u:?} at {headroom} m headroom, {width} m wide: up to {top} m");
                        assert!(g.triangle_count() > 0);
                        for l in bay_lamps(&b) {
                            assert!(l.y < headroom && l.y > headroom - 1.0);
                        }
                    }
                }
            }
        }
    }
}
