//! [r4-decks] Ground under raised roads: columns stand in the parks zoned
//! under a deck, an old raised road keeps the columns the city has built
//! round since, and what goes up under a deck fits below it.

use super::*;
use crate::placeable::Placeable;
use crate::services::ServiceKind;

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn way(points: &[Vec2], levels: &[f32], kind: RoadKind, lanes: u8) -> Gesture {
    Gesture::Road { points: points.to_vec(), lanes_fwd: lanes, lanes_bwd: lanes, curved: false, kind, elevation: levels.to_vec(), car_only: None }
}

/// Plan a gesture in a project of its own and implement it.
fn try_build(w: &mut World, g: Gesture) -> Result<(), String> {
    w.project = None;
    w.add_gesture(g);
    w.try_implement()
}

/// Build on every vacant lot whose centre is in the box, finished at once.
fn grow_all(w: &mut World, lo: Vec2, hi: Vec2) -> Vec<BuildingId> {
    let mut lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.building.is_none() && l.center.cmpge(lo).all() && l.center.cmple(hi).all()).map(|(id, _)| id).collect();
    lots.sort();
    let mut out = Vec::new();
    for lot in lots {
        w.build_on(lot);
        if let Some(b) = w.lots[lot].building {
            if w.buildings[b].construction.is_some() {
                w.finish_construction(b);
            }
            out.push(b);
        }
    }
    out
}

/// Harvey's day-9 city: a raised walkway at 6 m (too low for a storey)
/// over ground then zoned as parks, which grew on the ground of its
/// columns. Returns the world and the walkway as drawn.
fn walkway_over_parks() -> (World, Vec<Vec2>, Vec<f32>) {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let pts = vec![Vec2::new(-430.0, 24.0), Vec2::new(-330.0, 24.0), Vec2::new(330.0, 24.0), Vec2::new(430.0, 24.0)];
    let levels = vec![0.0, 6.0, 6.0, 0.0];
    try_build(&mut w, way(&pts, &levels, RoadKind::Path, 0)).expect("the walkway is built");
    assert!(w.pillars.len() >= 15, "columns along it: {}", w.pillars.len());
    w.add_zone(square(-200.0, 8.0, 400.0, 60.0), LandUse::Park);
    let parks = grow_all(&mut w, Vec2::new(-200.0, 8.0), Vec2::new(200.0, 68.0));
    assert!(parks.iter().all(|&b| w.buildings[b].land_use == LandUse::Park));
    let over = parks.iter().filter(|&&b| w.pillars_on(&w.lots[w.buildings[b].lot].polygon)).count();
    assert!(over >= 3, "parks stand on the ground of the columns: {over}");
    (w, pts, levels)
}

/// [r4-decks] A column stands in a park like on open ground: a project
/// elsewhere (and the rebuild it brings) keeps every column of a walkway
/// with parks zoned under it, finds no stretch without room, and pulls no
/// park down. On Harvey's day-9 save parks counted as something the city
/// keeps: every rebuild found no room for the columns standing in them
/// and drew spans of 45 m and 85 m in red. The same after a save and load.
#[test]
fn columns_stand_in_the_parks_zoned_under_a_walkway() {
    let (mut w, ..) = walkway_over_parks();
    let before = w.pillars.clone();
    let parks = w.buildings.len();
    try_build(&mut w, way(&[Vec2::new(-200.0, -150.0), Vec2::new(200.0, -150.0)], &[], RoadKind::Street, 1)).expect("a street elsewhere is implemented");
    assert!(w.pillar_gaps.is_empty(), "no stretch without room: {:?}", w.pillar_gaps);
    assert_eq!(w.pillars, before, "the columns stand where they stood");
    assert_eq!(w.buildings.len(), parks, "no park was pulled down");
    let mut back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.pillars, before);
    let check = {
        back.project = None;
        back.add_gesture(way(&[Vec2::new(-200.0, -250.0), Vec2::new(200.0, -250.0)], &[], RoadKind::Street, 1));
        back.active_project_check()
    };
    assert!(check.refusal.is_none() && check.pillar_gaps.is_empty(), "{:?} {:?}", check.refusal, check.pillar_gaps);
    back.try_implement().expect("implemented after a load");
    assert!(back.pillar_gaps.is_empty() && back.pillars == before);
}

/// [r4-decks] An old raised road keeps the columns it stood on where the
/// city has since put up its own structures round them (here every park
/// under the walkway turned into a depot): its stretches with no room for
/// a new column are held by the old ones, so no red span is left on a
/// road that stands anyway. (A road laid since where an old column stood
/// does take it: `place_pillars` keeps none on a road.)
#[test]
fn an_old_raised_road_keeps_its_columns_where_the_city_built_round_them() {
    let (mut w, pts, levels) = walkway_over_parks();
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Park).map(|(id, _)| id).collect();
    for id in ids {
        let b = &mut w.buildings[id];
        b.depot = true;
        b.floors = 1;
    }
    // Planned afresh (its own columns not counted), the walkway would
    // have no room: the depots fill the ground under it.
    let (bp, bl) = w.roads.as_built(&pts, &levels, RoadKind::Path);
    let fresh = w.plan_pillars(&bp, &bl, super::pillars::deck_half_width(RoadKind::Path, 0, 0), &[]);
    assert!(fresh.gaps.iter().any(|g| !g.end), "the test needs a stretch with no room now: {:?}", fresh.gaps);
    let before = w.pillars.clone();
    try_build(&mut w, way(&[Vec2::new(-200.0, -150.0), Vec2::new(200.0, -150.0)], &[], RoadKind::Street, 1)).expect("a street elsewhere is implemented");
    assert!(w.pillar_gaps.is_empty(), "the old columns hold it: {:?}", w.pillar_gaps);
    // Those among the depots stand (elsewhere it may be planned afresh).
    for p in before.iter().filter(|p| p.at.x.abs() < 190.0) {
        assert!(w.pillars.iter().any(|q| q.at.distance(p.at) < 0.01), "the column at {} stands", p.at);
    }
}

/// [r4-decks] Under a deck high enough for two storeys (12 m) what grows,
/// is rebuilt or is placed fits below it: new buildings, a redevelopment
/// to nine floors, a hospital refused with the reason, a surface car park
/// allowed; nothing goes on a column's ground; no tower or HQ plot.
#[test]
fn what_goes_up_under_a_deck_fits_below_it() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let pts = [Vec2::new(-430.0, 24.0), Vec2::new(-330.0, 24.0), Vec2::new(330.0, 24.0), Vec2::new(430.0, 24.0)];
    try_build(&mut w, way(&pts, &[0.0, 12.0, 12.0, 0.0], RoadKind::Street, 1)).expect("the viaduct is built");
    let cap = crate::pillars::floors_under(12.0);
    assert_eq!(cap, 2);
    w.add_zone(square(-200.0, 8.0, 400.0, 60.0), LandUse::Commercial);
    let under: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).filter(|(id, _)| w.deck_cap(*id).is_some()).map(|(id, _)| id).collect();
    assert!(under.len() >= 4, "lots under the deck: {}", under.len());
    for &lot in &under {
        assert!(w.under_a_deck(lot) && !w.tower_plot_ok(lot, LandUse::Commercial) && !w.hq_plot_ok(lot), "no tower or HQ under a deck");
    }
    let grown = grow_all(&mut w, Vec2::new(-200.0, 8.0), Vec2::new(200.0, 68.0));
    let mut n = 0;
    for &b in &grown {
        if w.deck_cap(w.buildings[b].lot).is_some() {
            assert!(w.buildings[b].floors <= cap, "{} has {} floors under a two-storey deck", w.buildings[b].name, w.buildings[b].floors);
            n += 1;
        }
    }
    assert!(n >= 4, "grown under the deck: {n}");
    // Redeveloped to nine floors: still two.
    let b = grown.iter().copied().find(|&b| w.deck_cap(w.buildings[b].lot).is_some()).unwrap();
    w.redevelop(b, Tier::High, 9, 1.0);
    assert!(w.buildings[b].floors <= cap, "rebuilt to {} floors", w.buildings[b].floors);
    // Placed: a hospital of eight storeys is refused, a surface lot fits.
    let lot = w.buildings[b].lot;
    let poly = w.lots[lot].polygon.clone();
    let why = w.deck_problem(Placeable::Service(ServiceKind::Health, Tier::High), &poly).expect("too tall");
    assert!(why.contains("too tall"), "{why}");
    assert!(w.deck_problem(Placeable::ParkingLot(crate::enforcement::LotSize::Small), &poly).is_none());
    assert!(w.deck_problem(Placeable::Service(ServiceKind::Police, Tier::Low), &poly).is_none(), "a one-storey post fits");
    // Nothing on the ground of a column.
    let col = w.pillars.iter().find(|p| p.level > 11.0).expect("a column under the deck").at;
    let pad = square(col.x - 6.0, col.y - 6.0, 12.0, 12.0);
    assert!(w.deck_problem(Placeable::ParkingLot(crate::enforcement::LotSize::Small), &pad).is_some_and(|why| why.contains("column")));
}
