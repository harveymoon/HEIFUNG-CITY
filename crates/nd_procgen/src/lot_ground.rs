//! [r15-hillside] Lots cut out of the coarse ground mesh, and their sides.
//!
//! The renderer draws the landscape's ground as a height field of 8 m cells
//! (`terrain::LandscapeTiles`), straight from vertex to vertex. A lot is
//! levelled to a flat pad (`nd_sim::landscape::Ground`), but a cell that
//! straddles its edge runs from the pad to the hill beyond: on a slope the
//! cells rose through the lot's floor near its edges, or hung under it,
//! and next to a walled terrace they ran up the wall's face in zig-zags
//! (the photographer's "saw-tooth skirts" under ParkHaus 8).
//!
//! Real hillside cities cut and fill each plot to its pad and hold the
//! difference with a wall or a bank. Here every lot whose ground mesh
//! leaves its floor is cut out of the mesh exactly (`lot_holes`, through
//! `holes::cut_holes`), floored with a flat cap at its pad (two on a
//! stepped lot), and where the cut ground meets the lot's edge off the
//! floor's height its side is closed (`lot_ground`): a low earth bank face
//! where the step is small, a retaining wall (board-formed concrete, stone
//! or Hong Kong's shotcrete "chunam" with weep holes) where it is not;
//! a lot stepping down to a lower neighbour shows its wall over it. The
//! sim's own retaining walls (`nd_sim::landscape::Retaining`) are drawn by
//! `terrain::retaining_walls`, which now runs each wall's face down to
//! (or up to) the ground the mesh draws in front of it.

use crate::buildings::parts::{mul, rgba};
use crate::mesh::MeshData;
use crate::palette::Palette;
use glam::{Vec2, Vec3};
use nd_sim::{LotId, World};
use std::cell::RefCell;
use std::collections::HashMap;

/// The renderer's ground cell (`LandscapeTiles::new(.., 8.0)`).
pub const LAND_CELL: f32 = 8.0;

/// A lot whose ground mesh stands this far off its floor anywhere near it
/// is cut out of the mesh.
pub const CUT_FROM: f32 = 0.12;

/// A side taller than this is drawn as a retaining wall, not an earth face.
pub const SIDE_WALL_FROM: f32 = 1.0;

/// The ground mesh as drawn at `LAND_CELL`: each vertex the built ground a
/// hair under (and held under the streets), each cell two triangles split
/// along the same diagonal as `terrain::LandscapeTiles`, so heights read
/// here are the heights drawn.
pub struct TerrainProbe<'a> {
    world: &'a World,
    cell: f32,
    half: f32,
    /// Per segment: how far it holds the vertices down (NaN: not at all).
    reach: Vec<f32>,
    max_reach: f32,
    cache: RefCell<HashMap<(i32, i32), f32>>,
}

impl<'a> TerrainProbe<'a> {
    pub fn new(world: &'a World) -> Self {
        Self::with_cell(world, LAND_CELL)
    }

    pub fn with_cell(world: &'a World, cell: f32) -> Self {
        let mut reach = vec![f32::NAN; world.roads.segments.len()];
        let mut max_reach = 0.0f32;
        for (sid, s) in world.roads.alive_segments() {
            if let Some(r) = crate::terrain::street_hold_reach(world, sid, s, cell) {
                reach[sid.0 as usize] = r;
                max_reach = max_reach.max(r);
            }
        }
        Self { world, cell, half: world.landscape.land_half(), reach, max_reach, cache: RefCell::new(HashMap::new()) }
    }

    /// World position of grid vertex (i, j).
    pub fn at(&self, i: i32, j: i32) -> Vec2 {
        Vec2::new(-self.half + i as f32 * self.cell, -self.half + j as f32 * self.cell)
    }

    /// The drawn height of grid vertex (i, j).
    pub fn vertex(&self, i: i32, j: i32) -> f32 {
        if let Some(&h) = self.cache.borrow().get(&(i, j)) {
            return h;
        }
        let w = self.world;
        let p = self.at(i, j);
        let mut h = w.ground_height(p) - 0.08;
        if self.max_reach > 0.0 {
            let r = Vec2::splat(self.max_reach + 1.0);
            for sid in w.roads.segments_in_box(p - r, p + r) {
                let reach = self.reach.get(sid.0 as usize).copied().unwrap_or(f32::NAN);
                if reach.is_nan() {
                    continue;
                }
                let (a, b) = w.roads.segment_points(sid);
                let (t, q) = nd_sim::roads::project_on_segment(p, a, b);
                if q.distance(p) > reach {
                    continue;
                }
                h = h.min(w.road_height(sid, t) - 0.08);
            }
        }
        self.cache.borrow_mut().insert((i, j), h);
        h
    }

    /// The drawn ground's height at `p` (on the cell's triangle).
    pub fn height(&self, p: Vec2) -> f32 {
        let fx = (p.x + self.half) / self.cell;
        let fy = (p.y + self.half) / self.cell;
        let (i, j) = (fx.floor() as i32, fy.floor() as i32);
        let (u, v) = (fx - i as f32, fy - j as f32);
        // Corners a (i, j), b (i+1, j), c (i, j+1), d (i+1, j+1); the
        // triangles a-c-b and b-c-d (split along b-c).
        if u + v <= 1.0 {
            let (a, b, c) = (self.vertex(i, j), self.vertex(i + 1, j), self.vertex(i, j + 1));
            a + (b - a) * u + (c - a) * v
        } else {
            let (b, c, d) = (self.vertex(i + 1, j), self.vertex(i, j + 1), self.vertex(i + 1, j + 1));
            d + (c - d) * (1.0 - u) + (b - d) * (1.0 - v)
        }
    }

    /// Where the drawn ground bends along the line from `a` to `b`: the
    /// parameters (0 and 1 included, ascending) at which it crosses a cell
    /// edge or diagonal. Between two of them the ground is a straight line.
    pub fn breaks(&self, a: Vec2, b: Vec2) -> Vec<f32> {
        let mut ts = vec![0.0f32, 1.0];
        let d = b - a;
        let c = self.cell;
        let mut cross = |from: f32, delta: f32, origin: f32| {
            // Lines origin + k * c crossed by from + delta * t, t in (0, 1).
            if delta.abs() < 1e-6 {
                return;
            }
            let (lo, hi) = if delta > 0.0 { (from, from + delta) } else { (from + delta, from) };
            let k0 = ((lo - origin) / c).ceil() as i64;
            let k1 = ((hi - origin) / c).floor() as i64;
            for k in k0..=k1 {
                let t = (origin + k as f32 * c - from) / delta;
                if t > 1e-4 && t < 1.0 - 1e-4 {
                    ts.push(t);
                }
            }
        };
        cross(a.x, d.x, -self.half);
        cross(a.y, d.y, -self.half);
        // The diagonals: x + z = -2 half + k cell.
        cross(a.x + a.y, d.x + d.y, -2.0 * self.half);
        ts.sort_by(|x, y| x.total_cmp(y));
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-4);
        ts
    }
}

/// The floor of lot `id`: the pad it is levelled to (its building's, for
/// every plot of a merged site, as its gravel is laid) and its step onto a
/// second pad, if it has one.
pub fn lot_floor(world: &World, id: LotId) -> Option<(f32, Option<nd_sim::landscape::PadStep>)> {
    let lot = world.lots.get(id)?;
    let own = lot.building.and_then(|b| world.buildings.get(b)).map_or(id, |b| b.lot);
    let pad = *world.ground.pads.get(&own)?;
    let step = if own == id { world.ground.steps.get(&id).copied() } else { None };
    Some((pad, step))
}

fn floor_at(floor: (f32, Option<nd_sim::landscape::PadStep>), p: Vec2) -> f32 {
    floor.1.map_or(floor.0, |s| s.pad_at(p, floor.0))
}

/// The lots cut out of the ground mesh: on a landscape with relief, every
/// lot with a pad (not a park, which lies on the ground, nor a strip mine's
/// pit) whose drawn ground near it stands `CUT_FROM` or more off its floor.
pub fn cut_lots(world: &World) -> Vec<LotId> {
    cut_lots_with(world, &TerrainProbe::new(world))
}

pub fn cut_lots_with(world: &World, probe: &TerrainProbe) -> Vec<LotId> {
    if !world.relief || world.landscape.is_flat() {
        return Vec::new();
    }
    let pits = crate::holes::holed_lots(world);
    let mut out: Vec<LotId> = world
        .lots
        .iter()
        .filter(|(id, l)| l.polygon.len() >= 3 && !(l.building.is_none() && l.park.is_some()) && !pits.contains(id))
        .filter_map(|(id, l)| {
            let floor = lot_floor(world, id)?;
            let (lo_f, hi_f) = match floor.1 {
                Some(s) => (floor.0.min(s.back), floor.0.max(s.back)),
                None => (floor.0, floor.0),
            };
            let (lo, hi) = nd_sim::geom::bounds(&l.polygon);
            let c = probe.cell;
            let i0 = ((lo.x + probe.half) / c).floor() as i32;
            let i1 = ((hi.x + probe.half) / c).floor() as i32 + 1;
            let j0 = ((lo.y + probe.half) / c).floor() as i32;
            let j1 = ((hi.y + probe.half) / c).floor() as i32 + 1;
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let v = probe.vertex(i, j) + 0.08;
                    if v > hi_f + CUT_FROM || v < lo_f - CUT_FROM {
                        return Some(id);
                    }
                }
            }
            None
        })
        .collect();
    out.sort();
    out
}

/// The holes `cut_lots` leaves in the ground mesh: each lot's outline in
/// triangles (the holes must be convex).
pub fn lot_holes(world: &World) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    for id in cut_lots(world) {
        let Some(l) = world.lots.get(id) else { continue };
        for t in crate::mesh::triangulate(&l.polygon) {
            let tri = vec![l.polygon[t[0]], l.polygon[t[1]], l.polygon[t[2]]];
            if (tri[1] - tri[0]).perp_dot(tri[2] - tri[0]).abs() > 1e-3 {
                out.push(tri);
            }
        }
    }
    out
}

/// Cut lots by square of 32 m, to find the one beyond an edge.
struct LotIndex<'a> {
    world: &'a World,
    bins: HashMap<(i32, i32), Vec<LotId>>,
}

impl<'a> LotIndex<'a> {
    fn new(world: &'a World, ids: &[LotId]) -> Self {
        let mut bins: HashMap<(i32, i32), Vec<LotId>> = HashMap::new();
        let b = |v: f32| (v / 32.0).floor() as i32;
        for &id in ids {
            let Some(l) = world.lots.get(id) else { continue };
            let (lo, hi) = nd_sim::geom::bounds(&l.polygon);
            for j in b(lo.y)..=b(hi.y) {
                for i in b(lo.x)..=b(hi.x) {
                    bins.entry((i, j)).or_default().push(id);
                }
            }
        }
        Self { world, bins }
    }

    fn at(&self, p: Vec2, not: LotId) -> Option<LotId> {
        let key = ((p.x / 32.0).floor() as i32, (p.y / 32.0).floor() as i32);
        self.bins.get(&key)?.iter().copied().find(|&o| o != not && self.world.lots.get(o).is_some_and(|l| l.contains(p)))
    }
}

/// How a hillside lot's side is faced where it is tall enough for a wall:
/// board-formed concrete, dressed stone or shotcrete ("chunam", Hong Kong's
/// sprayed slope cover, grey-beige with rows of weep holes), by the lot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallFace {
    Concrete,
    Stone,
    Chunam,
}

pub fn wall_face(at: Vec2) -> WallFace {
    match crate::hash::hash2(at.x.to_bits() as u64, at.y.to_bits() as u64) % 3 {
        0 => WallFace::Concrete,
        1 => WallFace::Stone,
        _ => WallFace::Chunam,
    }
}

pub fn wall_colour(face: WallFace, palette: &Palette) -> [f32; 3] {
    match face {
        WallFace::Concrete => mul(palette.concrete(0), 0.92),
        WallFace::Stone => mul(palette.stone, 0.85),
        WallFace::Chunam => [0.62, 0.58, 0.49],
    }
}

/// The floors of the cut lots and their sides: `(earth, walls)`, the caps
/// and low banks in the ground's colour, the tall sides as walls.
pub fn lot_ground(world: &World, palette: &Palette) -> (MeshData, MeshData) {
    let probe = TerrainProbe::new(world);
    let ids = cut_lots_with(world, &probe);
    lot_ground_for(world, palette, &probe, &ids)
}

pub fn lot_ground_for(world: &World, palette: &Palette, probe: &TerrainProbe, ids: &[LotId]) -> (MeshData, MeshData) {
    let mut earth = MeshData::default();
    let mut walls = MeshData::default();
    let ground = rgba(mul(palette.ground, 0.9));
    let bank = rgba(mul(palette.ground, 0.7));
    let index = LotIndex::new(world, ids);
    let floors: HashMap<LotId, (f32, Option<nd_sim::landscape::PadStep>)> = ids.iter().filter_map(|&id| lot_floor(world, id).map(|f| (id, f))).collect();
    let mut walls_of: HashMap<LotId, Vec<&nd_sim::landscape::Retaining>> = HashMap::new();
    for r in &world.ground.retaining {
        walls_of.entry(r.lot).or_default().push(r);
    }
    for &id in ids {
        let (Some(lot), Some(&floor)) = (world.lots.get(id), floors.get(&id)) else { continue };
        // The floor: one cap at the pad, or a cap either side of the step.
        match floor.1 {
            Some(st) => {
                for (poly, z) in [(nd_sim::geom::clip_half_plane(&lot.polygon, st.a, st.back_dir), floor.0), (nd_sim::geom::clip_half_plane(&lot.polygon, st.a, -st.back_dir), st.back)] {
                    if poly.len() >= 3 {
                        earth.ground_polygon(&poly, z - 0.03, ground);
                    }
                }
            }
            None => earth.ground_polygon(&lot.polygon, floor.0 - 0.03, ground),
        }
        // Into the lot: left of each edge on a counter-clockwise outline.
        let ccw = {
            let n = lot.polygon.len();
            (0..n).map(|k| lot.polygon[k].perp_dot(lot.polygon[(k + 1) % n])).sum::<f32>() > 0.0
        };
        let face = wall_face(lot.center);
        let wall_col = rgba(wall_colour(face, palette));
        let dark = rgba(mul(wall_colour(face, palette), 0.3));
        for (a, b) in nd_sim::landscape::lot_edges(&lot.polygon, floor.1.as_ref()) {
            let len = a.distance(b);
            if len < 0.05 {
                continue;
            }
            let mid = a.lerp(b, 0.5);
            let f = floor_at(floor, mid);
            // The sim's own walls along this edge are drawn by
            // `terrain::retaining_walls`.
            let walled = walls_of.get(&id).is_some_and(|v| v.iter().any(|r| nd_sim::geom::project_on_segment(mid, r.a, r.b).1.distance(mid) < 0.05));
            if walled {
                continue;
            }
            let d = (b - a) / len;
            let inward = if ccw { Vec2::new(-d.y, d.x) } else { Vec2::new(d.y, -d.x) };
            // Bends of the ground, and every 2 m to find what lies beyond.
            let mut ts = probe.breaks(a, b);
            let extra = (len / 2.0).floor() as usize;
            for k in 1..=extra {
                ts.push(k as f32 / (extra + 1) as f32);
            }
            ts.sort_by(|x, y| x.total_cmp(y));
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-4);
            for w in ts.windows(2) {
                let (p0, p1) = (a.lerp(b, w[0]), a.lerp(b, w[1]));
                let beyond = p0.lerp(p1, 0.5) - inward * 0.3;
                // What the side meets: a neighbouring cut lot's floor (the
                // higher of the two draws the step), else the drawn ground.
                let (g0, g1) = match index.at(beyond, id).and_then(|o| floors.get(&o)) {
                    Some(&nf) => {
                        let (n0, n1) = (floor_at(nf, p0 - inward * 0.3), floor_at(nf, p1 - inward * 0.3));
                        if n0.max(n1) >= f - 0.02 {
                            continue;
                        }
                        (n0 - 0.03, n1 - 0.03)
                    }
                    None => (probe.height(p0), probe.height(p1)),
                };
                side(&mut earth, &mut walls, p0, p1, f - 0.03, g0, g1, inward, bank, wall_col, dark, face);
            }
        }
    }
    (earth, walls)
}

/// One piece of a lot's side from `p0` to `p1` on its edge: between the
/// floor `f` and the ground beyond (`g0`, `g1` at its ends), split where
/// they cross; faced into the lot where the ground stands over the floor
/// (a cut bank), out where it lies under it (the lot's fill held up).
#[allow(clippy::too_many_arguments)]
fn side(earth: &mut MeshData, walls: &mut MeshData, p0: Vec2, p1: Vec2, f: f32, g0: f32, g1: f32, inward: Vec2, bank: [f32; 4], wall: [f32; 4], dark: [f32; 4], face: WallFace) {
    let (d0, d1) = (g0 - f, g1 - f);
    if d0.abs() < 0.02 && d1.abs() < 0.02 {
        return;
    }
    let v = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    let into = Vec3::new(inward.x, 0.0, inward.y);
    if d0 * d1 < 0.0 {
        let s = d0 / (d0 - d1);
        let x = p0.lerp(p1, s);
        side(earth, walls, p0, x, f, g0, f, inward, bank, wall, dark, face);
        side(earth, walls, x, p1, f, f, g1, inward, bank, wall, dark, face);
        return;
    }
    // Facing into the lot where the ground beyond stands over the floor.
    let hill = d0 + d1 > 0.0;
    let out2 = if hill { inward } else { -inward };
    let out = Vec3::new(out2.x, 0.0, out2.y);
    let tall = d0.abs().max(d1.abs()) >= SIDE_WALL_FROM;
    let m = if tall { &mut *walls } else { &mut *earth };
    let col = if tall { wall } else { bank };
    // A wall holding the hill back tops out a little proud of the ground
    // behind it; one holding the lot up runs a little under the ground in
    // front of it.
    let (e0, e1) = match (tall, hill) {
        (true, true) => (g0 + 0.3, g1 + 0.3),
        (true, false) => (g0 - 0.3, g1 - 0.3),
        _ => (g0, g1),
    };
    m.quad_facing([v(p0, f), v(p1, f), v(p1, e1), v(p0, e0)], out, col);
    if !tall {
        return;
    }
    // The coping along the wall's top, the wall's thickness behind its
    // face: over the hill side, or as a lip on the lot's edge.
    let (top0, top1) = if hill { (e0, e1) } else { (f, f) };
    let (c0, c1) = (p0 - out2 * 0.35, p1 - out2 * 0.35);
    m.quad_facing([v(p0, top0 + 0.12), v(p1, top1 + 0.12), v(c1, top1 + 0.12), v(c0, top0 + 0.12)], Vec3::Y, col);
    m.quad_facing([v(p0, top0), v(p1, top1), v(p1, top1 + 0.12), v(p0, top0 + 0.12)], out, col);
    let _ = into;
    // Weep holes in a chunam face: a row every 1.5 m up, every 2 m along.
    if face == WallFace::Chunam {
        let len = p0.distance(p1);
        let n = (len / 2.0).floor() as i32;
        let dir = (p1 - p0).normalize_or_zero();
        for k in 0..n {
            let s = (k as f32 + 0.5) / n as f32;
            let p = p0.lerp(p1, s) + out2 * 0.02;
            let g = e0 + (e1 - e0) * s;
            let (lo, hi) = (f.min(g), f.max(g));
            let mut y = lo + 1.0;
            while y < hi - 0.6 {
                let (q0, q1) = (p - dir * 0.06, p + dir * 0.06);
                m.quad_facing([v(q0, y), v(q1, y), v(q1, y + 0.12), v(q0, y + 0.12)], out, dark);
                y += 1.5;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe's heights are the ground mesh's: at its vertices and inside
    /// its triangles, against the landscape's mesh built at the same cell.
    #[test]
    fn the_probe_reads_the_drawn_ground() {
        let mut w = World::new(nd_sim::WorldConfig::default());
        nd_sim::scenarios::demo_district(&mut w);
        assert!(w.relief);
        let pal = Palette::default();
        let mesh = crate::terrain::build_landscape_terrain(&w, &pal, LAND_CELL);
        let probe = TerrainProbe::new(&w);
        let mut checked = 0;
        for t in mesh.indices.chunks_exact(3).step_by(97) {
            let p: Vec<Vec3> = t.iter().map(|&i| Vec3::from(mesh.positions[i as usize])).collect();
            let c = (p[0] + p[1] + p[2]) / 3.0;
            let h = probe.height(Vec2::new(c.x, c.z));
            assert!((h - c.y).abs() < 0.02, "triangle centre at ({:.1},{:.1}): mesh {:.2}, probe {h:.2}", c.x, c.z, c.y);
            checked += 1;
        }
        assert!(checked > 100);
    }

    fn built_demo() -> World {
        let mut w = World::new(nd_sim::WorldConfig::default());
        nd_sim::scenarios::demo_district(&mut w);
        let empty: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.park.is_none()).map(|(id, _)| id).collect();
        for l in empty {
            w.build_on(l);
        }
        w
    }

    /// [r15-hillside] Every lot whose drawn ground leaves its floor is cut
    /// out of the ground mesh: no triangle of the mesh is left inside one,
    /// and every lot left in the mesh has its ground within a hair of its
    /// floor at every vertex near it.
    #[test]
    fn hillside_lots_are_cut_out_of_the_ground_mesh() {
        let w = built_demo();
        let pal = Palette::default();
        let probe = TerrainProbe::new(&w);
        let cut = cut_lots_with(&w, &probe);
        assert!(cut.len() >= 5, "lots cut: {}", cut.len());
        let index = LotIndex::new(&w, &cut);
        let mesh = crate::terrain::build_landscape_terrain(&w, &pal, LAND_CELL);
        let mut left = 0;
        for t in mesh.indices.chunks_exact(3) {
            let p: Vec<Vec2> = t.iter().map(|&i| Vec2::new(mesh.positions[i as usize][0], mesh.positions[i as usize][2])).collect();
            let c = (p[0] + p[1] + p[2]) / 3.0;
            if (p[1] - p[0]).perp_dot(p[2] - p[0]).abs() < 0.02 {
                continue;
            }
            // Well inside a cut lot (not a sliver along its edge).
            if let Some(id) = index.at(c, LotId::default()) {
                let l = &w.lots[id];
                let edge = (0..l.polygon.len()).map(|k| nd_sim::geom::project_on_segment(c, l.polygon[k], l.polygon[(k + 1) % l.polygon.len()]).1.distance(c)).fold(f32::MAX, f32::min);
                if edge > 0.05 {
                    left += 1;
                }
            }
        }
        assert_eq!(left, 0, "ground triangles left inside cut lots");
        // A lot not cut: its ground stands within `CUT_FROM` of its floor.
        let cut: std::collections::HashSet<LotId> = cut.into_iter().collect();
        for (id, l) in w.lots.iter() {
            let Some((pad, step)) = lot_floor(&w, id) else { continue };
            if cut.contains(&id) || l.polygon.len() < 3 || l.park.is_some() || step.is_some() {
                continue;
            }
            for v in &l.polygon {
                assert!((probe.height(*v) + 0.08 - pad).abs() <= CUT_FROM + 1e-3, "lot {id:?} left in the mesh off its floor at {v:?}");
            }
        }
    }

    /// [r15-hillside] A lot's side runs from its floor exactly to the ground
    /// beyond at both ends, split where the two cross (a bank face into the
    /// lot where the hill stands over it, a wall face out where the lot
    /// stands over the drop), and a tall side is a wall, not earth.
    #[test]
    fn a_lots_side_meets_the_ground_at_both_ends() {
        let (mut earth, mut walls) = (MeshData::default(), MeshData::default());
        let (p0, p1) = (Vec2::new(0.0, 0.0), Vec2::new(8.0, 0.0));
        let inward = Vec2::new(0.0, 1.0);
        side(&mut earth, &mut walls, p0, p1, 10.0, 14.0, 6.0, inward, [1.0; 4], [0.5, 0.5, 0.5, 1.0], [0.1, 0.1, 0.1, 1.0], WallFace::Concrete);
        assert!(earth.positions.is_empty(), "a 4 m side is a wall");
        let span = |x: f32| {
            walls.positions.iter().filter(|p| (p[0] - x).abs() < 1e-3 && p[2].abs() < 1e-3).fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])))
        };
        let (lo0, hi0) = span(0.0);
        assert!(lo0 <= 10.0 + 1e-3 && hi0 >= 14.0, "at the hill end: {lo0}..{hi0}");
        let (lo1, hi1) = span(8.0);
        assert!(lo1 <= 6.0 && hi1 >= 10.0 - 1e-3, "at the drop end: {lo1}..{hi1}");
        // Where they cross (the middle) the side closes to the floor (less
        // the wall's lip over the ground and its coping).
        let (lo_m, hi_m) = span(4.0);
        assert!(lo_m >= 9.6 && hi_m <= 10.5, "at the crossing: {lo_m}..{hi_m}");
        // A small step is an earth face.
        let (mut earth, mut walls) = (MeshData::default(), MeshData::default());
        side(&mut earth, &mut walls, p0, p1, 10.0, 10.4, 10.6, inward, [1.0; 4], [0.5; 4], [0.1; 4], WallFace::Stone);
        assert!(walls.positions.is_empty() && !earth.positions.is_empty());
    }
}
