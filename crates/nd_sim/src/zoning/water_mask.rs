//! The water a cutter keeps lots out of, and the shoreline lots front.

use super::*;

/// Water to keep parcels off: polygons (with holes) and their boxes.
#[derive(Clone, Default)]
pub struct WaterMask {
    pub(super) polys: Vec<((Vec2, Vec2), Polygon<f64>)>,
}

impl WaterMask {
    /// From outlines with holes, as `Landscape::water_polygons` gives them.
    pub fn new(polys: &[(Vec<Vec2>, Vec<Vec<Vec2>>)]) -> Self {
        let mut out = Vec::new();
        for (outer, holes) in polys {
            if outer.len() < 3 || geom::self_intersects(outer) {
                continue;
            }
            let ring = |r: &[Vec2]| LineString::from(ccw(r).iter().map(|p| Coord { x: p.x as f64, y: p.y as f64 }).collect::<Vec<_>>());
            let poly = Polygon::new(ring(outer), holes.iter().filter(|h| h.len() >= 3).map(|h| ring(h)).collect());
            out.push((bounds_of(outer), poly));
        }
        WaterMask { polys: out }
    }

    pub fn is_empty(&self) -> bool {
        self.polys.is_empty()
    }

    /// [waterfront] Is `p` in the water?
    pub fn contains(&self, p: Vec2) -> bool {
        use geo::Contains;
        let c = geo::Point::new(p.x as f64, p.y as f64);
        self.polys.iter().any(|((lo, hi), poly)| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y && poly.contains(&c))
    }

    /// [waterfront] Does an outline touch the water: a vertex with water
    /// within `tol` metres of it (four probes round it)?
    pub fn touches(&self, poly: &[Vec2], tol: f32) -> bool {
        if self.polys.is_empty() || poly.is_empty() {
            return false;
        }
        let (lo, hi) = bounds_of(poly);
        if !self.polys.iter().any(|(b, _)| boxes_overlap(*b, (lo, hi), tol + 1.0)) {
            return false;
        }
        let d = tol * std::f32::consts::FRAC_1_SQRT_2;
        poly.iter().any(|&p| [Vec2::new(d, d), Vec2::new(-d, d), Vec2::new(d, -d), Vec2::new(-d, -d)].iter().any(|o| self.contains(p + *o)))
    }

    /// [waterfront] How much of an outline runs along the water (metres),
    /// and which way the water lies (unit, frontage weighted; zero when
    /// none). Each edge is probed a little outside it at three points.
    pub fn shoreline(&self, poly: &[Vec2]) -> (f32, Vec2) {
        if self.polys.is_empty() || poly.len() < 3 {
            return (0.0, Vec2::ZERO);
        }
        let (lo, hi) = bounds_of(poly);
        if !self.polys.iter().any(|(b, _)| boxes_overlap(*b, (lo, hi), 3.0)) {
            return (0.0, Vec2::ZERO);
        }
        let poly = ccw(poly);
        let n = poly.len();
        let (mut len, mut dir) = (0.0f32, Vec2::ZERO);
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let l = a.distance(b);
            if l < 0.5 {
                continue;
            }
            let d = (b - a) / l;
            let out = Vec2::new(d.y, -d.x);
            let wet = [0.2f32, 0.5, 0.8].iter().filter(|&&t| self.contains(a.lerp(b, t) + out * 1.5)).count();
            if wet > 0 {
                let part = l * wet as f32 / 3.0;
                len += part;
                dir += out * part;
            }
        }
        (len, dir.normalize_or_zero())
    }

    /// The water touching a box, as one multipolygon.
    pub(super) fn near(&self, lo: Vec2, hi: Vec2) -> Option<MultiPolygon<f64>> {
        let mut acc: Option<MultiPolygon<f64>> = None;
        for (b, p) in &self.polys {
            if !boxes_overlap(*b, (lo, hi), 1.0) {
                continue;
            }
            acc = Some(match acc {
                None => MultiPolygon(vec![p.clone()]),
                Some(m) => m.union(p),
            });
        }
        acc
    }
}
