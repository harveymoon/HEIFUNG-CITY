//! [ledger] The door money moves by (`transfer`), the accounts behind it,
//! rent to the owners, and the checks that nothing leaks (`crate::bank`).

use super::*;
use crate::bank::{dollars, units, Account, Class, Kind, Mirror};
use crate::estate::{OwnerId, OwnerKind};

/// [ledger] Share of the rent an owner spends on upkeep (repairs, cleaning,
/// insurance, bought from out of town).
pub const RENT_UPKEEP: f32 = 0.30;
/// [ledger] Property tax: a share of the rent collected, to the treasury.
pub const PROPERTY_TAX: f32 = 0.10;
/// [ledger] What a resident investor or a residents' property company pays
/// the bank a day on a property bought with a mortgage, as a share of its
/// rent. With upkeep and tax it leaves the old investor's 45%
/// (`rich::INVEST_RENT_NET`).
pub const RENT_LOAN: f32 = 0.15;
/// [ledger] A business's daily rent for its unit, per job, as a share of
/// what one home in the building rents for (`Building::rent`).
pub const SHOP_RENT_PER_JOB: f32 = 0.1;
/// [ledger] What an out-of-town owner (a developer, the independent
/// landlords' pool) keeps in town before its profit leaves: about this many
/// days of its rent roll.
pub const OUT_OF_TOWN_FLOAT: f64 = 20_000.0;
/// [ledger] Share of a residents' property company's day's net paid to its
/// founder's household; the rest stays in the company for the next deposit.
pub const COMPANY_DIVIDEND: f32 = 0.5;

impl World {
    // ── accounts ─────────────────────────────────────────────────────────

    /// The account an owner's money is in: a household or person's own
    /// purse for residents and billionaires, the owner record for the rest.
    pub fn owner_account(&self, oid: OwnerId) -> Account {
        match self.estate.owners.get(oid).map(|o| &o.kind) {
            Some(OwnerKind::Household { household }) => Account::Household(*household),
            Some(OwnerKind::Person { citizen } | OwnerKind::Elite { citizen }) => self.purse_of(*citizen),
            _ => Account::Owner(oid),
        }
    }

    /// Where `cid` pays from and is paid into: the household's purse, or
    /// their own pocket when they have no household.
    pub fn purse_of(&self, cid: CitizenId) -> Account {
        match self.citizens.get(cid).and_then(|c| c.household).filter(|h| self.households.contains_key(*h)) {
            Some(h) => Account::Household(h),
            None => Account::Person(cid),
        }
    }

    /// Who the rent of `bid` goes to: its owner's account, or the
    /// independent landlords when no owner record holds it.
    pub fn landlord_of(&self, bid: BuildingId) -> Account {
        match self.buildings.get(bid).and_then(|b| b.plot.owner).filter(|o| self.estate.owners.contains_key(*o)) {
            Some(o) => self.owner_account(o),
            None => Account::Landlords,
        }
    }

    /// An account's class, resolved like the door resolves it.
    pub fn account_class(&self, a: Account) -> Class {
        match a {
            Account::Outside => Class::Outside,
            Account::Treasury => Class::City,
            Account::Landlords => Class::Landlords,
            Account::Airline => Class::Airline,
            Account::Household(_) => Class::Household,
            Account::Person(_) => Class::Person,
            Account::Business(_) => Class::Business,
            Account::Tourist(_) => Class::Tourist,
            Account::Tower(_) => Class::Company,
            Account::Owner(o) => match self.estate.owners.get(o).map(|o| &o.kind) {
                Some(OwnerKind::MegaCorp { .. }) => Class::Corp,
                Some(OwnerKind::University { .. }) => Class::University,
                _ => Class::Company,
            },
        }
    }

    /// The field the rest of the sim reads an account's money from, or
    /// `None` when its holder is gone.
    fn mirror(&self, a: Account) -> Option<Mirror> {
        Some(match a {
            Account::Outside | Account::Landlords => Mirror::None,
            Account::Treasury => Mirror::F32(self.economy.treasury),
            Account::Airline => Mirror::F32(self.air.company.money),
            Account::Household(h) => Mirror::F32(self.households.get(h)?.money),
            Account::Person(c) => Mirror::F32(self.citizens.get(c)?.savings),
            Account::Business(b) => Mirror::F32(self.buildings.get(b)?.business.as_ref()?.money),
            Account::Tourist(t) => Mirror::F32(self.tourism.tourists.get(t)?.money),
            Account::Tower(i) => Mirror::F64(self.skyline.developers.get(i as usize)?.capital),
            Account::Owner(o) => {
                let ow = self.estate.owners.get(o)?;
                match ow.kind {
                    OwnerKind::MegaCorp { .. } => Mirror::F64(self.corps.corps.iter().find(|c| c.owner == o)?.cash),
                    OwnerKind::University { .. } => Mirror::F64(self.corps.campuses.iter().find(|c| c.owner == o)?.funds),
                    _ => Mirror::F64(ow.money),
                }
            }
        })
    }

    /// Write an account's balance back into its field.
    fn write_mirror(&mut self, a: Account) {
        let d = self.economy.bank.balance(a);
        match a {
            Account::Outside | Account::Landlords => {}
            Account::Treasury => self.economy.treasury = d as f32,
            Account::Airline => self.air.company.money = d as f32,
            Account::Household(h) => {
                if let Some(h) = self.households.get_mut(h) {
                    h.money = d as f32;
                }
            }
            Account::Person(c) => {
                if let Some(c) = self.citizens.get_mut(c) {
                    c.savings = d as f32;
                }
            }
            Account::Business(b) => {
                if let Some(z) = self.buildings.get_mut(b).and_then(|b| b.business.as_mut()) {
                    z.money = d as f32;
                }
            }
            Account::Tourist(t) => {
                if let Some(t) = self.tourism.tourists.get_mut(t) {
                    t.money = d as f32;
                }
            }
            Account::Tower(i) => {
                if let Some(t) = self.skyline.developers.get_mut(i as usize) {
                    t.capital = d;
                }
            }
            Account::Owner(o) => {
                let kind = self.estate.owners.get(o).map(|o| o.kind.clone());
                match kind {
                    Some(OwnerKind::MegaCorp { .. }) => {
                        if let Some(c) = self.corps.corps.iter_mut().find(|c| c.owner == o) {
                            c.cash = d;
                        }
                    }
                    Some(OwnerKind::University { .. }) => {
                        if let Some(c) = self.corps.campuses.iter_mut().find(|c| c.owner == o) {
                            c.funds = d;
                        }
                    }
                    Some(_) => self.estate.owners[o].money = d,
                    None => {}
                }
            }
        }
    }

    /// The account as the door uses it: owner records of residents turned
    /// into their purse, a gone holder into the outside world, and its
    /// entry in step with its mirror.
    fn settle_account(&mut self, a: Account) -> (Account, Class) {
        let a = match a {
            Account::Owner(o) => self.owner_account(o),
            Account::Person(c) if !self.citizens.contains_key(c) => Account::Outside,
            a => a,
        };
        let Some(m) = self.mirror(a) else {
            self.economy.bank.orphans += 1;
            return (Account::Outside, Class::Outside);
        };
        let class = self.account_class(a);
        self.economy.bank.sync(a, class, m);
        // The field shows the booked balance exactly from here on.
        if !m.shows(self.economy.bank.balance_units(a)) {
            self.write_mirror(a);
        }
        (a, class)
    }

    // ── the door ─────────────────────────────────────────────────────────

    /// Move `amount` dollars from `from` to `to`, booked under `kind`. The
    /// one way money changes hands in the sim: the same amount leaves one
    /// balance and arrives on the other, so nothing is made or lost (a
    /// negative amount moves the other way). Balances may go below zero
    /// (debts, overdrafts), as they always could. Returns what moved.
    pub fn transfer(&mut self, from: Account, to: Account, amount: f32, kind: Kind) -> f32 {
        self.transfer_units(from, to, units(amount as f64), kind) as f32
    }

    /// `transfer` of an `f64` amount (property prices, corporate cash).
    pub fn transfer64(&mut self, from: Account, to: Account, amount: f64, kind: Kind) -> f64 {
        self.transfer_units(from, to, units(amount), kind)
    }

    fn transfer_units(&mut self, from: Account, to: Account, u: i64, kind: Kind) -> f64 {
        if u == 0 {
            return 0.0;
        }
        let (from, to, u) = if u < 0 { (to, from, -u) } else { (from, to, u) };
        let f = self.settle_account(from);
        let t = self.settle_account(to);
        if f.0 == t.0 {
            return 0.0;
        }
        match kind {
            // The treasury's own flows keep the Economy window's ledger.
            Kind::Flow(flow) if t.0 == Account::Treasury => self.economy.credit_from(f.0, f.1, flow, dollars(u) as f32),
            Kind::Flow(flow) if f.0 == Account::Treasury => self.economy.credit_from(t.0, t.1, flow, -dollars(u) as f32),
            _ => self.economy.bank.post(f, t, u, kind),
        }
        self.write_mirror(f.0);
        self.write_mirror(t.0);
        dollars(u)
    }

    /// Someone in town pays the treasury under `flow` (a fare, a fine, a
    /// fee), itemised for the Economy window like `Economy::credit_item`.
    pub fn pay_city(&mut self, from: Account, flow: Flow, amount: f32, item: Option<crate::economy::Item>) -> f32 {
        let paid = self.transfer(from, Account::Treasury, amount, Kind::Flow(flow));
        if let Some(item) = item {
            if paid != 0.0 {
                self.economy.ledger.post_item(flow, item, paid);
            }
        }
        paid
    }

    /// The treasury pays someone in town under `flow` (a gift, a refund).
    pub fn city_pays(&mut self, to: Account, flow: Flow, amount: f32, item: Option<crate::economy::Item>) -> f32 {
        let paid = self.transfer(Account::Treasury, to, amount, Kind::Flow(flow));
        if let Some(item) = item {
            if paid != 0.0 {
                self.economy.ledger.post_item(flow, item, -paid);
            }
        }
        paid
    }

    /// [ledger] Money the player gives the city (a grant, the dev console):
    /// from outside, booked as such.
    pub fn grant_treasury(&mut self, amount: f32) {
        self.transfer(Account::Outside, Account::Treasury, amount, Kind::Grants);
    }

    /// A business's account, when the building has one.
    pub fn business_account(&self, bid: BuildingId) -> Option<Account> {
        self.buildings.get(bid)?.business.as_ref().map(|_| Account::Business(bid))
    }

    /// [ledger] The business in `bid` is replaced by a new one (`fresh`):
    /// the old till is wound up (out of town) and the new one opens with
    /// the float it was made with (from outside).
    pub(in crate::world) fn replace_business(&mut self, bid: BuildingId, fresh: crate::market::Business) {
        self.set_business(bid, Some(fresh));
    }

    /// [ledger] Put `z` (or nothing) in `bid` in place of whatever business
    /// was there: the old account closes at its booked balance (its money
    /// leaves town), the new one opens at its float (from outside). Every
    /// place a building's business is swapped goes through here, so a new
    /// till is never read as money appearing in the old one.
    pub(in crate::world) fn set_business(&mut self, bid: BuildingId, z: Option<crate::market::Business>) {
        let a = Account::Business(bid);
        if self.economy.bank.has(a) {
            self.economy.bank.close(a, Class::Business);
        }
        let Some(b) = self.buildings.get_mut(bid) else { return };
        let fresh = z.is_some();
        b.business = z;
        if fresh {
            self.settle_account(a);
        }
    }

    // ── rent ─────────────────────────────────────────────────────────────

    /// [ledger] `payer` pays `amount` of rent (`Kind::Rent` for a home,
    /// `Kind::ShopRent` for a unit) for `bid` to its owner, and the owner
    /// pays out of it at once: upkeep (out of town), property tax (to the
    /// treasury) and, for residents and their companies, the mortgage. The
    /// rest is the owner's to keep or reinvest. Rent to oneself (an owner
    /// living in or running their own building) moves nothing but still
    /// carries its upkeep and tax.
    pub fn collect_rent(&mut self, payer: Account, bid: BuildingId, amount: f32, kind: Kind) {
        if amount <= 0.0 {
            return;
        }
        let owner = self.landlord_of(bid);
        self.transfer(payer, owner, amount, kind);
        self.transfer(owner, Account::Outside, amount * RENT_UPKEEP, Kind::Upkeep);
        self.pay_city(owner, Flow::PropertyTax, amount * PROPERTY_TAX, Some(crate::economy::Item::Building(bid)));
        let resident = match self.buildings.get(bid).and_then(|b| b.plot.owner).and_then(|o| self.estate.owners.get(o)) {
            Some(o) => matches!(o.kind, OwnerKind::Household { .. } | OwnerKind::Person { .. } | OwnerKind::Company { .. }),
            None => false,
        };
        if resident {
            self.transfer(owner, Account::Outside, amount * RENT_LOAN, Kind::Loan);
        }
        if let Some(o) = self.buildings.get(bid).and_then(|b| b.plot.owner).and_then(|o| self.estate.owners.get_mut(o)) {
            o.rent_today += amount;
        }
    }

    /// [ledger] A business's daily rent for its unit: per job, a share of a
    /// home's rent here. Nothing when it runs its own building (an owner
    /// record that is not a landlord: a firm, a founder, a corp) or the
    /// city runs it. One business per building today: kept per business so
    /// a building of several tenants sums them.
    pub fn shop_rent(&self, bid: BuildingId) -> f32 {
        let Some(b) = self.buildings.get(bid) else { return 0.0 };
        let Some(z) = &b.business else { return 0.0 };
        if z.closed || b.land_use == LandUse::Civic || b.land_use == LandUse::Park || b.service.is_some() || b.city_parking || matches!(z.kind, BusinessKind::Office | BusinessKind::CargoHub) {
            return 0.0;
        }
        let landlord = match b.plot.owner.and_then(|o| self.estate.owners.get(o)) {
            None => true,
            Some(o) => matches!(o.kind, OwnerKind::Developer | OwnerKind::University { .. } | OwnerKind::Company { .. }),
        };
        if !landlord {
            return 0.0;
        }
        b.rent * SHOP_RENT_PER_JOB * b.jobs.max(1) as f32
    }

    /// [ledger] Once a day after the rents: owners out of town (developers,
    /// the independent landlords) send what they hold beyond a float home;
    /// residents' property companies pay their founders a dividend. Owners'
    /// day's rent rolls into yesterday's.
    pub(in crate::world) fn settle_owners(&mut self) {
        let landlords = self.economy.bank.balance(Account::Landlords);
        if landlords > OUT_OF_TOWN_FLOAT {
            self.transfer64(Account::Landlords, Account::Outside, landlords - OUT_OF_TOWN_FLOAT, Kind::Dividend);
        }
        let ids: Vec<OwnerId> = self.estate.owners.keys().collect();
        for oid in ids {
            let o = &self.estate.owners[oid];
            let (kind, money, rent) = (o.kind.clone(), o.money, o.rent_today);
            match kind {
                OwnerKind::Developer if money > OUT_OF_TOWN_FLOAT => {
                    self.transfer64(Account::Owner(oid), Account::Outside, money - OUT_OF_TOWN_FLOAT, Kind::Dividend);
                }
                OwnerKind::Company { founder } => {
                    let net = rent * (1.0 - RENT_UPKEEP - PROPERTY_TAX - RENT_LOAN) * COMPANY_DIVIDEND;
                    let pay = (net as f64).min(money.max(0.0)) as f32;
                    if pay > 0.0 {
                        let to = self.purse_of(founder);
                        self.transfer(Account::Owner(oid), to, pay, Kind::Dividend);
                        if let Some(o) = self.estate.owners.get_mut(oid) {
                            o.income_day = pay;
                            o.earned += pay as f64;
                        }
                    }
                }
                _ => {}
            }
            if let Some(o) = self.estate.owners.get_mut(oid) {
                o.rent_yesterday = std::mem::take(&mut o.rent_today);
            }
        }
    }

    /// [ledger] Who pays for the materials of a site: a developer or a
    /// residents' company out of its own money, anyone else's build is
    /// financed from out of town (as it always was).
    pub fn site_payer(&self, bid: BuildingId) -> Account {
        match self.buildings.get(bid).and_then(|b| b.plot.owner).and_then(|o| self.estate.owners.get(o).map(|x| (o, &x.kind))) {
            Some((o, OwnerKind::Developer | OwnerKind::Company { .. })) => Account::Owner(o),
            _ => Account::Outside,
        }
    }

    // ── checks ───────────────────────────────────────────────────────────

    /// [ledger] Bring every account in step: open the new ones, close the
    /// ones whose holder is gone (their money leaves town), book any mirror
    /// changed outside the door (`Kind::Untracked`). Returns the drifts
    /// found. Daily, and every tick under the debug check.
    pub fn reconcile_ledger(&mut self) -> usize {
        let before = self.economy.bank.untracked_moves;
        for a in self.economy.bank.keyed() {
            if self.mirror(a).is_none() {
                let class = self.account_class(a);
                self.economy.bank.close(a, class);
            }
        }
        for a in self.all_accounts() {
            self.settle_account(a);
        }
        (self.economy.bank.untracked_moves - before) as usize
    }

    /// Every account the city has now.
    pub fn all_accounts(&self) -> Vec<Account> {
        let mut v = vec![Account::Treasury, Account::Airline];
        v.extend(self.households.keys().map(Account::Household));
        v.extend(self.citizens.keys().map(Account::Person));
        v.extend(self.buildings.iter().filter(|(_, b)| b.business.is_some()).map(|(id, _)| Account::Business(id)));
        v.extend(
            self.estate
                .owners
                .iter()
                .filter(|(_, o)| !matches!(o.kind, OwnerKind::Household { .. } | OwnerKind::Person { .. } | OwnerKind::Elite { .. }))
                .filter(|(id, _)| self.mirror(Account::Owner(*id)).is_some())
                .map(|(id, _)| Account::Owner(id)),
        );
        v.extend(self.tourism.tourists.keys().map(Account::Tourist));
        v.extend((0..self.skyline.developers.len()).map(|i| Account::Tower(i as u16)));
        v
    }

    /// [ledger] On load (and for a new city): every balance read in from
    /// its money field. A save from before the ledger migrates here.
    pub(in crate::world) fn open_ledger(&mut self) {
        let bank = &mut self.economy.bank;
        bank.opened = false;
        bank.now = self.clock.tick;
        // The outside world is minus everything in town: the accounts not
        // saved are read in afresh below, each from outside.
        let s = bank.sum_units();
        *bank.outside_units_mut() -= s;
        self.reconcile_ledger();
        self.economy.bank.opened = true;
        self.economy.bank.check |= std::env::var_os("ND_LEDGER_CHECK").is_some();
    }

    /// [ledger] The sum of every balance and the outside world, in ledger
    /// units: zero unless something broke the door itself.
    pub fn ledger_sum(&self) -> i64 {
        self.economy.bank.sum_units()
    }

    /// [ledger] Midnight: reconcile, then close the day's totals.
    pub(in crate::world) fn close_ledger_day(&mut self) {
        self.reconcile_ledger();
        let day = self.clock.day().saturating_sub(1);
        self.economy.bank.roll(day);
    }

    /// [ledger] The debug check, after a tick: drifts reported once each.
    pub(in crate::world) fn check_ledger_tick(&mut self) {
        let n = self.reconcile_ledger();
        if n > 0 {
            let tick = self.clock.tick;
            let drifts: Vec<String> = self.economy.bank.drifts.iter().rev().take(n.min(5)).map(|d| format!("{} {:+.4} after {:?}", d.account, d.dollars, d.last_kind)).collect();
            eprintln!("[ledger] tick {tick}: {n} account(s) drifted outside the door: {}", drifts.join("; "));
        }
        let sum = self.ledger_sum();
        if sum != 0 {
            eprintln!("[ledger] tick {}: balances sum to {sum} units, not zero", self.clock.tick);
        }
    }
}
