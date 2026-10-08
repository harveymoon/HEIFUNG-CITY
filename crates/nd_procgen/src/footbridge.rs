//! [tools] Footbridges over the street (`nd_sim::walk::Footbridge`): a
//! walkway deck on two columns at `level` above the pavement, glass
//! balustrades with steel rails and a neon strip along each edge, and a
//! flight of stairs from each end down to the pavement, with stringers and
//! a handrail. Built from the walk graph, so what walkers climb is what is
//! drawn.

use crate::buildings::parts::{mul, rgba};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::World;

/// Rise of one step, metres.
const STEP_RISE: f32 = 0.3;
/// Depth of the walkway deck.
const DECK_DEPTH: f32 = 0.45;
/// Balustrade height.
const RAIL: f32 = 1.1;

/// Every footbridge of the walk graph.
pub fn footbridges(world: &World, palette: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let concrete = mul(palette.concrete(0), 0.9);
    let steel = palette.steel;
    let neon = [0.25, 1.4, 1.8];
    let ground = |p: Vec2| world.surface_height(p);
    for fb in &world.walk.footbridges {
        let w = fb.width;
        let [t0, t1] = fb.top;
        let z = ground(t0).max(ground(t1)) + fb.level;
        let a = Vec3::new(t0.x, z, t0.y);
        let b = Vec3::new(t1.x, z, t1.y);
        let across = (t1 - t0).normalize_or_zero();
        // The walkway, a little past the stair heads at each end.
        let ext = Vec3::new(across.x, 0.0, across.y) * (w * 0.5);
        g.part(Surface::Concrete).slab(a - ext, b + ext, w, DECK_DEPTH, rgba(concrete));
        g.part(Surface::Paving).slab(a - ext + Vec3::Y * 0.02, b + ext + Vec3::Y * 0.02, w - 0.3, 0.02, rgba(mul(concrete, 1.15)));
        let side = Vec3::new(-across.y, 0.0, across.x) * (w * 0.5 - 0.06);
        for s in [1.0f32, -1.0] {
            let o = side * s;
            // Glass panels, the steel top rail and posts, a neon strip.
            g.part(Surface::Glass).slab(a - ext + o + Vec3::Y * (RAIL * 0.5 + 0.05), b + ext + o + Vec3::Y * (RAIL * 0.5 + 0.05), 0.04, RAIL * 0.5, [0.55, 0.75, 0.85, 1.0]);
            g.part(Surface::Metal).beam(a - ext + o + Vec3::Y * RAIL, b + ext + o + Vec3::Y * RAIL, 0.08, rgba(steel));
            g.part(Surface::Glow).beam(a - ext + o - Vec3::Y * (DECK_DEPTH - 0.05), b + ext + o - Vec3::Y * (DECK_DEPTH - 0.05), 0.07, rgba(neon));
            let len = (b - a).length() + w;
            let posts = (len / 2.5).ceil().max(1.0) as usize;
            for k in 0..=posts {
                let p = (a - ext).lerp(b + ext, k as f32 / posts as f32) + o;
                g.part(Surface::Metal).beam(p, p + Vec3::Y * RAIL, 0.06, rgba(steel));
            }
        }
        // Stairs from each end down to its foot on the pavement.
        for (top, foot) in [(t0, fb.foot[0]), (t1, fb.foot[1])] {
            let gz = ground(foot);
            let rise = z - gz;
            let run = (foot - top).length();
            if run < 0.5 || rise < 0.5 {
                continue;
            }
            let along = (foot - top) / run;
            let yaw = along.y.atan2(along.x);
            let steps = (rise / STEP_RISE).ceil().max(1.0) as usize;
            let tread = run / steps as f32;
            for k in 0..steps {
                // Step k counted down from the top.
                let c = top + along * (tread * (k as f32 + 0.5));
                let h = z - rise * (k as f32 + 1.0) / steps as f32;
                g.part(Surface::Concrete).boxed(Vec3::new(c.x, h + STEP_RISE * 0.5 - 0.02, c.y), Vec3::new(tread + 0.02, STEP_RISE, w - 0.2), yaw, rgba(mul(concrete, 1.05)));
            }
            // The stringer slab under the steps.
            let top3 = Vec3::new(top.x, z - DECK_DEPTH, top.y);
            let foot3 = Vec3::new(foot.x, gz, foot.y);
            g.part(Surface::Concrete).slab(top3, foot3, w, 0.35, rgba(concrete));
            let sd = Vec3::new(-along.y, 0.0, along.x) * (w * 0.5 - 0.05);
            for s in [1.0f32, -1.0] {
                let o = sd * s;
                g.part(Surface::Metal).beam(top3 + o + Vec3::Y * (RAIL + DECK_DEPTH), foot3 + o + Vec3::Y * RAIL, 0.07, rgba(steel));
                g.part(Surface::Glow).beam(top3 + o + Vec3::Y * 0.05, foot3 + o + Vec3::Y * 0.05, 0.05, rgba(neon));
                for k in 0..=4 {
                    let p = top3.lerp(foot3, k as f32 / 4.0) + o;
                    g.part(Surface::Metal).beam(p, p + Vec3::Y * RAIL, 0.05, rgba(steel));
                }
            }
            // A column under the stair head, on the pavement.
            let col = top + along * 0.6;
            let cz = ground(col);
            g.part(Surface::Concrete).boxed(Vec3::new(col.x, (z - DECK_DEPTH + cz) * 0.5, col.y), Vec3::new(0.7, z - DECK_DEPTH - cz, 0.7), yaw, rgba(mul(concrete, 0.8)));
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::junctions::CrossingUpgrade;
    use nd_sim::{LandUse, WorldConfig};

    #[test]
    fn a_footbridge_is_drawn_up_at_its_walkway() {
        let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-200.0, 8.0), Vec2::new(200.0, 8.0), Vec2::new(200.0, 60.0), Vec2::new(-200.0, 60.0)], LandUse::Residential);
        let cr = w.walk.crossings.iter().position(|c| c.kind == nd_sim::walk::CrossingKind::Zebra).expect("a zebra") as u32;
        w.economy.treasury = 1.0e6;
        w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
        let g = footbridges(&w, &Palette::default());
        let top = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(f32::MIN, f32::max);
        assert!(top > nd_sim::junctions::FOOTBRIDGE_LEVEL, "the balustrade stands over the walkway: {top}");
        assert!(g.iter().any(|(s, m)| s == Surface::Glow && !m.positions.is_empty()), "neon strips");
    }

    /// [footbridge] The zebra (and its beacons) under a footbridge is not
    /// painted: the bridge replaced it.
    #[test]
    fn the_zebra_under_a_footbridge_is_not_painted() {
        use crate::street::{generate_street, StreetInput, StreetLayer};
        let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2, 2);
        w.add_zone(vec![Vec2::new(-200.0, 8.0), Vec2::new(200.0, 8.0), Vec2::new(200.0, 60.0), Vec2::new(-200.0, 60.0)], LandUse::Residential);
        let cr = w.walk.crossings.iter().position(|c| c.kind == nd_sim::walk::CrossingKind::Zebra).expect("a zebra") as u32;
        let mid = w.walk.crossings[cr as usize].mid;
        w.economy.treasury = 1.0e6;
        w.set_crossing_upgrade(cr, CrossingUpgrade::Footbridge).unwrap();
        let closed = w.closed_crosswalks();
        assert_eq!(closed, vec![mid]);
        let pal = Palette::default();
        let paint_near = |closed: &[Vec2]| {
            let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: 1, heights: &[], inner: &[], layer: StreetLayer::Surface, closed });
            // The street runs along x: the zebra's stripes lie across it.
            let paint = g.get(Surface::Paint).positions.iter().filter(|p| (p[0] - mid.x).abs() < 1.4 && (p[2] - mid.y).abs() < 9.0).count();
            let beacons = g.get(Surface::Glow).positions.iter().filter(|p| Vec2::new(p[0], p[2]).distance(mid) < 12.0).count();
            (paint, beacons)
        };
        let (paint, beacons) = paint_near(&[]);
        let (left, beacons_left) = paint_near(&closed);
        assert!(beacons > 0 && beacons_left == 0, "the beacons go: {beacons} -> {beacons_left}");
        assert!(paint >= left + 40, "the stripes go: {paint} -> {left}");
    }
}
