//! [capsule] The Capsule district's architecture: retro-futurist white
//! domes. Dome houses with a band of round windows, saucers on legs, domes
//! on a single thick stilt like a lolly, clusters joined by glass tubes,
//! pill towers, stacks of saucers round a core, dome halls under curved
//! glass, round office drums with a radar ball, and stilt towers with a
//! big pod on top. Warm white shells, thin pastel stripes (lit softly after
//! dark), round doors and portholes, lawns, round planters and ball trees.
//!
//! Every name and mark here is the city's own: the emblems are plain rings
//! and dots, no lettering.
//!
//! Each style follows the building's floors (`Ctx::height`): a pill tower
//! of thirty floors is thirty rows of portholes; the low styles add drum
//! storeys under their dome. Footprints are circles packed into the
//! buildable outline (`kit::pack_circles`), so a triangle or an L gets its
//! domes where it is widest.

use super::kit::*;
use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::LandUse;
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Warm off-white shells.
const WHITES: [[f32; 3]; 4] = [[0.93, 0.91, 0.86], [0.95, 0.94, 0.90], [0.91, 0.90, 0.87], [0.94, 0.90, 0.83]];
/// The thin stripes: soft yellow, orange, teal, red, sky blue, mint.
const ACCENTS: [[f32; 3]; 6] = [[0.96, 0.76, 0.24], [0.95, 0.50, 0.20], [0.22, 0.66, 0.68], [0.85, 0.26, 0.24], [0.32, 0.55, 0.86], [0.45, 0.80, 0.62]];
/// The coloured crowns of the domes: red, pink, orange, blue, green,
/// yellow, teal.
const CAPS: [[f32; 3]; 7] = [[0.72, 0.08, 0.09], [0.82, 0.22, 0.42], [0.92, 0.36, 0.06], [0.14, 0.28, 0.70], [0.16, 0.52, 0.24], [0.93, 0.68, 0.08], [0.06, 0.48, 0.54]];
/// Pastel shells for the odd dome that is not white.
const PASTELS: [[f32; 3]; 5] = [[0.95, 0.82, 0.42], [0.64, 0.74, 0.90], [0.92, 0.70, 0.76], [0.96, 0.77, 0.58], [0.72, 0.87, 0.77]];
/// Pale tinted glass for tubes, canopies and lobbies.
const GLASS: [f32; 3] = [0.42, 0.60, 0.68];
/// Porthole rims and fittings.
const RIM: [f32; 3] = [0.66, 0.68, 0.72];

/// One building's colours.
#[derive(Clone, Copy)]
struct Paint {
    wall: [f32; 3],
    /// The coloured crown cap of a dome.
    cap: [f32; 3],
    /// Window shape: 1 a round porthole, more a slot that many times as
    /// wide as high (round ended).
    slot: f32,
    accent: [f32; 3],
    accent2: [f32; 3],
    lawn: [f32; 3],
    leaf: [f32; 3],
    bark: [f32; 3],
}

fn paint(c: &Ctx, rng: &mut impl Rng) -> Paint {
    paint_slots(c, rng, 0.45)
}

/// [`paint`] with the chance of slot windows (bands of round-ended slots,
/// as towers have) instead of portholes.
fn paint_slots(c: &Ctx, rng: &mut impl Rng, slots: f32) -> Paint {
    // Mostly white; one in four a pastel shell.
    let wall = if coin(rng, 0.25) { PASTELS[rng.random_range(0..PASTELS.len())] } else { WHITES[rng.random_range(0..WHITES.len())] };
    let cap = CAPS[rng.random_range(0..CAPS.len())];
    let slot = if coin(rng, slots) { 2.6 } else { 1.0 };
    let i = rng.random_range(0..ACCENTS.len());
    let j = (i + rng.random_range(1..ACCENTS.len())) % ACCENTS.len();
    let foliage = c.palette.foliage;
    Paint {
        wall,
        cap,
        slot,
        accent: ACCENTS[i],
        accent2: ACCENTS[j],
        lawn: mix(foliage, [0.34, 0.56, 0.26], 0.55),
        leaf: mix(foliage, [0.30, 0.58, 0.26], 0.5),
        bark: mix(c.palette.bark, [0.42, 0.32, 0.24], 0.5),
    }
}

/// A window's half-width and half-height for half-height `pr`.
fn win<P: std::borrow::Borrow<Paint>>(pt: P, pr: f32) -> Vec2 {
    let pt = pt.borrow();
    Vec2::new(pr * pt.slot, if pt.slot > 1.0 { pr * 0.8 } else { pr })
}

/// A shell turned from `prof` (bottom to crown): the last `crown` points
/// (from the ring where the cap starts) in the cap's colour, the rest in
/// the wall's.
fn shell(g: &mut Geometry, c: &Ctx, p: Vec2, prof: &[(f32, f32)], crown: usize, sides: usize, pt: &Paint) {
    let f = &c.frame;
    let k = prof.len().saturating_sub(crown.max(2));
    turn(g, f, Matte, p, &prof[..=k], sides, pt.wall);
    // Smooth painted shells (Matte): clean and high tech, as drawn in the anime.
    turn(g, f, Matte, p, &prof[k..], sides, pt.cap);
}

/// The direction from `p` a door faces: the street (lot-local -v) if the
/// pod fits there, else the nearer side, else the back.
fn door_dirs() -> [Vec2; 4] {
    [Vec2::NEG_Y, Vec2::X, Vec2::NEG_X, Vec2::Y]
}

/// An entrance pod out of a drum towards the street (or wherever it fits),
/// and the angle (with its half-width) the portholes leave for it.
#[allow(clippy::too_many_arguments)]
fn door(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, pod_r: f32, pt: &Paint) -> Option<((f32, f32), Vec2)> {
    for d in door_dirs() {
        if let Some(end) = entry_pod(g, c, rng, p, r, d, pod_r, pt.wall, pt.accent) {
            return Some(((d.y.atan2(d.x), (pod_r + 0.7) / r.max(1.0)), end));
        }
    }
    None
}

/// A drum of radius `r` at lot-local `p`, `wall` high from `y0` (a raised
/// drum gets an underside), under a dome `dome_h` high: porthole rows on
/// the drum one a floor, a stripe at the springing with a line of light
/// under it, a row of smaller portholes on the dome when it is tall
/// enough. `skip` keeps portholes off doors and tubes. Returns the crown.
#[allow(clippy::too_many_arguments)]
fn drum_dome(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, y0: f32, wall: f32, dome_h: f32, pt: &Paint, skip: &[(f32, f32)]) -> f32 {
    let f = &c.frame;
    let fh = c.fh();
    let mut prof: Vec<(f32, f32)> = Vec::new();
    if y0 > 0.05 {
        prof.push((0.0, y0));
    }
    prof.push((r, y0));
    prof.extend(dome_profile(r, y0 + wall, dome_h, 9));
    shell(g, c, p, &prof, 5, sides_for(r), pt);
    let pr = (0.4 + r * 0.015).min(0.58);
    let spacing = porthole_spacing(r);
    let top = y0 + wall;
    let mut k = 0;
    loop {
        let y = y0 + k as f32 * fh + fh.min(3.4) * 0.55;
        if y + pr * 1.3 > top - 0.55 {
            break;
        }
        porthole_row(g, c, rng, p, r, y, win(pt, pr), spacing, if k == 0 { skip } else { &[] }, RIM);
        k += 1;
        if k > 400 {
            break;
        }
    }
    drum_stripe(g, f, Matte, p, r, top - 0.36, 0.3, pt.accent);
    night_ring(g, f, p, r, top - 0.47, 0.08, mul(pt.accent, 1.2));
    if dome_h > 3.2 && r > 3.0 {
        porthole_row_ball(g, c, rng, p, r, dome_h, top, 0.42, win(pt, pr * 0.85), spacing * 1.15, &[], RIM);
    }
    top + dome_h
}

/// An antenna on a crown: a mast, a little ball, and a red light when it
/// stands high.
fn antenna(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, h: f32, ball: [f32; 3]) {
    let f = &c.frame;
    turn(g, f, Matte, p, &[(0.09, y - 0.2), (0.05, y + h)], 6, c.palette.steel);
    turn(g, f, Matte, p, &ball_profile(0.28, 0.28, y + h * 0.55, 4), 8, ball);
    if y + h > 24.0 {
        aviation_light_local(g, f, c.palette, p.x, y + h + 0.12, p.y, 0.25);
    }
}

/// A round helideck on a short mast over a crown at `y` (the sim gave the
/// tower a pad): a white saucer underside, the pad with its rim lights.
/// The mast is `mast` high: clear of the antenna or radar ball the crown
/// carries without a pad.
fn sky_pad(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, mast: f32, room: f32, pt: &Paint) {
    let f = &c.frame;
    let r = (room * 0.85).clamp(3.0, 6.5);
    let deck = y + mast.max(2.6);
    turn(g, f, Matte, p, &[(0.7, y - 0.5), (0.6, deck - 0.6)], 10, pt.wall);
    turn(g, f, Matte, p, &[(0.6, deck - 1.2), (r * 0.9, deck - 0.45), (r, deck - 0.3)], sides_for(r), pt.wall);
    helipad(g, c, p, deck - 0.3, r);
}

/// A round sign for a shop or office: a white disc on a post facing the
/// street with a lit ring and a dot (no lettering), dimmed by day.
fn emblem(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, near: Vec2) {
    let f = &c.frame;
    let Some(p) = [near + Vec2::new(3.2, -1.0), near + Vec2::new(-3.2, -1.0), near + Vec2::new(2.5, 1.5)].into_iter().find(|q| c.inside_local(*q) && distance_to_boundary(*q, &c.lot_poly) > 1.4) else { return };
    let h = 3.2;
    turn(g, f, Matte, p, &[(0.1, 0.0), (0.1, h)], 6, RIM);
    let n = f.dir3(Vec2::NEG_Y);
    let centre = f.at2(p, h + 0.9) + n * 0.12;
    let t1 = f.dir3(Vec2::X);
    let disc = |rad: f32, off: f32, k: usize| -> Vec<Vec3> { (0..k).map(|i| centre + n * off + (t1 * (i as f32 / k as f32 * TAU).cos() + Vec3::Y * (i as f32 / k as f32 * TAU).sin()) * rad).collect() };
    // The disc is a slab: a white face, a back and a rim.
    let face = disc(0.95, 0.0, 16);
    let back = disc(0.95, -0.16, 16);
    g.part(Matte).frustum(&back, &face, rgba(WHITES[1]));
    let col = pick(rng, &c.palette.sign_colors);
    let from = g.get(Glow).colors.len();
    // A ring (as a band of quads) and an off-centre dot.
    let (ro, ri) = (disc(0.8, 0.03, 16), disc(0.62, 0.03, 16));
    for i in 0..16 {
        let j = (i + 1) % 16;
        g.part(Glow).quad_facing([ri[i], ro[i], ro[j], ri[j]], n, rgba(col));
    }
    let dot_c = centre + t1 * 0.18 + Vec3::Y * 0.12;
    let dot: Vec<Vec3> = (0..8).map(|i| dot_c + n * 0.035 + (t1 * (i as f32 / 8.0 * TAU).cos() + Vec3::Y * (i as f32 / 8.0 * TAU).sin()) * 0.2).collect();
    g.part(Glow).cap(&dot, n, rgba(mul(col, 0.9)));
    mark_glow(g, from, NEON_MARK);
}

/// A small hover car resting on a pad: a white pill with a glass bubble.
fn hover_car(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, yaw: f32, body: [f32; 3]) {
    let f = &c.frame;
    let d = Vec2::from_angle(yaw);
    let ax = Axis::level(f, p - d * 2.1, p + d * 2.1, y + 0.75);
    revolve(g.part(Matte), &ax, &ball_profile(0.85, 2.1, 2.1, 8), 12, None, rgba(body));
    turn(g, f, Glass, p - d * 0.2, &ball_profile(0.75, 0.55, y + 1.45, 5), 12, GLASS);
    turn(g, f, Matte, p, &[(0.5, y), (0.5, y + 0.2)], 8, RIM);
}

/// The lot's grounds: a lawn, a pale path from each door to the street,
/// globe lamps along it, then round planters, ball trees and palms where
/// there is room outside `keep` (circles the buildings stand in).
fn grounds(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, keep: &[(Vec2, f32)], doors: &[Vec2], pt: &Paint, trees: usize) {
    let f = &c.frame;
    let lawn = offset_poly(&c.lot_poly, 0.3);
    ground(g, f, Foliage, &lawn, 0.03, pt.lawn);
    // Clipped hedges down the sides and back (the street side stays open).
    if coin(rng, 0.5) {
        let edge = offset_poly(&c.lot_poly, 0.7);
        let front = c.front_face_index(&edge);
        for (i, face) in c.faces(&edge).iter().enumerate() {
            if Some(i) != front && face.len > 2.0 && face.out.y > -0.5 {
                hedge(g, c, face.a, face.b, 1.1, 0.8, mul(pt.leaf, 0.85));
            }
        }
    }
    let pave = [0.80, 0.78, 0.73];
    let mut paths: Vec<(Vec2, Vec2)> = Vec::new();
    for d in doors {
        // Straight to the street edge, as far as the lot goes.
        let mut v = d.y;
        while c.inside_local(Vec2::new(d.x, v - 0.5)) && v > d.y - 60.0 {
            v -= 0.5;
        }
        let a = Vec2::new(d.x, d.y + 0.4);
        let b = Vec2::new(d.x, v + 0.3);
        let len = a.y - b.y;
        if len > 0.8 {
            f.flat(g, Paving, d.x, 0.05, (a.y + b.y) * 0.5, 1.8, len, 0.0, pave);
            paths.push((a, b));
            for k in 1..=((len / 7.0) as i32).min(4) {
                let q = Vec2::new(d.x + 1.5, a.y - k as f32 * 7.0 + 2.0);
                if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.5 {
                    globe_lamp(g, c, q, 2.6);
                }
            }
        }
    }
    let near_path = |q: Vec2, r: f32| paths.iter().any(|(a, b)| (q.x - a.x).abs() < r + 1.2 && q.y < a.y + r && q.y > b.y - r);
    let mut placed: Vec<(Vec2, f32)> = Vec::new();
    let mut tries = 0;
    while placed.len() < trees && tries < trees * 6 {
        tries += 1;
        let kind = rng.random_range(0..10);
        let r = match kind {
            0..=4 => rng.random_range(1.6..2.6),
            // A palm's fronds and lean reach about 4 m out.
            5..=7 => 3.9,
            _ => rng.random_range(0.7..1.1),
        };
        let Some(q) = c.random_inside(rng, r + 0.4) else { break };
        if keep.iter().any(|(k, kr)| q.distance(*k) < kr + r + 0.6) || placed.iter().any(|(k, kr)| q.distance(*k) < kr + r + 0.4) || near_path(q, r) {
            continue;
        }
        placed.push((q, r));
        match kind {
            0..=4 => ball_tree(g, f, q, rng.random_range(3.6..5.8), r, pt.bark, mul(pt.leaf, rng.random_range(0.85..1.1))),
            5..=7 => tree_kind(g, f.to_world(q.x, q.y), 0.0, TreeKind::Palm, rng.random_range(5.0..7.5), 2.0, pt.bark, pt.leaf),
            _ => planter(g, f, q, r, pt.wall, pt.leaf),
        }
    }
}

/// Floors of a storey count: the building's own.
fn floors(c: &Ctx) -> usize {
    c.floors.max(1) as usize
}

/// Floors for the low styles (houses, saucers, pods): six at most, as the
/// villas keep to two; the site fit (`adjacency::site_fit`) leaves taller
/// sites to the towers.
const LOW_MAX: usize = 6;

fn low_floors(c: &Ctx) -> usize {
    floors(c).min(LOW_MAX)
}

/// Whether the land use is a shop, office or night out (gets an emblem).
fn trades(c: &Ctx) -> bool {
    matches!(c.land_use, LandUse::Commercial | LandUse::Entertainment)
}

fn emblem_maybe(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, near: Option<Vec2>) {
    if let Some(d) = near {
        if trades(c) && coin(rng, c.rules.sign_chance.max(0.35)) {
            emblem(g, c, rng, d);
        }
    }
}

/// Fallback when the lot holds no circle worth a dome: a small pod on the
/// lawn, or nothing on a sliver.
fn tiny(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, pt: &Paint) {
    let pk = pack_circles(&c.poly, &[4.0], 1.2, 0.5);
    if let Some(&(p, r)) = pk.first() {
        drum_dome(g, c, rng, p, r, 0.0, 2.6 + (floors(c) as f32 - 1.0).min(3.0) * c.fh(), r * 0.8, pt, &[]);
    }
    grounds(g, c, rng, &pk, &[], pt, 2);
}

// ── dome house ───────────────────────────────────────────────────────────────

/// A dome on a drum with a band of portholes and a round front door, a
/// smaller garage dome or a landing pad beside it, on a lawn.
pub fn dome_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let pk = pack_circles(&c.poly, &[9.5, 5.5, 4.0], 3.2, 2.5);
    let Some(&(p, r)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let fh = c.fh();
    let n = low_floors(c);
    let wall = (n.max(2) - 2) as f32 * fh + 3.2;
    let dome_h = (r * 0.72).max(fh * 1.1);
    let d = door(g, c, rng, p, r, 1.25, &pt);
    let skip: Vec<(f32, f32)> = d.iter().map(|(s, _)| *s).collect();
    let crown = drum_dome(g, c, rng, p, r, 0.0, wall, dome_h, &pt, &skip);
    // A skylight or an antenna on the crown.
    if coin(rng, 0.5) {
        turn(g, &c.frame, Glass, p, &dome_profile(r * 0.22, crown - dome_h * 0.06, r * 0.12, 3), 12, GLASS);
    } else {
        antenna(g, c, p + Vec2::new(r * 0.15, 0.0), crown - 0.3, 2.4, pt.accent);
    }
    let mut keep = vec![(p, r + 1.0)];
    if let Some(&(q, rq)) = pk.get(1) {
        if coin(rng, 0.55) && rq >= 3.5 {
            // A garage dome with its own stripe.
            let rr = rq.min(4.5);
            let pt2 = Paint { accent: pt.accent2, ..pt };
            drum_dome(g, c, rng, q, rr, 0.0, 2.6, rr * 0.75, &pt2, &[]);
            keep.push((q, rr + 0.5));
        } else if rq >= 3.0 {
            let rr = rq.min(5.0);
            landing_pad(g, c, q, rr, WHITES[1], pt.accent2);
            if coin(rng, 0.6) {
                hover_car(g, c, q, 0.28, rng.random_range(0.0..TAU), mix(pt.accent2, WHITES[0], 0.3));
            }
            keep.push((q, rr + 0.3));
        }
    }
    grounds(g, c, rng, &keep, &d.iter().map(|(_, e)| *e).collect::<Vec<_>>(), &pt, 6);
}

// ── saucer house ─────────────────────────────────────────────────────────────

/// A flattened saucer raised on a core and four slim legs, its rim ringed
/// with portholes, a small dome on top; the door is a pod at the core.
pub fn saucer_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let pk = pack_circles(&c.poly, &[10.0, 4.5], 3.5, 2.0);
    let Some(&(p, rs)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let f = &c.frame;
    let fh = c.fh();
    let n = low_floors(c);
    let hl = 2.7;
    let rim = 2.3 + (n.max(1) - 1) as f32 * fh;
    let core = 1.1f32.max(rs * 0.14);
    turn(g, f, Matte, p, &[(core, 0.0), (core, hl + 0.4)], sides_for(core), pt.wall);
    let y1 = hl + 1.1;
    let y2 = y1 + rim;
    let mut prof = vec![(0.0, hl), (rs * 0.38, hl), (rs * 0.9, hl + 0.75), (rs, y1), (rs, y2), (rs * 0.88, y2 + 0.45), (rs * 0.62, y2 + 0.6)];
    prof.extend(dome_profile(rs * 0.62, y2 + 0.6, (rs * 0.34).max(1.6), 7).into_iter().skip(1));
    shell(g, c, p, &prof, 8, sides_for(rs), &pt);
    let pr = (0.42 + rs * 0.012).min(0.55);
    for k in 0..n.min(200) {
        let y = y1 + k as f32 * fh + 1.15;
        if y + pr * 1.3 > y2 - 0.45 {
            break;
        }
        porthole_row(g, c, rng, p, rs, y, win(pt, pr), porthole_spacing(rs), &[], RIM);
    }
    drum_stripe(g, f, Matte, p, rs, y2 - 0.36, 0.3, pt.accent);
    night_ring(g, f, p, rs, y1 + 0.02, 0.09, mul(pt.accent, 1.2));
    // Four legs from the underside to the lawn.
    for k in 0..4 {
        let a = k as f32 * FRAC_PI_2 + PI / 4.0;
        let top = f.at2(p + Vec2::from_angle(a) * rs * 0.7, hl + 0.55);
        let foot = f.at2(p + Vec2::from_angle(a) * rs * 0.82, 0.0);
        g.part(Matte).tube(foot, top, 0.16, 6, rgba(mix(pt.wall, RIM, 0.4)));
    }
    // The door pod under the saucer.
    let mut doors = Vec::new();
    for d in door_dirs() {
        if let Some(end) = entry_pod(g, c, rng, p, core, d, 1.05, pt.wall, pt.accent2) {
            doors.push(end);
            break;
        }
    }
    antenna(g, c, p, y2 + 0.6 + (rs * 0.34).max(1.6) - 0.2, 2.0, pt.accent);
    let mut keep = vec![(p, rs + 0.6)];
    if let Some(&(q, rq)) = pk.get(1) {
        let rr = rq.min(4.0);
        landing_pad(g, c, q, rr, WHITES[1], pt.accent2);
        keep.push((q, rr + 0.3));
    }
    grounds(g, c, rng, &keep, &doors, &pt, 6);
}

// ── popsicle dome ────────────────────────────────────────────────────────────

/// A ball on a single thick stilt, like a lolly: porthole rows round it,
/// an equator stripe, a glass lift tube up the stilt and a lobby dome at
/// its foot. A second, shorter one where the lot has room.
pub fn popsicle_dome(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let pk = pack_circles(&c.poly, &[12.0, 7.0], 4.0, 3.0);
    let Some(&(p, r)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let h = c.height();
    let mut keep = Vec::new();
    let mut doors = Vec::new();
    let d = lolly(g, c, rng, p, r, h, &pt);
    doors.extend(d);
    keep.push((p, r + 0.5));
    if let Some(&(q, rq)) = pk.get(1) {
        if rq >= 4.5 {
            let pt2 = Paint { accent: pt.accent2, accent2: pt.accent, ..pt };
            doors.extend(lolly(g, c, rng, q, rq, h * 0.62, &pt2));
            keep.push((q, rq + 0.5));
        } else {
            landing_pad(g, c, q, rq.min(4.5), WHITES[1], pt.accent2);
            keep.push((q, rq.min(4.5) + 0.3));
        }
    }
    grounds(g, c, rng, &keep, &doors, &pt, 7);
}

/// One ball on its stilt in a circle of radius `r` reaching about `h`.
/// Returns the lobby door.
fn lolly(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32, pt: &Paint) -> Option<Vec2> {
    let f = &c.frame;
    let fh = c.fh();
    let rs = (r * 0.95).clamp(3.8, 12.0);
    let ry = rs * 0.92;
    let top = h.max(2.0 * ry + 4.0);
    let cy = top - ry;
    let rc = (rs * 0.24).clamp(1.1, 2.8);
    turn(g, f, Matte, p, &[(rc * 1.25, 0.0), (rc, 1.0), (rc, cy - ry * 0.5)], sides_for(rc), pt.wall);
    shell(g, c, p, &ball_profile(rs, ry, cy, 14), 4, sides_for(rs), pt);
    // Rows of portholes above and below the equator, a stripe on it.
    let pr = (0.4 + rs * 0.012).min(0.55);
    for j in 0..40 {
        let dy = (j / 2) as f32 * fh + fh * 0.5;
        let y = if j % 2 == 0 { cy + dy } else { cy - dy };
        let s = (y - cy) / ry;
        if s.abs() > 0.82 {
            if j > 2 {
                break;
            }
            continue;
        }
        porthole_row_ball(g, c, rng, p, rs, ry, cy, s.asin(), win(pt, pr), porthole_spacing(rs), &[], RIM);
    }
    ellipse_band(g, f, Matte, p, rs, ry, cy, -0.035, 0.035, 0.05, pt.accent);
    let from = g.get(Glow).colors.len();
    ellipse_band(g, f, Glow, p, rs, ry, cy, -0.075, -0.05, 0.08, mul(pt.accent, 1.2));
    mark_glow(g, from, NIGHT_MARK);
    // The lift: a glass tube up the stilt.
    let lift = p + Vec2::new(rc + 0.7, 0.0);
    let ly0 = 2.0;
    let ly1 = cy - ry * 0.75;
    if ly1 > ly0 + 1.0 {
        turn(g, f, Glass, lift, &[(0.55, ly0), (0.55, ly1)], 10, GLASS);
    }
    // The lobby: a low dome round the stilt's foot.
    let rl = (rc + 2.4).min(r - 0.3);
    let mut out = None;
    if rl > rc + 1.0 {
        let d = door(g, c, rng, p, rl, 1.1, pt);
        let skip: Vec<(f32, f32)> = d.iter().map(|(s, _)| *s).collect();
        drum_dome(g, c, rng, p, rl, 0.0, 3.2, 1.6, pt, &skip);
        out = d.map(|(_, e)| e);
    }
    if top > 20.0 {
        antenna(g, c, p, cy + ry - 0.2, 3.0, pt.accent2);
    }
    out
}

// ── twin domes ───────────────────────────────────────────────────────────────

/// Two or three domes of different sizes and heights joined by glass tubes.
pub fn twin_domes(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let pk = pack_circles(&c.poly, &[9.0, 6.5, 5.0], 3.0, 2.6);
    if pk.is_empty() {
        return tiny(g, c, rng, &pt);
    }
    let fh = c.fh();
    let n = low_floors(c);
    let link_y = 1.6;
    let mut doors = Vec::new();
    let mut keep = Vec::new();
    for (i, &(p, r)) in pk.iter().enumerate() {
        let fl = n.saturating_sub(i).max(1);
        let wall = (fl.max(2) - 2) as f32 * fh + 3.2;
        // Tube angles to the first dome (and from it to the others).
        let mut skip: Vec<(f32, f32)> = Vec::new();
        let links: Vec<Vec2> = if i == 0 { pk.iter().skip(1).map(|(q, _)| *q).collect() } else { vec![pk[0].0] };
        for q in &links {
            let d = *q - p;
            skip.push((d.y.atan2(d.x), 1.5 / r));
        }
        let d = if i == 0 || coin(rng, 0.5) { door(g, c, rng, p, r, 1.15, &pt) } else { None };
        if let Some((s, e)) = d {
            skip.push(s);
            doors.push(e);
        }
        let pti = if i % 2 == 1 { Paint { accent: pt.accent2, ..pt } } else { pt };
        drum_dome(g, c, rng, p, r, 0.0, wall, (r * 0.75).max(fh), &pti, &skip);
        keep.push((p, r + 0.6));
    }
    let (p0, r0) = pk[0];
    for &(q, rq) in pk.iter().skip(1) {
        let d = (q - p0).normalize_or(Vec2::X);
        let gap = p0.distance(q) - r0 - rq;
        if gap < 14.0 {
            link_tube(g, c, p0 + d * (r0 - 0.4), q - d * (rq - 0.4), link_y, 0.95, GLASS, WHITES[1]);
        }
    }
    if trades(c) {
        emblem_maybe(g, c, rng, doors.first().copied());
    }
    grounds(g, c, rng, &keep, &doors, &pt, 5);
}

// ── dome with pods ───────────────────────────────────────────────────────────

/// A main dome with two to four small pods round it, each joined by a
/// short glass tube.
pub fn dome_pods(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let pk = pack_circles(&c.poly, &[8.5], 3.5, 0.0);
    let Some(&(p, r0)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let fh = c.fh();
    let n = low_floors(c);
    let rp = (r0 * 0.42).clamp(2.0, 3.6);
    // The main dome may shrink to leave its pods room.
    let mut pods: Vec<(Vec2, f32)> = Vec::new();
    let mut r = r0;
    for _ in 0..3 {
        pods.clear();
        for k in 0..8 {
            let a = k as f32 / 8.0 * TAU + 0.2;
            let q = p + Vec2::from_angle(a) * (r + rp + 1.3);
            if a.sin() < -0.75 {
                continue; // the street side keeps the door
            }
            if circle_fits(c, q, rp, 0.6) && pods.iter().all(|(o, _)| o.distance(q) > 2.0 * rp + 0.6) {
                pods.push((q, rp));
            }
            if pods.len() >= 4 {
                break;
            }
        }
        if pods.len() >= 2 || r < 4.5 {
            break;
        }
        r *= 0.82;
    }
    let mut skip: Vec<(f32, f32)> = pods.iter().map(|(q, _)| ((q.y - p.y).atan2(q.x - p.x), 1.2 / r)).collect();
    let d = door(g, c, rng, p, r, 1.2, &pt);
    if let Some((s, _)) = d {
        skip.push(s);
    }
    let wall = (n.max(2) - 2) as f32 * fh + 3.2;
    let crown = drum_dome(g, c, rng, p, r, 0.0, wall, (r * 0.75).max(fh * 1.1), &pt, &skip);
    antenna(g, c, p, crown - 0.3, 2.2, pt.accent2);
    let mut keep = vec![(p, r + 0.6)];
    for (i, &(q, rr)) in pods.iter().enumerate() {
        let pti = Paint { accent: if i % 2 == 0 { pt.accent2 } else { pt.accent }, ..pt };
        let dir = (q - p).normalize_or(Vec2::X);
        let back = (-dir.y).atan2(-dir.x);
        drum_dome(g, c, rng, q, rr, 0.0, 2.6, rr * 0.85, &pti, &[(back, 1.1 / rr)]);
        link_tube(g, c, p + dir * (r - 0.35), q - dir * (rr - 0.35), 1.35, 0.75, GLASS, WHITES[1]);
        keep.push((q, rr + 0.5));
    }
    grounds(g, c, rng, &keep, &d.iter().map(|(_, e)| *e).collect::<Vec<_>>(), &pt, 5);
}

// ── capsule tower ────────────────────────────────────────────────────────────

/// A pill-shaped tower: a white shaft with a row of portholes every floor,
/// a stripe every few floors, a rounded top with an antenna or a radar
/// ball, over a glass lobby. A shorter twin joined by a sky tube where the
/// lot is long.
pub fn capsule_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, pad: bool) {
    let pt = paint_slots(c, rng, 0.75);
    let pk = pack_circles(&c.poly, &[13.0, 8.5], 4.0, 3.0);
    let Some(&(p, r)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let h = c.height();
    let mut doors = Vec::new();
    let mut keep = vec![(p, r + 0.5)];
    let (top0, door0) = pill(g, c, rng, p, r, h, &pt, true, pad);
    doors.extend(door0);
    if let Some(&(q, rq)) = pk.get(1) {
        if rq >= 4.5 && h > 15.0 {
            let pt2 = Paint { accent: pt.accent2, accent2: pt.accent, ..pt };
            let (top1, d1) = pill(g, c, rng, q, rq, h * 0.6, &pt2, false, false);
            doors.extend(d1);
            let gap = p.distance(q) - r - rq;
            if gap < 16.0 {
                let fh = c.fh();
                let y = ((top0.min(top1) * 0.55 - 3.2) / fh).floor().max(1.0) * fh + 3.2 + fh * 0.5;
                let d = (q - p).normalize_or(Vec2::X);
                link_tube(g, c, p + d * (r * 0.9 - 0.4), q - d * (rq * 0.9 - 0.4), y, 1.1, GLASS, WHITES[1]);
            }
            keep.push((q, rq + 0.5));
        } else if rq >= 3.5 {
            landing_pad(g, c, q, rq.min(5.5), WHITES[1], pt.accent2);
            if coin(rng, 0.5) {
                hover_car(g, c, q, 0.28, rng.random_range(0.0..TAU), mix(pt.accent, WHITES[0], 0.3));
            }
            keep.push((q, rq.min(5.5) + 0.3));
        }
    }
    emblem_maybe(g, c, rng, doors.first().copied());
    grounds(g, c, rng, &keep, &doors, &pt, 6);
}

/// One pill tower in a circle of radius `r`, about `h` tall. Returns its
/// top and its door.
#[allow(clippy::too_many_arguments)]
fn pill(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32, pt: &Paint, crown: bool, pad: bool) -> (f32, Option<Vec2>) {
    let f = &c.frame;
    let fh = c.fh();
    let lobby = 3.4;
    let rt = r * 0.88;
    let cap = (rt * 0.8).max(3.0);
    let wall_top = (h - cap * 0.35).max(lobby + 2.0 * fh);
    // The glass lobby under a white canopy ring.
    turn(g, f, Glass, p, &[(r * 0.97, 0.0), (r * 0.97, lobby)], sides_for(r), GLASS);
    turn(g, f, Matte, p, &[(rt, lobby - 0.05), (r, lobby), (r, lobby + 0.35), (rt, lobby + 0.4)], sides_for(r), pt.wall);
    let mut prof = vec![(rt, lobby + 0.3), (rt, wall_top)];
    prof.extend(dome_profile(rt, wall_top, cap, 9).into_iter().skip(1));
    shell(g, c, p, &prof, 5, sides_for(rt), pt);
    let pr = (0.42 + rt * 0.01).min(0.55);
    let spacing = porthole_spacing(rt);
    let y0 = lobby + 0.4;
    let mut k = 0usize;
    loop {
        let y = y0 + k as f32 * fh + fh.min(3.6) * 0.55;
        if y + pr * 1.3 > wall_top - 0.5 || k > 400 {
            break;
        }
        porthole_row(g, c, rng, p, rt, y, win(pt, pr), spacing, &[], RIM);
        // A stripe at every fifth floor line.
        if k % 5 == 4 {
            let yl = y0 + (k + 1) as f32 * fh;
            if yl + 0.35 < wall_top - 0.4 {
                drum_stripe(g, f, Matte, p, rt, yl + 0.02, 0.26, if (k / 5) % 2 == 0 { pt.accent } else { pt.accent2 });
            }
        }
        k += 1;
    }
    drum_stripe(g, f, Matte, p, rt, wall_top - 0.38, 0.3, pt.accent);
    night_ring(g, f, p, rt, wall_top - 0.5, 0.08, mul(pt.accent, 1.2));
    if cap > 4.0 {
        porthole_row_ball(g, c, rng, p, rt, cap, wall_top, 0.4, win(pt, pr), spacing * 1.1, &[], RIM);
    }
    let top = wall_top + cap;
    let radar = crown && c.tier == nd_sim::Tier::High && rt > 5.0;
    let rb = (rt * 0.16).clamp(1.2, 2.4);
    if pad {
        let item = if radar { 2.9 * rb + 0.4 } else { 3.4 };
        sky_pad(g, c, p, top - 0.4, item + 0.9, rt.min(r), pt);
    } else if radar {
        radar_ball(g, c, p, top - 0.35, rb, WHITES[1], RIM);
    } else {
        antenna(g, c, p, top - 0.3, 3.0, pt.accent2);
    }
    // The door: a pod out of the lobby.
    let mut door_at = None;
    for d in door_dirs() {
        if let Some(e) = entry_pod(g, c, rng, p, r * 0.97, d, 1.3, pt.wall, pt.accent) {
            door_at = Some(e);
            break;
        }
    }
    (top, door_at)
}

// ── saucer stack ─────────────────────────────────────────────────────────────

/// Saucers stacked round a white core, two floors apart, each with a rim
/// of portholes and a line of light under it; a dome, a radar ball and an
/// antenna on top, a glass lobby at the foot.
pub fn saucer_stack(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, pad: bool) {
    let pt = paint_slots(c, rng, 0.75);
    let pk = pack_circles(&c.poly, &[15.0, 5.0], 4.5, 3.0);
    let Some(&(p, r)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let f = &c.frame;
    let fh = c.fh();
    let n = floors(c);
    let rc = (r * 0.28).clamp(2.0, 4.5);
    let pitch = 2.0 * fh;
    let decks = n.div_ceil(2).max(1);
    let base = fh * 1.3;
    let rim = (fh * 0.72).max(1.9);
    let mut top = base;
    for k in 0..decks.min(200) {
        let y = base + k as f32 * pitch;
        let taper = 1.0 - 0.18 * k as f32 / decks.max(2) as f32;
        let rs = r * if k % 2 == 0 { 1.0 } else { 0.86 } * taper;
        if rs < rc + 1.5 {
            break;
        }
        let y1 = y + 1.0;
        let y2 = y1 + rim;
        turn(g, f, Matte, p, &[(rc, y), (rs * 0.9, y + 0.7), (rs, y1), (rs, y2), (rs * 0.93, y2 + 0.35), (rc, y2 + 0.9)], sides_for(rs), pt.wall);
        let pr = (rim * 0.24).min(0.52);
        porthole_row(g, c, rng, p, rs, y1 + rim * 0.5, win(pt, pr), porthole_spacing(rs), &[], RIM);
        drum_stripe(g, f, Matte, p, rs, y2 - 0.28, 0.22, if k % 2 == 0 { pt.accent } else { pt.accent2 });
        night_ring(g, f, p, rs, y1 + 0.02, 0.09, mul(pt.accent, 1.2));
        top = y2 + 0.9;
    }
    let core_top = top + 1.2;
    let mut prof = vec![(rc, 0.0), (rc, core_top)];
    prof.extend(dome_profile(rc, core_top, rc * 0.8, 6).into_iter().skip(1));
    shell(g, c, p, &prof, 7, sides_for(rc), &pt);
    let crown = core_top + rc * 0.8;
    if pad {
        sky_pad(g, c, p, crown - 0.3, 2.9 * (rc * 0.45).clamp(1.0, 2.0) + 1.3, r * 0.8, &pt);
    } else {
        radar_ball(g, c, p, crown - 0.3, (rc * 0.45).clamp(1.0, 2.0), WHITES[1], RIM);
    }
    // The lobby: glass round the foot of the core, a white roof.
    let rl = (rc + 2.6).min(r * 0.9);
    turn(g, f, Glass, p, &[(rl, 0.0), (rl, base - 0.3)], sides_for(rl), GLASS);
    turn(g, f, Matte, p, &[(rl - 0.1, base - 0.35), (rl + 0.25, base - 0.3), (rl + 0.25, base), (rc, base + 0.05)], sides_for(rl), pt.wall);
    let mut doors = Vec::new();
    for d in door_dirs() {
        if let Some(e) = entry_pod(g, c, rng, p, rl, d, 1.2, pt.wall, pt.accent) {
            doors.push(e);
            break;
        }
    }
    let mut keep = vec![(p, r + 0.5)];
    if let Some(&(q, rq)) = pk.get(1) {
        landing_pad(g, c, q, rq.min(5.0), WHITES[1], pt.accent2);
        keep.push((q, rq.min(5.0) + 0.3));
    }
    emblem_maybe(g, c, rng, doors.first().copied());
    grounds(g, c, rng, &keep, &doors, &pt, 6);
}

// ── stilt tower ──────────────────────────────────────────────────────────────

/// A tall slender stilt with a big domed pod at the top holding the upper
/// floors, a saucer half way up on taller ones, glass lifts alongside and a
/// lobby dome at the foot.
pub fn stilt_tower(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, pad: bool) {
    let pt = paint_slots(c, rng, 0.75);
    let pk = pack_circles(&c.poly, &[15.0, 5.0], 4.5, 3.0);
    let Some(&(p, r)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let f = &c.frame;
    let fh = c.fh();
    let n = floors(c);
    let h = c.height();
    let rp = (r * 0.95).clamp(4.5, 15.0);
    let pf = (n / 5).clamp(2, 4);
    let wall = pf as f32 * fh;
    let dome_h = rp * 0.32;
    let top = h.max(wall + 2.0 + dome_h + 16.0);
    let yb = top - wall - 2.0 - dome_h;
    let rc = (rp * 0.2).clamp(1.5, 3.0);
    turn(g, f, Matte, p, &[(rc * 1.3, 0.0), (rc, 1.5), (rc, yb + 1.0)], sides_for(rc), pt.wall);
    let mut prof = vec![(rc, yb), (rp * 0.85, yb + 1.2), (rp, yb + 2.0), (rp, yb + 2.0 + wall)];
    prof.extend(dome_profile(rp, yb + 2.0 + wall, dome_h, 8).into_iter().skip(1));
    shell(g, c, p, &prof, 4, sides_for(rp), &pt);
    let pr = (0.42 + rp * 0.01).min(0.55);
    for k in 0..pf {
        let y = yb + 2.0 + k as f32 * fh + fh.min(3.6) * 0.55;
        if y + pr * 1.3 > yb + 2.0 + wall - 0.5 {
            break;
        }
        porthole_row(g, c, rng, p, rp, y, win(pt, pr), porthole_spacing(rp), &[], RIM);
    }
    drum_stripe(g, f, Matte, p, rp, yb + 2.0 + wall - 0.38, 0.3, pt.accent);
    night_ring(g, f, p, rp, yb + 2.02, 0.09, mul(pt.accent, 1.2));
    porthole_row_ball(g, c, rng, p, rp, dome_h, yb + 2.0 + wall, 0.45, win(pt, pr * 0.9), porthole_spacing(rp) * 1.2, &[], RIM);
    // A saucer half way up a tall one.
    if yb > 40.0 {
        let ys = (yb * 0.55 / fh).floor() * fh;
        let rs = rp * 0.62;
        let rim = (fh * 0.72).max(1.9);
        turn(g, f, Matte, p, &[(rc, ys), (rs * 0.9, ys + 0.7), (rs, ys + 1.0), (rs, ys + 1.0 + rim), (rs * 0.92, ys + 1.35 + rim), (rc, ys + 1.9 + rim)], sides_for(rs), pt.wall);
        porthole_row(g, c, rng, p, rs, ys + 1.0 + rim * 0.5, win(&pt, (rim * 0.24).min(0.5)), porthole_spacing(rs), &[], RIM);
        drum_stripe(g, f, Matte, p, rs, ys + 1.0 + rim - 0.28, 0.22, pt.accent2);
        night_ring(g, f, p, rs, ys + 1.02, 0.09, mul(pt.accent2, 1.2));
    }
    // Glass lifts up the stilt.
    for s in [1.0f32, -1.0] {
        let q = p + Vec2::new(s * (rc + 0.65), 0.0);
        turn(g, f, Glass, q, &[(0.5, 3.0), (0.5, yb + 0.6)], 10, GLASS);
    }
    let crown = yb + 2.0 + wall + dome_h;
    if pad {
        sky_pad(g, c, p, crown - 0.3, 7.2, rp, &pt);
    } else {
        antenna(g, c, p, crown - 0.3, 6.0, pt.accent2);
    }
    if rp > 7.0 && !pad {
        dish_on_post(g, c, p + Vec2::new(rp * 0.45, 0.0), yb + 2.0 + wall + dome_h * 0.55, 1.3, 0.7, 0.4, WHITES[1]);
    }
    // The lobby dome at the foot.
    let rl = (rc + 3.2).min(r - 0.3);
    let mut doors = Vec::new();
    if rl > rc + 1.2 {
        let d = door(g, c, rng, p, rl, 1.15, &pt);
        let skip: Vec<(f32, f32)> = d.iter().map(|(s, _)| *s).collect();
        drum_dome(g, c, rng, p, rl, 0.0, 3.3, 1.8, &pt, &skip);
        doors.extend(d.map(|(_, e)| e));
    }
    let mut keep = vec![(p, rl.max(rc) + 1.0)];
    if let Some(&(q, rq)) = pk.get(1) {
        landing_pad(g, c, q, rq.min(5.0), WHITES[1], pt.accent2);
        keep.push((q, rq.min(5.0) + 0.3));
    }
    emblem_maybe(g, c, rng, doors.first().copied());
    grounds(g, c, rng, &keep, &doors, &pt, 7);
}

// ── dome hall ────────────────────────────────────────────────────────────────

/// A big shallow dome over a glass drum: a hall, a market, a mall. Lit
/// shop windows round the glass, portholes round the dome and a glass
/// oculus at its crown, a curved glass canopy over the way in, a landing
/// pad on the lawn.
pub fn dome_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let pk = pack_circles(&c.poly, &[22.0, 6.0, 4.5], 5.0, 3.0);
    let Some(&(p, r)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let f = &c.frame;
    let fh = c.fh();
    let n = floors(c);
    // The dome holds the top floor; a small hall's dome is a full half
    // ball, a great hall's a shallow one.
    let wall = n.saturating_sub(1).max(1) as f32 * fh + 0.9;
    let dome_h = r * (0.75 - 0.025 * (r - 6.0).clamp(0.0, 13.0));
    let glass_top = wall - 0.9;
    let s = sides_for(r);
    turn(g, f, Glass, p, &[(r, 0.0), (r, glass_top)], s, GLASS);
    // The white shell: a band over the glass, then the dome to the oculus.
    let full = dome_profile(r, wall, dome_h, 10);
    let mut prof = vec![(r * 0.985, glass_top - 0.05), (r, glass_top), (r, wall)];
    prof.extend(full[1..8].iter().copied());
    turn(g, f, Matte, p, &prof, s, pt.wall);
    if coin(rng, 0.5) {
        turn(g, f, Glass, p, &full[7..], s, GLASS);
    } else {
        turn(g, f, Matte, p, &full[7..], s, pt.cap);
    }
    // White floor bands across the glass, and lit shopfronts.
    let lit = c.lit_chance();
    for k in 0..n.saturating_sub(1).clamp(1, 60) {
        let y = k as f32 * fh;
        if k > 0 {
            turn(g, f, Matte, p, &[(r + 0.04, y - 0.25), (r + 0.04, y + 0.15)], s, pt.wall);
        }
        let segs = ((TAU * r / 4.0) as usize).clamp(6, 32);
        for i in 0..segs {
            if !coin(rng, lit) {
                continue;
            }
            let a0 = i as f32 / segs as f32 * TAU + 0.04;
            let a1 = (i + 1) as f32 / segs as f32 * TAU - 0.04;
            let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.55..0.95));
            let ax = Axis::vertical(f, p, 0.0);
            window_light(g, c, |g| revolve(g.part(Glow), &ax, &[(r + 0.12, y + 0.5), (r + 0.12, y + fh * 0.7)], 3, Some((a0, a1)), rgba(col)));
        }
    }
    drum_stripe(g, f, Matte, p, r, wall - 0.4, 0.3, pt.accent);
    night_ring(g, f, p, r, wall - 0.52, 0.09, mul(pt.accent, 1.2));
    porthole_row_ball(g, c, rng, p, r, dome_h, wall, 0.3, win(&pt, 0.55), porthole_spacing(r), &[], RIM);
    // The way in: a glass vault out to the street.
    let mut doors = Vec::new();
    let mut vault = None;
    for d in door_dirs() {
        let a = p + d * (r - 0.6);
        let b = p + d * (r + 5.5);
        let side = Vec2::new(-d.y, d.x) * 2.6;
        if [b + d * 0.6, b + side, b - side].iter().all(|q| c.inside_local(*q) && distance_to_boundary(*q, &c.lot_poly) > 0.4) {
            glass_vault(g, c, a, b, 1.9, 2.3, GLASS, WHITES[1]);
            doors.push(b);
            vault = Some(d.y.atan2(d.x));
            break;
        }
    }
    // Striped awnings round the ground floor's shopfronts.
    if trades(c) && coin(rng, 0.75) {
        let y = fh.min(4.2) - 0.25;
        let segs = ((TAU * r / 1.6) as usize).clamp(12, 72);
        let ax = Axis::vertical(f, p, 0.0);
        for i in 0..segs {
            let a0 = i as f32 / segs as f32 * TAU;
            let a1 = (i + 1) as f32 / segs as f32 * TAU;
            if let Some(v) = vault {
                let m = ((a0 + a1) * 0.5 - v).rem_euclid(TAU);
                if m.min(TAU - m) < 3.2 / r {
                    continue;
                }
            }
            let col = if i % 2 == 0 { WHITES[1] } else { pt.accent };
            revolve(g.part(Matte), &ax, &[(r + 1.5, y - 0.65), (r + 0.06, y)], 2, Some((a0, a1)), rgba(col));
        }
    }
    let mut keep = vec![(p, r + 0.5)];
    for &(q, rq) in pk.iter().skip(1).take(1) {
        landing_pad(g, c, q, rq.min(5.5), WHITES[1], pt.accent2);
        if coin(rng, 0.6) {
            hover_car(g, c, q, 0.28, rng.random_range(0.0..TAU), mix(pt.accent2, WHITES[0], 0.25));
        }
        keep.push((q, rq.min(5.5) + 0.3));
    }
    emblem_maybe(g, c, rng, doors.first().copied());
    grounds(g, c, rng, &keep, &doors, &pt, 8);
}

// ── dome campus ──────────────────────────────────────────────────────────────

/// A round office drum with a ribbon of glass every floor, a shallow dome
/// roof with a radar ball and a dish, a lab dome joined by a tube, and a
/// dish or a landing pad on the lawn.
pub fn dome_campus(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pt = paint(c, rng);
    let pk = pack_circles(&c.poly, &[16.0, 7.0, 4.5], 4.5, 3.0);
    let Some(&(p, r)) = pk.first() else { return tiny(g, c, rng, &pt) };
    let f = &c.frame;
    let fh = c.fh();
    let n = floors(c);
    let h = n as f32 * fh + 0.6;
    let s = sides_for(r);
    let roof = (r * 0.25).max(1.5);
    let mut prof = vec![(r, 0.0), (r, h)];
    prof.extend(dome_profile(r, h, roof, 6).into_iter().skip(1));
    shell(g, c, p, &prof, 4, s, &pt);
    // A ribbon of glass each floor, lit in stretches.
    let lit = c.lit_chance();
    let ax = Axis::vertical(f, p, 0.0);
    let ribbon = fh * 0.4;
    let segs = ((TAU * r / 3.5) as usize).clamp(6, 28);
    let mut skip_door: Option<(f32, f32)> = None;
    let mut doors = Vec::new();
    for d in door_dirs() {
        if let Some(e) = entry_pod(g, c, rng, p, r, d, 1.3, pt.wall, pt.accent) {
            skip_door = Some((d.y.atan2(d.x), 2.0 / r));
            doors.push(e);
            break;
        }
    }
    for k in 0..n.min(400) {
        let y = k as f32 * fh + fh * 0.32;
        turn(g, f, Glass, p, &[(r + 0.03, y), (r + 0.03, y + ribbon)], s, GLASS);
        for i in 0..segs {
            if !coin(rng, lit) {
                continue;
            }
            let a0 = i as f32 / segs as f32 * TAU + 0.03;
            let a1 = (i + 1) as f32 / segs as f32 * TAU - 0.03;
            if k == 0 {
                if let Some((sa, gap)) = skip_door {
                    let mid = (a0 + a1) * 0.5;
                    let dd = (mid - sa).rem_euclid(TAU);
                    if dd.min(TAU - dd) < gap {
                        continue;
                    }
                }
            }
            let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.55..0.95));
            window_light(g, c, |g| revolve(g.part(Glow), &ax, &[(r + 0.13, y + 0.06), (r + 0.13, y + ribbon - 0.06)], 2, Some((a0, a1)), rgba(col)));
        }
    }
    drum_stripe(g, f, Matte, p, r, h - 0.42, 0.3, pt.accent);
    night_ring(g, f, p, r, h - 0.54, 0.08, mul(pt.accent, 1.2));
    let crown = h + roof;
    radar_ball(g, c, p, crown - 0.25, (r * 0.17).clamp(1.3, 3.0), WHITES[1], RIM);
    if r > 6.0 {
        dish_on_post(g, c, p + Vec2::new(-r * 0.5, r * 0.2), h + roof * 0.5, (r * 0.12).clamp(0.9, 1.8), 0.8, -2.2, WHITES[1]);
    }
    let mut keep = vec![(p, r + 0.6)];
    if let Some(&(q, rq)) = pk.get(1) {
        let rr = rq.min(6.5);
        let pt2 = Paint { accent: pt.accent2, ..pt };
        let dir = (q - p).normalize_or(Vec2::X);
        let back = (-dir.y).atan2(-dir.x);
        drum_dome(g, c, rng, q, rr, 0.0, 3.2 + (n.min(2) as f32 - 1.0) * fh, rr * 0.8, &pt2, &[(back, 1.4 / rr)]);
        if p.distance(q) - r - rr < 14.0 {
            link_tube(g, c, p + dir * (r - 0.4), q - dir * (rr - 0.4), 1.6, 1.0, GLASS, WHITES[1]);
        }
        keep.push((q, rr + 0.5));
    }
    if let Some(&(q, rq)) = pk.get(2) {
        if coin(rng, 0.5) {
            dish_on_post(g, c, q, 0.0, rq.min(3.0), 0.75, rng.random_range(0.0..TAU), WHITES[1]);
        } else {
            landing_pad(g, c, q, rq.min(4.5), WHITES[1], pt.accent2);
        }
        keep.push((q, rq.min(4.5) + 0.3));
    }
    emblem_maybe(g, c, rng, doors.first().copied());
    grounds(g, c, rng, &keep, &doors, &pt, 7);
}
