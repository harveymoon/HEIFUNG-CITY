//! [waterfront] The looks of the shore (`nd_sim::shore`): beach resorts,
//! waterfront hotels, restaurants on piers, beach bars, boat rentals and
//! public beaches, plus the private jetties businesses build off their
//! lots.
//!
//! Waterfront lots run from the street (`v0`) down to the water, which
//! usually lies at the back (`v1`); [`ShoreLook::seaward`] says where it
//! really is and [`sea_local`] turns it into the lot frame. Everything is
//! neon by night: pools lit cyan from below, string lights over the decks,
//! tiki torches, glowing signs.
//!
//! Beaches are planned in world coordinates by the sim
//! (`nd_sim::shore::plan_beach`): a boardwalk and towel spots. They are
//! drawn here with the crowd of the hour (`BeachLook`), the lifeguard tower,
//! umbrellas and loungers, swimmers in the water off them, and a sign and
//! tape when industry has fouled them.

use super::parts::*;
use super::kit::clip_box;
use super::kit::{block, fit, flagpole, pane_row, table, R};
use super::{Ctx, Face, Frame};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::shore::{Jetty, JettyKind};

use rand::Rng;
use std::f32::consts::{FRAC_PI_2, PI};

/// Where the water is for a lot on it, and a beach's state, for the looks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShoreLook {
    /// Unit vector out over the water, world coordinates (zero: unknown).
    pub seaward: Vec2,
    /// How far the water's surface lies below the lot's pad, metres.
    pub drop: f32,
    /// A public beach's crowd and state.
    pub beach: Option<BeachLook>,
}

/// A beach as the hour leaves it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BeachLook {
    /// People lying on the sand (the sim's own walkers are drawn apart).
    pub bathers: u16,
    /// People in the water.
    pub swimmers: u16,
    /// Fouled: a sign and tape across the way in.
    pub closed: bool,
    /// After dark: the boardwalk lights and the tower lamp on.
    pub night: bool,
}

const SAND: [f32; 3] = [0.74, 0.66, 0.48];
const WHITE: [f32; 3] = [0.9, 0.89, 0.85];
const POOL_GLOW: [f32; 3] = [0.15, 1.1, 1.4];
const WARM: [f32; 3] = [1.6, 1.1, 0.55];
const TORCH: [f32; 3] = [2.2, 0.9, 0.25];
/// Umbrella and towel colours: bright stripes against the sand.
const BRIGHT: [[f32; 3]; 8] = [[0.95, 0.25, 0.3], [0.98, 0.75, 0.15], [0.15, 0.6, 0.9], [0.95, 0.95, 0.9], [0.2, 0.75, 0.55], [0.9, 0.4, 0.75], [0.98, 0.5, 0.15], [0.3, 0.35, 0.85]];
/// Swimwear and skin for the bathers.
const SKIN: [[f32; 3]; 4] = [[0.85, 0.65, 0.5], [0.6, 0.42, 0.3], [0.38, 0.25, 0.18], [0.9, 0.75, 0.62]];

/// Seaward in the lot frame (unit; +v, the back, when unknown).
pub fn sea_local(c: &Ctx, look: Option<ShoreLook>) -> Vec2 {
    let s = look.map_or(Vec2::ZERO, |l| l.seaward);
    if s == Vec2::ZERO {
        return Vec2::Y;
    }
    let f = &c.frame;
    Vec2::new(s.dot(f.along), s.dot(f.inward)).normalize_or(Vec2::Y)
}

use crate::hash::hash2 as hash;

// ── small props (lot frame) ──────────────────────────────────────────────

/// A beach umbrella: pole and a striped canopy.
fn umbrella(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, color: [f32; 3]) {
    f.bx(g, Matte, p.x, y + 1.1, p.y, Vec3::new(0.05, 2.2, 0.05), WHITE);
    let w = f.to_world(p.x, p.y);
    g.part(Tarp).cylinder(w, 1.4, 0.08, y + 1.95, y + 2.4, 8, rgba(color));
}

/// A sun lounger, its head towards `yaw` (lot frame, radians).
fn lounger(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, yaw: f32, color: [f32; 3]) {
    f.bx_yaw(g, Matte, p.x, y + 0.3, p.y, Vec3::new(1.9, 0.08, 0.65), color, yaw);
    let back = Vec2::new(yaw.cos(), yaw.sin()) * 0.8;
    f.bx_yaw(g, Matte, p.x + back.x, y + 0.5, p.y + back.y, Vec3::new(0.45, 0.06, 0.65), color, yaw);
    f.bx_yaw(g, Matte, p.x, y + 0.14, p.y, Vec3::new(1.6, 0.28, 0.08), mul(color, 0.6), yaw);
}

/// A palm at a lot point.
fn palm(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    // Its crown stays over the lot.
    if !on_lot(c, p, h * 0.75 + 0.5) {
        return;
    }
    let w = c.frame.to_world(p.x, p.y);
    tree_kind(g, w, 0.0, TreeKind::Palm, h, h * 0.55, c.palette.bark, mul(c.palette.foliage, 1.1));
}

/// A tiki torch: a pole and a flame that glows.
fn torch(g: &mut Geometry, f: &Frame, p: Vec2, y: f32) {
    f.bx(g, Wood, p.x, y + 0.9, p.y, Vec3::new(0.09, 1.8, 0.09), [0.35, 0.22, 0.12]);
    f.bx(g, Glow, p.x, y + 1.92, p.y, Vec3::new(0.16, 0.26, 0.16), TORCH);
}

/// A pool: coping, water lit from below, a ladder.
fn pool(g: &mut Geometry, f: &Frame, r: &R, y: f32) {
    let c = r.c();
    f.bx(g, Paving, c.x, y + 0.06, c.y, Vec3::new(r.w() + 1.2, 0.12, r.d() + 1.2), [0.85, 0.84, 0.8]);
    // The water lit from below: turquoise by day, glowing at night.
    f.bx(g, Glow, c.x, y + 0.13, c.y, Vec3::new(r.w(), 0.04, r.d()), mul(POOL_GLOW, 0.55));
    f.bx(g, Metal, r.u0 + 0.6, y + 0.6, r.v0 + 0.2, Vec3::new(0.5, 0.9, 0.06), [0.75, 0.76, 0.78]);
}

/// A cabana: four posts, a striped roof and curtains on three sides.
fn cabana(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, color: [f32; 3]) {
    let (hw, hd) = (1.4f32, 1.4f32);
    for (du, dv) in [(-hw, -hd), (hw, -hd), (hw, hd), (-hw, hd)] {
        f.bx(g, Wood, p.x + du, y + 1.2, p.y + dv, Vec3::new(0.1, 2.4, 0.1), WHITE);
    }
    f.bx(g, Tarp, p.x, y + 2.5, p.y, Vec3::new(hw * 2.0 + 0.4, 0.2, hd * 2.0 + 0.4), color);
    f.bx(g, Tarp, p.x, y + 2.65, p.y, Vec3::new(hw * 2.0 + 0.4, 0.1, 0.5), WHITE);
    f.bx(g, Tarp, p.x, y + 1.3, p.y + hd, Vec3::new(hw * 2.0, 2.2, 0.04), mul(WHITE, 0.95));
    f.bx(g, Matte, p.x, y + 0.3, p.y, Vec3::new(1.9, 0.3, 1.9), [0.95, 0.94, 0.9]);
}

/// A run of sand behind the lot's building: the clipped back strip.
fn sand_strip(g: &mut Geometry, c: &Ctx, from_v: f32) -> Option<R> {
    let a = R::of(&c.lot_poly);
    if a.v1 - from_v < 2.0 {
        return None;
    }
    let poly = clip_box(&offset_poly(&c.lot_poly, 0.2), a.u0, a.u1, from_v, a.v1);
    if poly.len() >= 3 {
        ground(g, &c.frame, Dirt, &poly, 0.045, SAND);
    }
    Some(R::new(a.u0 + 0.8, a.u1 - 0.8, from_v + 0.5, a.v1 - 0.8))
}

/// Rows of umbrellas and loungers over a rectangle of sand or deck.
fn umbrella_rows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y: f32, max: usize) {
    let f = &c.frame;
    let nu = ((r.w() - 1.0) / 4.5).floor().max(0.0) as usize;
    let nv = ((r.d() - 1.0) / 5.0).floor().max(0.0) as usize;
    let mut n = 0;
    for i in 0..nu {
        for j in 0..nv {
            if n >= max {
                return;
            }
            let p = Vec2::new(r.u0 + 2.2 + i as f32 * 4.5 + if j % 2 == 1 { 1.5 } else { 0.0 }, r.v0 + 2.5 + j as f32 * 5.0);
            if !on_lot(c, p, 1.0) {
                continue;
            }
            let col = BRIGHT[rng.random_range(0..BRIGHT.len())];
            umbrella(g, f, p, y, col);
            lounger(g, f, p + Vec2::new(0.9, 0.4), y, FRAC_PI_2, WHITE);
            if rng.random_bool(0.6) {
                lounger(g, f, p + Vec2::new(-0.9, 0.4), y, FRAC_PI_2, WHITE);
            }
            n += 1;
        }
    }
}

/// A neon word box: a dark board with a glowing face, on a wall.
fn neon_board(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, y, 0.15, Vec3::new(w, h, 0.12), [0.05, 0.05, 0.07]);
    face.decal(g, f, Glow, t, y, 0.23, w * 0.86, h * 0.5, color);
    glow_bar(g, c, face, t, y - h * 0.5 - 0.05, w, color);
}

fn glow_bar(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, color: [f32; 3]) {
    face.bx(g, &c.frame, Glow, t, y, 0.2, Vec3::new(w, 0.07, 0.07), color);
}

fn neon_color(rng: &mut impl Rng) -> [f32; 3] {
    [[0.2, 1.5, 1.6], [1.6, 0.35, 1.2], [1.7, 1.0, 0.25], [0.4, 0.6, 1.8]][rng.random_range(0..4usize)]
}

// ── the styles ───────────────────────────────────────────────────────────

/// A beach resort: a lobby block on the street, two low wings stepping
/// down towards the water round a lit pool, cabanas and umbrellas on the
/// sand at the back, palms and string lights.
pub fn beach_resort(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, look: Option<ShoreLook>) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    // Sand and the beach things go along the back only where the water is.
    let sea_back = sea_local(c, look).y > 0.6;
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.15));
    let area = R::of(&c.poly);
    let floors = c.floors.clamp(2, 5) as i32;
    let fh = 3.3;
    let wall = [WHITE, [0.93, 0.86, 0.74], [0.95, 0.94, 0.92]][t];
    let accent = neon_color(rng);
    // Sand along the water.
    let sand_from = area.v1 - (area.d() * 0.22).clamp(5.0, 14.0);
    let beach = if sea_back { sand_strip(g, c, sand_from) } else { None };
    // The lobby along the street.
    let lobby_d = (area.d() * 0.28).clamp(8.0, 16.0);
    let lobby = fit(c, R::new(area.u0 + 1.5, area.u1 - 1.5, area.v0 + 2.0, area.v0 + 2.0 + lobby_d), 6.0);
    if let Some(r) = lobby {
        let h = floors as f32 * fh;
        block(g, c, Plaster, &r, 0.0, h, wall);
        for face in r.faces() {
            pane_row(g, c, rng, &face, 0.6, floors, fh, 2.8, 1.6, 1.8, true);
        }
        // Balconies over the courtyard.
        let back = r.back();
        let room = clear_out(c, &back, 1.6);
        for k in (1..floors).filter(|_| room) {
            let n = (back.len / 4.0).floor() as i32;
            for i in 0..n {
                let tt = -back.len * 0.5 + (i as f32 + 0.5) * back.len / n as f32;
                balcony(g, c, &back, tt, k as f32 * fh, 2.6, 1.2, wall, mul(pal.glass_dark, 1.6));
            }
        }
        parapet(g, c, &r.poly(), h, 0.9, 0.15, Glass, [0.5, 0.7, 0.75]);
        // A porte-cochère and the name in neon.
        let fr = r.front();
        canopy(g, c, &fr, 0.0, 3.6, (fr.len * 0.4).min(10.0), (c.reach_out(&fr) - 0.6).clamp(0.0, 4.0), wall);
        neon_board(g, c, &fr, 0.0, h - 1.2, (fr.len * 0.5).min(9.0), 1.4, accent);
        // A roof bar: string lights between masts.
        let corners = [Vec2::new(r.u0 + 0.6, r.v0 + 0.6), Vec2::new(r.u1 - 0.6, r.v0 + 0.6), Vec2::new(r.u1 - 0.6, r.v1 - 0.6), Vec2::new(r.u0 + 0.6, r.v1 - 0.6)];
        for p in corners {
            f.bx(g, Metal, p.x, h + 1.4, p.y, Vec3::new(0.1, 2.8, 0.1), [0.3, 0.3, 0.32]);
        }
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            string_lights(g, f, Vec3::new(a.x, h + 2.7, a.y), Vec3::new(b.x, h + 2.7, b.y), 12, WARM);
        }
        for k in 0..3 {
            table(g, f, Vec2::new(r.u0 + 2.0 + k as f32 * (r.w() - 4.0) / 2.0, r.c().y), h, pal.wood, Some(BRIGHT[k % BRIGHT.len()]));
        }
    }
    let court_v0 = lobby.map_or(area.v0 + 2.0, |r| r.v1 + 1.0);
    let court_v1 = beach.map_or(area.v1 - 1.0, |b| b.v0 - 1.0);
    if court_v1 - court_v0 < 6.0 {
        return;
    }
    // Wings down each side, stepping down a floor at a time.
    let wing_w = (area.w() * 0.2).clamp(6.0, 11.0);
    let mut inner = R::new(area.u0 + 1.0, area.u1 - 1.0, court_v0, court_v1);
    if area.w() > wing_w * 2.0 + 14.0 {
        for side in [0, 1] {
            let (u0, u1) = if side == 0 { (area.u0 + 1.0, area.u0 + 1.0 + wing_w) } else { (area.u1 - 1.0 - wing_w, area.u1 - 1.0) };
            let steps = ((court_v1 - court_v0) / 9.0).floor().clamp(1.0, 4.0) as i32;
            let len = (court_v1 - court_v0) / steps as f32;
            for s in 0..steps {
                let lv = (floors - 1 - s).max(1);
                if let Some(r) = fit(c, R::new(u0, u1, court_v0 + s as f32 * len, court_v0 + (s + 1) as f32 * len - 0.4), 4.0) {
                    let h = lv as f32 * fh;
                    block(g, c, Plaster, &r, 0.0, h, wall);
                    let inward = if side == 0 { r.right() } else { r.left() };
                    pane_row(g, c, rng, &inward, 0.6, lv, fh, 3.0, 1.6, 1.9, true);
                    // A roof terrace with a rail on each step.
                    parapet(g, c, &r.poly(), h, 0.9, 0.08, Glass, [0.55, 0.75, 0.8]);
                    if s + 1 < steps {
                        f.bx(g, Foliage, r.c().x, h + 0.25, r.c().y, Vec3::new(r.w() * 0.6, 0.5, r.d() * 0.3), mul(pal.foliage, 1.1));
                    }
                }
            }
        }
        inner = R::new(area.u0 + wing_w + 2.5, area.u1 - wing_w - 2.5, court_v0, court_v1);
    }
    // The pool in the courtyard, loungers round it, palms.
    ground(g, f, Foliage, &clip_box(&c.poly, inner.u0, inner.u1, inner.v0, inner.v1), 0.05, mul(pal.foliage, 1.05));
    let pw = (inner.w() * 0.6).clamp(4.0, 22.0);
    let pd = (inner.d() * 0.5).clamp(3.0, 16.0);
    let pc = inner.c();
    if let Some(pr) = fit(c, R::new(pc.x - pw * 0.5, pc.x + pw * 0.5, pc.y - pd * 0.5, pc.y + pd * 0.5), 3.0) {
        let apron = fit(c, pr.inset(-2.0), 2.0).unwrap_or(pr);
        f.bx(g, Paving, apron.c().x, 0.06, apron.c().y, Vec3::new(apron.w(), 0.08, apron.d()), [0.88, 0.86, 0.8]);
        pool(g, f, &pr, 0.06);
        let n = (pr.w() / 2.4).floor() as i32;
        for i in 0..n {
            let u = pr.u0 + 1.2 + i as f32 * 2.4;
            for (v, yaw) in [(pr.v0 - 1.4, FRAC_PI_2), (pr.v1 + 1.4, -FRAC_PI_2)] {
                if on_lot(c, Vec2::new(u, v), 0.6) {
                    lounger(g, f, Vec2::new(u, v), 0.1, yaw + FRAC_PI_2, [0.95, 0.95, 0.92]);
                }
            }
            if i % 3 == 1 && on_lot(c, Vec2::new(u, pr.v0 - 2.6), 0.8) {
                umbrella(g, f, Vec2::new(u, pr.v0 - 2.6), 0.1, BRIGHT[(i as usize) % BRIGHT.len()]);
            }
        }
        // String lights across the pool, palms at its corners.
        for (a, b) in [(Vec2::new(pr.u0 - 1.5, pr.v0 - 1.5), Vec2::new(pr.u1 + 1.5, pr.v1 + 1.5)), (Vec2::new(pr.u1 + 1.5, pr.v0 - 1.5), Vec2::new(pr.u0 - 1.5, pr.v1 + 1.5))] {
            for p in [a, b] {
                if on_lot(c, p, 0.5) {
                    palm(g, c, p, rng.random_range(6.0..9.0));
                }
            }
            if on_lot(c, a, 0.5) && on_lot(c, b, 0.5) {
                string_lights(g, f, Vec3::new(a.x, 4.2, a.y), Vec3::new(b.x, 4.2, b.y), 16, WARM);
            }
        }
    }
    // The beach: cabanas along the back of the sand, umbrellas in front.
    if let Some(b) = beach {
        let n = (b.w() / 7.0).floor() as i32;
        for i in 0..n.min(6) {
            let p = Vec2::new(b.u0 + 3.5 + i as f32 * 7.0, b.v0 + 1.8);
            if on_lot(c, p, 1.6) && i % 2 == 0 {
                cabana(g, f, p, 0.05, BRIGHT[(i as usize + t) % BRIGHT.len()]);
            }
        }
        umbrella_rows(g, c, rng, &R::new(b.u0, b.u1, b.v0 + 3.5, b.v1), 0.05, 10);
        for k in 0..(b.w() / 6.0).floor() as i32 {
            let p = Vec2::new(b.u0 + 1.0 + k as f32 * 6.0, b.v0 + 0.4);
            if on_lot(c, p, 0.3) {
                torch(g, f, p, 0.05);
            }
        }
    }
}

/// A hotel on the water: a tower with balconies on every floor facing
/// the sea, a lit pool on the roof, a terrace of umbrellas down to the
/// sand, a vertical neon sign.
pub fn waterfront_hotel(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, look: Option<ShoreLook>) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    // Sand and the beach things go along the back only where the water is.
    let sea_back = sea_local(c, look).y > 0.6;
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.1));
    let area = R::of(&c.poly);
    let floors = c.floors.max(3) as i32;
    let fh = 3.2;
    let deep = (area.d() * 0.5).clamp(10.0, 24.0);
    let Some(r) = fit(c, R::new(area.u0 + 2.0, area.u1 - 2.0, area.v0 + 2.0, area.v0 + 2.0 + deep), 8.0) else {
        return beach_bar(g, c, rng, look);
    };
    let h = floors as f32 * fh;
    let (skin, glass) = match t {
        0 => (WornPlaster, mix(WHITE, pick(rng, &pal.plaster), 0.4)),
        1 => (Plaster, WHITE),
        _ => (Glass, pick(rng, &pal.glass)),
    };
    block(g, c, skin, &r, 0.0, h, glass);
    for face in [r.front(), r.left(), r.right()] {
        pane_row(g, c, rng, &face, 0.8, floors, fh, 2.6, 1.6, 2.0, true);
    }
    // The sea face: a balcony per room per floor, glass rails.
    let back = r.back();
    pane_row(g, c, rng, &back, 0.8, floors, fh, 3.2, 2.2, 2.3, true);
    let room = clear_out(c, &back, 1.8);
    for k in (1..floors).filter(|_| room) {
        let n = (back.len / 3.2).floor() as i32;
        for i in 0..n {
            let tt = -back.len * 0.5 + (i as f32 + 0.5) * back.len / n as f32;
            balcony(g, c, &back, tt, k as f32 * fh, 2.9, 1.4, WHITE, [0.55, 0.78, 0.85]);
        }
    }
    // Roof: a lit pool and a glass rail.
    parapet(g, c, &r.poly(), h, 1.1, 0.1, Glass, [0.55, 0.78, 0.85]);
    let pr = R::new(r.u0 + 2.0, r.u1 - 2.0, r.c().y, r.v1 - 1.5).inset(0.3);
    if pr.w() > 3.0 && pr.d() > 2.0 {
        pool(g, f, &pr, h);
        for i in 0..((pr.w() / 2.5) as i32).min(8) {
            lounger(g, f, Vec2::new(pr.u0 + 1.0 + i as f32 * 2.5, pr.v0 - 1.6), h, PI, WHITE);
        }
    }
    // The name up the corner in neon.
    let accent = neon_color(rng);
    let side = r.right();
    let sign_h = (h * 0.5).min(18.0);
    side.bx(g, f, Matte, side.len * 0.5 - 0.6, h - sign_h * 0.5 - 1.0, 0.5, Vec3::new(1.0, sign_h, 0.3), [0.05, 0.05, 0.07]);
    side.bx(g, f, Glow, side.len * 0.5 - 0.6, h - sign_h * 0.5 - 1.0, 0.68, Vec3::new(0.7, sign_h * 0.92, 0.05), accent);
    let fr = r.front();
    canopy(g, c, &fr, 0.0, 3.4, (fr.len * 0.35).min(8.0), (c.reach_out(&fr) - 0.6).clamp(0.0, 3.5), mul(glass, 0.9));
    glow_bar(g, c, &fr, 0.0, 3.3, (fr.len * 0.35).min(8.0), accent);
    // The terrace down to the water: tables, umbrellas, palms, sand.
    let sand_from = (r.v1 + 8.0).max(area.v1 - 10.0);
    let beach = if sea_back { sand_strip(g, c, sand_from) } else { None };
    let terrace = R::new(area.u0 + 1.0, area.u1 - 1.0, r.v1 + 1.0, sand_from - 0.5);
    if terrace.d() > 3.0 {
        if let Some(tr) = fit(c, terrace, 2.0) {
            f.bx(g, Wood, tr.c().x, 0.12, tr.c().y, Vec3::new(tr.w(), 0.18, tr.d()), mul(pal.wood, 1.05));
        }
        let n = ((terrace.w() - 2.0) / 3.6).floor() as i32;
        for i in 0..n.min(10) {
            for j in 0..((terrace.d() - 1.0) / 3.2).floor().min(3.0) as i32 {
                let p = Vec2::new(terrace.u0 + 2.0 + i as f32 * 3.6, terrace.v0 + 1.8 + j as f32 * 3.2);
                if on_lot(c, p, 0.8) {
                    table(g, f, p, 0.2, pal.wood, ((i + j) % 2 == 0).then_some(BRIGHT[((i + j) as usize) % BRIGHT.len()]));
                }
            }
        }
        for p in [Vec2::new(terrace.u0 + 0.6, terrace.v1 - 0.6), Vec2::new(terrace.u1 - 0.6, terrace.v1 - 0.6)] {
            if on_lot(c, p, 0.4) {
                palm(g, c, p, rng.random_range(7.0..10.0));
            }
        }
    }
    if let Some(b) = beach {
        umbrella_rows(g, c, rng, &b, 0.05, 8);
    }
}

/// The landward half of a restaurant on a pier: a glass pavilion at the
/// water's edge with a warm interior, a forecourt with a valet stand and
/// planters. Its deck out over the water is the business's jetty
/// ([`jetty`], `JettyKind::Pier`).
pub fn pier_restaurant(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _look: Option<ShoreLook>) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Paving, mul(pal.paving, [0.95, 1.1, 1.2][t]));
    let area = R::of(&c.poly);
    let floors = c.floors.clamp(1, 2) as i32;
    let fh = 4.0;
    let h = floors as f32 * fh;
    // A pavilion, not a hangar: at most 24 m along the water.
    let half = ((area.w() - 5.0) * 0.5).min(12.0);
    let want = R::new(area.c().x - half, area.c().x + half, (area.v1 - 14.0).max(area.v0 + 6.0), area.v1 - 1.0);
    let accent = neon_color(rng);
    if let Some(r) = fit(c, want, 5.0) {
        // Slab floor, slim columns, glass all round, a thin roof that
        // overhangs.
        f.bx(g, Concrete, r.c().x, 0.2, r.c().y, Vec3::new(r.w(), 0.4, r.d()), [0.82, 0.8, 0.76]);
        f.bx(g, Glass, r.c().x, h * 0.5 + 0.2, r.c().y, Vec3::new(r.w() - 0.4, h - 0.4, r.d() - 0.4), mul(pal.glass_dark, 1.4));
        // The warm room inside, seen through the glass at night.
        f.bx(g, Glow, r.c().x, 1.6, r.c().y, Vec3::new(r.w() - 1.2, 2.2, r.d() - 1.2), mul(WARM, 0.45));
        let lid = fit(c, r.inset(-1.2), 2.0).filter(|x| x.w() >= r.w() && x.d() >= r.d()).unwrap_or(r);
        f.bx(g, Concrete, lid.c().x, h + 0.25, lid.c().y, Vec3::new(lid.w() + 0.4, 0.3, lid.d() + 0.4), WHITE);
        for (u, v) in [(r.u0, r.v0), (r.u1, r.v0), (r.u1, r.v1), (r.u0, r.v1)] {
            f.bx(g, Metal, u, h * 0.5, v, Vec3::new(0.18, h, 0.18), [0.2, 0.2, 0.22]);
        }
        let fr = r.front();
        neon_board(g, c, &fr, 0.0, h + 1.2, (fr.len * 0.6).min(10.0), 1.0, accent);
        glow_bar(g, c, &fr, 0.0, h + 0.05, fr.len + 2.0, accent);
        // Tables on a terrace round the front.
        let terrace = R::new(r.u0, r.u1, r.v0 - 6.0, r.v0 - 0.8);
        let n = (terrace.w() / 3.4).floor() as i32;
        for i in 0..n.min(8) {
            let p = Vec2::new(terrace.u0 + 1.7 + i as f32 * 3.4, terrace.c().y);
            if on_lot(c, p, 0.8) {
                table(g, f, p, 0.0, mul(pal.wood, 0.8), (i % 2 == 0).then_some(WHITE));
            }
        }
        if on_lot(c, Vec2::new(terrace.u0, terrace.v0), 0.5) && on_lot(c, Vec2::new(terrace.u1, terrace.v0), 0.5) {
            string_lights(g, f, Vec3::new(terrace.u0, 3.2, terrace.v0), Vec3::new(terrace.u1, 3.2, terrace.v0), 14, WARM);
        }
    }
    // A valet stand and planters by the street.
    let front = c.front_face(&c.poly);
    front.bx(g, f, Wood, -front.len * 0.3, 0.55, -1.0, Vec3::new(0.8, 1.1, 0.5), mul(pal.wood, 0.7));
    front.bx(g, f, Glow, -front.len * 0.3, 1.15, -1.0, Vec3::new(0.6, 0.08, 0.3), accent);
    for k in 0..4 {
        let tt = -front.len * 0.4 + k as f32 * front.len * 0.27;
        front.bx(g, f, Concrete, tt, 0.4, -0.8, Vec3::new(1.2, 0.8, 1.2), [0.3, 0.3, 0.32]);
        let p = front.at(tt, -0.8);
        if on_lot(c, p, 0.3) {
            palm(g, c, p, rng.random_range(3.5..5.0));
        }
    }
}

/// A beach bar: a shack with a thatched roof and a bar along the water
/// side, stools, a deck, tiki torches, string lights, loungers and
/// umbrellas on the sand and a rack of surfboards.
pub fn beach_bar(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, _look: Option<ShoreLook>) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.25, Dirt, SAND);
    let area = R::of(&c.poly);
    let accent = neon_color(rng);
    let sw = (area.w() * 0.45).clamp(5.0, 12.0);
    let sd = (area.d() * 0.25).clamp(4.0, 8.0);
    let mid = Vec2::new(area.c().x, area.v0 + area.d() * 0.42);
    let thatch = [0.62, 0.5, 0.3];
    if let Some(r) = fit(c, R::new(mid.x - sw * 0.5, mid.x + sw * 0.5, mid.y - sd * 0.5, mid.y + sd * 0.5), 3.0) {
        // Deck all round.
        if let Some(d) = fit(c, R::new(r.u0 - 2.5, r.u1 + 2.5, r.v0 - 2.0, r.v1 + 5.0), 2.0) {
            f.bx(g, Wood, d.c().x, 0.15, d.c().y, Vec3::new(d.w(), 0.3, d.d()), mul(pal.wood, 1.1));
        }
        block(g, c, Wood, &r, 0.3, 3.0, mul(pal.wood, 0.75));
        // A thatched roof, overhanging.
        let roof = fit(c, r.inset(-1.6), 2.0).filter(|x| x.w() >= r.w() && x.d() >= r.d()).unwrap_or(r);
        loft(g, f, Foliage, &roof.poly(), &r.inset(r.w().min(r.d()) * 0.35).poly(), 3.0, 4.6, thatch);
        // The bar on the water side, stools in front.
        let bk = r.back();
        let bar_room = c.reach_out(&bk) > 2.6;
        if bar_room {
            bk.bx(g, f, Wood, 0.0, 0.85, 0.7, Vec3::new(bk.len - 0.6, 1.1, 0.6), mul(pal.wood, 0.6));
            bk.bx(g, f, Glow, 0.0, 1.43, 0.7, Vec3::new(bk.len - 0.8, 0.05, 0.5), accent);
            let n = (bk.len / 1.2).floor() as i32;
            for i in 0..n {
                let p = bk.at(-bk.len * 0.5 + 0.6 + i as f32 * 1.2, 1.7);
                f.bx(g, Metal, p.x, 0.55, p.y, Vec3::new(0.08, 0.8, 0.08), [0.25, 0.25, 0.27]);
                g.part(Wood).cylinder(f.to_world(p.x, p.y), 0.22, 0.22, 0.95, 1.02, 8, rgba(mul(pal.wood, 0.9)));
            }
        }
        neon_board(g, c, &r.front(), 0.0, 3.6, (r.w() * 0.7).min(6.0), 0.9, accent);
        // String lights from the eaves out to poles on the sand.
        for (k, corner) in [Vec2::new(roof.u0, roof.v1), Vec2::new(roof.u1, roof.v1)].into_iter().enumerate() {
            let pole = corner + Vec2::new(if k == 0 { -3.0 } else { 3.0 }, 5.0);
            if on_lot(c, pole, 0.5) {
                f.bx(g, Wood, pole.x, 1.6, pole.y, Vec3::new(0.12, 3.2, 0.12), [0.35, 0.22, 0.12]);
                string_lights(g, f, Vec3::new(corner.x, 3.0, corner.y), Vec3::new(pole.x, 3.1, pole.y), 9, WARM);
            }
        }
        // Torches round the deck.
        for p in [Vec2::new(roof.u0 - 1.4, roof.v0), Vec2::new(roof.u1 + 1.4, roof.v0), Vec2::new(roof.u0 - 1.4, roof.v1 + 2.0), Vec2::new(roof.u1 + 1.4, roof.v1 + 2.0)] {
            if on_lot(c, p, 0.3) {
                torch(g, f, p, 0.3);
            }
        }
        // Surfboards in a rack by the side.
        for i in 0..5 {
            let p = Vec2::new(roof.u1 + 1.0, r.v0 + i as f32 * 0.5);
            if on_lot(c, p, 0.3) {
                f.bx_yaw(g, Paint, p.x, 1.1, p.y, Vec3::new(0.55, 2.1, 0.08), BRIGHT[(i + 2) % BRIGHT.len()], FRAC_PI_2);
            }
        }
    }
    let sand = R::new(area.u0 + 0.5, area.u1 - 0.5, mid.y + sd * 0.5 + 6.0, area.v1);
    umbrella_rows(g, c, rng, &sand, 0.05, 8);
    for _ in 0..2 {
        if let Some(p) = c.random_inside(rng, 1.5) {
            palm(g, c, p, rng.random_range(5.0..8.0));
        }
    }
}

/// A boat rental: a kiosk with an awning, kayaks on racks, pedalos and
/// paddle boards on the sand, lifebuoys and a flag. The pontoon with its
/// boats is the business's jetty ([`jetty`], `JettyKind::Pontoon`).
pub fn boat_rental(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, look: Option<ShoreLook>) {
    let f = &c.frame;
    let pal = c.palette;
    // Sand and the beach things go along the back only where the water is.
    let sea_back = sea_local(c, look).y > 0.6;
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.05));
    let area = R::of(&c.poly);
    let sand = if sea_back { sand_strip(g, c, area.c().y) } else { Some(R::new(area.u0 + 0.8, area.u1 - 0.8, area.c().y, area.v1 - 0.8)) };
    let accent = neon_color(rng);
    let k = fit(c, R::new(area.c().x - 3.0, area.c().x + 3.0, area.v0 + 3.0, area.v0 + 7.0), 2.5);
    if let Some(r) = k {
        block(g, c, Paint, &r, 0.0, 3.0, pick(rng, &pal.metal_paint));
        f.bx(g, Metal, r.c().x, 3.15, r.c().y, Vec3::new(r.w() + 0.6, 0.3, r.d() + 0.6), WHITE);
        let fr = r.front();
        fr.glaze(g, c, 0.0, 1.6, 0.05, r.w() * 0.6, 1.0, Pane::Dark);
        neon_board(g, c, &fr, 0.0, 3.8, (r.w() * 0.9).min(5.5), 0.8, accent);
        if c.reach_out(&r.back()) > 2.5 {
            awning(g, c, rng, &r.back(), 0.0, 2.5, r.w());
        }
        let pole = Vec2::new(r.u1 + 1.5, r.v0);
        if on_lot(c, pole, 1.0) {
            flagpole(g, f, pole, 7.0, BRIGHT[rng.random_range(0..BRIGHT.len())], false);
        }
    }
    // Kayak racks and boats laid out on the sand.
    if let Some(s) = sand {
        let n = ((s.w() - 2.0) / 3.0).floor() as i32;
        for i in 0..n.min(8) {
            let u = s.u0 + 1.5 + i as f32 * 3.0;
            let p = Vec2::new(u, s.v0 + 2.0);
            if !on_lot(c, p, 2.2) {
                continue;
            }
            if i % 2 == 0 {
                // A rack of three kayaks.
                for lvl in 0..3 {
                    f.bx(g, Paint, p.x, 0.5 + lvl as f32 * 0.5, p.y, Vec3::new(0.6, 0.3, 3.6), BRIGHT[(i as usize + lvl) % BRIGHT.len()]);
                }
                f.bx(g, Metal, p.x, 0.85, p.y - 1.2, Vec3::new(0.9, 1.7, 0.08), [0.3, 0.3, 0.32]);
                f.bx(g, Metal, p.x, 0.85, p.y + 1.2, Vec3::new(0.9, 1.7, 0.08), [0.3, 0.3, 0.32]);
            } else {
                // A pedalo: a hull, two seats and a slide.
                let q = p + Vec2::new(0.0, 3.0);
                if !on_lot(c, q, 1.8) {
                    continue;
                }
                f.bx(g, Paint, q.x, 0.35, q.y, Vec3::new(1.6, 0.5, 2.6), BRIGHT[(i as usize * 3) % BRIGHT.len()]);
                f.bx(g, Matte, q.x, 0.75, q.y - 0.3, Vec3::new(1.2, 0.3, 0.6), WHITE);
                f.bx_yaw(g, Paint, q.x, 1.0, q.y + 0.9, Vec3::new(0.5, 0.06, 1.6), [0.95, 0.85, 0.2], 0.0);
            }
        }
        // Lifebuoys on posts.
        for i in 0..2 {
            let p = Vec2::new(s.u0 + 1.0 + i as f32 * (s.w() - 2.0), s.v1 - 1.0);
            if on_lot(c, p, 0.3) {
                f.bx(g, Wood, p.x, 0.9, p.y, Vec3::new(0.12, 1.8, 0.12), pal.wood);
                g.part(Paint).cylinder(f.to_world(p.x, p.y), 0.45, 0.45, 1.3, 1.45, 10, rgba([0.95, 0.4, 0.15]));
            }
        }
    }
}

// ── public beaches (world coordinates) ────────────────────────────────────

/// A public beach: sand over the lot, the boardwalk down to the water,
/// a towel at every spot, umbrellas and loungers among them, the crowd of
/// the hour lying in the sun, swimmers off the shore, a lifeguard tower
/// at the water's edge, and when it is fouled a sign and tape across the
/// way in.
pub fn beach(g: &mut Geometry, c: &Ctx, look: Option<ShoreLook>) {
    // The sim plans the sand (`plan_beach`); without a plan, bare sand.
    let (paths, benches, entrances): (&[Vec<Vec2>], &[(Vec2, f32)], &[Vec2]) = c.park.as_ref().map_or((&[], &[], &[]), |l| (&l.paths[..], &l.benches[..], &l.entrances[..]));
    let f = &c.frame;
    let pal = c.palette;
    let state = look.and_then(|l| l.beach).unwrap_or_default();
    let drop = look.map_or(1.5, |l| l.drop.max(0.3));
    let lot: Vec<Vec2> = c.lot_poly.iter().map(|p| f.to_world(p.x, p.y)).collect();
    let sea = look.map(|l| l.seaward).filter(|s| *s != Vec2::ZERO).unwrap_or_else(|| {
        let s = sea_local(c, None);
        f.along * s.x + f.inward * s.y
    });
    let across = Vec2::new(-sea.y, sea.x);
    // [beaches] (H2) One of several lots of a merged beach: the sand runs
    // over the seams (no inset), only the anchor has the lifeguard and the
    // sign.
    let (merged, anchor) = c.park.as_ref().map_or((false, true), |l| (l.cluster_lots > 1, l.anchor));
    // Sand.
    let inner = if merged { c.lot_poly.to_vec() } else { offset_poly(&c.lot_poly, 0.2) };
    ground(g, f, Dirt, &inner, 0.05, SAND);
    // Wet sand along the water.
    let (v0, v1) = lot.iter().map(|p| p.dot(sea)).fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let (u0, u1) = lot.iter().map(|p| p.dot(across)).fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let wet: Vec<Vec2> = c.lot_poly.iter().map(|p| f.to_world(p.x, p.y)).filter(|w| w.dot(sea) > v1 - 5.0).collect();
    if wet.len() >= 2 {
        let world_strip: Vec<Vec2> = clip_box(&lot.iter().map(|w| Vec2::new(w.dot(across), w.dot(sea))).collect::<Vec<_>>(), u0, u1, v1 - 4.0, v1).into_iter().map(|q| across * q.x + sea * q.y).collect();
        if world_strip.len() >= 3 {
            g.part(Dirt).ground_polygon(&world_strip, 0.06, rgba(mul(SAND, 0.72)));
        }
    }
    // The boardwalk.
    for path in paths {
        if path.len() >= 2 {
            g.part(Wood).path_strip(path, 0.0, 2.2, 0.16, rgba(mul(pal.wood, 1.05)));
            // Low glowing bollards along it.
            for seg in path.windows(2) {
                let len = seg[0].distance(seg[1]);
                let d = (seg[1] - seg[0]).normalize_or_zero();
                let side = Vec2::new(-d.y, d.x);
                let mut s = 3.0;
                while s < len {
                    for k in [-1.0f32, 1.0] {
                        let p = seg[0] + d * s + side * k * 1.5;
                        g.part(Matte).boxed(Vec3::new(p.x, 0.35, p.y), Vec3::new(0.18, 0.7, 0.18), 0.0, rgba([0.12, 0.12, 0.14]));
                        g.part(Glow).boxed(Vec3::new(p.x, 0.68, p.y), Vec3::new(0.2, 0.08, 0.2), 0.0, rgba(if state.night { [0.3, 1.4, 1.5] } else { [0.15, 0.6, 0.65] }));
                    }
                    s += 7.0;
                }
            }
        }
    }
    let seed = c.seed;
    beach_crowd(g, benches, seed, state.bathers as usize, state.night);
    beach_water(g, look.is_some(), &state, seed, drop, sea, across, (u0, u1), v1);
    if anchor {
        lifeguard_tower(g, pal, &state, &lot, sea, across, (u0, u1), v1);
    }
    if state.closed && anchor {
        beach_closed(g, paths, entrances, &lot, sea, across);
    }
    // A few palms at the back of the sand on a warm beach.
    for i in 0..((u1 - u0) / 14.0) as i32 {
        let h = hash(seed ^ 0x9A1, i as u64);
        let p = across * (u0 + 4.0 + i as f32 * 14.0 + (h % 4) as f32) + sea * (v0 + 3.0);
        if nd_sim::zoning::point_in_polygon(p, &lot) && nd_sim::geom::distance_to_outline(p, &lot) > 4.2 && paths.iter().all(|path| path.windows(2).all(|s| nd_sim::geom::project_on_segment(p, s[0], s[1]).1.distance(p) > 3.0)) {
            tree_kind(g, p, 0.0, TreeKind::Palm, 6.5 + (h % 3) as f32, 3.6, pal.bark, mul(pal.foliage, 1.1));
        }
    }
}

/// A beach's towels, umbrellas and loungers, and `bathers` of the crowd
/// lying on them. [beaches] (H2) After dark the umbrellas and towels are
/// packed away: only the loungers stay out, stacked in pairs.
fn beach_crowd(g: &mut Geometry, benches: &[(Vec2, f32)], seed: u64, bathers: usize, night: bool) {
    let mut lying = bathers;
    for (i, (p, yaw)) in benches.iter().enumerate() {
        let h = hash(seed, i as u64);
        let col = BRIGHT[(h % BRIGHT.len() as u64) as usize];
        let along = Vec2::new(yaw.cos(), yaw.sin());
        let towel_yaw = yaw + FRAC_PI_2;
        if night {
            if h % 5 == 2 {
                for k in 0..2 {
                    let y = 0.32 + k as f32 * 0.14;
                    g.part(Matte).boxed(Vec3::new(p.x, y, p.y), Vec3::new(0.65, 0.08, 1.9), towel_yaw, rgba(WHITE));
                }
            }
            if lying > 0 && h % 7 != 3 {
                lying -= 1;
                let skin = SKIN[((h >> 16) % 4) as usize];
                g.part(Matte).boxed(Vec3::new(p.x, 0.18, p.y), Vec3::new(0.4, 0.2, 1.55), towel_yaw, rgba(skin));
            }
            continue;
        }
        match h % 5 {
            0 | 1 => {
                // An umbrella over a towel.
                g.part(Matte).boxed(Vec3::new(p.x - along.x * 0.4, 1.1, p.y - along.y * 0.4), Vec3::new(0.05, 2.2, 0.05), 0.0, rgba(WHITE));
                g.part(Tarp).cylinder(*p - along * 0.4, 1.3, 0.08, 1.95, 2.35, 8, rgba(BRIGHT[((h >> 8) % BRIGHT.len() as u64) as usize]));
                g.part(Paint).boxed(Vec3::new(p.x, 0.08, p.y), Vec3::new(0.9, 0.02, 1.9), towel_yaw, rgba(col));
            }
            2 => {
                // A lounger.
                g.part(Matte).boxed(Vec3::new(p.x, 0.32, p.y), Vec3::new(0.65, 0.08, 1.9), towel_yaw, rgba(WHITE));
                g.part(Matte).boxed(Vec3::new(p.x - along.x * 0.75, 0.5, p.y - along.y * 0.75), Vec3::new(0.65, 0.06, 0.45), towel_yaw, rgba(WHITE));
            }
            _ => {
                g.part(Paint).boxed(Vec3::new(p.x, 0.08, p.y), Vec3::new(0.9, 0.02, 1.9), towel_yaw, rgba(col));
            }
        }
        if lying > 0 && h % 7 != 3 {
            lying -= 1;
            let skin = SKIN[((h >> 16) % 4) as usize];
            let suit = BRIGHT[((h >> 20) % BRIGHT.len() as u64) as usize];
            let y = if h % 5 == 2 { 0.42 } else { 0.18 };
            g.part(Matte).boxed(Vec3::new(p.x, y, p.y), Vec3::new(0.4, 0.2, 1.55), towel_yaw, rgba(skin));
            g.part(Matte).boxed(Vec3::new(p.x + along.x * 0.05, y + 0.06, p.y + along.y * 0.05), Vec3::new(0.42, 0.18, 0.4), towel_yaw, rgba(suit));
            g.part(Matte).boxed(Vec3::new(p.x - along.x * 0.85, y + 0.06, p.y - along.y * 0.85), Vec3::new(0.22, 0.2, 0.24), towel_yaw, rgba(skin));
        }
    }
}

/// Swimmers off the shore and the buoys of the swimming area (only on a
/// real shore, with the water off the lot).
#[allow(clippy::too_many_arguments)]
fn beach_water(g: &mut Geometry, shore: bool, state: &BeachLook, seed: u64, drop: f32, sea: Vec2, across: Vec2, (u0, u1): (f32, f32), v1: f32) {
    // (Only on a real shore: the water is off the lot.)
    let n = if shore { state.swimmers.min(60) as usize } else { 0 };
    for i in 0..n {
        let h = hash(seed ^ 0x5717, i as u64);
        let s = ((h % 1000) as f32 / 1000.0) * (u1 - u0 - 6.0) + u0 + 3.0;
        let out = 3.0 + ((h >> 10) % 1000) as f32 / 1000.0 * 22.0;
        let p = across * s + sea * (v1 + out);
        let skin = SKIN[((h >> 24) % 4) as usize];
        let y = -drop;
        g.part(Matte).boxed(Vec3::new(p.x, y + 0.08, p.y), Vec3::new(0.32, 0.18, 0.32), 0.0, rgba(skin));
        if h % 3 == 0 {
            // A float ring.
            g.part(Paint).cylinder(p, 0.55, 0.55, y - 0.05, y + 0.08, 10, rgba(BRIGHT[((h >> 30) % BRIGHT.len() as u64) as usize]));
        }
    }
    // Buoys marking the swimming area.
    let buoys = if shore { ((u1 - u0) / 9.0).floor() as i32 } else { -1 };
    for i in 0..=buoys {
        let p = across * (u0 + i as f32 * (u1 - u0) / buoys.max(1) as f32) + sea * (v1 + 30.0);
        g.part(Paint).cylinder(p, 0.3, 0.3, -drop - 0.1, -drop + 0.35, 8, rgba([0.95, 0.75, 0.1]));
        if state.night {
            g.part(Glow).boxed(Vec3::new(p.x, -drop + 0.45, p.y), Vec3::splat(0.12), 0.0, rgba([1.6, 1.0, 0.2]));
        }
    }
}

/// The lifeguard tower near the water, in the middle of the beach.
#[allow(clippy::too_many_arguments)]
fn lifeguard_tower(g: &mut Geometry, pal: &crate::palette::Palette, state: &BeachLook, lot: &[Vec2], sea: Vec2, across: Vec2, (u0, u1): (f32, f32), v1: f32) {
    let tower = across * ((u0 + u1) * 0.5 + (u1 - u0) * 0.18) + sea * (v1 - 7.0);
    if nd_sim::zoning::point_in_polygon(tower, lot) && nd_sim::geom::distance_to_outline(tower, lot) > 2.0 {
        let yaw = across.y.atan2(across.x);
        for (du, dv) in [(-0.9f32, -0.9f32), (0.9, -0.9), (0.9, 0.9), (-0.9, 0.9)] {
            let q = tower + across * du + sea * dv;
            g.part(Wood).boxed(Vec3::new(q.x, 1.2, q.y), Vec3::new(0.14, 2.4, 0.14), yaw, rgba(WHITE));
        }
        g.part(Wood).boxed(Vec3::new(tower.x, 2.45, tower.y), Vec3::new(2.4, 0.15, 2.4), yaw, rgba(WHITE));
        g.part(Paint).boxed(Vec3::new(tower.x, 3.25, tower.y), Vec3::new(2.0, 1.5, 2.0), yaw, rgba([0.9, 0.2, 0.15]));
        g.part(Paint).boxed(Vec3::new(tower.x, 4.15, tower.y), Vec3::new(2.5, 0.25, 2.5), yaw, rgba(WHITE));
        let slat = tower + sea * 1.01;
        g.part(Glass).boxed(Vec3::new(slat.x, 3.4, slat.y), Vec3::new(1.6, 0.6, 0.04), yaw, rgba(pal.glass_dark));
        g.part(Matte).boxed(Vec3::new(tower.x + across.x * 1.1, 5.0, tower.y + across.y * 1.1), Vec3::new(0.05, 1.6, 0.05), 0.0, rgba(WHITE));
        let flag = if state.closed { [0.95, 0.12, 0.1] } else { [0.95, 0.8, 0.1] };
        g.part(Tarp).boxed(Vec3::new(tower.x + across.x * 1.45, 5.55, tower.y + across.y * 1.45), Vec3::new(0.7, 0.45, 0.03), yaw, rgba(flag));
        if state.night {
            g.part(Glow).boxed(Vec3::new(tower.x, 4.45, tower.y), Vec3::new(0.3, 0.3, 0.3), 0.0, rgba([1.5, 1.2, 0.8]));
        }
        // A ramp up.
        let foot = tower - sea * 3.0;
        g.part(Wood).slab(Vec3::new(foot.x, 0.1, foot.y), Vec3::new(tower.x - sea.x, 2.45, tower.y - sea.y), 0.8, 0.08, rgba(mul(pal.wood, 1.1)));
    }
}

/// A shut beach: a sign at the way in and tape across it.
fn beach_closed(g: &mut Geometry, paths: &[Vec<Vec2>], entrances: &[Vec2], lot: &[Vec2], sea: Vec2, across: Vec2) {
    let entry = entrances.first().copied().or_else(|| paths.first().and_then(|p| p.first().copied())).unwrap_or(lot[0]);
    let post = entry + across * 1.8;
    g.part(Metal).boxed(Vec3::new(post.x, 1.2, post.y), Vec3::new(0.12, 2.4, 0.12), 0.0, rgba([0.3, 0.3, 0.32]));
    let yaw = across.y.atan2(across.x);
    g.part(Paint).boxed(Vec3::new(post.x, 2.3, post.y), Vec3::new(1.6, 1.0, 0.06), yaw, rgba([0.92, 0.9, 0.85]));
    g.part(Glow).boxed(Vec3::new(post.x - sea.x * 0.05, 2.45, post.y - sea.y * 0.05), Vec3::new(1.4, 0.3, 0.07), yaw, rgba([1.8, 0.15, 0.1]));
    g.part(Glow).boxed(Vec3::new(post.x - sea.x * 0.05, 2.05, post.y - sea.y * 0.05), Vec3::new(1.0, 0.12, 0.07), yaw, rgba([1.8, 0.15, 0.1]));
    // Tape between posts across the boardwalk.
    let a = entry - across * 2.5;
    let b = entry + across * 2.5;
    for q in [a, b] {
        g.part(Matte).boxed(Vec3::new(q.x, 0.5, q.y), Vec3::new(0.1, 1.0, 0.1), 0.0, rgba([0.95, 0.4, 0.1]));
    }
    let n = 8;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let q = a.lerp(b, t0 + 0.5 / n as f32);
        let col = if i % 2 == 0 { [0.95, 0.15, 0.1] } else { [0.95, 0.92, 0.88] };
        g.part(Paint).boxed(Vec3::new(q.x, 0.9, q.y), Vec3::new(a.distance(b) / n as f32, 0.08, 0.02), yaw, rgba(col));
    }
}

// ── private jetties (world coordinates, absolute heights) ────────────────

/// A business's own jetty off its lot: decking on piles from the shore out
/// over the water, with what its kind needs at the end: a trawler moored
/// alongside, a quay with a crane and a coaster, a restaurant's deck of
/// tables under string lights with a pavilion at the end, or a pontoon of
/// small boats. `deck` is the height of the decking, `water` of the water's
/// surface; `night` lights the lamps.
/// `boat`: draw the trawler or coaster alongside. [sweep-visual] Off when
/// the renderer draws it as a moving boat (a fishery's trawler sailing
/// with the fleets, a quay's coaster coming and going).
pub fn jetty(j: &Jetty, deck: f32, water: f32, night: bool, seed: u64, boat_alongside: bool) -> Geometry {
    let mut g = Geometry::default();
    let (_, w) = j.kind.size();
    let a = j.root;
    let b = j.tip;
    let len = a.distance(b).max(1.0);
    let d = (b - a) / len;
    let side = Vec2::new(-d.y, d.x);
    let yaw = d.y.atan2(d.x);
    let wood = [0.42, 0.3, 0.2];
    let dark = [0.16, 0.14, 0.13];
    let top = match j.kind {
        JettyKind::Pontoon => water + 0.35,
        _ => deck.max(water + 1.2),
    };
    let mid = (a + b) * 0.5;
    let surface = if j.kind == JettyKind::Cargo { Concrete } else { Wood };
    let deck_col = if j.kind == JettyKind::Cargo { [0.55, 0.54, 0.5] } else { wood };
    g.part(surface).boxed(Vec3::new(mid.x, top - 0.12, mid.y), Vec3::new(len, 0.24, w), yaw, rgba(deck_col));
    // Piles every few metres along both edges (pontoons float on drums).
    let n = (len / 3.5).ceil() as i32;
    for i in 0..=n {
        let p = a + d * (i as f32 * len / n as f32);
        for k in [-1.0f32, 1.0] {
            let q = p + side * k * (w * 0.5 - 0.15);
            if j.kind == JettyKind::Pontoon {
                g.part(Matte).boxed(Vec3::new(q.x, water + 0.1, q.y), Vec3::new(0.6, 0.35, 0.5), yaw, rgba(dark));
            } else {
                g.part(Wood).cylinder(q, 0.16, 0.16, water - 2.0, top + 0.35, 6, rgba(mul(wood, 0.7)));
            }
        }
    }
    // A lamp at the end.
    let lamp = b - d * 0.5 + side * (w * 0.5 - 0.3);
    g.part(Metal).cylinder(lamp, 0.06, 0.06, top, top + 3.2, 6, rgba([0.25, 0.25, 0.27]));
    g.part(Glow).boxed(Vec3::new(lamp.x, top + 3.3, lamp.y), Vec3::splat(0.3), 0.0, rgba(if night { [1.6, 1.2, 0.7] } else { [0.6, 0.5, 0.4] }));
    match j.kind {
        JettyKind::Fishing => {
            // Bollards, crates, a net pile, and a trawler alongside.
            for i in 0..3 {
                let p = a + d * (len * (0.3 + 0.25 * i as f32)) + side * (w * 0.5 - 0.3);
                g.part(Metal).cylinder(p, 0.18, 0.15, top, top + 0.5, 8, rgba(dark));
            }
            let crate_at = a + d * (len * 0.45) - side * 0.6;
            for k in 0..3 {
                g.part(Paint).boxed(Vec3::new(crate_at.x + d.x * k as f32 * 0.7, top + 0.2 + (k % 2) as f32 * 0.4, crate_at.y + d.y * k as f32 * 0.7), Vec3::new(0.6, 0.38, 0.45), yaw, rgba([0.2, 0.45, 0.75]));
            }
            if boat_alongside {
                boat(&mut g, j.berth, d, water, 14.0, 4.4, [0.75, 0.2, 0.15], true, night);
            }
        }
        JettyKind::Cargo => {
            // A small crane, stacked crates and a coaster alongside.
            let base = a + d * (len * 0.6) - side * (w * 0.25);
            g.part(Metal).cylinder(base, 0.5, 0.4, top, top + 7.0, 8, rgba([0.9, 0.65, 0.1]));
            let jib_end = base + side * 9.0 + d * 1.0;
            g.part(Metal).beam(Vec3::new(base.x, top + 7.0, base.y), Vec3::new(jib_end.x, top + 8.0, jib_end.y), 0.35, rgba([0.9, 0.65, 0.1]));
            g.part(Matte).wire(Vec3::new(jib_end.x, top + 8.0, jib_end.y), Vec3::new(jib_end.x, top + 2.0, jib_end.y), 0.0, 0.04, rgba(dark));
            let stack = a + d * (len * 0.3);
            let cols = [[0.75, 0.25, 0.2], [0.2, 0.45, 0.7], [0.85, 0.7, 0.2], [0.3, 0.6, 0.35]];
            for k in 0..4 {
                let q = stack + side * ((k % 2) as f32 * 2.6 - 1.3);
                g.part(Container).boxed(Vec3::new(q.x, top + CONTAINER_H * 0.5 + (k / 2) as f32 * CONTAINER_H, q.y), Vec3::new(CONTAINER_20, CONTAINER_H, CONTAINER_W), yaw, rgba(cols[(seed as usize + k) % 4]));
            }
            if boat_alongside {
                boat(&mut g, j.berth, d, water, 30.0, 8.0, [0.2, 0.25, 0.32], false, night);
            }
        }
        JettyKind::Pier => {
            // Rails, tables under umbrellas, string lights on posts, and a
            // glass pavilion at the end glowing warm.
            for k in [-1.0f32, 1.0] {
                let p0 = a + side * k * (w * 0.5 - 0.1);
                let p1 = b + side * k * (w * 0.5 - 0.1);
                g.part(Wood).beam(Vec3::new(p0.x, top + 1.0, p0.y), Vec3::new(p1.x, top + 1.0, p1.y), 0.08, rgba(mul(wood, 1.2)));
                let posts = (len / 4.0).ceil() as i32;
                let mut prev: Option<Vec3> = None;
                for i in 0..=posts {
                    let q = p0.lerp(p1, i as f32 / posts as f32);
                    g.part(Wood).boxed(Vec3::new(q.x, top + 1.6, q.y), Vec3::new(0.12, 3.2, 0.12), yaw, rgba(mul(wood, 0.8)));
                    let head = Vec3::new(q.x, top + 3.1, q.y);
                    if let Some(pv) = prev {
                        g.part(Matte).wire(pv, head, 0.45, 0.025, rgba(dark));
                        for s in 1..5 {
                            let t = s as f32 / 5.0;
                            let p = pv.lerp(head, t) - Vec3::Y * (0.45 * 4.0 * t * (1.0 - t) + 0.12);
                            g.part(Glow).boxed(p, Vec3::splat(0.14), 0.0, rgba(WARM));
                        }
                    }
                    prev = Some(head);
                }
            }
            let pav_len = (len * 0.35).clamp(4.0, 8.0);
            let pav = b - d * (pav_len * 0.5 + 0.3);
            g.part(Glass).boxed(Vec3::new(pav.x, top + 1.6, pav.y), Vec3::new(pav_len, 3.2, w - 1.0), yaw, rgba([0.25, 0.35, 0.42]));
            g.part(Glow).boxed(Vec3::new(pav.x, top + 1.3, pav.y), Vec3::new(pav_len - 0.6, 2.0, w - 1.8), yaw, rgba(mul(WARM, 0.5)));
            g.part(Concrete).boxed(Vec3::new(pav.x, top + 3.35, pav.y), Vec3::new(pav_len + 1.4, 0.25, w + 0.6), yaw, rgba(WHITE));
            let rows = ((len - pav_len - 2.0) / 3.2).floor().max(0.0) as i32;
            for i in 0..rows {
                for k in [-1.0f32, 1.0] {
                    let p = a + d * (2.0 + i as f32 * 3.2) + side * k * (w * 0.25);
                    let col = if (i + k as i32) % 2 == 0 { Some(WHITE) } else { None };
                    world_table(&mut g, p, top, wood, col);
                }
            }
        }
        JettyKind::Pontoon => {
            // Finger berths and small boats on both sides.
            let fingers = (len / 4.0).floor() as i32;
            for i in 0..fingers {
                for k in [-1.0f32, 1.0] {
                    let base = a + d * (2.0 + i as f32 * 4.0) + side * k * (w * 0.5);
                    let tip = base + side * k * 4.0;
                    let m = (base + tip) * 0.5;
                    g.part(Wood).boxed(Vec3::new(m.x, water + 0.25, m.y), Vec3::new(0.8, 0.18, 4.0), yaw, rgba(wood));
                    let h = hash(seed, (i * 2 + if k > 0.0 { 1 } else { 0 }) as u64);
                    if h % 4 != 0 {
                        let p = m + d * 1.4;
                        let col = BRIGHT[(h % BRIGHT.len() as u64) as usize];
                        boat(&mut g, p, side * k, water, 4.2, 1.6, col, false, false);
                    }
                }
            }
        }
    }
    g
}

/// A café table and chairs on a deck at a world point.
fn world_table(g: &mut Geometry, p: Vec2, y: f32, wood: [f32; 3], umbrella: Option<[f32; 3]>) {
    g.part(Wood).boxed(Vec3::new(p.x, y + 0.72, p.y), Vec3::new(0.8, 0.06, 0.8), 0.0, rgba(mul(wood, 1.2)));
    g.part(Matte).boxed(Vec3::new(p.x, y + 0.36, p.y), Vec3::new(0.08, 0.72, 0.08), 0.0, rgba([0.15, 0.15, 0.16]));
    for s in [-0.7f32, 0.7] {
        g.part(Wood).boxed(Vec3::new(p.x + s, y + 0.45, p.y), Vec3::new(0.42, 0.06, 0.42), 0.0, rgba(wood));
    }
    if let Some(col) = umbrella {
        g.part(Matte).boxed(Vec3::new(p.x, y + 1.3, p.y), Vec3::new(0.05, 1.2, 0.05), 0.0, rgba([0.8, 0.8, 0.78]));
        g.part(Tarp).cylinder(p, 1.25, 0.1, y + 1.95, y + 2.35, 8, rgba(col));
    }
}

/// A boat at a world point heading along `dir`: a hull, a wheelhouse for a
/// working boat, a mast with a light.
#[allow(clippy::too_many_arguments)]
fn boat(g: &mut Geometry, at: Vec2, dir: Vec2, water: f32, len: f32, beam: f32, hull: [f32; 3], wheelhouse: bool, night: bool) {
    let yaw = dir.y.atan2(dir.x);
    g.part(Paint).boxed(Vec3::new(at.x, water + 0.4, at.y), Vec3::new(len, 1.3, beam), yaw, rgba(hull));
    g.part(Paint).boxed(Vec3::new(at.x + dir.x * len * 0.52, water + 0.5, at.y + dir.y * len * 0.52), Vec3::new(len * 0.08, 1.1, beam * 0.6), yaw, rgba(hull));
    g.part(Matte).boxed(Vec3::new(at.x, water + 1.08, at.y), Vec3::new(len * 0.95, 0.06, beam * 0.92), yaw, rgba([0.85, 0.84, 0.8]));
    if wheelhouse {
        let wh = at - dir * len * 0.15;
        g.part(Paint).boxed(Vec3::new(wh.x, water + 2.2, wh.y), Vec3::new(len * 0.28, 2.2, beam * 0.7), yaw, rgba([0.92, 0.92, 0.9]));
        g.part(Glass).boxed(Vec3::new(wh.x + dir.x * len * 0.141, water + 2.6, wh.y + dir.y * len * 0.141), Vec3::new(0.05, 0.7, beam * 0.6), yaw, rgba([0.2, 0.28, 0.33]));
        let mast = at + dir * len * 0.2;
        g.part(Metal).cylinder(mast, 0.1, 0.08, water + 1.1, water + 7.5, 6, rgba([0.3, 0.3, 0.32]));
        g.part(Metal).beam(Vec3::new(mast.x, water + 6.5, mast.y), Vec3::new(at.x - dir.x * len * 0.45, water + 1.3, at.y - dir.y * len * 0.45), 0.06, rgba([0.3, 0.3, 0.32]));
        g.part(Glow).boxed(Vec3::new(mast.x, water + 7.6, mast.y), Vec3::splat(0.22), 0.0, rgba(if night { [1.6, 1.4, 1.0] } else { [0.6, 0.55, 0.5] }));
    }
}

/// On the lot, at least `margin` in from its edge (lot frame).
fn on_lot(c: &Ctx, p: Vec2, margin: f32) -> bool {
    c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) >= margin
}

/// [waterfront] What the looks need to know of the shore for a building:
/// where the water is off its lot and how far below the pad, and for a
/// public beach its crowd, swimmers, whether it is shut, and the night.
/// None inland.
pub fn shore_look(world: &nd_sim::World, id: nd_sim::BuildingId) -> Option<ShoreLook> {
    let b = world.buildings.get(id)?;
    let lot = world.lots.get(b.lot)?;
    if !lot.shore.is_waterfront() && !lot.shore.beach {
        return None;
    }
    let drop = (world.lot_pad(b.lot) - nd_sim::landscape::WATER_LEVEL).max(0.0);
    let beach = lot.shore.beach.then(|| {
        // [beaches] (H2) A lot of a merged beach draws its share of the
        // beach's crowd, by its sand.
        let s = world.shore.beach(id);
        let share = s.filter(|s| s.area > 0.0 && !s.members.is_empty()).map_or(1.0, |s| (lot.area() / s.area).clamp(0.0, 1.0));
        let part = |n: u32| (n as f32 * share).round() as u32;
        let visitors = s.map_or(0, |s| part(s.visitors));
        let swimmers = s.map_or(0, |s| part(s.swimmers));
        let here = s.map_or(0, |s| part(s.people_here));
        BeachLook {
            bathers: visitors.saturating_sub(swimmers).saturating_sub(here).min(400) as u16,
            swimmers: swimmers.min(200) as u16,
            closed: s.is_some_and(|s| !s.open),
            night: !(7..20).contains(&world.clock.hour()),
        }
    });
    Some(ShoreLook { seaward: lot.shore.seaward, drop, beach })
}

/// Is there `d` metres of the lot outside a face along all of it (its
/// middle and both ends)? For balconies and decks that hang off it.
fn clear_out(c: &Ctx, face: &Face, d: f32) -> bool {
    [-0.48f32, 0.0, 0.48].iter().all(|t| on_lot(c, face.at(face.len * t, d), 0.2))
}

// ── [pt-visual] the promenade along a long public beach ─────────────────────

/// Public beach lots touching along their sides from which the sand gets a
/// boardwalk promenade along its landward edge (Harvey: "if enough public
/// beach shoreline gets added together it can procedurally generate a
/// boardwalk; needs like 4 plots").
pub const BOARDWALK_MIN_LOTS: usize = 4;
/// Width of the promenade's deck.
pub const BOARDWALK_WIDTH: f32 = 4.5;
/// The deck's height over the sand.
const BOARDWALK_DECK: f32 = 0.6;

/// The beach lots of every run of [`BOARDWALK_MIN_LOTS`] or more public
/// beach lots that share their sides (world lot ids, run by run).
pub fn beach_runs(world: &nd_sim::World) -> Vec<Vec<nd_sim::LotId>> {
    let ids: Vec<nd_sim::LotId> = world.lots.iter().filter(|(_, l)| l.shore.beach && l.polygon.len() >= 3).map(|(id, _)| id).collect();
    let n = ids.len();
    let touch = |a: &[Vec2], b: &[Vec2]| {
        let (na, nb) = (a.len(), b.len());
        (0..na).any(|i| (0..nb).any(|j| nd_sim::geom::shared_stretch(a[i], a[(i + 1) % na], b[j], b[(j + 1) % nb]).is_some_and(|(lo, hi)| hi - lo >= 2.0)))
    };
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (&world.lots[ids[i]].polygon, &world.lots[ids[j]].polygon);
            if touch(a, b) {
                adj[i].push(j);
                adj[j].push(i);
            }
        }
    }
    let mut seen = vec![false; n];
    let mut runs = Vec::new();
    for s in 0..n {
        if seen[s] {
            continue;
        }
        let mut stack = vec![s];
        seen[s] = true;
        let mut run = Vec::new();
        while let Some(i) = stack.pop() {
            run.push(ids[i]);
            for &j in &adj[i] {
                if !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if run.len() >= BOARDWALK_MIN_LOTS {
            run.sort();
            runs.push(run);
        }
    }
    runs.sort();
    runs
}

/// The promenades of the long beaches ([`beach_runs`]): a wooden deck on
/// posts along each lot's landward edges, the lots' decks meeting at their
/// seams so the run reads as one boardwalk; a rail on the sand side with a
/// gap where each lot's path goes down to the water, lamps with a neon band,
/// benches facing the sea and the odd kiosk with a lit sign. Drawn in world
/// coordinates at the ground's height.
pub fn boardwalks(world: &nd_sim::World, palette: &crate::palette::Palette) -> Geometry {
    let mut g = Geometry::default();
    let wood = mul(palette.wood, 1.1);
    let dark = mul(palette.wood, 0.55);
    let neon = [0.25, 1.4, 1.7];
    for run in beach_runs(world) {
        for &id in &run {
            let lot = &world.lots[id];
            let poly = &lot.polygon;
            let m = poly.len();
            let sea = if lot.shore.seaward != Vec2::ZERO { lot.shore.seaward } else { -lot.front_dir };
            let ccw = nd_sim::zoning::polygon_area(poly) > 0.0;
            let entrance = lot.park.as_ref().and_then(|p| p.paths.first()).and_then(|p| p.first().copied());
            let others: Vec<&Vec<Vec2>> = run.iter().filter(|o| **o != id).map(|o| &world.lots[*o].polygon).collect();
            for k in 0..m {
                let (a, b) = (poly[k], poly[(k + 1) % m]);
                let len = a.distance(b);
                if len < 3.0 {
                    continue;
                }
                let d = (b - a) / len;
                let inward = if ccw { Vec2::new(-d.y, d.x) } else { Vec2::new(d.y, -d.x) };
                // The landward edges: facing away from the water, not a
                // seam with the next lot of the run.
                if (-inward).dot(sea) > -0.5 {
                    continue;
                }
                let seam = others.iter().any(|o| {
                    let n = o.len();
                    (0..n).any(|j| nd_sim::geom::shared_stretch(a, b, o[j], o[(j + 1) % n]).is_some_and(|(lo, hi)| hi - lo >= 2.0))
                });
                if seam {
                    continue;
                }
                let z = |p: Vec2| world.surface_height(p) + BOARDWALK_DECK;
                let w = BOARDWALK_WIDTH;
                let (a2, b2) = (a + inward * w, b + inward * w);
                // The deck, as a run of planks across it.
                let steps = (len / 2.0).ceil().max(1.0) as usize;
                for s in 0..steps {
                    let (t0, t1) = (s as f32 / steps as f32, (s + 1) as f32 / steps as f32);
                    let (p0, p1) = (a.lerp(b, t0), a.lerp(b, t1));
                    let (q0, q1) = (a2.lerp(b2, t0), a2.lerp(b2, t1));
                    let c = rgba(if s % 2 == 0 { wood } else { mul(wood, 0.93) });
                    g.part(Wood).quad_facing([Vec3::new(p0.x, z(p0), p0.y), Vec3::new(p1.x, z(p1), p1.y), Vec3::new(q1.x, z(q1), q1.y), Vec3::new(q0.x, z(q0), q0.y)], Vec3::Y, c);
                    // The skirt down to the sand on the water side.
                    let (f0, f1) = (world.surface_height(q0) - 0.2, world.surface_height(q1) - 0.2);
                    g.part(Wood).quad_facing([Vec3::new(q0.x, f0, q0.y), Vec3::new(q1.x, f1, q1.y), Vec3::new(q1.x, z(q1), q1.y), Vec3::new(q0.x, z(q0), q0.y)], Vec3::new(inward.x, 0.0, inward.y), rgba(dark));
                }
                // Posts and the rail on the sand side, open where the lot's
                // path goes down to the water.
                let gap = entrance.map(|e| nd_sim::geom::project_on_segment(e, a2, b2).0).unwrap_or(-1.0);
                let posts = (len / 2.5).floor().max(1.0) as usize;
                for s in 0..=posts {
                    let t = s as f32 / posts as f32;
                    if (t - gap).abs() * len < 2.0 {
                        continue;
                    }
                    let p = a2.lerp(b2, t) - inward * 0.15;
                    let y = z(p);
                    g.part(Wood).boxed(Vec3::new(p.x, y + 0.5, p.y), Vec3::new(0.12, 1.0, 0.12), 0.0, rgba(dark));
                    if s < posts && ((t + 0.5 / posts as f32) - gap).abs() * len >= 2.0 {
                        let q = a2.lerp(b2, (s + 1) as f32 / posts as f32) - inward * 0.15;
                        g.part(Wood).beam(Vec3::new(p.x, y + 1.0, p.y), Vec3::new(q.x, z(q) + 1.0, q.y), 0.1, rgba(wood));
                    }
                }
                // Lamps with a neon band every 12 m, benches facing the sea
                // between them.
                let mut s = 6.0;
                let yaw = sea.y.atan2(sea.x);
                while s < len - 2.0 {
                    let p = a + d * s + inward * (w - 0.6);
                    let y = z(p);
                    g.part(Matte).cylinder(p, 0.08, 0.06, y, y + 4.2, 6, rgba([0.15, 0.15, 0.17]));
                    g.part(Glow).boxed(Vec3::new(p.x, y + 4.3, p.y), Vec3::new(0.45, 0.25, 0.45), 0.0, rgba(palette.lamp));
                    g.part(Glow).boxed(Vec3::new(p.x, y + 3.2, p.y), Vec3::new(0.14, 0.9, 0.14), 0.0, rgba(neon));
                    let bp = a + d * (s + 6.0) + inward * (w - 1.0);
                    if s + 6.0 < len - 1.5 {
                        // A bench stands on 0: drawn apart and lifted onto the deck.
                        let mut b = Geometry::default();
                        bench_world(&mut b, bp, yaw - FRAC_PI_2, palette.wood, CHARCOAL);
                        let lift = Vec3::new(0.0, z(bp), 0.0);
                        for (surf, mesh) in b.iter() {
                            g.part(surf).append_transformed(mesh, lift, 0.0);
                        }
                    }
                    s += 12.0;
                }
                // A kiosk on the longer edges: a booth with an awning and a sign.
                let h = ((lot.center.x.to_bits() as u64) << 32 | lot.center.y.to_bits() as u64).wrapping_add(k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40;
                if len >= 18.0 && h % 3 == 0 {
                    let p = a + d * (len * 0.3) + inward * 1.6;
                    let y = z(p);
                    let ky = d.y.atan2(d.x);
                    g.part(Plaster).boxed(Vec3::new(p.x, y + 1.3, p.y), Vec3::new(3.0, 2.6, 2.2), ky, rgba([0.92, 0.9, 0.84]));
                    g.part(Tarp).boxed(Vec3::new(p.x + inward.x * 1.4, y + 2.5, p.y + inward.y * 1.4), Vec3::new(3.4, 0.08, 1.2), ky, rgba([0.95, 0.3, 0.4]));
                    g.part(Glow).boxed(Vec3::new(p.x, y + 2.95, p.y), Vec3::new(2.4, 0.4, 0.1), ky, rgba([2.2, 0.4, 1.4]));
                }
            }
        }
    }
    g
}
