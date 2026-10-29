//! [horizon] Square rings of ever coarser grids round the map: the far
//! ground's level of detail.
//!
//! Each ring's cell is twice the one inside it, and every ring boundary
//! lies on both rings' grid lines, so the coarse ring's edge meets the fine
//! one exactly: where a coarse cell borders the finer ring, it takes the
//! finer ring's midpoint vertex on that edge as well (a fan of three
//! triangles instead of two), so there are no T-junctions and no cracks.
//! Vertices are keyed by their (whole-metre) position and shared, so the
//! same point is one vertex wherever it is reached from. The innermost
//! ring's cell is the playable ground's own, starting on its edge, so the
//! far ground's first row of vertices sits exactly on the map's last one.

use crate::mesh::MeshData;
use glam::Vec2;
use std::collections::HashMap;

/// One square annulus: from `inner` to `outer` (half sides) in cells of `cell`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub inner: f32,
    pub outer: f32,
    pub cell: f32,
}

/// Rings from `inner` (the playable ground's half side, a multiple of
/// `seam_cell`) out to each of `reach` in turn, the cell doubling each
/// ring: the first ring one `seam_cell` wide, then each boundary rounded
/// out to a whole number of the next ring's cells.
pub fn layout(inner: f32, seam_cell: f32, reach: &[f32]) -> Vec<Ring> {
    let mut rings = Vec::new();
    let mut cell = seam_cell;
    let mut at = inner;
    let round_up = |v: f32, step: f32| (v / step).ceil() * step;
    // The first ring: one cell of the seam's size, out to a line of the next ring's grid.
    let first = round_up(inner + seam_cell, seam_cell * 2.0);
    rings.push(Ring { inner: at, outer: first, cell });
    at = first;
    for (k, &r) in reach.iter().enumerate() {
        cell *= 2.0;
        let last = k + 1 == reach.len();
        let step = if last { cell } else { cell * 2.0 };
        let outer = round_up(r.max(at + cell), step);
        rings.push(Ring { inner: at, outer, cell });
        at = outer;
    }
    rings
}

/// What the ground is at one vertex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub h: f32,
    pub normal: [f32; 3],
    pub rgba: [f32; 4],
}

/// Which quadrant mesh a cell goes in (by its centre): +x+z, -x+z, -x-z, +x-z.
fn quadrant(c: Vec2) -> usize {
    match (c.x >= 0.0, c.y >= 0.0) {
        (true, true) => 0,
        (false, true) => 1,
        (false, false) => 2,
        (true, false) => 3,
    }
}

/// The cells of `ring` (lower corners), skipping those inside its inner square.
fn cells(ring: &Ring) -> impl Iterator<Item = (f32, f32)> + '_ {
    let n = (ring.outer * 2.0 / ring.cell).round() as i64;
    (0..n).flat_map(move |j| (0..n).map(move |i| (-ring.outer + i as f32 * ring.cell, -ring.outer + j as f32 * ring.cell))).filter(move |&(x0, z0)| {
        let a = ring.inner;
        let c = ring.cell;
        !(x0 >= -a - 0.01 && x0 + c <= a + 0.01 && z0 >= -a - 0.01 && z0 + c <= a + 0.01)
    })
}

/// The polygon of a cell in winding order, with the midpoint of any edge
/// lying on the inner square of a ring whose inner neighbour is finer.
fn cell_polygon(x0: f32, z0: f32, c: f32, inner: f32, stitch: bool) -> Vec<Vec2> {
    let corners = [Vec2::new(x0, z0), Vec2::new(x0, z0 + c), Vec2::new(x0 + c, z0 + c), Vec2::new(x0 + c, z0)];
    let mut poly = Vec::with_capacity(5);
    for k in 0..4 {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        poly.push(a);
        if stitch && on_square(a, b, inner) {
            poly.push((a + b) * 0.5);
        }
    }
    poly
}

/// Does the edge a-b lie along the square of half side `s`?
fn on_square(a: Vec2, b: Vec2, s: f32) -> bool {
    let near = |v: f32, w: f32| (v - w).abs() < 0.01;
    let within = |u: f32, v: f32| u.min(v) >= -s - 0.01 && u.max(v) <= s + 0.01;
    ((near(a.x, s) || near(a.x, -s)) && near(a.x, b.x) && within(a.y, b.y)) || ((near(a.y, s) || near(a.y, -s)) && near(a.y, b.y) && within(a.x, b.x))
}

fn key(p: Vec2) -> (i32, i32) {
    ((p.x * 4.0).round() as i32, (p.y * 4.0).round() as i32)
}

/// The rings as four meshes, one per quadrant (so the renderer can cull
/// what is behind the camera), sampling `sample(point, cell)` once per
/// distinct vertex over a few threads. `keep(cell centre, cell)` can drop
/// cells (water meshes keep only the wet ones; ground keeps all).
pub fn build(rings: &[Ring], sample: &(dyn Fn(Vec2, f32) -> Sample + Sync), keep: &dyn Fn(Vec2, f32) -> bool) -> [MeshData; 4] {
    // Every cell's polygon, and each distinct vertex with the finest cell
    // it is reached from (its normal's step).
    let mut polys: Vec<(usize, Vec<Vec2>)> = Vec::new();
    let mut verts: Vec<(Vec2, f32)> = Vec::new();
    let mut index: HashMap<(i32, i32), u32> = HashMap::new();
    for (k, ring) in rings.iter().enumerate() {
        let stitch = k > 0 && rings[k - 1].cell < ring.cell;
        for (x0, z0) in cells(ring) {
            let centre = Vec2::new(x0 + ring.cell * 0.5, z0 + ring.cell * 0.5);
            if !keep(centre, ring.cell) {
                continue;
            }
            let poly = cell_polygon(x0, z0, ring.cell, ring.inner, stitch);
            for p in &poly {
                index.entry(key(*p)).or_insert_with(|| {
                    verts.push((*p, ring.cell));
                    (verts.len() - 1) as u32
                });
            }
            polys.push((quadrant(centre), poly));
        }
    }
    let samples = par_map(&verts, |&(p, cell)| sample(p, cell));
    let mut out: [MeshData; 4] = Default::default();
    let mut local: [HashMap<u32, u32>; 4] = Default::default();
    for (q, poly) in &polys {
        let mesh = &mut out[*q];
        let ids: Vec<u32> = poly
            .iter()
            .map(|p| {
                let g = index[&key(*p)];
                *local[*q].entry(g).or_insert_with(|| {
                    let s = samples[g as usize];
                    mesh.positions.push([p.x, s.h, p.y]);
                    mesh.normals.push(s.normal);
                    mesh.colors.push(s.rgba);
                    mesh.uvs.push([p.x, -p.y]);
                    (mesh.positions.len() - 1) as u32
                })
            })
            .collect();
        if ids.len() == 4 {
            mesh.indices.extend_from_slice(&[ids[0], ids[1], ids[3], ids[3], ids[1], ids[2]]);
        } else {
            // A fan from the corner two past the midpoint (never on its edge).
            let m = poly.len();
            let mid = (0..m).find(|&i| {
                let (a, b) = (poly[(i + m - 1) % m], poly[(i + 1) % m]);
                ((a + b) * 0.5 - poly[i]).length() < 0.01
            });
            let s = mid.map_or(0, |i| (i + 2) % m);
            for t in 1..m - 1 {
                mesh.indices.extend_from_slice(&[ids[s], ids[(s + t) % m], ids[(s + t + 1) % m]]);
            }
        }
    }
    out
}

/// `f` over `items` on a few threads, in order.
pub(crate) fn par_map<T: Sync, U: Send + Clone>(items: &[T], f: impl Fn(&T) -> U + Sync) -> Vec<U> {
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).clamp(1, 8);
    let per = items.len().div_ceil(threads).max(1);
    let mut parts: Vec<Vec<U>> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = items.chunks(per).map(|chunk| s.spawn(|| chunk.iter().map(&f).collect::<Vec<U>>())).collect();
        parts = handles.into_iter().map(|h| h.join().expect("far rings")).collect();
    });
    parts.concat()
}

/// The normal of a height function at `p`, by central differences `step` apart.
pub fn normal_of(h: &dyn Fn(Vec2) -> f32, p: Vec2, step: f32) -> [f32; 3] {
    let dx = h(p + Vec2::X * step) - h(p - Vec2::X * step);
    let dz = h(p + Vec2::Y * step) - h(p - Vec2::Y * step);
    glam::Vec3::new(-dx, 2.0 * step, -dz).normalize().to_array()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_double_and_meet_on_grid_lines() {
        let rings = layout(1800.0, 8.0, &[2112.0, 3200.0, 4608.0]);
        assert_eq!(rings[0], Ring { inner: 1800.0, outer: 1808.0, cell: 8.0 });
        for w in rings.windows(2) {
            assert_eq!(w[0].outer, w[1].inner);
            assert_eq!(w[1].cell, w[0].cell * 2.0);
            // The boundary is on both grids.
            assert_eq!((w[1].inner / w[1].cell).fract(), 0.0, "{w:?}");
        }
        // A 12 m seam (the flat map's) works the same.
        let flat = layout(1800.0, 12.0, &[2112.0, 3200.0]);
        assert_eq!(flat[0].inner, 1800.0);
        for r in &flat {
            assert_eq!(((r.outer * 2.0) / r.cell).fract(), 0.0, "{r:?}");
        }
    }

    /// No cracks: every edge used by one triangle is used by another,
    /// except along the outside of the inner hole and the outer rim.
    #[test]
    fn ring_mesh_is_watertight() {
        let rings = layout(64.0, 8.0, &[128.0, 256.0, 512.0]);
        let flat = |p: Vec2, _c: f32| Sample { h: (p.x * 0.01).sin() * 5.0, normal: [0.0, 1.0, 0.0], rgba: [1.0; 4] };
        let quads = build(&rings, &flat, &|_, _| true);
        let mut edges: HashMap<((i32, i32), (i32, i32)), i32> = HashMap::new();
        for m in &quads {
            for t in m.indices.chunks(3) {
                for k in 0..3 {
                    let a = m.positions[t[k] as usize];
                    let b = m.positions[t[(k + 1) % 3] as usize];
                    let ka = key(Vec2::new(a[0], a[2]));
                    let kb = key(Vec2::new(b[0], b[2]));
                    *edges.entry(if ka < kb { (ka, kb) } else { (kb, ka) }).or_default() += 1;
                }
            }
        }
        let outer = rings.last().unwrap().outer;
        for ((a, b), n) in edges {
            let pa = Vec2::new(a.0 as f32 / 4.0, a.1 as f32 / 4.0);
            let pb = Vec2::new(b.0 as f32 / 4.0, b.1 as f32 / 4.0);
            if on_square(pa, pb, 64.0) || on_square(pa, pb, outer) {
                assert_eq!(n, 1, "{pa} {pb}");
            } else {
                assert_eq!(n, 2, "open edge {pa} {pb}");
            }
        }
    }
}
