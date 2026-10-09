use super::*;

#[test]
fn surplus_charges_then_exports_and_shortfall_drains_then_imports() {
    // Surplus of 50: 30 into the batteries, 20 exported.
    let d = dispatch(150.0, 100.0, 30.0, 0.0);
    assert_eq!((d.charge_mw, d.exported_mw, d.imported_mw, d.curtailed_mw), (30.0, 20.0, 0.0, 0.0));
    assert_eq!(d.charge_share, 1.0);
    // A surplus beyond the batteries and the export line is curtailed.
    let d = dispatch(400.0, 100.0, 10.0, 0.0);
    assert_eq!((d.charge_mw, d.exported_mw), (10.0, GRID_EXPORT_MW));
    assert!((d.curtailed_mw - (290.0 - GRID_EXPORT_MW)).abs() < 1e-3);
    // Shortfall of 40: the batteries give 25, 15 is imported.
    let d = dispatch(60.0, 100.0, 0.0, 25.0);
    assert_eq!((d.discharge_mw, d.imported_mw, d.exported_mw), (25.0, 15.0, 0.0));
    // Batteries with more than enough give a share of what they could.
    let d = dispatch(90.0, 100.0, 0.0, 40.0);
    assert_eq!((d.discharge_mw, d.imported_mw), (10.0, 0.0));
    assert!((d.discharge_share - 0.25).abs() < 1e-6);
    // The import line still caps what is bought.
    assert_eq!(dispatch(0.0, 500.0, 0.0, 0.0).imported_mw, GRID_IMPORT_MW);
    assert!(GRID_EXPORT_PRICE < GRID_IMPORT_PRICE, "selling back never pays for buying");
}

/// [batteries] The day's six cheapest hours charge, its six dearest
/// deliver; a flat day plays nothing; buying cheap only when the round
/// trip pays.
#[test]
fn price_bands_split_the_day_and_the_rule_is_explainable() {
    let mild = crate::weather::Weather::default();
    let prices: Vec<f32> = (0..24).map(|h| import_price(h as f32, mild.temp_at(h as f32))).collect();
    let b = PriceBands::of(&prices);
    assert!(b.cheap < b.dear && b.worth_storing(), "{b:?}");
    let modes: Vec<BatteryMode> = prices.iter().map(|p| b.mode(*p)).collect();
    assert_eq!(modes.iter().filter(|m| **m == BatteryMode::Charge).count(), ARBITRAGE_HOURS, "{prices:?}");
    assert_eq!(modes.iter().filter(|m| **m == BatteryMode::Deliver).count(), ARBITRAGE_HOURS, "{prices:?}");
    // Charging in the small hours, delivering in the evening.
    assert_eq!(modes[3], BatteryMode::Charge, "{modes:?}");
    assert_eq!(modes[19], BatteryMode::Deliver, "{modes:?}");
    // A flat price: the old merit order (deliver in any shortfall).
    let flat = PriceBands::of(&[2.0; 24]);
    assert_eq!(flat.mode(2.0), BatteryMode::Deliver);
    assert!(!flat.worth_storing());
    // Barely different prices do not pay the losses: no buying.
    let tight = PriceBands { cheap: 2.0, dear: 2.1 };
    assert_ne!(tight.mode(2.0), BatteryMode::Charge);
}

#[test]
fn the_hour_rule_charges_holds_and_delivers() {
    // A shortfall of 30 MW with 20 MW of battery room and output.
    let hold = dispatch_tou(100.0, 130.0, 20.0, 20.0, BatteryMode::Hold);
    assert_eq!((hold.imported_mw, hold.discharge_mw, hold.charge_mw), (30.0, 0.0, 0.0), "holding imports first");
    let deliver = dispatch_tou(100.0, 130.0, 20.0, 20.0, BatteryMode::Deliver);
    assert_eq!((deliver.discharge_mw, deliver.imported_mw), (20.0, 10.0), "a dear hour delivers first");
    let cheap = dispatch_tou(100.0, 130.0, 20.0, 20.0, BatteryMode::Charge);
    assert_eq!((cheap.imported_mw, cheap.grid_charge_mw, cheap.charge_mw, cheap.discharge_mw), (50.0, 20.0, 20.0, 0.0), "a cheap hour buys to fill up");
    // Past the import line the battery keeps the lights on, whatever the hour.
    let short = dispatch_tou(0.0, GRID_IMPORT_MW + 15.0, 20.0, 20.0, BatteryMode::Hold);
    assert_eq!((short.imported_mw, short.discharge_mw), (GRID_IMPORT_MW, 15.0));
    let short = dispatch_tou(0.0, GRID_IMPORT_MW + 15.0, 20.0, 20.0, BatteryMode::Charge);
    assert_eq!((short.discharge_mw, short.grid_charge_mw), (15.0, 0.0), "never buys to charge while it must give");
    // A surplus always charges first; a cheap hour tops up from the grid.
    let sun = dispatch_tou(130.0, 100.0, 50.0, 20.0, BatteryMode::Charge);
    assert_eq!((sun.charge_mw, sun.grid_charge_mw, sun.imported_mw, sun.exported_mw), (50.0, 20.0, 20.0, 0.0));
    // Savings: imports spared at the price, less what was bought, less
    // the exports the surplus would have earned.
    assert_eq!(deliver.battery_savings(4.0, 0.8), 20.0 * 4.0);
    assert_eq!(cheap.battery_savings(1.0, 0.2), -20.0);
    let s = dispatch_tou(130.0, 100.0, 10.0, 0.0, BatteryMode::Hold);
    assert!((s.battery_savings(2.0, 0.4) + 10.0 * 0.4).abs() < 1e-4);
}

#[test]
fn a_battery_loses_a_tenth_on_the_round_trip_and_stays_in_bounds() {
    let mut b = Battery::new(BatteryKind::Medium);
    assert_eq!(b.available_mw(), 0.0);
    assert_eq!(b.room_mw(), BatteryKind::Medium.max_mw());
    let mut drawn = 0.0;
    while b.room_mw() > 1e-4 {
        let f = b.room_mw();
        drawn += f;
        b.run_hour(-f);
    }
    assert!((b.charge_mwh - BatteryKind::Medium.capacity_mwh()).abs() < 1e-3, "full: {}", b.charge_mwh);
    let mut given = 0.0;
    while b.available_mw() > 1e-4 {
        let f = b.available_mw();
        given += f;
        b.run_hour(f);
    }
    assert!(b.charge_mwh.abs() < 1e-3);
    let eff = given / drawn;
    assert!((eff - BATTERY_CHARGE_EFF * BATTERY_DISCHARGE_EFF).abs() < 1e-3, "round trip {eff}");
    for k in BatteryKind::ALL {
        assert!(k.capacity_mwh() / k.max_mw() >= 3.0, "{k:?} holds at least three hours at full power");
        assert_eq!(BatteryKind::from_key(k.key()), Some(k));
    }
}

/// A city's mix of full draw: homes, shops, some industry, a little
/// nightlife and civic.
fn city() -> DemandBase {
    DemandBase { by_use: [40.0, 25.0, 25.0, 5.0, 5.0, 0.5], sites: 0.0 }
}

fn day(base: &DemandBase, temp: impl Fn(f32) -> f32) -> Vec<f32> {
    (0..24).map(|h| base.at(h as f32, temp(h as f32))).collect()
}

#[test]
fn demand_peaks_in_the_evening_and_dips_at_night() {
    let mild = |_| 18.0;
    // Homes: a morning bump, a midday lull and the big evening peak.
    let res = |h: f32| hourly_load(LandUse::Residential, h);
    assert!(res(7.0) > res(3.0) && res(7.0) > res(11.0), "morning bump");
    assert!(res(19.0) > res(7.0) && res(19.0) > res(12.0), "evening peak");
    let peak_hour = (0..24).max_by(|a, b| res(*a as f32).total_cmp(&res(*b as f32))).unwrap();
    assert!((18..=21).contains(&peak_hour), "homes peak when people get home: {peak_hour}");
    // Offices run in the day, nightlife at night, industry flat.
    assert!(hourly_load(LandUse::Commercial, 12.0) > 2.5 * hourly_load(LandUse::Commercial, 3.0));
    assert!(hourly_load(LandUse::Entertainment, 21.0) > 2.5 * hourly_load(LandUse::Entertainment, 6.0));
    let ind: Vec<f32> = (0..24).map(|h| hourly_load(LandUse::Industrial, h as f32)).collect();
    let (lo, hi) = ind.iter().fold((9.0f32, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
    assert!(hi / lo < 1.6, "industry runs shifts round the clock");
    assert!(hourly_load(LandUse::Park, 23.0) > 2.0 * hourly_load(LandUse::Park, 12.0), "park lamps at night");
    // Interpolated smoothly between the hours, wrapping at midnight.
    assert!((hourly_load(LandUse::Residential, 23.5) - (PROFILE[0][23] + PROFILE[0][0]) / 2.0).abs() < 1e-5);
    // The city: evening peak over the night trough, and no more than a
    // touch above the old flat draw, so a city that had power has it.
    let base = city();
    let d = day(&base, mild);
    let (lo, hi) = d.iter().fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
    let mean = d.iter().sum::<f32>() / 24.0;
    let peak = d.iter().position(|v| *v == hi).unwrap();
    assert!(hi / lo > 1.6 && hi / lo < 2.4, "peak/trough {}", hi / lo);
    assert!((10..=20).contains(&peak), "the city peaks in the day or evening: {peak}");
    assert!(hi <= base.full() * 1.05, "a mild day's peak stays near the old flat draw: {hi} vs {}", base.full());
    assert!(mean < base.full() * 0.9, "the day uses less than running flat out: {mean}");
}

#[test]
fn heat_and_cold_add_load() {
    let base = city();
    let mild = base.at(15.0, 20.0);
    assert!(base.at(15.0, 40.0) > mild * 1.12, "air conditioning on a hot afternoon");
    assert!(base.at(15.0, -15.0) > mild * 1.12, "heating in the cold");
    assert_eq!(weather_load(LandUse::Park, 45.0), 1.0);
    // Desert, ordinary day: the evening peak with its AC still within
    // a sixth of the old flat draw (no blackouts every evening for a
    // city that had enough before).
    let w = Weather::for_climate(crate::landscape::Climate::Desert);
    let d = day(&base, |h| w.temp_at(h));
    let hi = d.iter().cloned().fold(0.0f32, f32::max);
    assert!(hi < base.full() * 1.15, "desert peak {hi} vs flat {}", base.full());
    // A heat wave pushes it up.
    let hot = Weather { spell: Spell::HeatWave, ..w.clone() };
    let dh = day(&base, |h| hot.temp_at(h));
    assert!(dh.iter().cloned().fold(0.0f32, f32::max) > hi * 1.05);
    // A demand_at for one building follows the same curve.
    let mut b = Building::new(crate::LotId::default(), LandUse::Residential, 10, Tier::Mid, 1, "test".into(), 0, 0.5);
    assert!((demand_at(&b, 19.0, 20.0) - demand_mw(&b) * load_factor(LandUse::Residential, 19.0, 20.0)).abs() < 1e-6);
    b.construction = Some(crate::buildings::Construction::new(1.0));
    assert_eq!(demand_at(&b, 19.0, 40.0), demand_mw(&b), "a site draws its flat figure");
}

#[test]
fn power_is_cheap_at_night_dear_in_the_evening_and_dearer_in_a_heat_wave() {
    let mild = Weather::for_climate(crate::landscape::Climate::Temperate);
    let prices: Vec<f32> = (0..24).map(|h| import_price(h as f32, mild.temp_at(h as f32))).collect();
    let mean = prices.iter().sum::<f32>() / 24.0;
    let (lo, hi) = prices.iter().fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
    let cheapest = prices.iter().position(|v| *v == lo).unwrap();
    let dearest = prices.iter().position(|v| *v == hi).unwrap();
    assert!(cheapest <= 5, "cheapest in the small hours: {cheapest}");
    assert!((16..=21).contains(&dearest), "dearest late afternoon or evening: {dearest}");
    assert!(hi > 2.0 * lo, "a real spread: {lo}..{hi}");
    assert!((mean - GRID_IMPORT_PRICE).abs() < 0.5, "a mild day averages the old price: {mean}");
    // Heat wave evenings cost more.
    let hot = Weather { spell: Spell::HeatWave, ..Weather::for_climate(crate::landscape::Climate::Desert) };
    assert!(import_price(18.0, hot.temp_at(18.0)) > hi * 1.15);
    // Exports always earn less than imports cost.
    for h in 0..24 {
        for t in [-20.0, 10.0, 30.0, 45.0] {
            assert!(export_price(h as f32, t) < import_price(h as f32, t));
        }
    }
}

#[test]
fn the_sun_rises_east_stands_high_at_noon_and_is_down_at_night() {
    assert!(sun_direction(0.0).is_none() && sun_direction(0.9).is_none());
    let morning = sun_direction(0.3).unwrap();
    assert!(morning[0] > 0.0 && morning[1] > 0.0);
    let noon = sun_direction(0.5).unwrap();
    assert!(noon[1] > 0.99);
    let evening = sun_direction(0.7).unwrap();
    assert!(evening[0] < 0.0);
    for t in [0.3, 0.42, 0.5, 0.61, 0.7] {
        let d = sun_direction(t).unwrap();
        assert!(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - 1.0).abs() < 1e-4);
    }
    assert!(PowerPlantKind::ALL.iter().all(|k| k.noise_db() > 30.0 && k.co2_t_per_mwh() >= 0.0));
    assert!(PowerPlantKind::Coal.co2_t_per_mwh() > PowerPlantKind::Geothermal.co2_t_per_mwh());
}
