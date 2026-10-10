//! [parking] The bays of a surface parking lot: laid out once from the
//! parcel's outline, so the spaces the sim counts (`Building::parking_capacity`)
//! are the bays the lot is drawn with, and a lot that says 13 of 30 shows
//! 13 cars in 30 painted bays (F35: "13 of 30 spaces but I only see 3 cars";
//! the old drawing fitted a fixed grid with a 3 m margin, and an irregular
//! 1,000 m² parcel got nine bays).
//!
//! Bays are 2.5 x 5 m in double rows back to back, an aisle of 6 m between
//! the rows that face each other, in the parcel's frame (`u` along the
//! frontage, `v` inward). Both row directions and a few offsets of the grid
//! are tried; the layout with the most bays inside the outline (less a
//! kerb margin) wins. Deterministic: the parcel's outline only.

use crate::zoning::Lot;
use glam::Vec2;

pub const BAY_W: f32 = 2.5;
pub const BAY_D: f32 = 5.0;
pub const AISLE: f32 = 6.0;
/// The lot's edge: the yard's inset plus a margin to its fence.
pub const EDGE: f32 = 1.4;

/// The parcel frame: centre of its bounding rectangle, the frontage
/// direction and the inward one (as `nd_procgen::buildings::Frame::from_lot`).
fn frame(lot: &Lot) -> (Vec2, Vec2, Vec2) {
    let along = (lot.corners[1] - lot.corners[0]).normalize_or_zero();
    let inward = Vec2::new(-along.y, along.x);
    let center = (lot.corners[0] + lot.corners[1] + lot.corners[2] + lot.corners[3]) * 0.25;
    (center, along, inward)
}

/// Bays in lot-local coordinates (`u`, `v`) with the heading a parked car
/// faces in each (local), for an outline `poly` (lot-local).
pub fn bays_local(poly: &[Vec2]) -> Vec<(Vec2, f32)> {
    if poly.len() < 3 {
        return Vec::new();
    }
    let mut ring = poly.to_vec();
    if crate::geom::polygon_area(&ring) < 0.0 {
        ring.reverse();
    }
    let inner = crate::geom::inset_ring(&ring, EDGE);
    if inner.len() < 3 {
        return Vec::new();
    }
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in &inner {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let inside = |p: Vec2, along_v: bool| {
        // A bay's four corners inside the outline.
        let (hu, hv) = if along_v { (BAY_D * 0.5, BAY_W * 0.5) } else { (BAY_W * 0.5, BAY_D * 0.5) };
        [Vec2::new(-hu, -hv), Vec2::new(hu, -hv), Vec2::new(hu, hv), Vec2::new(-hu, hv)].iter().all(|o| crate::geom::point_in_polygon(p + *o, &inner))
    };
    let mut best: Vec<(Vec2, f32)> = Vec::new();
    // Rows along u (bays deep in v), or along v (bays deep in u).
    for along_v in [false, true] {
        let (a0, a1, b0, b1) = if along_v { (lo.y, hi.y, lo.x, hi.x) } else { (lo.x, hi.x, lo.y, hi.y) };
        let module = BAY_D * 2.0 + AISLE;
        for shift_a in [0.0, 0.25, 0.5, 0.75] {
            for shift_b in [0.0, 1.0, 2.0, 3.0] {
                let mut out = Vec::new();
                let mut m = 0;
                loop {
                    let r0 = b0 + shift_b + m as f32 * module;
                    if r0 > b1 {
                        break;
                    }
                    for side in 0..2 {
                        let r = r0 + if side == 0 { BAY_D * 0.5 } else { BAY_D * 1.5 + AISLE };
                        if r + BAY_D * 0.5 > b1 + 0.01 {
                            continue;
                        }
                        // A car faces the aisle beside its row.
                        let face = if side == 0 { 1.0 } else { -1.0 };
                        let mut k = 0;
                        loop {
                            let a = a0 + (k as f32 + 0.5 + shift_a) * BAY_W;
                            if a + BAY_W * 0.5 > a1 + 0.01 {
                                break;
                            }
                            let p = if along_v { Vec2::new(r, a) } else { Vec2::new(a, r) };
                            if inside(p, along_v) {
                                let yaw = if along_v { if face > 0.0 { 0.0 } else { std::f32::consts::PI } } else if face > 0.0 { std::f32::consts::FRAC_PI_2 } else { -std::f32::consts::FRAC_PI_2 };
                                out.push((p, yaw));
                            }
                            k += 1;
                        }
                    }
                    m += 1;
                }
                if out.len() > best.len() {
                    best = out;
                }
            }
        }
    }
    best
}

/// The bays of a surface lot on `lot`, in world coordinates with the
/// heading of a car parked in each, in a fixed order.
pub fn surface_bays(lot: &Lot) -> Vec<(Vec2, f32)> {
    let (center, along, inward) = frame(lot);
    let local: Vec<Vec2> = lot.polygon.iter().map(|p| {
        let d = *p - center;
        Vec2::new(d.dot(along), d.dot(inward))
    }).collect();
    let yaw0 = along.y.atan2(along.x);
    bays_local(&local).into_iter().map(|(p, yaw)| (center + along * p.x + inward * p.y, yaw0 + yaw)).collect()
}

/// Spaces a surface lot on `lot` has: its bays.
pub fn surface_capacity(lot: &Lot) -> u16 {
    surface_bays(lot).len().min(u16::MAX as usize) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, d: f32) -> Vec<Vec2> {
        vec![Vec2::new(-w * 0.5, -d * 0.5), Vec2::new(w * 0.5, -d * 0.5), Vec2::new(w * 0.5, d * 0.5), Vec2::new(-w * 0.5, d * 0.5)]
    }

    #[test]
    fn a_rectangle_packs_close_to_its_area() {
        // 30 x 20 m: two double rows across the 20 m would need 32 m; one
        // double row (16 m) with 11 bays each side.
        let n = bays_local(&rect(30.0, 20.0)).len();
        assert!(n >= 20, "{n} bays in 600 m2");
        let big = bays_local(&rect(40.0, 34.0)).len();
        assert!(big >= 50, "{big} bays in 1,360 m2");
        assert!(bays_local(&rect(4.0, 4.0)).is_empty());
    }

    #[test]
    fn bays_do_not_overlap_and_stay_inside() {
        let poly = vec![Vec2::new(0.0, 0.0), Vec2::new(36.0, 0.0), Vec2::new(30.0, 28.0), Vec2::new(4.0, 22.0)];
        let b = bays_local(&poly);
        assert!(b.len() >= 15, "{}", b.len());
        for (i, (p, _)) in b.iter().enumerate() {
            assert!(crate::geom::point_in_polygon(*p, &poly));
            for (q, _) in &b[i + 1..] {
                assert!(p.distance(*q) > BAY_W * 0.99, "{p} {q}");
            }
        }
    }
}
