//! The market: businesses' daily and hourly bookkeeping, restocking,
//! the seller index, trucks, deliveries, imports and exports, and what
//! citizens buy in shops.

use super::*;

/// [r2-traffic] Minutes a site's load waits when every lorry is on the road,
/// and before a site no lorry could reach is tried again.
const SITE_TRUCK_WAIT: u64 = 15;
const SITE_RETRY: u64 = 60;

impl World {
    /// Businesses pay wages; those that cannot for several days close, and
    /// closed buildings are demolished after a week so the lot can be reused.
    pub(super) fn daily_businesses(&mut self) {
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        let mut demolish = Vec::new();
        let mut cover = [(0.0f32, 0.0f32); BusinessKind::COUNT];
        for bid in ids {
            if !self.buildings[bid].operating() {
                continue;
            }
            let revenue = self.buildings[bid].business.as_ref().map_or(0.0, |z| z.revenue_today);
            let (wage_bill, workers, publicly_funded, kind, tax_line) = {
                let b = &self.buildings[bid];
                let Some(bz) = &b.business else { continue };
                // [parking] Lots the city runs are paid for by the city.
                let publicly_funded = matches!(bz.kind, BusinessKind::Office | BusinessKind::CargoHub) || b.land_use == LandUse::Civic || self.vehicles.city_lots.contains_key(&bid);
                // [tax] What its takings are taxed under (the city's own
                // businesses at the standard rate, as before).
                let tax_line = if publicly_funded { None } else { crate::tax::TaxCategory::of_business(bz.kind, b.tier) };
                (b.workers.len() as f32 * b.wage() * 0.6, b.workers.clone(), publicly_funded, bz.kind, tax_line)
            };
            let (rate, surcharge) = tax_line.map_or((crate::tax::BUSINESS_TAX, 0.0), |c| (self.economy.taxes.rate(c), self.economy.taxes.surcharge(c)));
            let b = &mut self.buildings[bid];
            let bz = b.business.as_mut().unwrap();
            if bz.closed {
                // A shut business takes nothing and owes no tax; whatever
                // the inspector still showed from its last day is cleared.
                bz.close_books(0.0, 0.0);
                bz.revenue_today = 0.0;
                bz.spend_today = 0.0;
                bz.broke_days = bz.broke_days.saturating_add(1);
                if bz.broke_days > 7 {
                    if b.plot.tower.is_some() {
                        // [supertall] Nobody pulls down a landmark tower
                        // for a failed tenant: offices take the floors.
                        let jobs = b.jobs;
                        b.business = Some(Business::new(BusinessKind::Office, jobs));
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
            self.economy.credit_item(Flow::BusinessTax, tax, Item::LandUse(lu));
            if let Some(c) = tax_line {
                self.economy.taxes.collect(c, tax);
            }
            if !publicly_funded {
                bz.money -= wage_bill;
                bz.spend_today += wage_bill;
                // [tax] Prices were set on takings after the standard rate:
                // a rate over it is a cost the business carries (and its
                // cover, which decides whether more of it are built, shows
                // it); one under it is money kept.
                let extra = revenue * surcharge;
                if extra != 0.0 {
                    bz.money -= extra;
                    bz.spend_today += extra;
                }
                // The market's reading of this kind of business, past the
                // first day's stocking up: what it took against what it
                // paid out, wages and inputs.
                if bz.days_open >= 1 {
                    let e = &mut cover[kind.index()];
                    e.0 += revenue;
                    e.1 += bz.spend_today;
                }
            }
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
                b.condition = b.condition.min(0.25);
                b.workers.clear();
                let name = b.name.clone();
                for w in workers {
                    self.lay_off(w, Some(format!("{name} shut down. Out of a job.")));
                    if let Some(c) = self.citizens.get_mut(w) {
                        c.mood = (c.mood - 0.15).max(0.0);
                    }
                }
                self.economy.market.closed_total += 1;
                self.events.push(SimEvent::BusinessClosed(bid));
                self.events.push(SimEvent::BuildingChanged(bid));
            }
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
                    bz.produce(jobs, staffing * b.powered.clamp(0.0, 1.0));
                }
            }
        }
        // Walk-in trade: a share of the people who passed a shop this hour
        // step inside and buy something off the shelf, if it is open.
        let hour = self.clock.hour() as u8;
        // [megacorp] Students near a campus drop into pizza places, cafés
        // and bookshops.
        let pull = self.campus_pull();
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
            let b = &mut self.buildings[bid];
            if !b.operating() {
                continue;
            }
            let open = b.opening_hours().contains(hour);
            let Some(bz) = b.business.as_mut() else { continue };
            if bz.closed || !open {
                continue;
            }
            let Some(output) = bz.kind.walk_in_sale() else { continue };
            let want = feet * 0.06;
            let got = bz.take(output, want.min(bz.stock_of(output)));
            if got > 0.0 {
                let price = bz.sell_price(output, jobs);
                if students > 0.0 {
                    self.corps.stats.student_trade_today += got * price * students / feet;
                }
                bz.money += got * price;
                bz.revenue_today += got * price;
                bz.deals += 1;
                self.economy.walk_in_sales_today += got * price;
                if output == Resource::BakedGoods {
                    self.economy.market.meals_sold_today += got as u32;
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
                if bz.kind == BusinessKind::Grocery && grocery_short {
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
                            sb.money += cost;
                            sb.revenue_today += cost;
                            sb.deals += 1;
                        }
                        {
                            let bb = self.buildings[bid].business.as_mut().unwrap();
                            bb.money -= cost;
                            bb.spend_today += cost;
                        }
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
                            bb.money -= cost + duty;
                            bb.spend_today += cost + duty;
                        }
                        if duty > 0.0 {
                            self.economy.credit(Flow::ImportDuty, duty);
                            self.economy.taxes.duty_today += duty;
                        }
                        self.economy.taxes.note_import(r, amount);
                        self.economy.market.imports_today += cost;
                        if r == Resource::Groceries {
                            self.economy.market.grocery_imports_today += amount;
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
                let have = bz.stock_of(*r);
                if have > keep * 1.2 {
                    let surplus = have - keep;
                    bz.take(*r, surplus);
                    let income = surplus * r.base_price() * EXPORT_DISCOUNT * export_bonus;
                    shipped += surplus;
                    bz.money += income;
                    bz.revenue_today += income;
                    self.economy.market.exports_today += income;
                }
            }
        }
        // [freight] Onto the apron of a yard the freight trains call at.
        if let Some(&y) = self.yards.first().or(yards.first()) {
            self.buildings[y].rail_outbound += shipped;
        }
        // Exports through a cargo hub leave in full semis.
        self.hub_note_exports(shipped);
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
    /// asleep); people at home eat from the household pantry, those out eat
    /// the meal they packed. Returns how many exhausted people are at work
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
            if at_home && c.satiety < 0.65 {
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
        for c in self.citizens.values_mut() {
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
            if c.savings < tab {
                continue;
            }
            c.savings -= tab;
            bz.money += tab;
            bz.revenue_today += tab;
            c.mood = (c.mood + 0.01).min(1.0);
            if regular {
                self.places.reputation.entry(b).or_default().regular_revenue_today += tab;
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
        self.traffic.cars.values().filter(|c| c.cargo.is_some()).count()
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
        let to_site = self.buildings.get(cargo.to).is_some_and(|b| b.construction.is_some());
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            // [r2-traffic] A building site's load waits for a truck: the
            // site does not start on parts nobody brought (K6).
            if to_site {
                let from = match origin {
                    TruckOrigin::Building(b) => Some(b),
                    TruckOrigin::CityEdge => None,
                };
                self.freight_queue.push(crate::world::delivery_hours::QueuedTruck { due: self.clock.tick + SITE_TRUCK_WAIT, from, cargo, color });
                return;
            }
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
            self.traffic.net.find_route(from_seg, from_pt, to_seg, to_pt)
        })();
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
                    if let Some(l) = self.corp_livery(cargo.from) {
                        self.traffic.cars[id].model.livery = l;
                    } else if let Some(l) = self.corp_livery(cargo.to).filter(|_| self.buildings.get(cargo.to).and_then(|b| b.business.as_ref()).is_some_and(|z| z.kind.customers_per_job().is_some())) {
                        // [sweep-sim] A corp's shop is stocked by the corp's
                        // own vans, so a corp of shops alone has a fleet too.
                        let m = &mut self.traffic.cars[id].model;
                        m.livery = l;
                        m.body = crate::vehicle::BodyType::DeliveryVan;
                    }
                }
            }
            None => self.deliver(cargo),
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
            self.traffic.net.find_route(seg, node.pos, to_seg, to_pt)
        })
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
        let Some(b) = self.buildings.get_mut(cargo.to) else { return };
        if let Some(bz) = b.business.as_mut() {
            let left = (bz.inbound_of(cargo.resource) - cargo.amount).max(0.0);
            bz.inbound.insert(cargo.resource, left);
        }
        // Materials for a building site go onto the site, not the shelves.
        if let Some(c) = b.construction.as_mut() {
            c.deliveries = c.deliveries.saturating_add(1);
            c.ordered = false;
            // [r2-traffic] A truck got there: the way is open.
            if c.blocked == Some(crate::buildings::SiteBlock::Deliveries) {
                c.blocked = None;
            }
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
            c.remember(tick, format!("Shelves were empty at {name}."));
            self.concerns.heard(cid, c, crate::concerns::Concern::Food, 0.4);
            c.mood = (c.mood - 0.05).max(0.0);
            return;
        }
        let cost = got * price;
        // Everyone at home sees the restocked pantry, not just the shopper.
        let mut pantry: Option<(Vec<CitizenId>, f32)> = None;
        match hh {
            Some(h) => {
                h.money -= cost;
                h.groceries += got;
                c.groceries = h.groceries;
                pantry = Some((h.members.clone(), h.groceries));
            }
            None => c.savings -= cost,
        }
        bz.money += cost;
        bz.revenue_today += cost;
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

/// Where a truck sets off from.
#[derive(Clone, Copy, Debug)]
pub(super) enum TruckOrigin {
    Building(BuildingId),
    /// The far end of a road leaving the map: an import.
    CityEdge,
}
