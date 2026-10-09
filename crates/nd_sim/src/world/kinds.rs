//! [review] What kind of building this is, as the systems ask it: named
//! predicates for the checks that were written out inline, each exactly
//! as its callers wrote it (the differences between them are deliberate
//! or historical; see `docs/review/nd_sim_world.md` §2.1). The broad ones
//! live on `Building` itself (`is_city_structure`, `is_special`).

use crate::buildings::Building;
use crate::zoning::LandUse;

impl Building {
    /// An ordinary grown building: none of the city's own structures, no
    /// campus annex, no car park.
    pub fn is_grown(&self) -> bool {
        !self.is_special() && !self.is_parking()
    }

    /// A bus depot that runs buses: built, open and not derelict.
    pub fn working_depot(&self) -> bool {
        self.depot && !self.is_derelict() && self.operating()
    }

    /// Counted in the floors round a lot that set its height cap
    /// (`World::height_cap`): no park, car park, plant or battery, depot,
    /// rail facility or service. (Docks, metro entrances and cargo hubs
    /// count, as they always have.)
    pub fn counts_for_height_cap(&self) -> bool {
        self.land_use != LandUse::Park && !self.is_parking() && !self.is_power() && !self.depot && self.rail.is_none() && self.service.is_none()
    }
}
