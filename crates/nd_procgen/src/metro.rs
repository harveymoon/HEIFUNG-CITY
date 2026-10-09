//! [subway] What the Underground view shows of the metro: twin bores along
//! each line with track and a lit strip in the line's colour, a platform
//! hall at every station and the shaft up to its entrance; and the metro
//! cars themselves. The entrances at street level are buildings
//! (`buildings::subway`).

use crate::color::{mul, rgba};
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::subway::{CAR_LEN, DEPTH, TRACK_OFFSET};

/// Line colours, by `MetroLine::color`: bright enough to read underground.
pub const LINE_COLOURS: [[f32; 3]; 8] = [
    [1.9, 0.3, 0.35],  // red
    [0.25, 1.1, 1.9],  // blue
    [0.3, 1.7, 0.45],  // green
    [1.9, 1.3, 0.2],   // amber
    [1.5, 0.35, 1.8],  // violet
    [0.2, 1.7, 1.6],   // teal
    [1.9, 0.6, 1.1],   // pink
    [1.4, 1.5, 1.5],   // silver
];

pub fn line_colour(i: u8) -> [f32; 3] {
    LINE_COLOURS[i as usize % LINE_COLOURS.len()]
}

/// One line as the renderer hands it over.
pub struct TunnelLine<'a> {
    pub path: &'a [Vec2],
    pub colour: u8,
}

/// One station: the platform's middle and the way the line runs through
/// it, and the entrance at the street.
pub struct PlatformSite {
    pub at: Vec2,
    pub dir: Vec2,
    pub entrance: Vec2,
    pub colour: u8,
}

/// Half the width of one bore, wall to wall.
const BORE_HALF: f32 = 2.0;
const BORE_H: f32 = 4.4;
pub const PLATFORM_LEN: f32 = 90.0;

/// The tunnels and platforms, with the ground height under any point from
/// `ground` (tunnels follow it at `DEPTH` below).
pub fn tunnels(lines: &[TunnelLine], stations: &[PlatformSite], ground: &dyn Fn(Vec2) -> f32) -> Geometry {
    let mut g = Geometry::default();
    let wall = rgba([0.32, 0.33, 0.36]);
    let floor = rgba([0.2, 0.2, 0.22]);
    let rail = rgba([0.55, 0.55, 0.6]);
    for line in lines {
        let glow = rgba(mul(line_colour(line.colour), 0.8));
        // The whole line as one mitred path, its floor at `DEPTH` under the
        // ground at every vertex: no steps on a slope, no gaps or overlaps
        // at a bend (each leg used to be a box at its own middle's depth).
        let mut pts: Vec<Vec2> = Vec::with_capacity(line.path.len());
        for p in line.path {
            if pts.last().is_none_or(|q: &Vec2| q.distance(*p) >= 0.05) {
                pts.push(*p);
            }
        }
        if pts.len() < 2 {
            continue;
        }
        let ys: Vec<f32> = pts.iter().map(|p| ground(*p) - DEPTH).collect();
        for side in [-1.0f32, 1.0] {
            let c = side * TRACK_OFFSET;
            // Floor and the two walls of the bore (no roof: the view looks
            // down into it), track and the lit strip on the wall.
            sloped_box(g.part(Concrete), &pts, &ys, c, BORE_HALF * 2.0, -0.3, 0.0, floor);
            sloped_box(g.part(Concrete), &pts, &ys, c + side * BORE_HALF, 0.35, 0.0, BORE_H, wall);
            for r in [-0.72f32, 0.72] {
                sloped_box(g.part(Metal), &pts, &ys, c + r, 0.08, 0.0, 0.18, rail);
            }
            sloped_box(g.part(Glow), &pts, &ys, c + side * (BORE_HALF - 0.2), 0.08, 2.6, 2.75, glow);
        }
        // The wall between the bores.
        sloped_box(g.part(Concrete), &pts, &ys, 0.0, TRACK_OFFSET * 2.0 - BORE_HALF * 2.0 + 0.3, 0.0, BORE_H, wall);
    }
    for s in stations {
        let y = ground(s.at) - DEPTH;
        let d = s.dir.normalize_or_zero();
        let (a, b) = (s.at - d * PLATFORM_LEN * 0.5, s.at + d * PLATFORM_LEN * 0.5);
        let glow = rgba(mul(line_colour(s.colour), 0.9));
        // An island platform between the bores, lit edges, a hall box round it.
        g.part(Paving).strip_box(a, b, 0.0, TRACK_OFFSET * 2.0 - 1.6, y, y + 1.0, rgba([0.62, 0.6, 0.56]));
        for side in [-1.0f32, 1.0] {
            g.part(Glow).strip_box(a, b, side * (TRACK_OFFSET - 0.85), 0.12, y + 1.0, y + 1.04, rgba([1.6, 1.4, 0.3]));
            g.part(Concrete).strip_box(a, b, side * (TRACK_OFFSET + BORE_HALF + 1.2), 0.4, y - 0.3, y + 6.0, wall);
            g.part(Glow).strip_box(a, b, side * (TRACK_OFFSET + BORE_HALF + 0.95), 0.06, y + 3.2, y + 3.6, glow);
        }
        for k in 0..6 {
            let p = a.lerp(b, (k as f32 + 0.5) / 6.0);
            g.part(Glow).boxed(Vec3::new(p.x, y + 4.6, p.y), Vec3::new(5.0, 0.15, 0.6), d.y.atan2(d.x), rgba([1.4, 1.35, 1.2]));
            g.part(Concrete).boxed(Vec3::new(p.x, y + 3.0, p.y), Vec3::new(0.5, 4.0, 0.5), 0.0, wall);
        }
        // The shaft up to the street: escalators in a lit box.
        let top = ground(s.entrance);
        let e = s.entrance;
        let mid = (s.at + e) * 0.5;
        let run = s.at.distance(e).max(2.0);
        let yaw = (e - s.at).y.atan2((e - s.at).x);
        g.part(Glow).boxed(Vec3::new(mid.x, (y + 1.0 + top) * 0.5, mid.y), Vec3::new(run, 0.12, 0.12), yaw, glow);
        g.part(Glow).boxed(Vec3::new(e.x, (y + top) * 0.5, e.y), Vec3::new(0.3, (top - y).max(1.0), 0.3), 0.0, glow);
        g.part(Concrete).boxed(Vec3::new(e.x, (y + top) * 0.5 - 0.5, e.y), Vec3::new(3.2, (top - y - 1.0).max(1.0), 3.2), yaw, rgba([0.25, 0.26, 0.29]));
    }
    g
}

/// A box along the mitred path `pts`, `offset` to the right and `width`
/// across, from `dy0` to `dy1` over the height `ys[i]` at each vertex.
#[allow(clippy::too_many_arguments)]
fn sloped_box(m: &mut crate::mesh::MeshData, pts: &[Vec2], ys: &[f32], offset: f32, width: f32, dy0: f32, dy1: f32, color: [f32; 4]) {
    let l = crate::mesh::offset_path(pts, offset - width * 0.5);
    let r = crate::mesh::offset_path(pts, offset + width * 0.5);
    let n = l.len().min(r.len()).min(ys.len());
    if n < 2 {
        return;
    }
    let at = |p: Vec2, i: usize, dy: f32| Vec3::new(p.x, ys[i] + dy, p.y);
    for i in 0..n - 1 {
        let j = i + 1;
        let d = (pts[j] - pts[i]).normalize_or_zero();
        let out_l = Vec3::new(d.y, 0.0, -d.x);
        m.quad_facing([at(l[i], i, dy1), at(l[j], j, dy1), at(r[j], j, dy1), at(r[i], i, dy1)], Vec3::Y, color);
        m.quad_facing([at(l[i], i, dy0), at(l[j], j, dy0), at(l[j], j, dy1), at(l[i], i, dy1)], out_l, color);
        m.quad_facing([at(r[i], i, dy0), at(r[j], j, dy0), at(r[j], j, dy1), at(r[i], i, dy1)], -out_l, color);
    }
    let (d0, d1) = ((pts[1] - pts[0]).normalize_or_zero(), (pts[n - 1] - pts[n - 2]).normalize_or_zero());
    m.quad_facing([at(l[0], 0, dy0), at(r[0], 0, dy0), at(r[0], 0, dy1), at(l[0], 0, dy1)], Vec3::new(-d0.x, 0.0, -d0.y), color);
    m.quad_facing([at(l[n - 1], n - 1, dy0), at(r[n - 1], n - 1, dy0), at(r[n - 1], n - 1, dy1), at(l[n - 1], n - 1, dy1)], Vec3::new(d1.x, 0.0, d1.y), color);
}

/// One metro car at the origin along +x, `CAR_LEN` long: a dark body, a
/// band of windows lit from inside, a stripe and the head and tail lamps in
/// the line's colour.
pub fn metro_car(colour: u8) -> Geometry {
    let mut g = Geometry::default();
    let (l, w, h) = (CAR_LEN, 2.8, 3.3);
    let line = line_colour(colour);
    g.part(Panel).boxed(Vec3::new(0.0, h * 0.5 + 0.3, 0.0), Vec3::new(l, h, w), 0.0, rgba([0.75, 0.77, 0.8]));
    g.part(Metal).boxed(Vec3::new(0.0, 0.25, 0.0), Vec3::new(l * 0.9, 0.5, w * 0.8), 0.0, rgba([0.15, 0.15, 0.17]));
    for side in [-1.0f32, 1.0] {
        let z = side * (w * 0.5 + 0.02);
        g.part(Glow).boxed(Vec3::new(0.0, h * 0.62 + 0.3, z), Vec3::new(l * 0.86, 1.0, 0.04), 0.0, rgba([1.3, 1.25, 1.05]));
        g.part(Glow).boxed(Vec3::new(0.0, 1.1, z), Vec3::new(l * 0.98, 0.18, 0.05), 0.0, rgba(line));
    }
    g.part(Glow).boxed(Vec3::new(0.0, h + 0.32, 0.0), Vec3::new(l * 0.7, 0.06, 0.5), 0.0, rgba(mul(line, 0.6)));
    for end in [-1.0f32, 1.0] {
        g.part(Glow).boxed(Vec3::new(end * (l * 0.5 + 0.02), 1.0, 0.0), Vec3::new(0.04, 0.3, w * 0.6), 0.0, rgba([1.6, 1.5, 1.3]));
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnels_lie_below_the_ground_and_platforms_have_a_shaft() {
        let path = [Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(200.0, 50.0)];
        let lines = [TunnelLine { path: &path, colour: 1 }];
        let stations = [PlatformSite { at: Vec2::new(100.0, 0.0), dir: Vec2::X, entrance: Vec2::new(110.0, 30.0), colour: 1 }];
        let g = tunnels(&lines, &stations, &|_| 5.0);
        assert!(g.triangle_count() > 100);
        let mut top = f32::MIN;
        for (s, m) in g.iter() {
            for p in &m.positions {
                if s != Glow && s != Concrete {
                    top = top.max(p[1]);
                }
                assert!(p[0].is_finite() && p[1].is_finite() && p[2].is_finite());
            }
        }
        assert!(top < 5.0 - DEPTH + 6.5, "bores and platforms stay underground: {top}");
        assert!(metro_car(3).triangle_count() > 20);
        assert_eq!(line_colour(9), LINE_COLOURS[1]);
    }

    /// Review #10: on a slope and round a bend the bore floor is one
    /// surface: every point along each track has floor right under it, at
    /// the depth below the ground there (each leg used to be a level box at
    /// its middle's depth, stepping on slopes and gapping at bends).
    #[test]
    fn bore_floors_are_continuous_on_slopes_and_bends() {
        let path = [Vec2::ZERO, Vec2::new(60.0, 0.0), Vec2::new(110.0, 45.0), Vec2::new(120.0, 120.0)];
        let lines = [TunnelLine { path: &path, colour: 2 }];
        let ground = |p: Vec2| 20.0 + p.x * 0.12 + p.y * 0.05;
        let g = tunnels(&lines, &[], &ground);
        let m = g.get(Concrete);
        let tops: Vec<[Vec3; 3]> = m
            .indices
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize])))
            .filter(|v| (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero().y > 0.9)
            .collect();
        let len = crate::mesh::path_length(&path);
        for k in 0..=200 {
            let (p, d) = crate::mesh::point_along(&path, len * k as f32 / 200.0);
            for side in [-1.0f32, 1.0] {
                let q = p + Vec2::new(-d.y, d.x) * side * TRACK_OFFSET;
                // Level across the line, at the depth under its centre.
                let want = ground(p) - DEPTH;
                // The floor: the up-facing face under the track's middle
                // (the rails stand either side of it).
                let under = tops.iter().filter_map(|v| {
                    let (a, b, c) = (Vec2::new(v[0].x, v[0].z), Vec2::new(v[1].x, v[1].z), Vec2::new(v[2].x, v[2].z));
                    let area = (b - a).perp_dot(c - a);
                    let (u, w) = ((q - a).perp_dot(c - a) / area, (b - a).perp_dot(q - a) / area);
                    (u >= -1e-3 && w >= -1e-3 && u + w <= 1.0 + 1e-3).then(|| v[0].y + (v[1].y - v[0].y) * u + (v[2].y - v[0].y) * w)
                }).fold(f32::MIN, f32::max);
                // A mitred corner carries its vertex's depth a little way
                // along the inside of the bend.
                assert!((under - want).abs() < 0.25, "at {q}: floor at {under}, the depth under the ground is {want}");
            }
        }
    }
}
