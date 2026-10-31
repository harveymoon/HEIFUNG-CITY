//! [r4-leisure-places] Places to eat and go out (`nd_sim::leisure`): a
//! diner, a noodle bar, a pizza place, a drive-through, a food hall,
//! street-food stalls and food trucks, a fine-dining room, a bubble-tea
//! bar; a bar, karaoke boxes, a cinema, a theatre, a live-music hall, a
//! bowling alley, an arcade, a gym and a comedy club. Each is forced by its
//! business (`super::industry_styles`), whatever the zone or district, and
//! says what it is in stencilled neon over the door (and on a pole, a
//! blade or the roof). Cheap ones are worn, tagged and patched; dear ones
//! clad and lit. Stylised and gritty: boxes, decals and glow.
//!
//! The layouts work in axis-aligned rectangles of the lot frame ([`R`]):
//! `u` along the street, `v` in from it, so the street side is `v0`.

use super::kit::{block, fit, gable_roof, pane, pane_row, planter, table, R};
use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;

// ── shared ───────────────────────────────────────────────────────────────────

/// A word in neon stencil on `face`, `t` along it, `y` up, `d` proud, `h`
/// tall: letters drop out on a worn place. Returns its width.
#[allow(clippy::too_many_arguments)]
pub(crate) fn neon_word(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, d: f32, h: f32, text: &[u8], color: [f32; 3]) -> f32 {
    let f = &c.frame;
    let centre = f.at2(face.at(t, d), y);
    let out = f.dir3(face.out);
    let read = (-out).cross(Vec3::Y).normalize_or_zero();
    let seed = c.seed;
    let wear = c.wear;
    stencil(g.part(Glow), centre, read, out, h, text, rgba(color), move |k| wear > 0.6 && nd_sim::hash::unit(nd_sim::hash::mix(seed, k as u64 ^ 0x5EED)) < (wear - 0.6) * 0.5)
}

/// Width a word of `n` letters takes at height `h` (as `stencil` lays it).
fn word_w(n: usize, h: f32) -> f32 {
    (h / 5.0) * (4.0 * n as f32 - 1.0)
}

/// Letter height for `text` to fit `w`, at most `max`.
fn fit_h(text: &[u8], w: f32, max: f32) -> f32 {
    let per = word_w(text.len(), 1.0);
    (w / per).min(max).max(0.2)
}

/// A dark board with a neon word on it, sized to fit `w` along the face.
#[allow(clippy::too_many_arguments)]
fn word_board(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, max_h: f32, text: &[u8], color: [f32; 3]) {
    let h = fit_h(text, w * 0.86, max_h);
    let ww = word_w(text.len(), h);
    face.bx(g, &c.frame, Matte, t, y, 0.12, Vec3::new(ww + h * 0.8, h * 1.6, 0.14), DARK);
    neon_word(g, c, face, t, y, 0.21, h, text, color);
}

/// A sign on a pole at the street corner: a lit box with a word both ways.
#[allow(clippy::too_many_arguments)]
fn pole_sign(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32, w: f32, text: &[u8], color: [f32; 3], frame_col: [f32; 3]) -> bool {
    let f = &c.frame;
    if !c.all_inside(&rect(p.x, p.y, w * 0.5 + 0.2, 0.45)) {
        return false;
    }
    f.bx(g, Metal, p.x, h * 0.5, p.y, Vec3::new(0.35, h, 0.35), c.palette.steel);
    let bh = fit_h(text, w * 0.85, 1.4) * 1.8;
    f.bx(g, Panel, p.x, h + bh * 0.5, p.y, Vec3::new(w, bh, 0.5), frame_col);
    // Both faces of the box: the street's (-v) and the lot's (+v).
    for out in [Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0)] {
        let face = Face::new(p + Vec2::new(-w * 0.5, out.y * 0.25), p + Vec2::new(w * 0.5, out.y * 0.25), out);
        let lh = fit_h(text, w * 0.85, 1.4);
        neon_word(g, c, &face, 0.0, h + bh * 0.5, 0.04, lh, text, color);
    }
    true
}

/// A line on the ground from `a` to `b` (both inside the lot): a painted bay.
fn ground_line(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, color: [f32; 3]) {
    if !(c.inside_local(a) && c.inside_local(b)) {
        return;
    }
    let m = (a + b) * 0.5;
    let d = b - a;
    c.frame.bx_yaw(g, Glow, m.x, 0.06, m.y, Vec3::new(d.length(), 0.02, 0.08), color, d.y.atan2(d.x));
}

/// String lights from `a` to `b` at `y`, only when the run stays over the lot.
fn lights_over(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, y: f32, n: i32, color: [f32; 3]) {
    if (0..=8).all(|k| c.inside_local(a.lerp(b, k as f32 / 8.0))) {
        string_lights(g, &c.frame, Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y), n, color);
    }
}

/// The house colour of a place: a seeded pick among `options`.
fn house(c: &Ctx, salt: u64, options: &[[f32; 3]]) -> [f32; 3] {
    options[(nd_sim::hash::mix(c.seed, salt) % options.len() as u64) as usize]
}

/// The building's main block on the street, `w` wide and `d` deep at most,
/// `setback` from the front: the largest that fits.
fn street_block(c: &Ctx, w: f32, d: f32, setback: f32) -> Option<R> {
    let area = R::of(&c.poly);
    let w = w.min(area.w());
    let v0 = area.v0 + setback.min(area.d() * 0.4);
    let want = R::new(area.c().x - w * 0.5, area.c().x + w * 0.5, v0, (v0 + d).min(area.v1));
    // An odd lot: smaller and nearer its middle until it stands on it.
    fit(c, want, 3.0).or_else(|| {
        [0.7f32, 0.5].into_iter().find_map(|k| {
            let m = area.c();
            fit(c, R::new(m.x - w * k * 0.5, m.x + w * k * 0.5, m.y - d.min(area.d()) * k * 0.5, m.y + d.min(area.d()) * k * 0.5), 3.0)
        })
    })
}

/// Floors a style draws: the building's, kept between `lo` and `hi`.
fn floors(c: &Ctx, lo: u8, hi: u8) -> u8 {
    c.floors.clamp(lo, hi)
}

/// Lit windows of warm light over the street front, a door and grime.
fn upper_floors(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, r: &R, y0: f32, n: i32, fh: f32) {
    if n <= 0 {
        return;
    }
    for face in r.faces() {
        if face.len > 3.0 {
            pane_row(g, c, rng, &face, y0, n, fh, 2.8, 1.3, 1.5, true);
            grime(g, c, rng, &face, 0.0, y0 + n as f32 * fh, 0.6);
        }
    }
}

const NEON_RED: [f32; 3] = [1.6, 0.25, 0.25];
const NEON_PINK: [f32; 3] = [1.5, 0.35, 1.0];
const NEON_TEAL: [f32; 3] = [0.3, 1.4, 1.3];
const NEON_AMBER: [f32; 3] = [1.6, 0.95, 0.3];
const NEON_BLUE: [f32; 3] = [0.35, 0.7, 1.6];
const NEON_GREEN: [f32; 3] = [0.5, 1.5, 0.5];
const NEON_WHITE: [f32; 3] = [1.4, 1.4, 1.35];
const NEON_VIOLET: [f32; 3] = [0.9, 0.4, 1.6];

// ── diner ────────────────────────────────────────────────────────────────────

/// A long low box with a band of chrome, a ribbon of windows lit warm, a
/// stripe in the house colour, a sign on a pole and a few cars outside.
pub fn diner(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let t = c.tier.index();
    yard(g, c, 0.3, Asphalt, mul(pal.asphalt, 1.05));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w() * 0.7, 9.0, (area.d() * 0.3).min(7.0)) else { return };
    let h = 4.2;
    let stripe = house(c, 0xD1, &[[0.75, 0.12, 0.12], [0.1, 0.55, 0.55], [0.9, 0.75, 0.2], [0.2, 0.3, 0.7]]);
    block(g, c, Metal, &r, 0.0, h, mul([0.82, 0.83, 0.85], 1.0 - 0.3 * c.wear));
    // The chrome bands and the stripe.
    for face in r.faces() {
        face.bx(g, f, Metal, 0.0, 0.5, 0.05, Vec3::new(face.len + 0.1, 0.9, 0.1), mul(stripe, 0.9));
        face.bx(g, f, Metal, 0.0, h - 0.35, 0.06, Vec3::new(face.len + 0.12, 0.7, 0.12), [0.9, 0.9, 0.92]);
    }
    let fr = r.front();
    // The ribbon of windows and the booths' lamps behind them.
    let n = ((fr.len - 3.0) / 1.8).floor().max(2.0) as i32;
    for i in 0..n {
        let tt = -fr.len * 0.5 + 1.5 + (i as f32 + 0.5) * (fr.len - 3.0) / n as f32;
        if i == n / 2 {
            fr.decal(g, f, Matte, tt, 1.15, 0.06, 1.2, 2.3, DARK);
            fr.decal(g, f, Glow, tt, 2.45, 0.08, 1.0, 0.12, NEON_AMBER);
            continue;
        }
        pane(g, c, rng, &fr, tt, 2.0, 1.5, 1.4, true);
    }
    for face in [r.left(), r.right()] {
        pane_row(g, c, rng, &face, 0.0, 1, h, 2.2, 1.3, 1.3, true);
    }
    // The name over the door, and the big sign at the corner.
    let word: &[u8] = if c.seed % 3 == 0 { b"EAT" } else { b"DINER" };
    word_board(g, c, &fr, 0.0, h + 0.7, fr.len * 0.5, 0.9, word, if t == 0 { NEON_RED } else { NEON_TEAL });
    let pole = Vec2::new(area.u0 + 1.5, area.v0 + 1.0);
    if pole_sign(g, c, pole, 5.5 + t as f32, 3.6, b"DINER", NEON_RED, mul(stripe, 0.7)) {
        f.bx(g, Glow, pole.x, 4.6 + t as f32, pole.y, Vec3::new(2.4, 0.3, 0.55), NEON_AMBER);
    }
    // A vent on the roof over the grill, and the cars of the breakfast crowd.
    roof_gear(g, c, rng, &r.poly(), h, 0.4);
    f.bx(g, Metal, r.u1 - 1.5, h + 0.8, r.v1 - 1.5, Vec3::new(1.0, 1.6, 1.0), pal.steel);
    let lot_v = (area.v0 + r.v0) * 0.5;
    let cars = ((area.w() - 4.0) / 3.0) as i32;
    for k in 0..cars.min(5) {
        if !coin(rng, 0.6) {
            continue;
        }
        let p = Vec2::new(area.u0 + 3.0 + k as f32 * 3.0, lot_v);
        if vehicle_fits(c, p, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
            let paint = pick(rng, &pal.car_colors);
            vehicle(g, f, pal, rng, p, 0.0, std::f32::consts::FRAC_PI_2, Vehicle::Car, paint);
        }
    }
    if c.wear > 0.4 {
        grime(g, c, rng, &fr, 0.0, h, 1.0);
    }
}

// ── noodle bar ───────────────────────────────────────────────────────────────

/// A narrow shophouse: a dark timber front with a curtain over the door,
/// red lanterns, a blade sign, steam from a flue and flats of lit windows.
pub fn noodle_bar(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.85));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(11.0), area.d().min(14.0), 0.6) else { return };
    let n = floors(c, 2, 3);
    let fh = 3.3;
    let h = n as f32 * fh;
    let wall = mix(pick(rng, &pal.plaster), [0.35, 0.3, 0.28], 0.4);
    block(g, c, Plaster, &r, 0.0, h, mul(wall, 1.0 - 0.25 * c.wear));
    let fr = r.front();
    // The shopfront: dark wood, a lit counter window and the noren.
    fr.decal(g, f, Wood, 0.0, 1.5, 0.05, fr.len * 0.94, 3.0, mul(pal.wood, 0.45));
    fr.glaze(g, c, -fr.len * 0.18, 1.55, 0.08, fr.len * 0.45, 1.6, Pane::Lit([1.3, 0.9, 0.55]));
    let door_t = fr.len * 0.26;
    fr.decal(g, f, Matte, door_t, 1.1, 0.1, 1.1, 2.2, DARK);
    let cloth = house(c, 0x40, &[[0.08, 0.1, 0.25], [0.55, 0.08, 0.08], [0.1, 0.1, 0.1]]);
    for k in 0..4 {
        fr.decal(g, f, Tarp, door_t - 0.42 + k as f32 * 0.28, 2.0, 0.12, 0.24, 0.9, cloth);
    }
    // Lanterns along the eaves of the shop.
    let lanterns = ((fr.len - 1.0) / 1.6) as i32;
    for k in 0..lanterns.min(6) {
        let tt = -fr.len * 0.5 + 0.8 + (k as f32 + 0.5) * (fr.len - 1.6) / lanterns.max(1) as f32;
        fr.bx(g, f, Glow, tt, 3.0, 0.45, Vec3::new(0.38, 0.5, 0.38), [1.5, 0.3, 0.15]);
        fr.decal(g, f, Matte, tt, 3.4, 0.45, 0.04, 0.3, CHARCOAL);
    }
    fr.bx(g, f, Wood, 0.0, 3.25, 0.5, Vec3::new(fr.len * 0.98, 0.15, 1.0), mul(pal.wood, 0.5));
    word_board(g, c, &fr, -fr.len * 0.15, 3.75, fr.len * 0.5, 0.55, b"RAMEN", NEON_WHITE);
    // The blade sign up the front and the flats above.
    let blade_t = fr.len * 0.5 - 0.6;
    let word: &[u8] = if c.seed % 2 == 0 { b"NOODLE" } else { b"RAMEN" };
    let bh = (h - 3.4).max(2.4);
    fr.bx(g, f, Matte, blade_t, 3.4 + bh * 0.5, 0.8, Vec3::new(0.25, bh + 0.2, 1.2), DARK);
    let side = Face::new(fr.at(blade_t, 1.4) - Vec2::new(0.0, 0.0), fr.at(blade_t, 0.2), Vec2::new(-1.0, 0.0));
    let lh = (bh / word.len() as f32 * 0.8).min(0.7);
    for (i, ch) in word.iter().enumerate() {
        let y = 3.4 + bh - (i as f32 + 0.6) * (bh / word.len() as f32);
        neon_word(g, c, &side, 0.0, y, 0.15, lh, &[*ch], NEON_RED);
        let other = Face::new(side.b, side.a, Vec2::new(1.0, 0.0));
        neon_word(g, c, &other, 0.0, y, 0.15, lh, &[*ch], NEON_RED);
    }
    upper_floors(g, c, rng, &r, 3.6, n as i32 - 1, fh);
    for face in [r.left(), r.right()] {
        if face.len > 4.0 && coin(rng, 0.7) {
            ac_unit(g, c, &face, face.len * 0.2, 4.5, 0.4);
        }
    }
    // The flue over the kitchen at the back.
    chimney(g, f, Vec2::new(r.u1 - 1.2, r.v1 - 1.2), 0.35, h, 2.2, pal.steel, [1.0, 0.5, 0.2]);
    roof_gear(g, c, rng, &r.poly(), h, 0.5);
}

// ── pizza place ──────────────────────────────────────────────────────────────

/// Brick, a striped awning, the oven's chimney with its glow, a window
/// with the oven's fire in it, a few tables out front.
pub fn pizza_place(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, pal.paving);
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(14.0), area.d().min(12.0), (area.d() * 0.25).min(4.0)) else { return };
    let n = floors(c, 1, 2);
    let fh = 3.6;
    let h = n as f32 * fh;
    block(g, c, Brick, &r, 0.0, h, mul(pick(rng, &pal.brick), 1.0 - 0.2 * c.wear));
    let fr = r.front();
    // The oven window: fire behind glass.
    fr.glaze(g, c, -fr.len * 0.22, 1.6, 0.06, fr.len * 0.36, 1.8, Pane::Lit([1.6, 0.6, 0.2]));
    fr.glaze(g, c, fr.len * 0.25, 1.6, 0.06, fr.len * 0.25, 1.8, Pane::Lit([1.3, 1.0, 0.6]));
    fr.decal(g, f, Matte, fr.len * 0.02, 1.1, 0.08, 1.1, 2.2, DARK);
    // A striped awning in the colours.
    let reach = (c.reach_out(&fr) - 0.2).clamp(0.6, 1.6);
    let stripes = ((fr.len * 0.9) / 0.6) as i32;
    for k in 0..stripes {
        let col = if k % 2 == 0 { [0.75, 0.12, 0.1] } else { [0.9, 0.88, 0.8] };
        fr.bx(g, f, Tarp, -fr.len * 0.45 + (k as f32 + 0.5) * 0.6, 3.0, reach * 0.5, Vec3::new(0.6, 0.1, reach), mul(col, 1.0 - 0.35 * c.wear));
    }
    word_board(g, c, &fr, 0.0, 3.55, fr.len * 0.6, 0.6, b"PIZZA", NEON_AMBER);
    if n > 1 {
        upper_floors(g, c, rng, &r, fh, 1, fh);
    }
    // The oven's chimney, sooty at the top.
    let p = Vec2::new(r.u0 + 1.0, r.v1 - 1.0);
    f.bx(g, Brick, p.x, h + 1.4, p.y, Vec3::new(0.9, 2.8, 0.9), mul(pick(rng, &pal.brick), 0.8));
    f.bx(g, Matte, p.x, h + 2.85, p.y, Vec3::new(1.0, 0.15, 1.0), CHARCOAL);
    // Tables out front when there is room.
    let front_d = r.v0 - area.v0;
    if front_d > 2.2 {
        let k = ((r.w() - 2.0) / 2.6) as i32;
        for i in 0..k.min(4) {
            let p = Vec2::new(r.u0 + 1.3 + i as f32 * 2.6, area.v0 + front_d * 0.5);
            if c.all_inside(&rect(p.x, p.y, 1.3, 1.3)) {
                table(g, f, p, 0.0, mul(pal.wood, 1.1), if i % 2 == 0 { Some([0.75, 0.12, 0.1]) } else { None });
            }
        }
    }
    roof_gear(g, c, rng, &r.poly(), h, 0.3);
    if c.wear > 0.3 {
        grime(g, c, rng, &fr, 0.0, h, 1.0);
    }
}

// ── drive-through ────────────────────────────────────────────────────────────

/// A boxy one-storey restaurant set back on its lot, a drive-through lane
/// round it with a menu board and a window, a tall pylon sign, painted
/// bays and cars, one of them in the lane.
pub fn fast_food(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.2, Asphalt, mul(pal.asphalt, 0.95));
    let area = R::of(&c.poly);
    let lane = 3.2;
    let bw = (area.w() - lane * 2.0 - 1.0).clamp(5.0, 14.0);
    let bd = (area.d() * 0.45).clamp(5.0, 12.0);
    let want = R::new(area.c().x - bw * 0.5, area.c().x + bw * 0.5, area.v1 - bd - lane - 0.5, area.v1 - lane - 0.5);
    let Some(r) = fit(c, want, 4.0) else { return };
    let h = 4.6;
    let brand = house(c, 0xFF, &[[0.8, 0.15, 0.1], [0.95, 0.7, 0.1], [0.15, 0.45, 0.2], [0.2, 0.25, 0.6]]);
    block(g, c, Panel, &r, 0.0, h, mul([0.85, 0.82, 0.78], 1.0 - 0.3 * c.wear));
    // A wedge of the house colour along the roofline.
    parapet(g, c, &r.poly(), h, 1.0, 0.25, Panel, brand);
    let fr = r.front();
    fr.glaze(g, c, 0.0, 1.7, 0.06, fr.len * 0.7, 2.0, Pane::Lit([1.4, 1.25, 1.0]));
    fr.decal(g, f, Matte, fr.len * 0.38, 1.1, 0.08, 1.2, 2.2, DARK);
    let word: &[u8] = if c.seed % 2 == 0 { b"BURGERS" } else { b"DRIVE THRU" };
    word_board(g, c, &fr, 0.0, h + 0.5, fr.len * 0.85, 0.8, word, NEON_AMBER);
    // The lane: a dark loop round the back and down the right, painted
    // arrows, the menu board and the pick-up window.
    let right = R::new(r.u1 + 0.3, (r.u1 + 0.3 + lane).min(area.u1), area.v0 + 1.5, area.v1 - 0.3);
    let back = R::new(r.u0 - 0.3, right.u1, r.v1 + 0.3, (r.v1 + 0.3 + lane).min(area.v1));
    for l in [right, back] {
        if l.w() > 1.0 && l.d() > 1.0 && poly_inside(c, &l.poly()) {
            ground(g, f, Asphalt, &l.poly(), 0.05, mul(pal.asphalt, 0.7));
        }
    }
    let rf = r.right();
    if rf.len > 4.0 {
        rf.glaze(g, c, 0.0, 1.5, 0.06, 1.6, 1.2, Pane::Lit([1.4, 1.2, 0.9]));
        rf.bx(g, f, Panel, 0.0, 2.4, 0.4, Vec3::new(2.2, 0.15, 0.8), brand);
        for k in 0..3 {
            let p = Vec2::new(right.c().x, right.v0 + 2.0 + k as f32 * 3.0);
            ground_line(g, c, Vec2::new(p.x, p.y - 0.6), Vec2::new(p.x, p.y + 0.6), mul(pal.lane_marker, 0.8));
        }
        // The menu board: a lit panel on legs by the lane.
        let mb = Vec2::new(right.u1 - 0.4, back.v0 - 0.5);
        if c.inside_local(mb) {
            f.bx(g, Matte, mb.x, 1.2, mb.y, Vec3::new(0.2, 2.4, 1.8), DARK);
            f.bx(g, Glow, mb.x - 0.12, 1.5, mb.y, Vec3::new(0.04, 1.2, 1.6), [1.4, 1.1, 0.6]);
        }
    }
    // The pylon over the road, seen from far off.
    let pylon = Vec2::new(area.u0 + 1.2, area.v0 + 1.2);
    if pole_sign(g, c, pylon, 8.0, 3.6, b"EAT", NEON_AMBER, brand) {
        f.bx(g, Glow, pylon.x, 9.0 + 2.6, pylon.y, Vec3::new(1.2, 0.8, 0.6), NEON_RED);
    }
    // Bays in front, a car at the window.
    let bays = ((r.w()) / 2.8) as i32;
    for k in 0..bays.min(5) {
        let p = Vec2::new(r.u0 + 1.4 + k as f32 * 2.8, area.v0 + (r.v0 - area.v0) * 0.55);
        if c.inside_local(p) {
            ground_line(g, c, Vec2::new(p.x - 1.4, p.y - 2.0), Vec2::new(p.x - 1.4, p.y + 2.0), mul(pal.lane_marker, 0.6));
            if coin(rng, 0.5) && vehicle_fits(c, p, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
                let paint = pick(rng, &pal.car_colors);
                vehicle(g, f, pal, rng, p, 0.0, std::f32::consts::FRAC_PI_2, Vehicle::Car, paint);
            }
        }
    }
    let at_window = Vec2::new(right.c().x, r.c().y);
    if vehicle_fits(c, at_window, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
        let paint = pick(rng, &pal.car_colors);
        vehicle(g, f, pal, rng, at_window, 0.0, -std::f32::consts::FRAC_PI_2, Vehicle::Car, paint);
    }
    // Litter and a bin overflowing on a cheap one.
    if c.wear > 0.4 {
        grime(g, c, rng, &fr, 0.0, h, 1.2);
        graffiti(g, c, rng, &r.left(), 0.0, 2.5, 1.0);
    }
    roof_gear(g, c, rng, &r.poly(), h, 0.6);
}

// ── food hall ────────────────────────────────────────────────────────────────

/// A big hall under a pitched roof with clerestory lights, a glass front
/// showing a row of counters each lit in its own colour, a long sign and
/// string lights over the forecourt's shared tables.
pub fn food_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.05));
    let area = R::of(&c.poly);
    let court = (area.d() * 0.22).clamp(2.0, 6.0);
    let Some(r) = street_block(c, area.w() * 0.92, area.d() - court, court) else { return };
    let h = if floors(c, 1, 2) > 1 { 8.0 } else { 6.0 };
    let wall = if c.tier == nd_sim::Tier::Low { mul(pick(rng, &pal.brick), 0.9) } else { mix(pick(rng, &pal.panel), [0.3, 0.3, 0.32], 0.4) };
    block(g, c, if c.tier == nd_sim::Tier::Low { Brick } else { Panel }, &r, 0.0, h, wall);
    gable_roof(g, c, &r.poly(), h, (r.w().min(r.d()) * 0.18).clamp(1.2, 3.5), mul(pal.steel, 0.7));
    let fr = r.front();
    fr.glaze(g, c, 0.0, h * 0.42, 0.05, fr.len * 0.88, h * 0.7, Pane::Dark);
    // The counters inside, each its own colour and sign.
    let counters = ((fr.len * 0.85) / 3.2) as i32;
    let colours = [NEON_RED, NEON_TEAL, NEON_AMBER, NEON_PINK, NEON_GREEN, NEON_BLUE];
    for k in 0..counters.clamp(2, 10) {
        let tt = -fr.len * 0.42 + (k as f32 + 0.5) * fr.len * 0.84 / counters.clamp(2, 10) as f32;
        let col = colours[(nd_sim::hash::mix(c.seed, k as u64) % colours.len() as u64) as usize];
        fr.glaze(g, c, tt, 1.2, 0.07, 2.4, 1.4, Pane::Lit(mul(col, 0.55)));
        fr.decal(g, f, Glow, tt, 2.35, 0.09, 1.8, 0.3, col);
    }
    for k in 0..=((fr.len / 3.0) as i32).max(1) {
        let tt = -fr.len * 0.44 + k as f32 * fr.len * 0.88 / ((fr.len / 3.0) as i32).max(1) as f32;
        fr.decal(g, f, Matte, tt, h * 0.42, 0.1, 0.12, h * 0.7, CHARCOAL);
    }
    word_board(g, c, &fr, 0.0, h * 0.86, fr.len * 0.7, 1.0, b"FOOD HALL", NEON_WHITE);
    for face in [r.left(), r.right()] {
        face.decal(g, f, Glow, 0.0, h - 0.6, 0.05, face.len * 0.9, 0.5, mul(pal.window_warm, 0.6));
        grime(g, c, rng, &face, 0.0, h, 0.8);
    }
    // The forecourt: long tables under strings of lights.
    if court > 2.5 {
        let n = ((r.w() - 2.0) / 3.5) as i32;
        for i in 0..n.min(6) {
            let p = Vec2::new(r.u0 + 1.8 + i as f32 * 3.5, area.v0 + court * 0.5);
            if c.all_inside(&rect(p.x, p.y, 1.3, 1.3)) {
                table(g, f, p, 0.0, mul(pal.wood, 1.0), None);
            }
        }
        lights_over(g, c, Vec2::new(r.u0 + 0.5, area.v0 + 0.6), Vec2::new(r.u1 - 0.5, area.v0 + 0.6), 3.6, 16, [1.5, 1.1, 0.55]);
    }
}

// ── street food ──────────────────────────────────────────────────────────────

/// An open lot of food trucks and stalls under tarps, a sign arch at the
/// street, string lights criss-crossed overhead, tables and a drum fire.
pub fn street_food(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let ground_poly = yard(g, c, 0.2, if c.tier == nd_sim::Tier::Low { Dirt } else { Paving }, if c.tier == nd_sim::Tier::Low { pal.dirt } else { mul(pal.paving, 0.9) });
    let area = R::of(&c.poly);
    let front = c.front_face(&ground_poly);
    // Trucks along the sides, facing in.
    let along = (area.w() / 7.5) as i32;
    let truck_colors = [[0.85, 0.75, 0.2], [0.2, 0.5, 0.6], [0.75, 0.2, 0.2], [0.85, 0.85, 0.8], [0.3, 0.6, 0.3]];
    for k in 0..along.clamp(1, 4) {
        let p = Vec2::new(area.u0 + 3.8 + k as f32 * 7.5, area.v1 - 3.0);
        let col = truck_colors[(nd_sim::hash::mix(c.seed, 0x70 + k as u64) % truck_colors.len() as u64) as usize];
        if vehicle_fits(c, p, 0.0, Vehicle::Van) {
            vehicle(g, f, pal, rng, p, 0.0, 0.0, Vehicle::Van, mul(col, 1.0 - 0.3 * c.wear));
            // The serving hatch lit and an awning off it, facing the street.
            f.bx(g, Glow, p.x, 1.5, p.y - 1.05, Vec3::new(2.2, 0.7, 0.04), [1.4, 1.1, 0.6]);
            f.bx(g, Tarp, p.x, 2.25, p.y - 1.6, Vec3::new(2.6, 0.08, 1.1), pick(rng, &pal.awning_colors));
        }
    }
    // Stalls along the street front.
    let stalls = ((front.len - 2.0) / 3.5) as i32;
    for k in 0..stalls.clamp(1, 6) {
        let tt = -front.len * 0.5 + 1.8 + (k as f32 + 0.5) * (front.len - 3.6) / stalls.clamp(1, 6) as f32;
        if (tt).abs() < 1.8 {
            continue;
        }
        stall(g, c, rng, &front, tt, -2.2);
    }
    // The arch over the way in.
    let a = front.at(-1.6, -0.6);
    let b = front.at(1.6, -0.6);
    if c.all_inside(&[front.at(-2.0, -0.2), front.at(2.0, -0.2), front.at(-2.0, -1.0), front.at(2.0, -1.0)]) {
        for p in [a, b] {
            f.bx(g, Metal, p.x, 2.2, p.y, Vec3::new(0.2, 4.4, 0.2), pal.steel);
        }
        front.bx(g, f, Matte, 0.0, 4.6, -0.6, Vec3::new(3.6, 0.7, 0.15), DARK);
        let word: &[u8] = if c.seed % 2 == 0 { b"EATS" } else { b"STREET FOOD" };
        neon_word(g, c, &front, 0.0, 4.6, -0.5, fit_h(word, 3.2, 0.5), word, NEON_PINK);
    }
    // Tables in the middle, lights overhead, a drum fire on a cheap one.
    for _ in 0..5 {
        if let Some(p) = c.random_inside(rng, 3.0) {
            if p.y > area.v0 + 3.5 && p.y < area.v1 - 5.5 && c.all_inside(&rect(p.x, p.y, 1.3, 1.3)) {
                table(g, f, p, 0.0, mul(pal.wood, 0.9), if coin(rng, 0.4) { Some(pick(rng, &pal.tarp_colors)) } else { None });
            }
        }
    }
    let corners = [Vec2::new(area.u0 + 0.6, area.v0 + 0.6), Vec2::new(area.u1 - 0.6, area.v0 + 0.6), Vec2::new(area.u1 - 0.6, area.v1 - 0.6), Vec2::new(area.u0 + 0.6, area.v1 - 0.6)];
    for p in corners {
        if c.inside_local(p) {
            f.bx(g, Metal, p.x, 2.1, p.y, Vec3::new(0.12, 4.2, 0.12), pal.steel);
        }
    }
    for (i, j) in [(0usize, 2usize), (1, 3)] {
        lights_over(g, c, corners[i], corners[j], 4.1, 18, [1.5, 1.05, 0.5]);
    }
    if c.wear > 0.4 {
        if let Some(p) = c.random_inside(rng, 2.0) {
            drum_fire(g, f, pal, p);
        }
    }
}

// ── fine dining ──────────────────────────────────────────────────────────────

/// Dark stone or black glass, tall windows lit low and warm, a slim brass
/// line, a canopy over the door, planters, and a small discreet name.
pub fn fine_dining(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.15));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(16.0), area.d().min(14.0), (area.d() * 0.2).clamp(1.5, 4.0)) else { return };
    let n = floors(c, 1, 2);
    let fh = 4.2;
    let h = n as f32 * fh;
    let stone = c.tier != nd_sim::Tier::High;
    block(g, c, if stone { Stone } else { Glass }, &r, 0.0, h, if stone { mul(pal.stone, 0.45) } else { pal.glass_dark });
    let fr = r.front();
    let brass = [1.3, 1.0, 0.5];
    let k = ((fr.len - 2.0) / 2.4) as i32;
    for i in 0..k.max(1) {
        let tt = -fr.len * 0.5 + 1.0 + (i as f32 + 0.5) * (fr.len - 2.0) / k.max(1) as f32;
        if i == k / 2 {
            fr.decal(g, f, Wood, tt, 1.4, 0.06, 1.3, 2.8, mul(pal.wood, 0.4));
            continue;
        }
        fr.glaze(g, c, tt, 1.9, 0.06, 1.4, 3.0, Pane::Lit([1.1, 0.65, 0.3]));
    }
    for face in r.faces() {
        face.decal(g, f, Glow, 0.0, 3.7, 0.04, face.len * 0.96, 0.05, brass);
    }
    let reach = (c.reach_out(&fr) - 0.2).clamp(0.5, 2.5);
    fr.bx(g, f, Matte, 0.0, 3.2, reach * 0.5, Vec3::new(2.6, 0.12, reach), CHARCOAL);
    fr.bx(g, f, Glow, 0.0, 3.12, reach * 0.5, Vec3::new(2.0, 0.03, reach * 0.6), [1.2, 0.85, 0.45]);
    neon_word(g, c, &fr, -fr.len * 0.25, 3.45, 0.08, 0.28, b"MAISON", brass);
    // A slim brass blade by the door, lit from below.
    let bt = fr.len * 0.5 - 0.5;
    fr.bx(g, f, Metal, bt, 2.6, 0.5, Vec3::new(0.12, 3.2, 0.9), [0.12, 0.1, 0.08]);
    fr.bx(g, f, Glow, bt, 1.0, 0.5, Vec3::new(0.16, 0.08, 0.95), brass);
    // A roof terrace: planters round its edge, a pergola of warm lights.
    let roof = r.inset(1.2);
    if roof.w() > 4.0 && roof.d() > 4.0 {
        for face in roof.faces() {
            let n = (face.len / 2.2) as i32;
            for k in 0..n.max(1) {
                let p = face.at(-face.len * 0.5 + (k as f32 + 0.5) * face.len / n.max(1) as f32, -0.4);
                f.bx(g, Wood, p.x, h + 0.35, p.y, Vec3::new(0.9, 0.7, 0.9), mul(pal.wood, 0.5));
                f.bx(g, Foliage, p.x, h + 0.95, p.y, Vec3::new(0.8, 0.6, 0.8), mul(pal.foliage, 0.8));
            }
        }
        let (a, b) = (Vec2::new(roof.u0 + 0.5, roof.c().y), Vec2::new(roof.u1 - 0.5, roof.c().y));
        string_lights(g, f, Vec3::new(a.x, h + 2.6, a.y), Vec3::new(b.x, h + 2.6, b.y), 14, [1.5, 1.05, 0.55]);
        for p in [a, b] {
            f.bx(g, Metal, p.x, h + 1.3, p.y, Vec3::new(0.12, 2.6, 0.12), CHARCOAL);
        }
    }
    if n > 1 {
        for face in r.faces() {
            if face.len > 3.0 {
                pane_row(g, c, rng, &face, fh, n as i32 - 1, fh, 3.0, 1.6, 2.6, true);
            }
        }
    }
    // Planters either side of the door and along the front.
    let front_d = r.v0 - area.v0;
    if front_d > 1.2 {
        for s in [-1.0f32, 1.0] {
            let p = Vec2::new(r.c().x + s * 2.2, area.v0 + front_d * 0.5);
            planter(g, f, p, 0.45, CHARCOAL, mul(pal.foliage, 0.8));
        }
    }
    parapet(g, c, &r.poly(), h, 0.5, 0.2, Stone, mul(pal.stone, 0.4));
}

// ── bubble tea ───────────────────────────────────────────────────────────────

/// A small pastel kiosk with a wide serving window, a giant cup with a
/// straw on the roof, pearls in neon, stools at the counter.
pub fn bubble_tea(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.1));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(9.0), area.d().min(8.0), (area.d() * 0.3).clamp(1.0, 4.0)) else { return };
    let h = if floors(c, 1, 2) > 1 { 6.4 } else { 3.6 };
    let pastel = house(c, 0xB0, &[[0.95, 0.7, 0.75], [0.6, 0.85, 0.8], [0.95, 0.88, 0.6], [0.75, 0.7, 0.95]]);
    block(g, c, Panel, &r, 0.0, h, mul(pastel, 1.0 - 0.35 * c.wear));
    let fr = r.front();
    fr.glaze(g, c, 0.0, 1.6, 0.06, fr.len * 0.7, 1.6, Pane::Lit([1.3, 1.25, 1.2]));
    fr.bx(g, f, Panel, 0.0, 0.95, 0.35, Vec3::new(fr.len * 0.72, 0.1, 0.6), mul(pastel, 0.8));
    // Stools at the counter.
    for k in 0..3 {
        let p = fr.at(-fr.len * 0.25 + k as f32 * fr.len * 0.25, 0.9);
        if c.inside_local(p) {
            f.bx(g, Matte, p.x, 0.35, p.y, Vec3::new(0.35, 0.7, 0.35), CHARCOAL);
        }
    }
    let word: &[u8] = if c.seed % 2 == 0 { b"BOBA" } else { b"JUICE" };
    word_board(g, c, &fr, 0.0, 3.0, fr.len * 0.6, 0.55, word, NEON_PINK);
    if h > 4.0 {
        pane_row(g, c, rng, &fr, 3.6, 1, 2.8, 2.4, 1.4, 1.4, true);
    }
    // The cup: a tapered tumbler of boxes, a lid, a fat straw, pearls.
    let p = r.c();
    for k in 0..3 {
        let w = 1.4 - k as f32 * 0.15;
        f.bx(g, Panel, p.x, h + 0.45 + k as f32 * 0.8, p.y, Vec3::new(w, 0.8, w), [0.92, 0.9, 0.85]);
    }
    f.bx(g, Glow, p.x, h + 0.5, p.y, Vec3::new(1.42, 0.7, 1.42), mul([0.5, 0.3, 0.2], 1.0));
    f.bx(g, Panel, p.x, h + 2.55, p.y, Vec3::new(1.3, 0.15, 1.3), mul(pastel, 0.9));
    f.bx(g, Glow, p.x + 0.25, h + 3.3, p.y, Vec3::new(0.18, 1.4, 0.18), NEON_PINK);
    parapet(g, c, &r.poly(), h, 0.4, 0.15, Panel, mul(pastel, 0.75));
}

// ── bar ──────────────────────────────────────────────────────────────────────

/// A pub on the corner: brick or soot-dark render, small windows of warm
/// light, a hanging sign, neon in the window, kegs by the side door, a
/// bench and a smokers' ashtray out front.
pub fn bar(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.8));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(13.0), area.d().min(13.0), 0.8) else { return };
    let n = floors(c, 2, 3);
    let fh = 3.4;
    let h = n as f32 * fh;
    let low = c.tier == nd_sim::Tier::Low;
    block(g, c, if low { Brick } else { Plaster }, &r, 0.0, h, if low { mul(pick(rng, &pal.brick), 0.75) } else { [0.18, 0.2, 0.22] });
    let fr = r.front();
    fr.decal(g, f, Wood, 0.0, 1.5, 0.05, fr.len * 0.96, 3.0, if low { mul(pal.wood, 0.5) } else { [0.08, 0.2, 0.15] });
    let k = ((fr.len - 2.0) / 2.0) as i32;
    for i in 0..k.max(1) {
        let tt = -fr.len * 0.5 + 1.0 + (i as f32 + 0.5) * (fr.len - 2.0) / k.max(1) as f32;
        if i == k / 2 {
            fr.decal(g, f, Matte, tt, 1.1, 0.08, 1.1, 2.2, DARK);
            continue;
        }
        fr.glaze(g, c, tt, 1.7, 0.08, 1.4, 1.4, Pane::Lit([1.3, 0.7, 0.3]));
    }
    // Neon in the window.
    let col = house(c, 0xBA, &[NEON_RED, NEON_BLUE, NEON_GREEN, NEON_AMBER]);
    neon_word(g, c, &fr, -fr.len * 0.3, 1.9, 0.1, 0.32, b"OPEN", col);
    word_board(g, c, &fr, 0.0, 3.15, fr.len * 0.7, 0.55, b"BAR", col);
    // The hanging sign on its bracket.
    fr.bx(g, f, Metal, fr.len * 0.5 - 0.4, 3.9, 0.6, Vec3::new(0.08, 0.08, 1.2), CHARCOAL);
    fr.bx(g, f, Wood, fr.len * 0.5 - 0.4, 3.35, 1.05, Vec3::new(0.08, 0.9, 0.8), mul(pal.wood, 0.7));
    upper_floors(g, c, rng, &r, fh, n as i32 - 1, fh);
    // Kegs by the side, a bench at the front.
    let side = r.right();
    for k in 0..3 {
        let p = side.at(-side.len * 0.3 + k as f32 * 0.7, 0.6);
        if c.inside_local(p) {
            f.bx(g, Metal, p.x, 0.35, p.y, Vec3::new(0.5, 0.7, 0.5), pal.steel);
        }
    }
    if r.v0 - area.v0 > 1.0 {
        let p = fr.at(-fr.len * 0.3, 0.5);
        bench_world(g, f.to_world(p.x, p.y), f.yaw(), mul(pal.wood, 0.9), pal.steel);
    }
    graffiti(g, c, rng, &r.left(), 0.0, 2.6, 0.5 + c.wear);
    roof_gear(g, c, rng, &r.poly(), h, 0.5);
}

// ── karaoke ──────────────────────────────────────────────────────────────────

/// A stack of private rooms: each floor's windows lit in one colour (a
/// party in each), LED screens, a tall blade sign spelling it out.
pub fn karaoke(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.85));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(14.0), area.d().min(14.0), 0.8) else { return };
    let n = floors(c, 3, 7);
    let fh = 3.2;
    let h = n as f32 * fh;
    block(g, c, Concrete, &r, 0.0, h, mul(pick(rng, &pal.concrete), 0.6));
    let colours = [NEON_PINK, NEON_VIOLET, NEON_TEAL, NEON_AMBER, NEON_BLUE, NEON_RED];
    for fl in 0..n {
        let col = colours[(nd_sim::hash::mix(c.seed, fl as u64) % colours.len() as u64) as usize];
        let y = fl as f32 * fh + fh * 0.55;
        for face in r.faces() {
            let rooms = (face.len / 2.6) as i32;
            for k in 0..rooms.max(1) {
                let tt = -face.len * 0.5 + (k as f32 + 0.5) * face.len / rooms.max(1) as f32;
                if fl == 0 && face.is_front && tt.abs() < 2.0 {
                    continue;
                }
                let lit = coin(rng, 0.55 + 0.35 * c.lit_chance());
                face.glaze(g, c, tt, y, 0.05, 1.8, 1.4, if lit { Pane::Lit(mul(col, 0.7)) } else { Pane::Dark });
            }
            face.decal(g, f, Glow, 0.0, (fl + 1) as f32 * fh - 0.15, 0.06, face.len * 0.98, 0.08, mul(col, 0.8));
        }
    }
    let fr = r.front();
    entrance(g, c, rng, &fr, 0.0, 3.0, 2.8);
    // The blade sign up the corner.
    let word = b"KARAOKE";
    let bt = fr.len * 0.5 - 0.5;
    let bh = (h - 4.0).max(5.0);
    fr.bx(g, f, Matte, bt, 3.8 + bh * 0.5, 0.9, Vec3::new(0.3, bh, 1.4), DARK);
    let lh = (bh / word.len() as f32 * 0.75).min(0.9);
    let side = Face::new(fr.at(bt, 1.6), fr.at(bt, 0.2), Vec2::new(-1.0, 0.0));
    let other = Face::new(side.b, side.a, Vec2::new(1.0, 0.0));
    for (i, ch) in word.iter().enumerate() {
        let y = 3.8 + bh - (i as f32 + 0.6) * (bh / word.len() as f32);
        neon_word(g, c, &side, 0.0, y, 0.17, lh, &[*ch], NEON_PINK);
        neon_word(g, c, &other, 0.0, y, 0.17, lh, &[*ch], NEON_PINK);
    }
    if fr.len > 8.0 {
        led_screen(g, c, rng, &fr, -fr.len * 0.25, 4.6, 3.0, 2.0);
    }
    roof_gear(g, c, rng, &r.poly(), h, 0.7);
}

// ── cinema ───────────────────────────────────────────────────────────────────

/// A blank box of screens behind a glazed lobby, a marquee with the word in
/// lights, poster frames lit by the door, a blade sign at the corner.
pub fn multiplex(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, pal.paving);
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w() * 0.92, area.d() * 0.85, (area.d() * 0.12).clamp(1.0, 4.0)) else { return };
    let h = (floors(c, 2, 4) as f32 * 3.6).max(9.0);
    let lobby = R::new(r.u0, r.u1, r.v0, r.v0 + (r.d() * 0.3).clamp(3.0, 7.0));
    let hall = R::new(r.u0 + 0.5, r.u1 - 0.5, lobby.v1, r.v1);
    block(g, c, Panel, &hall, 0.0, h, mul(pick(rng, &pal.panel), 0.55));
    block(g, c, Glass, &lobby, 0.0, 6.0, pal.glass_dark);
    let fr = lobby.front();
    fr.glaze(g, c, 0.0, 2.6, 0.05, fr.len * 0.92, 4.4, Pane::Lit([1.0, 0.8, 0.55]));
    // The marquee: a deep canopy, the title board and its word.
    let reach = (c.reach_out(&fr) - 0.3).clamp(0.8, 3.0);
    fr.bx(g, f, Panel, 0.0, 4.6, reach * 0.5, Vec3::new(fr.len * 0.8, 0.4, reach), CHARCOAL);
    fr.bx(g, f, Glow, 0.0, 4.38, reach * 0.5, Vec3::new(fr.len * 0.75, 0.04, reach * 0.8), [1.3, 1.1, 0.75]);
    let mf = Face::new(fr.at(-fr.len * 0.4, reach), fr.at(fr.len * 0.4, reach), fr.out);
    mf.bx(g, f, Matte, 0.0, 5.3, -0.1, Vec3::new(fr.len * 0.8, 1.1, 0.15), DARK);
    neon_word(g, c, &mf, 0.0, 5.3, 0.02, fit_h(b"CINEMA", fr.len * 0.6, 0.8), b"CINEMA", NEON_WHITE);
    // Bulbs round the marquee board.
    let bulbs = (fr.len * 0.8 / 0.5) as i32;
    for k in 0..bulbs.min(40) {
        mf.decal(g, f, Glow, -fr.len * 0.4 + (k as f32 + 0.5) * 0.5, 5.92, 0.03, 0.12, 0.12, [1.6, 1.3, 0.7]);
    }
    // Poster frames on the hall's front above the lobby roof.
    let hf = hall.front();
    let posters = (hf.len / 4.0) as i32;
    for k in 0..posters.clamp(2, 6) {
        let tt = -hf.len * 0.4 + (k as f32 + 0.5) * hf.len * 0.8 / posters.clamp(2, 6) as f32;
        hf.decal(g, f, Matte, tt, 7.8, 0.06, 2.2, 3.0, DARK);
        hf.decal(g, f, Glow, tt, 7.8, 0.08, 1.9, 2.7, mul(pick(rng, &pal.led_colors), 0.7));
    }
    // Blade sign at the corner, both faces.
    let bt = hf.len * 0.5 - 0.6;
    let bh = (h - 3.0).max(5.0);
    hf.bx(g, f, Matte, bt, 2.0 + bh * 0.5, 0.9, Vec3::new(0.3, bh, 1.5), DARK);
    let side = Face::new(hf.at(bt, 1.7), hf.at(bt, 0.2), Vec2::new(-1.0, 0.0));
    let other = Face::new(side.b, side.a, Vec2::new(1.0, 0.0));
    let word = b"FILMS";
    for (i, ch) in word.iter().enumerate() {
        let y = 2.0 + bh - (i as f32 + 0.6) * (bh / word.len() as f32);
        let lh = (bh / word.len() as f32 * 0.7).min(1.0);
        neon_word(g, c, &side, 0.0, y, 0.17, lh, &[*ch], NEON_RED);
        neon_word(g, c, &other, 0.0, y, 0.17, lh, &[*ch], NEON_RED);
    }
    for face in [hall.left(), hall.right(), hall.back()] {
        if face.len > 6.0 {
            fire_escape(g, c, rng, &face, 0.0, 0.0, 2, h * 0.4);
            grime(g, c, rng, &face, 0.0, h, 1.0);
            break;
        }
    }
    roof_gear(g, c, rng, &hall.poly(), h, 0.6);
}

// ── theatre ──────────────────────────────────────────────────────────────────

/// A stone front with columns and a pediment, a lit marquee and banners,
/// the tall fly tower over the stage behind.
pub fn theatre(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 1.1));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w() * 0.9, area.d() * 0.88, (area.d() * 0.1).clamp(1.0, 4.0)) else { return };
    let h = (floors(c, 3, 5) as f32 * 3.4).max(10.0);
    let stone = mul(pal.stone, if c.tier == nd_sim::Tier::Low { 0.6 } else { 0.85 });
    let front_d = (r.d() * 0.35).clamp(4.0, 9.0);
    let foyer = R::new(r.u0, r.u1, r.v0, r.v0 + front_d);
    block(g, c, Stone, &foyer, 0.0, h, stone);
    let stage = R::new(r.u0 + r.w() * 0.15, r.u1 - r.w() * 0.15, foyer.v1, r.v1);
    if stage.d() > 3.0 {
        block(g, c, Concrete, &stage, 0.0, h * 1.5, mul(pick(rng, &pal.concrete), 0.55));
        roof_gear(g, c, rng, &stage.poly(), h * 1.5, 0.3);
    }
    let fr = foyer.front();
    // Columns across the front and a pediment over them.
    let cols = ((fr.len - 2.0) / 2.6) as i32;
    for k in 0..=cols.max(2) {
        let tt = -fr.len * 0.5 + 1.0 + k as f32 * (fr.len - 2.0) / cols.max(2) as f32;
        fr.bx(g, f, Stone, tt, h * 0.42, 0.4, Vec3::new(0.7, h * 0.84, 0.7), mul(stone, 1.1));
    }
    let ped = R::new(foyer.u0, foyer.u1, foyer.v0, foyer.v0 + 2.0);
    gable_roof(g, c, &ped.poly(), h, 2.0, mul(stone, 1.05));
    // Tall doors lit gold, the marquee and the word.
    for k in [-1.0f32, 0.0, 1.0] {
        fr.glaze(g, c, k * 2.4, 1.7, 0.06, 1.8, 3.2, Pane::Lit([1.4, 1.0, 0.5]));
    }
    let reach = (c.reach_out(&fr) - 0.3).clamp(0.8, 2.5);
    fr.bx(g, f, Metal, 0.0, 4.2, reach * 0.5, Vec3::new(fr.len * 0.6, 0.35, reach), [0.3, 0.1, 0.1]);
    fr.bx(g, f, Glow, 0.0, 4.0, reach * 0.5, Vec3::new(fr.len * 0.55, 0.04, reach * 0.8), [1.4, 1.1, 0.6]);
    word_board(g, c, &fr, 0.0, h * 0.72, fr.len * 0.6, 1.0, b"THEATRE", [1.5, 1.15, 0.6]);
    // Banners between the columns.
    let banner = house(c, 0x7E, &[[0.6, 0.08, 0.1], [0.1, 0.2, 0.45], [0.15, 0.35, 0.2]]);
    for k in [-1.0f32, 1.0] {
        fr.decal(g, f, Tarp, k * fr.len * 0.32, h * 0.45, 0.85, 0.9, h * 0.4, mul(banner, 1.0 - 0.3 * c.wear));
    }
    for face in [foyer.left(), foyer.right()] {
        pane_row(g, c, rng, &face, 4.0, ((h - 4.0) / 3.4) as i32, 3.4, 2.6, 1.2, 2.0, true);
        grime(g, c, rng, &face, 0.0, h, 0.8);
    }
    // Stage doors and the get-in at the back, the foyer's plant on its roof.
    if stage.d() > 3.0 {
        let back = stage.back();
        roller_door(g, c, &back, 0.0, 3.0, 3.6, mul(pal.steel, 0.6));
        fire_escape(g, c, rng, &stage.left(), 0.0, 0.0, 3, h * 0.45);
    }
    roof_gear(g, c, rng, &foyer.poly(), h, 0.4);
}

// ── live-music hall ──────────────────────────────────────────────────────────

/// A brick or steel hall under a barrel roof, its front plastered with gig
/// posters, a lit "LIVE" sign, a loading door at the side with the band's
/// van backed up to it.
pub fn music_hall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.25, Asphalt, mul(pal.asphalt, 1.0));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w() * 0.8, area.d() * 0.8, (area.d() * 0.12).clamp(1.0, 4.0)) else { return };
    let h = (floors(c, 2, 5) as f32 * 3.0).max(8.0);
    let low = c.tier == nd_sim::Tier::Low;
    block(g, c, if low { Brick } else { Metal }, &r, 0.0, h, if low { mul(pick(rng, &pal.brick), 0.7) } else { mul(pick(rng, &pal.metal_paint), 0.5) });
    // The barrel roof: steps of a shallow arch along the hall.
    let steps = 5;
    for k in 0..steps {
        let a = (k as f32 + 0.5) / steps as f32 * std::f32::consts::PI;
        let rise = a.sin() * (r.w() * 0.12).min(3.0);
        let u0 = r.u0 + r.w() * k as f32 / steps as f32;
        let u1 = r.u0 + r.w() * (k + 1) as f32 / steps as f32;
        block(g, c, Metal, &R::new(u0, u1, r.v0, r.v1), h, h + 0.3 + rise, mul(pal.steel, 0.7));
    }
    let fr = r.front();
    entrance(g, c, rng, &fr, -fr.len * 0.2, 3.0, 3.0);
    // Gig posters pasted up the front, layered.
    let cols = ((fr.len * 0.5) / 1.1) as i32;
    for i in 0..cols.max(2) {
        for j in 0..2 {
            let tt = fr.len * 0.05 + i as f32 * 1.1;
            if tt > fr.len * 0.45 {
                break;
            }
            let col = mix(pick(rng, &pal.tarp_colors), [0.9, 0.9, 0.85], 0.3);
            fr.decal(g, f, Matte, tt, 1.2 + j as f32 * 1.6, 0.04 + 0.005 * (i + j) as f32, 0.9, 1.3, mul(col, 1.0 - 0.4 * c.wear));
        }
    }
    word_board(g, c, &fr, 0.0, h - 1.2, fr.len * 0.5, 1.4, b"LIVE", NEON_RED);
    neon_word(g, c, &fr, -fr.len * 0.2, 3.9, 0.12, 0.35, b"TONIGHT", NEON_AMBER);
    // The loading door and the van.
    let side = r.right();
    if side.len > 5.0 {
        roller_door(g, c, &side, side.len * 0.25, 3.2, 3.4, mul(pick(rng, &pal.metal_paint), 0.6));
        let p = side.at(side.len * 0.25, 3.0);
        if vehicle_fits(c, p, 0.0, Vehicle::Van) {
            vehicle(g, f, pal, rng, p, 0.0, 0.0, Vehicle::Van, [0.12, 0.12, 0.14]);
        }
    }
    graffiti(g, c, rng, &r.left(), 0.0, 3.0, 1.0 + c.wear);
    grime(g, c, rng, &fr, 0.0, h, 0.8);
}

// ── bowling alley ────────────────────────────────────────────────────────────

/// A long low box with a strip of windows, a giant pin on the roof, the
/// word in big letters, and its car park.
pub fn bowling_alley(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.2, Asphalt, pal.asphalt);
    let area = R::of(&c.poly);
    let park = (area.d() * 0.3).clamp(3.0, 10.0);
    let Some(r) = street_block(c, area.w() * 0.9, area.d() - park, park) else { return };
    let h = if floors(c, 1, 2) > 1 { 7.0 } else { 5.5 };
    let paint = house(c, 0xB1, &[[0.15, 0.25, 0.45], [0.55, 0.15, 0.15], [0.2, 0.2, 0.22], [0.15, 0.4, 0.35]]);
    block(g, c, Panel, &r, 0.0, h, mul(paint, 1.0 - 0.3 * c.wear));
    let fr = r.front();
    for face in r.faces() {
        face.decal(g, f, Glow, 0.0, h - 0.3, 0.05, face.len * 0.98, 0.12, NEON_TEAL);
    }
    fr.glaze(g, c, -fr.len * 0.2, 1.6, 0.06, fr.len * 0.45, 1.8, Pane::Lit([1.2, 1.0, 1.3]));
    entrance(g, c, rng, &fr, fr.len * 0.25, 2.6, 2.6);
    word_board(g, c, &fr, -fr.len * 0.1, h + 0.8, fr.len * 0.6, 1.4, b"BOWL", NEON_RED);
    // The giant pin: white stacked boxes with a red neck band.
    let p = Vec2::new(r.u1 - 2.0, r.v0 + 2.0);
    let pin = [(0.9, 1.2), (1.2, 1.6), (0.7, 0.8), (0.55, 0.6), (0.8, 0.9)];
    let mut y = h;
    for (k, (w, hh)) in pin.iter().enumerate() {
        let col = if k == 2 { NEON_RED } else { [0.95, 0.95, 0.92] };
        f.bx(g, if k == 2 { Glow } else { Panel }, p.x, y + hh * 0.5, p.y, Vec3::new(*w, *hh, *w), col);
        y += hh;
    }
    // Bays and a few cars.
    let n = ((area.w() - 2.0) / 2.8) as i32;
    for k in 0..n.min(10) {
        let q = Vec2::new(area.u0 + 1.4 + k as f32 * 2.8, area.v0 + park * 0.5);
        if c.inside_local(q) {
            let half = (park - 1.0).min(4.5) * 0.5;
            ground_line(g, c, Vec2::new(q.x - 1.4, q.y - half), Vec2::new(q.x - 1.4, q.y + half), mul(pal.lane_marker, 0.5));
            if coin(rng, 0.45) && vehicle_fits(c, q, std::f32::consts::FRAC_PI_2, Vehicle::Car) {
                let paint = pick(rng, &pal.car_colors);
                vehicle(g, f, pal, rng, q, 0.0, std::f32::consts::FRAC_PI_2, Vehicle::Car, paint);
            }
        }
    }
    roof_gear(g, c, rng, &r.poly(), h, 0.4);
    grime(g, c, rng, &r.left(), 0.0, h, 1.0);
}

// ── arcade ───────────────────────────────────────────────────────────────────

/// A front wall of screens and pixel blocks, the cabinets' glow through a
/// wide window, "ARCADE" in big letters and "VR" up the side.
pub fn arcade(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.85));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(16.0), area.d().min(16.0), 0.8) else { return };
    let n = floors(c, 1, 3);
    let fh = 3.8;
    let h = n as f32 * fh;
    block(g, c, Concrete, &r, 0.0, h, mul(pick(rng, &pal.concrete), 0.45));
    let fr = r.front();
    fr.glaze(g, c, 0.0, 1.6, 0.06, fr.len * 0.8, 2.4, Pane::Lit([0.6, 0.4, 1.2]));
    // The cabinets inside: a row of coloured screens behind the glass.
    let cabs = (fr.len * 0.75 / 1.0) as i32;
    for k in 0..cabs.min(14) {
        let col = mul(pick(rng, &pal.led_colors), 0.9);
        fr.decal(g, f, Glow, -fr.len * 0.37 + (k as f32 + 0.5) * fr.len * 0.74 / cabs.max(1) as f32, 1.4, 0.09, 0.55, 0.5, col);
    }
    word_board(g, c, &fr, 0.0, 3.3, fr.len * 0.8, 0.8, b"ARCADE", NEON_VIOLET);
    // Pixel blocks over the front.
    let px = 0.5;
    let cols = (fr.len * 0.8 / px) as i32;
    for i in 0..cols.min(32) {
        for j in 0..4 {
            if !coin(rng, 0.35) {
                continue;
            }
            let col = pick(rng, &pal.led_colors);
            fr.decal(g, f, Glow, -fr.len * 0.4 + (i as f32 + 0.5) * px, 4.0 + j as f32 * px, 0.05, px * 0.9, px * 0.9, mul(col, 0.8));
        }
    }
    if n > 1 {
        led_screen(g, c, rng, &fr, 0.0, fh + fh * 0.75, (fr.len * 0.6).min(8.0), fh * 0.8);
    }
    let side = r.left();
    if side.len > 4.0 {
        neon_word(g, c, &side, 0.0, h * 0.5, 0.06, (h * 0.35).min(2.0), b"VR", NEON_TEAL);
    }
    graffiti(g, c, rng, &r.right(), 0.0, 2.5, 0.8 + c.wear);
    roof_gear(g, c, rng, &r.poly(), h, 0.5);
}

// ── gym ──────────────────────────────────────────────────────────────────────

/// A glazed box: the machines' rows lit inside, a giant dumbbell over the
/// door, "GYM" in hard white, a running track painted on the roof.
pub fn gym(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, pal.paving);
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w() * 0.85, area.d() * 0.75, (area.d() * 0.15).clamp(1.0, 4.0)) else { return };
    let n = floors(c, 1, 3);
    let fh = 4.0;
    let h = n as f32 * fh;
    block(g, c, Panel, &r, 0.0, h, mul(pick(rng, &pal.panel), 0.35));
    for fl in 0..n {
        for face in r.faces() {
            if face.len < 4.0 {
                continue;
            }
            let y = fl as f32 * fh + fh * 0.5;
            face.glaze(g, c, 0.0, y, 0.05, face.len * 0.85, fh * 0.65, Pane::Lit([1.0, 1.15, 1.25]));
            // The machines: dark blocks against the light.
            let m = (face.len * 0.8 / 1.4) as i32;
            for k in 0..m.min(12) {
                face.decal(g, f, Matte, -face.len * 0.4 + (k as f32 + 0.5) * face.len * 0.8 / m.max(1) as f32, fl as f32 * fh + 0.9, 0.08, 0.7, 1.1, CHARCOAL);
            }
        }
    }
    let fr = r.front();
    fr.decal(g, f, Matte, 0.0, 1.1, 0.1, 1.6, 2.2, DARK);
    word_board(g, c, &fr, -fr.len * 0.2, h + 0.7, fr.len * 0.4, 1.1, b"GYM", NEON_WHITE);
    // The dumbbell on the parapet.
    let p = fr.at(fr.len * 0.22, -0.5);
    for s in [-1.0f32, 1.0] {
        let q = p + fr.tangent * s * 1.1;
        f.bx(g, Metal, q.x, h + 0.8, q.y, Vec3::new(0.5, 1.4, 1.4), CHARCOAL);
    }
    f.bx_yaw(g, Metal, p.x, h + 0.8, p.y, Vec3::new(2.2, 0.25, 0.25), pal.steel, fr.yaw());
    f.bx(g, Glow, p.x, h + 0.1, p.y, Vec3::new(2.6, 0.06, 0.3), NEON_GREEN);
    // The track on the roof: a ring of red with white lines.
    let roof = r.inset(1.0);
    if roof.w() > 6.0 && roof.d() > 6.0 {
        for face in roof.faces() {
            face.bx(g, f, Matte, 0.0, h + 0.03, -0.6, Vec3::new(face.len, 0.04, 1.2), [0.55, 0.18, 0.12]);
            face.bx(g, f, Matte, 0.0, h + 0.06, -0.6, Vec3::new(face.len - 1.0, 0.02, 0.05), [0.9, 0.9, 0.88]);
        }
    }
    parapet(g, c, &r.poly(), h, 1.0, 0.2, Glass, pick(rng, &pal.glass));
}

// ── comedy club ──────────────────────────────────────────────────────────────

/// Old brick with a basement door down a few steps, a red neon "COMEDY", a
/// marquee of bulbs over it, posters of tonight's comics.
pub fn comedy_club(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.8));
    let area = R::of(&c.poly);
    let Some(r) = street_block(c, area.w().min(13.0), area.d().min(14.0), 1.5) else { return };
    let n = floors(c, 2, 3);
    let fh = 3.4;
    let h = n as f32 * fh;
    block(g, c, Brick, &r, 0.0, h, mul(pick(rng, &pal.brick), 0.65));
    let fr = r.front();
    // The way down: a dark well in front of the door, railings, a lamp.
    let well = R::new(r.c().x - 1.2, r.c().x + 1.2, r.v0 - 1.4, r.v0);
    if poly_inside(c, &well.poly()) {
        ground(g, f, Concrete, &well.poly(), 0.04, DARK);
        for s in [-1.0f32, 1.0] {
            let p = Vec2::new(r.c().x + s * 1.25, r.v0 - 0.7);
            f.bx(g, Metal, p.x, 0.5, p.y, Vec3::new(0.05, 1.0, 1.4), CHARCOAL);
        }
    }
    fr.decal(g, f, Matte, 0.0, 1.1, 0.06, 1.3, 2.3, DARK);
    fr.decal(g, f, Glow, 0.0, 2.45, 0.08, 1.2, 0.1, NEON_RED);
    // Bulbs round a board with the word in red.
    let bw = fr.len * 0.55;
    fr.bx(g, f, Matte, 0.0, 3.2, 0.15, Vec3::new(bw, 1.0, 0.2), DARK);
    neon_word(g, c, &fr, 0.0, 3.2, 0.27, fit_h(b"COMEDY", bw * 0.85, 0.6), b"COMEDY", NEON_RED);
    let bulbs = (bw / 0.45) as i32;
    for k in 0..bulbs.min(30) {
        let tt = -bw * 0.5 + (k as f32 + 0.5) * bw / bulbs.max(1) as f32;
        fr.decal(g, f, Glow, tt, 3.8, 0.27, 0.12, 0.12, [1.6, 1.3, 0.7]);
        fr.decal(g, f, Glow, tt, 2.6, 0.27, 0.12, 0.12, [1.6, 1.3, 0.7]);
    }
    // Tonight's posters either side.
    for s in [-1.0f32, 1.0] {
        fr.decal(g, f, Matte, s * fr.len * 0.36, 1.6, 0.05, 1.0, 1.4, mul([0.9, 0.85, 0.6], 1.0 - 0.4 * c.wear));
        fr.decal(g, f, Glow, s * fr.len * 0.36, 2.45, 0.07, 1.0, 0.06, NEON_AMBER);
    }
    upper_floors(g, c, rng, &r, fh, n as i32 - 1, fh);
    fire_escape(g, c, rng, &r.right(), 0.0, fh, n as i32 - 1, fh);
    graffiti(g, c, rng, &fr, 0.0, 2.5, 0.6 + c.wear);
    roof_gear(g, c, rng, &r.poly(), h, 0.5);
}
