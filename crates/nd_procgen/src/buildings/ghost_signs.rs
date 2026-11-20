//! [r11-lot-memory] Signs painted over (`crate::lot_memory::GhostSign`):
//! a shop that changed trade or shut keeps its old name faintly through a
//! patch of fresh paint above the shopfront, an outline of screw holes
//! where the board hung; sometimes its door is bricked up, and a shopfront
//! that is no shop any more filled in with blockwork round a small window.
//! Placed off the geometry already drawn (the ground floor's extent), like
//! the shopfront signs, so it works on any style.

use super::parts::*;
use super::{Ctx, Face};
use crate::lot_memory::{ghost_letters, GhostSign};
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::hash::{mix2, unit};
use nd_sim::lot_memory::SIGN_STAGES;

/// Fresh paint over an old sign: off-white, grey, putty, slate.
const PAINTS: [[f32; 3]; 4] = [[0.58, 0.56, 0.52], [0.42, 0.43, 0.42], [0.50, 0.44, 0.37], [0.30, 0.33, 0.36]];
/// The old lettering: black, oxblood, bottle green, navy.
const INKS: [[f32; 3]; 4] = [[0.08, 0.07, 0.07], [0.32, 0.07, 0.06], [0.07, 0.20, 0.12], [0.08, 0.10, 0.24]];
/// Blockwork filling a shopfront.
const BLOCK: [f32; 3] = [0.46, 0.46, 0.44];

/// Draw `signs` on the street front of the building in `g`; `units` are
/// its units (a sign over a shopfront goes over that shop).
pub fn draw(g: &mut Geometry, c: &Ctx, signs: &[GhostSign], units: &[nd_sim::buildings::Unit]) {
    if signs.is_empty() {
        return;
    }
    let Some((lo, hi, top, solid)) = front_extent(g, c) else { return };
    let front = Face::from_edge(Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y));
    if front.len < 3.0 {
        return;
    }
    // Above the shop's sign (3.3 m), under the first-floor windows; on a
    // low shed or a stall, under its eaves.
    let y = 4.35f32.min(top - 0.8);
    if y < 2.2 {
        return;
    }
    let f = &c.frame;
    let shops: Vec<usize> = units.iter().enumerate().filter(|(_, u)| u.shopfront).map(|(i, _)| i).collect();
    let out = f.dir3(front.out);
    let read = (-out).cross(Vec3::Y).normalize_or_zero();
    for s in signs {
        // Over its shopfront (as `shopfronts::draw` places them), or the
        // middle of the front.
        let (t, seg) = match s.unit.and_then(|u| shops.iter().position(|i| *i == u as usize)) {
            Some(k) => {
                let seg = front.len / shops.len() as f32;
                (-front.len * 0.5 + seg * (k as f32 + 0.5), seg)
            }
            None => ((unit(mix2(s.seed, 1)) - 0.5) * front.len * 0.2, front.len),
        };
        let fits = |w: f32| c.all_inside(&[front.at(t - w * 0.5, 0.0), front.at(t + w * 0.5, 0.0)]);
        let w = (seg * 0.8).min(7.5);
        let w = if fits(w) { w } else if fits(w * 0.6) { w * 0.6 } else { continue };
        if w < 2.0 {
            continue;
        }
        let paint = PAINTS[(mix2(s.seed, 2) % PAINTS.len() as u64) as usize];
        let ink = INKS[(mix2(s.seed, 3) % INKS.len() as u64) as usize];
        let h = 0.85f32.min((top - y) * 2.0 - 0.1).max(0.5);
        front.decal(g, f, Matte, t, y, 0.03, w, h, paint);
        // The board's screw holes at its corners.
        for (dx, dy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            front.decal(g, f, Matte, t + dx * (w * 0.5 - 0.12), y + dy * (h * 0.5 - 0.1), 0.035, 0.07, 0.07, mul(paint, 0.45));
        }
        // The old name through the paint: faint, fainter by the week.
        if !s.text.is_empty() {
            let k = 0.55 - 0.35 * (s.stage.min(SIGN_STAGES) as f32 / SIGN_STAGES as f32);
            let n = s.text.len() as f32;
            let lh = ((w * 0.88) / ((4.0 * n - 1.0) / 5.0)).min(h * 0.62);
            let centre = f.at2(front.at(t, 0.04), y);
            ghost_letters(g, centre, read, out, lh, &s.text, mix(paint, ink, k), s.seed, 0.12 + 0.04 * s.stage as f32);
        }
        // Now and then the old shop door bricked up.
        // (Only in a wall: not on an open stall front.)
        if solid && unit(mix2(s.seed, 4)) < 0.35 {
            let td = t + if unit(mix2(s.seed, 5)) < 0.5 { -w * 0.32 } else { w * 0.32 };
            if c.all_inside(&[front.at(td - 0.6, 0.0), front.at(td + 0.6, 0.0)]) {
                let brick = mix(c.palette.brick.first().copied().unwrap_or([0.42, 0.2, 0.14]), paint, 0.2);
                front.bx(g, f, Brick, td, 1.2, 0.05, Vec3::new(1.1, 2.3, 0.1), brick);
                front.bx(g, f, Stone, td, 2.42, 0.06, Vec3::new(1.4, 0.18, 0.12), mix(paint, [0.6, 0.58, 0.52], 0.5));
            }
        }
        // A shop no longer: its front filled with blockwork round a small
        // high window.
        if solid && s.shut && unit(mix2(s.seed, 6)) < 0.55 {
            let fw = (w * 0.75).min(4.0);
            front.bx(g, f, WornConcrete, t, 1.3, 0.07, Vec3::new(fw, 2.2, 0.12), BLOCK);
            front.decal(g, f, Glass, t, 1.85, 0.14, (fw * 0.35).min(1.2), 0.6, [0.05, 0.06, 0.07]);
        }
    }
}

/// Walls, shutters and stall fronts: what a sign can be painted on.
const FRONTS: [Surface; 14] = [Concrete, Plaster, Brick, Stone, Panel, Glass, WornConcrete, WornPlaster, WornBrick, WornPanel, Metal, WornMetal, Wood, Rust];

/// The street front of what is drawn (lot-local; the street on its low-y
/// side) between 0.5 and 9 m up, the top of the building along it, and
/// whether a wall (not glass, a shutter or a stall) stands on it at the
/// ground.
fn front_extent(g: &Geometry, c: &Ctx) -> Option<(Vec2, Vec2, f32, bool)> {
    let mut lo = Vec2::splat(f32::MAX);
    let mut hi = Vec2::splat(f32::MIN);
    let pts: Vec<(Vec2, f32)> = FRONTS
        .iter()
        .filter_map(|s| g.parts.get(s.index()))
        .flat_map(|m| m.positions.iter())
        .map(|p| (c.frame.to_local(Vec2::new(p[0], p[2])), p[1]))
        .collect();
    for (l, y) in &pts {
        if *y >= 0.5 && *y <= 9.0 {
            lo = lo.min(*l);
            hi = hi.max(*l);
        }
    }
    if lo.x >= hi.x || lo.y >= hi.y {
        return None;
    }
    let top = pts.iter().filter(|(l, _)| l.y < lo.y + 1.5).map(|(_, y)| *y).fold(0.0f32, f32::max);
    const WALLS: [Surface; 9] = [Concrete, Plaster, Brick, Stone, Panel, WornConcrete, WornPlaster, WornBrick, WornPanel];
    let solid = WALLS
        .iter()
        .filter_map(|s| g.parts.get(s.index()))
        .flat_map(|m| m.positions.iter())
        .filter(|p| p[1] < 0.3)
        .filter(|p| c.frame.to_local(Vec2::new(p[0], p[2])).y < lo.y + 0.4)
        .count()
        >= 4;
    Some((lo, hi, top, solid))
}
