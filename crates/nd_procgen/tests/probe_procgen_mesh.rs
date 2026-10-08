//! Mesh soundness: the October review's procgen probes (`review/oct`:
//! L10 flipped quads, L11 the ship's doubled deck, L12 a zero-width slab)
//! turned into regression tests, plus the tunnel portals and the highway's
//! barriers over a tunnel.
use glam::{Vec2, Vec3};
use nd_procgen::buildings::{generate_styled, Style};
use nd_procgen::street::{generate_street, StreetInput, StreetLayer};
use nd_procgen::{BuildingRules, MeshData, Palette, Surface};
use nd_sim::roads::{RoadKind, RoadNetwork};
use nd_sim::zoning::{generate_lots, Zone, ZoneId};
use nd_sim::{Gesture, LandUse, Lot, Tier, World, WorldConfig};
use slotmap::SlotMap;

/// Triangles whose winding disagrees with their vertices' stored normals.
fn flipped(m: &MeshData) -> usize {
    m.indices
        .chunks(3)
        .filter(|t| {
            let p: Vec<Vec3> = t.iter().map(|&i| Vec3::from(m.positions[i as usize])).collect();
            let g = (p[1] - p[0]).cross(p[2] - p[0]);
            let n: Vec3 = t.iter().map(|&i| Vec3::from(m.normals[i as usize])).sum();
            g.length() > 1e-5 && n.length() > 1e-6 && g.normalize().dot(n.normalize()) < -0.2
        })
        .count()
}

fn finite(m: &MeshData) -> bool {
    m.positions.iter().chain(m.normals.iter()).all(|p| p.iter().all(|v| v.is_finite())) && m.uvs.iter().all(|p| p.iter().all(|v| v.is_finite()))
}

fn lots(lu: LandUse) -> Vec<Lot> {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
    let zone = Zone { polygon: vec![Vec2::new(-300.0, -20.0), Vec2::new(300.0, -20.0), Vec2::new(300.0, 180.0), Vec2::new(-300.0, 180.0)], land_use: lu };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    lots.values().filter(|l| l.usable()).cloned().collect()
}

/// L10: quad normals came from corners 0, 1 and 3 only, so a twisted or
/// concave quad lit one of its triangles from behind (772 of 21,680 on a
/// twist tower). Every triangle now agrees with its normal.
#[test]
fn twisted_and_concave_quads_are_lit_from_the_front() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    for (style, lu) in [(Style::TwistTower, LandUse::Commercial), (Style::GardenTower, LandUse::Residential), (Style::Tenement, LandUse::Residential)] {
        let (mut bad, mut tris) = (0, 0);
        let mut by: std::collections::BTreeMap<String, usize> = Default::default();
        for (li, lot) in lots(lu).iter().enumerate() {
            let g = generate_styled(lot, lu, 14, Tier::High, 0.9, 7 + li as u64, style, &rules, &pal);
            for (s, m) in g.iter() {
                assert!(finite(m), "{style:?}: NaN");
                let f = flipped(m);
                bad += f;
                if f > 0 {
                    *by.entry(format!("{s:?}")).or_default() += f;
                }
            }
            tris += g.triangle_count();
        }
        assert!(tris > 1000, "{style:?} drew {tris}");
        // The twist tower's floor bands were the quads at fault. A garden
        // tower still has a few dozen in 85,000 (in its caps and terrace
        // glass, not quads): kept from growing, not yet chased down.
        if style == Style::TwistTower {
            assert_eq!(bad, 0, "{style:?}: {bad} of {tris} triangles lit from behind: {by:?}");
        } else {
            assert!(bad * 2000 < tris, "{style:?}: {bad} of {tris} triangles lit from behind: {by:?}");
        }
    }
    // The quads themselves: twisted, concave (either diagonal) and flat.
    let w = [1.0; 4];
    for c in [
        [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.4, 1.0), Vec3::Z],
        [Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.3, 0.3, 0.0), Vec3::new(0.0, 2.0, 0.0)],
        [Vec3::new(0.3, 0.3, 0.0), Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0)],
        [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y],
    ] {
        let mut m = MeshData::default();
        m.quad(c, w);
        assert!(finite(&m) && m.indices.len() == 6 && flipped(&m) == 0, "{c:?}");
    }
}

/// L11 and L12: the ship's deck no longer shares a plane with the hull's
/// cap (it z-fought), and a slab of no width draws nothing instead of NaN.
#[test]
fn the_ship_deck_and_a_zero_slab_are_sound() {
    let pal = Palette::default();
    for freighter in [true, false] {
        let m = nd_procgen::terrain::ship_mesh(freighter, &pal);
        assert!(finite(&m));
        // Upward faces at the top of the hull: one plane each, not two.
        let tops: Vec<f32> = m.indices.chunks(3).filter_map(|t| {
            let p: Vec<Vec3> = t.iter().map(|&i| Vec3::from(m.positions[i as usize])).collect();
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            (n.length() > 1e-4 && n.normalize().y > 0.99 && (p[0].y - 2.5).abs() < 0.2).then_some(p[0].y)
        }).collect();
        let mut planes: Vec<f32> = Vec::new();
        for y in tops {
            if !planes.iter().any(|q| (q - y).abs() < 1e-3) {
                planes.push(y);
            }
        }
        assert!(planes.len() == 2 && (planes[0] - planes[1]).abs() > 0.03, "deck planes {planes:?}");
    }
    let mut m = MeshData::default();
    m.slab(Vec3::ZERO, Vec3::new(10.0, 1.0, 0.0), 0.0, 0.4, [1.0; 4]);
    m.slab(Vec3::ZERO, Vec3::new(10.0, 1.0, 0.0), 2.0, 0.0, [1.0; 4]);
    assert!(m.positions.is_empty() && finite(&m));
}

fn tunnel_under_the_highway() -> World {
    let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    // A three-lane one-way carriageway and a tunnel passing under it.
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 3, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
    let t = nd_sim::roads::TUNNEL_DEPTH;
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(300.0, -300.0), Vec2::new(300.0, -150.0), Vec2::new(300.0, 150.0), Vec2::new(300.0, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, t, t, 0.0] });
    // A one-way tunnel along under the carriageway, as a ramp's might.
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(-200.0, -60.0), Vec2::new(-150.0, -6.0), Vec2::new(150.0, -6.0), Vec2::new(200.0, -60.0)], lanes_fwd: 3, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: vec![0.0, t, t, 0.0] });
    w.implement();
    w
}

/// A tunnel passing under the highway leaves its barriers standing: they
/// used to stop wherever another motorway's lanes lay beneath, whatever
/// its depth.
#[test]
fn barriers_stand_over_a_tunnel() {
    let w = tunnel_under_the_highway();
    assert!(w.roads.alive_segments().any(|(_, s)| s.tunnel));
    let pal = Palette::default();
    let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: 3, heights: &w.road_z, inner: &w.road_inner, layer: StreetLayer::Surface, closed: &[] });
    let hz = w.roads.nearest_where(Vec2::new(0.0, 0.0), |s| s.is_highway()).map(|(sid, t, _)| w.road_height(sid, t)).unwrap();
    // Barrier tops (0.8 m over the carriageway) along both edges of the
    // carriageway, from x = -120 to 120: no gap wider than a few metres.
    let hw = w.roads.alive_segments().find(|(_, s)| s.is_highway()).map(|(_, s)| s.half_width()).unwrap();
    for side in [-1.0f32, 1.0] {
        let mut xs: Vec<f32> = [Surface::Concrete, Surface::WornConcrete]
            .iter()
            .flat_map(|s| g.get(*s).positions.iter().copied())
            .filter(|p| (p[1] - hz - 0.8).abs() < 0.05 && (p[2] - side * (hw + 0.2)).abs() < 0.4 && p[0].abs() < 120.0)
            .map(|p| p[0])
            .collect();
        xs.sort_by(|a, b| a.total_cmp(b));
        assert!(xs.len() > 20, "no barrier on side {side}");
        let gap = xs.windows(2).map(|w| w[1] - w[0]).fold(0.0, f32::max);
        assert!(gap < 6.0, "a {gap:.1} m gap in the barrier on side {side}");
    }
}

/// Tunnel mouths are concrete arches, sound and wide enough for the bore.
#[test]
fn tunnel_portals_are_arches() {
    let w = tunnel_under_the_highway();
    let mut g = nd_procgen::Geometry::default();
    nd_procgen::terrain::tunnel_works(&w, &mut g, [0.5; 3], [0.4; 3]);
    let m = g.get(Surface::Concrete);
    assert!(finite(m) && flipped(m) == 0);
    // Around the south portal of the crossing tunnel at (300, -150): a
    // headwall reaching well over the road and spanning past its walls,
    // with the opening clear over the carriageway.
    let portal = Vec2::new(300.0, -150.0);
    let z = w.roads.node_near(portal, 1.0).map(|n| w.road_z[n.0 as usize]).unwrap();
    let near: Vec<[f32; 3]> = m.positions.iter().copied().filter(|p| Vec2::new(p[0], p[2]).distance(portal) < 12.0).collect();
    let top = near.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let span = near.iter().map(|p| p[0]).fold(f32::MIN, f32::max) - near.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
    assert!(top - z > 6.5 && span > 10.0, "portal {top} over {z}, {span} m across");
    // Nothing of the wall in the opening over the lanes (up to 5 m).
    let lanes = 3.5;
    assert!(!m.indices.chunks(3).any(|t| t.iter().all(|&i| {
        let p = m.positions[i as usize];
        (p[0] - 300.0).abs() < lanes && (p[2] + 150.0).abs() < 1.0 && p[1] > z + 0.5 && p[1] < z + 5.0
    })), "the arch closes over the carriageway");
}
