//! [r12-yards] Free ground is found round the house and never in it, a
//! house at the street steps onto the pavement, a block's windows are its
//! street-side ones, placing is deterministic, a click finds the owner,
//! and every thing is drawn at a believable size.

use super::*;
use crate::buildings::{generate_building, BuildingRules, Extras};
use crate::palette::Palette;
use nd_sim::citizens::CitizenId;
use nd_sim::{LandUse, Tier};

/// A plain lot of a street-front block, 8..40 m wide and 15..60 deep.
fn a_lot() -> Lot {
    use nd_sim::roads::RoadNetwork;
    use nd_sim::zoning::{generate_lots, Zone, ZoneId};
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, -40.0), Vec2::new(300.0, -40.0)], 1, 1);
    let zone = Zone { polygon: vec![Vec2::new(-100.0, -34.0), Vec2::new(100.0, -34.0), Vec2::new(100.0, 10.0), Vec2::new(-100.0, 10.0)], land_use: LandUse::Residential };
    let mut lots = slotmap::SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let mut v: Vec<Lot> = lots.values().filter(|l| l.usable() && (8.0..40.0).contains(&l.width()) && (15.0..60.0).contains(&l.depth())).cloned().collect();
    v.sort_by(|a, b| a.center.x.total_cmp(&b.center.x));
    let n = lots.len(); v.into_iter().next().unwrap_or_else(|| panic!("a plain lot among {n}: {:?}", lots.values().map(|l| (l.usable(), l.width(), l.depth())).collect::<Vec<_>>()))
}

/// The lot's frame: front-left corner, along the front, into the lot.
fn frame(l: &Lot) -> (Vec2, Vec2, Vec2) {
    let (c0, c1, c3) = (l.corners[0], l.corners[1], l.corners[3]);
    (c0, (c1 - c0).normalize(), (c3 - c0).normalize())
}

/// A box house `v0..v1` metres into the lot, 1.5 m in from its sides, 6 m
/// high with a flat roof, and a lawn under everything.
fn house(l: &Lot, v0: f32, v1: f32) -> Geometry {
    let (c0, u, v) = frame(l);
    let w = l.width();
    let mut g = Geometry::default();
    g.part(Surface::Matte).ground_polygon(&l.polygon, 0.03, [0.2, 0.3, 0.1, 1.0]);
    let mid = c0 + u * (w * 0.5) + v * ((v0 + v1) * 0.5);
    let yaw = u.y.atan2(u.x);
    g.part(Surface::Brick).boxed(Vec3::new(mid.x, 3.0, mid.y), Vec3::new(w - 3.0, 6.0, v1 - v0), yaw, [0.4, 0.2, 0.15, 1.0]);
    g
}

fn local(l: &Lot, p: Vec3) -> Vec2 {
    let (c0, u, v) = frame(l);
    let d = Vec2::new(p.x, p.z) - c0;
    Vec2::new(d.dot(u), d.dot(v))
}

#[test]
fn free_ground_is_found_round_a_house_and_never_in_it() {
    let l = a_lot();
    let (v0, v1) = (6.0, (l.depth() - 7.0).min(16.0));
    let s = spots(&house(&l, v0, v1), &l, false, 7);
    assert!(!s.step.is_empty() && !s.front.is_empty() && !s.yard.is_empty(), "{s:?}");
    for p in s.all() {
        let q = local(&l, p.at);
        assert!(!(q.y > v0 - 0.2 && q.y < v1 + 0.2 && q.x > 1.3 && q.x < l.width() - 1.3), "a spot in the house: {q:?}");
        assert!(p.pavement || point_in_polygon(Vec2::new(p.at.x, p.at.z), &l.polygon), "a spot off the lot: {q:?}");
        assert_eq!(p.at.y, 0.0);
    }
    for p in &s.step {
        let q = local(&l, p.at);
        assert!(q.y < v0 && q.y > v0 - 1.8, "against the front wall: {q:?}");
    }
    assert!(s.front.iter().all(|p| local(&l, p.at).y < v0), "the front garden is in front");
    assert!(s.yard.iter().all(|p| local(&l, p.at).y > v1), "the yard is behind");
    // Things face the street.
    let street = l.front_dir.normalize();
    assert!(s.step.iter().all(|p| Vec2::from_angle(p.yaw).dot(street) > 0.99));
    // The same house, the same spots; raised by the pad.
    let mut again = spots(&house(&l, v0, v1), &l, false, 7);
    assert_eq!(again, s);
    again.raise(4.0, |_| 3.0);
    assert!(again.all().all(|p| p.at.y == 4.0));
}

/// [r14-photofix] On a deep lot with a small house, yard and garden things
/// stay near the house, not out in the middle of the empty ground (Tide
/// Towers' grill stood alone 30 m from its house).
#[test]
fn yard_things_stay_near_the_house() {
    let l = a_lot();
    let (v0, v1) = (4.0, 9.0);
    assert!(l.depth() > v1 + YARD_REACH + 6.0, "a deep lot: {}", l.depth());
    let s = spots(&house(&l, v0, v1), &l, false, 7);
    assert!(!s.yard.is_empty(), "{s:?}");
    for p in &s.yard {
        let q = local(&l, p.at);
        assert!(q.y < v1 + YARD_REACH + 1.5, "a yard spot {} m behind the house", q.y - v1);
    }
}

#[test]
fn a_house_at_the_street_puts_its_step_on_the_pavement() {
    let l = a_lot();
    let s = spots(&house(&l, 0.2, 10.0), &l, false, 3);
    let pav: Vec<&Spot> = s.step.iter().filter(|p| p.pavement).collect();
    assert!(!pav.is_empty(), "{s:?}");
    for p in pav {
        let q = local(&l, p.at);
        assert!(q.y < 0.0 && q.y > -0.8, "just outside the front, against the wall: {q:?}");
    }
    assert!(s.front.is_empty(), "no front garden");
}

#[test]
fn a_blocks_windows_are_its_street_side_ones() {
    let l = a_lot();
    let extras = Extras { dynamic_lights: true, ..Extras::default() };
    let mut found = 0;
    for seed in 0..6u64 {
        let g = generate_building(&l, LandUse::Residential, 6, Tier::Low, 0.9, 40 + seed, &BuildingRules::default(), &Palette::default(), &extras);
        let s = spots(&g, &l, true, seed);
        found += !s.windows.is_empty() as usize;
        for w in &s.windows {
            assert!(Vec2::from_angle(w.yaw).dot(l.front_dir.normalize()) > 0.3, "faces the street");
            assert!(w.at.y >= 2.0 && w.at.y <= WINDOW_TOP);
            assert!(w.room >= 0.5);
        }
    }
    assert!(found >= 4, "most blocks have street-side windows: {found} of 6");
}

fn owners(n: usize) -> Vec<CitizenId> {
    let mut sm: slotmap::SlotMap<CitizenId, ()> = slotmap::SlotMap::with_key();
    (0..n).map(|_| sm.insert(())).collect()
}

#[test]
fn placing_is_deterministic_and_a_click_finds_the_owner() {
    let l = a_lot();
    let s = spots(&house(&l, 6.0, (l.depth() - 7.0).min(16.0)), &l, false, 11);
    let who = owners(5);
    let hh = nd_sim::citizens::HouseholdId::default();
    let wish = |kind, owner, place, seed| Wish { kind, owner, household: hh, flat: 0, place, seed };
    let wishes = vec![
        wish(PropKind::Laundry, who[0], Place::Yard, 1),
        wish(PropKind::Chair, who[1], Place::Step, 2),
        wish(PropKind::Plant, who[1], Place::Step, 3),
        wish(PropKind::KidBike, who[2], Place::Front, 4),
        wish(PropKind::Grill, who[3], Place::Yard, 5),
        wish(PropKind::Bike, who[4], Place::Front, 6),
    ];
    let a = place(&wishes, &s, 99);
    assert_eq!(a, place(&wishes, &s, 99));
    assert!(a.len() >= 5, "{a:?}");
    // Nothing stands on another.
    for (i, p) in a.iter().enumerate() {
        for q in &a[i + 1..] {
            assert!(p.at.distance(q.at) > 0.5, "{p:?} on {q:?}");
        }
    }
    // A click from a camera up the street, on each thing, finds its owner.
    // [r14-photofix] From high up: the yard's things now stand near the
    // house, so from a low eye the washing line can hide the grill.
    let props: Vec<(PropKind, u8, Vec3)> = a.iter().map(|p| (p.kind, p.variant, p.at)).collect();
    let eye_dir = Vec3::new(l.front_dir.x, 0.0, l.front_dir.y).normalize();
    for (i, p) in a.iter().enumerate() {
        let (up, _) = reach(p.kind, p.variant);
        let target = p.at + Vec3::Y * up;
        let eye = target + eye_dir * 8.0 + Vec3::Y * 30.0;
        let (hit, _) = pick_ray(&props, eye, target - eye).expect("a hit");
        assert_eq!(a[hit].owner, p.owner, "clicked {:?}, got {:?}", p.kind, a[hit].kind);
        let _ = i;
    }
    // Off to the side: nothing.
    let p = a[0].at + Vec3::new(30.0, 0.0, 30.0);
    assert!(pick_ray(&props, p + Vec3::Y * 10.0, -Vec3::Y).is_none());
}

#[test]
fn a_households_window_things_go_to_one_window_of_its_own() {
    let mut s = Spots::default();
    for k in 0..6 {
        s.windows.push(Spot { at: Vec3::new(k as f32 * 3.0, 6.0, 0.0), yaw: 0.0, room: 1.2, pavement: false });
    }
    s.step.push(Spot { at: Vec3::new(0.0, 0.0, -1.0), yaw: 0.0, room: 0.5, pavement: true });
    let who = owners(3);
    let hh = nd_sim::citizens::HouseholdId::default();
    let wishes = vec![
        Wish { kind: PropKind::Plant, owner: who[0], household: hh, flat: 2, place: Place::Window, seed: 1 },
        Wish { kind: PropKind::Laundry, owner: who[1], household: hh, flat: 2, place: Place::Window, seed: 2 },
        Wish { kind: PropKind::Plant, owner: who[2], household: hh, flat: 4, place: Place::Window, seed: 3 },
        Wish { kind: PropKind::Bike, owner: who[2], household: hh, flat: 4, place: Place::Window, seed: 4 },
    ];
    let a = place(&wishes, &s, 5);
    assert_eq!(a.len(), 4);
    assert_eq!(a[0].at, a[1].at, "one flat, one window");
    assert_ne!(a[0].at, a[2].at, "another flat, another window");
    assert!(a[0].variant >= WINDOW_BOX && a[1].variant >= POLE);
    assert_eq!(a[3].at, s.step[0].at, "a block's bike stands by its door");
}

#[test]
fn every_thing_is_drawn_small_and_believable() {
    for kind in PropKind::ALL {
        for v in 0..variants(kind) {
            let g = prop_geometry(kind, v);
            let tris = g.triangle_count();
            assert!(tris > 0 && tris < 2_000, "{kind:?} {v}: {tris} triangles");
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for (_, m) in g.iter() {
                for p in &m.positions {
                    lo = lo.min(Vec3::from(*p));
                    hi = hi.max(Vec3::from(*p));
                }
                assert!(m.positions.iter().all(|p| p.iter().all(|c| c.is_finite())));
            }
            let size = hi - lo;
            let most = if kind == PropKind::Trampoline { 3.8 } else { 3.2 };
            assert!(size.max_element() < most, "{kind:?} {v}: {size:?}");
            let window = (kind == PropKind::Plant && v >= WINDOW_BOX) || (kind == PropKind::Laundry && v >= POLE);
            if window {
                assert!(hi.y < 0.4 && lo.x > -0.05, "{kind:?} {v} hangs off the wall: {lo:?} {hi:?}");
            } else {
                assert!(lo.y > -0.02, "{kind:?} {v} stands on the ground: {lo:?}");
                assert!(hi.y < 2.1, "{kind:?} {v} no taller than a person or a line: {hi:?}");
            }
        }
    }
    // A bike is about a bike's size.
    let g = prop_geometry(PropKind::Bike, 0);
    let ys: Vec<f32> = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).collect();
    let top = ys.iter().copied().fold(0.0, f32::max);
    assert!((0.9..1.15).contains(&top), "bike height {top}");
}

/// [r13-detail] A household's washing goes on the building style's own
/// lines first (a flat's on a pole, a house's on a line), in runs along
/// it; a household with no line left puts out its own.
#[test]
fn washing_goes_on_the_styles_lines_first() {
    let mut s = Spots::default();
    s.yard.push(Spot { at: Vec3::new(0.0, 0.0, 5.0), yaw: 0.0, room: 2.0, pavement: false });
    s.lines.push(nd_procgen_line(Vec3::new(0.0, 2.0, 0.0), Vec3::new(3.3, 2.0, 0.0), false));
    s.lines.push(nd_procgen_line(Vec3::new(0.0, 6.0, 0.0), Vec3::new(1.2, 6.0, 0.0), true));
    let who = owners(3);
    let hh = nd_sim::citizens::HouseholdId::default();
    let wishes = vec![
        Wish { kind: PropKind::Laundry, owner: who[0], household: hh, flat: 0, place: Place::Yard, seed: 1 },
        Wish { kind: PropKind::Laundry, owner: who[1], household: hh, flat: 1, place: Place::Window, seed: 2 },
        Wish { kind: PropKind::Laundry, owner: who[2], household: hh, flat: 2, place: Place::Yard, seed: 3 },
    ];
    let a = place(&wishes, &s, 7);
    assert_eq!(a, place(&wishes, &s, 7));
    let of = |o| a.iter().filter(|p| p.owner == o).collect::<Vec<_>>();
    let line = of(who[0]);
    assert_eq!(line.len(), 3, "three runs on a 3.3 m line: {line:?}");
    assert!(line.iter().all(|p| p.variant >= HUNG && (1.85..=2.0).contains(&p.at.y)));
    let pole = of(who[1]);
    assert!(pole.len() == 1 && pole[0].variant >= HUNG && (pole[0].at.y - 6.0).abs() < 0.15, "a flat's washing on the pole: {pole:?}");
    let own = of(who[2]);
    assert!(own.len() == 1 && own[0].variant < POLE && own[0].at.y == 0.0, "no line left: its own line or airer: {own:?}");
}

fn nd_procgen_line(a: Vec3, b: Vec3, window: bool) -> crate::surface::WashLine {
    crate::surface::WashLine { a, b, sag: 0.1, window }
}
