//! [r17-goods] The new goods and chains: drinks for the bars, medicine for
//! the pharmacies and hospitals, ready-mix and boards for the sites, the
//! clothes trade, and the nuclear plant's fuel convoy.

use super::*;
use crate::mind::IdleMind;
use crate::power::PowerPlantKind;
use crate::services::ServiceKind;

fn tick(w: &mut World, n: u32) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// A street with homes, shops, civic land and industry along it.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_zone(square(-650.0, 10.0, 300.0, 60.0), LandUse::Residential);
    w.add_zone(square(-300.0, 10.0, 250.0, 70.0), LandUse::Commercial);
    w.add_zone(square(-30.0, 10.0, 180.0, 90.0), LandUse::Civic);
    w.add_zone(square(200.0, 10.0, 480.0, 120.0), LandUse::Industrial);
    tick(&mut w, 2);
    w
}

fn free_lot(w: &World, lu: LandUse) -> LotId {
    w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == lu && l.building.is_none() && l.car_access()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id).expect("a free lot")
}

fn open(w: &mut World, lu: LandUse, kind: BusinessKind, staff: usize) -> BuildingId {
    let lot = free_lot(w, lu);
    let id = w.open_business(lot, kind, true).expect("opens");
    w.buildings[id].jobs = w.buildings[id].jobs.max(staff as u16);
    for k in 0..staff {
        let at = w.lots[lot].center;
        let c = w.citizens.insert(Citizen::newcomer(format!("Hand {k}"), 700 + k as u64, 30, None, None, at, 0));
        w.assign_job(c, id);
    }
    id
}

/// The chains read off the catalogue: every new good has a maker (or a
/// note), bars pour drinks not groceries, the sites take ready-mix from a
/// batching plant and boards from a saw mill.
#[test]
fn the_new_chains_are_in_the_catalogue() {
    use crate::catalogue::{local_source, makers, users};
    let has = |r: Resource, k: BusinessKind| makers(r).any(|m| m == k);
    assert!(has(Resource::Drinks, BusinessKind::Brewery) && has(Resource::Drinks, BusinessKind::Distillery) && has(Resource::Drinks, BusinessKind::Bottler));
    assert!(has(Resource::Medicine, BusinessKind::DrugMaker));
    assert!(has(Resource::Cement, BusinessKind::CementWorks) && has(Resource::Concrete, BusinessKind::BatchingPlant) && has(Resource::Limestone, BusinessKind::Quarry));
    assert!(has(Resource::Timber, BusinessKind::SawMill) && has(Resource::Cloth, BusinessKind::TextileMill) && has(Resource::Clothing, BusinessKind::GarmentWorkshop));
    for k in [BusinessKind::Bar, BusinessKind::Venue, BusinessKind::KaraokeBox, BusinessKind::BeachBar] {
        let r = k.recipe();
        assert!(r.inputs.iter().any(|(x, _)| *x == Resource::Drinks), "{k:?} pours drinks");
        assert!(!r.inputs.iter().any(|(x, _)| *x == Resource::Groceries), "{k:?} no longer buys groceries");
    }
    assert!(users(Resource::Drinks).any(|k| k == BusinessKind::Grocery) && users(Resource::Drinks).any(|k| k == BusinessKind::Diner));
    assert!(users(Resource::Medicine).any(|k| k == BusinessKind::Pharmacy));
    // Sites order ready-mix, not cement; builders buy it and boards from the makers.
    assert!(crate::catalogue::BUILD_MIXES.iter().all(|m| m.lines.iter().all(|l| l.good != Resource::Cement)));
    assert!(crate::catalogue::BUILD_MIXES.iter().any(|m| m.lines.iter().any(|l| l.good == Resource::Concrete)));
    assert_eq!(local_source(Resource::Concrete), Some(Resource::Concrete));
    assert_eq!(local_source(Resource::Timber), Some(Resource::Timber));
    assert_eq!(local_source(Resource::Steel), Some(Resource::Metal));
    assert!(!crate::catalogue::merchant_stock().contains(&Resource::Concrete));
    // The lorries: drinks lorries, cement tankers, mixers, log lorries, the flask carrier.
    use crate::vehicle::{BodyType, Load, VehicleModel};
    let mut rng = crate::vehicle::model_rng(3, 4);
    assert_eq!(VehicleModel::for_cargo(Resource::Drinks, Some(BusinessKind::Brewery), false, false, &mut rng).body, BodyType::DrinksTruck);
    assert_eq!(VehicleModel::for_cargo(Resource::Cement, Some(BusinessKind::CementWorks), false, false, &mut rng).body, BodyType::CementTanker);
    assert_eq!(VehicleModel::for_cargo(Resource::Concrete, None, true, false, &mut rng).body, BodyType::Mixer);
    let logs = VehicleModel::for_cargo(Resource::Logs, None, false, true, &mut rng);
    assert_eq!((logs.body, Load::from_code(logs.boxed)), (BodyType::Flatbed, Some(Load::Logs)), "logs come in on a log lorry, not in a box");
    assert_eq!(VehicleModel::for_cargo(Resource::NuclearFuel, None, false, true, &mut rng).body, BodyType::FlaskCarrier);
}

/// A brewery in town brews from grain, and a bar buys its drinks from it
/// (a drinks lorry), not from the grocers.
#[test]
fn a_bar_buys_its_drinks_from_the_brewery() {
    let mut w = town();
    let brewery = open(&mut w, LandUse::Industrial, BusinessKind::Brewery, 6);
    let bar = open(&mut w, LandUse::Commercial, BusinessKind::Bar, 3);
    w.buildings[brewery].business.as_mut().unwrap().add(Resource::Drinks, 200.0);
    let before = w.buildings[brewery].business.as_ref().unwrap().stock_of(Resource::Drinks);
    let mut revenue = 0.0;
    for _ in 0..6 {
        tick(&mut w, 60);
        revenue += w.buildings[brewery].business.as_ref().unwrap().revenue_today;
    }
    let bz = w.buildings[bar].business.as_ref().unwrap();
    assert!(bz.stock_of(Resource::Drinks) + bz.inbound_of(Resource::Drinks) > 0.0, "the bar has drinks in or on the way");
    let sold = before - w.buildings[brewery].business.as_ref().unwrap().stock_of(Resource::Drinks);
    assert!(sold > 0.0 || revenue > 0.0, "the brewery sold some");
}

/// A hospital uses medicine, orders from the drug maker in town (the city
/// pays, a lorry brings it), and with none in town imports it.
#[test]
fn hospitals_order_medicine_from_the_drug_maker() {
    let mut w = town();
    let lot = free_lot(&w, LandUse::Civic);
    let h = w.build_service(lot, ServiceKind::Health, Tier::Mid).expect("a hospital");
    for _ in 0..(24 * 6) {
        if w.buildings[h].operating() {
            break;
        }
        tick(&mut w, 10);
    }
    assert!(w.buildings[h].operating(), "the hospital opened");
    tick(&mut w, 61);
    // Starts with its keep, which it uses.
    let m0 = w.hospital_medicine(h).expect("on the books");
    assert!(m0 > 0.0);
    // Empty shelves: with no drug maker in town it imports.
    w.goods.medicine.insert(h, 0.0);
    w.goods.medicine_inbound.insert(h, 0.0);
    let imported0 = w.goods.today.medicine_imported;
    w.hospital_supplies();
    assert!(w.goods.today.medicine_imported > imported0, "imported with no maker in town");
    // A drug maker opens: the next order is bought from it.
    let maker = open(&mut w, LandUse::Industrial, BusinessKind::DrugMaker, 6);
    w.buildings[maker].business.as_mut().unwrap().add(Resource::Medicine, 100.0);
    w.seller_index_hour = None;
    w.goods.medicine.insert(h, 0.0);
    w.goods.medicine_inbound.insert(h, 0.0);
    let local0 = w.goods.today.medicine_local;
    let treasury = w.economy.treasury;
    w.hospital_supplies();
    assert!(w.goods.today.medicine_local > local0, "bought from the drug maker");
    assert!(w.economy.treasury < treasury, "the city paid");
    // The lorry gets there.
    for _ in 0..200 {
        if w.hospital_medicine(h).unwrap_or(0.0) > 0.5 {
            break;
        }
        tick(&mut w, 10);
    }
    assert!(w.hospital_medicine(h).unwrap_or(0.0) > 0.5, "medicine on the shelves");
}

/// A new nuclear plant makes nothing until its convoy (an escort, the
/// flask carrier, an escort) brings the fuel; then it runs. A plant from an
/// old save (no entry in the book) is given a core.
#[test]
fn a_nuclear_plant_waits_for_its_fuel_convoy() {
    let mut w = town();
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.car_access()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id).unwrap();
    let id = w.build_power(lot, PowerPlantKind::Nuclear).unwrap_or_else(|e| panic!("no nuclear plant on the test street: {e}"));
    assert_eq!(w.nuclear_fuel(lot).map(|f| f.until), Some(0), "built empty");
    assert!(!w.plant_fuelled(&w.buildings[id]));
    assert_eq!(w.plant_output_mw(&w.buildings[id]), 0.0);
    // The hour sends the convoy, once the lot is cleared.
    let mut sent = false;
    for _ in 0..(60 * 48) {
        tick(&mut w, 1);
        let n = w.traffic.cars.values().filter(|c| c.cargo.is_some_and(|k| k.resource == Resource::NuclearFuel)).count();
        if n > 0 {
            assert_eq!(n, 3, "an escort, the flask carrier and an escort");
            assert!(w.traffic.cars.values().any(|c| c.model.body == crate::vehicle::BodyType::FlaskCarrier));
            sent = true;
            break;
        }
        if w.nuclear_fuel(lot).is_some_and(|f| f.until > 0) {
            // No road for it: handed over at the gate.
            sent = true;
            break;
        }
    }
    assert!(sent, "a convoy went out");
    for _ in 0..(60 * 24) {
        if w.nuclear_fuel(lot).is_some_and(|f| f.fuelled(w.clock.tick)) {
            break;
        }
        tick(&mut w, 1);
    }
    let f = w.nuclear_fuel(lot).unwrap();
    assert!(f.fuelled(w.clock.tick) && f.loads == 1, "{f:?}");
    assert!(w.plant_fuelled(&w.buildings[id]));
    // An old save's plant: no entry, fuelled, and given a core on the hour.
    w.goods.nuclear.clear();
    assert!(w.plant_fuelled(&w.buildings[id]));
    tick(&mut w, 60);
    assert!(w.nuclear_fuel(lot).is_some_and(|f| f.fuelled(w.clock.tick)));
}

/// The pressures read the town's imports: drinks bought in from outside
/// ask for a brewery.
#[test]
fn imports_ask_for_the_new_chains() {
    use crate::pressure::{assess, PressureKind, Signals, Supply};
    let mut s = Signals::default();
    s.imports = vec![0.0; Resource::COUNT];
    s.imports[crate::catalogue::good_index(Resource::Drinks)] = 600.0;
    s.imports[crate::catalogue::good_index(Resource::Timber)] = 30.0;
    let r = assess(&s, &Supply::default());
    let get = |k: PressureKind| r.iter().find(|p| p.kind == k).unwrap().clone();
    assert!(get(PressureKind::Brewery).wants_more(), "{:?}", get(PressureKind::Brewery));
    assert!(!get(PressureKind::SawMill).wants_more());
    assert!(get(PressureKind::Brewery).why.contains("drinks"));
}
