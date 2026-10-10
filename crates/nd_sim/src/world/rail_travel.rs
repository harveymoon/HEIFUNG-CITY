//! [pt-transit] The railway out of town. Passenger trains come in over the
//! edge of the map and leave by it, so even a town with a single station is
//! on a line to somewhere, as a ferry terminal is: with one station there
//! was no trip within the town to take and the station never saw a rider.
//! When a train calls at a station that is on the streets:
//!
//! - visitors who chose to come by rail (`world/tourism.rs` books them
//!   `Aboard` the next train in) step off and go to their hotel;
//! - on the morning trains, commuters from out of town come for a shift at
//!   businesses near the station that are short of staff (as off the
//!   ferries: `commuters_arriving`), and take a train home after it;
//! - residents back from a day out in the city get off and go home.
//!
//! Residents living near a station sometimes take the train out for the
//! day (`plan_rail_day_trips`), paying the fare both ways. Every one of
//! them is counted in the station's riders and fares.

use super::*;
use crate::ports::DayTrip;
use crate::tourism::{Arrival, TouristId, TouristState};

/// Salt for the draws here.
const SALT: u64 = 0x7A11_0C17_u64;
/// How far from a station residents set off on a day trip by train, and
/// commuters off the train walk to a job.
pub const RAIL_CATCHMENT: f32 = 900.0;
/// Day trips out by train per station per day, at most.
pub const MAX_RAIL_DAY_TRIPS: u32 = 16;
/// The cities down the line, by station.
const RAIL_TOWNS: [&str; 6] = ["Calder Junction", "Port Meridian", "Halvard", "New Ashby", "Kessel Reach", "Orrin Vale"];

impl World {
    /// The city a station's trains run to.
    pub fn rail_town_name(&self, station: BuildingId) -> String {
        let k = slotmap::Key::data(&station).as_ffi();
        RAIL_TOWNS[(k as usize ^ (k >> 32) as usize) % RAIL_TOWNS.len()].to_string()
    }

    /// A passenger train stands at `station` (one on the streets): the
    /// visitors, commuters and day trippers it brings from out of town get
    /// off.
    pub(super) fn rail_arrivals(&mut self, station: BuildingId) {
        let tick = self.clock.tick;
        let aboard: Vec<TouristId> = self.tourism.tourists.iter().filter(|(_, t)| t.via == Arrival::Rail && matches!(t.state, TouristState::Aboard { terminal, .. } if terminal == station)).map(|(id, _)| id).collect();
        for id in aboard {
            self.rail_disembark(id, station);
        }
        self.commuters_arriving(station, Arrival::Rail, RAIL_CATCHMENT);
        let back: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Abroad { terminal, until, .. } if terminal == station && until <= tick)).map(|(id, _)| id).collect();
        for id in back {
            self.home_from_day_trip(id);
        }
    }

    /// A visitor or commuter off the train at `station`: the fare in (paid
    /// at the far end, to the city's railway), then to the hotel or the job.
    pub(super) fn rail_disembark(&mut self, id: TouristId, station: BuildingId) {
        let at = self.building_entrance(station);
        let Some(t) = self.tourism.tourists.get_mut(id) else { return };
        if let Some(p) = at {
            t.pos = p;
            t.prev_pos = p;
        }
        let (commuter, hotel) = (t.commuter, t.hotel);
        self.economy.credit_item(Flow::RailFares, rail::RAIL_FARE, Item::Building(station));
        let s = &mut self.rail.stats;
        s.riders_today += 1;
        s.fares_today += rail::RAIL_FARE;
        s.from_out_of_town_today += 1;
        match commuter {
            Some(work) => match self.building_entrance(work) {
                Some(to) => self.tourist_leg(id, crate::tourism::Goal::Work(work), to),
                None => self.tourist_depart(id),
            },
            None => {
                let here = self.tourism.tourists[id].pos;
                let to = self.building_entrance(hotel).unwrap_or(here);
                self.tourist_leg(id, crate::tourism::Goal::Hotel, to);
            }
        }
    }

    /// A visitor or commuter at the station to go home: on the next train
    /// out, fare paid.
    pub(super) fn rail_departure(&mut self, station: BuildingId) {
        self.economy.credit_item(Flow::RailFares, rail::RAIL_FARE, Item::Building(station));
        let s = &mut self.rail.stats;
        s.riders_today += 1;
        s.fares_today += rail::RAIL_FARE;
        s.to_out_of_town_today += 1;
    }

    /// Every hour, late morning: residents near a station may take the
    /// train out for the day.
    pub(super) fn plan_rail_day_trips(&mut self) {
        let tick = self.clock.tick;
        if tick % 60 != 23 || !(9..13).contains(&(self.clock.hour() as u8)) || !Rail::in_service(self.clock.minute_of_day() as u32) {
            return;
        }
        let hour = self.clock.hour() as u8;
        let stations: Vec<BuildingId> = self.stations.clone();
        for station in stations {
            let Some(at) = self.building_entrance(station) else { continue };
            let mut left = MAX_RAIL_DAY_TRIPS.saturating_sub(self.ports.trips_today.get(&station).copied().unwrap_or(0));
            let candidates: Vec<CitizenId> = self
                .citizens
                .iter()
                .filter(|(_, c)| {
                    matches!(c.activity, Activity::AtHome)
                        && c.inside.is_some()
                        && c.inside == c.home
                        && !c.asleep
                        && c.wakefulness > 0.5
                        && c.stage() != crate::citizens::LifeStage::Child
                        && c.savings >= 2.0 * rail::RAIL_FARE + 15.0
                        && c.home.and_then(|h| self.building_entrance(h)).is_some_and(|p| p.distance(at) <= RAIL_CATCHMENT)
                        && !c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| (hour..20).any(|h| b.shift_for(c.seed).contains(h)))
                })
                .map(|(id, _)| id)
                .filter(|id| !self.services.in_custody(*id) && !self.is_patient(*id))
                .collect();
            for id in candidates {
                if left == 0 {
                    break;
                }
                let c = &self.citizens[id];
                let curiosity = crate::vehicle::traits_of(c.seed)[4];
                let mut rng = crate::rng_for(c.seed ^ tick.wrapping_mul(0x7A11_5EED), SALT);
                if rng.random::<f32>() >= 0.006 * (0.4 + curiosity) {
                    continue;
                }
                let lu = self.buildings.get(station).map_or(LandUse::Civic, |b| b.land_use);
                self.travel_to(id, Some(station), Purpose::Visit(lu));
                let going = self.citizens.get(id).is_some_and(|c| c.activity.is_traveling() || c.to_car.is_some());
                if going {
                    let name = self.rail_town_name(station);
                    self.citizens[id].thought = format!("Taking the train to {name} for the day.");
                    self.ports.trippers.push(DayTrip { citizen: id, terminal: station, town: 0, since: tick });
                    *self.ports.trips_today.entry(station).or_insert(0) += 1;
                    left -= 1;
                }
            }
        }
    }

    /// At the station for a day out: pay the fare there and back and take
    /// the next train out; `Abroad` until an evening train brings them back.
    pub(super) fn board_rail_trip(&mut self, id: CitizenId, b: BuildingId, trip: DayTrip) -> bool {
        if !self.stations.contains(&b) {
            return false;
        }
        let tick = self.clock.tick;
        let fare = 2.0 * rail::RAIL_FARE;
        self.economy.credit_item(Flow::RailFares, fare, Item::Building(b));
        let s = &mut self.rail.stats;
        s.riders_today += 1;
        s.fares_today += fare;
        s.to_out_of_town_today += 1;
        self.settle_footfall(id);
        let name = self.rail_town_name(b);
        let mut rng = crate::rng_for(self.citizens[id].seed ^ tick, SALT ^ 0xB0A7);
        let back = tick - tick % 1440 + 17 * 60 + rng.random_range(0..150u64);
        let c = &mut self.citizens[id];
        c.savings -= fare;
        c.inside = None;
        c.activity = Activity::Abroad { terminal: b, town: trip.town, until: back.max(tick + 180) };
        c.thought = format!("On the train to {name}.");
        c.remember(tick, format!("Took the train to {name} for the day."));
        true
    }
}
