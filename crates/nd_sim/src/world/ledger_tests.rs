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

/// [r15-money] Wages: a business pays its staff's households the whole wage
/// out of its own money, holding back the income tax for the treasury
/// (booked business → treasury); nothing comes from out of town for them,
/// and the market's books count the wage bill as the business's spending.
#[test]
fn a_business_pays_its_staff_the_whole_wage_and_holds_back_the_tax() {
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("loads");
    let (bid, worker) = w
        .buildings
        .iter()
        .filter(|(id, b)| b.business.as_ref().is_some_and(|z| !z.closed) && b.operating() && !w.publicly_funded(*id))
        .find_map(|(id, b)| b.workers.iter().copied().find(|c| w.citizens.get(*c).is_some_and(|c| c.income > 0.0 && c.household.is_some_and(|h| w.households.contains_key(h)))).map(|c| (id, c)))
        .expect("a business with staff in a household");
    let hid = w.citizens[worker].household.unwrap();
    assert_eq!(w.earnings_payer(worker), super::market::Payer::Business(bid));
    let members = w.households[hid].members.clone();
    let mut staff_gross = 0.0;
    for m in &members {
        if w.earnings_payer(*m) == super::market::Payer::Business(bid) {
            staff_gross += w.pay_bill(*m).0;
        }
    }
    let biz0 = w.buildings[bid].business.as_ref().unwrap().money;
    let sum0 = w.ledger_sum();
    w.daily_household(hid);
    let paid = w.wages_paid.get(&bid).copied().unwrap_or(0.0);
    assert!((paid - staff_gross).abs() < 0.01, "the whole wage on the business's books: {paid} of {staff_gross}");
    let cells = w.economy.bank.today_cells();
    assert!(cells.iter().any(|c| c.kind == Kind::Wages && c.from == Class::Business && c.to == Class::Household), "booked business to household");
    assert!(cells.iter().any(|c| c.kind == Kind::Flow(Flow::IncomeTax) && c.from == Class::Business && c.to == Class::City), "the tax held back, business to treasury");
    // (The household may also have spent some of it there.)
    let biz1 = w.buildings[bid].business.as_ref().unwrap().money;
    assert!(biz0 - biz1 <= staff_gross + 0.05 && biz1 < biz0, "the business paid: {} of {staff_gross}", biz0 - biz1);
    assert_eq!(w.ledger_sum(), sum0);
    // The market's books: no payroll out of town any more.
    let out = |w: &World| w.economy.bank.today_cells().iter().filter(|c| c.kind == Kind::Payroll && c.to == Class::Outside).map(|c| c.dollars()).sum::<f64>();
    w.daily_businesses();
    assert_eq!(out(&w), 0.0, "no payroll leaves town");
    assert!(w.wages_paid.is_empty(), "cleared with the day's books");
    assert_eq!(w.ledger_sum(), 0);
}

/// [r15-money] Who pays whom: the city pays its own staff (civic and
/// service buildings) out of the treasury, an office its staff out of its
/// clients' fees from out of town, pensions and gig money come from out of
/// town; each holds back the income tax.
#[test]
fn the_city_pays_its_staff_and_offices_their_clients_fees() {
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("loads");
    // A civic worker and an office worker, made where the save has none.
    let worker = |w: &World, want: &dyn Fn(&World, BuildingId) -> bool| {
        w.buildings
            .iter()
            .filter(|(id, b)| b.operating() && b.construction.is_none() && want(w, *id))
            .find_map(|(id, b)| b.workers.iter().copied().find(|c| w.citizens.get(*c).is_some_and(|c| c.income > 0.0 && c.household.is_some())).map(|c| (id, c)))
    };
    let civic = worker(&w, &|w, id| w.buildings[id].service.is_some() || w.buildings[id].land_use == LandUse::Civic);
    if let Some((bid, c)) = civic {
        assert_eq!(w.earnings_payer(c), super::market::Payer::City(bid));
        let hid = w.citizens[c].household.unwrap();
        let t0 = w.economy.treasury;
        w.pay_earnings(c, Account::Household(hid));
        let (bill, _) = w.pay_bill(c);
        assert!(w.economy.ledger.today_of(Flow::StaffWages) <= -bill + 0.01, "the wage bill on the staff wages line");
        assert!(w.economy.treasury < t0, "the treasury paid its staff");
    }
    // A retiree: from out of town, the tax held back from it.
    let retiree = w.citizens.iter().find(|(_, c)| c.work.is_none() && c.income > 0.0 && c.household.is_some()).map(|(id, _)| id).expect("someone on a pension or gigs");
    assert_eq!(w.earnings_payer(retiree), super::market::Payer::Outside);
    let hid = w.citizens[retiree].household.unwrap();
    let purse0 = w.households[hid].money;
    let (bill, tax) = w.pay_bill(retiree);
    let net = w.pay_earnings(retiree, Account::Household(hid));
    assert!((net - (bill - tax)).abs() < 0.01, "net of the tax: {net} of {bill} less {tax}");
    let surcharge = w.economy.taxes.surcharge(w.income_tax_category(retiree));
    assert!((net - w.citizens[retiree].income * (1.0 - surcharge)).abs() < 0.01, "the wage is what is kept at the standard rate");
    assert!((w.households[hid].money - purse0 - net).abs() < 0.01);
    // An office: its clients' fees in, its staff's pay out, the till even.
    let office = w.buildings.iter().find(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Office && !z.closed) && b.land_use != LandUse::Civic).map(|(id, _)| id);
    if let Some(o) = office {
        let hid = w.households.keys().next().unwrap();
        let c = w.households[hid].members[0];
        if let Some(old) = w.citizens[c].work {
            w.buildings[old].workers.retain(|x| *x != c);
        }
        w.citizens[c].work = Some(o);
        w.citizens[c].income = 100.0;
        w.buildings[o].workers.push(c);
        assert_eq!(w.earnings_payer(c), super::market::Payer::Office(o));
        let till = w.buildings[o].business.as_ref().unwrap().money;
        let before = w.economy.bank.today_cells().iter().filter(|x| x.kind == Kind::Services).map(|x| x.dollars()).sum::<f64>();
        w.pay_earnings(c, Account::Household(hid));
        let fees = w.economy.bank.today_cells().iter().filter(|x| x.kind == Kind::Services).map(|x| x.dollars()).sum::<f64>() - before;
        assert!((fees - w.pay_bill(c).0 as f64).abs() < 0.01, "the clients paid the office the wage bill: {fees}");
        assert!((w.buildings[o].business.as_ref().unwrap().money - till).abs() < 0.01, "and it paid it on");
    }
    assert_eq!(w.ledger_sum(), 0);
}

/// [r15-money] A day's money is conserved: the books still balance, the
/// change in the money the town holds is exactly what came in from outside
/// less what went out, and every flow across the town edge is one of the
/// named outside flows (`bank::EDGE_KINDS`, the list in docs/DESIGN.md).
#[test]
fn a_days_money_is_conserved_and_only_named_flows_cross_the_edge() {
    let text = super::legacy_save_text();
    let mut w = World::from_ron(&text).expect("loads");
    let mut minds = IdleMind;
    // Every flow across the edge booked so far (the kept days and today):
    // in from outside less out to it.
    let edge = |w: &World| -> f64 {
        let b = &w.economy.bank;
        b.history.iter().flat_map(|d| d.cells.iter().copied()).chain(b.today_cells()).filter(|c| (c.from == Class::Outside) != (c.to == Class::Outside)).map(|c| if c.from == Class::Outside { c.dollars() } else { -c.dollars() }).sum()
    };
    let held = |w: &World| -w.economy.bank.balance(Account::Outside);
    let (held0, edge0, day0) = (held(&w), edge(&w), w.clock.day());
    // A whole day and its midnight (wages, rents, the books).
    for _ in 0..1440 + 30 {
        w.tick(&mut minds);
    }
    let moved = held(&w) - held0;
    let net = edge(&w) - edge0;
    assert!((moved - net).abs() < 0.01 * (1.0 + net.abs() * 1e-6), "the town's money moved by {moved} against the edge flows {net}");
    // Every crossing since is a named one.
    let b = &w.economy.bank;
    let strays: Vec<String> = b
        .history
        .iter()
        .filter(|d| d.day >= day0)
        .flat_map(|d| d.cells.iter().copied())
        .chain(b.today_cells())
        .filter(|c| (c.from == Class::Outside) != (c.to == Class::Outside) && c.kind != Kind::Opening)
        .filter(|c| !crate::bank::EDGE_KINDS.iter().any(|(k, _)| *k == c.kind))
        .map(|c| format!("{:?} {:?}->{:?} {:.0}", c.kind, c.from, c.to, c.dollars()))
        .collect();
    assert!(strays.is_empty(), "flows across the edge not on the list: {strays:?}");
    // Wages crossed only for pay with no payer in town: business staff,
    // city staff and offices are paid in town.
    let cells: Vec<_> = b.history.iter().filter(|d| d.day >= day0).flat_map(|d| d.cells.iter().copied()).chain(b.today_cells()).collect();
    assert!(cells.iter().any(|c| c.kind == Kind::Wages && c.from == Class::Business && c.to == Class::Household), "businesses paid wages");
    assert!(!cells.iter().any(|c| c.kind == Kind::Payroll && c.to == Class::Outside), "no payroll out of town");
    assert!(!cells.iter().any(|c| c.kind == Kind::WalkIn && c.from == Class::Outside), "walk-in custom is paid by those who walk in");
    assert!(!cells.iter().any(|c| c.kind == Kind::Flow(Flow::BusinessTax) && c.from == Class::Outside), "business tax is paid by the business");
    assert_eq!(w.ledger_sum(), 0);
    assert_eq!(w.reconcile_ledger(), 0, "nothing moved outside the door");
}

/// [p1-sim] The independent landlords' rent profit stays in town: what
/// their pool holds over the float, up to the last day's rent after upkeep
/// and tax, goes to the households with savings in proportion to them.
/// [r15-money] The rest (a building's sale price) stays in town too: the
/// pool keeps it to build with and the savers draw `LANDLORD_DRAW` of it a
/// day.
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
    let draw = 4_000.0 * super::ledger::LANDLORD_DRAW as f32;
    assert!((w.economy.bank.balance(Account::Landlords) - OUT_OF_TOWN_FLOAT - (4_000.0 - draw) as f64).abs() < 1e-2, "the float and the sale's capital stay");
    assert!((w.households[hids[0]].money - 30_000.0 - (profit + draw) * 0.75).abs() < 0.01, "three quarters to the bigger saver: {}", w.households[hids[0]].money);
    assert!((w.households[hids[1]].money - 10_000.0 - (profit + draw) * 0.25).abs() < 0.01, "a quarter to the other: {}", w.households[hids[1]].money);
    let cells = w.economy.bank.today_cells();
    let out = |k: Kind| cells.iter().filter(|c| c.kind == k && c.from == Class::Landlords && c.to == Class::Outside).map(|c| c.dollars()).sum::<f64>();
    assert_eq!(out(Kind::Dividend), 0.0, "no profit out of town");
    assert_eq!(out(Kind::PropertySale), 0.0, "the sale price stays in town");
    // A site nobody owns is built with the pool's money now.
    let site = w.buildings.keys().next().unwrap();
    w.buildings[site].plot.owner = None;
    assert_eq!(w.site_payer(site), Account::Landlords);
    assert_eq!(w.ledger_sum(), 0);
}
