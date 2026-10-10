//! [pt-transit] Why people do (not) take the bus or the metro: every
//! working resident's commute, home door to work door, put to the same
//! choice `mode_options` offers a traveller, and counted: which modes are
//! on offer, which is cheapest, and for the bus what ruled it out. For the
//! remote API (`report {section: "ridership"}`) and probes; reads only.

use super::*;
use crate::citizens::TripMode;
use crate::transit::NoBus;

/// Counts over the commutes looked at (see the module notes).
#[derive(Clone, Debug, Default, Serialize)]
pub struct RidershipReport {
    /// Commutes looked at (working residents with a home and a job).
    pub commutes: u32,
    /// Median crow-flight length of a commute, metres.
    pub median_m: f32,
    /// Commutes a bus is offered for (a line serves both ends).
    pub bus_offered: u32,
    /// Why not, for the rest (`NoBus::key`).
    pub no_bus: std::collections::BTreeMap<String, u32>,
    pub metro_offered: u32,
    pub train_offered: u32,
    /// Homes and workplaces within a walk (`STOP_REACH`) of a bus stop, and
    /// within reach of a metro entrance.
    pub homes_near_stop: u32,
    pub jobs_near_stop: u32,
    pub homes: u32,
    pub jobs: u32,
    /// The cheapest option of each commute, by mode (`TripMode::label`),
    /// as the traveller weighs it (before the day's whim).
    pub cheapest: std::collections::BTreeMap<String, u32>,
    /// Of the commutes a bus was offered for, the cheapest.
    pub cheapest_when_bus_offered: std::collections::BTreeMap<String, u32>,
    /// Average weighed cost (minute-equivalents) and minutes of the bus,
    /// the car and walking where each was offered alongside the bus.
    pub bus_cost: f32,
    pub bus_minutes: f32,
    pub car_cost: f32,
    pub car_minutes: f32,
    pub walk_cost: f32,
    pub walk_minutes: f32,
    /// Trips counted yesterday by mode (`Economy::trip_modes`), for
    /// comparison with what the commutes alone would give.
    pub trips_yesterday: std::collections::BTreeMap<String, u32>,
}

impl World {
    /// [pt-transit] The ridership report (see the module notes). The
    /// options are weighed as if the buses and trains were running, so it
    /// reads the same at night. At most `limit` commutes are looked at,
    /// spread over the town.
    pub fn ridership_report(&self, limit: usize) -> RidershipReport {
        let mut r = RidershipReport::default();
        let stops: Vec<Vec2> = self.transit.routes.values().filter(|x| x.runnable()).flat_map(|x| x.stops.iter().map(|s| s.pos)).collect();
        let near_stop = |p: Vec2| stops.iter().any(|s| s.distance(p) <= transit::STOP_REACH);
        let mut homes = std::collections::HashSet::new();
        let mut jobs = std::collections::HashSet::new();
        let working: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.home.is_some() && c.work.is_some()).map(|(id, _)| id).collect();
        let step = (working.len() / limit.max(1)).max(1);
        let mut lengths: Vec<f32> = Vec::new();
        let (mut bus_n, mut car_n, mut walk_n) = (0u32, 0u32, 0u32);
        for &id in working.iter().step_by(step) {
            let c = &self.citizens[id];
            let (Some(home), Some(work)) = (c.home, c.work) else { continue };
            let (Some(from), Some(to)) = (self.building_entrance(home), self.building_entrance(work)) else { continue };
            homes.insert(home);
            jobs.insert(work);
            r.commutes += 1;
            lengths.push(from.distance(to));
            let q = trips::TripQuery { from, to, target: work, purpose: Purpose::Work, allow_transit: true, allow_car: true, allow_bike: true };
            let mut options = self.mode_options(id, &q);
            // As if in service: the bus is planned whatever the hour.
            let bus = self.transit.plan_trip(from, to);
            if !options.iter().any(|o| o.mode == TripMode::Bus) {
                if let Some((route, board, alight)) = bus {
                    let rt = &self.transit.routes[route];
                    let walk = self.config.walk_speed.max(0.1);
                    let secs = self.config.traffic_seconds_per_tick;
                    let wait = (rt.headway(secs) as f32 * 0.5).clamp(transit::MIN_EXPECTED_WAIT, transit::MAX_EXPECTED_WAIT);
                    let minutes = (from.distance(rt.stops[board].pos) + rt.stops[alight].pos.distance(to)) / walk + wait + rt.ride_length(board, alight) / (7.0 * secs);
                    let [sociable, ambition, ..] = crate::vehicle::traits_of(c.seed);
                    let cost = minutes * (0.7 + 0.6 * ambition) + rt.fare * self.money_weight(id) + (6.0 - 6.0 * sociable) + trips::PAIN_WEIGHT * c.commute.pain_of(TripMode::Bus);
                    options.push(trips::ModeOption { mode: TripMode::Bus, minutes, money: rt.fare, comfort: 6.0 - 6.0 * sociable, pain: c.commute.pain_of(TripMode::Bus), cost, plan: trips::ModePlan::Walk });
                }
            }
            match self.transit.why_no_bus(from, to) {
                None => r.bus_offered += 1,
                Some(why) => *r.no_bus.entry(why.key().to_string()).or_default() += 1,
            }
            if options.iter().any(|o| o.mode == TripMode::Subway) || self.plan_subway_trip(from, to).is_some() {
                r.metro_offered += 1;
            }
            if self.plan_rail_trip(from, to).is_some() {
                r.train_offered += 1;
            }
            options.sort_by(|a, b| a.cost.total_cmp(&b.cost));
            if let Some(best) = options.first() {
                *r.cheapest.entry(best.mode.key().to_string()).or_default() += 1;
                if bus.is_some() {
                    *r.cheapest_when_bus_offered.entry(best.mode.key().to_string()).or_default() += 1;
                }
            }
            if bus.is_some() {
                for o in &options {
                    match o.mode {
                        TripMode::Bus => {
                            r.bus_cost += o.cost;
                            r.bus_minutes += o.minutes;
                            bus_n += 1;
                        }
                        TripMode::Car => {
                            r.car_cost += o.cost;
                            r.car_minutes += o.minutes;
                            car_n += 1;
                        }
                        TripMode::Walk => {
                            r.walk_cost += o.cost;
                            r.walk_minutes += o.minutes;
                            walk_n += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        let avg = |x: &mut f32, n: u32| *x = if n > 0 { *x / n as f32 } else { 0.0 };
        avg(&mut r.bus_cost, bus_n);
        avg(&mut r.bus_minutes, bus_n);
        avg(&mut r.car_cost, car_n);
        avg(&mut r.car_minutes, car_n);
        avg(&mut r.walk_cost, walk_n);
        avg(&mut r.walk_minutes, walk_n);
        lengths.sort_by(|a, b| a.total_cmp(b));
        r.median_m = lengths.get(lengths.len() / 2).copied().unwrap_or(0.0);
        r.homes = homes.len() as u32;
        r.jobs = jobs.len() as u32;
        r.homes_near_stop = homes.iter().filter(|&&h| self.building_entrance(h).is_some_and(near_stop)).count() as u32;
        r.jobs_near_stop = jobs.iter().filter(|&&j| self.building_entrance(j).is_some_and(near_stop)).count() as u32;
        for m in TripMode::ALL {
            let n = self.vehicles.stats.trips_yesterday.get(m.index()).copied().unwrap_or(0);
            r.trips_yesterday.insert(m.key().to_string(), n);
        }
        let _ = NoBus::ALL;
        r
    }
}
