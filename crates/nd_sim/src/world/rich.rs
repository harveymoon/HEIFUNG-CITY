//! [rich] The well-off hold more than a home: second homes and businesses
//! bought as investments, and cars in the garage.
//!
//! *Investments* (`invest_rich`, daily, a 35% roll): the richest household
//! earning [`RICH_INCOME`] or more a day (the McMansion bar), not a
//! billionaire's, with [`INVEST_CASH`] in the bank and fewer holdings than
//! its income carries ([`holdings_cap`]), buys one property in its earner's
//! name: a business doing well (open, three days in profit) if it owns
//! none yet, else a second home in a middle or high block. It puts a
//! deposit down ([`INVEST_DEPOSIT`] of its cash); the rest is the bank's.
//! The tenants and staff stay; the owner record changes, so selecting any
//! of it outlines the rest and the person's pane pins every one. The
//! super-elite already buy their extra homes and firms when they arrive
//! (`spawn_elite`).
//!
//! *Returns* (`pay_investors`, daily): a second home pays its owner the
//! rent its tenants paid less upkeep and the bank ([`INVEST_RENT_NET`]); a
//! business bought into pays [`INVEST_PROFIT_SHARE`] of yesterday's profit
//! out of its till. Founders who run their own firm (a taxi company, a car
//! park) keep the money in the business.
//!
//! *Garages*: a billionaire's mansion, once built, takes the supercars
//! beyond the one they drive (`Elite::cars`), and a family's McMansion a
//! second car: real cars parked in the house's own spaces
//! (`OwnedCar::spare`), kept with their owner and sold when the owner is
//! gone. At the weekend the owner may take the dearest out instead
//! (`weekend_cars`): the two swap places in the garage, and on Monday the
//! everyday car is back in use.
//!
//! Draws come from the estate's hash-seeded generators, never the world's.

use super::*;
use crate::estate::Form;
use crate::vehicle::{CarEvent, CarPlace, OwnedCar, ParkKind, VehicleId, VehicleModel};

/// Daily household income from which a family invests (the McMansion bar).
pub const RICH_INCOME: f32 = 220.0;
/// Cash a household must hold before it buys.
pub const INVEST_CASH: f32 = 1_000.0;
/// Share of its cash a household puts down on a purchase.
pub const INVEST_DEPOSIT: f32 = 0.3;
/// [sweep-sim] Share of a let home's rent its investor keeps (the rest is
/// upkeep and the bank's).
pub const INVEST_RENT_NET: f32 = 0.45;
/// [sweep-sim] Share of a business's day's profit paid to an investor.
pub const INVEST_PROFIT_SHARE: f32 = 0.3;
/// [sweep-sim] Chance (hash roll) an owner takes the dearest car out on a
/// weekend day.
pub const WEEKEND_DRIVE: f64 = 0.5;

/// How many properties a household earning `income` a day holds at most
/// (its home not counted): one at the bar, up to four for the very rich.
pub fn holdings_cap(income: f32) -> usize {
    ((income / RICH_INCOME).floor() as usize).clamp(1, 4)
}

impl World {
    /// The household's top earner: whose name the deeds go in.
    fn earner_of(&self, hid: HouseholdId) -> Option<CitizenId> {
        let h = self.households.get(hid)?;
        h.members.iter().copied().filter(|m| self.citizens.get(*m).is_some_and(|c| c.stage() == crate::citizens::LifeStage::Adult)).max_by(|a, b| self.citizens[*a].income.total_cmp(&self.citizens[*b].income).then(b.cmp(a)))
    }

    /// Once a day: one well-off household buys a business or a second home.
    pub(super) fn invest_rich(&mut self) {
        let mut rng = self.erng(super::estate::erng_salt::INVEST_RICH);
        if !rng.random_bool(0.35) {
            return;
        }
        let mut rich: Vec<(f32, HouseholdId)> = self
            .households
            .iter()
            .filter(|(_, h)| h.money >= INVEST_CASH && h.members.iter().all(|m| !self.estate.is_elite(*m)))
            .map(|(id, _)| (self.household_income(id), id))
            .filter(|(i, _)| *i >= RICH_INCOME)
            .collect();
        rich.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (income, hid) in rich.into_iter().take(4) {
            let Some(cid) = self.earner_of(hid) else { continue };
            let home = self.households[hid].home;
            let held: Vec<BuildingId> = self.properties_of(cid).into_iter().filter(|b| Some(*b) != home).collect();
            if held.len() >= holdings_cap(income) {
                continue;
            }
            let owns_business = held.iter().any(|b| self.buildings.get(*b).is_some_and(|x| x.business.is_some()));
            let pick = if owns_business { self.second_home_for(hid) } else { self.investment_business().or_else(|| self.second_home_for(hid)) };
            let Some(bid) = pick else { continue };
            let Some(owner) = self.owner_for_citizen(cid) else { continue };
            let price = self.sale_price(bid);
            let deposit = self.households[hid].money * INVEST_DEPOSIT;
            self.households[hid].money -= deposit;
            self.give_to(bid, owner, price);
            let tick = self.clock.tick;
            let (name, is_business, pos) = {
                let b = &self.buildings[bid];
                (b.name.clone(), b.business.is_some(), self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center))
            };
            let what = if is_business { "Bought into a business" } else { "Bought a second place" };
            self.citizens[cid].remember(tick, format!("{what}: {name}. Put ${deposit:.0} down."));
            if is_business {
                let who = self.citizens[cid].name.clone();
                self.social.push_news(tick, format!("💼 {who} bought {name}."), pos, None);
            }
            self.estate.stats.investments_total += 1;
            return;
        }
    }

    /// A business worth buying into: open, independently owned, the market
    /// may sell it, three days in profit; the best earner first.
    fn investment_business(&self) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter(|(id, b)| b.plot.owner.is_none() && b.operating() && self.buyable(*id))
            .filter_map(|(id, b)| {
                let z = b.business.as_ref()?;
                if z.closed || z.owner.is_some() || z.books.len() < 3 {
                    return None;
                }
                let last = &z.books[z.books.len() - 3..];
                let profit: f32 = last.iter().map(|d| d.revenue - d.spend).sum();
                (last.iter().all(|d| d.revenue > d.spend)).then_some((profit, id))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|(_, id)| id)
    }

    /// A second home for `hid`: a middle or high block of a few homes,
    /// independently owned, not the one it lives in; the best land first.
    fn second_home_for(&self, hid: HouseholdId) -> Option<BuildingId> {
        let home = self.households.get(hid)?.home;
        self.buildings
            .iter()
            .filter(|(id, b)| Some(*id) != home && b.land_use == LandUse::Residential && b.business.is_none() && b.tier >= Tier::Mid && b.homes > 0 && b.homes <= 24 && b.plot.owner.is_none() && b.operating() && self.buyable(*id))
            .max_by(|a, b| a.1.land_value.total_cmp(&b.1.land_value).then(b.0.cmp(&a.0)))
            .map(|(id, _)| id)
    }

    /// [sweep-sim] Once a day: every household or person holding a home
    /// they do not live in, or a business they bought into, is paid by it.
    pub(super) fn pay_investors(&mut self) {
        use crate::estate::OwnerKind;
        // What each home's households paid in rent today.
        let mut rent: std::collections::HashMap<BuildingId, f32> = std::collections::HashMap::new();
        for (hid, h) in self.households.iter() {
            if let Some(home) = h.home.filter(|b| self.buildings.contains_key(*b)) {
                *rent.entry(home).or_default() += self.rent_due(hid, home);
            }
        }
        let owners: Vec<(crate::estate::OwnerId, HouseholdId, Vec<BuildingId>)> = self
            .estate
            .owners
            .iter()
            .filter_map(|(oid, o)| {
                let hid = match o.kind {
                    OwnerKind::Household { household } => household,
                    OwnerKind::Person { citizen } => self.citizens.get(citizen)?.household?,
                    _ => return None,
                };
                self.households.contains_key(hid).then(|| (oid, hid, o.properties.clone()))
            })
            .collect();
        for (oid, hid, props) in owners {
            let home = self.households[hid].home;
            let mut paid = 0.0f32;
            for b in props {
                if Some(b) == home {
                    continue;
                }
                let Some(bld) = self.buildings.get_mut(b) else { continue };
                if let Some(z) = bld.business.as_mut() {
                    // A founder runs their own firm: the money stays in it.
                    if z.owner.is_some() || z.closed {
                        continue;
                    }
                    let profit = z.books.last().map_or(0.0, |d| d.revenue - d.spend);
                    let share = (profit * INVEST_PROFIT_SHARE).min(z.money).max(0.0);
                    z.money -= share;
                    paid += share;
                } else if bld.homes > 0 {
                    paid += rent.get(&b).copied().unwrap_or(0.0) * INVEST_RENT_NET;
                }
            }
            self.households[hid].money += paid;
            if let Some(o) = self.estate.owners.get_mut(oid) {
                o.income_day = paid;
                o.earned += paid as f64;
            }
        }
    }

    /// [sweep-sim] What `cid`'s holdings paid yesterday (0 for none).
    pub fn investment_income(&self, cid: CitizenId) -> f32 {
        let hh = self.citizens.get(cid).and_then(|c| c.household);
        self.estate.owners.values().filter(|o| o.kind.citizen() == Some(cid) || hh.is_some_and(|h| o.kind == crate::estate::OwnerKind::Household { household: h })).map(|o| o.income_day).sum()
    }

    /// [sweep-sim] Weekend drives: on a Saturday or Sunday an owner with a
    /// spare car in the same garage as the one they drive may take the
    /// dearest out (the two swap); on a weekday the cheapest is the one in
    /// use again. Only cars standing in the same building swap, so nothing
    /// moves and no space changes hands. Hash rolls, no world draws.
    pub(super) fn weekend_cars(&mut self) {
        let day = self.clock.day();
        let weekend = day % 7 >= 5;
        let owners: Vec<CitizenId> = {
            let mut v: Vec<CitizenId> = self.vehicles.cars.values().filter(|c| c.spare).map(|c| c.owner).collect();
            v.sort();
            v.dedup();
            v
        };
        for cid in owners {
            let Some(c) = self.citizens.get(cid) else { continue };
            if c.to_car.is_some() {
                continue;
            }
            let Some(now) = c.car else { continue };
            let Some(garage) = self.vehicles.cars.get(now).and_then(|x| x.spot()).and_then(|s| s.building) else { continue };
            if weekend && crate::rng_for(self.config.seed ^ c.seed ^ 0x3EE6_E4D, day).random::<f64>() >= WEEKEND_DRIVE {
                continue;
            }
            let at_home: Vec<(VehicleId, f32)> = self.spare_cars_of(cid).into_iter().filter(|v| self.vehicles.cars.get(*v).and_then(|x| x.spot()).and_then(|s| s.building) == Some(garage)).map(|v| (v, self.vehicles.cars[v].model.price())).collect();
            let now_price = self.vehicles.cars[now].model.price();
            let pick = if weekend {
                at_home.iter().copied().filter(|(_, p)| *p > now_price).max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            } else {
                // Monday: the everyday car, marked when it was left at home.
                at_home.iter().copied().find(|(v, _)| self.vehicles.cars[*v].usual)
            };
            let Some((take, _)) = pick else { continue };
            self.vehicles.cars[now].spare = true;
            self.vehicles.cars[now].usual = weekend;
            self.vehicles.cars[take].spare = false;
            self.vehicles.cars[take].usual = false;
            self.citizens[cid].car = Some(take);
            if weekend {
                let tick = self.clock.tick;
                self.citizens[cid].remember(tick, "Took the good car out for the weekend.".to_string());
            }
        }
    }

    // ── garages ──────────────────────────────────────────────────────────

    /// The cars `cid` keeps in a garage: spare cars, out only at weekends
    /// (`weekend_cars`).
    pub fn spare_cars_of(&self, cid: CitizenId) -> Vec<VehicleId> {
        self.vehicles.cars.iter().filter(|(_, c)| c.spare && c.owner == cid).map(|(id, _)| id).collect()
    }

    /// Park `models` as `cid`'s spare cars in `at`'s own spaces (the garage
    /// grows to take them).
    pub(super) fn garage_spares(&mut self, cid: CitizenId, at: BuildingId, models: &[VehicleModel]) {
        if models.is_empty() || !self.citizens.contains_key(cid) {
            return;
        }
        let Some(b) = self.buildings.get_mut(at) else { return };
        b.parking_capacity = b.parking_capacity.max(b.parking_used.saturating_add(models.len() as u16));
        let tick = self.clock.tick;
        for (k, model) in models.iter().enumerate() {
            let Some(spot) = self.building_spot(ParkKind::Home, at) else { return };
            let color = ((self.citizens[cid].seed >> 8) as u8).wrapping_add(k as u8) % 8;
            let vid = self.vehicles.cars.insert(OwnedCar { owner: cid, model: *model, place: CarPlace::Parked(spot), color, since: tick, spare: true, usual: false, dealer: None });
            // `park_at` books the space.
            self.park_at(vid, spot);
            self.vehicles.note(CarEvent::Bought);
        }
    }

    /// A house bought outright is finished: a billionaire's mansion takes
    /// the supercars beyond the one they drive, a McMansion a second car.
    /// Called from `estate_finished`.
    pub(super) fn fill_garage(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        match b.plot.form {
            Some(Form::Mansion) => {
                let Some(e) = self.estate.elites.iter().find(|e| e.mansion == Some(bid)) else { return };
                let (cid, models) = (e.citizen, e.cars.iter().skip(1).copied().collect::<Vec<_>>());
                if self.spare_cars_of(cid).is_empty() {
                    self.garage_spares(cid, bid, &models);
                }
            }
            Some(Form::McMansion) => {
                let Some(hid) = b.plot.buyer else { return };
                let Some(cid) = self.earner_of(hid) else { return };
                if self.spare_cars_of(cid).is_empty() {
                    let c = &self.citizens[cid];
                    let model = VehicleModel::fallback(c.seed ^ 0x5EC0_4D, c.income.max(crate::vehicle::HIGH_INCOME), c.age);
                    self.garage_spares(cid, bid, &[model]);
                }
            }
            _ => {}
        }
    }

    /// Spare cars whose owner is gone are sold off. Daily.
    pub(super) fn prune_spare_cars(&mut self) {
        let gone: Vec<VehicleId> = self.vehicles.cars.iter().filter(|(_, c)| c.spare && !self.citizens.contains_key(c.owner)).map(|(id, _)| id).collect();
        for vid in gone {
            self.drop_car(vid, CarEvent::Sold);
        }
    }

    /// For the after-load sweep: a spare car is kept while its owner
    /// lives here.
    pub(super) fn spare_kept(&self, c: &OwnedCar) -> bool {
        c.spare && self.citizens.contains_key(c.owner)
    }
}
