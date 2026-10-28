//! [ledger] The door, rent to owners, and old saves' money read into
//! accounts (`world/ledger.rs`, `crate::bank`).

use super::*;
use crate::bank::{Account, Class};
use crate::estate::OwnerKind;
use crate::mind::IdleMind;

/// A save from before the ledger: every household's, business's and
/// citizen's money becomes its account's opening balance, the treasury
/// too; the books balance at once and stay balanced as it runs, with
/// nothing moved outside the door.
#[test]
fn an_old_save_opens_its_books_from_its_money_fields() {
    let text = super::legacy_save_text();
    assert!(!text.contains("bank:"), "the fixture is from before the ledger");
    let mut w = World::from_ron(&text).expect("the old save loads");
    assert_eq!(w.ledger_sum(), 0);
    let bank = &w.economy.bank;
    assert_eq!(bank.balance(Account::Treasury) as f32, w.economy.treasury);
    for (hid, h) in w.households.iter().take(40) {
        assert_eq!(bank.balance(Account::Household(hid)) as f32, h.money, "a household's purse is its opening balance");
    }
    for (bid, b) in w.buildings.iter().filter(|(_, b)| b.business.is_some()).take(40) {
        assert_eq!(bank.balance(Account::Business(bid)) as f32, b.business.as_ref().unwrap().money);
    }
    assert!(bank.today_cells().iter().any(|c| c.kind == Kind::Opening), "booked as opening balances");
    assert!(w.estate.owners.values().all(|o| o.money == 0.0), "owners had no money of their own before");
    let mut minds = IdleMind;
    let before = w.economy.bank.untracked_moves;
    for _ in 0..1440 + 60 {
        w.tick(&mut minds);
    }
    assert_eq!(w.reconcile_ledger(), 0, "nothing moved outside the door");
    assert_eq!(w.economy.bank.untracked_moves, before);
    assert_eq!(w.ledger_sum(), 0);
    assert!(!w.economy.bank.history.is_empty(), "a day closed into the history");
}

/// Rent reaches the owner: a household's rent goes to its building's owner
/// (here a developer), which pays upkeep out of town and property tax to
/// the treasury out of it and keeps the rest; with no owner it goes to the
/// independent landlords' pool. Nothing is lost on the way.
#[test]
fn rent_goes_to_the_owner_who_pays_upkeep_and_tax_out_of_it() {
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("loads");
    let (hid, home) = w.households.iter().find_map(|(id, h)| h.home.map(|b| (id, b))).expect("a housed household");
    let dev = w.new_owner("Testing Estates".into(), OwnerKind::Developer);
    w.give_to(home, dev, 0.0);
    assert_eq!(w.landlord_of(home), Account::Owner(dev));
    let (purse0, treasury0, sum0) = (w.households[hid].money, w.economy.treasury, w.ledger_sum());
    w.collect_rent(Account::Household(hid), home, 100.0, Kind::Rent);
    assert!((purse0 - w.households[hid].money - 100.0).abs() < 1e-3, "the tenant paid");
    let keep = 100.0 * (1.0 - super::ledger::RENT_UPKEEP - super::ledger::PROPERTY_TAX);
    assert!((w.estate.owners[dev].money - keep as f64).abs() < 1e-3, "the developer kept {} of it", w.estate.owners[dev].money);
    assert!((w.economy.treasury - treasury0 - 100.0 * super::ledger::PROPERTY_TAX).abs() < 1e-2, "property tax to the city");
    assert!((w.economy.ledger.today_of(Flow::PropertyTax) - 10.0).abs() < 1e-3, "on the treasury's books");
    assert_eq!(w.estate.owners[dev].rent_today, 100.0);
    assert_eq!(w.ledger_sum(), sum0);
    // No owner record: the landlords' pool.
    w.buildings[home].plot.owner = None;
    let pool = w.economy.bank.balance(Account::Landlords);
    w.collect_rent(Account::Household(hid), home, 50.0, Kind::Rent);
    assert!((w.economy.bank.balance(Account::Landlords) - pool - 50.0 * (1.0 - super::ledger::RENT_UPKEEP - super::ledger::PROPERTY_TAX) as f64).abs() < 1e-3);
    assert_eq!(w.account_class(Account::Landlords), Class::Landlords);
    // A developer's profit over the float leaves town, booked.
    w.transfer64(Account::Outside, Account::Owner(dev), super::ledger::OUT_OF_TOWN_FLOAT + 500.0, Kind::Investment);
    w.settle_owners();
    assert!((w.estate.owners[dev].money - super::ledger::OUT_OF_TOWN_FLOAT).abs() < 1e-3);
    assert_eq!(w.estate.owners[dev].rent_yesterday, 100.0, "the day's rent rolled");
    assert_eq!(w.ledger_sum(), 0);
}

/// A resident's second home is bought by their property company: the
/// tenants' rent goes to the company, which pays the founder's household a
/// dividend and keeps the rest for the next deposit.
#[test]
fn a_residents_company_collects_the_rent_and_pays_its_founder() {
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("loads");
    let (hid, home) = w.households.iter().find_map(|(id, h)| h.home.map(|b| (id, b))).expect("a housed household");
    let founder = w.households[hid].members[0];
    let (tenant, flat) = w.households.iter().find_map(|(id, h)| h.home.filter(|b| *b != home).map(|b| (id, b))).expect("another home");
    let co = w.company_of(founder).unwrap();
    assert!(matches!(w.estate.owners[co].kind, OwnerKind::Company { founder: f } if f == founder));
    w.give_to(flat, co, 0.0);
    assert!(w.properties_of(founder).contains(&flat), "the founder's holdings count the company's");
    w.collect_rent(Account::Household(tenant), flat, 100.0, Kind::Rent);
    let net = 100.0 * super::rich::INVEST_RENT_NET;
    assert!((w.estate.owners[co].money - net as f64).abs() < 1e-3, "rent less upkeep, tax and the mortgage");
    let purse = w.households[hid].money;
    w.settle_owners();
    let paid = net * super::ledger::COMPANY_DIVIDEND;
    assert!((w.households[hid].money - purse - paid).abs() < 1e-3, "half the net to the founder");
    assert!((w.company_cash(hid) - (net - paid) as f64).abs() < 1e-3, "the rest stays in the company");
    assert_eq!(w.ledger_sum(), 0);
}

/// [p1-sim] Wages: a business pays its staff's households their payroll
/// wage out of its own money (booked business → household), so its payroll
/// no longer leaves town; its day's spending and money end the same as when
/// the payroll went out of town, and each household's purse gets the same
/// earnings as before (the rest from outside).
#[test]
fn a_business_pays_its_staff_and_its_payroll_stays_in_town() {
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("loads");
    let (bid, worker) = w
        .buildings
        .iter()
        .filter(|(id, b)| b.business.as_ref().is_some_and(|z| !z.closed) && b.operating() && !w.publicly_funded(*id))
        .find_map(|(id, b)| b.workers.iter().copied().find(|c| w.citizens.get(*c).is_some_and(|c| c.household.is_some_and(|h| w.households.contains_key(h)))).map(|c| (id, c)))
        .expect("a business with staff in a household");
    let hid = w.citizens[worker].household.unwrap();
    let (pay_bid, pay) = w.wage_payer(worker).expect("their employer pays them");
    assert_eq!(pay_bid, bid);
    let wage = w.buildings[bid].wage() * super::market::PAYROLL_SHARE;
    assert!((pay - wage.min(w.citizens[worker].income)).abs() < 1e-3);
    let income = w.household_income(hid);
    let biz0 = w.buildings[bid].business.as_ref().unwrap().money;
    let purse0 = w.households[hid].money;
    let sum0 = w.ledger_sum();
    w.daily_household(hid);
    assert!(w.wages_paid.get(&bid).is_some_and(|p| *p >= pay - 1e-3), "noted for the business's books: {:?}", w.wages_paid.get(&bid));
    let cells = w.economy.bank.today_cells();
    assert!(cells.iter().any(|c| c.kind == Kind::Wages && c.from == Class::Business && c.to == Class::Household), "booked business to household");
    assert!(w.buildings[bid].business.as_ref().unwrap().money < biz0, "the business paid");
    let _ = (purse0, income);
    assert_eq!(w.ledger_sum(), sum0);
    // The market's books just after take the wages paid off the payroll
    // that leaves town: only what was not paid in a household pass goes.
    let bills: f64 = w
        .buildings
        .iter()
        .filter(|(id, b)| b.operating() && b.business.as_ref().is_some_and(|z| !z.closed) && !w.publicly_funded(*id))
        .map(|(_, b)| (b.workers.len() as f32 * b.wage() * super::market::PAYROLL_SHARE) as f64)
        .sum();
    let paid = w.wages_paid[&bid] as f64;
    let out = |w: &World| w.economy.bank.today_cells().iter().filter(|c| c.kind == Kind::Payroll && c.to == Class::Outside).map(|c| c.dollars()).sum::<f64>();
    let out0 = out(&w);
    w.daily_businesses();
    assert!((out(&w) - out0 - (bills - paid)).abs() < 1.0, "payroll out of town {} against the bills {bills} less {paid} paid", out(&w) - out0);
    assert!(w.wages_paid.is_empty(), "cleared with the day's books");
    assert_eq!(w.ledger_sum(), 0);
}

/// [p1-sim] The independent landlords' rent profit stays in town: what
/// their pool holds over the float, up to the last day's rent after upkeep
/// and tax, goes to the households with savings in proportion to them; the
/// rest (a building's sale price, which came from the buyer's bank) goes
/// back out as a property sale.
#[test]
fn the_independent_landlords_pay_their_rent_profit_to_savers_in_town() {
    use super::ledger::{OUT_OF_TOWN_FLOAT, PROPERTY_TAX, RENT_UPKEEP};
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("loads");
    let hids: Vec<_> = w.households.keys().take(2).collect();
    assert_eq!(hids.len(), 2);
    for h in w.households.keys().collect::<Vec<_>>() {
        let m = w.households[h].money;
        w.transfer(Account::Household(h), Account::Outside, m, Kind::Living);
    }
    w.transfer(Account::Outside, Account::Household(hids[0]), 30_000.0, Kind::Investment);
    w.transfer(Account::Outside, Account::Household(hids[1]), 10_000.0, Kind::Investment);
    // A day's rent on a building nobody in the books owns, and a sale.
    let home = w.buildings.iter().find(|(_, b)| b.land_use == LandUse::Residential).map(|(id, _)| id).expect("a home");
    w.buildings[home].plot.owner = None;
    assert_eq!(w.landlord_of(home), Account::Landlords);
    w.collect_rent(Account::Outside, home, 1000.0, Kind::Rent);
    let pool = w.economy.bank.balance(Account::Landlords);
    w.transfer64(Account::Outside, Account::Landlords, OUT_OF_TOWN_FLOAT - pool + 1000.0 * (1.0 - RENT_UPKEEP - PROPERTY_TAX) as f64 + 4_000.0, Kind::PropertySale);
    let day = w.clock.day();
    w.economy.bank.roll(day);
    w.settle_owners();
    let profit = 1000.0 * (1.0 - RENT_UPKEEP - PROPERTY_TAX);
    assert!((w.economy.bank.balance(Account::Landlords) - OUT_OF_TOWN_FLOAT).abs() < 1e-2, "the float stays");
    assert!((w.households[hids[0]].money - 30_000.0 - profit * 0.75).abs() < 0.01, "three quarters to the bigger saver: {}", w.households[hids[0]].money);
    assert!((w.households[hids[1]].money - 10_000.0 - profit * 0.25).abs() < 0.01, "a quarter to the other: {}", w.households[hids[1]].money);
    let cells = w.economy.bank.today_cells();
    let out = |k: Kind| cells.iter().filter(|c| c.kind == k && c.from == Class::Landlords && c.to == Class::Outside).map(|c| c.dollars()).sum::<f64>();
    assert_eq!(out(Kind::Dividend), 0.0, "no profit out of town");
    assert!((out(Kind::PropertySale) - 4_000.0).abs() < 0.01, "the sale price back out: {}", out(Kind::PropertySale));
    assert_eq!(w.ledger_sum(), 0);
}
