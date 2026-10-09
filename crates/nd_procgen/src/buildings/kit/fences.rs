//! Fences.

use super::super::Ctx;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};

/// Post-and-rail fence from `a` to `b` (lot-local).
pub(crate) fn rail_fence(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let n = (len / 3.0).ceil() as i32;
    for k in 0..=n {
        let p = a.lerp(b, k as f32 / n as f32);
        f.bx(g, Wood, p.x, h * 0.5, p.y, Vec3::new(0.14, h, 0.14), color);
    }
    let yaw = (b - a).y.atan2((b - a).x);
    let m = (a + b) * 0.5;
    for y in [h * 0.45, h * 0.85] {
        f.bx_yaw(g, Wood, m.x, y, m.y, Vec3::new(len, 0.1, 0.06), color, yaw);
    }
}

