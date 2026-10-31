//! [economy] Tests for the new trades' and docks' looks.

use super::adjacency::{building_style, traits, Family};
use super::*;
use nd_sim::ports::PortKind;
use nd_sim::world::{World, WorldConfig};
use nd_sim::{BusinessKind as B, ShipKind};

/// Every new style, in the order they were appended.
const NEW: [Style; 19] = [
    Style::Mechanic,
    Style::CarDealer,
    Style::TrashCompany,
    Style::BikeShop,
    Style::CowFarm,
    Style::ChickenFarm,
    Style::Slaughterhouse,
    Style::StripMine,
    Style::FishGutting,
    Style::FishStickFactory,
    Style::FishMarket,
    Style::SeafoodRestaurant,
    Style::GolfRange,
    Style::ArtStudio,
    Style::Gallery,
    Style::MarinaSmall,
    Style::MarinaLarge,
    Style::FishingPortSmall,
    Style::FishingPortLarge,
];

/// Each new business and the look it must wear.
const LOOKS: [(B, Style); 15] = [
    (B::Mechanic, Style::Mechanic),
    (B::CarDealer, Style::CarDealer),
    (B::TrashCompany, Style::TrashCompany),
    (B::BikeShop, Style::BikeShop),
    (B::CowFarm, Style::CowFarm),
    (B::ChickenFarm, Style::ChickenFarm),
    (B::Slaughterhouse, Style::Slaughterhouse),
    (B::StripMine, Style::StripMine),
    (B::FishGutting, Style::FishGutting),
    (B::FishStickFactory, Style::FishStickFactory),
    (B::FishMarket, Style::FishMarket),
    (B::SeafoodRestaurant, Style::SeafoodRestaurant),
    (B::GolfRange, Style::GolfRange),
    (B::ArtStudio, Style::ArtStudio),
    (B::Gallery, Style::Gallery),
];

/// Lots of a zone beside a street: the largest, a typical and the oddest.
fn sample_lots(lu: LandUse) -> Vec<Lot> {
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-60.0, -100.0), Vec2::new(10.0, 200.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-150.0, 8.0), Vec2::new(150.0, 8.0), Vec2::new(150.0, 80.0), Vec2::new(-150.0, 80.0)], lu);
    let mut lots: Vec<Lot> = w.lots.values().filter(|l| l.usable()).cloned().collect();
    assert!(!lots.is_empty());
    lots.sort_by(|a, b| b.area().total_cmp(&a.area()));
    let mut out = vec![lots[0].clone(), lots[lots.len() / 2].clone()];
    if let Some(odd) = lots.iter().find(|l| l.polygon.len() != 4) {
        out.push(odd.clone());
    }
    out
}

fn valid(g: &Geometry, what: &str) {
    for (s, m) in g.iter() {
        assert_eq!(m.positions.len(), m.uvs.len(), "{what} {s:?} uv count");
        assert_eq!(m.positions.len(), m.normals.len(), "{what} {s:?} normal count");
        assert!(m.positions.iter().flatten().all(|v| v.is_finite()), "{what} {s:?} has NaN");
        assert!(m.normals.iter().flatten().all(|v| v.is_finite()), "{what} {s:?} has NaN normals");
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{what} {s:?} bad index");
    }
}

/// Every new style draws something, sound and within a modest budget, at
/// every tier, on a big lot, a typical one and an odd-shaped one; and the
/// same seed draws the same thing.
#[test]
fn new_styles_build_at_every_tier() {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = sample_lots(LandUse::Commercial);
    let mut worst = (0, Style::Mechanic);
    for style in NEW {
        for (i, lot) in lots.iter().enumerate() {
            for tier in [Tier::Low, Tier::Mid, Tier::High] {
                let g = generate_styled(lot, LandUse::Commercial, 2, tier, 0.6, 7 + i as u64, style, &rules, &pal);
                let tris = g.triangle_count();
                assert!(tris > 50, "{style:?} {tier:?} on lot {i} drew only {tris} triangles");
                assert!(tris < 120_000, "{style:?} {tier:?} on lot {i} drew {tris} triangles");
                valid(&g, &format!("{style:?} {tier:?}"));
                if tris > worst.0 {
                    worst = (tris, style);
                }
            }
        }
        let a = generate_styled(&lots[0], LandUse::Commercial, 2, Tier::Mid, 0.6, 3, style, &rules, &pal);
        let b = generate_styled(&lots[0], LandUse::Commercial, 2, Tier::Mid, 0.6, 3, style, &rules, &pal);
        assert_eq!(a.get(crate::surface::Surface::Glow).positions, b.get(crate::surface::Surface::Glow).positions, "{style:?} is not deterministic");
        assert_eq!(a.triangle_count(), b.triangle_count(), "{style:?} is not deterministic");
        assert_ne!(style.label(), "", "{style:?} has a label");
    }
    eprintln!("heaviest new style: {worst:?}");
}

/// Each new business wears its own look whatever the zone and whatever the
/// district's architecture; the families are the sensible ones.
#[test]
fn new_businesses_wear_their_own_looks() {
    use nd_sim::districts::Look;
    let rules = BuildingRules::default();
    for lu in [LandUse::Commercial, LandUse::Industrial, LandUse::Entertainment] {
        let lot = &sample_lots(lu)[0];
        for (kind, want) in LOOKS {
            for tier in [Tier::Low, Tier::Mid, Tier::High] {
                assert!(industry_styles(kind, tier).is_some(), "{kind:?} has looks");
                for look in [None, Some(Look::Kowloon), Some(Look::NeonStrip), Some(Look::Suburban)] {
                    for seed in 0..3 {
                        let extras = Extras { business: Some(kind), look, ..Extras::default() };
                        assert_eq!(choose_style(lot, lu, 2, tier, seed, &rules, &extras), want, "{kind:?} on {lu:?} {tier:?} under {look:?}");
                    }
                }
            }
        }
    }
    // Docks by their port kind, the dock operator as a fallback.
    let lot = &sample_lots(LandUse::Industrial)[0];
    for (port, want) in [(PortKind::MarinaSmall, Style::MarinaSmall), (PortKind::MarinaLarge, Style::MarinaLarge), (PortKind::FishingPortSmall, Style::FishingPortSmall), (PortKind::FishingPortLarge, Style::FishingPortLarge)] {
        let extras = Extras { port: Some(port), business: port.business(), ..Extras::default() };
        assert_eq!(choose_style(lot, port.land_use(), 1, Tier::Mid, 1, &rules, &extras), want);
    }
    let extras = Extras { business: Some(B::FishingPort), ..Extras::default() };
    assert!(matches!(choose_style(lot, LandUse::Industrial, 1, Tier::Low, 1, &rules, &extras), Style::FishingPortSmall | Style::FishingPortLarge));
    // Families.
    for s in [Style::CowFarm, Style::ChickenFarm] {
        assert_eq!(traits(s).0, Family::Farm);
    }
    for s in [Style::Slaughterhouse, Style::StripMine, Style::FishStickFactory] {
        assert_eq!(traits(s).0, Family::Heavy);
    }
    for s in [Style::TrashCompany, Style::FishGutting] {
        assert_eq!(traits(s).0, Family::Logistics);
    }
    for s in [Style::BikeShop, Style::CarDealer, Style::FishMarket, Style::Mechanic] {
        assert_eq!(traits(s).0, Family::Retail);
    }
    for s in [Style::SeafoodRestaurant, Style::GolfRange, Style::Gallery] {
        assert_eq!(traits(s).0, Family::Night);
    }
    for s in [Style::MarinaSmall, Style::MarinaLarge, Style::FishingPortSmall, Style::FishingPortLarge] {
        assert_eq!(traits(s).0, Family::Special);
    }
    // Indices of older styles are untouched: the block keeps its place.
    for (k, s) in NEW.iter().enumerate() {
        assert_eq!(s.index(), 80 + k);
        assert_eq!(Style::ALL[80 + k], *s);
    }
}

/// A business opened on a lot in a real world is drawn and named in its
/// look, through the style book's path too.
#[test]
fn opened_businesses_are_named_in_their_look() {
    let rules = BuildingRules::default();
    let mut w = World::new(WorldConfig::default());
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-280.0, 8.0), Vec2::new(280.0, 8.0), Vec2::new(280.0, 70.0), Vec2::new(-280.0, 70.0)], LandUse::Commercial);
    let lots: Vec<_> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
    let mut checked = 0;
    for ((kind, want), lot) in LOOKS.iter().zip(lots) {
        let Ok(b) = w.open_business(lot, *kind, true) else { continue };
        assert_eq!(building_style(&w, b, &rules, None), Some(*want), "{kind:?}");
        checked += 1;
    }
    assert!(checked >= 5, "only {checked} businesses opened");
    let mut book = StyleBook::default();
    book.refresh(&w, &rules);
    for (id, b) in w.buildings.iter() {
        if let Some((_, want)) = b.business.as_ref().and_then(|z| LOOKS.iter().find(|(k, _)| *k == z.kind)) {
            assert_eq!(book.style(id), Some(*want));
        }
    }
}

/// Boat meshes: something for every kind, sound, and the right size.
#[test]
fn boat_meshes_are_sound() {
    use crate::terrain::{boat_mesh, boat_size, ship_mesh};
    let pal = Palette::default();
    for kind in [ShipKind::Freighter, ShipKind::Ferry, ShipKind::FishingBoat, ShipKind::Yacht, ShipKind::Sailboat] {
        let m = boat_mesh(kind, &pal);
        assert!(m.indices.len() >= 36, "{kind:?} is empty");
        assert!(m.positions.iter().flatten().all(|v| v.is_finite()));
        assert!(m.normals.iter().flatten().all(|v| v.is_finite()));
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
        let (len, _) = boat_size(kind);
        let reach = m.positions.iter().map(|p| p[0].abs()).fold(0.0, f32::max);
        assert!(reach <= len * 0.5 + 1.5, "{kind:?} reaches {reach} m for a {len} m boat");
        assert!(m.indices.len() / 3 < 3000, "{kind:?} is heavy");
    }
    assert_eq!(boat_mesh(ShipKind::Freighter, &pal).indices, ship_mesh(true, &pal).indices);
    assert_eq!(boat_mesh(ShipKind::Ferry, &pal).indices, ship_mesh(false, &pal).indices);
}

/// Marinas and fishing ports on a river bank: drawn in their land style,
/// with boats moored on the water side.
#[test]
fn waterfront_docks_moor_their_boats() {
    use nd_sim::landscape::{LandscapeParams, LandscapePreset};
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, 8), ..WorldConfig::default() });
    w.economy.treasury = 1_000_000.0;
    let r = &w.landscape.rivers[0];
    let p = *r.points.iter().min_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap();
    let (seg, _, _) = r.nearest(p);
    let flow = r.flow(seg);
    let width = r.width;
    let mut n = Vec2::new(-flow.y, flow.x);
    if n.dot(-p) < 0.0 {
        n = -n;
    }
    let mut placed = Vec::new();
    for (k, kind) in [PortKind::MarinaSmall, PortKind::FishingPortSmall, PortKind::MarinaLarge, PortKind::FishingPortLarge].into_iter().enumerate() {
        for attempt in 0..6 {
            let along = (k as f32 * 2.0 + attempt as f32 * 0.5 - 4.0) * 110.0;
            let click = p + flow * along + n * (width * 0.5 + 20.0);
            if let Ok(id) = w.place_port(kind, click) {
                placed.push((kind, id));
                break;
            }
        }
    }
    assert!(!placed.is_empty(), "no dock could be placed");
    let mut moored = 0;
    for (kind, id) in &placed {
        let want = match kind {
            PortKind::MarinaSmall => Style::MarinaSmall,
            PortKind::MarinaLarge => Style::MarinaLarge,
            PortKind::FishingPortSmall => Style::FishingPortSmall,
            _ => Style::FishingPortLarge,
        };
        assert_eq!(building_style(&w, *id, &rules, None), Some(want), "{kind:?}");
        let port = w.ports.port_of(*id).unwrap();
        let boats = crate::terrain::moorings(port, &w.landscape);
        assert!(boats.len() <= kind.berths() as usize);
        for b in &boats {
            assert!(w.landscape.is_water(b.pos), "{kind:?} boat on land");
            let ok = if kind.is_fishing() { b.kind == ShipKind::FishingBoat } else { matches!(b.kind, ShipKind::Sailboat | ShipKind::Yacht) };
            assert!(ok, "{kind:?} moors a {:?}", b.kind);
        }
        moored += boats.len();
        // The land side draws on its lot.
        let b = &w.buildings[*id];
        let lot = &w.lots[b.lot];
        let g = generate_building(lot, b.land_use, b.floors, b.tier, 0.8, b.seed, &rules, &pal, &adjacency::style_extras(&w, *id, &[]));
        assert!(g.triangle_count() > 100);
        valid(&g, &format!("{kind:?}"));
    }
    assert!(moored > 0, "no boat moored at {placed:?}");
    let g = crate::terrain::waterworks(&w, &pal);
    valid(&g, "waterworks");
    // [pt-visual] The moored boats are drawn apart from the dock works.
    let boats = crate::terrain::moored_boats(&w, &pal);
    assert!(boats.get(crate::surface::Surface::Panel).indices.len() > 100, "the moored boats are drawn");
    valid(&boats, "moored boats");
}

/// [p2-look] A strip mine's pit is dug below the ground, not heaped up on
/// a rim: its faces stand under ground level inside the hole the renderer
/// cuts out of the ground (`strip_mine_hole`), its floor well below, and
/// the yard round it leaves the hole open.
#[test]
fn a_strip_mine_pit_is_dug_below_the_ground() {
    let rules = BuildingRules::default();
    let pal = crate::palette::Palette::default();
    let local = [Vec2::new(0.0, 0.0), Vec2::new(52.0, 0.0), Vec2::new(52.0, 64.0), Vec2::new(0.0, 64.0)];
    let mut lot = super::sweep_tests::lot_of(&local, 0.7, Vec2::new(120.0, -40.0));
    lot.land_use = LandUse::Industrial;
    let hole = strip_mine_hole(&lot).expect("a pit on a 52 by 64 m lot");
    assert!(crate::mesh::is_convex(&hole));
    let inside = |p: Vec2, slack: f32| -> bool {
        let n = hole.len();
        let s = (0..n).map(|i| hole[i].perp_dot(hole[(i + 1) % n])).sum::<f32>().signum();
        (0..n).all(|i| s * (hole[(i + 1) % n] - hole[i]).perp_dot(p - hole[i]) / hole[i].distance(hole[(i + 1) % n]) >= -slack)
    };
    assert!(hole.iter().all(|p| nd_sim::geom::point_in_polygon(*p, &lot.polygon)), "the pit is on its lot");
    for tier in [Tier::Low, Tier::Mid, Tier::High] {
        let g = generate_styled(&lot, LandUse::Industrial, 1, tier, 0.8, 7, Style::StripMine, &rules, &pal);
        let stone = g.parts.get(crate::surface::Surface::Stone.index()).expect("rock faces");
        let low = stone.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(low < -8.0, "{tier:?}: the floor is dug {low:.1} m down");
        for p in &stone.positions {
            assert!(p[1] <= 0.1, "{tier:?}: no rock face stands above the ground ({:.2} m)", p[1]);
            assert!(inside(Vec2::new(p[0], p[2]), 0.05), "{tier:?}: a rock face outside the hole cut for it");
        }
        // Nothing floors the hole at ground level: the yard is cut round it.
        for (s, m) in g.iter() {
            for t in m.indices.chunks_exact(3) {
                let ps: Vec<[f32; 3]> = t.iter().map(|&i| m.positions[i as usize]).collect();
                let flat = ps.iter().all(|p| (p[1] - 0.03).abs() < 0.02);
                let mid = Vec2::new(ps.iter().map(|p| p[0]).sum::<f32>() / 3.0, ps.iter().map(|p| p[2]).sum::<f32>() / 3.0);
                assert!(!(flat && inside(mid, -0.5)), "{tier:?}: {s:?} ground over the pit at {mid:?}");
            }
        }
    }
}

/// [r4-leisure-places] Every place to eat and go out wears its own look in
/// any zone and district, builds at every tier and height within budget,
/// the same seed drawing the same thing; and says what it is in neon.
#[test]
fn places_to_eat_and_go_out_wear_their_own_looks() {
    use nd_sim::districts::Look;
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let kinds: Vec<B> = nd_sim::leisure::EATERIES.into_iter().chain(nd_sim::leisure::NIGHTS_OUT).collect();
    assert_eq!(kinds.len(), Style::EAT_OUT.len());
    for (kind, want) in kinds.iter().zip(Style::EAT_OUT) {
        for lu in [LandUse::Commercial, LandUse::Entertainment] {
            let lot = &sample_lots(lu)[0];
            for look in [None, Some(Look::NeonStrip), Some(Look::Lantern), Some(Look::HarbourPastel), Some(Look::Patchwork), Some(Look::SkylineLuxe), Some(Look::GlassCampus), Some(Look::SolarpunkGreen)] {
                let extras = Extras { business: Some(*kind), look, ..Extras::default() };
                assert_eq!(choose_style(lot, lu, 2, Tier::Mid, 3, &rules, &extras), want, "{kind:?} on {lu:?} under {look:?}");
            }
        }
    }
    let mut heaviest = (0, Style::Diner);
    for style in Style::EAT_OUT {
        for lu in [LandUse::Commercial, LandUse::Entertainment] {
            for (i, lot) in sample_lots(lu).iter().enumerate() {
                for (tier, floors) in [(Tier::Low, 1u8), (Tier::Mid, 3), (Tier::High, 8)] {
                    let g = generate_styled(lot, lu, floors, tier, 0.6, 11 + i as u64, style, &rules, &pal);
                    let tris = g.triangle_count();
                    assert!(tris > 200, "{style:?} {tier:?} on lot {i} drew only {tris} triangles");
                    let glow = g.get(crate::surface::Surface::Glow).positions.len();
                    assert!(glow >= 16, "{style:?} {tier:?} on lot {i}: no sign ({glow} glow vertices)");
                    valid(&g, &format!("{style:?} {tier:?}"));
                    if tris > heaviest.0 {
                        heaviest = (tris, style);
                    }
                }
            }
        }
        let a = generate_styled(&sample_lots(LandUse::Commercial)[0], LandUse::Commercial, 2, Tier::Mid, 0.6, 3, style, &rules, &pal);
        let b = generate_styled(&sample_lots(LandUse::Commercial)[0], LandUse::Commercial, 2, Tier::Mid, 0.6, 3, style, &rules, &pal);
        assert_eq!(a.triangle_count(), b.triangle_count(), "{style:?} is not deterministic");
        assert_ne!(style.label(), "");
    }
    eprintln!("heaviest place to eat or go out: {heaviest:?}");
    // The block keeps its place at the end of the styles.
    let first = Style::EAT_OUT[0].index();
    for (k, s) in Style::EAT_OUT.iter().enumerate() {
        assert_eq!(Style::ALL[first + k], *s);
    }
}
