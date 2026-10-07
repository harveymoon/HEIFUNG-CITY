//! Regression (review 2, M6): a dock is a civic structure and
//! `Building::is_special`, so the daily market update never retiers it or
//! recomputes its jobs as an ordinary industrial building. (The review's
//! probe asserted the defect; it now asserts the fix.)
//!
//! cargo test --release -p nd_sim --test probe_society_port -- --nocapture

use nd_sim::landscape::{LandscapeParams, LandscapePreset};
use nd_sim::mind::IdleMind;
use nd_sim::{LandUse, PortKind, Vec2, World, WorldConfig};

#[test]
fn a_port_is_never_retiered_by_the_daily_update() {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, 8), ..WorldConfig::default() });
    w.economy.treasury = 500_000.0;
    let r = &w.landscape.rivers[0];
    let p = *r.points.iter().min_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap();
    let (seg, _, _) = r.nearest(p);
    let flow = r.flow(seg);
    let width = r.width;
    let mut n = Vec2::new(-flow.y, flow.x);
    if n.dot(-p) < 0.0 {
        n = -n;
    }
    let road_at = p + n * (width * 0.5 + 110.0);
    w.add_road(&[road_at - flow * 300.0, road_at + flow * 300.0], 1, 1);
    w.add_zone(vec![road_at + n * 10.0 - flow * 100.0, road_at + n * 10.0 + flow * 100.0, road_at + n * 80.0 + flow * 100.0, road_at + n * 80.0 - flow * 100.0], LandUse::Residential);
    let id = w.place_port(PortKind::ShippingPort, p + n * (width * 0.5 + 20.0)).expect("a port");
    let b = &w.buildings[id];
    assert!(b.is_special(), "a dock is special");
    println!("placed: tier {:?} jobs {} special {} lv {:.2}", b.tier, b.jobs, b.is_special(), w.land_value.at(w.lots[b.lot].center));
    let (tier0, jobs0) = (b.tier, b.jobs);
    let mut minds = IdleMind;
    for day in 1..=90 {
        loop {
            w.tick(&mut minds);
            if w.clock.minute_of_day() == 0 {
                break;
            }
        }
        let Some(b) = w.buildings.get(id) else {
            panic!("day {day}: the port building is gone");
        };
        if b.tier != tier0 || b.jobs != jobs0 || b.construction.is_some() {
            println!("day {day}: port tier {:?} -> {:?}, jobs {jobs0} -> {}, construction {:?}", tier0, b.tier, b.jobs, b.construction.is_some());
            panic!("the market update changed a dock (tier {:?} -> {:?}, jobs {jobs0} -> {})", tier0, b.tier, b.jobs);
        }
    }
    println!("90 days: port unchanged");
}
