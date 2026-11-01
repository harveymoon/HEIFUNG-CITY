//! [megablocks] A mega-corp's arcology (`nd_sim::megablock`): a Kowloon /
//! Hong Kong megastructure over a whole block. A podium fills the site:
//! two storeys of street halls behind shopfronts and blade signs, the
//! market hall's lit band, office floors, car-deck mouths and a grand
//! entrance under the corp's logo; its roof is a park. Three to six
//! residential towers of mismatched height rise from it, each in sections
//! of weathered tile and plaster, every flat its own frame, AC units and
//! cages (the Hong Kong detail pass, `hk`), sky gardens open to the air
//! every dozen storeys (planters and trees behind a lit slab edge), bridges
//! between them, and roofs of tanks, aerials and shacks; the tallest wears
//! the logo at its crown.
//!
//! Building it: the site hoarded off, piling rigs on the raft, the podium's
//! slabs going up under a crane per tower, the towers' cores climbing with
//! jump cranes and the frames behind them, then the cladding and the lit
//! flats climbing, the sky gardens planted last.
//!
//! The geometry stays inside a budget: walls are single extrusions per
//! section, flats one pane each (frames, AC units and cages are the detail
//! layers the renderer drops with distance).

use super::kit::*;
use super::parts::*;
use super::{hk, Ctx, Extras};
use crate::surface::{Geometry, Lod, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::megablock::{MegaBlock, GARDEN_EVERY, PODIUM_FLOORS};
use nd_sim::zoning::point_in_polygon;
use rand::Rng;

/// Storey heights: the street halls and market hall, the offices, the
/// flats (the sim's residential floor).
const HALL_FH: f32 = 4.6;
const OFFICE_FH: f32 = 3.8;
fn flat_fh() -> f32 {
    nd_sim::towers::FLOOR_M[nd_sim::LandUse::Residential.index()]
}

/// One tower: its centre, half sizes (lot-local) and storeys over the
/// podium.
#[derive(Clone, Copy, Debug)]
struct TowerSpot {
    p: Vec2,
    hw: f32,
    hd: f32,
    floors: u8,
}

impl TowerSpot {
    fn poly(&self) -> Vec<Vec2> {
        rect(self.p.x, self.p.y, self.hw, self.hd)
    }
}

/// Where everything stands.
struct Plan {
    site: Vec<Vec2>,
    podium_floors: u8,
    podium_h: f32,
    towers: Vec<TowerSpot>,
}

/// The block's record, or one made up from the building (a preview, the
/// style browser): the same layout rules either way.
fn record(c: &Ctx, e: &Extras) -> MegaBlock {
    let area = nd_sim::zoning::polygon_area(&c.lot_poly).abs();
    let podium = PODIUM_FLOORS.min(c.floors.max(2));
    match e.megablock {
        Some(m) => m,
        None => MegaBlock::plan(c.seed, Default::default(), area.max(400.0), c.floors.saturating_sub(podium).max(1), 0),
    }
}

/// Podium height for `floors` podium storeys: three hall storeys, the rest
/// offices.
fn podium_height(floors: u8) -> f32 {
    let halls = floors.min(3) as f32;
    halls * HALL_FH + floors.saturating_sub(3) as f32 * OFFICE_FH
}

fn plan(c: &Ctx, m: &MegaBlock) -> Plan {
    let site = {
        let s = offset_poly(&c.lot_poly, 1.5);
        if s.len() >= 3 && nd_sim::zoning::polygon_area(&s).abs() > 20.0 { s } else { c.poly.clone() }
    };
    let podium_floors = m.podium_floors.min(c.floors.max(2)).max(2);
    let max_tower = c.floors.saturating_sub(podium_floors).max(1);
    let (lo, hi) = bounds(&site);
    let span = (hi - lo).min_element();
    let target = (span / if m.towers >= 4 { 4.6 } else { 3.8 }).clamp(5.0, 16.0);
    let gap = 7.0;
    let mut towers: Vec<TowerSpot> = Vec::new();
    let step = (span / 24.0).clamp(1.5, 4.0);
    let nu = ((hi.x - lo.x) / step) as i32;
    let nv = ((hi.y - lo.y) / step) as i32;
    let centre = poly_center(&site);
    for i in 0..m.towers {
        let mut best: Option<(f32, Vec2)> = None;
        for jv in 0..=nv {
            for ju in 0..=nu {
                let p = Vec2::new(lo.x + ju as f32 * step, lo.y + jv as f32 * step);
                if !point_in_polygon(p, &site) {
                    continue;
                }
                // A square of half `s` round p stays `margin` inside the site
                // when its half diagonal does.
                let mut s = (distance_to_boundary(p, &site) - 2.0) / std::f32::consts::SQRT_2;
                for t in &towers {
                    let d = (p - t.p).abs();
                    let room = d.x.max(d.y) - t.hw.max(t.hd) - gap;
                    s = s.min(room);
                }
                let s = s.min(target);
                if s < 4.0 {
                    continue;
                }
                // Bigger first; then spread out from the middle (ties by
                // position, so the same lot always plans the same).
                let score = s * 10.0 + p.distance(centre) * 0.01;
                if best.is_none_or(|(b, _)| score > b + 1e-4) {
                    best = Some((score, p));
                }
            }
        }
        let Some((score, p)) = best else { break };
        let s = (score / 10.0).floor().max(4.0).min(target);
        // Slabs, not boxes: a little longer one way by hash.
        let k = 0.75 + 0.25 * nd_sim::towers::unit(m.seed ^ (i as u64 * 0x9E37));
        let (hw, hd) = if i % 2 == 0 { (s, s * k) } else { (s * k, s) };
        towers.push(TowerSpot { p, hw, hd, floors: m.floors_of(i).min(max_tower).max(1) });
    }
    if towers.is_empty() {
        // A small lot: one slab on the podium where it fits.
        let a = interior_anchor(&site);
        let s = ((distance_to_boundary(a, &site) - 1.0) / std::f32::consts::SQRT_2).min(10.0);
        if s >= 1.5 {
            towers.push(TowerSpot { p: a, hw: s, hd: s * 0.85, floors: m.tower_floors.min(max_tower).max(1) });
        }
    }
    Plan { site, podium_floors, podium_h: podium_height(podium_floors), towers }
}

/// The weathered tile and plaster tones of the towers.
const TONES: [[f32; 3]; 6] = [[0.62, 0.60, 0.55], [0.55, 0.58, 0.56], [0.66, 0.58, 0.50], [0.50, 0.52, 0.56], [0.60, 0.52, 0.50], [0.57, 0.57, 0.50]];

fn tone(m: &MegaBlock, i: usize, s: usize) -> [f32; 3] {
    TONES[(nd_sim::hash::mix(m.seed ^ 0x70E5, (i * 31 + s) as u64) % TONES.len() as u64) as usize]
}

/// The finished arcology.
pub fn megablock(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras) {
    let m = record(c, e);
    let pl = plan(c, &m);
    podium(g, c, rng, e, &m, &pl);
    let fh = flat_fh();
    let mut tops: Vec<(usize, f32)> = Vec::new();
    // The corp's colour (else a sign colour) ringing every crown in neon.
    let neon = e.brand.map(|b| mul(b.colour, 2.2)).unwrap_or_else(|| mul(pick(rng, &c.palette.sign_colors), 1.4));
    for (i, t) in pl.towers.iter().enumerate() {
        let top = tower(g, c, rng, &m, i, t, pl.podium_h, fh);
        glow_outline(g, c, &offset_poly(&t.poly(), -0.08), top - 1.4, 0.35, neon);
        glow_outline(g, c, &offset_poly(&t.poly(), -0.08), top - fh * 3.0, 0.2, neon);
        tops.push((i, top));
    }
    bridges(g, c, &m, &pl, fh);
    // The logo on the tallest tower's crown, and over the entrance.
    if let Some(brand) = e.brand.as_ref() {
        if let Some(&(i, top)) = tops.iter().max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0))) {
            let t = pl.towers[i];
            let size = (t.hw.min(t.hd) * 1.5).clamp(4.0, 22.0);
            for (at, out) in [(Vec2::new(t.p.x, t.p.y - t.hd - 0.2), Vec2::new(0.0, -1.0)), (Vec2::new(t.p.x, t.p.y + t.hd + 0.2), Vec2::new(0.0, 1.0)), (Vec2::new(t.p.x - t.hw - 0.2, t.p.y), Vec2::new(-1.0, 0.0)), (Vec2::new(t.p.x + t.hw + 0.2, t.p.y), Vec2::new(1.0, 0.0))] {
                super::brand::logo(g, c, brand, at, out, top - size * 0.5 - 2.0, size);
            }
        }
        let front = c.front_face(&pl.site);
        let size = (front.len * 0.16).clamp(3.0, 11.0);
        if pl.podium_h > size + 10.0 {
            super::brand::logo(g, c, brand, front.at(0.0, 0.2), front.out, pl.podium_h - size * 0.5 - 2.0, size);
        }
    }
}

// ── the podium ───────────────────────────────────────────────────────────────

fn podium(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, e: &Extras, m: &MegaBlock, pl: &Plan) {
    let f = &c.frame;
    let pal = c.palette;
    let site = &pl.site;
    let h = pl.podium_h;
    let concrete = mul(pick(rng, &pal.concrete), 0.72);
    ground(g, f, Paving, &c.lot_poly, 0.04, mul(pal.paving, 0.85));
    // The halls' glass recessed under the upper podium's concrete.
    let halls_top = HALL_FH * pl.podium_floors.min(2) as f32;
    let recess = offset_poly(site, 0.8);
    let recess = if recess.len() >= 3 { recess } else { site.clone() };
    extrude(g, f, Concrete, &recess, 0.0, halls_top, mul(concrete, 0.8));
    extrude(g, f, WornConcrete, site, halls_top, h, concrete);
    let mut detail = hk::stream(c);
    let let_shops = e.units.iter().filter(|u| u.shopfront && !u.vacant()).count();
    let mut sign_k = 0usize;
    let faces = c.faces(site);
    let front_i = c.front_face_index(site).unwrap_or(0);
    for (fi, face) in c.faces(&recess).iter().enumerate() {
        if face.len < 4.0 {
            continue;
        }
        // The street halls: shops behind roller shutters, lit signs.
        hk::shops(g, c, &mut detail, face, 0.0, HALL_FH);
        if pl.podium_floors >= 2 {
            hk::shops(g, c, &mut detail, face, HALL_FH, HALL_FH);
        }
        let _ = fi;
    }
    for (fi, face) in faces.iter().enumerate() {
        if face.len < 4.0 {
            continue;
        }
        // A deep ledge over the halls and the grime streaked down them.
        hk::ledge(g, c, face, halls_top, 1.2, (0.0, 0.0), mul(concrete, 0.9));
        // The market hall: one long lit band of counters.
        if pl.podium_floors >= 3 {
            let y = halls_top + HALL_FH * 0.5;
            face.decal(g, f, Glass, 0.0, y, 0.03, face.len * 0.94, HALL_FH * 0.6, mul(pal.glass_dark, 1.1));
            window_light(g, c, |g| face.decal(g, f, Glow, 0.0, y, 0.05, face.len * 0.9, HALL_FH * 0.45, mul(pal.window_warm, 0.8)));
        }
        // The offices over it.
        let office_floors = pl.podium_floors.saturating_sub(3) as i32;
        if office_floors > 0 {
            band_windows(g, c, rng, face, podium_height(3), office_floors, OFFICE_FH);
        }
        grime(g, c, rng, face, halls_top, h, 1.4 + c.wear);
        // Blade signs down the podium, one per shop let (a pale board for
        // a few to let), Hong Kong style.
        let blades = ((face.len / 9.0) as usize).clamp(1, 6);
        for b in 0..blades {
            let t = -face.len * 0.5 + (b as f32 + 0.5) * face.len / blades as f32;
            if sign_k < let_shops.max(e.units.is_empty() as usize * 99) {
                vertical_sign(g, c, &mut detail, face, t, halls_top + 0.6, (h - halls_top - 2.0).min(9.0));
            } else if c.reach_out(face) > 0.4 {
                face.bx(g, f, Panel, t, halls_top + 4.0, 0.25, Vec3::new(0.18, 5.0, 1.2), [0.62, 0.60, 0.55]);
            }
            sign_k += 1;
        }
        if fi == front_i {
            // The grand entrance: a two-storey portal under a canopy, its
            // name on an LED band, lamps either side.
            let w = (face.len * 0.25).clamp(6.0, 18.0);
            face.decal(g, f, Glass, 0.0, halls_top * 0.5, -0.70, w, halls_top * 0.9, mul(pal.glass_dark, 0.8));
            window_light(g, c, |g| face.decal(g, f, Glow, 0.0, halls_top * 0.45, -0.68, w * 0.9, halls_top * 0.7, mul(pal.window_warm, 0.6)));
            if c.reach_out(face) > 1.5 {
                canopy(g, c, face, 0.0, halls_top - 0.6, w + 3.0, 2.4, mul(pal.steel, 0.6));
            }
            if face.len > 14.0 {
                led_screen(g, c, rng, face, -face.len * 0.32, halls_top + 3.0, (face.len * 0.14).min(10.0), 4.0);
            }
            bollards(g, c, face, 6, 1.2);
        } else if face.len > 18.0 && fi % 2 == 1 {
            // A car deck's mouth: dark, striped, a lit P.
            let w = 7.0;
            face.decal(g, f, Matte, face.len * 0.3, 2.3, -0.72, w, 4.2, [0.03, 0.03, 0.035]);
            face.decal(g, f, Glow, face.len * 0.3, 4.4, -0.70, w, 0.2, [1.6, 1.1, 0.2]);
            face.bx(g, f, Glow, face.len * 0.3 - w * 0.5 - 1.0, halls_top + 1.2, 0.1, Vec3::new(1.2, 1.2, 0.1), [0.2, 0.5, 1.8]);
        }
        // Corner neon.
        hk::corner_neon(g, c, &mut detail, face, hk::street_end(face), halls_top + 1.0, (h - halls_top - 3.0).max(3.0));
    }
    // The podium roof: a park round the towers' feet.
    let roof = offset_poly(site, 0.6);
    if roof.len() >= 3 {
        ground(g, f, Foliage, &roof, h + 0.05, mul(pal.foliage, 0.85));
        let parapet_col = mul(concrete, 0.95);
        parapet(g, c, site, h, 1.1, 0.25, WornConcrete, parapet_col);
        glow_outline(g, c, &offset_poly(site, 0.3), h + 1.15, 0.08, mul(pal.window_cold, 0.7));
        let towers: Vec<Vec<Vec2>> = pl.towers.iter().map(|t| offset_poly(&t.poly(), -2.5)).collect();
        let (lo, hi) = bounds(&roof);
        let step = 7.0;
        let mut n = 0;
        let mut v = lo.y + 4.0;
        while v < hi.y - 3.0 && n < 40 {
            let mut u = lo.x + 4.0 + ((v / step) as i32 % 2) as f32 * step * 0.5;
            while u < hi.x - 3.0 && n < 40 {
                let p = Vec2::new(u, v);
                if point_in_polygon(p, &roof) && distance_to_boundary(p, &roof) > 2.5 && !towers.iter().any(|t| point_in_polygon(p, t)) {
                    let k = nd_sim::hash::mix(m.seed, (n as u64) << 8);
                    if k % 3 != 0 {
                        raised_tree(g, c, p, h + 0.05, 4.0 + (k % 4) as f32, 2.4);
                    } else {
                        g.lod(Lod::Mid, |g| raised_planter(g, c, p, h + 0.05, 1.2, mul(concrete, 1.1)));
                    }
                    n += 1;
                }
                u += step;
            }
            v += step;
        }
    }
}

// ── the towers ───────────────────────────────────────────────────────────────

/// One tower from the podium roof at `y0`: sections of its own tones
/// between sky gardens, a side core, a roof of clutter. Returns its top.
#[allow(clippy::too_many_arguments)]
fn tower(g: &mut Geometry, c: &Ctx, _rng: &mut impl Rng, m: &MegaBlock, i: usize, t: &TowerSpot, y0: f32, fh: f32) -> f32 {
    let f = &c.frame;
    let pal = c.palette;
    let mut detail = nd_sim::rng_for(m.seed ^ 0x70E5, i as u64 + 1);
    let base = t.poly();
    let floors = t.floors as i32;
    let every = GARDEN_EVERY as i32;
    let mut y = y0;
    let mut k = 0;
    let mut section = 0usize;
    let ac = 0.35 + 0.3 * c.wear;
    while k < floors {
        // Floors to the next sky garden (one storey open to the air).
        let run = (every - 1).min(floors - k);
        // Each section jogs in or out a little: the stacked, mismatched look.
        let jog = [0.0f32, 0.6, -0.4, 0.9, 0.3][(nd_sim::hash::mix(m.seed, (i * 13 + section) as u64) % 5) as usize];
        let poly = offset_poly(&base, jog.max(0.0));
        let poly = if poly.len() >= 3 { poly } else { base.clone() };
        let wall = mul(tone(m, i, section), 0.9 - 0.25 * c.wear);
        let h = run as f32 * fh;
        let surf = if section % 2 == 0 { WornPlaster } else { WornConcrete };
        extrude(g, f, surf, &poly, y, y + h, wall);
        for face in c.faces(&poly) {
            if face.len < 3.0 {
                continue;
            }
            hk::flat_windows(g, c, &mut detail, &face, y, run, fh, 1.9, ac, 0.12, hk::FlatDress { more_ac: 0.15, ledge_below: false });
            hk::tile_bands(g, c, &face, y, run, fh, mul(wall, 1.12));
            if (section + i) % 3 == 0 {
                g.lod(Lod::Mid, |g| laundry_poles(g, c, &mut detail, &face, y, run.min(6), fh, 0.25));
            }
        }
        y += h;
        k += run;
        if k < floors {
            sky_garden(g, c, &mut detail, &poly, y, fh, wall);
            y += fh;
            k += 1;
        }
        section += 1;
    }
    // Blade signs on its lower floors, on the street side.
    let front = c.front_face(&base);
    if front.len > 6.0 {
        vertical_sign(g, c, &mut detail, &front, -front.len * 0.3, y0 + 1.0, (fh * 6.0).min(y - y0 - 1.0).max(3.0));
    }
    // A stair and lift core up one side of the first tower.
    if i == 0 {
        let _ = hk::side_core(g, c, &mut detail, &base, y0, y, fh, mul(tone(m, i, 9), 0.8));
    }
    // The roof: a parapet, tanks, aerials, shacks and the clutter; a red
    // light on the tall ones.
    let roof = offset_poly(&base, 0.2);
    let roof = if roof.len() >= 3 { roof } else { base.clone() };
    parapet(g, c, &roof, y, 1.0, 0.2, WornConcrete, mul(tone(m, i, 7), 0.8));
    antenna_forest(g, c, &mut detail, &offset_poly(&roof, 0.8), y, 0.7);
    hk::roof_clutter(g, c, &mut detail, &roof, y, 1.0);
    water_tank(g, f, t.p + Vec2::new(t.hw * 0.35, t.hd * 0.3), y, (t.hw * 0.18).clamp(0.8, 2.4), 3.2, Metal, mul(pal.steel, 0.7));
    if y > 60.0 {
        for q in [t.p + Vec2::new(-t.hw + 0.6, -t.hd + 0.6), t.p + Vec2::new(t.hw - 0.6, t.hd - 0.6)] {
            aviation_light_local(g, f, pal, q.x, y + 1.6, q.y, 0.8);
        }
    }
    y
}

/// A sky garden: one storey open to the air at `y` round a glazed core,
/// on columns, its slab edge lit, planters and small trees on it.
fn sky_garden(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y: f32, fh: f32, wall: [f32; 3]) {
    let f = &c.frame;
    let pal = c.palette;
    let core = offset_poly(poly, 2.4);
    if core.len() < 3 || nd_sim::zoning::polygon_area(&core).abs() < 6.0 {
        extrude(g, f, WornConcrete, poly, y, y + fh, wall);
        return;
    }
    extrude(g, f, Glass, &core, y, y + fh, mul(pal.glass_dark, 1.2));
    window_light(g, c, |g| {
        for face in c.faces(&core).iter().filter(|x| x.len > 2.0) {
            face.decal(g, f, Glow, 0.0, y + fh * 0.5, 0.05, face.len * 0.8, fh * 0.5, mul(pal.window_warm, 0.7));
        }
    });
    // The slab it stands on, its edge lit, and the columns at the corners.
    extrude(g, f, Concrete, poly, y - 0.05, y + 0.3, mul(wall, 0.85));
    glow_outline(g, c, &offset_poly(poly, -0.02), y + 0.1, 0.1, mul(pal.window_cold, 0.9));
    for p in poly {
        let q = p.lerp(poly_center(poly), 0.05);
        f.bx(g, Concrete, q.x, y + fh * 0.5, q.y, Vec3::new(0.7, fh, 0.7), mul(wall, 0.8));
    }
    let deck = offset_poly(poly, 0.4);
    if deck.len() >= 3 {
        ground(g, f, Foliage, &deck, y + 0.32, mul(pal.foliage, 0.9));
    }
    // Planters round the core with small trees in some.
    for (n, face) in c.faces(poly).iter().enumerate() {
        if face.len < 5.0 {
            continue;
        }
        let k = ((face.len / 5.5) as i32).clamp(1, 4);
        for j in 0..k {
            let t = -face.len * 0.5 + (j as f32 + 0.5) * face.len / k as f32;
            let p = face.at(t, -1.2);
            if (n + j as usize) % 2 == 0 {
                raised_tree(g, c, p, y + 0.32, (fh * 0.6).min(1.9), 1.0);
            } else {
                g.lod(Lod::Mid, |g| raised_planter(g, c, p, y + 0.32, 0.5, mul(wall, 1.1)));
            }
        }
        // A glass balustrade.
        g.lod(Lod::Mid, |g| face.decal(g, f, Glass, 0.0, y + 0.85, -0.1, face.len * 0.96, 1.0, mul(pal.glass_dark, 1.4)));
    }
    let _ = rng;
}

/// Bridges between neighbouring towers, at one or two heights.
fn bridges(g: &mut Geometry, c: &Ctx, m: &MegaBlock, pl: &Plan, fh: f32) {
    for i in 1..pl.towers.len() {
        let (a, b) = (pl.towers[i - 1], pl.towers[i]);
        let low = a.floors.min(b.floors) as f32 * fh;
        if low < fh * 8.0 {
            continue;
        }
        let d = (b.p - a.p).normalize_or_zero();
        if d == Vec2::ZERO {
            continue;
        }
        // From face to face: the square's edge along `d`.
        let ext = |t: &TowerSpot| (t.hw / d.x.abs().max(1e-3)).min(t.hd / d.y.abs().max(1e-3));
        let pa = a.p + d * (ext(&a) - 0.3);
        let pb = b.p - d * (ext(&b) - 0.3);
        if pa.distance(pb) < 3.0 || (pb - pa).dot(d) <= 0.0 {
            continue;
        }
        let n = 1 + (nd_sim::hash::mix(m.seed, 0xB21D ^ i as u64) % 2) as usize;
        for k in 0..n {
            let y = pl.podium_h + low * if k == 0 { 0.45 } else { 0.78 };
            sky_bridge(g, c, pa, pb, (y / fh).floor() * fh, 3.2, fh * 1.2);
        }
    }
}

// ── the building site ────────────────────────────────────────────────────────

/// The arcology going up, `progress` in `0..1`, in its four stages (the
/// quarters of `Construction`): piling the raft; the podium's slabs; the
/// towers' cores and frames; the cladding, lit flats and sky gardens.
pub fn site(g: &mut Geometry, c: &Ctx, e: &Extras, progress: f32) {
    let m = record(c, e);
    let pl = plan(c, &m);
    let f = &c.frame;
    let pal = c.palette;
    let p = progress.clamp(0.0, 1.0);
    let fh = flat_fh();
    let concrete = mul(pick(&mut nd_sim::rng_for(m.seed, 3), &pal.concrete), 0.95);
    // The ground and the hoarding round the block, lit along its top.
    ground(g, f, Dirt, &c.lot_poly, 0.03, pal.dirt);
    let hoard = offset_poly(&c.lot_poly, 0.3);
    for face in c.faces(if hoard.len() >= 3 { &hoard } else { &c.lot_poly }) {
        if face.len < 0.5 {
            continue;
        }
        face.bx(g, f, Wood, 0.0, 1.4, -0.05, Vec3::new(face.len, 2.8, 0.1), mul(pal.paving, 0.85));
        face.bx(g, f, Glow, 0.0, 2.85, -0.05, Vec3::new(face.len, 0.08, 0.12), mul(pal.window_warm, 0.8));
    }
    let raft = (p / 0.25).clamp(0.0, 1.0);
    extrude(g, f, Concrete, &pl.site, 0.0, 0.2 + 0.6 * raft, mul(concrete, 0.7));
    if p < 0.25 {
        // Piling rigs across the raft, more done as it goes.
        let (lo, hi) = bounds(&pl.site);
        let mut n = 0;
        let mut v = lo.y + 6.0;
        while v < hi.y - 4.0 {
            let mut u = lo.x + 6.0;
            while u < hi.x - 4.0 {
                let q = Vec2::new(u, v);
                if point_in_polygon(q, &pl.site) && distance_to_boundary(q, &pl.site) > 3.0 {
                    let h = nd_sim::hash::mix(m.seed, n);
                    if (h % 100) as f32 / 100.0 < raft + 0.15 {
                        f.bx(g, Concrete, q.x, 1.0, q.y, Vec3::new(0.9, 1.2, 0.9), mul(concrete, 0.8));
                    }
                    if n % 7 == 0 {
                        let paint = super::construction::crane_paint(m.seed ^ n);
                        f.bx(g, Matte, q.x, 9.0, q.y, Vec3::new(0.6, 18.0, 0.6), paint);
                        f.bx(g, Matte, q.x + 1.2, 1.5, q.y, Vec3::new(3.2, 2.2, 2.4), mul(paint, 0.8));
                    }
                    n += 1;
                }
                u += 9.0;
            }
            v += 9.0;
        }
        return;
    }
    // The podium's slabs up to where the work is, glazed once the towers go up.
    let pod_k = ((p - 0.25) / 0.25).clamp(0.0, 1.0);
    let pod_top = pl.podium_h * pod_k;
    if p >= 0.5 {
        extrude(g, f, WornConcrete, &pl.site, 0.0, pl.podium_h, mul(concrete, 0.85));
        for face in c.faces(&pl.site).iter().filter(|x| x.len > 4.0) {
            face.decal(g, f, Glass, 0.0, HALL_FH, 0.03, face.len * 0.9, HALL_FH * 1.4, mul(pal.glass_dark, 1.0));
        }
    } else {
        slab_floors(g, c, &pl.site, 0.0, pod_top.max(HALL_FH * 0.5), HALL_FH, concrete);
    }
    // The towers: cores lead, frames behind, then cladding and lights.
    let tow_k = ((p - 0.5) / 0.35).clamp(0.0, 1.0);
    let fit_k = ((p - 0.85) / 0.15).clamp(0.0, 1.0);
    for (i, t) in pl.towers.iter().enumerate() {
        let full = t.floors as f32 * fh;
        // The towers start one after another.
        let lag = (i as f32 * 0.12).min(0.4);
        let k = ((tow_k - lag) / (1.0 - lag)).clamp(0.0, 1.0);
        let core_h = full * k;
        let frame_h = (core_h - 4.0 * fh).max(0.0);
        let base = t.poly();
        let core = scale_poly(&base, 0.35);
        let y0 = pl.podium_h;
        if p >= 0.5 && core_h > 0.5 {
            extrude(g, f, Concrete, &core, y0, y0 + core_h, concrete);
        }
        if frame_h > 0.5 {
            slab_floors(g, c, &base, y0, y0 + frame_h, fh, concrete);
        }
        // Netting at the working floors.
        if frame_h > fh * 2.0 && k < 1.0 {
            extrude(g, f, Tarp, &offset_poly(&base, -0.4), y0 + frame_h - 2.0 * fh, y0 + frame_h + 1.0, [0.18, 0.32, 0.22]);
        }
        // The cladding climbing in the fit-out, the flats lit behind it.
        if fit_k > 0.0 {
            let clad = full * fit_k;
            extrude(g, f, WornPlaster, &offset_poly(&base, -0.1), y0, y0 + clad, mul(tone(&m, i, 0), 0.9));
            let mut rng = nd_sim::rng_for(m.seed ^ 0x51DE, i as u64);
            curtain_lights(g, c, &mut rng, &offset_poly(&base, -0.1), y0, (clad / fh) as i32, fh);
        }
        // A jump crane on each core until the fit-out is under way; a crane
        // on the podium for its slabs.
        let crane_on = if p < 0.5 { Some(y0 * pod_k) } else if fit_k < 0.6 { Some(y0 + core_h) } else { None };
        if let Some(top) = crane_on {
            crane(g, c, &m, i, poly_center(&core), top.max(4.0), (t.hw.max(t.hd) * 2.2).clamp(18.0, 45.0));
        }
    }
}

/// Bare floor plates every storey from `y0` to `y1` round `poly`, with
/// corner columns: the frame before the walls (solid every fourth).
fn slab_floors(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y0: f32, y1: f32, fh: f32, concrete: [f32; 3]) {
    let f = &c.frame;
    let floors = ((y1 - y0) / fh).floor() as i32;
    for k in 0..=floors {
        let y = y0 + k as f32 * fh;
        if k % 4 == 0 {
            extrude(g, f, Concrete, poly, y, y + 0.35, concrete);
        } else {
            loft(g, f, Concrete, poly, poly, y, y + 0.3, mul(concrete, 0.95));
        }
    }
    if y1 - y0 > 0.5 {
        for p in poly {
            let q = p.lerp(poly_center(poly), 0.06);
            f.bx(g, Concrete, q.x, (y0 + y1) * 0.5, q.y, Vec3::new(0.9, y1 - y0, 0.9), mul(concrete, 0.9));
        }
    }
}

/// A tower crane on a mast at `at` up to `top` and a jib of `jib` m, with
/// red lights at its head and tip.
fn crane(g: &mut Geometry, c: &Ctx, m: &MegaBlock, i: usize, at: Vec2, top: f32, jib: f32) {
    let f = &c.frame;
    let pal = c.palette;
    let paint = super::construction::crane_paint(m.seed ^ i as u64);
    let mast_top = top + 18.0;
    f.bx(g, Matte, at.x, mast_top * 0.5, at.y, Vec3::new(1.4, mast_top, 1.4), paint);
    let ang = nd_sim::towers::unit(m.seed ^ (0xC2A + i as u64)) * std::f32::consts::TAU;
    let d = Vec2::new(ang.cos(), ang.sin());
    // Keep the jib over the lot: swing it to the way with the most room.
    let d = [d, -d, Vec2::new(-d.y, d.x), Vec2::new(d.y, -d.x)].into_iter().max_by(|a, b| {
        let ra = (0..=6).filter(|k| c.inside_local(at + *a * jib * *k as f32 / 6.0)).count();
        let rb = (0..=6).filter(|k| c.inside_local(at + *b * jib * *k as f32 / 6.0)).count();
        ra.cmp(&rb)
    }).unwrap_or(d);
    let reach = (0..=12).take_while(|k| c.inside_local(at + d * jib * *k as f32 / 12.0)).count().saturating_sub(1) as f32 / 12.0 * jib;
    if reach > 4.0 {
        let mid = at + d * reach * 0.5;
        f.bx_yaw(g, Matte, mid.x, mast_top, mid.y, Vec3::new(reach, 1.1, 1.1), paint, d.y.atan2(d.x));
        let tip = at + d * reach * 0.95;
        aviation_light_local(g, f, pal, tip.x, mast_top + 0.8, tip.y, 0.5);
    }
    let back = at - d * 5.0;
    if c.inside_local(back) {
        f.bx_yaw(g, Concrete, back.x, mast_top - 0.8, back.y, Vec3::new(4.0, 1.8, 2.0), mul(pal.concrete(0), 0.8), d.y.atan2(d.x));
    }
    aviation_light_local(g, f, pal, at.x, mast_top + 2.0, at.y, 0.6);
}

/// A tree standing on a roof or terrace at height `y`.
fn raised_tree(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, h: f32, canopy: f32) {
    let w = c.frame.to_world(p.x, p.y);
    tree_kind(g, w, y, TreeKind::Round, h, canopy, c.palette.bark, c.palette.foliage);
}

/// A concrete planter of shrubs on a roof or terrace at height `y`.
fn raised_planter(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, r: f32, tub: [f32; 3]) {
    let f = &c.frame;
    f.bx(g, Concrete, p.x, y + 0.3, p.y, Vec3::new(r * 2.0, 0.6, r * 2.0), tub);
    f.bx(g, Foliage, p.x, y + 0.75, p.y, Vec3::new(r * 1.7, 0.4, r * 1.7), c.palette.foliage);
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use nd_sim::megablock::MegaBlock;

    fn block_lot() -> Lot {
        super::super::sweep_tests::lot_of(&super::super::sweep_tests::rect(96.0, 92.0), 0.37, Vec2::new(310.0, -170.0))
    }

    /// The arcology on a real block: within its triangle budget, its towers
    /// reaching their storeys, lit flats, the logo on the crown; at every
    /// stage of its site, on the lot, cranes and all.
    #[test]
    fn the_arcology_draws_within_budget_at_every_stage() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lot = block_lot();
        for seed in [3u64, 11, 29] {
            let m = MegaBlock::plan(seed, Default::default(), lot.area(), 44, 0);
            let floors = m.total_floors();
            let brand = Some(super::super::sweep_tests::hq());
            for progress in [None, Some(0.05), Some(0.3), Some(0.55), Some(0.8), Some(0.95)] {
                let ex = Extras { megablock: Some(m), form: Some(nd_sim::estate::Form::MegaBlock), construction: progress, dynamic_lights: true, brand, ..Extras::default() };
                let g = generate_building(&lot, LandUse::Residential, floors, Tier::Mid, 0.8, seed, &rules, &pal, &ex);
                let faults = super::super::sweep_tests::faults(&g, &lot, Style::MegaBlock);
                assert!(faults.is_empty(), "{seed} {progress:?}: {faults:?}");
                let top = g.parts.iter().flat_map(|x| x.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
                let tall = super::podium_height(m.podium_floors) + m.tower_floors as f32 * super::flat_fh();
                assert!(top < tall + 40.0, "{seed} {progress:?}: top {top} of {tall}");
                if progress.is_none() {
                    assert!(top > tall * 0.95, "the tallest tower stands its height: {top} of {tall}");
                    let (base, all) = (g.base_triangle_count(), g.triangle_count());
                    assert!(base < 150_000 && all < 450_000, "{seed}: {base} base, {all} triangles");
                    assert!(g.get(Surface::Glow).positions.len() > 2000, "lit windows and signs");
                } else {
                    assert!(g.triangle_count() < 250_000, "{seed} {progress:?}: {} triangles", g.triangle_count());
                }
            }
        }
    }
}
