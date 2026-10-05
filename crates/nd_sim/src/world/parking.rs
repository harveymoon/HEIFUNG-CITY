//! Parking: car parks, finding a space on arrival and releasing it.

use super::*;

impl World {
    /// A driver has pulled up at their destination: use the building's own
    /// spaces, else pay at the nearest parking lot with room, else leave the
    /// car on the street and note the shortage.
    pub(super) fn park_car(&mut self, id: CitizenId) {
        let dest = match &self.citizens[id].activity {
            Activity::Driving { to: Some(b), .. } => *b,
            _ => return,
        };
        let tick = self.clock.tick;
        if let Some(b) = self.buildings.get_mut(dest) {
            if b.parking_free() > 0 {
                b.parking_used += 1;
                self.citizens[id].parked_at = Some(dest);
                return;
            }
        }
        let Some(center) = self.building_lot(dest).map(|l| l.center) else { return };
        let best = self
            .buildings
            .iter()
            .filter(|(_, b)| b.is_parking() && b.parking_free() > 0 && !b.business.as_ref().map_or(true, |z| z.closed))
            .filter_map(|(bid, b)| self.lots.get(b.lot).map(|l| (bid, l.center.distance(center))))
            .filter(|(_, d)| *d < 180.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(bid, _)| bid);
        match best {
            Some(bid) => {
                let (fee, name, step_before) = {
                    let b = &mut self.buildings[bid];
                    let before = b.parking_used * 8 / b.parking_capacity.max(1);
                    b.parking_used += 1;
                    (b.parking_price, b.name.clone(), before)
                };
                let step_after = {
                    let b = &self.buildings[bid];
                    b.parking_used * 8 / b.parking_capacity.max(1)
                };
                if step_after != step_before {
                    self.events.push(SimEvent::BuildingChanged(bid));
                }
                if let Some(bz) = self.buildings[bid].business.as_mut() {
                    bz.money += fee;
                    bz.revenue_today += fee;
                    bz.deals += 1;
                }
                self.economy.parking_fees_today += fee;
                let c = &mut self.citizens[id];
                c.savings -= fee;
                c.parked_at = Some(bid);
                if self.rng.random_bool(0.05) {
                    c.remember(tick, format!("Paid {fee:.0} to park at {name}."));
                }
            }
            None => {
                self.economy.parking_street_today += 1;
                self.economy.parking_shortage_at = Some(center);
                let c = &mut self.citizens[id];
                c.mood = (c.mood - 0.02).max(0.0);
                if self.rng.random_bool(0.08) {
                    c.remember(tick, "Circled for ages. Nowhere to park.");
                }
            }
        }
    }

    /// A driver whose destination lines a footpath leaves the car at the
    /// street and walks the rest. Returns false when there is nothing to walk.
    pub(super) fn walk_last_leg(&mut self, id: CitizenId) -> bool {
        let (to, purpose) = match &self.citizens[id].activity {
            Activity::Driving { to: Some(b), purpose, .. } => (*b, *purpose),
            _ => return false,
        };
        let Some(lot) = self.building_lot(to) else { return false };
        if lot.car_access() {
            return false;
        }
        let entrance = lot.entrance();
        let here = self.citizens[id].pos;
        let mut path = match self.walk.route(here, entrance) {
            Some((pts, _)) => pts,
            None => vec![here],
        };
        path.push(entrance);
        let c = &mut self.citizens[id];
        c.mode = TravelMode::Walk;
        c.activity = Activity::Traveling { to: Some(to), purpose, path, idx: 0 };
        true
    }

    pub(super) fn release_parking(&mut self, id: CitizenId) {
        let Some(bid) = self.citizens[id].parked_at.take() else { return };
        let Some(b) = self.buildings.get_mut(bid) else { return };
        let before = b.parking_used * 8 / b.parking_capacity.max(1);
        b.parking_used = b.parking_used.saturating_sub(1);
        let after = b.parking_used * 8 / b.parking_capacity.max(1);
        if b.is_parking() && before != after {
            self.events.push(SimEvent::BuildingChanged(bid));
        }
    }

    /// Open a parking business on a vacant lot: a surface lot where land is
    /// cheap, a multi-storey car park in the middle. The cheapest business
    /// there is: two to four jobs and a little float.
    pub fn build_parking(&mut self, lot_id: LotId) -> Option<BuildingId> {
        let lot = self.lots.get(lot_id)?;
        if lot.building.is_some() || lot.landlocked || self.under_facility(lot.center) {
            return None;
        }
        let area = lot.area();
        let value = self.land_value.at(lot.center);
        let tier = Tier::for_land_value(value).min(Tier::Mid);
        let floors: u8 = if tier == Tier::Mid && area > 500.0 { self.rng.random_range(3..=5) } else { 0 };
        let capacity = if floors == 0 { (area / 28.0) as u16 } else { ((area / 32.0) as u16) * floors as u16 };
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
            b.parking_price = 3.0 * tier.wage_multiplier() * (0.6 + value);
        }))
    }
}
