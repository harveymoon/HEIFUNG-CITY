//! [p2-look] Holes through flat ground: a strip mine's pit is dug below
//! the ground, so the ground mesh over it (the landscape's tiles, the flat
//! map's terrain, the mine's own yard) has the pit's outline cut out of it,
//! exactly, and the benches drawn below show through. A hole is a convex
//! polygon on the ground plane (x, z); a triangle it touches is split into
//! the convex pieces outside it, their corners interpolated on the
//! triangle's own plane, so the ground keeps its shape to the hole's edge.

use crate::mesh::MeshData;
use glam::Vec2;

/// Twice the signed area of a polygon (positive counter-clockwise).
fn area2(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum()
}

/// Split the convex `piece` by the line through `a`, `b`: the part on the
/// hole's side (`inside`, the side `s` times the cross product is >= 0) and
/// the part beyond it. New corners where an edge crosses come from `lerp`.
fn split<V: Clone>(piece: &[V], pos: &impl Fn(&V) -> Vec2, lerp: &mut impl FnMut(&V, &V, f32) -> V, a: Vec2, b: Vec2, s: f32) -> (Vec<V>, Vec<V>) {
    let side = |p: Vec2| s * (b - a).perp_dot(p - a);
    let (mut inside, mut outside) = (Vec::new(), Vec::new());
    let n = piece.len();
    for i in 0..n {
        let (p, q) = (&piece[i], &piece[(i + 1) % n]);
        let (dp, dq) = (side(pos(p)), side(pos(q)));
        if dp >= 0.0 {
            inside.push(p.clone());
        }
        if dp <= 0.0 {
            outside.push(p.clone());
        }
        if (dp > 0.0 && dq < 0.0) || (dp < 0.0 && dq > 0.0) {
            let x = lerp(p, q, dp / (dp - dq));
            inside.push(x.clone());
            outside.push(x);
        }
    }
    (inside, outside)
}

/// The convex pieces of the convex polygon `piece` outside the convex
/// `hole` (either winding): one per hole edge at most, nothing when the
/// hole covers it, the piece itself when they do not meet.
fn subtract<V: Clone>(piece: Vec<V>, hole: &[Vec2], pos: &impl Fn(&V) -> Vec2, lerp: &mut impl FnMut(&V, &V, f32) -> V) -> Vec<Vec<V>> {
    let s = if area2(hole) >= 0.0 { 1.0 } else { -1.0 };
    let m = hole.len();
    let mut out = Vec::new();
    let mut rest = piece;
    for i in 0..m {
        let (inside, outside) = split(&rest, pos, lerp, hole[i], hole[(i + 1) % m], s);
        let pts: Vec<Vec2> = outside.iter().map(pos).collect();
        if outside.len() >= 3 && area2(&pts).abs() > 1e-5 {
            out.push(outside);
        }
        rest = inside;
        let pts: Vec<Vec2> = rest.iter().map(pos).collect();
        if rest.len() < 3 || area2(&pts).abs() <= 1e-5 {
            break;
        }
    }
    out
}

fn bbox(poly: &[Vec2]) -> (Vec2, Vec2) {
    poly.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
}

fn overlaps(a: (Vec2, Vec2), b: (Vec2, Vec2)) -> bool {
    a.0.x <= b.1.x && b.0.x <= a.1.x && a.0.y <= b.1.y && b.0.y <= a.1.y
}

/// The convex pieces of `poly` (any simple polygon on the ground plane)
/// outside every one of `holes` (convex).
pub fn polygon_minus(poly: &[Vec2], holes: &[Vec<Vec2>]) -> Vec<Vec<Vec2>> {
    let mut lerp = |a: &Vec2, b: &Vec2, t: f32| a.lerp(*b, t);
    let pos = |v: &Vec2| *v;
    let mut pieces: Vec<Vec<Vec2>> = crate::mesh::triangulate(poly).into_iter().map(|t| t.iter().map(|&i| poly[i]).collect()).collect();
    for hole in holes.iter().filter(|h| h.len() >= 3) {
        let hb = bbox(hole);
        pieces = pieces.into_iter().flat_map(|p| if overlaps(bbox(&p), hb) { subtract(p, hole, &pos, &mut lerp) } else { vec![p] }).collect();
    }
    pieces
}

/// One vertex of a mesh, every attribute it carries.
#[derive(Clone)]
struct Vtx {
    p: [f32; 3],
    n: [f32; 3],
    c: [f32; 4],
    uv: [f32; 2],
    t: Option<[f32; 4]>,
}

fn mix<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

/// Cut `holes` (convex polygons on the ground plane, world x, z) out of a
/// ground mesh: every triangle a hole's box touches is replaced by its
/// pieces outside the holes (new corners interpolated on the triangle);
/// the rest are left as they are. Vertices no triangle uses any more stay
/// in the buffers.
pub fn cut_holes(mesh: &mut MeshData, holes: &[Vec<Vec2>]) {
    let holes: Vec<(&Vec<Vec2>, (Vec2, Vec2))> = holes.iter().filter(|h| h.len() >= 3).map(|h| (h, bbox(h))).collect();
    if holes.is_empty() || mesh.indices.is_empty() {
        return;
    }
    let tangents = mesh.tangents.len() == mesh.positions.len();
    let old = std::mem::take(&mut mesh.indices);
    let mut fresh: Vec<Vtx> = Vec::new();
    let mut indices: Vec<u32> = Vec::with_capacity(old.len());
    let pos = |v: &Vtx| Vec2::new(v.p[0], v.p[2]);
    let mut lerp = |a: &Vtx, b: &Vtx, t: f32| {
        let n = mix(a.n, b.n, t);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        Vtx { p: mix(a.p, b.p, t), n: [n[0] / len, n[1] / len, n[2] / len], c: mix(a.c, b.c, t), uv: mix(a.uv, b.uv, t), t: a.t.zip(b.t).map(|(x, y)| mix(x, y, t)) }
    };
    let base = mesh.positions.len() as u32;
    for tri in old.chunks_exact(3) {
        let corners: Vec<Vec2> = tri.iter().map(|&i| Vec2::new(mesh.positions[i as usize][0], mesh.positions[i as usize][2])).collect();
        let tb = bbox(&corners);
        if !holes.iter().any(|(_, hb)| overlaps(tb, *hb)) {
            indices.extend_from_slice(tri);
            continue;
        }
        let vtx = |i: u32| {
            let i = i as usize;
            Vtx { p: mesh.positions[i], n: mesh.normals[i], c: mesh.colors[i], uv: mesh.uvs[i], t: tangents.then(|| mesh.tangents[i]) }
        };
        let mut pieces = vec![tri.iter().map(|&i| vtx(i)).collect::<Vec<Vtx>>()];
        for (hole, hb) in &holes {
            if !overlaps(tb, *hb) {
                continue;
            }
            pieces = pieces.into_iter().flat_map(|p| subtract(p, hole, &pos, &mut lerp)).collect();
        }
        for piece in pieces {
            let first = base + fresh.len() as u32;
            for k in 1..piece.len() - 1 {
                indices.extend_from_slice(&[first, first + k as u32, first + k as u32 + 1]);
            }
            fresh.extend(piece);
        }
    }
    for v in fresh {
        mesh.positions.push(v.p);
        mesh.normals.push(v.n);
        mesh.colors.push(v.c);
        mesh.uvs.push(v.uv);
        if let Some(t) = v.t.filter(|_| tangents) {
            mesh.tangents.push(t);
        }
    }
    mesh.indices = indices;
}

/// The holes in the ground of `world`: the pit of every strip mine that is
/// open (not still going up or coming down), in building order, each the
/// edge its benches are drawn from (`strip_mine_hole` on the building's
/// footprint).
pub fn ground_holes(world: &nd_sim::World) -> Vec<Vec<Vec2>> {
    let mut out: Vec<Vec<Vec2>> = world
        .buildings
        .iter()
        .filter(|(_, b)| b.construction.is_none() && b.demolition.is_none() && b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::StripMine))
        .filter_map(|(id, _)| world.footprint(id).and_then(|lot| crate::buildings::strip_mine_hole(&lot)))
        .collect();
    out.extend(bench_holes(world));
    out.extend(step_holes(world));
    out
}

/// [r5-hillside] The ground cut out under every lot stepped onto two pads
/// (`nd_sim::landscape::PadStep`): its two caps and the wall between them
/// are drawn (`terrain::retaining_walls`), and the coarse cells ramped
/// from one pad to the other across the step, poking through the wall.
pub fn step_holes(world: &nd_sim::World) -> Vec<Vec<Vec2>> {
    let mut ids: Vec<nd_sim::LotId> = world.ground.steps.keys().copied().collect();
    ids.sort();
    ids.into_iter().filter_map(|id| world.lots.get(id)).filter(|l| l.polygon.len() >= 3).map(|l| l.polygon.clone()).collect()
}

/// [r4-hillside] The ground cut out under every way standing in a bench on
/// a hillside (`nd_sim::landscape::Hold::Bench`), out to its walls' faces:
/// the way draws its own surface and verge there and the walls their faces,
/// and the hill beside it keeps its own shape (held down to the way, the
/// coarse cells dug a trench and fanned the slope into zig-zags).
pub fn bench_holes(world: &nd_sim::World) -> Vec<Vec<Vec2>> {
    let roads = &world.roads;
    if world.landscape.is_flat() {
        return Vec::new();
    }
    roads
        .alive_segments()
        .filter(|(sid, _)| nd_sim::landscape::earthwork_hold(&world.landscape, roads, &world.road_z, &world.road_inner, *sid) == Some(nd_sim::landscape::Hold::Bench))
        .filter_map(|(sid, seg)| {
            let (a, b) = roads.segment_points(sid);
            let len = a.distance(b);
            if len < 0.5 {
                return None;
            }
            let d = (b - a) / len;
            let side = Vec2::new(-d.y, d.x);
            let (face, _) = nd_sim::landscape::held_band(seg, nd_sim::landscape::Hold::Bench);
            let (a, b) = (a - d * 0.3, b + d * 0.3);
            Some(vec![a - side * face, b - side * face, b + side * face, a + side * face])
        })
        .collect()
}

/// The lots a hole is dug in: every plot of an open strip mine's site, so
/// the lot ground drawn over them can leave them to the mine's own yard.
pub fn holed_lots(world: &nd_sim::World) -> Vec<nd_sim::LotId> {
    world
        .buildings
        .values()
        .filter(|b| b.construction.is_none() && b.demolition.is_none() && b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::StripMine))
        .flat_map(|b| std::iter::once(b.lot).chain(b.plot.extra_lots.iter().copied()))
        .collect()
}

/// A cheap key that changes whenever `ground_holes` may: every open strip
/// mine and the plots its site spans (fitting the pits themselves is left
/// for when it does).
pub fn holes_key(world: &nd_sim::World) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // [r4-hillside] The benches follow the roads.
    world.roads.version.hash(&mut h);
    // [r5-hillside] And the stepped lots.
    let mut steps: Vec<(nd_sim::LotId, u32)> = world.ground.steps.iter().map(|(id, st)| (*id, st.back.to_bits())).collect();
    steps.sort();
    steps.hash(&mut h);
    for (id, b) in world.buildings.iter() {
        if b.construction.is_none() && b.demolition.is_none() && b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::StripMine) {
            id.hash(&mut h);
            b.lot.hash(&mut h);
            b.plot.extra_lots.hash(&mut h);
        }
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_area(m: &MeshData) -> f32 {
        m.indices
            .chunks_exact(3)
            .map(|t| {
                let p = |i: u32| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2]);
                (p(t[1]) - p(t[0])).perp_dot(p(t[2]) - p(t[0])) * 0.5
            })
            .sum()
    }

    fn square_grid(n: usize, cell: f32) -> MeshData {
        let mut m = MeshData::default();
        for j in 0..=n {
            for i in 0..=n {
                m.positions.push([i as f32 * cell, 0.1 * i as f32, j as f32 * cell]);
                m.normals.push([0.0, 1.0, 0.0]);
                m.colors.push([1.0; 4]);
                m.uvs.push([0.0; 2]);
            }
        }
        let w = (n + 1) as u32;
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let a = j * w + i;
                m.indices.extend_from_slice(&[a, a + w, a + 1, a + 1, a + w, a + w + 1]);
            }
        }
        m
    }

    /// [p2-look] A pit's outline cut out of a ground grid leaves exactly
    /// the grid less the hole, nothing of the ground inside the hole, and
    /// the ground's heights where it was.
    #[test]
    fn a_hole_cut_out_of_the_ground_leaves_the_rest_exact() {
        let mut m = square_grid(8, 8.0);
        let before = tri_area(&m);
        let sign = before.signum();
        let hole: Vec<Vec2> = (0..24).map(|i| Vec2::new(30.0, 34.0) + Vec2::from_angle(i as f32 / 24.0 * std::f32::consts::TAU) * Vec2::new(17.0, 11.0)).collect();
        let hole_area = area2(&hole).abs() * 0.5;
        cut_holes(&mut m, std::slice::from_ref(&hole));
        let after = tri_area(&m);
        assert!((before.abs() - after.abs() - hole_area).abs() < 0.05, "ground {before:.2} less hole {hole_area:.2} = {after:.2}");
        // Every triangle keeps the grid's winding and none has
        // its middle in the hole.
        for t in m.indices.chunks_exact(3) {
            let p = |i: u32| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2]);
            assert!((p(t[1]) - p(t[0])).perp_dot(p(t[2]) - p(t[0])) * sign > 0.0);
            let mid = (p(t[0]) + p(t[1]) + p(t[2])) / 3.0;
            let inside = (0..hole.len()).all(|i| (hole[(i + 1) % hole.len()] - hole[i]).perp_dot(mid - hole[i]) > 1e-3);
            assert!(!inside, "a triangle left in the hole at {mid:?}");
        }
        // The new corners lie on the sloping ground (y = 0.1 x / 8).
        for p in &m.positions {
            assert!((p[1] - 0.1 * p[0] / 8.0).abs() < 1e-3);
        }
        // A polygon less the same hole: the same area.
        let square = vec![Vec2::ZERO, Vec2::new(64.0, 0.0), Vec2::new(64.0, 64.0), Vec2::new(0.0, 64.0)];
        let left: f32 = polygon_minus(&square, std::slice::from_ref(&hole)).iter().map(|p| area2(p) * 0.5).sum();
        assert!((left - (64.0 * 64.0 - hole_area)).abs() < 0.05);
    }
}
