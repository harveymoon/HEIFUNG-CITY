//! [supertall] League towers (`nd_sim::towers`): skyscrapers, supertalls
//! and megatalls on an assembled site. A podium fills the site; the tower
//! rises from it in one of four forms:
//! - a stepped Art Deco spire (the Empire State type): stone piers, gold
//!   neon on every setback, a floodlit crown and a mast;
//! - bundled tubes on a Y plan (the Burj type): three glass wings round a
//!   hexagonal core, each stepping back in turn up a spiral, the core
//!   running on into a long needle;
//! - a twisting tower: glass floor plates turning as they rise, a halo
//!   crown;
//! - a needle tower: a slim tapering shaft with neon edges and a long lit
//!   needle.
//! Every form carries red aviation lights up its height and a crown that
//! glows at night. The geometry stays cheap for its size: walls are single
//! extrusions per section (the glass and stone textures carry the detail),
//! window lights are one quad per bay per floor, ornaments one quad each.
//!
//! While it is built the site shows the podium going up, the concrete core
//! leading, bare floor plates under it, the glazing following a few floors
//! behind, and a jump crane riding the core.

use super::kit::*;
use super::parts::*;
use super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::towers::{Tower, TowerDesign, TOWER_FLOOR_M};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Floor to floor of the drawn tower (m), as the sim counts it.
const FH: f32 = TOWER_FLOOR_M;

/// Where the tower stands on its site and how much room it has: the
/// deepest point of the podium and the radius of the circle round it that
/// stays on the site.
fn tower_seat(c: &Ctx, site: &[Vec2]) -> (Vec2, f32) {
    let anchor = interior_anchor(site);
    // Search a little round the anchor for a deeper seat.
    let mut best = (distance_to_boundary(anchor, site), anchor);
    let r0 = best.0.max(2.0);
    for k in 0..16 {
        let a = k as f32 / 16.0 * TAU;
        for s in [0.25f32, 0.5] {
            let p = anchor + Vec2::new(a.cos(), a.sin()) * r0 * s;
            if point_in(site, p) {
                let d = distance_to_boundary(p, site);
                if d > best.0 {
                    best = (d, p);
                }
            }
        }
    }
    let _ = c;
    (best.1, best.0.max(5.0))
}

fn point_in(poly: &[Vec2], p: Vec2) -> bool {
    nd_sim::zoning::point_in_polygon(p, poly)
}

/// The site the tower and podium stand on: the parcel (the merged plots)
/// pulled in a little from the pavement.
fn site_poly(c: &Ctx) -> Vec<Vec2> {
    let s = offset_poly(&c.lot_poly, 1.6);
    if s.len() >= 3 { s } else { c.poly.clone() }
}

/// Podium height by class: the glass lobby and a floor or two of stone
/// over it, lower than the old full-site block so the tower, not its base,
/// carries the mass.
fn podium_h(t: &Tower) -> f32 {
    match t.league() {
        nd_sim::towers::League::Skyscraper => 12.0,
        nd_sim::towers::League::Supertall => 13.0,
        nd_sim::towers::League::Megatall => 15.0,
    }
}

/// [sweep-visual] The podium's setback tier: this much higher, stepped in
/// from the lower tier's edge.
const SETBACK_H: f32 = 2.0 * FH;
const SETBACK_IN: f32 = 4.0;
/// How far the podium reaches past the tower's base on each side (a lobby
/// round it); a bigger site beyond that is open plaza.
const PODIUM_WRAP: f32 = 5.0;

/// How far the tower's base reaches from its seat along the lot's axes:
/// the half width of its widest plan, as the four forms size it.
fn foot(t: &Tower, room: f32) -> f32 {
    match t.design {
        TowerDesign::DecoSpire => (t.height / 15.0).max(10.0).min(room * 0.92),
        TowerDesign::Bundled => (t.height / 11.0).max(12.0).min(room * 0.95),
        TowerDesign::Twist => (t.height / 15.0).max(10.0).min(room * 0.95),
        TowerDesign::Needle => (t.height / 16.0).max(9.0).min(room * 0.95),
    }
}

/// [sweep-visual] The podium's plan: the site round the tower's base, a
/// lobby's depth beyond it. On a site of one or two plots that is the whole
/// site; on an assembled megatall site the rest is plaza.
fn podium_plan(site: &[Vec2], seat: Vec2, reach: f32) -> Vec<Vec2> {
    let a = reach + PODIUM_WRAP;
    let p = clip_box(site, seat.x - a, seat.x + a, seat.y - a, seat.y + a);
    if p.len() >= 3 { p } else { site.to_vec() }
}

/// Spire share of the height by design.
fn spire_share(d: TowerDesign) -> f32 {
    match d {
        TowerDesign::DecoSpire => 0.13,
        TowerDesign::Bundled => 0.2,
        TowerDesign::Twist => 0.06,
        TowerDesign::Needle => 0.24,
    }
}

/// The finished tower.
pub fn tower(g: &mut Geometry, c: &Ctx, t: &Tower) {
    let mut rng = nd_sim::rng_for(t.seed, 71);
    let rng = &mut rng;
    let site = site_poly(c);
    let ph = podium_h(t);
    let (seat, room) = tower_seat(c, &site);
    let pod = podium_plan(&site, seat, foot(t, room));
    podium(g, c, rng, &pod, ph);
    plaza(g, c, t.seed, &site, &pod);
    let neon = neon_colour(c, t.seed);
    match t.design {
        TowerDesign::DecoSpire => deco_spire(g, c, rng, t, seat, room, ph, neon),
        TowerDesign::Bundled => bundled(g, c, rng, t, seat, room, ph, neon),
        TowerDesign::Twist => twist(g, c, rng, t, seat, room, ph, neon),
        TowerDesign::Needle => needle(g, c, rng, t, seat, room, ph, neon),
    }
}

/// The tower's own neon: one of the city's sign colours, by its seed.
fn neon_colour(c: &Ctx, seed: u64) -> [f32; 3] {
    let cols = &c.palette.sign_colors;
    if cols.is_empty() {
        return [1.0, 0.3, 0.8];
    }
    cols[(seed >> 7) as usize % cols.len()]
}

// ── podium ───────────────────────────────────────────────────────────────────

fn podium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, site: &[Vec2], h: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let stone = mix(pal.stone, [0.86, 0.84, 0.8], 0.35);
    // Plaza paving round it.
    ground(g, f, Paving, &c.lot_poly, 0.04, mul(pal.paving, 1.05));
    let lobby_h = 7.0;
    // A recessed glass lobby under a stone podium.
    let lobby = offset_poly(site, 1.2);
    let lobby = if lobby.len() >= 3 { lobby } else { site.to_vec() };
    extrude(g, f, Glass, &lobby, 0.0, lobby_h, mul(pal.glass_dark, 1.2));
    extrude(g, f, Stone, site, lobby_h, h, stone);
    cornice(g, c, site, lobby_h, 0.6, 0.5, Stone, mul(stone, 0.9));
    cornice(g, c, site, h, 0.8, 0.4, Stone, mul(stone, 0.85));
    // Lobby lights and the podium's own windows.
    let faces = c.faces(site);
    let front_i = c.front_face_index(site).unwrap_or(0);
    for (i, face) in faces.iter().enumerate() {
        if face.len < 4.0 {
            continue;
        }
        band_windows(g, c, rng, face, lobby_h + 0.6, ((h - lobby_h - 1.0) / 4.2).floor().max(1.0) as i32, 4.2);
        // A neon line along the lobby head.
        face.bx(g, f, Glow, 0.0, lobby_h - 0.3, 0.12, Vec3::new(face.len * 0.94, 0.14, 0.1), mul(pal.window_warm, 1.1));
        if i == front_i {
            // The entrance: a deep canopy and a lit name band over it.
            canopy(g, c, face, 0.0, lobby_h - 1.2, (face.len * 0.4).min(16.0), 3.2, mul(pal.steel, 0.7));
            if face.len > 14.0 {
                led_screen(g, c, rng, face, face.len * 0.3, lobby_h + 4.0, (face.len * 0.22).min(10.0), 5.0);
            }
            let lamps = ((face.len / 9.0) as i32).clamp(2, 6);
            for k in 0..lamps {
                let tt = -face.len * 0.45 + (k as f32 + 0.5) * face.len * 0.9 / lamps as f32;
                let p = face.at(tt, 2.2);
                if c.inside_local(p) {
                    lamp_post_in(g, c, p, 4.5, pal.lamp);
                }
            }
            bollards(g, c, face, 6, 1.2);
        }
    }
    // [sweep-visual] A setback tier: glass between stone bands, stepped in
    // from the edge, with a planted terrace on the ledge round it.
    let upper = offset_poly(site, SETBACK_IN);
    let terrace = offset_poly(site, 1.0);
    if terrace.len() >= 3 {
        ground(g, f, Foliage, &terrace, h + 0.05, mul(pal.foliage, 0.8));
    }
    if upper.len() >= 3 && nd_sim::zoning::polygon_area(&upper).abs() > 60.0 {
        extrude(g, f, Glass, &upper, h, h + SETBACK_H, mul(pal.glass_dark, 1.35));
        cornice(g, c, &upper, h + SETBACK_H, 0.6, 0.4, Stone, mul(stone, 0.85));
        curtain_lights(g, c, rng, &upper, h, 2, FH);
        // Low hedges along the terrace edge.
        for face in c.faces(&offset_poly(site, 1.6)) {
            if face.len > 3.0 {
                face.bx(g, f, Foliage, 0.0, h + 0.35, 0.0, Vec3::new(face.len * 0.9, 0.7, 0.6), pal.foliage);
            }
        }
    }
}

/// [sweep-visual] The open part of a tower's site round its podium: paving
/// with tree planters, lawn beds, benches and lamps in a grid, and a
/// fountain when there is room for one. Nothing goes within a few metres of
/// the podium or the site's edge.
fn plaza(g: &mut Geometry, c: &Ctx, seed: u64, site: &[Vec2], pod: &[Vec2]) {
    let f = &c.frame;
    let pal = c.palette;
    let near_pod = offset_poly(pod, -2.0);
    let near_pod = if near_pod.len() >= 3 { near_pod } else { pod.to_vec() };
    let (lo, hi) = bounds(site);
    let step = 6.5;
    let nu = ((hi.x - lo.x) / step).floor() as i32;
    let nv = ((hi.y - lo.y) / step).floor() as i32;
    let (pad_u, pad_v) = (((hi.x - lo.x) - nu as f32 * step) * 0.5, ((hi.y - lo.y) - nv as f32 * step) * 0.5);
    let mut open: Vec<(Vec2, i32)> = Vec::new();
    for j in 0..nv.max(0) {
        for i in 0..nu.max(0) {
            let p = Vec2::new(lo.x + pad_u + (i as f32 + 0.5) * step, lo.y + pad_v + (j as f32 + 0.5) * step);
            if !point_in(site, p) || distance_to_boundary(p, site) < 2.2 || point_in(&near_pod, p) || distance_to_boundary(p, &near_pod) < 1.2 {
                continue;
            }
            open.push((p, (i + 2 * j).rem_euclid(5)));
        }
    }
    if open.is_empty() {
        return;
    }
    // A fountain at the open cell furthest from the podium, on a big plaza.
    let fountain = if open.len() >= 14 { open.iter().map(|(p, _)| *p).max_by(|a, b| distance_to_boundary(*a, pod).total_cmp(&distance_to_boundary(*b, pod))) } else { None };
    if let Some(q) = fountain {
        pool(g, c, q.x, q.y, 9.0, 9.0);
        for k in 0..4 {
            let a = k as f32 * FRAC_PI_2 + 0.4;
            let p = q + Vec2::new(a.cos(), a.sin()) * 2.6;
            f.bx(g, Glow, p.x, 0.9, p.y, Vec3::new(0.18, 1.4, 0.18), mul(pal.window_cold, 0.9));
        }
    }
    let kerb = mul(pal.stone, 1.15);
    let mut trees = 0;
    for (k, (p, pat)) in open.iter().enumerate() {
        if fountain.is_some_and(|q| q.distance(*p) < 9.0) {
            continue;
        }
        let h = crate::hash::hash2_salted(seed, k as u64, crate::hash::SALT_FISHING);
        match pat {
            // A tree in a raised planter.
            0 | 2 if trees < 60 => {
                trees += 1;
                let bed = rect(p.x, p.y, 1.7, 1.7);
                extrude(g, f, Stone, &bed, 0.0, 0.5, kerb);
                ground(g, f, Foliage, &offset_poly(&bed, 0.2), 0.52, mul(pal.foliage, 0.7));
                tree_in(g, f, pal, *p, 7.0 + (h % 5) as f32, 2.6 + ((h >> 8) & 3) as f32 * 0.3, pal.foliage);
            }
            // A lawn bed with a low kerb.
            1 => {
                let bed = rect(p.x, p.y, 2.4, 2.4);
                extrude(g, f, Stone, &bed, 0.0, 0.18, kerb);
                ground(g, f, Foliage, &offset_poly(&bed, 0.25), 0.2, mul(pal.foliage, 0.95));
            }
            // A bench, with a lamp by every other one.
            3 => {
                let w = f.to_world(p.x, p.y);
                bench_world(g, w, f.yaw() + if h & 1 == 0 { 0.0 } else { PI }, mul(pal.paving, 0.6), pal.steel);
                if (h >> 3) & 1 == 0 {
                    lamp_post_in(g, c, *p + Vec2::new(1.6, 0.0), 4.5, pal.lamp);
                }
            }
            _ => {}
        }
    }
}

// ── shared pieces ───────────────────────────────────────────────────────────

/// One straight section of shaft: a solid of `poly` from `y0` to `y1` in
/// `s`, a floor-plate band at its top, and window lights per floor (or per
/// two floors high up, where nobody can count them).
#[allow(clippy::too_many_arguments)]
fn shaft(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y0: f32, y1: f32, s: crate::surface::Surface, wall: [f32; 3], band: [f32; 3]) {
    let f = &c.frame;
    extrude(g, f, s, poly, y0, y1, wall);
    // Sides only: its caps would lie in the shaft's top and the next
    // section's bottom.
    let edge = offset_poly(poly, -0.15);
    super::parts::band(g, f, Panel, &edge, &edge, y1 - 0.5, y1, band);
    lights(g, c, rng, poly, y0, y1);
}

/// `a` and `b` (same vertex count) mixed `t` of the way: the plan of a
/// lofted section at a height inside it.
fn lerp_plan(a: &[Vec2], b: &[Vec2], t: f32) -> Vec<Vec2> {
    if a.len() != b.len() {
        return a.to_vec();
    }
    a.iter().zip(b).map(|(p, q)| p.lerp(*q, t)).collect()
}

/// The plan at height `y` of a shaft lofted in `sections` straight sections
/// from `plan(0)` at `y0` to `plan(1)` at `y1`: on the glass, not on the
/// plan of the section's foot.
fn plan_at(plan: &impl Fn(f32) -> Vec<Vec2>, sections: i32, y0: f32, y1: f32, y: f32) -> Vec<Vec2> {
    let k = ((y - y0) / (y1 - y0).max(1e-3)).clamp(0.0, 1.0) * sections as f32;
    let i = (k.floor() as i32).min(sections - 1).max(0);
    lerp_plan(&plan(i as f32 / sections as f32), &plan((i + 1) as f32 / sections as f32), k - i as f32)
}

/// [`lights`] on a lofted section from `prev` at `y0` to `next` at `y1`:
/// each row on the plan at its own height, so a twisting or tapering shaft
/// keeps its lights on the glass.
fn lofted_lights(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, prev: &[Vec2], next: &[Vec2], y0: f32, y1: f32) {
    let floors = ((y1 - y0) / FH).floor() as i32;
    if floors < 1 {
        return;
    }
    let (rows, rh) = if y0 < 200.0 { (floors, FH) } else { ((floors / 2).max(1), FH * 2.0) };
    for k in 0..rows {
        let y = y0 + k as f32 * rh;
        let plan = lerp_plan(prev, next, (y + rh * 0.5 - y0) / (y1 - y0));
        curtain_lights(g, c, rng, &plan, y, 1, rh);
    }
}

/// [`lit_band`] on a lofted section: each floor on the plan at its height.
#[allow(clippy::too_many_arguments)]
fn lofted_lit_band(g: &mut Geometry, c: &Ctx, prev: &[Vec2], next: &[Vec2], ys0: f32, ys1: f32, y0: f32, floors: i32, color: [f32; 3]) {
    let f = &c.frame;
    for k in 0..floors {
        let y = y0 + k as f32 * FH + FH * 0.5;
        let plan = lerp_plan(prev, next, (y - ys0) / (ys1 - ys0).max(1e-3));
        for (fi, face) in c.faces(&plan).iter().enumerate() {
            let segs = (face.len / 3.2).floor().max(1.0) as i32;
            let seg = face.len / segs as f32;
            for i in 0..segs {
                if (i * 7 + k * 3 + fi as i32 * 5) % 9 == 0 {
                    continue;
                }
                let t = -face.len * 0.5 + (i as f32 + 0.5) * seg;
                face.decal(g, f, Glow, t, y, 0.12, seg * 0.85, FH * 0.5, color);
            }
        }
    }
}

fn lights(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y0: f32, y1: f32) {
    let floors = ((y1 - y0) / FH).floor() as i32;
    if floors < 1 {
        return;
    }
    // Every floor low down; every other floor above 200 m (a band two
    // floors high), so a megatall is not ten thousand quads of light.
    if y0 < 200.0 {
        curtain_lights(g, c, rng, poly, y0, floors, FH);
    } else {
        curtain_lights(g, c, rng, poly, y0, (floors / 2).max(1), FH * 2.0);
    }
}

/// [sweep-visual] Floors of a landmark that stay lit all night (a sky
/// lobby, the floors under the crown): plain glow, not window lights, so
/// the renderer never switches them off and the skyline keeps its shape
/// after the offices empty. `floors` floors from `y0`, a few bays dark.
fn lit_band(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y0: f32, floors: i32, color: [f32; 3]) {
    let f = &c.frame;
    for (fi, face) in c.faces(poly).iter().enumerate() {
        let segs = (face.len / 3.2).floor().max(1.0) as i32;
        let seg = face.len / segs as f32;
        for k in 0..floors {
            let y = y0 + k as f32 * FH + FH * 0.5;
            for i in 0..segs {
                if (i * 7 + k * 3 + fi as i32 * 5) % 9 == 0 {
                    continue;
                }
                let t = -face.len * 0.5 + (i as f32 + 0.5) * seg;
                face.decal(g, f, Glow, t, y, 0.12, seg * 0.85, FH * 0.5, color);
            }
        }
    }
}

/// The colour of a landmark's always-lit floors: warm white, a touch of
/// its neon.
fn landmark_light(c: &Ctx, neon: [f32; 3]) -> [f32; 3] {
    mul(mix(mix(c.palette.window_warm, [1.0, 0.95, 0.85], 0.4), neon, 0.15), 1.1)
}

/// Red aviation lights at the corners of `poly` at height `y`.
fn aviation(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32) {
    let f = &c.frame;
    let step = (poly.len() / 4).max(1);
    for p in poly.iter().step_by(step) {
        aviation_light_local(g, f, c.palette, p.x, y, p.y, 1.4);
    }
}

/// A mast or needle from `y0` to `y1` at `p`: a tapering octagon in
/// segments, rings of light on it and a red light at the tip.
fn mast(g: &mut Geometry, c: &Ctx, p: Vec2, r0: f32, y0: f32, y1: f32, neon: [f32; 3], rings: bool) {
    let f = &c.frame;
    let segs = (((y1 - y0) / 18.0).ceil() as i32).clamp(2, 10);
    let steel = mul(c.palette.steel, 1.15);
    for k in 0..segs {
        let a = k as f32 / segs as f32;
        let b = (k + 1) as f32 / segs as f32;
        let ra = (r0 * (1.0 - a * 0.92)).max(0.12);
        let rb = (r0 * (1.0 - b * 0.92)).max(0.08);
        let ya = y0 + (y1 - y0) * a;
        let yb = y0 + (y1 - y0) * b;
        loft(g, f, Metal, &ngon(p.x, p.y, ra, 8, PI / 8.0), &ngon(p.x, p.y, rb, 8, PI / 8.0), ya, yb, steel);
        if rings && k > 0 {
            // A band: its caps would be inside the mast.
            g.part(Glow).band(&f.poly3(&ngon(p.x, p.y, ra + 0.35, 8, PI / 8.0), ya - 0.9), &f.poly3(&ngon(p.x, p.y, ra + 0.35, 8, PI / 8.0), ya + 0.9), rgba(neon));
        }
    }
    aviation_light_local(g, f, c.palette, p.x, y1 + 1.0, p.y, 2.0);
    aviation_light_local(g, f, c.palette, p.x, y0 + (y1 - y0) * 0.5, p.y, r0 * 0.6 + 0.5);
}

/// Aviation lights up the shaft: every ninety metres or so, on the plan
/// the shaft has at that height (`plan(y)`), so they stay on a tapering or
/// twisting tower instead of hanging in the air beside it.
fn aviation_up(g: &mut Geometry, c: &Ctx, plan: impl Fn(f32) -> Vec<Vec2>, y0: f32, y1: f32) {
    let mut y = y0 + 90.0;
    while y < y1 - 20.0 {
        aviation(g, c, &plan(y), y);
        y += 90.0;
    }
}

// ── the Art Deco spire ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn deco_spire(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, t: &Tower, seat: Vec2, room: f32, base: f32, neon: [f32; 3]) {
    let f = &c.frame;
    let pal = c.palette;
    let top = t.height * (1.0 - spire_share(t.design));
    let hw = (t.height / 15.0).max(10.0).min(room * 0.92);
    let hd = hw * 0.78;
    let stone = mix(pal.stone, [0.88, 0.84, 0.74], 0.45);
    let pier = mul(stone, 1.12);
    let gold = mix(neon, [1.0, 0.75, 0.3], 0.6);
    // Tiers: share of the height to the top of each, and its scale.
    let tiers: [(f32, f32); 4] = [(0.52, 1.0), (0.72, 0.8), (0.86, 0.62), (1.0, 0.44)];
    let mut y0 = base;
    let mut last = rect(seat.x, seat.y, hw, hd);
    for (i, (share, k)) in tiers.iter().enumerate() {
        let y1 = base + (top - base) * share;
        let plan = rect(seat.x, seat.y, hw * k, hd * k);
        extrude(g, f, Stone, &plan, y0, y1, stone);
        // Piers: stone ribs up each face with dark glass between, one quad each.
        for face in c.faces(&plan) {
            let n = ((face.len / 4.2) as i32).max(2);
            let step = face.len / n as f32;
            for j in 0..n {
                let tt = -face.len * 0.5 + (j as f32 + 0.5) * step;
                // 3 cm out: clear of the dark glass a switchable window
                // light lays 1.3 cm behind itself (0.037).
                face.decal(g, f, Glass, tt, (y0 + y1) * 0.5, 0.03, step * 0.55, (y1 - y0) - 1.0, mul(pal.glass_dark, 1.1));
                face.bx(g, f, Stone, tt - step * 0.5, (y0 + y1) * 0.5, 0.2, Vec3::new(0.7, y1 - y0, 0.4), pier);
            }
        }
        lights(g, c, rng, &plan, y0, y1);
        // [sweep-visual] The floors under each setback stay lit.
        if y1 - y0 > 4.0 * FH {
            lit_band(g, c, &plan, y1 - 3.0 * FH, 2, landmark_light(c, gold));
        }
        // The setback: a stone cornice and gold neon along its edge.
        cornice(g, c, &plan, y1, 0.9, 0.35, Stone, mul(stone, 0.92));
        glow_outline(g, c, &offset_poly(&plan, -0.5), y1 + 1.0, 1.0, gold);
        if i == tiers.len() - 1 {
            // The crown: floodlit fins and a lantern.
            for face in c.faces(&plan) {
                let n = ((face.len / 2.6) as i32).max(2);
                for j in 0..n {
                    let tt = -face.len * 0.45 + (j as f32 + 0.5) * face.len * 0.9 / n as f32;
                    face.decal(g, f, Glow, tt, y1 - (y1 - y0) * 0.3, 0.3, 1.3, (y1 - y0) * 0.55, mul(gold, 1.2));
                }
            }
        }
        last = plan;
        y0 = y1;
    }
    // The tier standing at each height.
    let tier_plan = |y: f32| {
        let k = tiers.iter().find(|(share, _)| y <= base + (top - base) * share).map_or(tiers[3].1, |t| t.1);
        rect(seat.x, seat.y, hw * k, hd * k)
    };
    aviation_up(g, c, tier_plan, base, top * 0.86);
    // A stepped lantern and the mast.
    let lantern = rect(seat.x, seat.y, hw * 0.24, hd * 0.24 / 0.78 * 0.78);
    extrude(g, f, Stone, &lantern, top, top + 8.0, stone);
    glow_outline(g, c, &lantern, top + 8.4, 0.8, gold);
    let lit = scale_poly(&lantern, 1.02);
    super::parts::band(g, f, Glow, &lit, &lit, top + 2.0, top + 5.0, mul(gold, 0.8));
    let _ = last;
    mast(g, c, seat, (hw * 0.12).clamp(0.8, 3.0), top + 8.0, t.height, gold, true);
}

// ── bundled tubes, Y plan ───────────────────────────────────────────────────

/// One wing of a Y plan: a rectangle from near the centre outward.
fn wing(seat: Vec2, dir: f32, len: f32, half_w: f32) -> Vec<Vec2> {
    let d = Vec2::new(dir.cos(), dir.sin());
    let n = Vec2::new(-d.y, d.x);
    let a = seat + d * (half_w * 0.2);
    let b = seat + d * len;
    // A rounded nose: the far end chamfered.
    let ch = half_w * 0.45;
    vec![a - n * half_w, b - n * half_w - d * ch, b - n * (half_w - ch), b + n * (half_w - ch), b + n * half_w - d * ch, a + n * half_w]
}

#[allow(clippy::too_many_arguments)]
fn bundled(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, t: &Tower, seat: Vec2, room: f32, base: f32, neon: [f32; 3]) {
    let f = &c.frame;
    let pal = c.palette;
    let core_top = t.height * (1.0 - spire_share(t.design));
    let reach = (t.height / 11.0).max(12.0).min(room * 0.95);
    let half_w = (reach * 0.3).max(4.0);
    let glass = mix(pick(rng, &pal.glass), [0.75, 0.82, 0.9], 0.35);
    let steel = mul(pal.steel, 1.2);
    let rot = (t.seed % 360) as f32 / 360.0 * TAU;
    // The core: a hexagon to the top of the shaft.
    let core = ngon(seat.x, seat.y, half_w * 1.15, 6, rot);
    shaft(g, c, rng, &core, base, core_top, Glass, glass, steel);
    // [sweep-visual] Sky lobbies every 120 m and the floors under the crown
    // stay lit all night.
    let warm = landmark_light(c, neon);
    let mut y = base + 120.0;
    while y < core_top - 40.0 {
        lit_band(g, c, &core, y, 1, warm);
        y += 120.0;
    }
    lit_band(g, c, &core, core_top - 5.0 * FH, 3, warm);
    // Three wings stepping back in turn: each in `steps` segments, the
    // wings' steps staggered a third of a segment, so the setbacks spiral.
    let steps = match t.league() {
        nd_sim::towers::League::Megatall => 8,
        nd_sim::towers::League::Supertall => 6,
        nd_sim::towers::League::Skyscraper => 4,
    };
    for w in 0..3 {
        let dir = rot + w as f32 * TAU / 3.0 + FRAC_PI_2;
        let end = base + (core_top - base) * (0.66 + 0.1 * w as f32);
        let seg = (end - base) / steps as f32;
        let mut y0 = base;
        for s in 0..steps {
            let y1 = if s == steps - 1 { end } else { base + seg * (s as f32 + 1.0 + w as f32 / 3.0) }.min(end);
            if y1 <= y0 + 1.0 {
                continue;
            }
            let len = reach * (1.0 - 0.6 * s as f32 / steps as f32);
            let plan = wing(seat, dir, len, half_w * (1.0 - 0.15 * s as f32 / steps as f32));
            shaft(g, c, rng, &plan, y0, y1, Glass, mul(glass, 1.0 - 0.04 * s as f32), steel);
            // Each wing's floors under every setback stay lit: a spiral of
            // light up the tower at night.
            if y1 - y0 > 3.0 * FH {
                lit_band(g, c, &plan, y1 - 3.0 * FH, if s == steps - 1 { 2 } else { 1 }, warm);
            }
            // The setback lights up.
            let nose = &plan[1..5];
            for k in 0..nose.len() - 1 {
                let (a, b) = (nose[k], nose[k + 1]);
                let m = (a + b) * 0.5;
                let yaw = (b - a).y.atan2((b - a).x);
                f.bx_yaw(g, Glow, m.x, y1 + 0.5, m.y, Vec3::new(a.distance(b), 0.9, 0.9), neon, yaw);
            }
            aviation_light_local(g, f, pal, plan[2].lerp(plan[3], 0.5).x, y1 + 0.8, plan[2].lerp(plan[3], 0.5).y, 1.3);
            y0 = y1;
        }
        // Vertical fins up the wing's nose.
        let nose = wing(seat, dir, reach, half_w);
        let tip = (nose[2] + nose[3]) * 0.5;
        f.bx(g, Panel, tip.x, (base + base + seg) * 0.5, tip.y, Vec3::new(0.5, seg, 0.5), steel);
    }
    aviation_up(g, c, |_| core.clone(), base, core_top);
    // The crown: the core's last floors lit, then the needle.
    glow_outline(g, c, &offset_poly(&core, -0.3), core_top - 2.0, 1.2, neon);
    glow_outline(g, c, &offset_poly(&core, -0.3), core_top - 7.0, 0.8, mul(neon, 0.8));
    glow_outline(g, c, &offset_poly(&core, -0.3), core_top - 12.0, 0.6, mul(neon, 0.6));
    let cap = ngon(seat.x, seat.y, half_w * 0.8, 6, rot);
    loft(g, f, Glass, &core, &cap, core_top, core_top + 10.0, mul(glass, 1.1));
    mast(g, c, seat, (half_w * 0.55).clamp(1.0, 5.0), core_top + 10.0, t.height, neon, true);
}

// ── the twisting tower ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn twist(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, t: &Tower, seat: Vec2, room: f32, base: f32, neon: [f32; 3]) {
    let f = &c.frame;
    let pal = c.palette;
    let top = t.height * (1.0 - spire_share(t.design));
    let r = (t.height / 15.0).max(10.0).min(room * 0.95);
    let glass = mix(pick(rng, &pal.glass), [0.6, 0.8, 0.95], 0.25);
    let band = mul(pal.steel, 1.25);
    let turn = (0.9 + 0.8 * nd_sim::towers::unit(t.seed ^ 0x7715)) * if t.seed & 1 == 0 { 1.0 } else { -1.0 };
    let taper = 0.7;
    // A rounded square: eight corners.
    let plate = |k: f32| -> Vec<Vec2> {
        let rr = r * (1.0 - (1.0 - taper) * k);
        let sq: Vec<Vec2> = (0..8)
            .map(|i| {
                let a = i as f32 / 8.0 * TAU + PI / 8.0;
                let s = if i % 2 == 0 { 1.0 } else { 0.86 };
                Vec2::new(a.cos(), a.sin()) * rr * s
            })
            .collect();
        let (sn, cs) = (turn * k).sin_cos();
        sq.iter().map(|p| seat + Vec2::new(p.x * cs - p.y * sn, p.x * sn + p.y * cs)).collect()
    };
    let sections = (((top - base) / 12.0).ceil() as i32).clamp(8, 48);
    let sh = (top - base) / sections as f32;
    let mut prev = plate(0.0);
    for k in 0..sections {
        let y0 = base + k as f32 * sh;
        let next = plate((k + 1) as f32 / sections as f32);
        loft(g, f, Glass, &prev, &next, y0, y0 + sh, glass);
        // The floor band round the section's top: on the glass there, and
        // without caps.
        let low = offset_poly(&lerp_plan(&prev, &next, 1.0 - 0.4 / sh), -0.2);
        super::parts::band(g, f, Panel, &low, &offset_poly(&next, -0.2), y0 + sh - 0.4, y0 + sh, band);
        lofted_lights(g, c, rng, &prev, &next, y0, y0 + sh);
        // [sweep-visual] The top sections and a band every quarter stay lit.
        if k >= sections - 2 || (k > 0 && k % (sections / 4).max(2) == 0) {
            lofted_lit_band(g, c, &prev, &next, y0, y0 + sh, y0, ((sh / FH) as i32).clamp(1, 2), landmark_light(c, neon));
        }
        prev = next;
    }
    aviation_up(g, c, |y| plan_at(&plate, sections, base, top, y), base, top);
    // A halo crown: rings of neon over an open frame, and a short mast.
    for (i, k) in [0.0f32, 6.0, 12.0].iter().enumerate() {
        let ring = scale_poly(&prev, 1.0 - 0.08 * i as f32);
        glow_outline(g, c, &offset_poly(&ring, -0.3), top + k + 0.6, 1.1, if i == 1 { mul(neon, 0.8) } else { neon });
        for p in ring.iter().step_by(2) {
            f.bx(g, Metal, p.x, top + k * 0.5 + 3.0, p.y, Vec3::new(0.35, 6.0 + k, 0.35), band);
        }
    }
    mast(g, c, seat, (r * 0.08).clamp(0.6, 2.0), top + 12.0, t.height.max(top + 20.0), neon, false);
}

// ── the needle tower ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn needle(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, t: &Tower, seat: Vec2, room: f32, base: f32, neon: [f32; 3]) {
    let f = &c.frame;
    let pal = c.palette;
    let top = t.height * (1.0 - spire_share(t.design));
    let r = (t.height / 16.0).max(9.0).min(room * 0.95);
    let glass = mix(pick(rng, &pal.glass), [0.3, 0.35, 0.45], 0.4);
    let edge = mul(neon, 1.0);
    let rot = PI / 4.0 + (t.seed % 7) as f32 * 0.05;
    let plan = |k: f32| -> Vec<Vec2> {
        // A chamfered square, tapering to 45% at the top.
        let rr = r * (1.0 - 0.55 * k * k.sqrt());
        (0..8)
            .map(|i| {
                let a = rot + (i / 2) as f32 * FRAC_PI_2 + if i % 2 == 0 { -0.32 } else { 0.32 };
                seat + Vec2::new(a.cos(), a.sin()) * rr
            })
            .collect()
    };
    let sections = (((top - base) / 20.0).ceil() as i32).clamp(6, 30);
    let sh = (top - base) / sections as f32;
    let mut prev = plan(0.0);
    for k in 0..sections {
        let y0 = base + k as f32 * sh;
        let next = plan((k + 1) as f32 / sections as f32);
        loft(g, f, Glass, &prev, &next, y0, y0 + sh, glass);
        lofted_lights(g, c, rng, &prev, &next, y0, y0 + sh);
        // [sweep-visual] The top sections and a band every quarter stay lit.
        if k >= sections - 2 || (k > 0 && k % (sections / 4).max(2) == 0) {
            lofted_lit_band(g, c, &prev, &next, y0, y0 + sh, y0, ((sh / FH) as i32).clamp(1, 2), landmark_light(c, neon));
        }
        // Neon edges up the chamfered corners.
        for i in (0..8).step_by(2) {
            let (a, b) = (prev[i], next[i]);
            g.part(Glow).tube(f.at2(a, y0), f.at2(b, y0 + sh), 0.5, 4, rgba(edge));
        }
        prev = next;
    }
    aviation_up(g, c, |y| plan_at(&plan, sections, base, top, y), base, top);
    glow_outline(g, c, &offset_poly(&prev, -0.3), top + 0.6, 1.2, neon);
    // A sky deck: a glass drum with a lit rim, then the long needle.
    let deck = ngon(seat.x, seat.y, inradius(&prev).max(3.0) * 1.15, 12, 0.0);
    extrude(g, f, Glass, &deck, top, top + 7.0, mul(glass, 1.3));
    glow_outline(g, c, &offset_poly(&deck, -0.3), top + 7.4, 1.0, mul(pal.window_cold, 1.2));
    let lit = scale_poly(&deck, 0.98);
    super::parts::band(g, f, Glow, &lit, &lit, top + 2.0, top + 4.5, mul(neon, 0.7));
    mast(g, c, seat, (r * 0.14).clamp(0.8, 3.5), top + 7.0, t.height, neon, true);
}

// ── the building site ───────────────────────────────────────────────────────

/// The tower going up, `progress` in `0..1`: the podium first, the core
/// leading the frame, bare floor plates, glazing a few floors behind, and a
/// jump crane on the core.
pub fn site(g: &mut Geometry, c: &Ctx, t: &Tower, progress: f32) {
    let mut rng = nd_sim::rng_for(t.seed, 72);
    let rng = &mut rng;
    let f = &c.frame;
    let pal = c.palette;
    let p = progress.clamp(0.0, 1.0);
    let site = site_poly(c);
    let ph = podium_h(t);
    let (seat, room) = tower_seat(c, &site);
    // [sweep-visual] The podium goes up on its own plan (`podium_plan`).
    let pod_plan = podium_plan(&site, seat, foot(t, room));
    let top = t.height * (1.0 - spire_share(t.design));
    // The ground and the hoarding round the plots.
    ground(g, f, Dirt, &c.lot_poly, 0.03, pal.dirt);
    let hoard = offset_poly(&c.lot_poly, 0.3);
    for face in c.faces(if hoard.len() >= 3 { &hoard } else { &c.lot_poly }) {
        face.bx(g, f, Wood, 0.0, 1.25, -0.05, Vec3::new(face.len, 2.5, 0.1), mul(pal.paving, 0.9));
        face.bx(g, f, Glow, 0.0, 2.55, -0.05, Vec3::new(face.len, 0.08, 0.12), mul(pal.window_warm, 0.8));
    }
    if p < 0.04 {
        // Piling rigs on the raft.
        extrude(g, f, Concrete, &site, 0.0, 0.4, mul(pal.concrete(0), 0.7));
        return;
    }
    // Progress: the core rises over 0.05..0.85, the frame trails it.
    let k = ((p - 0.05) / 0.8).clamp(0.0, 1.0);
    let core_h = base_lerp(ph, top, k);
    let frame_h = (core_h - 6.0 * FH).max(0.0);
    let glazed_h = (frame_h - 8.0 * FH).max(0.0);
    let concrete = mul(pal.concrete(0), 0.95);
    // The podium.
    let pod = ph.min(core_h);
    extrude(g, f, Concrete, &site, 0.0, 0.4, mul(pal.concrete(0), 0.7));
    if glazed_h > ph {
        extrude(g, f, Stone, &pod_plan, 0.0, ph, mix(pal.stone, [0.86, 0.84, 0.8], 0.35));
    } else {
        slabs(g, c, &pod_plan, 0.0, pod, concrete);
    }
    // The tower's plan at the base, as the design will have it.
    let r = match t.design {
        TowerDesign::DecoSpire => (t.height / 15.0).max(10.0).min(room * 0.92),
        TowerDesign::Bundled => (t.height / 11.0).max(12.0).min(room * 0.95) * 0.75,
        TowerDesign::Twist => (t.height / 15.0).max(10.0).min(room * 0.95),
        TowerDesign::Needle => (t.height / 16.0).max(9.0).min(room * 0.95),
    };
    let plan = match t.design {
        TowerDesign::DecoSpire => rect(seat.x, seat.y, r, r * 0.78),
        TowerDesign::Bundled => ngon(seat.x, seat.y, r, 6, (t.seed % 360) as f32 / 360.0 * TAU),
        _ => ngon(seat.x, seat.y, r, 8, PI / 8.0),
    };
    let core = scale_poly(&plan, 0.36);
    // The core: bare concrete to the top, with the climbing formwork.
    if core_h > ph {
        extrude(g, f, Concrete, &core, ph, core_h, concrete);
        extrude(g, f, Metal, &offset_poly(&core, -0.6), core_h - 6.0, core_h + 1.5, mul(pal.steel, 0.7));
        glow_outline(g, c, &offset_poly(&core, -0.6), core_h + 1.6, 0.12, mul(pal.window_warm, 0.9));
    }
    // Bare floor plates and columns behind the core, glazing behind them.
    if frame_h > ph {
        slabs(g, c, &plan, ph, frame_h, concrete);
    }
    if glazed_h > ph {
        let glass = mix(pick(rng, &pal.glass), [0.6, 0.75, 0.9], 0.3);
        extrude(g, f, Glass, &offset_poly(&plan, 0.15), ph, glazed_h, glass);
        lights(g, c, rng, &offset_poly(&plan, 0.15), ph, glazed_h);
    }
    // Safety netting at the working floors.
    if frame_h > ph {
        let net = offset_poly(&plan, -0.4);
        extrude(g, f, Tarp, &net, frame_h - 2.0 * FH, frame_h + 1.0, [0.18, 0.32, 0.22]);
    }
    // The jump crane on the core, and its lights.
    if p < 0.97 {
        let mast_top = core_h + 22.0;
        let paint = super::construction::crane_paint(t.seed);
        let mp = poly_center(&core);
        for (du, dv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            f.bx(g, Matte, mp.x + du * 0.8, core_h + 11.0, mp.y + dv * 0.8, Vec3::new(0.2, 22.0, 0.2), paint);
        }
        let ang = nd_sim::towers::unit(t.seed ^ 0xC2A) * TAU;
        let d = Vec2::new(ang.cos(), ang.sin());
        let jib = (r * 1.6).clamp(25.0, 60.0);
        let mid = mp + d * jib * 0.4;
        f.bx_yaw(g, Matte, mid.x, mast_top, mid.y, Vec3::new(jib, 1.2, 1.2), paint, d.y.atan2(d.x));
        let back = mp - d * 7.0;
        f.bx_yaw(g, Concrete, back.x, mast_top - 0.8, back.y, Vec3::new(4.0, 1.8, 2.0), mul(pal.concrete(0), 0.8), d.y.atan2(d.x));
        aviation_light_local(g, f, pal, mp.x, mast_top + 2.0, mp.y, 0.6);
        let tip = mp + d * jib * 0.9;
        aviation_light_local(g, f, pal, tip.x, mast_top + 0.8, tip.y, 0.5);
        // A second crane low down on the podium for the first months.
        if p < 0.45 {
            let q = site.iter().copied().fold(Vec2::ZERO, |a, b| a + b) / site.len() as f32;
            let q = q.lerp(site[0], 0.55);
            if c.inside_local(q) {
                f.bx(g, Matte, q.x, 22.0, q.y, Vec3::new(1.4, 44.0, 1.4), paint);
                f.bx_yaw(g, Matte, q.x + 10.0, 44.5, q.y, Vec3::new(30.0, 1.0, 1.0), paint, 0.3);
            }
        }
    }
    aviation(g, c, &plan, core_h.max(ph) + 0.6);
}

fn base_lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

/// Bare concrete floor plates every floor from `y0` to `y1` round `poly`,
/// with corner columns: what a frame looks like before the glass.
fn slabs(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y0: f32, y1: f32, concrete: [f32; 3]) {
    let f = &c.frame;
    let floors = ((y1 - y0) / FH).floor() as i32;
    // Plates as rings every floor would be thousands of boxes for a
    // megatall: draw every floor's edge, but solid plates every fourth.
    for k in 0..=floors {
        let y = y0 + k as f32 * FH;
        if k % 4 == 0 {
            extrude(g, f, Concrete, poly, y, y + 0.35, concrete);
        } else {
            loft(g, f, Concrete, poly, poly, y, y + 0.3, mul(concrete, 0.95));
        }
    }
    for p in poly {
        let q = p.lerp(poly_center(poly), 0.06);
        f.bx(g, Concrete, q.x, (y0 + y1) * 0.5, q.y, Vec3::new(0.9, (y1 - y0).max(0.1), 0.9), mul(concrete, 0.9));
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use nd_sim::towers::{Tower, TowerDesign};

    fn lots() -> Vec<Lot> {
        use nd_sim::roads::RoadNetwork;
        use nd_sim::zoning::{generate_lots, Zone, ZoneId};
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, -40.0), Vec2::new(300.0, -40.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
        let zone = Zone { polygon: vec![Vec2::new(-100.0, -20.0), Vec2::new(100.0, -20.0), Vec2::new(100.0, 120.0), Vec2::new(-100.0, 120.0)], land_use: LandUse::Commercial };
        let mut lots = slotmap::SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        lots.values().filter(|l| l.usable()).cloned().collect()
    }

    /// [sweep-visual] On an assembled site the podium wraps the tower's
    /// base and leaves the rest as plaza; on a site of a plot or two it
    /// still fills the site.
    #[test]
    fn the_podium_covers_the_tower_not_the_whole_big_site() {
        use super::podium_plan as plan;
        use nd_sim::zoning::polygon_area;
        let big = vec![Vec2::new(-60.0, -35.0), Vec2::new(60.0, -35.0), Vec2::new(60.0, 35.0), Vec2::new(-60.0, 35.0)];
        let pod = plan(&big, Vec2::ZERO, 25.0);
        let (a_pod, a_site) = (polygon_area(&pod).abs(), polygon_area(&big).abs());
        assert!(a_pod < a_site * 0.65, "podium {a_pod} of site {a_site}");
        assert!(pod.iter().all(|p| p.x.abs() <= 30.01 && p.y.abs() <= 35.01), "round the tower: {pod:?}");
        let small = vec![Vec2::new(-20.0, -18.0), Vec2::new(20.0, -18.0), Vec2::new(20.0, 18.0), Vec2::new(-20.0, 18.0)];
        let pod = plan(&small, Vec2::ZERO, 17.0);
        assert!((polygon_area(&pod).abs() - polygon_area(&small).abs()).abs() < 1.0, "a small site is all podium");
    }

    /// Every form on every plot (slivers and triangles among them), built
    /// and at every stage of its site, without a panic, inside a sane
    /// height, with lights and a red light at the top.
    #[test]
    fn every_design_draws_on_any_plot() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        for lot in lots() {
            for (i, design) in TowerDesign::ALL.into_iter().enumerate() {
                for height in [160.0f32, 420.0, 820.0] {
                    let t = Tower { height, design, seed: 7 + i as u64, started_day: 0, topped_out_day: None, record_bid: false };
                    let floors = t.floors();
                    for progress in [None, Some(0.0), Some(0.02), Some(0.3), Some(0.7), Some(0.99)] {
                        let ex = Extras { tower: Some(t), form: Some(nd_sim::estate::Form::Tower), construction: progress, dynamic_lights: true, ..Extras::default() };
                        let g = generate_building(&lot, LandUse::Commercial, floors, Tier::High, 1.0, 3, &rules, &pal, &ex);
                        let top = g.parts.iter().flat_map(|m| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
                        assert!(top.is_finite() && top < height + 80.0, "{design:?} {height} {progress:?}: top {top}");
                        if progress.is_none() {
                            assert!(top > height * 0.95, "{design:?} reaches its height: {top} of {height}");
                            assert!(g.triangle_count() < 400_000, "{design:?} {height}: {} triangles", g.triangle_count());
                        }
                    }
                }
            }
        }
    }

    /// Distance from `p` to the triangle `a b c`.
    fn to_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f32 {
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 {
            return f32::MAX;
        }
        let n = n.normalize();
        let q = p - n * (p - a).dot(n);
        let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= 0.0);
        if inside {
            return (p - q).length();
        }
        let seg = |u: Vec3, v: Vec3| {
            let t = ((p - u).dot(v - u) / (v - u).length_squared().max(1e-12)).clamp(0.0, 1.0);
            (p - (u + (v - u) * t)).length()
        };
        seg(a, b).min(seg(b, c)).min(seg(c, a))
    }

    /// The farthest an aviation light and a window light (marked, drawn
    /// with dynamic lights) of `g` stand from the building's mass, with
    /// where.
    fn light_gaps(g: &Geometry, red: [f32; 3]) -> ((f32, Vec3), (f32, Vec3)) {
        let mass: Vec<[Vec3; 3]> = [Surface::Glass, Surface::Stone, Surface::Panel, Surface::Metal, Surface::Concrete]
            .iter()
            .flat_map(|s| {
                let m = g.get(*s);
                m.indices.chunks_exact(3).map(move |t| [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]))).collect::<Vec<_>>()
            })
            .collect();
        let glow = g.get(Surface::Glow);
        // The beacon over the tip of the mast stands clear of it by design.
        let top = mass.iter().flatten().map(|v| v.y).fold(0.0f32, f32::max);
        let (mut aviation, mut other) = ((0.0f32, Vec3::ZERO), (0.0f32, Vec3::ZERO));
        // Each light's centre: the light boxes and decals are small, so
        // their vertex mean stands for them.
        for t in glow.indices.chunks_exact(6) {
            let ids: Vec<usize> = t.iter().map(|i| *i as usize).collect();
            let p = ids.iter().map(|&i| Vec3::from(glow.positions[i])).sum::<Vec3>() / ids.len() as f32;
            let col = glow.colors[ids[0]];
            let is_red = (col[0] - red[0]).abs() < 1e-3 && (col[1] - red[1]).abs() < 1e-3;
            if p.y > top - 1.0 {
                continue;
            }
            let near = mass.iter().filter(|tri| tri.iter().map(|v| v.y).fold(f32::MAX, f32::min) < p.y + 3.0 && tri.iter().map(|v| v.y).fold(f32::MIN, f32::max) > p.y - 3.0).map(|tri| to_triangle(p, tri[0], tri[1], tri[2])).fold(f32::MAX, f32::min);
            let window = col[3] >= WINDOW_LIGHT_MARK;
            if !is_red && !window {
                continue;
            }
            let slot = if is_red { &mut aviation } else { &mut other };
            if near > slot.0 {
                *slot = (near, p);
            }
        }
        (aviation, other)
    }

    /// Lights sit on the surfaces at their height: the aviation lights on
    /// the shaft as it tapers and steps back, the window lights and lit
    /// bands on the twisting and tapering glass rather than on the plan of
    /// the section's foot.
    #[test]
    fn lights_stay_on_the_shafts() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lot = lots().into_iter().max_by(|a, b| a.area().total_cmp(&b.area())).unwrap();
        for (i, design) in TowerDesign::ALL.into_iter().enumerate() {
            for height in [420.0f32, 820.0] {
                let t = Tower { height, design, seed: 7 + i as u64, started_day: 0, topped_out_day: None, record_bid: false };
                let ex = Extras { tower: Some(t), form: Some(nd_sim::estate::Form::Tower), dynamic_lights: true, ..Extras::default() };
                let g = generate_building(&lot, LandUse::Commercial, t.floors(), Tier::High, 1.0, 3, &rules, &pal, &ex);
                let ((a, pa), (o, po)) = light_gaps(&g, pal.aviation_light);
                assert!(a < 1.6, "{design:?} {height} m: an aviation light {a:.1} m off the tower at {pa}");
                assert!(o < 0.4, "{design:?} {height} m: a window light {o:.2} m off the glass at {po}");
            }
        }
        // The commercial twist tower and the glass castle's seams.
        for style in [Style::TwistTower, Style::Fortress] {
            for seed in 0..4 {
                let g = generate_styled_extras(&lot, LandUse::Commercial, 30, Tier::High, 1.0, seed, style, &rules, &pal, &Extras { dynamic_lights: true, ..Extras::default() });
                let (_, (o, po)) = light_gaps(&g, pal.aviation_light);
                assert!(o < 0.4, "{style:?} seed {seed}: a window light {o:.2} m off the glass at {po}");
            }
        }
    }
}
