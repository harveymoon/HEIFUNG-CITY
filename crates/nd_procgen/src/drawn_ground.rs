//! [r16-ground] The one answer to "how high is the ground here" for
//! anything the renderer sets on it: people, props, fire crews, camps,
//! vents, marks, lots and parks laid on the ground.
//!
//! Two heights exist on purpose:
//! - the **sim's** ground, `World::ground_height` (the built ground: a 4 m
//!   raster graded to the ways and lots, read bilinearly), with
//!   `World::surface_height` / `surface_height_near` adding decks and the
//!   water and `World::car_height_at` / `road_height` the ways. The sim
//!   decides with these (grades, pads, flooding, what a walker can reach);
//!   they never change for the renderer's sake.
//! - the **drawn** ground: the landscape mesh (`terrain::LandscapeTiles`)
//!   samples the sim's ground only every 8 m and runs straight between,
//!   held down under the streets and cut out under levelled lots, benches,
//!   pits and covered shore (each of those drawn from the sim's own heights
//!   by its own mesh). On a hillside the two part by up to metres between
//!   the 8 m vertices (`examples/ground_height_probe.rs` counts it):
//!   walkers, props and crews set at the sim's height floated over the
//!   slope or sank into it.
//!
//! [`DrawnGround`] is the second: the mesh's own triangles where the mesh is
//! what shows, the sim's ground where another surface is drawn from it (a
//! cut-out lot's floor, a street's carriageway and pavement, a bench). The
//! renderer installs the one it draws ([`install`]); [`height`], [`near`]
//! and [`lie`] answer from it for that world and fall back to the sim's
//! answer anywhere else (headless, a preview world, before the first draw).
//! `nd_procgen::terrain::terrain_height` is neither: it is the flat map's
//! natural landscape the sim's ground is graded from. `World::ground_at`
//! (industry) is not a height at all but the soil (fertility, oil, rock).

use crate::holes::HoleSet;
use glam::Vec2;
use nd_sim::World;
use std::sync::{Arc, RwLock};

/// The drawn landscape ground: the mesh's vertex heights as drawn (held
/// under the streets), the sim's at the same vertices (to tell which were
/// held), and the holes cut through it.
pub struct DrawnGround {
    half: f32,
    cell: f32,
    n: usize,
    raw: Vec<f32>,
    kept: Vec<f32>,
    holes: HoleSet,
    /// Every levelled lot's outline and floor (`lot_ground::lot_floor`),
    /// binned by 32 m square.
    lots: Vec<(Vec<Vec2>, f32, Option<nd_sim::landscape::PadStep>)>,
    lot_bins: std::collections::HashMap<(i32, i32), Vec<u32>>,
    /// The world it was drawn for: its landscape's version base (a loaded
    /// or new city has a new one; the sim thread's copies keep it) and its
    /// land's half size.
    key: (u64, u32),
}

/// How far the mesh's vertices sit under the sim's ground
/// (`terrain::landscape_heights`).
const DRAWN_UNDER: f32 = 0.08;

fn key_of(w: &World) -> (u64, u32) {
    (w.landscape.version() >> 32, w.landscape.land_half().to_bits())
}

impl DrawnGround {
    /// From the mesh's own heights (`raw`: the sim's ground at each vertex,
    /// `kept`: as drawn) on an `n` by `n` grid of `cell` from `-half`.
    pub(crate) fn from_grid(w: &World, half: f32, cell: f32, n: usize, raw: Vec<f32>, kept: Vec<f32>, holes: HoleSet) -> Self {
        let (lots, lot_bins) = lot_floors(w);
        Self { half, cell, n, raw, kept, holes, lots, lot_bins, key: key_of(w) }
    }

    /// The ground the renderer draws for `w` at the 8 m cell, built afresh
    /// (probes, tests; the game takes it from its tiles).
    pub fn new(w: &World) -> Self {
        crate::terrain::drawn_ground(w, crate::lot_ground::LAND_CELL)
    }

    fn fits(&self, w: &World) -> bool {
        self.key == key_of(w) && self.n >= 2
    }

    /// The floor of the levelled lot `p` is in, if any: `Some(None)` on
    /// a lot's edge (within [`LOT_EDGE`]), where which floor is meant is
    /// the caller's (a lot's own mesh, or its neighbour's).
    fn lot_floor(&self, p: Vec2) -> Option<Option<f32>> {
        let b = |v: f32| (v / LOT_BIN).floor() as i32;
        let v = self.lot_bins.get(&(b(p.x), b(p.y)))?;
        let mut inside = None;
        for (poly, pad, step) in v.iter().map(|&k| &self.lots[k as usize]) {
            let edge = (0..poly.len()).any(|i| nd_sim::roads::project_on_segment(p, poly[i], poly[(i + 1) % poly.len()]).1.distance(p) < LOT_EDGE);
            if edge {
                return Some(None);
            }
            if inside.is_none() && nd_sim::geom::point_in_polygon(p, poly) {
                inside = Some(step.map_or(*pad, |s| s.pad_at(p, *pad)));
            }
        }
        inside.map(Some)
    }

    /// The ground as drawn at `p` (see the module notes).
    pub fn height(&self, w: &World, p: Vec2) -> f32 {
        if !w.relief || !self.fits(w) {
            return w.ground_height(p);
        }
        // A levelled lot: its floor, to its edge (cut out of the mesh and
        // capped there, or with the mesh within `CUT_FROM` of it). The
        // sim's 4 m raster runs a slope over the last cell to a
        // neighbour's pad or the hill, where a wall is drawn.
        match self.lot_floor(p) {
            Some(Some(f)) => return f,
            // On the edge: the sim's ground, as before.
            Some(None) => return w.ground_height(p),
            None => {}
        }
        // Other holes (benches, pits, covered shore): their own meshes
        // follow the sim's ground.
        if self.holes.contains(p) {
            return w.ground_height(p);
        }
        let fx = (p.x + self.half) / self.cell;
        let fy = (p.y + self.half) / self.cell;
        let (i, j) = (fx.floor() as i64, fy.floor() as i64);
        if i < 0 || j < 0 || i as usize + 1 >= self.n || j as usize + 1 >= self.n {
            return w.ground_height(p);
        }
        let (i, j) = (i as usize, j as usize);
        let (u, v) = (fx - i as f32, fy - j as f32);
        let n = self.n;
        // The triangles a-c-b and b-c-d, split along b-c as drawn.
        let (ka, kb, kc, kd) = (j * n + i, j * n + i + 1, (j + 1) * n + i, (j + 1) * n + i + 1);
        let tri = if u + v <= 1.0 { [ka, kb, kc] } else { [kd, kc, kb] };
        // A street holds this triangle down: on its carriageway or
        // pavement the street's own surface shows, drawn at the sim's.
        if tri.iter().any(|&k| self.kept[k] < self.raw[k] - 1e-4) && on_street(w, p) {
            return w.ground_height(p);
        }
        let h = |k: usize| self.kept[k] + DRAWN_UNDER;
        if u + v <= 1.0 {
            h(ka) + (h(kb) - h(ka)) * u + (h(kc) - h(ka)) * v
        } else {
            h(kd) + (h(kc) - h(kd)) * (1.0 - u) + (h(kb) - h(kd)) * (1.0 - v)
        }
    }

    /// The drawn ground, or the water's surface over it.
    fn ground_or_water(&self, w: &World, p: Vec2) -> f32 {
        let g = self.height(w, p);
        if !w.landscape.is_flat() && w.landscape.is_water(p) { g.max(nd_sim::landscape::WATER_LEVEL) } else { g }
    }

    /// `World::surface_height_near` on the drawn ground: the surface at
    /// `p` nearest the height `hint`, the ground or a deck over it.
    pub fn near(&self, w: &World, p: Vec2, hint: f32) -> f32 {
        if !w.relief {
            return 0.0;
        }
        let ground = self.ground_or_water(w, p);
        w.ground.decks_at(p).into_iter().fold(ground, |best, z| if (z - hint).abs() < (best - hint).abs() { z } else { best })
    }

    /// Where something is laid on the ground at `p`: the drawn ground,
    /// under a viaduct still the ground (over the water, a bridge deck is
    /// the nearest surface).
    pub fn lie(&self, w: &World, p: Vec2) -> f32 {
        let g = self.height(w, p);
        self.near(w, p, g)
    }
}

const LOT_BIN: f32 = 32.0;

/// Metres from a lot's edge within which a point is on it.
const LOT_EDGE: f32 = 0.25;

/// The levelled lots as `lot_ground` floors them: every lot with a pad but
/// a bare park or a strip mine's pit.
#[allow(clippy::type_complexity)]
fn lot_floors(w: &World) -> (Vec<(Vec<Vec2>, f32, Option<nd_sim::landscape::PadStep>)>, std::collections::HashMap<(i32, i32), Vec<u32>>) {
    let mut lots = Vec::new();
    let mut bins: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::new();
    if !w.relief || w.landscape.is_flat() {
        return (lots, bins);
    }
    let pits = crate::holes::holed_lots(w);
    let b = |v: f32| (v / LOT_BIN).floor() as i32;
    for (id, l) in w.lots.iter() {
        if l.polygon.len() < 3 || (l.building.is_none() && l.park.is_some()) || pits.contains(&id) {
            continue;
        }
        let Some((pad, step)) = crate::lot_ground::lot_floor(w, id) else { continue };
        let (lo, hi) = nd_sim::geom::bounds(&l.polygon);
        let k = lots.len() as u32;
        for j in b(lo.y)..=b(hi.y) {
            for i in b(lo.x)..=b(hi.x) {
                bins.entry((i, j)).or_default().push(k);
            }
        }
        lots.push((l.polygon.clone(), pad, step));
    }
    (lots, bins)
}

/// Is `p` on the carriageway or pavement of a street on the ground (one
/// that holds the mesh down, `terrain::street_hold_reach`)?
fn on_street(w: &World, p: Vec2) -> bool {
    let r = Vec2::splat(nd_sim::roads::PAVEMENT_REACH_MAX);
    w.roads.segments_in_box(p - r, p + r).any(|sid| {
        let Some(s) = w.roads.try_segment(sid) else { return false };
        if s.is_rail() || s.bridge || s.viaduct || s.tunnel {
            return false;
        }
        let (a, b) = w.roads.segment_points(sid);
        nd_sim::roads::project_on_segment(p, a, b).1.distance(p) <= s.half_width() + nd_sim::zoning::SIDEWALK
    })
}

static CURRENT: RwLock<Option<Arc<DrawnGround>>> = RwLock::new(None);
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The renderer's drawn ground, from now on the answer for its world.
pub fn install(d: DrawnGround) {
    if let Ok(mut c) = CURRENT.write() {
        *c = Some(Arc::new(d));
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Forget the installed ground (a flat map draws none).
pub fn clear() {
    if let Ok(mut c) = CURRENT.write() {
        if c.take().is_some() {
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Bumped whenever the installed ground changes: whatever was set on the
/// old one is set again.
pub fn serial() -> u64 {
    SERIAL.load(std::sync::atomic::Ordering::Relaxed)
}

/// The installed drawn ground if it was drawn for `w`. Take it once for a
/// loop over many things.
pub fn current(w: &World) -> Option<Arc<DrawnGround>> {
    CURRENT.read().ok().and_then(|c| c.as_ref().filter(|d| d.fits(w)).cloned())
}

/// The ground as drawn at `p` (the sim's ground where nothing is drawn
/// for `w`).
pub fn height(w: &World, p: Vec2) -> f32 {
    current(w).map_or_else(|| w.ground_height(p), |d| d.height(w, p))
}

/// The drawn surface at `p` nearest `hint` (`World::surface_height_near`).
pub fn near(w: &World, p: Vec2, hint: f32) -> f32 {
    current(w).map_or_else(|| w.surface_height_near(p, hint), |d| d.near(w, p, hint))
}

/// Where something is laid on the ground at `p` (see [`DrawnGround::lie`]).
pub fn lie(w: &World, p: Vec2) -> f32 {
    current(w).map_or_else(|| w.surface_height_near(p, w.ground_height(p)), |d| d.lie(w, p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Palette;

    fn built_demo() -> World {
        let mut w = World::new(nd_sim::WorldConfig::default());
        nd_sim::scenarios::demo_district(&mut w);
        let empty: Vec<nd_sim::LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.park.is_none()).map(|(id, _)| id).collect();
        for l in empty {
            w.build_on(l);
        }
        w
    }

    /// [r16-ground] Wherever the ground mesh is what shows (no lot, hole or
    /// street there), the drawn ground is the mesh's own triangle; inside a
    /// levelled lot it is the lot's floor, edge to edge; and the answer
    /// stays the sim's for another world or with nothing installed.
    #[test]
    fn the_drawn_ground_is_the_mesh_and_the_lot_floors() {
        let w = built_demo();
        assert!(w.relief);
        let d = DrawnGround::new(&w);
        let mesh = crate::terrain::build_landscape_terrain(&w, &Palette::default(), crate::lot_ground::LAND_CELL);
        let mut checked = 0;
        for t in mesh.indices.chunks_exact(3).step_by(13) {
            let p: Vec<glam::Vec3> = t.iter().map(|&i| glam::Vec3::from(mesh.positions[i as usize])).collect();
            let c = (p[0] + p[1] + p[2]) / 3.0;
            let q = Vec2::new(c.x, c.z);
            if d.lot_floor(q).is_some() || d.holes.contains(q) || on_street(&w, q) {
                continue;
            }
            let h = d.height(&w, q);
            assert!((h - (c.y + DRAWN_UNDER)).abs() < 0.02, "at ({:.1},{:.1}): mesh {:.2}, drawn {h:.2}", q.x, q.y, c.y);
            checked += 1;
        }
        assert!(checked > 200, "checked {checked}");
        // Lots: the floor, also where the sim's raster slopes to the edge.
        let mut lots = 0;
        for (id, l) in w.lots.iter() {
            let Some((pad, None)) = crate::lot_ground::lot_floor(&w, id) else { continue };
            if l.park.is_some() || l.polygon.len() < 3 {
                continue;
            }
            let c = l.center;
            for v in &l.polygon {
                // A metre in from each corner.
                let q = *v + (c - *v).normalize_or_zero() * 1.0;
                if l.contains(q) && d.lot_floor(q) == Some(Some(pad)) {
                    assert_eq!(d.height(&w, q), pad);
                    lots += 1;
                }
            }
        }
        assert!(lots > 20, "lot corners checked {lots}");
        // Another world (a fresh landscape) gets the sim's answer.
        let other = built_demo();
        let p = Vec2::new(500.0, 500.0);
        install(DrawnGround::new(&w));
        assert!(current(&w).is_some());
        assert!(current(&other).is_none());
        assert_eq!(height(&other, p), other.ground_height(p));
    }
}
