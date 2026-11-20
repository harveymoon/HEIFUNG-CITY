//! [r11-lot-memory] Ghosts only on walls that really stood against the old
//! ground, as high as the wall; slabs inside their lot, gone with the
//! weeds; painted-over signs on most styles; all deterministic.

use super::*;
use crate::surface::Surface;
use nd_sim::buildings::DemolitionReason;
use nd_sim::clock::MINUTES_PER_DAY;

/// A 4-storey house that stood on x 0..10, z 0..12.
fn trace(floors: u8, seed: u64) -> Trace {
    Trace {
        id: 1,
        kind: TraceKind::Demolished,
        tick: 0,
        built: 0,
        at: Vec2::new(5.0, 6.0),
        outline: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 12.0), Vec2::new(0.0, 12.0)],
        floors,
        land_use: LandUse::Residential,
        business: None,
        name: "Rust Row".into(),
        seed,
        reason: DemolitionReason::Bulldozed,
        unit: None,
        now: None,
        building: BuildingId::default(),
    }
}

/// A brick block from x `x0..x1`, z 0..12, `h` high.
fn block(x0: f32, x1: f32, h: f32) -> Geometry {
    let mut g = Geometry::default();
    g.part(Brick).boxed(Vec3::new((x0 + x1) * 0.5, h * 0.5, 6.0), Vec3::new(x1 - x0, h, 12.0), 0.0, [0.4, 0.2, 0.15, 1.0]);
    g
}

fn verts(g: &Geometry) -> Vec<[f32; 3]> {
    g.parts.iter().flat_map(|m| m.positions.iter().copied()).collect()
}

#[test]
fn a_ghost_on_the_wall_that_stood_against_it() {
    let t = trace(4, 11);
    // The neighbour's wall 0.5 m off the shared line (x = 0), facing it.
    let g = block(-9.5, -0.5, 18.0);
    let ghost = wall_ghosts(&g, &[&t], 0);
    assert!(ghost.triangle_count() > 20, "a ghost: {}", ghost.triangle_count());
    for p in verts(&ghost) {
        assert!(p[0] > -0.5 && p[0] < -0.35, "on the wall face, just proud of it: {p:?}");
        assert!(p[1] >= 0.0 && p[1] <= 18.0, "no higher than the wall: {p:?}");
        assert!(p[2] >= 0.0 && p[2] <= 12.0, "along the old house's side: {p:?}");
    }
    // Over the old roof line nothing: 4 storeys and a roof at most.
    assert!(verts(&ghost).iter().all(|p| p[1] <= 4.0 * 3.1 + 4.5 + 1.2));
}

#[test]
fn no_ghost_where_no_wall_abutted() {
    let t = trace(4, 11);
    // Set back 4 m from the line.
    assert_eq!(wall_ghosts(&block(-14.0, -4.0, 18.0), &[&t], 0).triangle_count(), 0, "a wall across a gap");
    // Across the street from it, not on its side.
    assert_eq!(wall_ghosts(&block(20.0, 30.0, 18.0), &[&t], 0).triangle_count(), 0, "a building further off");
    // Standing on the old ground itself (the successor): its walls face away.
    assert_eq!(wall_ghosts(&block(1.5, 8.5, 18.0), &[&t], 0).triangle_count(), 0, "the new building on the lot");
    // A car park or a park left nothing to see.
    assert_eq!(wall_ghosts(&block(-9.5, -0.5, 18.0), &[&trace(0, 11)], 0).triangle_count(), 0);
}

#[test]
fn a_ghost_is_held_under_a_lower_wall() {
    let t = trace(6, 3);
    let ghost = wall_ghosts(&block(-9.5, -0.5, 5.0), &[&t], 0);
    assert!(ghost.triangle_count() > 0);
    assert!(verts(&ghost).iter().all(|p| p[1] <= 5.0), "under the neighbour's top");
}

#[test]
fn ghosts_are_deterministic_and_fade() {
    let t = trace(3, 99);
    let g = block(-9.5, -0.5, 14.0);
    let a = wall_ghosts(&g, &[&t], 0);
    let b = wall_ghosts(&g, &[&t], 0);
    assert_eq!(verts(&a), verts(&b));
    let wall = [0.4f32, 0.2, 0.15];
    let off = |g: &Geometry| -> f32 {
        let m = g.get(Surface::WornPlaster);
        m.colors.iter().map(|c| (0..3).map(|k| (c[k] - wall[k]).abs()).sum::<f32>()).sum::<f32>() / m.colors.len().max(1) as f32
    };
    let late = wall_ghosts(&g, &[&t], 10 * 7 * MINUTES_PER_DAY);
    assert!(late.triangle_count() > 0, "a faint scar stays");
    assert!(off(&late) < off(&a) * 0.5, "weeks on it has faded towards the wall: {} vs {}", off(&late), off(&a));
}

#[test]
fn a_slab_lies_inside_its_lot_until_the_weeds_have_it() {
    let t = trace(3, 5);
    let pal = Palette::default();
    let lot = t.outline.clone();
    let mut g = Geometry::default();
    slab(&mut g, &lot, &t, 0, &pal);
    assert!(g.get(Surface::WornConcrete).indices.len() / 3 > 20, "a slab");
    for p in verts(&g) {
        assert!(p[0] > 0.0 && p[0] < 10.0 && p[2] > 0.0 && p[2] < 12.0, "inside the lot: {p:?}");
        assert!(p[1] < 1.0);
    }
    let mut later = Geometry::default();
    slab(&mut later, &lot, &t, SLAB_STAGES - 2, &pal);
    assert!(later.get(Surface::WornConcrete).indices.len() < g.get(Surface::WornConcrete).indices.len() / 2, "the weeds take it");
    let mut gone = Geometry::default();
    slab(&mut gone, &lot, &t, SLAB_STAGES, &pal);
    assert_eq!(gone.triangle_count(), 0);
    let mut again = Geometry::default();
    slab(&mut again, &lot, &t, 0, &pal);
    assert_eq!(verts(&again), verts(&g), "deterministic");
}

#[test]
fn sign_text_keeps_what_the_stencil_draws() {
    assert_eq!(sign_text("Lucky Bowl #2!"), "LUCKY BOWL 2");
    assert_eq!(sign_text("a very long shop name indeed"), "A VERY LONG");
    assert_eq!(sign_text("Supercalifragilistic"), "SUPERCALIFRAGI");
}

/// A painted-over sign (and its patch) on most styles' street fronts, low
/// down; none without one.
#[test]
fn painted_over_signs_on_the_front() {
    use crate::buildings::*;
    use nd_sim::world::{World, WorldConfig};
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-150.0, 8.0), Vec2::new(150.0, 8.0), Vec2::new(150.0, 80.0), Vec2::new(-150.0, 80.0)], LandUse::Commercial);
    let mut lots: Vec<nd_sim::Lot> = w.lots.values().filter(|l| l.usable()).cloned().collect();
    lots.sort_by(|a, b| b.area().total_cmp(&a.area()));
    let lot = &lots[0];
    let (rules, pal) = (BuildingRules::default(), Palette::default());
    let signs = [GhostSign { unit: None, text: "NOODLE KING".into(), seed: 4, stage: 0, shut: true }];
    let mut drawn = 0;
    for seed in 0..12u64 {
        let plain = generate_building(lot, LandUse::Commercial, 3, nd_sim::Tier::Low, 0.6, seed, &rules, &pal, &Extras::default());
        let ghosted = generate_building(lot, LandUse::Commercial, 3, nd_sim::Tier::Low, 0.6, seed, &rules, &pal, &Extras { ghost_signs: &signs, ..Extras::default() });
        let matte = |g: &Geometry| g.get(Surface::Matte).positions.len();
        if matte(&ghosted) > matte(&plain) {
            drawn += 1;
            let new = &ghosted.get(Surface::Matte).positions[matte(&plain)..];
            assert!(new.iter().all(|p| p[1] > 1.8 && p[1] < 5.0), "above the shopfront, under the eaves: seed {seed}");
        }
    }
    assert!(drawn >= 8, "on most styles: {drawn} of 12");
}
