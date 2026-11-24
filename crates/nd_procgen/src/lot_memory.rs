//! [r11-lot-memory] The city's past drawn on it, from the lot history
//! (`nd_sim::lot_memory`):
//!
//! - **Wall ghosts** ([`wall_ghosts`]): where a building came down next to
//!   one that stands, the old house's outline stays on its neighbour's
//!   wall: a patch of old plaster up to its roof line (a dark flashing line
//!   along a gable, a mono-pitch or a flat roof with a lower back
//!   extension), the floor lines, scraps of each room's wallpaper, the
//!   chimney breast and its fireplaces, a row of tile scars under a pitched
//!   roof. Only on a wall that really stood against the old ground: one
//!   facing it, within [`ABUT`] of the shared line, reaching the ground,
//!   and only as high as that wall is. It fades over weeks to a faint scar
//!   (`WALL_STAGES`).
//! - **Old slabs** ([`slab`]): on a lot left vacant, the old floor slab in
//!   cracked squares, its footings broken along the outline, a patch of
//!   kitchen tiles and weeds in the cracks; the weeds take a little more
//!   each week until it is gone (`SLAB_STAGES`) or something is built.
//! - **Painted-over signs** ([`GhostSign`], drawn with the building by
//!   `buildings::ghost_signs`): a shop that changed trade or shut keeps its
//!   old name faintly through the paint above the shopfront, and
//!   sometimes a bricked-up door or a shopfront filled in with blockwork.
//!
//! Everything is procedural, from the trace's seed and the drawn geometry:
//! no textures, no random draws, the same history draws the same. Drawn as
//! part of the building chunks and the vacant-lot dressing, so it costs
//! nothing a frame; the renderer redraws a chunk only when a trace is new
//! or its fade stage turns over.

use crate::buildings::parts::{distance_to_boundary, offset_poly, stencil};
use crate::color::{mix, mul, rgba};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::hash::{mix2, unit};
use nd_sim::lot_memory::{stage, Trace, TraceKind, SIGN_STAGES, SLAB_STAGES, WALL_STAGES};
use nd_sim::zoning::{point_in_polygon, polygon_area};
use nd_sim::{BuildingId, LandUse};

/// A wall stands against the old ground when its face is within this of
/// the shared line (the usual setback is 1.5 m), m.
pub const ABUT: f32 = 2.0;
/// Width of a column of the ghost, m.
const STEP: f32 = 0.5;
/// The wall surfaces a ghost is painted on (not glass).
const WALLS: [Surface; 9] = [Concrete, Plaster, Brick, Stone, Panel, WornConcrete, WornPlaster, WornBrick, WornPanel];
/// Old plaster, limewash and render.
const PLASTERS: [[f32; 3]; 5] = [[0.66, 0.62, 0.55], [0.58, 0.55, 0.50], [0.62, 0.57, 0.47], [0.52, 0.52, 0.50], [0.60, 0.52, 0.46]];
/// Faded wallpaper and room paint: rose, sage, cream, duck egg, ochre,
/// lilac, nicotine.
const WALLPAPERS: [[f32; 3]; 7] = [[0.62, 0.42, 0.42], [0.45, 0.55, 0.44], [0.70, 0.66, 0.52], [0.44, 0.56, 0.60], [0.66, 0.54, 0.30], [0.52, 0.46, 0.60], [0.58, 0.50, 0.36]];
/// Tar and old flashing along a roof line.
const TAR: [f32; 3] = [0.07, 0.065, 0.06];
/// Soot of a chimney breast.
const SOOT: [f32; 3] = [0.14, 0.12, 0.11];

use nd_sim::hash::mix2_01 as h01;

fn at3(c: &[[f32; 3]], i: u64) -> [f32; 3] {
    c[(i % c.len() as u64) as usize]
}

/// Storey height the old building was drawn with (`BuildingRules`).
fn storey(lu: LandUse) -> f32 {
    match lu {
        LandUse::Commercial => 3.5,
        LandUse::Industrial => 5.0,
        LandUse::Civic => 4.2,
        _ => 3.1,
    }
}

/// The outline counter-clockwise.
fn ccw(poly: &[Vec2]) -> Vec<Vec2> {
    let mut p = poly.to_vec();
    if polygon_area(&p) < 0.0 {
        p.reverse();
    }
    p
}

/// A wall of a standing building that faces a trace's edge: its plane's
/// offset from the edge (`s`, along the normal into the old ground, so
/// negative), its colour, and per column of [`STEP`] along the edge the
/// top of the wall there (None where no wall reaches the ground).
#[derive(Clone, Debug)]
pub struct Wall {
    pub s: f32,
    pub color: [f32; 3],
    pub tops: Vec<Option<f32>>,
}

/// The wall in `g` (world x/z, heights from its pad) that faces the edge
/// `a`→`a + d * len` from outside, `n` pointing from the wall into the old
/// ground: the plane with the most wall on it within [`ABUT`] of the line.
pub fn find_wall(g: &Geometry, a: Vec2, d: Vec2, n: Vec2, len: f32) -> Option<Wall> {
    // (plane bin, area, triangle in (t, y), colour)
    let mut tris: Vec<(i32, f32, [Vec2; 3], [f32; 3])> = Vec::new();
    for s in WALLS {
        let Some(m) = g.parts.get(s.index()) else { continue };
        for tri in m.indices.chunks_exact(3) {
            let p = [0, 1, 2].map(|k| Vec3::from(m.positions[tri[k] as usize]));
            let nn = (p[1] - p[0]).cross(p[2] - p[0]);
            let area = nn.length() * 0.5;
            if area < 1e-3 {
                continue;
            }
            let nn = nn / (2.0 * area);
            // Upright, facing the old ground.
            if nn.y.abs() > 0.15 || Vec2::new(nn.x, nn.z).dot(n) < 0.95 {
                continue;
            }
            let sd = p.map(|q| (Vec2::new(q.x, q.z) - a).dot(n));
            let s_mid = (sd[0] + sd[1] + sd[2]) / 3.0;
            if !(-ABUT..=0.3).contains(&s_mid) {
                continue;
            }
            let ty = p.map(|q| Vec2::new((Vec2::new(q.x, q.z) - a).dot(d), q.y));
            let (t0, t1) = (ty.iter().map(|v| v.x).fold(f32::MAX, f32::min), ty.iter().map(|v| v.x).fold(f32::MIN, f32::max));
            if t1 < 0.0 || t0 > len {
                continue;
            }
            let col = m.colors.get(tri[0] as usize).map_or([0.5; 3], |c| [c[0], c[1], c[2]]);
            tris.push(((s_mid * 10.0).round() as i32, area, ty, col));
        }
    }
    // The plane with the most wall.
    let mut bins: Vec<(i32, f32)> = Vec::new();
    for (b, area, _, _) in &tris {
        match bins.iter_mut().find(|(k, _)| k == b) {
            Some(e) => e.1 += area,
            None => bins.push((*b, *area)),
        }
    }
    let best = bins.iter().max_by(|x, y| x.1.total_cmp(&y.1).then(y.0.cmp(&x.0)))?.0;
    let mine: Vec<&(i32, f32, [Vec2; 3], [f32; 3])> = tris.iter().filter(|t| (t.0 - best).abs() <= 1).collect();
    let total: f32 = mine.iter().map(|t| t.1).sum();
    if total < 2.0 {
        return None;
    }
    let mut color = [0.0f32; 3];
    for t in &mine {
        for k in 0..3 {
            color[k] += t.3[k] * t.1 / total;
        }
    }
    let cols = (len / STEP).ceil().max(1.0) as usize;
    let mut tops = Vec::with_capacity(cols + 1);
    for i in 0..=cols {
        let tc = (i as f32 * STEP).min(len);
        // The wall's span up the vertical line at `tc`.
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for (_, _, ty, _) in &mine {
            for k in 0..3 {
                let (u, v) = (ty[k], ty[(k + 1) % 3]);
                let (a0, a1) = (u.x.min(v.x), u.x.max(v.x));
                if tc < a0 - 0.02 || tc > a1 + 0.02 {
                    continue;
                }
                let y = if (v.x - u.x).abs() < 1e-4 { u.y.max(v.y) } else { u.y + (v.y - u.y) * ((tc - u.x) / (v.x - u.x)).clamp(0.0, 1.0) };
                let y0 = if (v.x - u.x).abs() < 1e-4 { u.y.min(v.y) } else { y };
                lo = lo.min(y0);
                hi = hi.max(y);
            }
        }
        tops.push((lo < 1.5 && hi > 2.5).then_some(hi));
    }
    Some(Wall { s: best as f32 * 0.1, color, tops })
}

/// The roof of the old building along the wall: the height of its outline
/// at `u` (`0..1` from one end of its side to the other).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Roof {
    Flat { back: Option<(f32, f32)> },
    Gable { rise: f32 },
    Mono { rise: f32, up_at_end: bool },
}

impl Roof {
    fn of(t: &Trace, span: f32) -> Roof {
        let r = h01(t.seed, 0x900F);
        let low = t.floors <= 4 && t.land_use != LandUse::Industrial;
        if low && r < 0.5 {
            return Roof::Gable { rise: (span * 0.5 * 0.65).min(4.5) };
        }
        if low && r < 0.7 {
            return Roof::Mono { rise: (span * 0.25).min(2.5), up_at_end: h01(t.seed, 0x900E) < 0.5 };
        }
        // A lower back extension on a third of flat roofs.
        let back = (h01(t.seed, 0x900D) < 0.35 && t.floors >= 2).then(|| (if h01(t.seed, 0x900C) < 0.5 { 0.0 } else { 0.66 }, storey(t.land_use)));
        Roof::Flat { back }
    }

    fn pitched(self) -> bool {
        !matches!(self, Roof::Flat { .. })
    }

    /// Height of the outline at `u`, over eaves `h`.
    fn at(self, u: f32, h: f32) -> f32 {
        match self {
            Roof::Flat { back: Some((from, drop)) } if u >= from && u <= from + 0.34 => (h - drop).max(storey(LandUse::Residential)),
            Roof::Flat { .. } => h,
            Roof::Gable { rise } => h + rise * (1.0 - (2.0 * u - 1.0).abs()),
            Roof::Mono { rise, up_at_end } => h + rise * if up_at_end { u } else { 1.0 - u },
        }
    }
}

/// Paint the ghost of trace `t` on `wall`, which faces its edge `a`→`a +
/// d * len` (`n` into the old ground), at fade `stage` (`0..=WALL_STAGES`).
#[allow(clippy::too_many_arguments)]
fn paint_ghost(out: &mut Geometry, wall: &Wall, t: &Trace, edge: usize, a: Vec2, d: Vec2, n: Vec2, len: f32, stage: u8) {
    let seed = mix2(t.seed, edge as u64);
    let fh = storey(t.land_use);
    let floors = t.floors.clamp(1, 20);
    let eaves = (floors as f32 * fh).min(62.0);
    // Its own side was set back a little from each end of the edge.
    let (ta, tb) = (0.3 + h01(seed, 1) * 0.9, len - 0.3 - h01(seed, 2) * 0.9);
    if tb - ta < 2.5 {
        return;
    }
    let roof = Roof::of(t, tb - ta);
    // Strength: fresh, fading by the week to a faint scar.
    let k = 1.0 - 0.78 * (stage.min(WALL_STAGES) as f32 / WALL_STAGES as f32);
    let plaster = mix(wall.color, at3(&PLASTERS, mix2(seed, 3)), 0.8 * k);
    let tar = mix(wall.color, TAR, 0.85 * k);
    let facing = Vec3::new(n.x, 0.0, n.y);
    let p3 = |tt: f32, y: f32, off: f32| {
        let p = a + d * tt + n * (wall.s + off);
        Vec3::new(p.x, y, p.y)
    };
    let top_at = |tt: f32| -> Option<f32> {
        let i = (tt / STEP).round() as usize;
        wall.tops.get(i.min(wall.tops.len().saturating_sub(1))).copied().flatten()
    };
    // Sample points along its side: every STEP and its two ends.
    let mut ts: Vec<f32> = vec![ta];
    let mut x = (ta / STEP).floor() * STEP + STEP;
    while x < tb - 0.05 {
        ts.push(x);
        x += STEP;
    }
    ts.push(tb);
    // The outline: the old roof, held under the wall's own top; the roof
    // line shows only where the wall rises above it.
    let line = 0.22;
    let prof: Vec<Option<(f32, bool)>> = ts
        .iter()
        .map(|&tt| {
            let top = top_at(tt)? - 0.25;
            let r = roof.at((tt - ta) / (tb - ta), eaves);
            let y = r.min(top);
            (y >= 1.5).then_some((y, r <= top))
        })
        .collect();
    let mut any = false;
    for i in 0..ts.len().saturating_sub(1) {
        let (Some((y0, l0)), Some((y1, l1))) = (prof[i], prof[i + 1]) else { continue };
        let (t0, t1) = (ts[i], ts[i + 1]);
        any = true;
        // The patch of old plaster.
        out.part(WornPlaster).quad_facing([p3(t0, 0.15, 0.03), p3(t1, 0.15, 0.03), p3(t1, y1 - line, 0.03), p3(t0, y0 - line, 0.03)], facing, rgba(plaster));
        // The roof line (flashing, tar): dark where the roof met the wall.
        let c = if l0 && l1 { tar } else { mix(plaster, tar, 0.25) };
        out.part(Matte).quad_facing([p3(t0, y0 - line, 0.05), p3(t1, y1 - line, 0.05), p3(t1, y1, 0.05), p3(t0, y0, 0.05)], facing, rgba(c));
        // Floor lines: where the joists sat.
        for f in 1..floors {
            let y = f as f32 * fh;
            if y + 0.35 < y0.min(y1) {
                out.part(Matte).quad_facing([p3(t0, y - 0.08, 0.045), p3(t1, y - 0.08, 0.045), p3(t1, y + 0.08, 0.045), p3(t0, y + 0.08, 0.045)], facing, rgba(mix(plaster, TAR, 0.45 * k)));
            }
        }
    }
    if !any {
        return;
    }
    let low = |tt: f32| prof[ts.iter().position(|x| *x >= tt).unwrap_or(ts.len() - 1)].map_or(0.0, |p| p.0);
    // Scraps of each room's wallpaper, floor by floor: two or three rooms
    // deep, some stripped to the plaster.
    let rooms = if tb - ta > 9.0 { 3 } else { 2 };
    let rw = (tb - ta) / rooms as f32;
    for f in 0..floors as u32 {
        for r in 0..rooms {
            let hs = mix2(seed, 100 + f as u64 * 8 + r as u64);
            if unit(hs) < 0.3 {
                continue;
            }
            let (r0, r1) = (ta + rw * r as f32 + 0.25, ta + rw * (r + 1) as f32 - 0.25);
            let (y0, y1) = (f as f32 * fh + 0.3, (f + 1) as f32 * fh - 0.3);
            if r1 - r0 < 0.8 || low(r0).min(low(r1)) < y1 + 0.3 || low((r0 + r1) * 0.5) < y1 + 0.3 {
                continue;
            }
            // Torn: strips of paper, each torn off at its own height, some
            // gone; grimy (on the weathered plaster).
            let paper = mix(plaster, at3(&WALLPAPERS, hs >> 7), 0.68 * k);
            let strips = 3 + (hs >> 13) % 3;
            let sw = (r1 - r0) / strips as f32;
            for i in 0..strips {
                let hi = mix2(hs, 0x57 + i);
                if unit(hi) < 0.15 {
                    continue;
                }
                let (a0, a1) = (r0 + sw * i as f32, r0 + sw * (i + 1) as f32);
                let (b0, b1) = (y0 + 0.5 * unit(hi >> 8), y1 - 0.6 * unit(hi >> 16));
                let col = mul(paper, 0.92 + 0.12 * unit(hi >> 24));
                out.part(WornPlaster).quad_facing([p3(a0, b0, 0.04), p3(a1, b0, 0.04), p3(a1, b1, 0.04), p3(a0, b1, 0.04)], facing, rgba(col));
            }
            // A paler square where a picture or a wardrobe stood.
            if unit(hs >> 3) < 0.25 {
                let (cx, cy, w, h) = (r0 + (r1 - r0) * (0.3 + 0.4 * unit(hs >> 11)), y0 + (y1 - y0) * 0.55, 0.7, 0.6);
                out.part(Matte).quad_facing([p3(cx - w * 0.5, cy - h * 0.5, 0.042), p3(cx + w * 0.5, cy - h * 0.5, 0.042), p3(cx + w * 0.5, cy + h * 0.5, 0.042), p3(cx - w * 0.5, cy + h * 0.5, 0.042)], facing, rgba(mix(paper, [0.8, 0.78, 0.72], 0.3 * k)));
            }
        }
    }
    // The stairs: a dark diagonal up one room from floor to floor.
    if floors >= 2 && h01(seed, 9) < 0.55 {
        let r = ((mix2(seed, 10) % rooms as u64) as f32).min(rooms as f32 - 1.0);
        let s0 = ta + rw * r + 0.3;
        let run = (rw - 0.6).min(fh * 1.2);
        for f in 0..(floors as u32 - 1) {
            let (ya, yb) = (f as f32 * fh + 0.15, (f + 1) as f32 * fh);
            if run > 1.5 && low(s0).min(low(s0 + run)) > yb + 0.4 {
                out.part(Matte).quad_facing([p3(s0, ya, 0.05), p3(s0 + run, yb, 0.05), p3(s0 + run, yb + 0.3, 0.05), p3(s0, ya + 0.3, 0.05)], facing, rgba(mix(plaster, TAR, 0.6 * k)));
            }
        }
    }
    // The chimney breast up the wall of an old house, with a fireplace on
    // each floor, its stack's outline above the roof line.
    if floors <= 6 && h01(seed, 5) < 0.65 {
        let tc = if roof.pitched() && matches!(roof, Roof::Gable { .. }) { (ta + tb) * 0.5 } else { ta + (tb - ta) * (0.2 + 0.6 * h01(seed, 6)) };
        let w = 0.9;
        if let Some((yc, _)) = prof[ts.iter().position(|x| *x >= tc).unwrap_or(0)] {
            let stack = (yc + 1.1).min(top_at(tc).map_or(yc, |t| t - 0.25));
            let soot = mix(plaster, SOOT, 0.7 * k);
            out.part(Matte).quad_facing([p3(tc - w * 0.5, 0.15, 0.055), p3(tc + w * 0.5, 0.15, 0.055), p3(tc + w * 0.5, stack, 0.055), p3(tc - w * 0.5, stack, 0.055)], facing, rgba(soot));
            for f in 0..floors as u32 {
                let y = f as f32 * fh + 0.25;
                if y + 0.8 < yc {
                    out.part(Matte).quad_facing([p3(tc - 0.3, y, 0.065), p3(tc + 0.3, y, 0.065), p3(tc + 0.3, y + 0.7, 0.065), p3(tc - 0.3, y + 0.7, 0.065)], facing, rgba(mix(plaster, TAR, 0.9 * k)));
                }
            }
        }
    }
    // Tile scars: a row of dashes under a pitched roof line.
    if roof.pitched() {
        let mut tt = ta + 0.3;
        while tt < tb - 0.3 {
            if let Some((y, true)) = prof[ts.iter().position(|x| *x >= tt).unwrap_or(0)] {
                let y = y - line - 0.18;
                if y > eaves - 0.1 {
                    out.part(Matte).quad_facing([p3(tt - 0.15, y - 0.05, 0.06), p3(tt + 0.15, y - 0.05, 0.06), p3(tt + 0.15, y + 0.05, 0.06), p3(tt - 0.15, y + 0.05, 0.06)], facing, rgba(mix(plaster, TAR, 0.55 * k)));
                }
            }
            tt += 0.45;
        }
    }
}

/// The ghosts of the buildings in `traces` that stood against the walls in
/// `g` (a standing building's geometry: world x/z, heights from its pad),
/// as of `tick`. Only on a wall facing the old ground within [`ABUT`] of
/// its edge (see [`find_wall`]), only as high as that wall.
pub fn wall_ghosts(g: &Geometry, traces: &[&Trace], tick: u64) -> Geometry {
    let mut out = Geometry::default();
    for t in traces {
        if t.kind != TraceKind::Demolished || t.floors == 0 || t.outline.len() < 3 {
            continue;
        }
        let st = stage(t, tick, WALL_STAGES);
        let poly = ccw(&t.outline);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let len = a.distance(b);
            if len < 3.0 {
                continue;
            }
            let d = (b - a) / len;
            // Counter-clockwise: the old ground is on the left.
            let n = Vec2::new(-d.y, d.x);
            if let Some(w) = find_wall(g, a, d, n, len) {
                paint_ghost(&mut out, &w, t, i, a, d, n, len, st);
            }
        }
    }
    out
}

/// Traces near building `id` whose ghosts it may carry: buildings gone from
/// the ground around it, not from under it.
pub fn ghost_traces<'a>(world: &'a nd_sim::world::World, id: BuildingId) -> Vec<&'a Trace> {
    let Some(b) = world.buildings.get(id) else { return Vec::new() };
    if world.lot_memory.is_empty() || b.construction.is_some() || b.demolition.is_some() || b.is_tenant() {
        return Vec::new();
    }
    let Some(fp) = world.footprint(id) else { return Vec::new() };
    let (lo, hi) = fp.polygon.iter().fold((fp.center, fp.center), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let r = Vec2::splat(ABUT + 0.5);
    world.lot_memory.near(lo - r, hi + r).into_iter().filter(|t| t.kind == TraceKind::Demolished && t.floors > 0 && t.outline.len() >= 3 && !fp.contains(t.at)).collect()
}

/// The old slab of trace `t` on vacant lot `lot_poly`, at fade `stage`
/// (`0..SLAB_STAGES`; nothing at the last): cracked squares of concrete,
/// broken footings along its outline, a patch of tiles, weeds in the
/// cracks, all inside the lot.
pub fn slab(out: &mut Geometry, lot_poly: &[Vec2], t: &Trace, stage: u8, pal: &Palette) {
    if t.kind != TraceKind::Demolished || t.floors == 0 || t.outline.len() < 3 || stage >= SLAB_STAGES {
        return;
    }
    let lot = ccw(lot_poly);
    let poly = offset_poly(&ccw(&t.outline), 1.2);
    if poly.len() < 3 || polygon_area(&poly).abs() < 12.0 {
        return;
    }
    // Weeds over the slab: none at first, nearly all by the last stage.
    let cover = stage as f32 / SLAB_STAGES as f32;
    let seed = t.seed ^ 0x51AB;
    let d = (poly[1] - poly[0]).normalize_or_zero();
    let (d, n) = if d == Vec2::ZERO { (Vec2::X, Vec2::Y) } else { (d, Vec2::new(-d.y, d.x)) };
    let o = poly[0];
    let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in &poly {
        let q = *p - o;
        (u0, u1, v0, v1) = (u0.min(q.dot(d)), u1.max(q.dot(d)), v0.min(q.dot(n)), v1.max(q.dot(n)));
    }
    let cell = 1.6;
    let base = pal.concrete.first().copied().unwrap_or([0.45, 0.45, 0.44]);
    // Darker than the ground round it, so the cracks read.
    let slab_col = mix(mul(base, 0.62), [0.30, 0.30, 0.29], 0.3);
    let inside = |p: Vec2, m: f32| point_in_polygon(p, &poly) && point_in_polygon(p, &lot) && distance_to_boundary(p, &lot) > m;
    // A patch of kitchen or bathroom tiles: a few cells across.
    let (tile_i, tile_j) = (((u1 - u0) / cell * h01(seed, 7)) as i32, ((v1 - v0) / cell * h01(seed, 8)) as i32);
    let tiles = [at3(&[[0.72, 0.70, 0.64], [0.66, 0.64, 0.60]], seed), at3(&[[0.22, 0.38, 0.40], [0.42, 0.16, 0.14], [0.14, 0.14, 0.16], [0.52, 0.44, 0.22]], seed >> 5)];
    let mut i = 0;
    let mut u = u0;
    while u < u1 {
        let mut j = 0;
        let mut v = v0;
        while v < v1 {
            let c = o + d * (u + cell * 0.5) + n * (v + cell * 0.5);
            let hc = mix2(seed, ((i as u64) << 20) ^ j as u64);
            if inside(c, 0.3) && unit(hc) >= cover * 0.95 {
                // A square of slab, a crack's width in from its neighbours,
                // each its own stain.
                let half = cell * 0.5 - 0.06;
                let q = [c - d * half - n * half, c + d * half - n * half, c + d * half + n * half, c - d * half + n * half];
                let col = mul(slab_col, 0.82 + 0.3 * unit(hc >> 9));
                out.part(WornConcrete).ground_polygon(&q, 0.06, rgba(col));
                let tiled = (i - tile_i).abs() <= 1 && (j - tile_j).abs() <= 1;
                if tiled {
                    for (ti, tu) in [-0.6f32, -0.2, 0.2, 0.6].iter().enumerate() {
                        for (tj, tv) in [-0.6f32, -0.2, 0.2, 0.6].iter().enumerate() {
                            if unit(mix2(hc, (ti * 4 + tj) as u64)) < 0.18 + cover {
                                continue;
                            }
                            let p = c + d * *tu + n * *tv;
                            let h = 0.18;
                            let q = [p - d * h - n * h, p + d * h - n * h, p + d * h + n * h, p - d * h + n * h];
                            out.part(Matte).ground_polygon(&q, 0.072, rgba(tiles[(ti + tj) % 2]));
                        }
                    }
                }
            } else if inside(c, 0.3) && unit(hc >> 17) < 0.6 {
                // Weeds where the slab has gone.
                let s = 0.35 + 0.5 * unit(hc >> 23);
                out.part(Foliage).boxed(Vec3::new(c.x, s * 0.25, c.y), Vec3::new(s, s * 0.5, s * 0.8), unit(hc >> 29) * 6.28, rgba(mul(if unit(hc >> 31) < 0.5 { pal.foliage } else { pal.foliage_dry }, 0.75 + 0.3 * unit(hc >> 35))));
            }
            // Weeds in the cracks, more each week.
            if inside(c, 0.3) && unit(hc >> 41) < 0.12 + cover * 0.5 {
                let p = c + d * (cell * 0.5) + n * (cell * (unit(hc >> 45) - 0.5));
                if inside(p, 0.2) {
                    let s = 0.2 + 0.3 * unit(hc >> 49);
                    out.part(Foliage).boxed(Vec3::new(p.x, 0.06 + s * 0.2, p.y), Vec3::new(s, s * 0.4, s), unit(hc >> 51) * 6.28, rgba(mul(pal.foliage, 0.7 + 0.3 * unit(hc >> 53))));
                }
            }
            v += cell;
            j += 1;
        }
        u += cell;
        i += 1;
    }
    // The footings along the outline, broken where the weeds have them.
    let foot = mul(base, 0.5);
    for e in 0..poly.len() {
        let (a, b) = (poly[e], poly[(e + 1) % poly.len()]);
        let len = a.distance(b);
        let dd = (b - a) / len.max(1e-3);
        let mut s = 0.0;
        let mut k = 0u64;
        while s < len - 0.2 {
            let l = (len - s).min(1.6);
            let m = a + dd * (s + l * 0.5);
            if inside(m, 0.25) && unit(mix2(seed, 0xF00 + ((e as u64) << 16) + k)) >= cover * 0.8 {
                out.part(WornConcrete).boxed(Vec3::new(m.x, 0.13, m.y), Vec3::new(l - 0.12, 0.26, 0.32), dd.y.atan2(dd.x), rgba(foot));
            }
            s += 1.6;
            k += 1;
        }
    }
}

/// [r11-lot-memory] The newest trace on a vacant lot that leaves a slab,
/// and its stage now (None: nothing to draw).
pub fn slab_trace<'a>(world: &'a nd_sim::world::World, lot: &nd_sim::Lot) -> Option<(&'a Trace, u8)> {
    if world.lot_memory.is_empty() {
        return None;
    }
    let t = world.lot_memory.on_lot(lot).into_iter().find(|t| t.kind == TraceKind::Demolished)?;
    let s = stage(t, world.clock.tick, SLAB_STAGES);
    (t.floors > 0 && t.outline.len() >= 3 && s < SLAB_STAGES).then_some((t, s))
}

/// An old sign painted over on a shop that changed (see the module docs):
/// over shopfront `unit` (None: the building's own front), its old name
/// (or trade) in faint letters through the paint.
#[derive(Clone, Debug, PartialEq)]
pub struct GhostSign {
    pub unit: Option<u8>,
    /// Upper-case letters, digits and spaces.
    pub text: String,
    pub seed: u64,
    /// `0..=SIGN_STAGES`: the letters fade, then stay faint.
    pub stage: u8,
    /// It shut, or became something that is not a shop: its shopfront may
    /// be filled in.
    pub shut: bool,
}

/// Letters a ghost sign can show: the stencil's, 14 at most.
pub fn sign_text(s: &str) -> String {
    let t: String = s.to_ascii_uppercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' }).collect();
    // Whole words while they fit; a first word too long is cut.
    let mut out = String::new();
    for w in t.split_whitespace() {
        if out.is_empty() {
            out = w.chars().take(14).collect();
        } else if out.len() + 1 + w.len() <= 14 {
            out = format!("{out} {w}");
        } else {
            break;
        }
    }
    out
}

/// The painted-over signs of building `id`: the newest change of each of
/// its shopfronts and of its own trade, two at most.
pub fn ghost_signs(world: &nd_sim::world::World, id: BuildingId) -> Vec<GhostSign> {
    let Some(b) = world.buildings.get(id) else { return Vec::new() };
    if world.lot_memory.is_empty() || b.construction.is_some() || b.demolition.is_some() {
        return Vec::new();
    }
    let Some(lot) = world.lots.get(b.lot) else { return Vec::new() };
    let mut out: Vec<GhostSign> = Vec::new();
    for t in world.lot_memory.on_lot(lot) {
        if t.kind != TraceKind::Changed || t.building != id || out.iter().any(|g| g.unit == t.unit) || t.tick < b.built_tick {
            continue;
        }
        let text = if t.name.is_empty() { t.business.map(|k| k.label().to_string()).unwrap_or_default() } else { t.name.clone() };
        let shut = t.now.is_none_or(|k| !nd_sim::world::fits_shopfront(k));
        out.push(GhostSign { unit: t.unit, text: sign_text(&text), seed: t.seed, stage: stage(t, world.clock.tick, SIGN_STAGES), shut });
        if out.len() >= 2 {
            break;
        }
    }
    out
}

/// Stencil `text` faintly into `g` as the ghost lettering of a sign.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ghost_letters(g: &mut Geometry, centre: Vec3, along: Vec3, facing: Vec3, h: f32, text: &str, color: [f32; 3], seed: u64, gaps: f32) {
    stencil(g.part(Matte), centre, along, facing, h, text.as_bytes(), rgba(color), move |k| unit(mix2(seed, k as u64 ^ 0x6057)) < gaps);
}

#[cfg(test)]
#[path = "lot_memory_tests.rs"]
mod tests;
