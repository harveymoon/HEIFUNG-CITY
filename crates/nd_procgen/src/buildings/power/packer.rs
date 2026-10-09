//! The layout packer: boxes laid as near where they are wanted as they fit.

use super::*;

/// Lays boxes out on a lot: each goes as near where it is wanted as it fits
/// (inside the outline by a margin, clear of what is already down).
pub(crate) struct Packer<'a> {
    poly: &'a [Vec2],
    pts: Vec<Vec2>,
    pub(super) taken: Vec<(Vec2, Vec2)>,
}

impl<'a> Packer<'a> {
    pub fn new(poly: &'a [Vec2]) -> Self {
        let (lo, hi) = bounds(poly);
        let area = nd_sim::zoning::polygon_area(poly).abs();
        let step = (area.sqrt() / 60.0).max(1.0);
        let mut pts = Vec::new();
        let mut v = lo.y + step * 0.5;
        while v < hi.y {
            let mut u = lo.x + step * 0.5;
            while u < hi.x {
                let p = Vec2::new(u, v);
                if nd_sim::zoning::point_in_polygon(p, poly) {
                    pts.push(p);
                }
                u += step;
            }
            v += step;
        }
        Self { poly, pts, taken: Vec::new() }
    }

    fn free(&self, c: Vec2, h: Vec2, margin: f32, gap: f32) -> bool {
        let (a, b) = (c - h - Vec2::splat(margin), c + h + Vec2::splat(margin));
        let corners = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)];
        for i in 0..4 {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            let n = (p.distance(q) / 0.75).ceil().max(1.0) as i32;
            if (0..n).any(|k| !nd_sim::zoning::point_in_polygon(p.lerp(q, k as f32 / n as f32), self.poly)) {
                return false;
            }
        }
        // A notch of the outline poking in between the samples.
        if self.poly.iter().any(|v| v.x > a.x && v.x < b.x && v.y > a.y && v.y < b.y) {
            return false;
        }
        let (a, b) = (c - h - Vec2::splat(gap), c + h + Vec2::splat(gap));
        !self.taken.iter().any(|(lo, hi)| a.x < hi.x && b.x > lo.x && a.y < hi.y && b.y > lo.y)
    }

    /// Put down a box of half size `h` as near `want` as it fits, no
    /// further than `reach`; `None` when it fits nowhere there.
    pub fn place(&mut self, want: Vec2, h: Vec2, margin: f32, gap: f32, reach: f32) -> Option<Vec2> {
        let at = if self.free(want, h, margin, gap) {
            Some(want)
        } else {
            let mut near: Vec<(f32, Vec2)> = self.pts.iter().map(|p| (p.distance_squared(want), *p)).filter(|(d, _)| *d <= reach * reach).collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.x.total_cmp(&b.1.x)).then(a.1.y.total_cmp(&b.1.y)));
            near.into_iter().map(|x| x.1).find(|p| self.free(*p, h, margin, gap))
        }?;
        self.taken.push((at - h, at + h));
        Some(at)
    }
}
