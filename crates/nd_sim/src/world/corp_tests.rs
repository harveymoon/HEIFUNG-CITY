//! [megacorp] Tests of mega-corps (growing into one, buying by road
//! distance from the HQ, the HQ tower, staff housing, the cap and the
//! levy), universities (dorms, annexes, plazas, students and the trade
//! they bring), schools and families, old saves and determinism.

use super::*;
use crate::corp::*;
use crate::estate::OwnerKind;
use crate::market::DayBook;
use crate::mind::IdleMind;
use crate::services::ServiceKind;

fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1e7;
    w
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn free_lots(w: &World, lu: LandUse) -> Vec<LotId> {
    let mut v: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.car_access() && l.building.is_none()).map(|(id, _)| id).collect();
    v.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x));
    v
}

/// A finished business of `kind` on `lot`, paying its way for days.
fn business(w: &mut World, lot: LotId, kind: BusinessKind, revenue: f32) -> BuildingId {
    let b = w.build_on_as(lot, Some(kind)).expect("a site");
    w.finish_construction(b);
    let z = w.buildings[b].business.as_mut().unwrap();
    z.books = vec![DayBook { revenue, spend: revenue * 0.5 }; 4];
    z.money = 5_000.0;
    z.days_open = 10;
    z.closed = false;
    b
}

fn homes(w: &mut World, lot: LotId) -> BuildingId {
    w.build_on(lot);
    let b = w.lots[lot].building.unwrap();
    w.finish_construction(b);
    b
}

/// An avenue along y = 0 and a back street along y = 260 joined to it only
/// at the far east end: across the gap a lot is near as the crow flies and
/// far by road. Shops on both, homes on the avenue.
fn two_streets() -> World {
    let mut w = town();
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-700.0, 260.0), Vec2::new(700.0, 260.0)], 1, 1);
    w.add_road(&[Vec2::new(700.0, 0.0), Vec2::new(700.0, 260.0)], 1, 1);
    w.add_zone(rect(-650.0, 8.0, 700.0, 60.0), LandUse::Commercial);
    w.add_zone(rect(-650.0, 192.0, 700.0, 60.0), LandUse::Commercial);
    w.add_zone(rect(100.0, 8.0, 500.0, 60.0), LandUse::Residential);
    w.add_zone(rect(100.0, -68.0, 500.0, 60.0), LandUse::Residential);
    w
}

/// A firm of two businesses on the west end of the avenue, taking
/// `revenue` a day each, with `staff` heads on the first one's books.
fn firm(w: &mut World, revenue: f32, staff: usize) -> (BuildingId, BuildingId) {
    let lots: Vec<LotId> = free_lots(w, LandUse::Commercial).into_iter().filter(|l| w.lots[*l].center.y < 100.0).collect();
    let a = business(w, lots[0], BusinessKind::Office, revenue);
    let b = business(w, lots[1], BusinessKind::GadgetShop, revenue);
    // Stand-in staff: the books count heads.
    w.buildings[a].workers = vec![CitizenId::default(); staff];
    let founder = w.new_owner("Okafor Trading".into(), OwnerKind::Firm { kind: BusinessKind::Office });
    w.give_to(a, founder, 0.0);
    w.give_to(b, founder, 0.0);
    (a, b)
}

fn people(w: &mut World) {
    w.config.immigration_interval = 2;
    run(w, 600);
}

// ── mega-corps ───────────────────────────────────────────────────────────

#[test]
fn a_company_past_the_thresholds_becomes_a_mega_corp_with_a_brand_and_an_hq() {
    let mut w = two_streets();
    assert_eq!(w.corp_room(), 0, "a hamlet has no mega-corps");
    let (a, b) = firm(&mut w, 600.0, 10);
    for l in free_lots(&w, LandUse::Residential).into_iter().take(10) {
        homes(&mut w, l);
    }
    people(&mut w);
    // A town big enough to carry one: stand-ins for the rest of the people.
    let template = w.citizens.values().next().cloned().expect("people arrived");
    while w.citizens.len() < CORP_MIN_POP {
        w.citizens.insert(Citizen { home: None, work: None, household: None, ..template.clone() });
    }
    assert!(w.corp_room() >= 1);
    // Too small: no promotion, however often the dice are rolled.
    for _ in 0..30 {
        w.promote_companies();
        w.clock.tick += 1440;
    }
    assert!(w.corps.corps.is_empty(), "under the thresholds nothing happens");
    // Past them: takings and staff.
    for bid in [a, b] {
        let z = w.buildings[bid].business.as_mut().unwrap();
        z.books = vec![DayBook { revenue: PROMOTE_REVENUE, spend: PROMOTE_REVENUE * 0.6 }; 4];
        z.closed = false;
    }
    w.buildings[a].workers = vec![CitizenId::default(); PROMOTE_STAFF];
    let size = w.companies().into_iter().find(|c| c.flagship == a).expect("the firm is a company");
    assert!(size.qualifies(), "{size:?}");
    for _ in 0..40 {
        w.promote_companies();
        if !w.corps.corps.is_empty() {
            break;
        }
        w.clock.tick += 1440;
    }
    w.buildings[a].workers.clear();
    assert_eq!(w.corps.corps.len(), 1, "it grew into a mega-corp");
    let c = w.corps.corps[0].clone();
    assert!(matches!(w.estate.owners[c.owner].kind, OwnerKind::MegaCorp { kind: BusinessKind::Office }));
    assert_eq!(w.estate.owners[c.owner].name, c.brand.name, "under its new name");
    assert_eq!(c.origin_name, "Okafor Trading");
    assert!(w.owned_with(a).contains(&b), "both businesses are the corp's");
    assert_eq!(c.brand.logo_rows().len(), LOGO_N);
    assert_eq!(c.brand.monogram, c.brand.name.chars().next().unwrap());
    assert!(w.social.news.iter().any(|n| n.text.contains(&c.brand.name)), "the news");
    // The HQ: a tall office tower downtown, the corp's.
    let hq = c.hq.expect("it broke ground on its HQ");
    assert!(w.buildings[hq].floors >= HQ_MIN_FLOORS, "a tower: {}", w.buildings[hq].floors);
    assert_eq!(w.corp_of(hq).map(|x| x.owner), Some(c.owner));
    w.finish_construction(hq);
    let h = &w.buildings[hq];
    assert!(h.name.ends_with("Tower"), "{}", h.name);
    assert_eq!(h.business.as_ref().map(|z| z.kind), Some(BusinessKind::Office));
    assert_eq!(h.tier, Tier::High);
    assert_eq!(w.brand_mark(hq).map(|m| m.role), Some(BrandRole::Headquarters));
    assert_eq!(w.brand_mark(b).map(|m| m.role), Some(BrandRole::Holding));
    // Its fleet wears the brand.
    let letter = c.brand.monogram as u8 - b'A';
    assert_eq!(w.corp_livery(b), Some(crate::vehicle::Livery::Corp(c.brand.colour, c.brand.shape as u8 | (letter << 3))));
    // The market leaves its holdings alone.
    assert!(!w.buyable(b));
    // Promoting the same company again is the same corp.
    assert_eq!(w.promote_to_corp(a), Ok(0));
}

#[test]
fn a_corp_buys_what_is_nearest_its_hq_by_road_not_as_the_crow_flies() {
    let mut w = two_streets();
    let (a, _) = firm(&mut w, 600.0, 10);
    let ci = w.promote_to_corp(a).unwrap();
    w.buildings[a].workers.clear();
    // Measure from the flagship at the west end of the avenue.
    w.corps.corps[ci].hq = None;
    let o = w.lots[w.buildings[a].lot].center;
    // Across the gap: near as the crow flies, kilometres by road.
    let across = free_lots(&w, LandUse::Commercial).into_iter().filter(|l| w.lots[*l].center.y > 100.0).min_by(|x, y| w.lots[*x].center.distance(o).total_cmp(&w.lots[*y].center.distance(o))).unwrap();
    // Along the avenue, further as the crow flies but near by road.
    let along = free_lots(&w, LandUse::Commercial).into_iter().filter(|l| w.lots[*l].center.y < 100.0 && w.lots[*l].center.distance(o) > w.lots[across].center.distance(o) + 40.0).min_by(|x, y| w.lots[*x].center.distance(o).total_cmp(&w.lots[*y].center.distance(o))).unwrap();
    let far_one = business(&mut w, across, BusinessKind::Bakery, 300.0);
    let near_one = business(&mut w, along, BusinessKind::Grocery, 300.0);
    let owner = w.corps.corps[ci].owner;
    let d_across = w.corp_drive_m(owner, across).unwrap();
    let d_along = w.corp_drive_m(owner, along).unwrap();
    assert!(w.lots[across].center.distance(o) < w.lots[along].center.distance(o));
    assert!(d_along < d_across, "by road the avenue is nearer: {d_along:.0} vs {d_across:.0}");
    assert!(d_across > 1500.0, "round by the east end: {d_across:.0}");
    let v = w.roads.version;
    assert!(w.corps.drive_cache.values().all(|(ver, _)| *ver == v), "cached against the roads");
    w.corps.corps[ci].cash = 1e6;
    // Homes on the avenue: a town big enough for the cap to allow two buys.
    for l in free_lots(&w, LandUse::Residential) {
        homes(&mut w, l);
    }
    // Nothing vacant to tempt it.
    for l in free_lots(&w, LandUse::Commercial).into_iter().chain(free_lots(&w, LandUse::Residential)) {
        w.estate.reserved.insert(l, a);
    }
    let cands: Vec<(f32, super::corp::Buy)> = w.corp_candidates(ci);
    assert!(!cands.is_empty(), "something to buy (cap {}, held {})", w.corp_cap(), w.estate.owners[owner].properties.len());
    w.corps.rules.max_share = MAX_MAX_SHARE;
    assert!(w.corp_buy(ci), "{cands:?}");
    assert_eq!(w.corp_of(near_one).map(|c| c.owner), Some(owner), "the one near by road first");
    assert!(w.corp_of(far_one).is_none());
    assert!(w.corp_buy(ci));
    assert_eq!(w.corp_of(far_one).map(|c| c.owner), Some(owner), "then the far one");
    assert_eq!(w.corps.stats.purchases_total, 2);
}

#[test]
fn staff_move_into_company_housing_at_a_discount_and_the_cap_holds() {
    let mut w = two_streets();
    for l in free_lots(&w, LandUse::Residential).into_iter().take(8) {
        homes(&mut w, l);
    }
    let (a, _) = firm(&mut w, 600.0, 0);
    people(&mut w);
    // Fresh blocks with room.
    let mut blocks = Vec::new();
    for l in free_lots(&w, LandUse::Residential).into_iter().take(2) {
        blocks.push(homes(&mut w, l));
    }
    let ci = w.promote_to_corp(a).unwrap();
    let owner = w.corps.corps[ci].owner;
    // Someone living in town works for the corp.
    let worker = w.citizens.iter().find(|(_, c)| c.can_work() && c.home.is_some() && c.household.is_some()).map(|(id, _)| id).expect("a resident");
    if let Some(old) = w.citizens[worker].work {
        w.buildings[old].workers.retain(|x| *x != worker);
    }
    let corp_staff: Vec<CitizenId> = w.corp_staff(owner);
    for s in corp_staff {
        w.citizens[s].work = None;
    }
    for p in w.estate.owners[owner].properties.clone() {
        w.buildings[p].workers.clear();
    }
    w.citizens[worker].work = Some(a);
    w.buildings[a].workers.push(worker);
    w.citizens[worker].income = 200.0;
    let hid = w.citizens[worker].household.unwrap();
    let home = w.households[hid].home.unwrap();
    // The corp owns a block with room, not the one the worker lives in.
    let block = *blocks.iter().find(|b| **b != home && w.buildings[**b].free_homes() > 0).expect("a block with room");
    w.give_to(block, owner, 0.0);
    assert_eq!(w.corp_staff(owner), vec![worker]);
    w.house_staff(ci);
    assert_eq!(w.households[hid].home, Some(block), "moved into staff housing");
    assert_eq!(w.brand_mark(block).map(|m| m.role), Some(BrandRole::Housing));
    let full = w.buildings[block].rent;
    assert!((w.rent_due(hid, block) - full * (1.0 - STAFF_RENT_DISCOUNT)).abs() < 1e-3, "staff pay less");
    if let Some((other, oh)) = w.households.iter().find(|(h, x)| *h != hid && x.home.is_some()).map(|(h, x)| (h, x.home.unwrap())) {
        if !w.households[other].members.iter().any(|m| w.citizens.get(*m).is_some_and(|c| c.work == Some(a))) {
            assert!((w.rent_due(other, oh) - w.buildings[oh].rent).abs() < 1e-3, "others pay the full rent");
        }
    }
    assert_eq!(w.corps.stats.housed_today, 1);

    // The cap: at the lowest share it may hold only so many.
    w.set_corp_rules(None, Some(0.0), None);
    assert_eq!(w.corps.rules.max_share, MIN_MAX_SHARE, "clamped");
    let cap = w.corp_cap();
    w.corps.corps[ci].cash = 1e9;
    for _ in 0..40 {
        w.corp_buy(ci);
    }
    assert!(w.estate.owners[owner].properties.len() <= cap, "{} > {cap}", w.estate.owners[owner].properties.len());
    // Handed more than the cap, it sells down, never the HQ.
    let extra: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.plot.owner.is_none() && b.land_use == LandUse::Residential && b.construction.is_none()).map(|(id, _)| id).take(4).collect();
    for b in &extra {
        w.give_to(*b, owner, 0.0);
    }
    for _ in 0..10 {
        w.divest(ci);
    }
    assert!(w.estate.owners[owner].properties.len() <= cap);
    if let Some(hq) = w.corps.corps[ci].hq {
        assert!(w.estate.owners[owner].properties.contains(&hq), "the HQ is kept");
    }
}

#[test]
fn the_levy_takes_a_share_of_the_corps_takings_for_the_treasury() {
    let mut w = two_streets();
    let (a, b) = firm(&mut w, 1000.0, 0);
    let ci = w.promote_to_corp(a).unwrap();
    for bid in [a, b] {
        let z = w.buildings[bid].business.as_mut().unwrap();
        z.books = vec![DayBook { revenue: 1000.0, spend: 400.0 }];
        z.money = 5_000.0;
    }
    w.set_corp_rules(None, None, Some(0.1));
    let before = w.economy.treasury;
    w.corp_books(ci);
    let c = &w.corps.corps[ci];
    assert!((c.revenue_yesterday - 2000.0).abs() < 1e-3, "{}", c.revenue_yesterday);
    assert!((c.levy_total - 200.0).abs() < 1e-3, "{}", c.levy_total);
    assert!(w.economy.treasury > before + 199.0);
    assert!(w.buildings[a].business.as_ref().unwrap().money < 5_000.0, "the corp skimmed the profit");
    // Corps can be switched off.
    w.set_corp_rules(Some(false), None, None);
    assert!(!w.corps.rules.allowed);
}

// ── universities ─────────────────────────────────────────────────────────

/// A campus road: a big civic plot in the middle, homes either side, a
/// park and shops across the street, a bar nearby and shops far away.
fn campus_town() -> (World, BuildingId) {
    let mut w = town();
    w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 1, 1);
    w.add_zone(rect(-70.0, 8.0, 140.0, 110.0), LandUse::Civic);
    w.add_zone(rect(-330.0, 8.0, 250.0, 60.0), LandUse::Residential);
    w.add_zone(rect(80.0, 8.0, 250.0, 60.0), LandUse::Residential);
    w.add_zone(rect(-120.0, -68.0, 90.0, 60.0), LandUse::Park);
    w.add_zone(rect(-20.0, -68.0, 300.0, 60.0), LandUse::Commercial);
    w.add_zone(rect(290.0, -68.0, 80.0, 60.0), LandUse::Entertainment);
    w.add_zone(rect(700.0, -68.0, 150.0, 60.0), LandUse::Commercial);
    for l in free_lots(&w, LandUse::Residential) {
        homes(&mut w, l);
    }
    for l in free_lots(&w, LandUse::Park) {
        w.build_on(l);
        if let Some(b) = w.lots[l].building {
            w.finish_construction(b);
        }
    }
    let big = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.car_access()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id).unwrap();
    let uni = w.build_service(big, ServiceKind::University, Tier::Low).expect("a university on the big plot");
    (w, uni)
}

#[test]
fn a_university_buys_dorms_claims_the_park_and_builds_its_annexes_nearby() {
    let (mut w, uni) = campus_town();
    assert_eq!(w.corps.campuses.len(), 1, "the campus was founded");
    let k = w.corps.campuses[0].clone();
    assert_eq!(k.building, uni);
    assert_eq!(k.brand.shape, LogoShape::Shield);
    assert!(matches!(w.estate.owners[k.owner].kind, OwnerKind::University { .. }));
    assert_eq!(w.brand_mark(uni).map(|m| m.role), Some(BrandRole::Campus));
    people(&mut w);
    let at = w.lots[w.buildings[uni].lot].center;
    for _ in 0..30 {
        w.clock.tick += 1440;
        w.daily_campuses();
        let sites: Vec<BuildingId> = w.corps.campuses[0].annexes.iter().map(|(b, _)| *b).filter(|b| w.buildings[*b].construction.is_some()).collect();
        for s in sites {
            w.finish_construction(s);
        }
    }
    let k = &w.corps.campuses[0];
    let kinds: Vec<Annex> = k.annexes.iter().map(|(_, a)| *a).collect();
    assert!(kinds.contains(&Annex::Dorm), "a dorm: {kinds:?}");
    assert!(kinds.contains(&Annex::Plaza), "the park became campus plaza: {kinds:?}");
    assert!(kinds.iter().any(|a| matches!(a, Annex::Admin | Annex::Sports | Annex::Theatre | Annex::Frat)), "an annex built: {kinds:?}");
    assert!(kinds.iter().filter(|a| **a != Annex::Plaza).count() <= ANNEX_CAP[Tier::Low.index()], "within its cap");
    for (b, a) in &k.annexes {
        let d = w.lots[w.buildings[*b].lot].center.distance(at);
        assert!(d <= CAMPUS_REACH + 1.0, "{a:?} {d:.0} m from campus");
        assert_eq!(w.buildings[*b].plot.owner, Some(k.owner));
        assert!(matches!(w.brand_mark(*b).map(|m| m.role), Some(BrandRole::Annex(_))));
    }
    assert!(!k.students.is_empty(), "students enrolled");
    let s = k.students.iter().copied().find(|s| w.citizens[*s].home.is_some_and(|h| k.annex_of(h) == Some(Annex::Dorm))).expect("a student lives in a dorm");
    let (hid, home) = (w.citizens[s].household.unwrap(), w.citizens[s].home.unwrap());
    assert!(w.rent_due(hid, home) < w.buildings[home].rent, "dorm rent is discounted");
    // Nobody else rents a dorm room.
    assert!(w.choose_home(1000.0, None, false).is_none_or(|b| w.buildings[b].annex.is_none()));
}

#[test]
fn students_bring_trade_to_pizza_places_and_bars_by_campus() {
    let (mut w, uni) = campus_town();
    people(&mut w);
    let at = w.lots[w.buildings[uni].lot].center;
    let shops = free_lots(&w, LandUse::Commercial);
    let near_lot = *shops.iter().min_by(|a, b| w.lots[**a].center.distance(at).total_cmp(&w.lots[**b].center.distance(at))).unwrap();
    let far_lot = *shops.iter().max_by(|a, b| w.lots[**a].center.distance(at).total_cmp(&w.lots[**b].center.distance(at))).unwrap();
    assert!(w.lots[far_lot].center.distance(at) > STUDENT_TRADE_REACH);
    let near = business(&mut w, near_lot, BusinessKind::Bakery, 100.0);
    let far = business(&mut w, far_lot, BusinessKind::Bakery, 100.0);
    let bar_lot = free_lots(&w, LandUse::Entertainment)[0];
    let bar = business(&mut w, bar_lot, BusinessKind::Venue, 100.0);
    // A full intake, by stand-ins.
    let template = w.citizens.values().next().cloned().unwrap();
    while w.corps.campuses[0].students.len() < 100 {
        let id = w.citizens.insert(Citizen { age: 20, home: None, work: None, household: None, ..template.clone() });
        w.corps.campuses[0].students.push(id);
    }
    let pull = w.campus_pull();
    let fn_near = World::student_feet(&pull, w.lots[near_lot].center);
    let fn_far = World::student_feet(&pull, w.lots[far_lot].center);
    assert!(fn_near > 1.0 && fn_far == 0.0, "students walk in near campus only: {fn_near} vs {fn_far}");
    // An open hour with nobody passing by: only students come in.
    for b in [near, far] {
        let z = w.buildings[b].business.as_mut().unwrap();
        z.stock.insert(Resource::BakedGoods, 50.0);
        z.revenue_today = 0.0;
    }
    for l in w.lots.values_mut() {
        l.footfall_hour = 0;
    }
    w.clock = crate::clock::SimClock::starting_at(3, 12, 0);
    w.hourly_economy();
    let rn = w.buildings[near].business.as_ref().unwrap().revenue_today;
    let rf = w.buildings[far].business.as_ref().unwrap().revenue_today;
    assert!(rn > rf, "the pizza place by campus sold more: {rn} vs {rf}");
    assert!(w.corps.stats.student_trade_today > 0.0);
    // A new shop by a busy campus opens as a pizza place (half the plots).
    let picks: Vec<Option<BusinessKind>> = shops.iter().filter(|l| w.lots[**l].center.distance(at) <= CAMPUS_REACH).map(|l| w.campus_business(*l, LandUse::Commercial)).collect();
    assert!(picks.iter().any(|k| *k == Some(BusinessKind::Bakery)), "{picks:?}");
    assert!(w.campus_business(far_lot, LandUse::Commercial).is_none());
    // At night, students living here go out to a haunt near campus.
    let stand_ins = std::mem::take(&mut w.corps.campuses[0].students);
    for s in stand_ins {
        w.citizens.remove(s);
    }
    let real: Vec<CitizenId> = w.citizens.iter().filter(|(_, c)| c.home.is_some() && c.household.is_some() && c.can_work()).map(|(id, _)| id).take(40).collect();
    assert!(real.len() >= 6, "residents to send out");
    for c in &real {
        w.citizens[*c].activity = Activity::AtHome;
        w.citizens[*c].savings = 50.0;
    }
    w.corps.campuses[0].students = real.clone();
    w.clock = crate::clock::SimClock::starting_at(4, 20, 29);
    run(&mut w, 1);
    assert_eq!(w.clock.minute_of_day(), NIGHT_OUT_MINUTE);
    assert!(w.corps.stats.students_out_today > 0, "some went out");
    let going: Vec<BuildingId> = real
        .iter()
        .filter_map(|c| match &w.citizens[*c].activity {
            Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } => Some(*b),
            Activity::Visiting(b) => Some(*b),
            _ => None,
        })
        .collect();
    assert!(going.iter().all(|b| w.lots[w.buildings[*b].lot].center.distance(at) <= STUDENT_TRADE_REACH), "to haunts near campus");
    assert!(going.contains(&bar) || going.contains(&near), "the bar or the pizza place: {going:?}");
}

// ── schools and families ─────────────────────────────────────────────────

#[test]
fn families_with_children_prefer_homes_a_school_covers_and_the_childless_do_not_care() {
    let mut w = town();
    w.add_road(&[Vec2::new(-1100.0, 0.0), Vec2::new(1100.0, 0.0)], 1, 1);
    w.add_zone(rect(-1050.0, 8.0, 300.0, 60.0), LandUse::Residential);
    w.add_zone(rect(750.0, 8.0, 300.0, 60.0), LandUse::Residential);
    w.add_zone(rect(590.0, 8.0, 150.0, 60.0), LandUse::Civic);
    let mut west = Vec::new();
    let mut east = Vec::new();
    for l in free_lots(&w, LandUse::Residential) {
        let b = homes(&mut w, l);
        w.buildings[b].tier = Tier::Mid;
        w.buildings[b].recompute_capacity();
        w.buildings[b].rent = 20.0;
        if w.lots[l].center.x < 0.0 {
            west.push(b);
        } else {
            east.push(b);
        }
    }
    let school_lot = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.building.is_none()).map(|(id, _)| id).next().expect("a civic lot");
    w.build_service(school_lot, ServiceKind::School, Tier::Mid).unwrap();
    w.refresh_coverage();
    assert!(east.iter().any(|b| w.school_cover(*b) >= super::campus::FAMILY_SCHOOL_COVER), "the school covers the east end");
    assert!(west.iter().all(|b| w.school_cover(*b) == 0.0), "not the west end");
    let fam: Vec<Option<BuildingId>> = (0..20).map(|_| w.choose_home(200.0, None, true)).collect();
    assert!(fam.iter().all(|b| b.is_some_and(|b| w.school_cover(b) >= super::campus::FAMILY_SCHOOL_COVER)), "families pick homes near the school");
    let single: Vec<Option<BuildingId>> = (0..40).map(|_| w.choose_home(200.0, None, false)).collect();
    assert!(single.iter().any(|b| b.is_some_and(|b| west.contains(&b))), "the childless take either");
    // A family out west moves near the school sooner or later.
    people(&mut w);
    let hid = match w.households.iter().find(|(h, x)| x.home.is_some_and(|b| west.contains(&b)) && w.has_children(*h)).map(|(h, _)| h) {
        Some(h) => h,
        None => {
            let hid = w.households.iter().find(|(h, _)| w.has_children(*h)).map(|(h, _)| h).expect("a family arrived");
            let to = *west.iter().find(|b| w.buildings[**b].free_homes() > 0).expect("room out west");
            w.move_household(hid, to, "test");
            hid
        }
    };
    let income = w.household_income(hid).max(200.0);
    let mut moved = false;
    for _ in 0..80 {
        w.clock.tick += 1440;
        if w.family_school_move(hid, income) {
            moved = true;
            break;
        }
    }
    assert!(moved, "the family moved");
    let home = w.households[hid].home.unwrap();
    assert!(w.school_cover(home) >= super::campus::FAMILY_SCHOOL_COVER);
    // A childless household out west stays put.
    if let Some(single) = w.households.iter().find(|(h, x)| x.home.is_some_and(|b| west.contains(&b)) && !w.has_children(*h)).map(|(h, _)| h) {
        for _ in 0..40 {
            w.clock.tick += 1440;
            assert!(!w.family_school_move(single, 500.0));
        }
    }
}

#[test]
fn a_well_staffed_school_lifts_land_value_more_than_an_empty_one() {
    let mut w = town();
    w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 1, 1);
    w.add_zone(rect(-100.0, 8.0, 200.0, 60.0), LandUse::Civic);
    let lot = free_lots(&w, LandUse::Civic)[0];
    let s = w.build_service(lot, ServiceKind::School, Tier::Mid).unwrap();
    let at = w.lots[lot].center + Vec2::new(0.0, 120.0);
    w.update_land_value();
    let empty = w.land_value.at(at);
    w.buildings[s].workers = vec![CitizenId::default(); w.buildings[s].jobs as usize];
    w.update_land_value();
    let full = w.land_value.at(at);
    w.buildings[s].workers.clear();
    assert!(full > empty, "{full} > {empty}");
}

// ── saves and determinism ────────────────────────────────────────────────

#[test]
fn corps_and_campuses_are_saved_and_deterministic() {
    let build = || {
        let (mut w, _) = campus_town();
        w.add_zone(rect(-650.0, -68.0, 300.0, 60.0), LandUse::Commercial);
        let lots = free_lots(&w, LandUse::Commercial);
        let a = business(&mut w, lots[0], BusinessKind::Office, 600.0);
        w.promote_to_corp(a).unwrap();
        people(&mut w);
        for _ in 0..4 {
            w.clock.tick += 1440;
            w.daily_corps();
        }
        w
    };
    let a = build();
    let b = build();
    let ka: Vec<(String, u8, String)> = a.corps.corps.iter().map(|c| (c.brand.name.clone(), c.brand.colour, format!("{:?}", c.hq.map(|h| a.buildings[h].lot)))).collect();
    let kb: Vec<(String, u8, String)> = b.corps.corps.iter().map(|c| (c.brand.name.clone(), c.brand.colour, format!("{:?}", c.hq.map(|h| b.buildings[h].lot)))).collect();
    assert_eq!(ka, kb);
    assert_eq!(a.corps.campuses[0].annexes.len(), b.corps.campuses[0].annexes.len());
    assert_eq!(a.corps.campuses[0].students.len(), b.corps.campuses[0].students.len());
    let back = World::from_ron(&a.to_ron().unwrap()).unwrap();
    assert_eq!(back.corps.corps, a.corps.corps);
    assert_eq!(back.corps.campuses, a.corps.campuses);
    assert_eq!(back.corps.rules, a.corps.rules);
}

#[test]
fn an_old_save_loads_with_no_corps_and_runs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../saves/flags/flag_patch.ron");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no {}", path.display());
        return;
    };
    assert!(!text.contains("corps:"), "a save from before mega-corps");
    let mut w = World::from_ron(&text).expect("the old save loads");
    assert!(w.corps.corps.is_empty() && w.corps.campuses.is_empty());
    assert_eq!(w.corps.rules, CorpRules::default());
    assert!(w.buildings.values().all(|b| b.annex.is_none()));
    let mut minds = IdleMind;
    let start = w.clock.day();
    while w.clock.day() == start || w.clock.minute_of_day() < 30 {
        w.tick(&mut minds);
    }
    let again = World::from_ron(&w.to_ron().unwrap()).expect("and saves again");
    assert_eq!(again.corps.rules, w.corps.rules);
}
