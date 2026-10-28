//! [lantern] Lantern Alley, the Lantern Lanes story's look: the low,
//! crowded lanes of a night-market quarter. Shophouses with flats over the
//! shop behind an arcade; narrow walk-ups either side of an alley strung
//! with lanterns and washing; stall rows under patchwork tarps; a food
//! court hall; a tea house under stacked tiled eaves; a night-market hall
//! behind a painted gateway; small workshops and back-alley kitchens; and,
//! for the families and billionaires who buy up the lanes, a courtyard
//! shophouse compound.
//!
//! Stylised and gritty (docs/STYLE_REFERENCE.md, section 1, the low
//! forms): stained render and tile, roller shutters, AC units, laundry,
//! tangled wires. Paper lanterns are strung across what the lot holds (an
//! alley between its two blocks, a courtyard, the aisles of a market, the
//! arcade along its frontage): a red paper body by day (mid detail) and a
//! warm light just round it after dark (`NIGHT_MARK`, base mesh, so a lit
//! lane reads from across the city). Everything is drawn from the
//! building's seed, the same from every camera; the grain (frames, AC
//! units, washing, crates, pipes) goes in the detail layers.

use super::hk::{self, FlatDress};
use super::kit::*;
use super::parts::*;
use super::{Ctx, Face};
use crate::surface::{Geometry, Lod, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::zoning::{point_in_polygon, polygon_area};
use rand::Rng;

/// Paper by day: red, deep red, orange, a faded cream.
const PAPER: [[f32; 3]; 4] = [[0.66, 0.10, 0.07], [0.50, 0.07, 0.06], [0.78, 0.34, 0.08], [0.80, 0.74, 0.60]];
/// Their light after dark, by the same index.
const LIGHT: [[f32; 3]; 4] = [[2.6, 0.55, 0.20], [2.3, 0.38, 0.16], [2.6, 1.10, 0.30], [2.2, 1.70, 0.95]];
/// Mostly red.
const PAPER_ODDS: [usize; 8] = [0, 0, 0, 1, 1, 2, 2, 3];
/// Roof tiles: glazed grey-green, terracotta, weathered black.
const TILES: [[f32; 3]; 3] = [[0.22, 0.27, 0.24], [0.44, 0.21, 0.14], [0.17, 0.16, 0.16]];
/// Lacquered timber: red, dark green, black.
const LACQUER: [[f32; 3]; 3] = [[0.52, 0.09, 0.06], [0.10, 0.26, 0.18], [0.11, 0.10, 0.10]];
/// Shop light behind the counter: warm, amber, a kitchen's orange.
const SHOP_LIGHT: [[f32; 3]; 3] = [[1.5, 0.95, 0.5], [1.45, 1.1, 0.62], [1.4, 0.62, 0.32]];

/// Lot-local (u, y, v).
fn p3(u: f32, y: f32, v: f32) -> Vec3 {
    Vec3::new(u, y, v)
}

/// A paper colour index.
fn paper(rng: &mut impl Rng) -> usize {
    PAPER_ODDS[rng.random_range(0..PAPER_ODDS.len())]
}

/// The footprint the lanes build on: the lot pulled in `inset` (closer to
/// the street than the land use's setback: shophouses meet the pavement),
/// or the land use's own footprint when that leaves too little.
fn site(c: &Ctx, inset: f32) -> Vec<Vec2> {
    let p = clean_poly(&offset_poly(&c.lot_poly, inset), 0.2);
    if p.len() >= 3 && polygon_area(&p).abs() > 12.0 { p } else { c.poly.clone() }
}

/// The strip of a footprint behind a face: `t0..t1` along it, `d0..d1`
/// in from it, counter-clockwise like the footprint.
fn strip(front: &Face, t0: f32, t1: f32, d0: f32, d1: f32) -> Vec<Vec2> {
    vec![front.at(t0, -d0), front.at(t1, -d0), front.at(t1, -d1), front.at(t0, -d1)]
}

fn all_in(poly: &[Vec2], pts: &[Vec2]) -> bool {
    pts.iter().all(|p| point_in_polygon(*p, poly))
}

/// A wall finish: stained render, tile or bare concrete, from the look's
/// walls and the palette's plaster.
fn wall(c: &Ctx, rng: &mut impl Rng) -> (Surface, [f32; 3]) {
    let col = match rng.random_range(0..3) {
        0 => pick(rng, &c.style.wall_colors),
        1 => hk::tile_tone(c, rng),
        _ => mix(pick(rng, &c.palette.plaster), pick(rng, &c.style.wall_colors), 0.5),
    };
    let s = match rng.random_range(0..4) {
        0 => Panel,
        1 => Concrete,
        _ => Plaster,
    };
    (s, mul(col, 1.0 - 0.25 * c.wear))
}

fn tiles(c: &Ctx, rng: &mut impl Rng) -> [f32; 3] {
    mul(TILES[rng.random_range(0..TILES.len())], 1.0 - 0.2 * c.wear)
}

// ── lanterns ─────────────────────────────────────────────────────────────────

/// One paper lantern hung from lot-local `top`, `s` across: its paper body,
/// cap and tassel (mid detail) and its light, a hair larger round the body
/// and lit only after dark (base mesh). `lit` false: a dead one.
pub(crate) fn lantern(g: &mut Geometry, c: &Ctx, top: Vec3, s: f32, k: usize, lit: bool) {
    let f = &c.frame;
    let h = s * 1.3;
    let y = top.y - h * 0.5;
    let k = k % PAPER.len();
    g.lod(Lod::Mid, |g| {
        f.bx(g, Tarp, top.x, y, top.z, Vec3::new(s, h, s), mul(PAPER[k], 1.0 - 0.35 * c.wear));
        f.bx(g, Matte, top.x, top.y + 0.04, top.z, Vec3::new(s * 0.55, 0.08, s * 0.55), DARK);
    });
    g.lod(Lod::Near, |g| f.bx(g, Matte, top.x, top.y - h - 0.1, top.z, Vec3::new(0.05, 0.2, 0.05), [0.72, 0.52, 0.14]));
    if lit {
        let from = g.part(Glow).positions.len();
        f.bx(g, Glow, top.x, y, top.z, Vec3::new(s * 1.06, h * 1.03, s * 1.06), mul(LIGHT[k], 1.0 - 0.3 * c.wear));
        mark_glow(g, from, NIGHT_MARK);
    }
}

/// A string of lanterns on a sagging wire between lot-local anchors, about
/// `spacing` apart: one colour or a mix, a few dead on a run-down lane.
pub(crate) fn lantern_string(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, a: Vec3, b: Vec3, sag: f32, spacing: f32) {
    let len = a.distance(b);
    if len < 1.0 {
        return;
    }
    let f = &c.frame;
    let (wa, wb) = (f.at(a.x, a.y, a.z), f.at(b.x, b.y, b.z));
    g.lod(Lod::Mid, |g| g.part(Matte).wire(wa, wb, sag, 0.03, rgba(DARK)));
    // The sag the wire itself is drawn with (`MeshData::wire` clamps it).
    let steep = ((b.y - a.y) / len).abs();
    let sag = if steep > 1e-3 { sag.min(0.8 * len / (4.0 * steep)) } else { sag };
    let n = ((len / spacing).round() as i32).max(2);
    let k0 = paper(rng);
    let mixed = coin(rng, 0.4);
    for i in 1..n {
        let t = i as f32 / n as f32;
        let p = a.lerp(b, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t) + 0.05);
        let k = if mixed { paper(rng) } else { k0 };
        let lit = !coin(rng, 0.35 * c.wear);
        lantern(g, c, p, 0.4, k, lit);
    }
}

/// A lantern on a short bracket out from a face at `t`, `y` (the bracket's
/// height).
fn bracket_lantern(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, out: f32) {
    let f = &c.frame;
    g.lod(Lod::Near, |g| face.bx(g, f, Matte, t, y, out * 0.5, Vec3::new(0.05, 0.05, out), CHARCOAL));
    let p = face.at(t, out);
    let lit = !coin(rng, 0.3 * c.wear);
    lantern(g, c, p3(p.x, y - 0.06, p.y), 0.42, paper(rng), lit);
}

// ── street level ─────────────────────────────────────────────────────────────

/// A shop on a ground-floor face `h` high: rolled up on a lit counter or a
/// shutter down (tagged), a door, and either a name board over it or (out
/// on the street, not under an arcade) a striped awning and a sign hung
/// out at right angles; a lantern either side.
fn shop(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, h: f32, arcade: bool) {
    let f = &c.frame;
    let pal = c.palette;
    let w = face.len * 0.84;
    if w < 1.6 {
        return;
    }
    let sh = h * 0.66;
    let shutter = mul(pick(rng, &pal.metal_paint), 0.72 - 0.25 * c.wear);
    if coin(rng, 0.72 - 0.4 * c.wear) {
        face.glaze(g, c, 0.0, sh * 0.5 + 0.05, 0.04, w, sh, Pane::Dark);
        let wc = SHOP_LIGHT[rng.random_range(0..SHOP_LIGHT.len())];
        face.glaze(g, c, 0.0, sh * 0.5 + 0.05, 0.06, w * 0.96, sh * 0.9, Pane::Lit(mul(wc, rng.random_range(0.55..0.8))));
        // The shutter rolled up into its drum.
        g.lod(Lod::Mid, |g| hk::wall_box(g, c, Metal, face, 0.0, sh + 0.15, 0.0, Vec3::new(w, 0.26, 0.24), mul(shutter, 0.8)));
    } else {
        face.decal(g, f, Metal, 0.0, sh * 0.5 + 0.05, 0.075, w, sh, shutter);
        g.lod(Lod::Mid, |g| hk::wall_box(g, c, Metal, face, 0.0, sh + 0.15, 0.0, Vec3::new(w, 0.26, 0.24), mul(shutter, 0.8)));
        graffiti(g, c, rng, face, 0.0, sh, 0.5 + c.wear);
    }
    let reach = (c.reach_out(face) + 0.5).min(1.5);
    if !arcade && reach > 0.8 && coin(rng, 0.45) {
        let col = mul(pick(rng, &pal.awning_colors), 1.0 - 0.4 * c.wear);
        let stripe = coin(rng, 0.6).then(|| mix(col, [0.86, 0.84, 0.78], 0.7));
        g.lod(Lod::Mid, |g| hk::awning(g, c, face, 0.0, sh + 0.42, w * 0.92, reach - 0.3, col, stripe));
        if face.len > 3.0 {
            hanging_sign(g, c, rng, face, face.len * 0.5 - 0.25, h + 1.3, reach.min(1.2), 1.1);
        }
    } else {
        sign_board(g, c, rng, face, 0.0, h - 0.38, (w * 0.8).min(5.0), 0.4);
    }
    for side in [-1.0f32, 1.0] {
        bracket_lantern(g, c, rng, face, side * (w * 0.5 + 0.05).min(face.len * 0.5 - 0.3), h - 0.25, 0.4);
    }
}

/// A one-storey kitchen or store at the back of a lot: block walls, a tin
/// roof, a lit doorway with a bulb, a flue, gas bottles by the door.
fn kitchen_shed(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2]) {
    let f = &c.frame;
    let pal = c.palette;
    if poly.len() < 3 || polygon_area(poly).abs() < 6.0 {
        return;
    }
    let h = 2.8;
    extrude(g, f, Concrete, poly, 0.0, h, mul(pick(rng, &pal.concrete), 0.8 - 0.2 * c.wear));
    let tin = if c.wear > 0.4 { (Rust, pal.rust) } else { (Metal, mul(pick(rng, &pal.metal_paint), 0.7)) };
    let roof = offset_poly(poly, -0.2);
    loft(g, f, tin.0, &roof, &roof, h + 0.05, h + 0.2, tin.1);
    let front = c.front_face(poly);
    if front.len > 2.0 {
        front.glaze(g, c, 0.0, 1.1, 0.04, 1.0, 2.1, Pane::Lit(mul(SHOP_LIGHT[2], 0.7)));
        front.bx(g, f, Glow, 0.0, 2.45, 0.2, Vec3::new(0.2, 0.2, 0.2), mul(pal.lamp, 0.9));
        g.lod(Lod::Near, |g| {
            for k in 0..3 {
                let t = 0.9 + k as f32 * 0.4;
                if t < front.len * 0.5 - 0.2 {
                    let p = front.at(t, 0.35);
                    g.part(Matte).cylinder(f.to_world(p.x, p.y), 0.16, 0.16, 0.0, 0.9, 8, rgba([0.62, 0.2, 0.12]));
                }
            }
        });
    }
    // The flue, up through the tin.
    let q = poly_center(poly);
    f.bx(g, Metal, q.x, h + 0.9, q.y, Vec3::new(0.4, 1.8, 0.4), mul(pal.steel, 0.7));
}

/// A painted gateway (paifang) across lot-local `cu`, `v`: lacquered posts
/// on stone plinths, two beams, a lit name board, a tiled roof with
/// upturned ends, and a big lantern hung in each bay.
#[allow(clippy::too_many_arguments)]
fn gateway(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, cu: f32, v: f32, w: f32, h: f32, lacquer: [f32; 3], tile: [f32; 3]) {
    let f = &c.frame;
    let posts = if w > 7.0 { 4 } else { 2 };
    let stone = mul(pick(rng, &c.palette.concrete), 0.9);
    for i in 0..posts {
        let u = cu - w * 0.5 + i as f32 * w / (posts - 1) as f32;
        f.bx(g, Wood, u, h * 0.5, v, Vec3::new(0.42, h, 0.42), lacquer);
        f.bx(g, Concrete, u, 0.3, v, Vec3::new(0.72, 0.6, 0.72), stone);
    }
    f.bx(g, Wood, cu, h - 0.3, v, Vec3::new(w + 0.8, 0.45, 0.5), mul(lacquer, 0.85));
    f.bx(g, Wood, cu, h - 1.45, v, Vec3::new(w, 0.28, 0.36), mul(lacquer, 0.85));
    // The name board between the beams, lit both sides.
    let bw = (w * 0.45).clamp(1.6, 5.0);
    f.bx(g, Matte, cu, h - 0.88, v, Vec3::new(bw, 0.62, 0.24), DARK);
    let ink = mul(pick(rng, &c.palette.sign_colors), 1.0 - 0.4 * c.wear);
    let n = ((bw / 0.6).floor() as i32).clamp(2, 7);
    let board = Face::new(Vec2::new(cu - 1.0, v - 0.12), Vec2::new(cu + 1.0, v - 0.12), Vec2::new(0.0, -1.0));
    for sf in [board, Face::new(Vec2::new(cu - 1.0, v + 0.12), Vec2::new(cu + 1.0, v + 0.12), Vec2::new(0.0, 1.0))] {
        let mid = sf.mid();
        let off = (Vec2::new(cu, v) - mid).dot(sf.tangent);
        for i in 0..n {
            let t = off - bw * 0.42 + (i as f32 + 0.5) * bw * 0.84 / n as f32;
            sf.decal(g, f, Glow, t, h - 0.88, 0.012, bw * 0.84 / n as f32 * 0.7, 0.42, ink);
        }
    }
    // The tiled roof on the top beam.
    let bottom = rect(cu, v, w * 0.5 + 1.0, 0.9);
    let top = rect(cu, v, w * 0.5 + 0.25, 0.06);
    loft(g, f, Brick, &bottom, &top, h - 0.04, h + 0.85, tile);
    g.lod(Lod::Mid, |g| {
        for side in [-1.0f32, 1.0] {
            let a = f.at(cu + side * (w * 0.5 + 0.3), h + 0.85, v);
            let b = f.at(cu + side * (w * 0.5 + 0.9), h + 1.25, v);
            g.part(Brick).slab(a, b, 0.3, 0.14, rgba(mul(tile, 0.8)));
        }
    });
    for i in 0..posts - 1 {
        let u = cu - w * 0.5 + (i as f32 + 0.5) * w / (posts - 1) as f32;
        // Not under the name board.
        if (u - cu).abs() < bw * 0.5 + 0.4 {
            continue;
        }
        lantern(g, c, p3(u, h - 1.6, v), 0.62, paper(rng), !coin(rng, 0.3 * c.wear));
    }
    if posts == 2 {
        for side in [-1.0f32, 1.0] {
            lantern(g, c, p3(cu + side * (bw * 0.5 + 0.5).min(w * 0.5 - 0.5), h - 1.6, v), 0.5, paper(rng), !coin(rng, 0.3 * c.wear));
        }
    }
}

/// The widest gateway, up to `w`, whose roof (and its upturned ends)
/// stands on the lot at `cu`, `v`; `None` when not even a narrow one does.
fn gate_width(c: &Ctx, cu: f32, v: f32, w: f32) -> Option<f32> {
    let mut w = w;
    while w >= 2.4 {
        let (hw, hd) = (w * 0.5 + 1.3, 0.95);
        if c.all_inside(&[Vec2::new(cu - hw, v - hd), Vec2::new(cu + hw, v - hd), Vec2::new(cu + hw, v + hd), Vec2::new(cu - hw, v + hd)]) {
            return Some(w);
        }
        w -= 0.8;
    }
    None
}

/// A hipped tile roof on a rectangle: eaves `over` out from the walls at
/// `y`, a short ridge along the longer side `rise` up, the ridge beam and
/// upturned corners.
fn hip_roof(g: &mut Geometry, c: &Ctx, r: &R, y: f32, rise: f32, over: f32, tile: [f32; 3]) {
    let f = &c.frame;
    let q = r.c();
    let (hw, hd) = (r.w() * 0.5, r.d() * 0.5);
    let bottom = rect(q.x, q.y, hw + over, hd + over);
    let (rw, rd) = if hw >= hd { ((hw - hd).max(0.3), 0.06) } else { (0.06, (hd - hw).max(0.3)) };
    loft(g, f, Brick, &bottom, &rect(q.x, q.y, rw, rd), y, y + rise, tile);
    f.bx(g, Brick, q.x, y + rise + 0.08, q.y, Vec3::new(rw * 2.0 + 0.3, 0.16, rd * 2.0 + 0.3), mul(tile, 0.7));
    g.lod(Lod::Mid, |g| {
        for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let corner = Vec2::new(q.x + sx * (hw + over), q.y + sz * (hd + over));
            let dir = Vec2::new(sx, sz).normalize();
            let a = corner - dir * 0.5;
            let b = corner + dir * 0.35;
            g.part(Brick).slab(f.at(a.x, y + 0.12, a.y), f.at(b.x, y + 0.5, b.y), 0.3, 0.1, rgba(mul(tile, 0.85)));
        }
    });
}

// ── shophouses ───────────────────────────────────────────────────────────────

/// A row of shophouses: narrow units of two to six storeys (as tall as the
/// block where it was built taller), each its own
/// finish and height, flats over a shop behind an arcade of pillars (on a
/// deep enough lot), a lantern string along the arcade, AC units, washing
/// and frames per flat, a vertical sign here and there, tile roofs or flat
/// roofs with tanks and aerials, a kitchen shed out the back.
pub fn shophouses(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let site = site(c, 0.6);
    let front = c.front_face(&site);
    let units = ((front.len / 5.2).floor() as i32).clamp(1, 8);
    let uw = front.len / units as f32;
    let deep = (c.fd * 0.8).clamp(4.0, 15.0);
    let tile = tiles(c, rng);
    let pillar = mul(pick(rng, &pal.concrete), 0.85);
    for i in 0..units {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * uw;
        // A party-wall gap wider than a parapet's overlap at the corners.
        let (t0, t1) = (t - uw * 0.5 + 0.1, t + uw * 0.5 - 0.1);
        let mut d = deep;
        while d > 3.0 && !all_in(&site, &[front.at(t0, -d), front.at(t1, -d)]) {
            d *= 0.85;
        }
        if d <= 3.0 || t1 - t0 < 2.5 {
            continue;
        }
        let floors = (c.floors as i32 + rng.random_range(-1..=1)).clamp(2, (c.floors as i32 + 1).max(6));
        let h = floors as f32 * fh;
        let (s, col) = wall(c, rng);
        let arc = if d > 7.5 && t1 - t0 > 3.2 { 1.8 } else { 0.0 };
        extrude(g, f, Concrete, &strip(&front, t0, t1, arc, d), 0.0, fh, mul(col, 0.8));
        extrude(g, f, s, &strip(&front, t0, t1, 0.0, d), fh, h, col);
        let upper = Face::from_edge(front.at(t0, 0.0), front.at(t1, 0.0));
        let ground = Face::from_edge(front.at(t0, -arc), front.at(t1, -arc));
        shop(g, c, rng, &ground, fh, arc > 0.0);
        if arc > 0.0 {
            for tp in [t0 + 0.2, t1 - 0.2] {
                front.bx(g, f, Concrete, tp, fh * 0.5, -0.22, Vec3::new(0.36, fh, 0.36), pillar);
            }
            ground_at(g, c, &strip(&front, t0, t1, 0.0, arc), 0.035, mul(pal.paving, 0.85));
            // Lanterns along the arcade, pillar to pillar.
            let (a, b) = (front.at(t0 + 0.2, -0.22), front.at(t1 - 0.2, -0.22));
            lantern_string(g, c, rng, p3(a.x, fh - 0.12, a.y), p3(b.x, fh - 0.12, b.y), 0.25, 1.1);
            // The name over the arcade.
            sign_board(g, c, rng, &upper, 0.0, fh + 0.42, ((t1 - t0) * 0.7).min(4.0), 0.42);
        } else {
            let (a, b) = (front.at(t0 + 0.3, 0.45), front.at(t1 - 0.3, 0.45));
            lantern_string(g, c, rng, p3(a.x, fh + 0.35, a.y), p3(b.x, fh + 0.35, b.y), 0.3, 1.0);
        }
        hk::flat_windows(g, c, rng, &upper, fh, floors - 1, fh, 1.8, 0.3, 0.06 + 0.15 * c.wear, FlatDress { more_ac: 0.3, ledge_below: true });
        g.lod(Lod::Near, |g| laundry_poles(g, c, rng, &upper, fh, floors - 1, fh, 0.3));
        if floors >= 3 && coin(rng, 0.45) {
            let side = if coin(rng, 0.5) { 1.0 } else { -1.0 };
            vertical_sign(g, c, rng, &upper, side * ((t1 - t0) * 0.5 - 0.75), fh + 0.7, h - fh - 1.3);
        }
        grime(g, c, rng, &upper, fh, h, 1.0 + c.wear);
        // The back of the flats.
        let back = Face::from_edge(front.at(t1, -d), front.at(t0, -d));
        hk::flat_windows(g, c, rng, &back, fh, floors - 1, fh, 2.2, 0.35, 0.05, FlatDress { more_ac: 0.2, ledge_below: false });
        // An end wall: pipes and stains.
        for (end, edge) in [(i == 0, t0), (i == units - 1, t1)] {
            if !end {
                continue;
            }
            let ef = if edge == t0 { Face::from_edge(front.at(t0, -d), front.at(t0, 0.0)) } else { Face::from_edge(front.at(t1, 0.0), front.at(t1, -d)) };
            hk::pipe_run(g, c, rng, &ef, ef.len * 0.25, 0.2, h, fh, 2);
            grime(g, c, rng, &ef, 0.0, h, 1.5);
        }
        let roof = strip(&front, t0, t1, 0.0, d);
        if coin(rng, 0.45) {
            ridge_roof(g, c, &roof, h, (d * 0.16).clamp(1.0, 2.2), front.out, 0.96, 0.25, Brick, tile);
        } else {
            parapet(g, c, &roof, h, 0.9, 0.18, Concrete, mul(col, 0.9));
            let q = front.at(t, -d * 0.6);
            g.lod(Lod::Mid, |g| {
                water_tank(g, f, q, h, 0.7, 1.4, Metal, mul(pal.steel, 0.75));
            });
            if d > 6.0 && coin(rng, 0.5) {
                // A tin shack on the roof.
                let sp = front.at(t, -d * 0.25);
                g.lod(Lod::Mid, |g| f.bx_yaw(g, Metal, sp.x, h + 1.1, sp.y, Vec3::new((t1 - t0) * 0.5, 2.2, 2.0), mul(pick(rng, &pal.metal_paint), 0.65), front.yaw()));
            }
            hk::roof_clutter(g, c, rng, &roof, h, 0.9);
        }
        // A kitchen out the back where the lot runs on.
        if d < deep * 0.95 || !all_in(&site, &[front.at(t0 + 0.4, -d - 3.2), front.at(t1 - 0.4, -d - 3.2)]) || c.fd < d + 6.0 {
            continue;
        }
        if coin(rng, 0.6) {
            kitchen_shed(g, c, rng, &strip(&front, t0 + 0.4, t1 - 0.4, d + 0.3, d + 3.2));
        }
    }
}

/// A ground plate in a footprint at `y`.
fn ground_at(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, color: [f32; 3]) {
    ground(g, &c.frame, Paving, poly, y, color);
}

// ── alley walk-ups ───────────────────────────────────────────────────────────

/// Two walk-up blocks either side of an alley running back from the
/// street: shops on the street, flats above with cages, AC units and
/// pipes, lanterns zigzagging across the alley at every floor, washing on
/// lines across it, a lit name board over its mouth. A lot too narrow for
/// an alley is a row of shophouses.
pub fn alley_walkups(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let site = site(c, 0.6);
    let (lo, hi) = bounds(&site);
    if hi.x - lo.x < 11.0 || hi.y - lo.y < 8.0 {
        return shophouses(g, c, rng);
    }
    let aw = rng.random_range(2.6..3.4);
    let mid = lo.x + (hi.x - lo.x) * rng.random_range(0.42..0.58);
    let v1 = lo.y + (hi.y - lo.y).min(24.0);
    let (au0, au1) = (mid - aw * 0.5, mid + aw * 0.5);
    let blocks = [clip_box(&site, lo.x - 1.0, au0, lo.y - 1.0, v1), clip_box(&site, au1, hi.x + 1.0, lo.y - 1.0, v1)];
    let mut tops = Vec::new();
    for b in blocks.iter() {
        if b.is_empty() || polygon_area(b).abs() < 15.0 {
            tops.push(0.0);
            continue;
        }
        let floors = (c.floors as i32 + rng.random_range(0..=2)).clamp(3, (c.floors as i32 + 2).max(7));
        let h = floors as f32 * fh;
        tops.push(h);
        let (s, col) = wall(c, rng);
        extrude(g, f, s, b, 0.0, h, col);
        let band = hk::tile_tone(c, rng);
        for face in c.faces(b) {
            if face.len < 2.4 {
                continue;
            }
            let alley = face.out.x.abs() > 0.9 && ((face.mid().x - au0).abs() < 0.2 || (face.mid().x - au1).abs() < 0.2);
            if face.is_front && face.len > 3.0 {
                // Shops on the street under a ledge.
                let n = ((face.len / 4.8).floor() as i32).max(1);
                let w = face.len / n as f32;
                for k in 0..n {
                    let t = -face.len * 0.5 + (k as f32 + 0.5) * w;
                    let sub = Face::from_edge(face.at(t - w * 0.5 + 0.05, 0.0), face.at(t + w * 0.5 - 0.05, 0.0));
                    shop(g, c, rng, &sub, fh, false);
                }
                hk::ledge(g, c, &face, fh, 0.8, (0.0, 0.0), mul(col, 0.7));
                hk::flat_windows(g, c, rng, &face, fh, floors - 1, fh, 1.9, 0.35, 0.1 + 0.2 * c.wear, FlatDress { more_ac: 0.3, ledge_below: true });
                if coin(rng, 0.6) {
                    // On the block's outer corner.
                    let end = if face.mid().x < mid { -1.0 } else { 1.0 };
                    hk::corner_neon(g, c, rng, &face, end, fh + 0.8, h - fh - 1.4);
                }
            } else {
                hk::flat_windows(g, c, rng, &face, fh, floors - 1, fh, if alley { 1.7 } else { 2.2 }, 0.45, 0.12 + 0.2 * c.wear, FlatDress { more_ac: 0.25, ledge_below: false });
                if alley {
                    hk::pipe_run(g, c, rng, &face, face.len * 0.3, 0.2, h, fh, 3);
                    // Back doors onto the alley, a bulb over each.
                    let n = ((face.len / 6.0).floor() as i32).max(1);
                    for k in 0..n {
                        let t = -face.len * 0.5 + (k as f32 + 0.5) * face.len / n as f32;
                        face.decal(g, f, Metal, t, 1.1, 0.04, 1.0, 2.2, mul(pick(rng, &pal.metal_paint), 0.5));
                        face.bx(g, f, Glow, t, 2.55, 0.15, Vec3::new(0.18, 0.18, 0.18), mul(pal.lamp, 0.85));
                    }
                } else {
                    hk::tile_bands(g, c, &face, 0.0, floors - 1, fh, band);
                }
            }
            grime(g, c, rng, &face, 0.0, h, 1.0 + c.wear);
        }
        parapet(g, c, b, h, 0.9, 0.18, Concrete, mul(col, 0.88));
        hk::roof_clutter(g, c, rng, b, h, 0.9);
        let q = poly_center(b);
        let tank = mul(pal.steel, 0.7);
        g.lod(Lod::Mid, |g| water_tank(g, f, q, h, 0.8, 1.6, Metal, tank));
    }
    // The alley: its paving, lanterns at every floor, washing, the board
    // over its mouth.
    let lane = clip_box(&site, au0, au1, lo.y - 1.0, v1);
    let low = tops.iter().copied().fold(f32::MAX, f32::min);
    if lane.is_empty() || low < fh * 2.0 {
        return;
    }
    let (llo, lhi) = bounds(&lane);
    ground_at(g, c, &lane, 0.04, mul(pal.paving, 0.6));
    let (va, vb) = (llo.y + 0.6, lhi.y - 0.8);
    let levels = ((low - fh) / fh).floor().clamp(1.0, 4.0) as i32;
    for lv in 0..levels {
        let y = fh * (1.0 + lv as f32) + 0.9;
        let mut v = va + 1.4 + lv as f32 * 0.7;
        let mut flip = lv % 2 == 0;
        while v + 1.4 < vb {
            let (ua, ub) = if flip { (au0 + 0.05, au1 - 0.05) } else { (au1 - 0.05, au0 + 0.05) };
            if all_in(&site, &[Vec2::new(ua, v), Vec2::new(ub, v + 1.4)]) {
                lantern_string(g, c, rng, p3(ua, y, v), p3(ub, y - 0.25, v + 1.4), 0.3, 0.9);
            }
            flip = !flip;
            v += 2.8;
        }
    }
    // Washing on lines across, between the lantern rows.
    g.lod(Lod::Near, |g| {
        for k in 0..levels {
            let y = fh * (1.0 + k as f32) + 2.2;
            let v = va + 2.2 + (k as f32 * 3.1) % (vb - va - 2.0).max(1.0);
            if v > vb {
                continue;
            }
            let (a, b) = (f.at(au0 + 0.05, y, v), f.at(au1 - 0.05, y, v));
            g.part(Matte).wire(a, b, 0.15, 0.025, rgba(CHARCOAL));
            for i in 0..3 {
                let u = au0 + aw * (0.25 + 0.25 * i as f32);
                f.bx(g, Tarp, u, y - 0.45, v, Vec3::new(0.45, 0.6, 0.05), mul(pick(rng, &pal.tarp_colors), 1.15));
            }
        }
    });
    // The name board over the alley's mouth, wall to wall.
    let mouth = llo.y + 0.25;
    if all_in(&site, &[Vec2::new(au0 + 0.1, mouth), Vec2::new(au1 - 0.1, mouth)]) {
        let y = fh + 1.1;
        f.bx(g, Matte, mid, y, mouth, Vec3::new(aw - 0.04, 0.7, 0.14), DARK);
        let ink = mul(pick(rng, &pal.sign_colors), 1.0 - 0.4 * c.wear);
        let board = Face::new(Vec2::new(au0, mouth - 0.07), Vec2::new(au1, mouth - 0.07), Vec2::new(0.0, -1.0));
        let n = ((aw / 0.55).floor() as i32).clamp(2, 6);
        let off = (Vec2::new(mid, mouth) - board.mid()).dot(board.tangent);
        for i in 0..n {
            let t = off - aw * 0.4 + (i as f32 + 0.5) * aw * 0.8 / n as f32;
            board.decal(g, f, Glow, t, y, 0.012, aw * 0.8 / n as f32 * 0.7, 0.46, ink);
        }
        for side in [-1.0f32, 1.0] {
            lantern(g, c, p3(mid + side * (aw * 0.5 - 0.35), y - 0.4, mouth + 0.3), 0.45, paper(rng), !coin(rng, 0.3 * c.wear));
        }
    }
    // A kitchen at the far end of a deep lot.
    if hi.y - v1 > 4.5 {
        let back = clip_box(&site, lo.x + 1.0, hi.x - 1.0, v1 + 0.8, (v1 + 4.5).min(hi.y - 0.3));
        kitchen_shed(g, c, rng, &back);
    }
}

// ── stall rows ───────────────────────────────────────────────────────────────

/// One bay of a double-sided stall row at lot-local (`u`, `v`): a ridged
/// tarp over two counters back to back, posts, goods, a lit strip under
/// each eave and the stall's name on it.
fn stall_bay(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, u: f32, v: f32, tarp: [f32; 3]) {
    let f = &c.frame;
    let pal = c.palette;
    let wood = mul(pal.wood, 0.9 - 0.2 * c.wear);
    for side in [-1.0f32, 1.0] {
        let a = f.at(u, 3.05, v);
        let b = f.at(u + side * 1.95, 2.45, v);
        g.part(Tarp).slab(a, b, 2.96, 0.05, rgba(tarp));
        // The lit strip under the eave, facing down onto the counter.
        let lc = mul(SHOP_LIGHT[rng.random_range(0..SHOP_LIGHT.len())], 0.8);
        g.part(Glow).decal(f.at(u + side * 1.6, 2.36, v), f.dir3(Vec2::Y), f.dir3(Vec2::X), 2.3, 0.1, -Vec3::Y, rgba(lc));
        // The stall's name under the eave, facing the aisle.
        let face = Face::new(Vec2::new(u + side * 1.97, v - 1.0), Vec2::new(u + side * 1.97, v + 1.0), Vec2::new(side, 0.0));
        face.decal(g, f, Glow, 0.0, 2.2, 0.01, 1.6, 0.26, mul(pick(rng, &pal.sign_colors), 0.9 - 0.4 * c.wear));
        let goods = pick(rng, &pal.awning_colors);
        // Counters and goods read from a few blocks off, not from afar.
        g.lod(Lod::Mid, |g| f.bx(g, Wood, u + side * 1.0, 0.45, v, Vec3::new(0.75, 0.9, 2.5), wood));
        g.lod(Lod::Near, |g| f.bx(g, Tarp, u + side * 1.05, 1.05, v, Vec3::new(0.5, 0.3, 2.1), mul(goods, 1.1)));
    }
    // Two posts a bay (the next bay's stand at its other corners).
    g.lod(Lod::Mid, |g| {
        for (du, dv) in [(-1.8f32, -1.35f32), (1.8, 1.35)] {
            f.bx(g, Matte, u + du, 1.25, v + dv, Vec3::new(0.08, 2.5, 0.08), CHARCOAL);
        }
    });
    if coin(rng, 0.3) {
        // Steam off a pot.
        g.lod(Lod::Near, |g| {
            f.bx(g, Metal, u, 1.1, v + 0.7, Vec3::new(0.5, 0.4, 0.5), mul(pal.steel, 0.8));
            f.bx(g, Tarp, u, 1.75, v + 0.7, Vec3::new(0.35, 0.8, 0.35), [0.78, 0.78, 0.8]);
        });
    }
}

/// Rows of stalls down a stretch of a lot (`v0..v1`), aisles between them
/// strung with lanterns, tables and stools in the aisles.
fn stall_field(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, site: &[Vec2], v0: f32, v1: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let (lo, hi) = bounds(site);
    let pitch = 7.0;
    let rows = (((hi.x - lo.x - 3.6) / pitch).floor() as i32 + 1).max(1);
    let u0 = (lo.x + hi.x) * 0.5 - (rows - 1) as f32 * pitch * 0.5;
    let fits = |u: f32, v: f32| all_in(site, &[Vec2::new(u - 2.05, v - 1.5), Vec2::new(u + 2.05, v - 1.5), Vec2::new(u + 2.05, v + 1.5), Vec2::new(u - 2.05, v + 1.5)]);
    for r in 0..rows {
        let u = u0 + r as f32 * pitch;
        let tarps: Vec<[f32; 3]> = (0..2).map(|_| mul(pick(rng, &pal.tarp_colors), 0.95 - 0.25 * c.wear)).collect();
        let mut v = v0 + 1.5;
        let mut k = 0;
        // A big lot's field stops at a few hundred stalls' worth.
        while v + 1.5 <= v1 && k < 14 {
            if fits(u, v) {
                // Patchwork: the tarp changes every few bays.
                let tarp = if coin(rng, 0.25) { mul(pick(rng, &pal.awning_colors), 0.9) } else { tarps[(k / 3) % 2] };
                stall_bay(g, c, rng, u, v, tarp);
                // Lanterns across the aisle to the next row.
                let un = u + pitch;
                if r + 1 < rows && fits(un, v) {
                    let (ua, ub) = if k % 2 == 0 { (u + 1.98, un - 1.98) } else { (un - 1.98, u + 1.98) };
                    lantern_string(g, c, rng, p3(ua, 2.5, v - 1.2), p3(ub, 2.5, v + 1.2), 0.25, 1.0);
                    if coin(rng, 0.35) {
                        let p = Vec2::new(u + pitch * 0.5, v);
                        g.lod(Lod::Near, |g| table(g, f, p, 0.0, mul([0.75, 0.2, 0.15], 1.0), None));
                    }
                }
            }
            v += 3.0;
            k += 1;
        }
    }
}

/// Stall rows under patchwork tarps filling the lot, a store and kitchen
/// shed along the back.
pub fn stall_rows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let pal = c.palette;
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.7));
    let site = site(c, 0.8);
    let (lo, hi) = bounds(&site);
    let back = if hi.y - lo.y > 16.0 { 4.0 } else { 0.0 };
    if back > 0.0 {
        let shed = clip_box(&site, lo.x - 1.0, hi.x + 1.0, hi.y - back, hi.y + 1.0);
        kitchen_shed(g, c, rng, &shed);
    }
    stall_field(g, c, rng, &site, lo.y + 0.4, hi.y - back - 0.6);
}

// ── food court ───────────────────────────────────────────────────────────────

/// A food court hall: an open hall on concrete posts under a tin gable,
/// kitchens along the back with a lit menu over each counter, tables in
/// rows, lanterns strung under the roof, the name over the front. A big
/// one on dear land has the cooked-food hall upstairs over the market,
/// band windows lit late.
pub fn food_court(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let Some(r) = fit(c, R::of(&offset_poly(&c.lot_poly, 1.2)), 8.0) else { return stall_rows(g, c, rng) };
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.75));
    let hall_h = 4.8;
    let col = mul(pick(rng, &pal.concrete), 0.85 - 0.2 * c.wear);
    // Kitchens along the back, their counters facing the hall.
    let kd = (r.d() * 0.22).clamp(2.4, 4.0);
    let kitchens = R::new(r.u0, r.u1, r.v1 - kd, r.v1);
    pilotis(g, c, &R::new(r.u0, r.u1, r.v0, kitchens.v0).poly(), 0.0, hall_h, 4.5, 0.4, Concrete, col);
    let (ks, kcol) = wall(c, rng);
    block(g, c, ks, &kitchens, 0.0, hall_h, kcol);
    let kf = kitchens.front();
    let n = ((kf.len / 3.4).floor() as i32).max(1);
    let step = kf.len / n as f32;
    for i in 0..n {
        let t = -kf.len * 0.5 + (i as f32 + 0.5) * step;
        kf.bx(g, f, Wood, t, 0.5, 0.45, Vec3::new(step * 0.8, 1.0, 0.8), mul(pal.wood, 0.85));
        kf.glaze(g, c, t, 1.9, 0.04, step * 0.6, 1.4, Pane::Lit(mul(SHOP_LIGHT[i as usize % 3], 0.6)));
        kf.decal(g, f, Glow, t, 3.3, 0.04, step * 0.7, 0.5, mul(pick(rng, &pal.sign_colors), 0.95 - 0.4 * c.wear));
        g.lod(Lod::Mid, |g| hk::wall_box(g, c, Metal, &kf, t, 2.75, 0.0, Vec3::new(step * 0.6, 0.35, 0.7), mul(pal.steel, 0.85)));
        lantern(g, c, p3(kf.at(t, 1.3).x, hall_h - 0.4, kf.at(t, 1.3).y), 0.45, paper(rng), !coin(rng, 0.3 * c.wear));
    }
    // Tables in rows.
    let tables = R::new(r.u0 + 1.4, r.u1 - 1.4, r.v0 + 1.4, kitchens.v0 - 1.8);
    if tables.w() > 1.0 && tables.d() > 1.0 {
        let (nu, nv) = (((tables.w() / 2.8).floor() as i32 + 1).min(10), ((tables.d() / 2.6).floor() as i32 + 1).min(6));
        let top = mul(pick(rng, &[[0.75, 0.2, 0.15], [0.2, 0.42, 0.6], [0.82, 0.78, 0.7]]), 1.0);
        g.lod(Lod::Near, |g| {
            for a in 0..nu {
                for b in 0..nv {
                    let p = Vec2::new(tables.u0 + tables.w() * (a as f32 + 0.5) / nu as f32, tables.v0 + tables.d() * (b as f32 + 0.5) / nv as f32);
                    table(g, f, p, 0.0, top, None);
                }
            }
        });
    }
    // Lanterns strung across under the roof.
    let lines = ((kitchens.v0 - r.v0) / 4.0).floor().clamp(1.0, 6.0) as i32;
    for k in 0..lines {
        let v = r.v0 + (k as f32 + 0.6) * (kitchens.v0 - r.v0) / lines as f32;
        lantern_string(g, c, rng, p3(r.u0 + 0.6, hall_h - 0.35, v), p3(r.u1 - 0.6, hall_h - 0.35, v), 0.5, 2.0);
    }
    let front = r.front();
    let upper = c.floors >= 3 && c.tier != nd_sim::Tier::Low && r.w() > 12.0;
    if upper {
        // The cooked-food hall upstairs.
        let floors = (c.floors as i32 - 1).clamp(1, 3);
        let fh = c.fh();
        let top = hall_h + floors as f32 * fh;
        let (s, ucol) = wall(c, rng);
        block(g, c, s, &r, hall_h, top, ucol);
        for face in r.faces() {
            band_windows(g, c, rng, &face, hall_h, floors, fh);
            grime(g, c, rng, &face, hall_h, top, 1.0 + c.wear);
        }
        parapet(g, c, &r.poly(), top, 0.9, 0.2, Concrete, mul(ucol, 0.9));
        sign_board(g, c, rng, &front, 0.0, top - 0.6, (r.w() * 0.5).min(12.0), 0.8);
        let end = if coin(rng, 0.5) { 1.0 } else { -1.0 };
        hk::corner_neon(g, c, rng, &front, end, hall_h + 0.5, top - hall_h - 1.0);
        let m = ((r.w() / 8.0) as i32).clamp(1, 4);
        for k in 0..m {
            let p = Vec2::new(r.u0 + (k as f32 + 0.5) * r.w() / m as f32, r.v1 - 2.0);
            g.lod(Lod::Mid, |g| chiller(g, c, p, top, 2.6, 1.6, 0.0, mul(pal.steel, 0.8)));
        }
    } else {
        gable_on(g, c, &r, hall_h, (r.d() * 0.12).clamp(1.0, 2.6), true, mul(pick(rng, &pal.metal_paint), 0.7 - 0.2 * c.wear));
        sign_board(g, c, rng, &front, 0.0, hall_h - 0.75, (r.w() * 0.45).min(10.0), 0.7);
        // Lanterns along the front eave, either side of the name, where
        // the street sees them.
        let half = (r.w() * 0.45).min(10.0) * 0.5 + 0.6;
        for (ua, ub) in [(r.u0 + 0.3, r.c().x - half), (r.c().x + half, r.u1 - 0.3)] {
            if ub - ua > 2.0 {
                lantern_string(g, c, rng, p3(ua, hall_h - 0.15, r.v0 - 0.12), p3(ub, hall_h - 0.15, r.v0 - 0.12), 0.3, 1.3);
            }
        }
        // Extract flues through the roof over the kitchens.
        for i in 0..n.min(5) {
            let t = -kf.len * 0.5 + (i as f32 + 0.5) * kf.len / n.min(5) as f32;
            let p = kf.at(t, -kd * 0.5);
            f.bx(g, Metal, p.x, hall_h + 2.0, p.y, Vec3::new(0.45, 3.0, 0.45), mul(pal.steel, 0.7));
        }
    }
}

// ── tea house ────────────────────────────────────────────────────────────────

/// A tea house of two or three storeys stepping in, each under a skirt of
/// tiles with upturned corners, a hipped roof on top; lacquered columns
/// along a verandah, lattice windows lit warm, lanterns under every eave,
/// a garden at the street with potted trees and a gate.
pub fn tea_house(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let want = R::of(&offset_poly(&c.lot_poly, 2.0));
    let (w, d) = (want.w().min(18.0), want.d().min(14.0));
    let v0 = want.v0 + (want.d() - d).min(3.5);
    // Fitted with its eaves (1.2 m and the upturned corners) on the lot.
    let Some(r) = fit(c, R::new(want.c().x - w * 0.5, want.c().x + w * 0.5, v0, v0 + d).inset(-1.7), 9.4).map(|r| r.inset(1.7)) else { return shophouses(g, c, rng) };
    yard(g, c, 0.3, Paving, mul(mix(pal.paving, [0.55, 0.48, 0.38], 0.4), 0.8));
    let storeys = (c.floors as i32).clamp(2, 3);
    let sh = 3.4;
    let walls = mul(mix(pick(rng, &pal.plaster), [0.86, 0.80, 0.68], 0.5), 1.0 - 0.25 * c.wear);
    let lacquer = mul(LACQUER[rng.random_range(0..LACQUER.len())], 1.0 - 0.3 * c.wear);
    let tile = tiles(c, rng);
    for k in 0..storeys {
        let rk = r.inset(k as f32 * 0.6);
        let (y0, y1) = (k as f32 * sh, (k + 1) as f32 * sh);
        block(g, c, Plaster, &rk, y0, y1, walls);
        for (fi, face) in rk.faces().iter().enumerate() {
            let n = ((face.len - 1.0) / 2.2).floor() as i32;
            if n < 1 {
                continue;
            }
            let step = (face.len - 1.0) / n as f32;
            for i in 0..n {
                let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * step;
                // Clear of the door in the middle of the street front.
                if k == 0 && fi == 0 && t.abs() < 1.7 {
                    continue;
                }
                pane(g, c, rng, face, t, y0 + sh * 0.5, (step * 0.62).min(1.5), 1.5, true);
                g.lod(Lod::Near, |g| {
                    for dx in [-0.25f32, 0.25] {
                        face.decal(g, f, Matte, t + dx * (step * 0.62).min(1.5), y0 + sh * 0.5, 0.068, 0.04, 1.5, lacquer);
                    }
                    face.decal(g, f, Matte, t, y0 + sh * 0.5, 0.068, (step * 0.62).min(1.5), 0.04, lacquer);
                });
            }
        }
        if k == 0 {
            let door = rk.front();
            door.decal(g, f, Wood, 0.0, 1.2, 0.04, 1.8, 2.4, lacquer);
            door.glaze(g, c, 0.0, 1.5, 0.06, 1.3, 1.6, Pane::Lit(mul(SHOP_LIGHT[1], 0.7)));
        }
        if k + 1 < storeys {
            // The skirt of tiles round the storey, and its corners.
            let out = rk.inset(-1.1);
            let next = r.inset((k + 1) as f32 * 0.6 - 0.04);
            loft(g, f, Brick, &out.poly(), &next.poly(), y1 - 0.45, y1 + 0.3, tile);
            g.lod(Lod::Mid, |g| {
                let q = out.c();
                for corner in out.poly() {
                    let dir = (corner - q).normalize_or_zero();
                    let (a, b) = (corner - dir * 0.5, corner + dir * 0.35);
                    g.part(Brick).slab(f.at(a.x, y1 - 0.33, a.y), f.at(b.x, y1 + 0.05, b.y), 0.3, 0.1, rgba(mul(tile, 0.85)));
                }
            });
        } else {
            hip_roof(g, c, &rk, y1 - 0.45, (rk.w().min(rk.d()) * 0.3).clamp(1.4, 3.0), 1.2, tile);
        }
        // Lanterns under the eave along the front and back.
        let eave = rk.inset(-0.8);
        for face in [eave.front(), eave.back()] {
            let n = ((face.len / 2.2).floor() as i32).max(1);
            for i in 0..n {
                let t = -face.len * 0.5 + (i as f32 + 0.5) * face.len / n as f32;
                let p = face.at(t, 0.0);
                lantern(g, c, p3(p.x, y1 - 0.6, p.y), 0.42, paper(rng), !coin(rng, 0.3 * c.wear));
            }
        }
    }
    // The verandah's columns along the street front.
    let front = r.front();
    let n = ((front.len / 2.4).floor() as i32).max(1);
    for i in 0..=n {
        let t = -front.len * 0.5 + 0.3 + i as f32 * (front.len - 0.6) / n as f32;
        front.bx(g, f, Wood, t, (sh - 0.45) * 0.5, 0.75, Vec3::new(0.28, sh - 0.45, 0.28), lacquer);
    }
    // The name, on a board over the door.
    sign_board(g, c, rng, &front, 0.0, sh - 0.85, 2.6, 0.45);
    // The garden: a path to the door, potted trees, a gate at the street.
    let path = R::new(r.c().x - 0.9, r.c().x + 0.9, want.v0 - 1.0, r.v0);
    if path.d() > 0.6 {
        ground_at(g, c, &path.poly(), 0.06, mul(pick(rng, &pal.concrete), 0.95));
        for side in [-1.0f32, 1.0] {
            let p = Vec2::new(r.c().x + side * 2.4, (path.v0 + path.v1) * 0.5);
            if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 1.2 {
                g.lod(Lod::Mid, |g| {
                    f.bx(g, Concrete, p.x, 0.3, p.y, Vec3::new(0.8, 0.6, 0.8), mul(pick(rng, &pal.concrete), 0.7));
                    tree(g, f, p, 2.6, 1.1, pal.bark, mul(pal.foliage, 0.9));
                });
            }
        }
        if path.d() > 2.5 && gate_width(c, r.c().x, path.v0 + 0.6, 2.4) == Some(2.4) {
            gateway(g, c, rng, r.c().x, path.v0 + 0.6, 2.4, 3.2, lacquer, tile);
        }
    }
    // The kitchen out the back.
    let back = clip_box(&offset_poly(&c.lot_poly, 1.0), r.u0 + 1.0, r.u1 - 1.0, r.v1 + 1.6, r.v1 + 5.0);
    if !back.is_empty() && poly_inside(c, &back) && bounds(&back).1.y - bounds(&back).0.y > 2.5 {
        kitchen_shed(g, c, rng, &back);
    }
}

// ── night-market hall ────────────────────────────────────────────────────────

/// The night market's own building: a painted gateway at the street, a
/// forecourt of stall rows under a canopy of lantern strings, and a market
/// hall at the back: open stalls on the ground floor, restaurants upstairs
/// behind lit bands, signs and neon on its front, a tile roof.
pub fn lantern_market(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let fh = c.fh();
    let site = site(c, 0.8);
    let (lo, hi) = bounds(&site);
    if hi.y - lo.y < 18.0 || hi.x - lo.x < 10.0 {
        return stall_rows(g, c, rng);
    }
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.65));
    let lacquer = mul(LACQUER[0], 1.0 - 0.3 * c.wear);
    let tile = tiles(c, rng);
    let hd = ((hi.y - lo.y) * 0.42).clamp(7.0, 16.0);
    // Fitted with its eaves (0.5 m and the corners) on the lot.
    let hall = fit(c, R::new(lo.x + 0.4, hi.x - 0.4, hi.y - hd, hi.y - 0.3).inset(-0.9), 7.8).map(|r| r.inset(0.9));
    let fore_end = hall.map_or(hi.y - 1.0, |h| h.v0 - 1.0);
    stall_field(g, c, rng, &site, lo.y + 3.4, fore_end);
    let cu = (lo.x + hi.x) * 0.5;
    let gh = 5.6;
    let gw = gate_width(c, cu, lo.y + 0.9, (hi.x - lo.x - 3.0).min(12.0));
    if let Some(gw) = gw {
        gateway(g, c, rng, cu, lo.y + 0.9, gw, gh, lacquer, tile);
    }
    let gw = gw.unwrap_or(0.0);
    let Some(h) = hall else { return };
    let ground_h = 4.2;
    // Open stalls on the ground floor, on posts.
    let col = mul(pick(rng, &pal.concrete), 0.8);
    pilotis(g, c, &h.poly(), 0.0, ground_h, 4.0, 0.42, Concrete, col);
    let inner = R::new(h.u0 + 1.2, h.u1 - 1.2, h.v0 + 1.4, h.v1 - 1.2);
    if inner.w() > 3.0 && inner.d() > 2.0 {
        let n = ((inner.w() / 3.4).floor() as i32).max(1);
        for i in 0..n {
            let u = inner.u0 + (i as f32 + 0.5) * inner.w() / n as f32;
            f.bx(g, Wood, u, 0.45, inner.c().y, Vec3::new(2.4, 0.9, (inner.d() * 0.5).min(2.4)), mul(pal.wood, 0.85));
            f.bx(g, Glow, u, ground_h - 0.25, inner.c().y, Vec3::new(1.8, 0.06, 0.1), mul(SHOP_LIGHT[i as usize % 3], 0.8));
        }
    }
    // Restaurants upstairs.
    let floors = (c.floors as i32 - 1).clamp(1, 3);
    let top = ground_h + floors as f32 * fh;
    let (s, ucol) = wall(c, rng);
    block(g, c, s, &h, ground_h, top, ucol);
    let front = h.front();
    for face in h.faces() {
        band_windows(g, c, rng, &face, ground_h, floors, fh);
        grime(g, c, rng, &face, ground_h, top, 1.0 + c.wear);
    }
    sign_board(g, c, rng, &front, 0.0, ground_h + 0.5, (h.w() * 0.55).min(10.0), 0.55);
    hk::corner_neon(g, c, rng, &front, 1.0, ground_h + 0.3, top - ground_h - 0.6);
    hk::corner_neon(g, c, rng, &front, -1.0, ground_h + 0.3, top - ground_h - 0.6);
    hip_roof(g, c, &h, top, (h.w().min(h.d()) * 0.22).clamp(1.2, 3.0), 0.5, tile);
    // A canopy of lantern strings from the gate's beam to the hall.
    if gw <= 0.0 {
        return;
    }
    let strings = ((gw / 3.0).floor() as i32).clamp(2, 5);
    for k in 0..strings {
        let ua = cu - gw * 0.5 + (k as f32 + 0.5) * gw / strings as f32;
        let ub = h.u0 + (k as f32 + 0.5) * h.w() / strings as f32;
        let a = p3(ua, gh - 0.55, lo.y + 1.3);
        let b = p3(ub, ground_h + 0.3, h.v0 - 0.25);
        lantern_string(g, c, rng, a, b, 0.9, 1.5);
    }
}

// ── workshops ────────────────────────────────────────────────────────────────

/// A row of small workshops behind a work apron: one or two storeys each,
/// roller doors up on a lit bench or down and tagged, a tin lean-to over
/// the door, the trade's board, a bulb and a lantern, stock and crates on
/// the apron, a flat over some with washing at the window, scrap out the
/// back.
pub fn alley_workshops(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    yard(g, c, 0.3, Concrete, mul(pal.concrete(0), 0.65));
    let site = site(c, 1.0);
    let front = c.front_face(&site);
    let units = ((front.len / 6.0).floor() as i32).clamp(1, 6);
    let uw = front.len / units as f32;
    let set = (c.fd * 0.15).clamp(1.5, 4.0);
    let deep = (c.fd * 0.5).clamp(5.0, 12.0);
    let mut back_v = Vec::new();
    // One tin for the row's roofs: their eaves overlap.
    let tin = mul(pick(rng, &pal.metal_paint), 0.65 - 0.2 * c.wear);
    for i in 0..units {
        let t = -front.len * 0.5 + (i as f32 + 0.5) * uw;
        let (t0, t1) = (t - uw * 0.5 + 0.1, t + uw * 0.5 - 0.1);
        let mut d1 = set + deep;
        while d1 > set + 3.0 && !all_in(&site, &[front.at(t0, -d1), front.at(t1, -d1), front.at(t0, -set), front.at(t1, -set)]) {
            d1 *= 0.9;
        }
        if d1 <= set + 3.0 || !all_in(&site, &[front.at(t0, -set), front.at(t1, -set)]) {
            continue;
        }
        back_v.push((t, d1));
        let two = c.floors >= 2 && coin(rng, 0.6);
        let gh = 3.8;
        let h = if two { gh + 3.0 } else { gh };
        let (s, col) = match rng.random_range(0..3) {
            0 => (Brick, mul(pick(rng, &pal.brick), 1.0 - 0.25 * c.wear)),
            _ => wall(c, rng),
        };
        let body = strip(&front, t0, t1, set, d1);
        extrude(g, f, s, &body, 0.0, h, col);
        let df = Face::from_edge(front.at(t0, -set), front.at(t1, -set));
        let dw = ((t1 - t0) * 0.62).min(4.0);
        let dh = 2.9;
        let paint = mul(pick(rng, &pal.metal_paint), 0.7 - 0.25 * c.wear);
        if coin(rng, 0.65) {
            // Up, on a lit workshop: a bench out front, a welder's spark.
            df.glaze(g, c, 0.0, dh * 0.5, 0.04, dw, dh, Pane::Lit(mul([1.25, 1.2, 1.1], rng.random_range(0.5..0.75))));
            df.decal(g, f, Metal, 0.0, dh - 0.22, 0.075, dw, 0.44, paint);
            df.bx(g, f, Wood, 0.0, 0.45, 0.6, Vec3::new(dw * 0.6, 0.9, 0.7), mul(pal.wood, 0.8));
            if coin(rng, 0.35) {
                let from = g.part(Glow).positions.len();
                df.bx(g, f, Glow, 0.4, 1.0, 0.65, Vec3::new(0.12, 0.12, 0.12), [2.4, 2.6, 3.0]);
                mark_glow(g, from, NIGHT_MARK);
            }
        } else {
            df.decal(g, f, Metal, 0.0, dh * 0.5, 0.075, dw, dh, paint);
            graffiti(g, c, rng, &df, 0.0, dh, 0.6 + c.wear);
        }
        df.bx(g, f, Matte, 0.0, dh + 0.15, 0.1, Vec3::new(dw + 0.3, 0.3, 0.2), CHARCOAL);
        df.bx(g, f, Glow, dw * 0.5 + 0.35, dh + 0.25, 0.18, Vec3::new(0.18, 0.18, 0.18), mul(pal.lamp, 0.85));
        // The tin lean-to over the door.
        let reach = (set - 0.4).min(2.2);
        if reach > 0.8 {
            let a = df.at(0.0, 0.0);
            let b = df.at(0.0, reach);
            let tin = if c.wear > 0.35 { (Rust, pal.rust) } else { (Metal, mul(pick(rng, &pal.metal_paint), 0.7)) };
            g.part(tin.0).slab(f.at(a.x, dh + 0.75, a.y), f.at(b.x, dh + 0.45, b.y), (t1 - t0) * 0.9, 0.05, rgba(tin.1));
        }
        sign_board(g, c, rng, &df, 0.0, dh + 0.6, ((t1 - t0) * 0.6).min(3.5), 0.32);
        bracket_lantern(g, c, rng, &df, -(dw * 0.5 + 0.4).min(df.len * 0.5 - 0.25), dh - 0.2, 0.4);
        if two {
            pane_row(g, c, rng, &df, gh, 1, 3.0, 2.0, 1.1, 1.3, true);
            g.lod(Lod::Near, |g| laundry_poles(g, c, rng, &df, gh - 3.0, 2, 3.0, 0.5));
        }
        grime(g, c, rng, &df, 0.0, h, 1.2 + c.wear);
        let roof = strip(&front, t0, t1, set, d1);
        if coin(rng, 0.5) {
            parapet(g, c, &roof, h, 0.6, 0.18, Concrete, mul(col, 0.9));
            let q = front.at(t + uw * 0.15, -(set + d1) * 0.5);
            g.lod(Lod::Mid, |g| water_tank(g, f, q, h, 0.6, 1.3, Metal, mul(pal.steel, 0.75)));
        } else {
            gable_roof(g, c, &roof, h, 0.9, tin);
        }
        // Stock on the apron.
        let stock: Vec<(f32, f32, u8)> = (0..rng.random_range(1..4)).map(|_| (rng.random_range(-0.45..0.45), rng.random_range(0.3..0.8), rng.random_range(0..3))).collect();
        g.lod(Lod::Near, |g| {
            for (dt, dd, kind) in &stock {
                let tt = dt * (t1 - t0);
                if (tt.abs() < dw * 0.5 + 0.3) && *kind != 2 {
                    continue;
                }
                let p = df.at(tt, dd * (set - 0.6).max(0.4));
                match kind {
                    0 => f.bx_yaw(g, Wood, p.x, 0.35, p.y, Vec3::new(1.2, 0.7, 0.9), mul(pal.wood, 0.75), df.yaw()),
                    1 => f.bx_yaw(g, Metal, p.x, 0.15, p.y, Vec3::new(2.4, 0.3, 0.6), mul(pal.steel, 0.7), df.yaw()),
                    _ => g.part(Matte).cylinder(f.to_world(p.x, p.y), 0.15, 0.15, 0.0, 1.0, 8, rgba([0.25, 0.42, 0.62])),
                }
            }
        });
    }
    // Lanterns along the apron, door to door.
    if units > 1 && set > 1.4 {
        // Out past the lean-tos, on two poles.
        let (a, b) = (front.at(-front.len * 0.5 + 0.8, 0.3), front.at(front.len * 0.5 - 0.8, 0.3));
        if c.spans_lot(a, b) {
            for q in [a, b] {
                f.bx(g, Matte, q.x, 2.1, q.y, Vec3::new(0.1, 4.2, 0.1), CHARCOAL);
            }
            lantern_string(g, c, rng, p3(a.x, 4.1, a.y), p3(b.x, 4.1, b.y), 0.35, 1.4);
        }
    }
    // Offcuts out the back under a tarp, a stack of pallets.
    for (t, d1) in back_v {
        let p = front.at(t, -d1 - 2.2);
        if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 2.0 && coin(rng, 0.5) {
            let tarp = mul(pick(rng, &pal.tarp_colors), 0.85);
            let q = front.at(t + 1.6, -d1 - 2.0);
            g.lod(Lod::Mid, |g| {
                heap(g, c, p, 1.3, 1.1, tarp);
                if c.inside_local(q) && distance_to_boundary(q, &c.lot_poly) > 0.8 {
                    f.bx_yaw(g, Wood, q.x, 0.35, q.y, Vec3::new(1.2, 0.7, 1.0), mul(pal.wood, 0.7), front.yaw());
                }
            });
        }
    }
}

// ── back-alley kitchens ──────────────────────────────────────────────────────

/// A row of kitchens behind a service alley: open back doors lit white
/// and orange, steel hoods with ducts up the wall and over the roof,
/// steam, gas bottles in a cage, crates and bins, a bulb over each door;
/// a dormitory floor over some with washing; lanterns strung across the
/// alley from the wall to poles at the street.
pub fn back_kitchens(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    let site = site(c, 1.0);
    let front = c.front_face(&site);
    let alley = (c.fd * 0.22).clamp(3.0, 6.0);
    let deep = (c.fd * 0.5).clamp(6.0, 14.0);
    // As much of the frontage and depth as stands on the site.
    let (mut half, mut d1) = (front.len * 0.5 - 0.3, alley + deep);
    while (half > 2.5 || d1 > alley + 3.0) && !all_in(&site, &[front.at(-half, -d1), front.at(half, -d1), front.at(-half, -alley), front.at(half, -alley)]) {
        half = (half * 0.94).max(2.5);
        d1 = (d1 * 0.95).max(alley + 3.0);
        if half <= 2.5 && d1 <= alley + 3.0 {
            break;
        }
    }
    let (t0, t1) = (-half, half);
    if !all_in(&site, &[front.at(t0, -d1), front.at(t1, -d1), front.at(t0, -alley), front.at(t1, -alley)]) {
        return alley_workshops(g, c, rng);
    }
    if t1 - t0 < 5.0 {
        return alley_workshops(g, c, rng);
    }
    yard(g, c, 0.3, Concrete, mul(pal.concrete(0), 0.55));
    let two = c.floors >= 2;
    let gh = 3.4;
    let h = if two { gh + 3.0 } else { gh };
    let tilecol = mul(mix([0.82, 0.82, 0.78], pick(rng, &c.style.wall_colors), 0.35), 1.0 - 0.35 * c.wear);
    let body = strip(&front, t0, t1, alley, d1);
    extrude(g, f, Panel, &body, 0.0, h, tilecol);
    let kf = Face::from_edge(front.at(t0, -alley), front.at(t1, -alley));
    hk::tile_bands(g, c, &kf, 0.0, 1, gh, mul(tilecol, 0.85));
    let n = ((kf.len / 4.5).floor() as i32).max(1);
    let step = kf.len / n as f32;
    let steel = mul(pal.steel, 0.9);
    for i in 0..n {
        let t = -kf.len * 0.5 + (i as f32 + 0.5) * step;
        let dt = t - step * 0.2;
        if coin(rng, 0.7) {
            let col = if coin(rng, 0.5) { [1.3, 1.3, 1.25] } else { SHOP_LIGHT[2] };
            kf.glaze(g, c, dt, 1.1, 0.04, 1.1, 2.2, Pane::Lit(mul(col, rng.random_range(0.55..0.8))));
        } else {
            kf.decal(g, f, Metal, dt, 1.1, 0.04, 1.1, 2.2, mul(pick(rng, &pal.metal_paint), 0.55));
        }
        kf.bx(g, f, Glow, dt, 2.55, 0.15, Vec3::new(0.18, 0.18, 0.18), mul(pal.lamp, 0.85));
        // The hood and its duct up the wall, over the parapet.
        let ht = t + step * 0.22;
        hk::wall_box(g, c, Metal, &kf, ht, 2.2, 0.0, Vec3::new(1.2, 0.5, 0.6), steel);
        hk::wall_box(g, c, Metal, &kf, ht, (2.45 + h + 0.6) * 0.5, 0.0, Vec3::new(0.45, h + 0.6 - 2.45, 0.42), mul(steel, 0.9));
        let p = kf.at(ht, 0.21);
        let q = kf.at(ht, -0.6);
        g.part(Metal).slab(f.at(p.x, h + 0.82, p.y), f.at(q.x, h + 0.82, q.y), 0.45, 0.42, rgba(mul(steel, 0.85)));
        g.lod(Lod::Near, |g| {
            // Steam off the duct.
            for (k, s) in [0.5f32, 0.4, 0.3].iter().enumerate() {
                f.bx(g, Tarp, q.x, h + 1.1 + k as f32 * 0.45, q.y, Vec3::splat(*s), [0.8, 0.8, 0.82]);
            }
            // Gas bottles and crates by the door.
            for b in 0..2 {
                let gp = kf.at(dt + 0.9 + b as f32 * 0.38, 0.4);
                g.part(Matte).cylinder(f.to_world(gp.x, gp.y), 0.16, 0.16, 0.0, 0.95, 8, rgba([0.62, 0.2, 0.12]));
            }
            let cp = kf.at(dt - 1.0, 0.5);
            f.bx_yaw(g, Tarp, cp.x, 0.25, cp.y, Vec3::new(0.6, 0.5, 0.45), [0.2, 0.42, 0.62], kf.yaw());
            f.bx_yaw(g, Tarp, cp.x, 0.73, cp.y, Vec3::new(0.6, 0.46, 0.45), [0.75, 0.2, 0.15], kf.yaw());
        });
    }
    if two {
        hk::flat_windows(g, c, rng, &kf, gh, 1, 3.0, 2.0, 0.4, 0.1, FlatDress { more_ac: 0.3, ledge_below: true });
        g.lod(Lod::Near, |g| laundry_poles(g, c, rng, &kf, gh - 3.0, 2, 3.0, 0.6));
    }
    grime(g, c, rng, &kf, 0.0, h, 1.4 + c.wear);
    let back = Face::from_edge(front.at(t1, -d1), front.at(t0, -d1));
    grime(g, c, rng, &back, 0.0, h, 1.5);
    hk::pipe_run(g, c, rng, &back, 0.0, 0.2, h, 3.0, 2);
    parapet(g, c, &body, h, 0.6, 0.18, Concrete, mul(tilecol, 0.8));
    let q = front.at(0.0, -(alley + d1) * 0.5 - 1.0);
    g.lod(Lod::Mid, |g| chiller(g, c, q, h, 2.4, 1.5, front.yaw(), mul(pal.steel, 0.75)));
    // Lanterns across the alley, wall to poles at the street side.
    let m = ((kf.len / 5.0).floor() as i32).clamp(1, 6);
    for k in 0..=m {
        let t = -kf.len * 0.5 + 0.4 + k as f32 * (kf.len - 0.8) / m as f32;
        let pole = front.at(t, -0.5);
        if !c.inside_local(pole) {
            continue;
        }
        f.bx(g, Matte, pole.x, 1.75, pole.y, Vec3::new(0.1, 3.5, 0.1), CHARCOAL);
        let zig = if k % 2 == 0 { 1.2 } else { -1.2 };
        let wallp = kf.at((t + zig).clamp(-kf.len * 0.5 + 0.3, kf.len * 0.5 - 0.3), 0.05);
        lantern_string(g, c, rng, p3(pole.x, 3.4, pole.y), p3(wallp.x, 3.1, wallp.y), 0.35, 1.1);
    }
    // A delivery van in the alley, room permitting.
    let vp = front.at(rng.random_range(-0.25..0.25) * kf.len, -alley * 0.5);
    let yaw = front.yaw();
    if alley > 3.4 && vehicle_fits(c, vp, yaw, Vehicle::Van) {
        let col = pick(rng, &[[0.85, 0.85, 0.82], [0.7, 0.18, 0.12], [0.2, 0.36, 0.6]]);
        g.lod(Lod::Mid, |g| vehicle(g, f, pal, rng, vp, 0.0, yaw, Vehicle::Van, col));
    }
}

// ── the compound ─────────────────────────────────────────────────────────────

/// A family's or a billionaire's courtyard shophouse compound: a range of
/// shophouses on the street with the family's shops and a gateway in the
/// middle, side ranges and a tall main hall with galleries round a paved
/// courtyard, a pond, a tree, lanterns strung across from range to range;
/// tile roofs throughout. On an estate's four plots the hall rises a
/// storey higher and a pavilion stands in the court.
pub fn compound(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng) {
    let f = &c.frame;
    let pal = c.palette;
    // Fitted with the hall's eaves (0.8 m and the corners) on the lot.
    let Some(r) = fit(c, R::of(&offset_poly(&c.lot_poly, 0.2)), 12.4).map(|r| r.inset(1.2)) else { return shophouses(g, c, rng) };
    yard(g, c, 0.3, Paving, mul(pal.paving, 0.75));
    let fh = 3.3;
    let rd = (r.d() * 0.22).clamp(5.0, 8.0);
    let bd = (r.d() * 0.28).clamp(6.0, 10.0);
    let sd = (r.w() * 0.16).clamp(4.0, 6.5);
    let court = R::new(r.u0 + sd, r.u1 - sd, r.v0 + rd, r.v1 - bd);
    if court.w() < 5.0 || court.d() < 5.0 {
        return shophouses(g, c, rng);
    }
    let big = r.w() * r.d() > 1300.0;
    let tile = tiles(c, rng);
    let lacquer = mul(LACQUER[rng.random_range(0..2)], 1.0 - 0.3 * c.wear);
    let walls = mul(mix(pick(rng, &pal.plaster), pick(rng, &c.style.wall_colors), 0.4), 1.0 - 0.2 * c.wear);
    let cx = r.c().x;
    let gate = 4.2;
    // The street range, either side of the gate: shops under flats.
    for (u0, u1) in [(r.u0, cx - gate * 0.5), (cx + gate * 0.5, r.u1)] {
        let b = R::new(u0, u1, r.v0, r.v0 + rd);
        if b.w() < 2.5 {
            continue;
        }
        block(g, c, Plaster, &b, 0.0, fh * 2.0, walls);
        let front = b.front();
        let n = ((front.len / 4.6).floor() as i32).max(1);
        for k in 0..n {
            let t = -front.len * 0.5 + (k as f32 + 0.5) * front.len / n as f32;
            let w = front.len / n as f32;
            let sub = Face::from_edge(front.at(t - w * 0.5 + 0.05, 0.0), front.at(t + w * 0.5 - 0.05, 0.0));
            shop(g, c, rng, &sub, fh, false);
        }
        hk::flat_windows(g, c, rng, &front, fh, 1, fh, 2.0, 0.2, 0.0, FlatDress { more_ac: 0.1, ledge_below: true });
        pane_row(g, c, rng, &b.back(), 0.0, 2, fh, 2.4, 1.2, 1.5, true);
        ridge_roof(g, c, &b.poly(), fh * 2.0, (rd * 0.22).clamp(1.0, 1.8), Vec2::Y, 0.96, 0.35, Brick, tile);
    }
    // The gate between them.
    gateway(g, c, rng, cx, r.v0 + 0.5, gate - 0.8, fh * 1.6, lacquer, tile);
    // Side ranges along the court.
    for (u0, u1, inner) in [(r.u0, r.u0 + sd, 1.0f32), (r.u1 - sd, r.u1, -1.0)] {
        let b = R::new(u0, u1, court.v0, court.v1);
        block(g, c, Plaster, &b, 0.0, fh * 1.5, walls);
        let face = if inner > 0.0 { b.right() } else { b.left() };
        pane_row(g, c, rng, &face, 0.0, 1, fh, 2.6, 1.3, 1.7, true);
        ridge_roof(g, c, &b.poly(), fh * 1.5, (sd * 0.25).clamp(0.9, 1.6), Vec2::X, 0.96, 0.35, Brick, tile);
    }
    // The main hall at the back, galleries facing the court.
    let storeys = if big { 4 } else { 3 };
    let hall = R::new(r.u0, r.u1, court.v1, r.v1);
    let top = storeys as f32 * fh;
    block(g, c, Plaster, &hall, 0.0, top, walls);
    let hf = hall.front();
    for k in 0..storeys {
        let y0 = k as f32 * fh;
        let n = ((hf.len - 1.0) / 2.4).floor() as i32;
        for i in 0..n.max(1) {
            let t = -(hf.len - 1.0) * 0.5 + (i as f32 + 0.5) * (hf.len - 1.0) / n.max(1) as f32;
            pane(g, c, rng, &hf, t, y0 + fh * 0.5, 1.3, 1.8, true);
        }
        if k > 0 {
            // A gallery: its floor, rail and a lantern per bay.
            // Along the court, between the side ranges.
            let (gc, gw) = ((court.u0 + court.u1) * 0.5 - hall.c().x, court.w() - 0.3);
            hf.bx(g, f, Concrete, gc, y0 - 0.1, 0.6, Vec3::new(gw, 0.2, 1.2), mul(walls, 0.8));
            g.lod(Lod::Mid, |g| hf.bx(g, f, Wood, gc, y0 + 0.98, 1.12, Vec3::new(gw, 0.08, 0.08), lacquer));
            let m = ((gw / 3.0).floor() as i32).max(1);
            for i in 0..=m {
                let t = gc - gw * 0.5 + 0.1 + i as f32 * (gw - 0.2) / m as f32;
                hf.bx(g, f, Wood, t, y0 + 0.5, 1.12, Vec3::new(0.1, 1.0, 0.1), lacquer);
                if i < m {
                    let p = hf.at(t + (gw - 0.2) / m as f32 * 0.5, 1.0);
                    lantern(g, c, p3(p.x, y0 + fh - 0.35, p.y), 0.42, paper(rng), !coin(rng, 0.3 * c.wear));
                }
            }
        }
    }
    pane_row(g, c, rng, &hall.back(), 0.0, storeys, fh, 2.6, 1.2, 1.5, true);
    hip_roof(g, c, &hall, top - 0.1, (hall.d() * 0.3).clamp(1.5, 3.0), 0.8, tile);
    // The court: stone paving, a pond, a tree, lanterns from range to range.
    ground_at(g, c, &court.inset(0.1).poly(), 0.06, mul(pick(rng, &pal.concrete), 1.0));
    let pond = R::new(court.c().x - court.w() * 0.2, court.c().x + court.w() * 0.2, court.c().y - court.d() * 0.15, court.c().y + court.d() * 0.15);
    if pond.w() > 1.5 && pond.d() > 1.2 {
        block(g, c, Concrete, &pond.inset(-0.3), 0.0, 0.35, mul(pick(rng, &pal.concrete), 0.8));
        ground(g, f, Glass, &pond.poly(), 0.37, [0.05, 0.12, 0.14]);
    }
    let tp = Vec2::new(court.u0 + 1.4, court.v1 - 1.4);
    tree(g, f, tp, 4.5 + if big { 1.5 } else { 0.0 }, 2.0, pal.bark, mul(pal.foliage, 0.85));
    if big && court.w() > 12.0 && court.d() > 9.0 {
        // A pavilion in the court.
        let pv = R::new(court.u1 - 4.6, court.u1 - 1.4, court.v0 + 1.4, court.v0 + 4.6);
        for (du, dv) in [(pv.u0 + 0.2, pv.v0 + 0.2), (pv.u1 - 0.2, pv.v0 + 0.2), (pv.u1 - 0.2, pv.v1 - 0.2), (pv.u0 + 0.2, pv.v1 - 0.2)] {
            f.bx(g, Wood, du, 1.4, dv, Vec3::new(0.24, 2.8, 0.24), lacquer);
        }
        hip_roof(g, c, &pv, 2.8, 1.4, 0.6, tile);
        lantern(g, c, p3(pv.c().x, 2.6, pv.c().y), 0.55, 0, !coin(rng, 0.3 * c.wear));
    }
    let lines = ((court.d() / 3.0).floor() as i32).clamp(1, 5);
    for k in 0..lines {
        let v = court.v0 + (k as f32 + 0.5) * court.d() / lines as f32;
        let (ua, ub) = if k % 2 == 0 { (court.u0 + 0.1, court.u1 - 0.1) } else { (court.u1 - 0.1, court.u0 + 0.1) };
        lantern_string(g, c, rng, p3(ua, fh * 1.4, v), p3(ub, fh * 1.4, v + 1.0), 0.6, 1.4);
    }
}
