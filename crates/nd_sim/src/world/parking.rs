//! Parking businesses. Where cars park and how drivers look for a space
//! is the car lifecycle's (`world/cars.rs`): a car keeps its space until it
//! drives off, so a lot's occupancy is the cars standing in it.

use super::*;

impl World {
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
