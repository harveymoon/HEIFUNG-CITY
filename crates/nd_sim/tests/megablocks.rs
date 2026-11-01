//! [megablocks] A mega-corp's arcology lived in by the rule-based minds:
//! people move in, work downstairs, shop and go out inside and go up to
//! the sky gardens, and those trips never touch the streets.
//!
//! cargo test --profile fast -p nd_sim --test megablocks -- --nocapture

use nd_minds::RuleBasedMind;
use nd_sim::WorldConfig;
use nd_sim::{BusinessKind, LandUse, Vec2, World};

fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

#[test]
fn its_people_live_work_shop_and_relax_without_leaving() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: nd_sim::landscape::LandscapeParams::flat(), dead_ends_lead_elsewhere: true, ..WorldConfig::default() });
    w.economy.treasury = 1e7;
    for y in [-100.0f32, 0.0, 100.0] {
        w.add_road(&[Vec2::new(-460.0, y), Vec2::new(460.0, y)], 1, 1);
    }
    for x in [-460.0f32, 460.0] {
        w.add_road(&[Vec2::new(x, -100.0), Vec2::new(x, 0.0), Vec2::new(x, 100.0)], 1, 1);
    }
    w.add_road(&[Vec2::new(460.0, 0.0), Vec2::new(760.0, 0.0)], 1, 1);
    w.add_zone(rect(-440.0, -92.0, 880.0, 84.0), LandUse::Commercial);
    w.add_zone(rect(-440.0, 8.0, 880.0, 84.0), LandUse::Residential);
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial && l.car_access()).min_by(|a, b| a.1.center.x.total_cmp(&b.1.center.x)).map(|(id, _)| id).unwrap();
    let office = w.build_on_as(lot, Some(BusinessKind::Office)).unwrap();
    w.finish_construction(office);
    w.promote_to_corp(office).expect("a corp");
    let bid = w.force_megablock(None, None).expect("its mega-block, built and open");
    w.config.immigration_interval = 2;
    let mut minds = RuleBasedMind::default();
    for _ in 0..1440 * 3 {
        w.tick(&mut minds);
    }
    let r = w.megablock_report(bid).unwrap();
    let m = w.buildings[bid].plot.megablock.unwrap();
    eprintln!(
        "residents {} in {} households of {} homes; work inside {} of {}; jobs {}/{}; shops let {}/{}; offices {}/{}; trips yesterday inside {} out {} gardens {}; inside total {}; garden visits {}; jobs moved inside {}",
        r.residents, r.households, r.homes, r.work_inside, r.workers, r.jobs_filled, r.jobs, r.shops_let, r.shops, r.offices_let, r.offices, r.inside_yesterday, r.out_yesterday, r.gardens_yesterday, m.inside_total, w.megablocks.garden_visits_total, w.megablocks.jobs_moved_inside_total
    );
    assert!(r.residents > 100, "people moved in: {}", r.residents);
    assert!(r.work_inside > 0, "some work downstairs: {} of {}", r.work_inside, r.workers);
    assert!(m.inside_total > 0, "trips that never left the block");
    assert!(w.megablocks.garden_visits_total > 0, "sky garden visits");
    assert!(m.inside_yesterday + m.out_yesterday > 0, "trips counted");
    // An inside trip goes by lift: nobody on their way inside is out on
    // the streets more than a step from the door.
    let door = w.building_entrance(bid).unwrap();
    let before = w.megablocks.inside_trips_total;
    for _ in 0..300 {
        w.tick(&mut minds);
    }
    assert!(w.megablocks.inside_trips_total > before, "still going on");
    let far = w
        .citizens
        .values()
        .filter(|c| matches!(&c.activity, nd_sim::Activity::Traveling { to: Some(t), path, .. } if w.megablock_of(*t) == Some(bid) && path.len() == 1 && path[0] == door))
        .filter(|c| c.pos.distance(door) > 2.0)
        .count();
    assert_eq!(far, 0, "a lift ride never wanders the streets");
}
