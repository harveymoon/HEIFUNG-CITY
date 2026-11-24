//! The market: businesses' daily and hourly bookkeeping, restocking,
//! the seller index, trucks, deliveries, imports and exports, and what
//! citizens buy in shops.

use super::*;

/// [r2-traffic] Site lorries allowed on the road past `MAX_TRUCKS`, and
/// minutes before a site no lorry could reach is tried again.
const SITE_TRUCKS_EXTRA: usize = 40;
/// [K34] Room for this many lorries past the cap for loads that leave a
/// yard or the port in their shipping containers: the boxes go out on the
/// road to the buyer (in a city whose building sites fill the cap, every
/// such load was handed over on the spot).
pub(super) const BOX_TRUCKS_EXTRA: usize = 20;
const SITE_RETRY: u64 = 60;
/// [ledger] The most of a day's profit a business's unit rent takes
/// (`World::shop_rent` is the full rent).
const SHOP_RENT_PROFIT_CAP: f32 = 0.5;
/// [r15-money] Share of a household's everyday living costs spent at the
/// town's own shops, eateries and places (`World::spend_in_town`); the rest
/// (power, phone, insurance, things bought from away) goes out of town.
pub const LIVING_LOCAL: f32 = 0.5;
/// [r15-money] Share of what a household's day's pay leaves over rent and
/// living costs that it spends at the town's shops and places (while it
/// has a few days' living in the purse). With the wage now paid in full by
/// the employer, this is where the shops' takings to pay it come from.
pub const SPARE_SPEND: f32 = 0.15;
/// [r15-money] Passers-by remembered per lot an hour, to pay for walk-in
/// sales (the rest of the footfall shops through them).
pub const PASSERS_KEPT: usize = 8;

/// [r15-money] Someone who walked past a lot this hour: a resident (pays
/// from their household's purse) or a tourist (from their wallet).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Passer {
    Citizen(CitizenId),
    Tourist(crate::tourism::TouristId),
}

/// [r15-money] Who pays someone's daily earnings (`World::earnings_payer`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::world) enum Payer {
    /// The business they work for, out of its own till.
    Business(BuildingId),
    /// An office (on commercial land, or a regional agency on civic ground):
    /// out of the fees its clients (or its budget) away pay it for the work
    /// (`Kind::Services`), booked as they are paid.
    Office(BuildingId),
    /// The treasury: the city's own staff (civic ground, services, plants,
    /// depots, stations, ports, airports, the city's car parks and hubs).
    City(BuildingId),
    /// A building site's crew: the site's owner (a developer, residents'
    /// company or builder), or the bank out of town for everyone else's
    /// build (`World::site_payer`).
    Site(Account),
    /// From out of town: pensions and gig work.
    Outside,
}

impl World {
    /// Businesses pay wages; those that cannot for several days close, and
    /// closed buildings are demolished after a week so the lot can be reused.
    pub(super) fn daily_businesses(&mut self) {
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        let mut demolish = Vec::new();
        let mut cover = [(0.0f32, 0.0f32); BusinessKind::COUNT];
        let mut reoffice: Vec<(BuildingId, u16)> = Vec::new();
        // [tenants] Shut for good with other businesses still renting units
        // here: the building stands, its own units to let.
        let mut let_go: Vec<BuildingId> = Vec::new();
        let any_policy = self.any_district_policy();
        let mut stray: Vec<CitizenId> = Vec::new();
        for bid in ids {
            if !self.buildings[bid].operating() {
                continue;
            }
            let revenue = self.buildings[bid].business.as_ref().map_or(0.0, |z| z.revenue_today);
            let (wage_bill, workers, publicly_funded, kind, tax_line) = {
                let b = &self.buildings[bid];
                let Some(bz) = &b.business else { continue };
                // [parking] Lots the city runs are paid for by the city.
                let publicly_funded = self.publicly_funded(bid);
                // [tax] What its takings are taxed under (the city's own
                // businesses at the standard rate, as before).
                let tax_line = if publicly_funded { None } else { crate::tax::TaxCategory::of_business(bz.kind, b.tier) };
                (0.0f32, b.workers.clone(), publicly_funded, bz.kind, tax_line)
            };
            let rate = tax_line.map_or(crate::tax::BUSINESS_TAX, |c| self.economy.taxes.rate(c));
            let b = &mut self.buildings[bid];
            let bz = b.business.as_mut().unwrap();
            if bz.closed {
                // A shut business takes nothing and owes no tax; whatever
                // the inspector still showed from its last day is cleared.
                bz.close_books(0.0, 0.0);
                bz.revenue_today = 0.0;
                bz.spend_today = 0.0;
                bz.broke_days = bz.broke_days.saturating_add(1);
                // [r15-money] Nobody works for a shut business: anyone still
                // on its staff (hired since it shut) is let go.
                if !b.workers.is_empty() {
                    stray.append(&mut b.workers);
                }
                let bz = b.business.as_mut().unwrap();
                if bz.broke_days > 7 {
                    let tenanted = b.units.iter().any(|u| matches!(u.occupant, crate::buildings::Occupant::Tenant(_)));
                    if b.is_tenant() {
                        // [tenants] Its unit is let again (`daily_tenants`).
                    } else if tenanted {
                        let_go.push(bid);
                    } else if b.plot.tower.is_some() {
                        // [supertall] Nobody pulls down a landmark tower
                        // for a failed tenant: offices take the floors.
                        let jobs = b.jobs;
                        reoffice.push((bid, jobs));
                        self.events.push(SimEvent::BuildingChanged(bid));
                    } else {
                        demolish.push(bid);
                    }
                }
                continue;
            }
            let tax = revenue * rate;
            self.economy.tax_today += tax;
            let lu = b.land_use;
            if let Some(c) = tax_line {
                self.economy.taxes.collect(c, tax);
            }
            // [r15-money] Paid out of the takings (the standard rate was in
            // the prices its customers paid; a rate over it is a cost the
            // business carries, one under it money kept), no longer from
            // out of town.
            self.pay_city(Account::Business(bid), Flow::BusinessTax, tax, Some(Item::LandUse(lu)));
            let extra = if publicly_funded { 0.0 } else { tax };
            // [r15-money] The wages went to the staff's households in their
            // daily pass (`pay_earnings`, `wages_paid`): the whole wage, the
            // income tax held back for the treasury.
            let wage_bill = wage_bill + self.wages_paid.remove(&bid).unwrap_or(0.0);
            if !publicly_funded {
                // [stories] The district's levies on its takings (or relief):
                // a cost it carries like a tax over the standard.
                let extra = extra + if any_policy { self.district_business_levies(bid, revenue) } else { 0.0 };
                // [ledger] Rent for its unit, to the building's owner: the
                // unit's rent out of the day's profit, at most half of it
                // (a lean day pays less, a losing day nothing), so rent
                // never tips a business into the red.
                let spent = self.buildings[bid].business.as_ref().map_or(0.0, |z| z.spend_today) + wage_bill + extra;
                let shop_rent = self.shop_rent(bid).min((revenue - spent).max(0.0) * SHOP_RENT_PROFIT_CAP);
                if shop_rent > 0.0 {
                    // [tenants] To the owner of the building it stands in.
                    let host = self.host_of(bid);
                    self.collect_rent(Account::Business(bid), host, shop_rent, Kind::ShopRent);
                }
                let b = &mut self.buildings[bid];
                let bz = b.business.as_mut().unwrap();
                bz.rent_day = shop_rent;
                bz.spend_today += wage_bill + extra + shop_rent;
                // The market's reading of this kind of business, past the
                // first day's stocking up: what it took against what it
                // paid out, wages and inputs.
                if bz.days_open >= 1 {
                    let e = &mut cover[kind.index()];
                    e.0 += revenue;
                    e.1 += bz.spend_today;
                }
            }
            let b = &mut self.buildings[bid];
            let bz = b.business.as_mut().unwrap();
            bz.days_open = bz.days_open.saturating_add(1);
            let spent = bz.spend_today;
            bz.close_books(revenue, spent);
            bz.revenue_today = 0.0;
            bz.spend_today = 0.0;
            if bz.money < -300.0 && !publicly_funded {
                bz.broke_days = bz.broke_days.saturating_add(1);
            } else {
                bz.broke_days = 0;
            }
            if bz.broke_days >= 5 {
                bz.closed = true;
                bz.broke_days = 0;
                // [tenants] A shut shop does not run down a building others
                // still trade in (nor a tenant its host).
                if !b.is_tenant() && !b.units.iter().any(|u| matches!(u.occupant, crate::buildings::Occupant::Tenant(_))) {
                    b.condition = b.condition.min(0.25);
                }
                b.workers.clear();
                let name = b.name.clone();
                for w in workers {
                    self.lay_off(w, Some(format!("{name} shut down. Out of a job.")));
                    if let Some(c) = self.citizens.get_mut(w) {
                        c.mood = (c.mood - 0.15).max(0.0);
                    }
                }
                self.economy.market.closed_total += 1;
                self.note_business_closed(kind);
                self.events.push(SimEvent::BusinessClosed(bid));
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
        for c in stray {
            if self.citizens.get(c).and_then(|x| x.work).is_some_and(|w| !self.buildings.get(w).is_some_and(|b| b.workers.contains(&c))) {
                self.lay_off(c, None);
            }
        }
        // [ledger] Offices into the failed tenant's tower floors.
        for (bid, jobs) in reoffice {
            self.set_business(bid, Some(Business::new(BusinessKind::Office, jobs)));
        }
        for bid in let_go {
            self.vacate_main(bid);
        }
        for (i, (takings, costs)) in cover.into_iter().enumerate() {
            if let Some(kind) = BusinessKind::ALL.get(i) {
                self.economy.record_cover(*kind, takings, costs);
            }
        }
        for bid in demolish {
            self.demolish(bid);
            // [pt-growth] Shut a week: the lot changes hands.
            self.tag_demolition(bid, crate::buildings::DemolitionReason::Abandoned);
        }
        self.economy.market.deals_today = 0;
        self.economy.market.imports_today = 0.0;
        self.economy.market.exports_today = 0.0;
        self.economy.market.meals_sold_today = 0;
        self.economy.market.grocery_imports_today = 0.0;
        self.economy.market.turned_away_today = 0;
        self.economy.market.leisure.roll();
        self.economy.market.food.roll();
        self.wages_paid.clear();
    }

    /// [parking] The city's own businesses (offices, cargo hubs, civic
    /// ground, the lots the city runs): their staff are paid for by the
    /// city, not out of the takings.
    pub(in crate::world) fn publicly_funded(&self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        let Some(bz) = &b.business else { return false };
        matches!(bz.kind, BusinessKind::Office | BusinessKind::CargoHub) || b.land_use == LandUse::Civic || self.vehicles.city_lots.contains_key(&bid)
    }

    /// [r15-money] Who pays `cid`'s earnings: their employer (a business,
    /// an office through its clients' fees, the city for its own staff, a
    /// site's owner), or the outside world for pensions and gig work.
    pub(in crate::world) fn earnings_payer(&self, cid: CitizenId) -> Payer {
        let Some(bid) = self.citizens.get(cid).and_then(|c| c.work) else { return Payer::Outside };
        let Some(b) = self.buildings.get(bid) else { return Payer::Outside };
        if !b.workers.contains(&cid) {
            return Payer::Outside;
        }
        if let Some(c) = &b.construction {
            // A firm from out of town pays the crew it hired here (its fee
            // was paid out to it); anyone else's crew, the site's owner.
            if c.contract.as_ref().is_some_and(|k| k.local().is_none()) {
                return Payer::Outside;
            }
            // An out-of-town developer keeps only a float here: its crews
            // are paid by its backers away, as its profit goes back to them.
            return match self.site_payer(bid) {
                Account::Owner(o) if self.estate.owners.get(o).is_some_and(|x| matches!(x.kind, crate::estate::OwnerKind::Developer)) => Payer::Outside,
                a => Payer::Site(a),
            };
        }
        match &b.business {
            Some(z) if z.closed => Payer::Outside,
            Some(z) if self.publicly_funded(bid) => {
                // Offices on civic ground too: the region's and the
                // state's agencies, paid from their budgets away.
                if z.kind == BusinessKind::Office {
                    Payer::Office(bid)
                } else {
                    Payer::City(bid)
                }
            }
            Some(_) => Payer::Business(bid),
            None => Payer::City(bid),
        }
    }

    /// [r15-money] `payer` spends `amount` at one of the town's open shops,
    /// eateries and places (picked by `key`, more often the bigger ones):
    /// its takings. Out of town when the town has none. Returns what moved.
    pub(in crate::world) fn spend_in_town(&mut self, payer: Account, amount: f32, key: u64, kind: Kind) -> f32 {
        if amount <= 0.0 {
            return 0.0;
        }
        let tick = self.clock.tick;
        if self.town_shops.0 != tick || self.town_shops.1.is_empty() {
            let mut sum = 0.0;
            let mut v = Vec::new();
            for (id, b) in self.buildings.iter() {
                if !b.operating() || !matches!(b.land_use, LandUse::Commercial | LandUse::Entertainment) || b.workers.is_empty() {
                    continue;
                }
                let Some(z) = &b.business else { continue };
                if z.closed || self.publicly_funded(id) {
                    continue;
                }
                sum += b.workers.len() as f32;
                v.push((sum, id));
            }
            self.town_shops = (tick, v);
        }
        let shops = &self.town_shops.1;
        let Some(&(total, _)) = shops.last() else {
            return self.transfer(payer, Account::Outside, amount, kind);
        };
        let at = crate::hash::unit(crate::hash::mix(key, tick)) * total;
        let i = shops.partition_point(|(c, _)| *c <= at).min(shops.len() - 1);
        let bid = shops[i].1;
        let paid = self.transfer(payer, Account::Business(bid), amount, kind);
        if let Some(z) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()) {
            z.revenue_today += paid;
            z.deals += 1;
        }
        paid
    }

    /// [r15-money] What paying `cid` for a day costs their payer (the wage
    /// plus the standard income tax on it) and the tax held back from it
    /// (at the rate of their home's tier): (bill, tax).
    pub(in crate::world) fn pay_bill(&self, cid: CitizenId) -> (f32, f32) {
        let Some(c) = self.citizens.get(cid) else { return (0.0, 0.0) };
        let wage = c.income.max(0.0);
        let cat = self.income_tax_category(cid);
        let rate = self.economy.taxes.rate(cat).clamp(0.0, 1.0);
        (wage * (1.0 + (rate - self.economy.taxes.surcharge(cat)).clamp(0.0, 1.0)), wage * rate)
    }

    /// [r15-money] Pay `cid`'s day into `purse`. Wages were tuned as what is
    /// kept after the standard income tax (`docs/DESIGN.md`, Taxes), so the
    /// payer's bill is the wage plus the standard rate on it: it holds back
    /// the tax at the rate of their home's tier for the treasury and pays
    /// the rest (a rate over the standard comes off the pay, one under it is
    /// kept). An office is paid its clients' fees for the work first; the
    /// city pays its own staff the bill and takes the tax back (both on the
    /// Economy window's ledger). Returns what reached the purse.
    pub(in crate::world) fn pay_earnings(&mut self, cid: CitizenId, purse: Account) -> f32 {
        let Some(c) = self.citizens.get(cid) else { return 0.0 };
        let wage = c.income.max(0.0);
        if wage <= 0.0 {
            return 0.0;
        }
        let class = c.class();
        let cat = self.income_tax_category(cid);
        let (gross, tax) = self.pay_bill(cid);
        self.economy.tax_today += tax;
        self.economy.taxes.collect(cat, tax);
        let item = Some(Item::Class(class));
        match self.earnings_payer(cid) {
            Payer::City(bid) => {
                let paid = self.city_pays(purse, Flow::StaffWages, gross, Some(Item::Building(bid)));
                self.pay_city(purse, Flow::IncomeTax, tax, item);
                paid - tax
            }
            p => {
                let from = match p {
                    Payer::Business(b) | Payer::Office(b) => Account::Business(b),
                    Payer::Site(a) => a,
                    _ => Account::Outside,
                };
                if matches!(p, Payer::Office(_)) {
                    self.transfer(Account::Outside, from, gross, Kind::Services);
                }
                self.pay_city(from, Flow::IncomeTax, tax, item);
                let paid = self.transfer(from, purse, gross - tax, Kind::Wages);
                if let Payer::Business(b) = p {
                    *self.wages_paid.entry(b).or_default() += paid + tax;
                }
                paid
            }
        }
    }

    /// Every sim hour: hunger, eating at home, production, restocking through
    /// the market or imports, exporting surplus, and market statistics.
    pub(super) fn hourly_economy(&mut self) {
        let (climate, seed) = (self.landscape.params.climate, self.landscape.params.seed);
        self.weather.step(&mut self.rng, climate, self.clock.tick / 60, seed);
        self.allocate_power();
        self.sync_households();
        let exhausted = self.sleep_and_eat();
        self.venue_tabs();
        // Production. Workers dead on their feet do half a shift's work.
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        // [ferries] Commuters off the boat work their shift too.
        let ferry_staff = self.ferry_staff();
        for &bid in &ids {
            // [fishing] A fishing port's fish come in on its boats
            // (`world/fishing.rs`), not by the hour.
            if self.port_fishes_grounds(bid) {
                continue;
            }
            let b = &mut self.buildings[bid];
            if !b.operating() {
                continue;
            }
            let tired = exhausted.get(&bid).copied().unwrap_or(0) as f32;
            let jobs = b.jobs;
            let staff = b.workers.len() as f32 + ferry_staff.get(&bid).copied().unwrap_or(0) as f32;
            let staffing = if jobs == 0 { 0.0 } else { ((staff - tired * 0.5) / jobs as f32).clamp(0.0, 1.0) };
            if let Some(bz) = b.business.as_mut() {
                if !bz.closed {
                    let made = bz.produce(jobs, staffing * b.powered.clamp(0.0, 1.0));
                    if bz.kind == BusinessKind::Grocery {
                        self.economy.market.food.today.made += made;
                    }
                }
            }
        }
        // Walk-in trade: a share of the people who passed a shop this hour
        // step inside and buy something off the shelf, if it is open.
        let hour = self.clock.hour() as u8;
        // [megacorp] Students near a campus drop into pizza places, cafés
        // and bookshops.
        let pull = self.campus_pull();
        let tick = self.clock.tick;
        let passers = std::mem::take(&mut self.passers);
        for &bid in &ids {
            let (lot, jobs) = (self.buildings[bid].lot, self.buildings[bid].jobs);
            let students = if pull.is_empty() || !self.buildings[bid].business.as_ref().is_some_and(|z| z.kind.student_trade()) {
                0.0
            } else {
                self.lots.get(lot).map_or(0.0, |l| Self::student_feet(&pull, l.center))
            };
            let feet = self.lots.get(lot).map_or(0, |l| l.footfall_hour) as f32 + students;
            if feet <= 0.0 {
                continue;
            }
            {
                let b = &self.buildings[bid];
                if !b.operating() || !b.opening_hours().contains(hour) {
                    continue;
                }
                match &b.business {
                    Some(z) if !z.closed && z.kind.walk_in_sale().is_some() => {}
                    _ => continue,
                }
            }
            // [r15-money] Who steps in pays: the passers-by noted on the
            // lot this hour (a resident from their household's purse, a
            // tourist from their wallet) and, by a campus, one of its
            // students; only those with money on them. Nobody to name
            // (the hour after a load), no sale.
            let key = slotmap::Key::data(&bid).as_ffi();
            let mut payers: Vec<Account> = passers
                .get(&lot)
                .into_iter()
                .flatten()
                .filter_map(|p| match *p {
                    Passer::Citizen(c) if self.citizens.contains_key(c) => Some(self.purse_of(c)),
                    Passer::Tourist(t) if self.tourism.tourists.contains_key(t) => Some(Account::Tourist(t)),
                    _ => None,
                })
                .collect();
            if students > 0.0 {
                if let Some(c) = self.lots.get(lot).and_then(|l| self.student_near(l.center, crate::hash::mix(key, tick))) {
                    payers.push(self.purse_of(c));
                }
            }
            payers.retain(|a| self.mirror(*a).is_some_and(|m| m.units() > crate::bank::PER_DOLLAR as i64));
            payers.dedup();
            if payers.is_empty() {
                continue;
            }
            let b = &mut self.buildings[bid];
            let Some(bz) = b.business.as_mut() else { continue };
            let Some(output) = bz.kind.walk_in_sale() else { continue };
            let want = feet * 0.06;
            let got = bz.take(output, want.min(bz.stock_of(output)));
            if got > 0.0 {
                let price = bz.sell_price(output, jobs);
                if students > 0.0 {
                    self.corps.stats.student_trade_today += got * price * students / feet;
                }
                bz.revenue_today += got * price;
                bz.deals += 1;
                self.economy.walk_in_sales_today += got * price;
                if output == Resource::BakedGoods {
                    self.economy.market.meals_sold_today += got as u32;
                }
                if output == Resource::Groceries {
                    self.economy.market.food.today.walk_in += got;
                }
                let each = got * price / payers.len() as f32;
                for p in payers {
                    self.transfer(p, Account::Business(bid), each, Kind::WalkIn);
                }
            }
        }
        for l in self.lots.values_mut() {
            l.footfall_hour = 0;
        }
        // Restocking: find the cheapest delivered supplier or import. The
        // food chain orders ahead of a growing town, and grocers short of
        // groceries for the people they serve buy some in from outside.
        self.refresh_seller_index();
        let food_factor = self.economy.food_order_factor();
        let grocery_share = self.grocery_share_per_day();
        let grocery_short = grocery_share > 0.0 && self.town_grocery_stock() < self.citizens.len() as f32 * crate::citizens::GROCERIES_PER_PERSON_DAY * 0.25;
        // [r12-hunger] ... or where people round it are hungry: the town's
        // stock can stand in a few shops across town while the rest are bare.
        let hunger_near = |w: &World, at: Vec2| grocery_share > 0.0 && w.food_need.pull(at) >= super::food_need::FOOD_PULL;
        for &bid in &ids {
            let (kind, needs, at, closed) = {
                let b = &self.buildings[bid];
                if !b.operating() {
                    continue;
                }
                let Some(bz) = &b.business else { continue };
                let Some(lot) = self.lots.get(b.lot) else { continue };
                // (resource, what is short of the usual target, what more
                // the food chain wants ahead of growth). The extra is only
                // bought locally: ordering ahead never imports.
                let factor = if bz.kind.is_food() { food_factor } else { 1.0 };
                let mut needs: Vec<(Resource, f32, f32)> = bz
                    .kind
                    .recipe()
                    .inputs
                    .iter()
                    .filter_map(|(r, _)| {
                        let target = bz.target_input(*r, b.jobs);
                        let have = bz.stock_of(*r) + bz.inbound_of(*r);
                        if have < target * factor * 0.5 {
                            let short = (target - have).max(0.0);
                            Some((*r, short, target * factor - have - short))
                        } else {
                            None
                        }
                    })
                    .collect();
                // Shelves nearly bare while the whole town is short: buy
                // groceries in from outside to cover half a day.
                if bz.kind == BusinessKind::Grocery && (grocery_short || hunger_near(self, lot.center)) {
                    let have = bz.stock_of(Resource::Groceries) + bz.inbound_of(Resource::Groceries);
                    if have < grocery_share * 0.25 {
                        needs.push((Resource::Groceries, grocery_share * 0.5 - have, 0.0));
                    }
                }
                (bz.kind, needs, lot.center, bz.closed)
            };
            if closed {
                continue;
            }
            for (r, short, ahead) in needs {
                // Grocers never buy groceries from each other: short shelves
                // in town are filled from outside.
                let local = !(r == Resource::Groceries && kind == BusinessKind::Grocery);
                let full = (short + ahead).min(400.0);
                let seller = if !local {
                    None
                } else {
                    match self.best_seller(r, at, full, bid) {
                        Some(s) => Some((s, full)),
                        None if short > 0.0 => self.best_seller(r, at, short.min(400.0), bid).map(|s| (s, short.min(400.0))),
                        None => None,
                    }
                };
                let (seller, amount) = match seller {
                    Some((s, amount)) => (Some(s), amount),
                    None => (None, short.min(400.0)),
                };
                if amount <= 0.0 {
                    continue;
                }
                match seller {
                    Some((seller_id, price)) => {
                        let got = self.buildings[seller_id].business.as_mut().unwrap().take(r, amount);
                        if got <= 0.0 {
                            continue;
                        }
                        let cost = got * price;
                        {
                            let sb = self.buildings[seller_id].business.as_mut().unwrap();
                            sb.revenue_today += cost;
                            sb.deals += 1;
                        }
                        {
                            let bb = self.buildings[bid].business.as_mut().unwrap();
                            bb.spend_today += cost;
                        }
                        self.transfer(Account::Business(bid), Account::Business(seller_id), cost, Kind::Supplies);
                        self.economy.market.deals_today += 1;
                        self.ship(seller_id, bid, r, got, cost);
                    }
                    None => {
                        // Groceries bought in for a hungry town come at wholesale, not
                        // at the import markup: a grocer covering a shortage
                        // should not be ruined by it.
                        let markup = if r == Resource::Groceries { 1.0 } else { IMPORT_MARKUP };
                        let cost = amount * r.base_price() * markup;
                        // [tax] The import duty on top, to the treasury.
                        let duty = cost * self.economy.taxes.import_duty;
                        {
                            let bb = self.buildings[bid].business.as_mut().unwrap();
                            bb.spend_today += cost + duty;
                        }
                        self.transfer(Account::Business(bid), Account::Outside, cost, Kind::Imports);
                        if duty > 0.0 {
                            self.pay_city(Account::Business(bid), Flow::ImportDuty, duty, None);
                            self.economy.taxes.duty_today += duty;
                        }
                        self.economy.taxes.note_import(r, amount);
                        self.economy.market.imports_today += cost;
                        if r == Resource::Groceries {
                            self.economy.market.grocery_imports_today += amount;
                            self.economy.market.food.today.imported += amount;
                        }
                        self.import(bid, r, amount, cost);
                    }
                }
            }
        }
        // Exports: surplus outputs leave town at a discount, less of one
        // when a freight yard ships them by rail.
        let yards: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::FreightYard) && !b.is_derelict() && b.operating()).map(|(id, _)| id).collect();
        let export_bonus = (if yards.is_empty() { 1.0 } else { rail::RAIL_EXPORT_BONUS }).max(self.sea_export_bonus()) * self.hub_export_bonus();
        let mut shipped = 0.0f32;
        let mut sold: Vec<(BuildingId, f32)> = Vec::new();
        let mut boxed: Vec<(BuildingId, Resource, f32, f32)> = Vec::new();
        // [airport] Dear exports flown out earn more, and go by lorry to the cargo terminal.
        let air_bonus: Vec<f32> = Resource::ALL.iter().map(|r| self.air_export_bonus(*r)).collect();
        let mut by_air: Vec<(BuildingId, Resource, f32)> = Vec::new();
        for &bid in &ids {
            let b = &mut self.buildings[bid];
            if !b.operating() {
                continue;
            }
            let jobs = b.jobs;
            let Some(bz) = b.business.as_mut() else { continue };
            if bz.closed {
                continue;
            }
            for (r, per) in bz.kind.recipe().outputs {
                if r.is_retail() {
                    continue;
                }
                let keep = per * jobs as f32 * crate::market::JOB_RATE * r.export_keep_hours();
                // [K34] A scrap yard keeps a stock of steel for the sites.
                let keep = if bz.kind == BusinessKind::ScrapYard { keep.max(40.0) } else { keep };
                let have = bz.stock_of(*r);
                if have > keep * 1.2 {
                    let surplus = have - keep;
                    bz.take(*r, surplus);
                    let air = air_bonus[Resource::ALL.iter().position(|x| x == r).unwrap_or(0)];
                    let income = surplus * r.base_price() * EXPORT_DISCOUNT * export_bonus * air;
                    if air > 1.0 {
                        by_air.push((bid, *r, surplus));
                    } else {
                        // [r8] (flown goods go by air, not in a box)
                        boxed.push((bid, *r, surplus, income));
                    }
                    shipped += surplus;
                    sold.push((bid, income));
                    bz.revenue_today += income;
                    self.economy.market.exports_today += income;
                }
            }
        }
        for (bid, income) in sold {
            self.transfer(Account::Outside, Account::Business(bid), income, Kind::Exports);
        }
        // [r8] Through a freight forwarder where there is one: boxed up firm
        // by firm and shipped from the port or the freight yard (its fee to
        // pay), not counted onto the quay as a number.
        self.containers.stats.export_units_today += shipped;
        let shipped = shipped - self.forwarders_take_exports(&boxed);
        // [freight] Onto the apron of a yard the freight trains call at.
        if let Some(&y) = self.yards.first().or(yards.first()) {
            self.buildings[y].rail_outbound += shipped;
        }
        // Exports through a cargo hub leave in full semis.
        self.hub_note_exports(shipped);
        self.air_exports(by_air);
        // [berthing] And by sea: onto the quay for the next freighter's
        // crane (what it loads; a show of the trade, the money is paid).
        if shipped > 0.0 && !self.ports.working(crate::ports::PortKind::ShippingPort).is_empty() {
            self.ports.outbound = (self.ports.outbound + shipped).min(crate::ports::CONTAINER_UNITS * crate::ports::MAX_BOXES as f32 * 4.0);
        }
        self.drain_stranded_rail_imports();
        // Statistics.
        let mut sums: std::collections::BTreeMap<Resource, (f32, u32, f32)> = std::collections::BTreeMap::new();
        let mut businesses = 0;
        for b in self.buildings.values() {
            let Some(bz) = &b.business else { continue };
            if bz.closed || !b.operating() {
                continue;
            }
            businesses += 1;
            for r in Resource::ALL {
                let e = sums.entry(r).or_insert((0.0, 0, 0.0));
                e.2 += bz.stock_of(r);
                if bz.sells(r) {
                    e.0 += bz.sell_price(r, b.jobs);
                    e.1 += 1;
                }
            }
        }
        let m = &mut self.economy.market;
        m.businesses = businesses;
        for (r, (sum, n, stock)) in sums {
            m.avg_price.insert(r, if n > 0 { sum / n as f32 } else { r.base_price() });
            m.stock.insert(r, stock);
        }
        m.hungry = self.citizens.values().filter(|c| c.satiety < 0.3).count() as u32;
    }

    /// Every hour: people sleep at home or tire; satiety drains (slower
    /// asleep); people at home eat from the household pantry (a household
    /// with no home, wherever it is), those out eat the meal they packed. Returns how many exhausted people are at work
    /// in each building, for production; working exhausted costs mood.
    pub(super) fn sleep_and_eat(&mut self) -> HashMap<BuildingId, u16> {
        let tick = self.clock.tick;
        let hour = self.clock.hour();
        let mut exhausted: HashMap<BuildingId, u16> = HashMap::new();
        let buildings = &self.buildings;
        for c in self.citizens.values_mut() {
            let at_home = c.home.is_some() && c.inside == c.home && matches!(c.activity, Activity::AtHome);
            // Each keeps their own bedtime, so the city goes dark home by home.
            let night = crate::citizens::night_for(c.seed, hour);
            // [services] Night crews at round-the-clock emergency posts
            // (police, fire, ambulance, 911, jail) keep a night worker's
            // hours: up for a shift that is on or about to start, a sleep
            // in the day before it. Without this nightfall sent them to bed
            // like everyone else and the 911 desk stood empty until morning.
            let shift = c.work.and_then(|w| buildings.get(w)).filter(|b| b.service.is_some_and(|s| s.kind.hours().always())).map(|b| b.shift_for(c.seed));
            let duty_in = |k: u32| shift.is_some_and(|sh| sh.contains(((hour as u32 + k) % 24) as u8));
            let night_duty = night && (duty_in(0) || duty_in(1));
            let night_worker = shift.is_some_and(|sh| sh.contains(2));
            let day_sleep = night_worker && !night && !duty_in(0) && (1..=10).any(duty_in) && c.wakefulness < 0.75;
            if !at_home {
                c.asleep = false;
            } else if !c.asleep && (night || c.wakefulness < crate::citizens::TIRED || day_sleep) && !(night_duty && c.wakefulness >= crate::citizens::TIRED) {
                c.asleep = true;
            }
            if c.asleep {
                c.wakefulness = (c.wakefulness + 0.12).min(1.0);
                // A night crew due on shift gets up rested enough, not fully
                // slept (at 0.94 one more hour in bed made the desk an hour late).
                if (c.wakefulness >= 0.95 && !night) || (night_duty && c.wakefulness >= 0.6) {
                    c.asleep = false;
                    c.next_decision_tick = c.next_decision_tick.min(tick + 1);
                }
            } else {
                let working = matches!(c.activity, Activity::Working);
                c.wakefulness = (c.wakefulness - if working { 0.055 } else { 0.045 }).max(0.0);
                if working && c.wakefulness < crate::citizens::EXHAUSTED {
                    if let Some(w) = c.work {
                        *exhausted.entry(w).or_default() += 1;
                    }
                    c.mood = (c.mood - 0.02).max(0.0);
                }
            }
            c.satiety = (c.satiety - if c.asleep { 0.02 } else { 0.04 }).max(0.0);
            let pantry = c.household.and_then(|h| self.households.get_mut(h));
            // [r4-hunger] A household with no home carries its groceries
            // and eats them where it is. Only a home's kitchen fed anyone,
            // so on Harvey's day-62 city the 1,100 people on the street
            // with no camp bed (the mission van feeds those with one) went
            // hungry with food in the bag and money in the purse, and left
            // over hunger by the hundred a day.
            let carried = c.home.is_none() && pantry.is_some();
            if (at_home || carried) && c.satiety < 0.65 {
                if let Some(hh) = pantry {
                    if hh.groceries >= 1.0 {
                        hh.groceries -= 1.0;
                        c.satiety = (c.satiety + 0.45).min(1.0);
                    }
                    c.groceries = hh.groceries;
                } else if c.groceries >= 1.0 {
                    c.groceries -= 1.0;
                    c.satiety = (c.satiety + 0.45).min(1.0);
                }
            } else if !at_home && c.satiety < 0.45 && c.packed_meal >= 1.0 {
                c.packed_meal -= 1.0;
                c.satiety = (c.satiety + 0.45).min(1.0);
            }
        }
        exhausted
    }

    /// Every hour out at a venue is a round of drinks, paid from the
    /// pocket while there is pocket money for it.
    pub(super) fn venue_tabs(&mut self) {
        let mut tabs: Vec<(CitizenId, BuildingId, f32)> = Vec::new();
        for (cid, c) in self.citizens.iter_mut() {
            let Activity::Visiting(b) = c.activity else { continue };
            let Some(bld) = self.buildings.get_mut(b) else { continue };
            // [places] Drinks cost more somewhere polished; a regular
            // stands an extra round now and then.
            let mut tab = VENUE_TAB * super::places::tab_factor(bld.tier);
            let regular = c.places.get(b).is_some_and(|p| p.regular);
            if regular {
                tab *= 1.0 + crate::places::REGULAR_TIP;
            }
            if c.savings < tab {
                continue;
            }
            let Some(bz) = bld.business.as_mut() else { continue };
            // [economy] Every place to go out charges, each at its own rate.
            if !bz.kind.is_outing() || bz.closed {
                continue;
            }
            let mut tab = tab * bz.kind.tab_factor();
            // [economy] A seafood restaurant cooks the catch: a diner's hour
            // takes fillets, and with none the menu is short.
            if bz.kind == BusinessKind::SeafoodRestaurant && bz.take(Resource::Fillets, 0.3) < 0.3 {
                tab *= 0.5;
            }
            // [r4-leisure-places] An hour at a bar, a fine-dining table or a
            // cinema takes its stock (drinks and snacks from the grocers, a
            // fine kitchen's produce); short of it the evening is short too.
            // The kitchens' meals are eaten on arrival (`World::arrive`).
            if bz.kind.is_eat_or_go_out() && !bz.kind.cooks_meals() {
                let mut short = false;
                let seats = bz.kind.customers_per_job().unwrap_or(4).max(1) as f32;
                for (r, per) in bz.kind.recipe().inputs {
                    let want = per * crate::market::JOB_RATE * VISIT_STOCK / seats;
                    if bz.take(*r, want) < want * 0.5 {
                        short = true;
                    }
                }
                if short {
                    tab *= 0.6;
                }
            }
            if c.savings < tab {
                continue;
            }
            tabs.push((cid, b, tab));
            bz.revenue_today += tab;
            c.mood = (c.mood + 0.01).min(1.0);
            if regular {
                self.places.reputation.entry(b).or_default().regular_revenue_today += tab;
            }
        }
        // [ledger] The rounds, from the pocket to the venue.
        for (cid, b, tab) in tabs {
            self.transfer(Account::Person(cid), Account::Business(b), tab, Kind::Outings);
            // [r4-leisure-places] What is on was booked from out of town: the
            // film's distributor, the touring company, the band take a share.
            let share = self.buildings.get(b).and_then(|x| x.business.as_ref()).map_or(0.0, |z| z.kind.booking_share());
            if share > 0.0 {
                let fee = tab * share;
                if let Some(bz) = self.buildings.get_mut(b).and_then(|x| x.business.as_mut()) {
                    bz.spend_today += fee;
                }
                self.economy.market.leisure.bookings_today += fee;
                self.transfer(Account::Business(b), Account::Outside, fee, Kind::Imports);
            }
        }
    }

    /// Groceries on the shelves of every open grocer in town.
    pub(super) fn town_grocery_stock(&self) -> f32 {
        self.buildings.values().filter(|b| b.operating()).filter_map(|b| b.business.as_ref()).filter(|z| z.kind == BusinessKind::Grocery && !z.closed).map(|z| z.stock_of(Resource::Groceries)).sum()
    }

    /// Groceries a day each open grocer should have for its share of the
    /// town (everyone eats `GROCERIES_PER_PERSON_DAY`), grown by the food
    /// order factor; zero when no grocer is open.
    pub(super) fn grocery_share_per_day(&self) -> f32 {
        let grocers = self.buildings.values().filter(|b| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Grocery && !z.closed)).count();
        if grocers == 0 {
            return 0.0;
        }
        // Bakeries feed some of it; grocers cover two thirds.
        self.citizens.len() as f32 * crate::citizens::GROCERIES_PER_PERSON_DAY * 0.66 * self.economy.food_order_factor() / grocers as f32
    }

    /// [freight] Rail orders no freight train will bring come by road from
    /// the edge of the map instead: their yard lost its track or its street
    /// (or is gone), no freight has run for two headways (the dispatcher is
    /// stuck, say at `MAX_TRAINS`), or the order has waited far too long.
    pub(super) fn drain_stranded_rail_imports(&mut self) {
        let tick = self.clock.tick;
        let stuck = tick.saturating_sub(self.rail.last_freight_tick) >= rail::FREIGHT_HEADWAY * 2;
        let works = |w: &World, y: BuildingId| w.buildings.get(y).is_some_and(|b| b.rail.as_ref().is_some_and(|l| l.works()) && !b.is_derelict());
        let orders = std::mem::take(&mut self.rail.orders);
        let (stranded, keep): (Vec<rail::RailOrder>, Vec<rail::RailOrder>) = orders.into_iter().partition(|o| stuck || !works(self, o.cargo.from) || tick.saturating_sub(o.since) >= rail::ORDER_GIVE_UP);
        self.rail.orders = keep;
        for o in stranded {
            self.queue_truck(TruckOrigin::CityEdge, Cargo { from: BuildingId::default(), ..o.cargo }, 6);
        }
    }

    /// Who sells what this hour, with their lot centres. Built once an hour
    /// so each buyer's search is a walk over the sellers of one resource,
    /// not over every building; prices and stock are still read live.
    pub(super) fn refresh_seller_index(&mut self) {
        let hour = self.clock.tick / 60;
        if self.seller_index_hour == Some(hour) {
            return;
        }
        self.seller_index_hour = Some(hour);
        self.seller_index.clear();
        for (id, b) in self.buildings.iter() {
            if !b.operating() {
                continue;
            }
            let Some(bz) = &b.business else { continue };
            if bz.closed {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            for (r, _) in bz.kind.recipe().outputs {
                self.seller_index.entry(*r).or_default().push((id, lot.center));
            }
            // [r14-builders] The builders' merchant sells its trade stock
            // over the counter too: to sites and to anyone restocking.
            if bz.kind == BusinessKind::BuildersMerchant {
                for r in crate::catalogue::site_materials() {
                    let v = self.seller_index.entry(r).or_default();
                    if !v.iter().any(|(b, _)| *b == id) {
                        v.push((id, lot.center));
                    }
                }
            }
        }
    }

    /// Best local seller of `r` for a buyer at `at` (other than `buyer`):
    /// cheapest delivered price among those holding at least half the lot.
    pub(super) fn best_seller(&mut self, r: Resource, at: Vec2, amount: f32, buyer: BuildingId) -> Option<(BuildingId, f32)> {
        self.refresh_seller_index();
        let mut best: Option<(BuildingId, f32)> = None;
        for &(id, pos) in self.seller_index.get(&r).map(|v| v.as_slice()).unwrap_or(&[]) {
            if id == buyer {
                continue;
            }
            let Some(b) = self.buildings.get(id) else { continue };
            let Some(bz) = &b.business else { continue };
            if bz.closed || !b.operating() || bz.stock_of(r) < amount * 0.5 {
                continue;
            }
            let price = bz.sell_price(r, b.jobs) + HAULAGE * pos.distance(at) * r.base_price();
            if best.map_or(true, |(_, p)| price < p) {
                best = Some((id, price));
            }
        }
        best
    }

    /// Send a truck from one business to another; deliver instantly when no
    /// road route exists.
    pub(super) fn ship(&mut self, from: BuildingId, to: BuildingId, resource: Resource, amount: f32, price: f32) {
        let cargo = Cargo { from, to, resource, amount, price };
        self.note_inbound(to, resource, amount);
        // [industry] a lorry parked at a truck lot near the seller takes the job
        let origin = self.fleet_origin(from);
        // [delivery-hours] In working hours, at the buyer's own minute.
        self.queue_truck(TruckOrigin::Building(origin), cargo, 7);
    }

    pub(super) fn truck_count(&self) -> usize {
        // [r11-box-stock] Not the lorries hired to clear a depot's backlog.
        // [r15-freight] Nor those standing in a depot's bays, off the street.
        let haulers = &self.containers.haulers;
        self.traffic.cars.iter().filter(|(id, c)| c.cargo.is_some() && !c.bay && !haulers.contains(id)).count()
    }

    /// [r15-freight] How many lorries the roads take before loads are
    /// handed over on the spot (`dispatch_truck`): `MAX_TRUCKS`, and one
    /// per `TRUCK_PEOPLE` residents in a bigger town. A flat 60 in a city
    /// of 17,000 was full 97% of the day (95 lorries counted, 57 of them
    /// driving back empty, 21 standing in depot bays), so nearly every load
    /// that was not for a site or in boxes was handed over by magic, and
    /// site loads past the site allowance too (the site lorry counts that
    /// fell to near zero, todo r14-materials).
    pub fn truck_cap(&self) -> usize {
        crate::market::MAX_TRUCKS.max(self.citizens.len() / crate::market::TRUCK_PEOPLE)
    }

    /// The buyer counts a load as on its way so it does not reorder.
    pub(super) fn note_inbound(&mut self, to: BuildingId, resource: Resource, amount: f32) {
        if let Some(bz) = self.buildings.get_mut(to).and_then(|b| b.business.as_mut()) {
            *bz.inbound.entry(resource).or_insert(0.0) += amount;
        }
    }

    /// Import from outside: by rail through the nearest working yard when
    /// there is one, else a truck from the city edge.
    pub(super) fn import(&mut self, to: BuildingId, resource: Resource, amount: f32, price: f32) {
        // Out of town: no seller in the city, so a null `from` (it used to
        // be the buyer itself, which read as a truck driving from a shop to
        // the same shop).
        let cargo = Cargo { from: BuildingId::default(), to, resource, amount, price };
        self.note_inbound(to, resource, amount);
        // With a freight yard in town the goods come by rail, cheaper and
        // later: the order waits at the yard for the next train. Building
        // sites are the exception: a quarter of the work waiting hours for
        // each load would quadruple build times, so their parts come by
        // the developer's own trucks.
        let site = self.buildings.get(to).is_some_and(|b| b.construction.is_some());
        // A cargo hub within reach takes it in (by sea, rail or a shared
        // semi) and sends it on in its own lorry (`world/cargo_hubs.rs`).
        // [airport] Dear goods fly in, to the cargo terminal.
        if !site && self.import_by_air(cargo) {
            return;
        }
        if !site {
            if let Some(hub) = self.hub_for(to) {
                self.import_via_hub(hub, cargo);
                return;
            }
        }
        // [waterfront] Its own quay on the water first.
        if !site && self.import_by_jetty(to, cargo) {
            return;
        }
        // By sea is cheaper still than by rail.
        if !site && self.import_by_sea(to, cargo) {
            return;
        }
        if !site {
            if let Some(yard) = self.nearest_yard(to) {
                let refund = price * (1.0 - rail::RAIL_IMPORT_FACTOR);
                self.refund_import(to, refund);
                // [freight] Out of town until a freight train brings it in.
                self.rail.orders.push(rail::RailOrder { cargo: Cargo { from: yard, to, resource, amount, price: price * rail::RAIL_IMPORT_FACTOR }, since: self.clock.tick });
                return;
            }
        }
        self.queue_truck(TruckOrigin::CityEdge, cargo, 6);
    }

    /// Where a truck can pull up at a building: its street, or for a lot
    /// that lines a footpath, the nearest street it was given.
    pub(super) fn truck_stop(&self, b: BuildingId) -> Option<(SegmentId, Vec2)> {
        let lot = self.building_lot(b)?;
        Some(match lot.car_segment {
            Some(s) => (s, lot.car_point),
            None => (lot.front_segment, lot.entrance()),
        })
    }

    /// Put a truck on the road with `cargo`, from a building or the city
    /// edge. Delivered on the spot when there is no route or the road is
    /// full of trucks already: the buyer counts the load as inbound.
    pub(super) fn dispatch_truck(&mut self, origin: TruckOrigin, cargo: Cargo, color: u8) {
        // [r8] A load in boxes at a depot: a container lorry comes for it,
        // waits in a bay and has its box lifted onto its chassis.
        if let TruckOrigin::Building(b) = origin {
            if self.box_pickup(b, cargo) {
                return;
            }
        }
        let to_site = self.buildings.get(cargo.to).is_some_and(|b| b.construction.is_some());
        // [r2-traffic] A building site's load goes by truck (the site
        // starts on what a truck brought, K6), with room for
        // `SITE_TRUCKS_EXTRA` site lorries past the cap; only past that, in
        // a boom, is it delivered on the spot as other loads are. (Queued
        // behind the cap instead, a growing city's sites starved: a
        // generated city opened 17 businesses by day 4 instead of 20+.)
        let boxed = !to_site && self.has_full_boxes(cargo.from, cargo.resource, cargo.to);
        let cap = self.truck_cap() + if to_site { SITE_TRUCKS_EXTRA } else if boxed { BOX_TRUCKS_EXTRA } else { 0 };
        if self.truck_count() >= cap {
            // [K34] Its boxes go with it.
            self.boxes_follow(&cargo);
            self.deliver(cargo);
            return;
        }
        let mut route = (|| {
            let (from_seg, from_pt) = match origin {
                TruckOrigin::Building(b) => self.truck_stop(b)?,
                TruckOrigin::CityEdge => {
                    let entry = self.entry_node_const()?;
                    let node = self.roads.node(entry);
                    let seg = *node.segments.iter().find(|&&s| self.roads.segment(s).alive)?;
                    (seg, node.pos)
                }
            };
            let (to_seg, to_pt) = self.truck_stop(cargo.to)?;
            // [r15-lots] (F10) A site's lorry goes into a pocket of streets
            // with no way out when the site is in it.
            if to_site {
                self.traffic.net.find_route_into_pocket(from_seg, from_pt, to_seg, to_pt)
            } else {
                self.traffic.net.find_route(from_seg, from_pt, to_seg, to_pt)
            }
        })();
        // [ferry-islands] No road to an island: the load waits at the
        // terminal for the next boat across (and comes off on the island).
        if route.is_none() {
            let from = match origin {
                TruckOrigin::Building(b) => Some(b),
                TruckOrigin::CityEdge => None,
            };
            if self.queue_ferry_load(from, cargo) {
                return;
            }
        }
        // [r2-traffic] A site's load comes in from out of town by any road
        // that leads there (the one entry tried first may be on another
        // network: an island's own road, a stub); and with none at all,
        // nothing is dropped on the site by magic: it waits and says so.
        if route.is_none() && to_site {
            route = self.site_route_from_edge(cargo.to);
            if route.is_none() {
                self.site_unsupplied(cargo, origin, color);
                return;
            }
        }
        match route {
            Some((route, s0, s1)) => {
                // The lorry by the goods and the seller's trade, in its livery.
                let seller = self.buildings.get(cargo.from).and_then(|b| b.business.as_ref()).map(|bz| bz.kind);
                let long_haul = matches!(origin, TruckOrigin::CityEdge) || seller.is_none();
                let id = self.traffic.spawn_car(CitizenId::default(), Some(cargo), route, s0, s1, color);
                let mut rng = crate::vehicle::model_rng(self.clock.tick, slotmap::Key::data(&id).as_ffi());
                self.traffic.cars[id].model = crate::vehicle::VehicleModel::for_cargo(cargo.resource, seller, to_site, long_haul, &mut rng);
                // [megacorp] A corp's lorries wear its brand (not on a
                // building-site run or a haul in from out of town).
                if !to_site && !long_haul {
                    // [companies] A local company's lorry in its own colours.
                    let base = self.traffic.cars[id].model.livery;
                    if let Some(l) = self.fleet_livery(cargo.from, base) {
                        self.traffic.cars[id].model.livery = l;
                    } else if let Some(l) = self.corp_livery(cargo.to).filter(|_| self.buildings.get(cargo.to).and_then(|b| b.business.as_ref()).is_some_and(|z| z.kind.customers_per_job().is_some())) {
                        // [sweep-sim] A corp's shop is stocked by the corp's
                        // own vans, so a corp of shops alone has a fleet too.
                        let m = &mut self.traffic.cars[id].model;
                        m.livery = l;
                        m.body = crate::vehicle::BodyType::DeliveryVan;
                    }
                }
                // [companies] A builder's site lorries wear its colours.
                // [r14-builders] A contractor's or merchant's own lorry
                // (to a site, or a builder's run to the merchant) wears
                // that firm's.
                let own = match origin {
                    TruckOrigin::Building(o) if self.buildings.get(o).and_then(|b| b.business.as_ref()).is_some_and(|z| matches!(z.kind, BusinessKind::Contractor | BusinessKind::BuildersMerchant)) => Some(o),
                    _ => None,
                };
                if let Some(o) = own {
                    let base = crate::vehicle::Livery::Construction;
                    let l = self.fleet_livery(o, base).unwrap_or(base);
                    let m = &mut self.traffic.cars[id].model;
                    m.livery = l;
                    // A builder's own runs go in its own lorry.
                    if self.buildings.get(o).and_then(|b| b.business.as_ref()).is_some_and(|z| z.kind == BusinessKind::Contractor) {
                        m.body = crate::vehicle::BodyType::PanelTruck;
                    }
                } else if to_site {
                    if let Some(c) = self.builder_colour(cargo.to) {
                        self.traffic.cars[id].model.livery = crate::vehicle::Livery::fleet(crate::vehicle::Livery::Construction, c);
                    }
                }
                // [K34] The boxes the load is in ride with it.
                self.boxes_on_dispatch(id, matches!(origin, TruckOrigin::CityEdge), &cargo, to_site);
            }
            None => {
                self.boxes_follow(&cargo);
                self.deliver(cargo)
            }
        }
    }

    /// [r2-traffic] A way for a lorry from out of town to building site
    /// `to`: from the farthest of the city's ways in on the site's own
    /// street network.
    fn site_route_from_edge(&self, to: BuildingId) -> Option<(Vec<crate::traffic::LaneId>, f32, f32)> {
        let (to_seg, to_pt) = self.truck_stop(to)?;
        let s = self.roads.try_segment(to_seg).filter(|s| s.alive && s.is_street())?;
        let net = self.drive_component(s.a)?;
        let mut leaves: Vec<NodeId> = self.city_leaves().into_iter().filter(|&n| self.drive_component(n) == Some(net)).collect();
        leaves.sort_by(|a, b| self.roads.node(*b).pos.length().total_cmp(&self.roads.node(*a).pos.length()).then(a.0.cmp(&b.0)));
        leaves.into_iter().take(4).find_map(|n| {
            let node = self.roads.node(n);
            let seg = *node.segments.iter().find(|&&s| self.roads.segment(s).alive && self.roads.segment(s).is_street())?;
            self.traffic.net.find_route_into_pocket(seg, node.pos, to_seg, to_pt)
        })
    }

    /// [r15-lots] (F10) For probes: how a lorry would reach building `to`:
    /// its stop, the street there, and a route (or none) from each of the
    /// city's ways in on its network.
    pub fn site_reach_debug(&self, to: BuildingId) -> Vec<String> {
        let mut out = Vec::new();
        let Some((seg, pt)) = self.truck_stop(to) else {
            out.push("no truck stop".into());
            return out;
        };
        let s = self.roads.segment(seg);
        out.push(format!("stop seg {} at ({:.1},{:.1}) kind {:?} lanes {}/{} alive {} street {} levels {}/{} bridge {} viaduct {} tunnel {} comp {:?}/{:?}", seg.0, pt.x, pt.y, s.kind, s.lanes_fwd, s.lanes_bwd, s.alive, s.is_street(), self.roads.node(s.a).level, self.roads.node(s.b).level, s.bridge, s.viaduct, s.tunnel, self.drive_component(s.a), self.drive_component(s.b)));
        out.push(format!("nearest lane on stop: {:?}", self.traffic.net.nearest_lane_on(seg, pt)));
        let net = &self.traffic.net;
        // [r4-flags-sim] A pocket no route enters: the trapped lanes the
        // stop's lane can drive to, and where they meet the rest.
        if let Some((goal, _, _)) = net.nearest_lane_on(seg, pt) {
            let trapped = |l: u32| net.trap.get(l as usize).copied().unwrap_or(false);
            out.push(format!("stop lane {goal} trapped {}", trapped(goal)));
            let mut pocket = std::collections::BTreeSet::new();
            let mut stack = vec![goal];
            while let Some(l) = stack.pop() {
                if !pocket.insert(l) {
                    continue;
                }
                let lane = &net.lanes[l as usize];
                stack.extend(lane.next.iter().copied());
                stack.extend(lane.left);
                stack.extend(lane.right);
            }
            let segs: std::collections::BTreeSet<u32> = pocket.iter().filter_map(|&l| match net.lanes[l as usize].kind { crate::traffic::LaneKind::Road { segment, .. } => Some(segment.0), _ => None }).collect();
            out.push(format!("forward from the stop: {} lanes ({} trapped) on segments {:?}", pocket.len(), pocket.iter().filter(|&&l| trapped(l)).count(), segs.iter().take(40).collect::<Vec<_>>()));
            // Lanes into the pocket from outside, and the nodes at its rim.
            for &l in &pocket {
                for &p in &net.lanes[l as usize].prev {
                    if !pocket.contains(&p) {
                        let k = &net.lanes[p as usize].kind;
                        out.push(format!("   into the pocket: lane {p} {k:?} -> {l} {:?} end_node {}", net.lanes[l as usize].kind, net.lanes[p as usize].end_node.0));
                    }
                }
            }
            for &sid in &segs {
                let sg = self.roads.segment(SegmentId(sid));
                out.push(format!("   seg {sid} {:?} lanes {}/{} a {} ({:.0},{:.0}) b {} ({:.0},{:.0}) ramp {} oneway {}", sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.a.0, self.roads.node(sg.a).pos.x, self.roads.node(sg.a).pos.y, sg.b.0, self.roads.node(sg.b).pos.x, self.roads.node(sg.b).pos.y, sg.ramp, sg.lanes_fwd == 0 || sg.lanes_bwd == 0));
            }
        }
        let leaves = self.city_leaves();
        out.push(format!("city leaves {} ({} on its network)", leaves.len(), leaves.iter().filter(|&&n| self.drive_component(n) == self.drive_component(s.a)).count()));
        for n in leaves.into_iter().filter(|&n| self.drive_component(n) == self.drive_component(s.a)).take(12) {
            let node = self.roads.node(n);
            let Some(&ls) = node.segments.iter().find(|&&x| self.roads.segment(x).alive && self.roads.segment(x).is_street()) else { continue };
            let show = |r: Option<(Vec<crate::traffic::LaneId>, f32, f32)>| r.map_or("NO ROUTE".to_string(), |(l, _, _)| format!("{} lanes", l.len()));
            let (plain, lorry) = (net.find_route(ls, node.pos, seg, pt), net.find_route_into_pocket(ls, node.pos, seg, pt));
            out.push(format!("  from leaf {} at ({:.0},{:.0}) seg {}: any vehicle {}, a site's lorry {}", n.0, node.pos.x, node.pos.y, ls.0, show(plain), show(lorry)));
        }
        out
    }

    /// [r2-traffic] No lorry can get to building site `cargo.to`. A home
    /// on a footpath (or with no street near it) takes the load on foot,
    /// as before; any other site waits (`SiteBlock::Deliveries`, an alert
    /// once), and the load is sent again after `SITE_RETRY` minutes.
    fn site_unsupplied(&mut self, cargo: Cargo, origin: TruckOrigin, color: u8) {
        let path_home = self.buildings.get(cargo.to).and_then(|b| self.lots.get(b.lot).map(|l| (b, l))).is_some_and(|(b, l)| b.land_use == crate::zoning::LandUse::Residential && (l.car_segment.is_none() || !self.roads.try_segment(l.front_segment).is_some_and(|s| s.is_street())));
        if path_home {
            self.deliver(cargo);
            return;
        }
        let tick = self.clock.tick;
        let newly = match self.buildings.get_mut(cargo.to).and_then(|b| b.construction.as_mut()) {
            Some(c) => {
                let newly = c.blocked.is_none();
                if c.blocked.is_none() {
                    c.blocked = Some(crate::buildings::SiteBlock::Deliveries);
                }
                c.ordered = false;
                c.retry_at = tick + SITE_RETRY;
                // [r14-materials] What rode in it is ordered again then.
                c.due.retain(|d| d.via != cargo.resource);
                newly
            }
            None => return,
        };
        let _ = (origin, color);
        if newly {
            self.site_blocked_alert(cargo.to, crate::buildings::SiteBlock::Deliveries);
        }
    }

    pub(super) fn entry_node_const(&self) -> Option<NodeId> {
        let leaves = self.city_leaves();
        let max = leaves.iter().map(|&n| self.roads.node(n).pos.length()).fold(0.0, f32::max);
        leaves.into_iter().find(|&n| self.roads.node(n).pos.length() >= max * 0.6)
    }

    pub(super) fn deliver(&mut self, cargo: Cargo) {
        // Goods for a cargo hub go into its shed for the local lorries.
        if self.hub_receive(&cargo) {
            return;
        }
        // [airport] Exports for the holds at an airport's cargo terminal.
        if self.airport_receive(&cargo) {
            return;
        }
        let Some(b) = self.buildings.get_mut(cargo.to) else { return };
        if let Some(bz) = b.business.as_mut() {
            let left = (bz.inbound_of(cargo.resource) - cargo.amount).max(0.0);
            bz.inbound.insert(cargo.resource, left);
        }
        // Materials for a building site go onto the site, not the shelves.
        if let Some(c) = b.construction.as_mut() {
            // [r2-traffic] A truck got there: the way is open.
            if c.blocked == Some(crate::buildings::SiteBlock::Deliveries) {
                c.blocked = None;
            }
            // [r14-materials] Onto the stockpile; the stage goes on when
            // all its materials are in.
            self.site_received(cargo.to, cargo.resource, cargo.amount);
            return;
        }
        if let Some(bz) = b.business.as_mut() {
            bz.add(cargo.resource, cargo.amount);
        }
    }

    /// Any business currently stocking a retail resource.
    pub(super) fn retail_available(&self, r: Resource) -> bool {
        self.buildings.values().any(|b| b.operating() && b.business.as_ref().map_or(false, |bz| !bz.closed && bz.sells(r) && bz.stock_of(r) >= 1.0))
    }

    /// The cheapest shop for `r` counting the walk, open and stocked; the
    /// plain market answer (citizens choose with `choose_shop`). For tests.
    #[cfg(test)]
    pub(crate) fn best_shop(&self, r: Resource, at: Vec2) -> Option<BuildingId> {
        let hour = self.clock.hour() as u8;
        let mut best: Option<(f32, BuildingId)> = None;
        for (id, b) in self.buildings.iter() {
            let Some(bz) = &b.business else { continue };
            if !b.operating() || bz.closed || !bz.sells(r) || bz.stock_of(r) < 1.0 || !b.serves_at(hour) {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let score = bz.sell_price(r, b.jobs) + lot.center.distance(at) * 0.02;
            if best.map_or(true, |(s, _)| score < s) {
                best = Some((score, id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// A citizen buys at a shop they have reached.
    pub(super) fn buy(&mut self, cid: CitizenId, shop: BuildingId, r: Resource) {
        let tick = self.clock.tick;
        let Some(b) = self.buildings.get_mut(shop) else { return };
        let jobs = b.jobs;
        let name = b.name.clone();
        let Some(bz) = b.business.as_mut() else { return };
        let price = bz.sell_price(r, jobs);
        let c = &mut self.citizens[cid];
        // Groceries are bought for the household's pantry from its purse,
        // enough to fill it; the rest from the shopper's pocket.
        let hh = if r == Resource::Groceries { c.household.and_then(|h| self.households.get_mut(h)) } else { None };
        let want: f32 = match (&hh, r) {
            (Some(h), _) => (h.pantry_target() - h.groceries).clamp(2.0, 15.0).floor(),
            (None, Resource::Groceries) => 3.0,
            _ => 1.0,
        };
        let purse = hh.as_ref().map_or(c.savings, |h| h.money + c.savings.max(0.0));
        // Buy what the overdraft allows, whole units; nothing if not even one.
        let affordable = ((purse - OVERDRAFT) / price).floor();
        if affordable < 1.0 {
            c.remember(tick, format!("Couldn't afford anything at {name}."));
            self.concerns.heard(cid, c, crate::concerns::Concern::Jobs, 0.4);
            c.thought = "Skint. Can't even afford the basics.".to_string();
            c.mood = (c.mood - 0.06).max(0.0);
            return;
        }
        let got = bz.take(r, want.min(affordable));
        if got <= 0.0 {
            if r == Resource::Groceries {
                self.economy.market.food.today.empty_shelves += 1;
            }
            c.remember(tick, format!("Shelves were empty at {name}."));
            self.concerns.heard(cid, c, crate::concerns::Concern::Food, 0.4);
            c.mood = (c.mood - 0.05).max(0.0);
            return;
        }
        let cost = got * price;
        // Everyone at home sees the restocked pantry, not just the shopper.
        let mut pantry: Option<(Vec<CitizenId>, f32)> = None;
        let payer = match hh {
            Some(h) => {
                h.groceries += got;
                c.groceries = h.groceries;
                pantry = Some((h.members.clone(), h.groceries));
                Account::Household(c.household.unwrap())
            }
            None => Account::Person(cid),
        };
        bz.revenue_today += cost;
        self.transfer(payer, Account::Business(shop), cost, Kind::Shopping);
        self.economy.market.deals_today += 1;
        if let Some((members, g)) = pantry {
            for m in members {
                if let Some(mc) = self.citizens.get_mut(m) {
                    mc.groceries = g;
                }
            }
        }
        let c = &mut self.citizens[cid];
        match r {
            Resource::Groceries => {
                if c.household.is_none() {
                    c.groceries += got;
                }
                self.economy.market.food.today.bought += got;
                self.economy.market.meals_sold_today += got as u32;
            }
            Resource::BakedGoods => {
                c.satiety = (c.satiety + 0.35 * got).min(1.0);
                self.economy.market.meals_sold_today += 1;
                if self.rng.random_bool(0.3) {
                    c.remember(tick, format!("Grabbed a bite at {name}."));
                }
            }
            Resource::Goods => {
                c.mood = (c.mood + 0.12).min(1.0);
                c.remember(tick, format!("Treated myself to something new at {name}."));
            }
            _ => {}
        }
    }
}

/// A round at a venue, per hour spent there.
pub(super) const VENUE_TAB: f32 = 5.0;
/// [r4-leisure-places] A visitor's hour at a place to go out takes this
/// many job-hours of its recipe over its seats per job (a round of drinks,
/// a box of popcorn, a course): a full house goes through its stock of
/// `target_hours` in about twelve.
const VISIT_STOCK: f32 = 3.0;

/// Where a truck sets off from.
#[derive(Clone, Copy, Debug)]
pub(super) enum TruckOrigin {
    Building(BuildingId),
    /// The far end of a road leaving the map: an import.
    CityEdge,
}
