//! Parking businesses. Where cars park and how drivers look for a space
//! is the car lifecycle's (`world/cars.rs`): a car keeps its space until it
//! drives off, so a lot's occupancy is the cars standing in it.

use super::*;

impl World {
    /// [parking] Put down a parking lot the city runs (the Buildings tool,
    /// Transit): on any lot with a street, buying out what grew there; its
    /// size sets the spaces and the look, the fee starts at the default
    /// (`set_parking_price` changes it) and goes to the treasury.
    pub fn build_city_parking(&mut self, lot_id: LotId, size: crate::enforcement::LotSize) -> Result<BuildingId, String> {
        let what = crate::services::Placeable::ParkingLot(size);
        if let Some(problem) = self.placement_problem(what, lot_id) {
            return Err(problem);
        }
        // [parking-fit] Next to one of the city's own lots of the same kind:
        // the plot joins it, and pays for the ground it adds.
        if let Some(next) = self.city_parking_beside(lot_id, size) {
            return self.extend_city_parking(next, lot_id);
        }
        let area = self.lots[lot_id].area();
        let cost = size.cost_for(area);
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; {} on {:.0} m² costs {:.0}", self.economy.treasury, crate::names::a(size.label()), area, cost));
        }
        self.take_over_lot(lot_id, size.label())?;
        // [free-placement] The city's own lot: out of the zone.
        self.take_for_city(lot_id);
        self.economy.debit_item(Flow::Construction, cost, Item::Named(size.label().to_string()));
        let n = self.fresh_seed() % 90 + 1;
        let name = match size {
            crate::enforcement::LotSize::Small => format!("City Lot {n}"),
            crate::enforcement::LotSize::Large => format!("City Parking {n}"),
            crate::enforcement::LotSize::MultiStorey => format!("City Car Park {n}"),
        };
        let price = size.default_price();
        let jobs = size.jobs_for(area);
        let bz = Business::new(BusinessKind::Parking, jobs);
        // [parking] A surface lot has the bays its parcel fits (as drawn);
        // [parking-fit] a car park its decks' spaces, by the plot's area.
        let capacity = self.city_lot_spaces(size, lot_id);
        let id = self.place_special(lot_id, name, jobs, capacity, |b| {
            b.tier = Tier::Mid;
            b.floors = size.floors();
            b.business = Some(bz);
            b.parking_price = price;
            b.city_parking = true;
            // [free-placement] Its own use (the ground is the city's).
            b.land_use = LandUse::Commercial;
        });
        self.vehicles.city_lots.insert(id, crate::enforcement::CityLot { size, price, area });
        Ok(id)
    }

    /// Spaces in a city lot of `size` on `lot_id`: a surface lot the bays
    /// drawn on its ground (`parking_layout`), a car park its decks by the
    /// plot's area.
    pub(super) fn city_lot_spaces(&self, size: crate::enforcement::LotSize, lot_id: LotId) -> u16 {
        match self.lots.get(lot_id) {
            Some(l) if size.surface() => crate::parking_layout::surface_capacity(l).max(1),
            Some(l) => size.capacity_for(l.area()),
            None => size.capacity(),
        }
    }

    /// [parking-fit] One of the city's own parking lots of the same kind
    /// (surface, or decks) sharing a boundary with `lot_id`, whose ground
    /// and the plot's join into one piece.
    pub(super) fn city_parking_beside(&self, lot_id: LotId, size: crate::enforcement::LotSize) -> Option<BuildingId> {
        let lot = self.lots.get(lot_id)?;
        let mut ids: Vec<BuildingId> = self.vehicles.city_lots.iter().filter(|(_, c)| c.size.surface() == size.surface()).map(|(b, _)| *b).collect();
        ids.sort();
        ids.into_iter().find(|&b| {
            let Some(other) = self.buildings.get(b).filter(|x| x.demolition.is_none()).and_then(|x| self.lots.get(x.lot)) else { return false };
            if other.center.distance(lot.center) > 200.0 {
                return false;
            }
            let Some(o) = join_plots(&other.polygon, &lot.polygon) else { return false };
            let shared = (crate::geom::perimeter(&other.polygon) + crate::geom::perimeter(&lot.polygon) - crate::geom::perimeter(&o)) * 0.5;
            shared >= 4.0 && crate::geom::polygon_area(&o).abs() > other.area() + lot.area() * 0.9
        })
    }

    /// [parking-fit] Grow the city's lot `b` over the plot `lot_id` next to
    /// it: whatever stood there is bought out and cleared, the ground joins
    /// the lot (and its spaces, jobs and upkeep grow with it), and the city
    /// pays for the added area.
    pub(super) fn extend_city_parking(&mut self, b: BuildingId, lot_id: LotId) -> Result<BuildingId, String> {
        let size = self.vehicles.city_lots.get(&b).map(|c| c.size).ok_or("not a parking lot the city runs")?;
        let home = self.buildings.get(b).map(|x| x.lot).ok_or("no such building")?;
        let outline = join_plots(&self.lots[home].polygon, &self.lots[lot_id].polygon).ok_or("the plots do not join")?;
        let added = self.lots[lot_id].area();
        let cost = size.cost_for(added);
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; {:.0} m² more of {} costs {:.0}", self.economy.treasury, added, size.label(), cost));
        }
        if let Some(old) = self.lots[lot_id].building {
            self.take_over_lot(lot_id, size.label())?;
            if self.buildings.get(old).is_some_and(|x| x.demolition.is_some()) {
                self.finish_demolition(old);
            }
            if self.lots.get(lot_id).is_some_and(|l| l.building.is_some()) {
                return Err("the plot could not be cleared".into());
            }
        }
        self.economy.debit_item(Flow::Construction, cost, Item::Named(size.label().to_string()));
        self.lots.remove(lot_id);
        for p in self.parcels.parcels.iter_mut().filter(|p| p.lot == Some(lot_id)) {
            p.lot = Some(home);
            p.land_use = Some(LandUse::Civic);
        }
        let roads = &self.roads;
        let l = &mut self.lots[home];
        l.polygon = outline;
        l.center = crate::zoning::inner_center(&l.polygon);
        super::plan::refresh_lot_roads(l, roads, true);
        let area = l.area();
        if let Some(z) = self.zones.get_mut(l.zone) {
            z.polygon = l.polygon.clone();
        }
        if let Some(c) = self.vehicles.city_lots.get_mut(&b) {
            c.area = area;
        }
        let spaces = self.city_lot_spaces(size, home);
        if let Some(x) = self.buildings.get_mut(b) {
            x.parking_capacity = spaces;
            x.jobs = size.jobs_for(area);
        }
        self.rebuild_walk();
        self.events.push(SimEvent::LotsChanged);
        self.events.push(SimEvent::BuildingChanged(b));
        Ok(b)
    }

    /// Open a parking business on a vacant lot: a surface lot where land is
    /// cheap, a multi-storey car park in the middle. The cheapest business
    /// there is: two to four jobs and a little float.
    pub fn build_parking(&mut self, lot_id: LotId) -> Option<BuildingId> {
        // [review] Not on a plot an owner bought, nor on rubble.
        if !self.lot_free_for(lot_id, SiteRule::STREET) {
            return None;
        }
        let lot = self.lots.get(lot_id)?;
        let area = lot.area();
        let value = self.land_value.at(lot.center);
        let tier = Tier::for_land_value(value).min(Tier::Mid);
        let floors: u8 = if tier == Tier::Mid && area > 500.0 { self.rng.random_range(3..=5) } else { 0 };
        // [parking] A surface lot has the bays its parcel fits (as drawn).
        let capacity = if floors == 0 { crate::parking_layout::surface_capacity(lot) } else { ((area / 32.0) as u16) * floors as u16 };
        if capacity < 4 {
            return None;
        }
        let name = if floors == 0 { format!("Lot {}", self.rng.random_range(1..99)) } else { format!("ParkHaus {}", self.rng.random_range(1..40)) };
        let jobs = if floors == 0 { 2 } else { 4 };
        let mut bz = Business::new(BusinessKind::Parking, jobs);
        bz.money = 80.0;
        Some(self.place_special(lot_id, name, jobs, capacity, |b| {
            // A surface lot is no building at all; a ParkHaus is a cheap one.
            b.tier = tier;
            b.floors = floors;
            b.business = Some(bz);
            b.parking_price = Self::parking_base_price(tier, value);
        }))
    }

    /// [parking-market] The fee a parking business starts at on its land:
    /// by tier and land value.
    pub fn parking_base_price(tier: Tier, value: f32) -> f32 {
        3.0 * tier.wage_multiplier() * (0.6 + value)
    }

    /// [parking-market] At midnight every parking business (not the
    /// city's) sets tomorrow's fee by how full it got today: at least
    /// [`PARKING_FULL`] full it charges 15% more (up to 2.5x its base),
    /// under [`PARKING_SLACK`] 10% less (down to 0.6x). A busy lot earns
    /// more, which is what draws others to open near it.
    pub(super) fn reprice_parking(&mut self) {
        let city: Vec<BuildingId> = self.vehicles.city_lots.keys().copied().collect();
        let lots = &self.lots;
        let lv = &self.land_value;
        for (id, b) in self.buildings.iter_mut() {
            if !b.is_parking() || b.parking_capacity == 0 || city.contains(&id) || !b.operating() {
                b.parking_peak = 0;
                continue;
            }
            let full = b.parking_peak as f32 / b.parking_capacity as f32;
            let value = lots.get(b.lot).map_or(b.land_value, |l| lv.at(l.center));
            let base = Self::parking_base_price(b.tier, value);
            if full >= PARKING_FULL {
                b.parking_price = (b.parking_price * 1.15).min(base * 2.5);
            } else if full < PARKING_SLACK {
                b.parking_price = (b.parking_price * 0.9).max(base * 0.6);
            }
            b.parking_peak = 0;
        }
    }

    /// [parking-market] How full parking near `at` got today: spaces taken
    /// at the busiest moment over spaces, in every parking business within
    /// `r` m (`None` with none).
    pub fn parking_fill_near(&self, at: Vec2, r: f32) -> Option<f32> {
        let (mut peak, mut cap) = (0u32, 0u32);
        for b in self.buildings.values().filter(|b| b.is_parking() && b.operating() && b.parking_capacity > 0) {
            if self.lots.get(b.lot).is_some_and(|l| l.center.distance(at) <= r) {
                peak += b.parking_peak.max(b.parking_used) as u32;
                cap += b.parking_capacity as u32;
            }
        }
        (cap > 0).then(|| peak as f32 / cap as f32)
    }

    /// [parking-market] Where drivers have most been circling: the concerns'
    /// Parking hotspot of the last three days, else where the last driver
    /// gave up looking.
    #[cfg(test)]
    pub(crate) fn parking_trouble_spot(&self) -> Option<Vec2> {
        self.concerns.hotspot(crate::concerns::Concern::Parking, self.clock.day(), 3).map(|(p, _)| p).or(self.economy.parking_shortage_at)
    }

    /// [parking-market] The vacant lot a new parking business would take
    /// for trouble at `at`: none while the lots round it had room at their
    /// busiest (drivers circle for the kerb, not for want of spaces) or one
    /// opened near it these two days; else the nearest vacant lot within
    /// 220 m on ground that is not dear, not a park or civic site.
    pub fn parking_site_near(&self, at: Vec2, vacant: &[LotId]) -> Option<LotId> {
        if self.parking_fill_near(at, 220.0).is_some_and(|f| f < PARKING_FULL) {
            return None;
        }
        let tick = self.clock.tick;
        // Every parking business, and whether it opened these two days.
        let lots: Vec<(Vec2, bool)> = self.buildings.values().filter(|b| b.is_parking()).filter_map(|b| self.lots.get(b.lot).map(|l| (l.center, tick.saturating_sub(b.built_tick) < 2 * 1440))).collect();
        if lots.iter().any(|(p, fresh)| *fresh && p.distance(at) < PARKING_SPACING) {
            return None;
        }
        // Never right beside another lot, nor near one just opened.
        let crowded = |p: Vec2| lots.iter().any(|(q, fresh)| q.distance(p) < if *fresh { PARKING_SPACING } else { PARKING_NEIGHBOUR });
        vacant
            .iter()
            .copied()
            .filter(|&l| self.lots.get(l).is_some_and(|lot| lot.building.is_none() && !matches!(lot.land_use, LandUse::Park | LandUse::Civic) && !crowded(lot.center)))
            .filter(|&l| Tier::for_land_value(self.land_value.at(self.lots[l].center)) != Tier::High)
            .map(|l| (self.lots[l].center.distance(at), l))
            .filter(|(d, _)| *d <= 220.0)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, l)| l)
    }

    /// [parking-market] Where drivers circle and the parking there is full,
    /// someone opens more: a resident with the savings to build it (a
    /// surface lot or a car park, by the land), on the vacant lot nearest
    /// the trouble. The trigger is drivers circling today
    /// ([`PARKING_CIRCLING`], fewer while the town's parking businesses pay
    /// well, more while they lose money); the spot is where the concerns
    /// heard about parking most these three days, then where the last driver
    /// gave up. Nothing opens while the lots near the spot had room at
    /// their busiest, within [`PARKING_SPACING`] of a lot opened these two
    /// days, or once the town's paid spaces match its cars ([`PARKING_MIN_CAP`] at least).
    /// True if one opened.
    pub(super) fn maybe_open_parking(&mut self, vacant: &[LotId]) -> bool {
        let circling = self.economy.parking_street_today;
        // The trade's own books: a well-paid trade draws newcomers sooner.
        let need = match self.economy.cover_of(BusinessKind::Parking) {
            Some(c) if c >= 1.3 => PARKING_CIRCLING - 1,
            Some(c) if c < 0.9 => PARKING_CIRCLING * 2 + 2,
            _ => PARKING_CIRCLING,
        };
        if circling < need {
            return false;
        }
        // A few rounds a day look, not every one.
        if (self.clock.tick / self.config.development_interval.max(1)) % 3 != 0 {
            return false;
        }
        let paid: u32 = self.buildings.values().filter(|b| b.is_parking()).map(|b| b.parking_capacity as u32).sum();
        if paid as usize >= self.vehicles.cars.len().max(PARKING_MIN_CAP) {
            return false;
        }
        // The worst spot of these days first, else where the last driver
        // gave up (the hotspot may be dear ground with nothing free).
        let hotspot = self.concerns.hotspot(crate::concerns::Concern::Parking, self.clock.day(), 3).map(|(p, _)| p);
        let spots: Vec<Vec2> = hotspot.into_iter().chain(self.economy.parking_shortage_at).collect();
        let Some(lot_id) = spots.iter().find_map(|&at| self.parking_site_near(at, vacant)) else { return false };
        let lot_pos = self.lots[lot_id].center;
        let car_park = Tier::for_land_value(self.land_value.at(lot_pos)) == Tier::Mid && self.lots[lot_id].area() > 500.0;
        let cost = if car_park { PARKING_CAR_PARK_COST } else { PARKING_LOT_COST };
        // The founder: a grown-up who can pay for it, the nearer the better.
        let founder = self
            .citizens
            .iter()
            .filter(|(id, c)| c.stage() == crate::citizens::LifeStage::Adult && !self.estate.is_elite(*id) && self.wealth_of(c) >= cost)
            .filter_map(|(id, c)| c.home.and_then(|h| self.building_lot(h)).map(|l| (id, l.center.distance(lot_pos), self.wealth_of(c))))
            .filter(|(_, d, _)| *d <= 1500.0)
            .max_by(|a, b| (a.2 - a.1 * 0.5).total_cmp(&(b.2 - b.1 * 0.5)).then(b.0.cmp(&a.0)))
            .map(|(id, _, _)| id);
        // Without a local who can pay, an operator from out of town comes
        // only for real trouble.
        if founder.is_none() && circling < need * 3 {
            return false;
        }
        let Some(bid) = self.build_parking(lot_id) else { return false };
        self.economy.parking_street_today = circling.saturating_sub(8);
        let name = self.buildings[bid].name.clone();
        let tick = self.clock.tick;
        match founder {
            Some(f) => {
                let stake = cost.max(self.wealth_of(&self.citizens[f]) * 0.25).min(cost * 2.0);
                // [ledger] The founder's savings into the business.
                if self.business_account(bid).is_some() {
                    self.transfer(self.purse_of(f), Account::Business(bid), stake, Kind::Investment);
                }
                if let Some(z) = self.buildings[bid].business.as_mut() {
                    z.origin = Some(format!("Opened by a local with savings where drivers were circling for a space ({circling} today)."));
                }
                self.set_business_owner(bid, f);
                let who = self.citizens[f].name.clone();
                self.citizens[f].remember(tick, format!("Put my savings into {name}. Everyone round here circles for a space."));
                self.social.push_news(tick, format!("🅿 {who} opened {name} where drivers were circling for a space."), lot_pos, None);
            }
            None => {
                if let Some(z) = self.buildings[bid].business.as_mut() {
                    z.origin = Some(format!("Opened by an operator from out of town: {circling} drivers circled for a space today."));
                }
            }
        }
        self.economy.parking_opened_total += 1;
        true
    }
}

/// [parking-market] A lot at least this full at its busiest is full: its
/// fee rises, and drivers circling near it may draw a new one.
pub const PARKING_FULL: f32 = 0.85;
/// Under this full at its busiest, a lot cuts its fee.
pub const PARKING_SLACK: f32 = 0.4;
/// Drivers circling in a day before someone opens a lot.
pub const PARKING_CIRCLING: u32 = 4;
/// Paid spaces stop growing once they match the town's cars (or this many).
pub const PARKING_MIN_CAP: usize = 60;
/// No new lot this near any other parking business, m.
pub const PARKING_NEIGHBOUR: f32 = 70.0;
/// No new lot this near one opened in the last two days, m.
pub const PARKING_SPACING: f32 = 150.0;
/// What a founder must have to open a surface lot or a car park.
pub const PARKING_LOT_COST: f32 = 300.0;
pub const PARKING_CAR_PARK_COST: f32 = 1200.0;

/// [r1-roads] (K42) How far a plot's corner may lie off its neighbour's edge
/// and still be joined along it, m: plots cut from different parcels of a
/// curved or skewed block meet along edges that are not quite the same
/// line, and the union left a sliver between them (two pieces), so the
/// third of three car parks side by side never joined the other two.
pub const PLOT_JOIN_SLACK: f32 = 1.0;

/// [r1-roads] (K42) The outline of two neighbouring plots joined: the
/// corners of each lying within `PLOT_JOIN_SLACK` of the other's outline are
/// put on it first, so the two meet along one line. None when they do not
/// make one piece.
pub fn join_plots(a: &[Vec2], b: &[Vec2]) -> Option<Vec<Vec2>> {
    let snap = |pts: &[Vec2], onto: &[Vec2]| -> Vec<Vec2> {
        pts.iter()
            .map(|&p| {
                let n = onto.len();
                let mut best = (f32::INFINITY, p);
                for i in 0..n {
                    let (_, q) = crate::geom::project_on_segment(p, onto[i], onto[(i + 1) % n]);
                    let d = q.distance(p);
                    if d < best.0 {
                        best = (d, q);
                    }
                }
                if best.0 <= PLOT_JOIN_SLACK { best.1 } else { p }
            })
            .collect()
    };
    let b2 = snap(b, a);
    let pieces = crate::zoning::union_outlines(&[a.to_vec(), b2.clone()]);
    if pieces.len() != 1 {
        return None;
    }
    crate::zoning::joined_outline(&[a.to_vec(), b2])
}
