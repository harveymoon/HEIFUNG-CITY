//! The tick: the midnight work (`daily`), the staggered periodic jobs
//! (`periodic`), then every system's step in a fixed order; and the traffic
//! step with its arrivals handed to the systems that own them.

use super::*;

impl World {
    pub fn tick(&mut self, minds: &mut dyn MindProvider) {
        // [park-paths] A path whose park went since the last tick is cut back.
        self.settle_path_links();
        // [under-construction] Park walkways and gates shut while their
        // building is a site.
        self.refresh_closed_grounds();
        self.clock.advance();
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            self.daily();
        }
        self.periodic(tick);
        // [pt-walk] Stranded areas, when the networks changed (and hourly).
        self.step_connectivity();
        // [megacorp] Students to class and out at night.
        self.step_campus();
        // [parking] Drivers worn out by the hunt and stuck in a queue leave
        // the car where it stands.
        if tick % 5 == 2 {
            self.hunters_give_up();
        }
        // [parking] Hunters turn into a lot with room they are passing.
        if tick % 2 == 1 {
            self.hunters_spot_lots();
        }
        // Answers a mind worked out in the background (an LLM's) land
        // whenever they are ready. Someone mid-journey (driving, on a bus or
        // train, waiting at the stop) keeps going: only the thought and the
        // memory are taken now, and the next decision asks again (REVIEW2
        // M4: acting on it stranded cars and bus riders).
        for (id, decision) in minds.poll() {
            if self.citizens.contains_key(id) {
                self.apply_late_decision(id, decision);
            }
        }
        // Late answers nobody came back for (they are still out, or gone).
        if tick % 60 == 0 {
            self.deferred_intents.retain(|_, (_, at)| tick.saturating_sub(*at) <= trips::DEFERRED_INTENT_TTL);
        }
        self.step_traffic();
        // [road-inspect] The vehicles that drove onto each piece, counted.
        self.log_road_traffic();
        // [land] Billionaires' convoys form and follow.
        self.step_convoys();
        self.step_air();
        self.step_taxis();
        // [ped-highway] Nobody stands about on a highway or ramp.
        if tick % kerbside::STRANDED_SWEEP_EVERY == 7 {
            self.rescue_stranded();
            // [walk-crossings] Nor in a street's lanes.
            self.clear_carriageways();
        }
        self.step_driving();
        self.step_journeys();
        // [events] Street races and warehouse raves.
        self.step_nights();
        if tick % crate::road_wear::SAMPLE_EVERY == 3 {
            self.sample_road_load();
        }
        self.step_enforcement();
        self.step_services();
        // [society] Trash trucks.
        self.step_society();
        self.step_through_traffic();
        self.step_transit();
        self.step_rail();
        self.step_subway();
        self.step_ships();
        // [economy] marina boats out on the water
        self.step_boats();
        // [fishing] trawlers out to the grounds and home
        self.step_fishing();
        // [ferries] Day trips out, and anyone stranded abroad.
        self.step_ferry_people();
        self.step_cargo_hubs();
        // [delivery-hours] Lorries due out now.
        self.step_freight_queue();
        // [games] Away fans driving in to a game, and home after.
        self.step_games();
        // [airlift] Stuck vehicles lifted off the road.
        self.step_airlifts();
        // [watchdog] No service vehicle stands for long on a carriageway.
        self.step_watchdog();
        if self.clock.minute_of_day() == 0 {
            self.bots.delivered_today = 0;
            self.bots.van_orders_today = 0;
        }
        if tick % crate::bots::ROUND_TICKS == 11 {
            crate::prof!("dispatch_deliveries", self.dispatch_deliveries());
        }
        crate::prof!("step_bots", self.step_bots());
        // [tourism] Arrivals on the hour, visitors' days, departures.
        self.step_tourism();
        self.step_citizens(minds);
        // [passengers] With their drivers, in or out of the car.
        self.step_passengers();
        // [social] Who gets talking, and friends calling round.
        self.step_social();
        // [concerns] Happiness on the hour and at midnight.
        crate::prof!("step_concerns", self.step_concerns());
        // Every system that moves people has run: lay the breadcrumbs.
        for c in self.citizens.values_mut() {
            c.push_trail();
            // [pt-walk] An "o" on the trail wherever they stopped.
            if c.inside.is_some() || matches!(c.activity, Activity::Sitting { .. } | Activity::Waiting { .. }) {
                c.mark_stop();
            }
        }
    }

    /// Midnight: the day's work, in order. The books close
    /// (`close_day_record`) after the systems that settle into them and
    /// before the estate, towers and corps that read them.
    fn daily(&mut self) {
        self.events.push(SimEvent::NewDay(self.clock.day()));
        // The day's counters, read before anything below resets them.
        let record = self.open_day_record();
        // Services settle their upkeep before the books close.
        crate::prof!("daily_services", self.daily_services());
        // [subway] Upkeep before the books close.
        crate::prof!("daily_subway", self.daily_subway());
        // [events] Roads wear and the crews resurface, before the books close.
        crate::prof!("daily_roads", self.daily_roads());
        // [road-inspect] Each piece's wear a day and its repairs, read off.
        self.note_road_day();
        // [society] crime, the street and the bins settle their day.
        self.daily_society();
        // [clarity] Dangerous streets and far hospitals, complained of.
        self.daily_dangerous_streets();
        self.daily_healthcare();
        self.daily_update();
        // [economy] the city's needs read and met, before the vehicles' day rolls
        crate::prof!("daily_pressure", self.daily_pressure());
        self.daily_vehicles();
        self.industry_daily();
        self.daily_places();
        // [social] ties fade, friends help, groups form and make things
        crate::prof!("daily_social", self.daily_social());
        self.rides.stats.roll();
        // [benches] seats and bench-neighbour tallies
        self.daily_benches();
        // [districts] facelifts; [tourism] hotel nights and the books.
        crate::prof!("daily_districts", self.daily_districts());
        crate::prof!("daily_tourism", self.daily_tourism());
        // [waterfront] beach tallies, and the households near a good one
        self.daily_shore();
        // [parks] litter, campers, dealers: how kept each park is
        self.daily_parks();
        crate::prof!("close_day_record", self.close_day_record(record));
        // [land] Buyouts, branches, merges, McMansions, billionaires.
        self.daily_estate();
        // [supertall] Tower developers' money, and maybe a new project.
        crate::prof!("daily_towers", self.daily_towers());
        // [megacorp] Mega-corps and campuses grow.
        crate::prof!("daily_corps", self.daily_corps());
    }

    /// The periodic jobs by minute of the hour (or of two hours), staggered
    /// so no tick carries more than one heavy job.
    fn periodic(&mut self, tick: u64) {
    // Periodic work is staggered so no tick carries more than one heavy
    // job: the hour starts at :00, sites advance at :30, land value runs
    // in strips from :20, development and immigration sit between.
    if tick % 60 == 0 {
        crate::prof!("hourly_economy", self.hourly_economy());
        crate::prof!("industry_hourly", self.industry_hourly());
        // [dirty-industry] Clean-up crews dig on.
        self.hourly_cleanups();
        // [economy] repair shops, bike shops, galleries and boats
        crate::prof!("trades_hourly", self.trades_hourly());
        crate::prof!("record_hour", self.record_hour());
        // [toll-inspect] each booth's hour on its chart
        self.hourly_tolls();
    }
    // [pt-growth] A sixth of the sites every ten minutes (each its own slot).
    if tick % 10 == 0 {
        crate::prof!("step_construction", self.step_construction());
    }
    // [waterfront] beaches (access, weather, crowds), private jetties
    if tick % 60 == 33 {
        crate::prof!("hourly_shore", self.hourly_shore());
    }
    // [parks] the park list (one per park of lots side by side) and who is in each
    if tick % 60 == 37 {
        crate::prof!("hourly_parks", self.hourly_parks());
    }
    // [land] People present and moving, at :13 clear of the rest.
    if tick % 60 == 13 {
        crate::prof!("hourly_pressure", self.hourly_pressure());
    }
    let lv = tick % 120;
    if (LAND_VALUE_PHASE..LAND_VALUE_PHASE + LAND_VALUE_STRIPS).contains(&lv) {
        crate::prof!("update_land_value_strip", self.update_land_value_strip((lv - LAND_VALUE_PHASE) as u8));
    }
    if tick % 10 == 7 {
        crate::prof!("update_economy", self.update_economy());
    }
    if (tick + 5) % self.config.development_interval == 0 {
        crate::prof!("develop", self.develop());
    }
    if (tick + 15) % self.config.immigration_interval == 0 {
        crate::prof!("immigrate", self.immigrate());
    }
    if tick % 120 == 90 {
        crate::prof!("job_search", self.job_search());
    }
    // Fires, crimes and illness roll hourly at :50, clear of the rest.
    if tick % 60 == 50 {
        crate::prof!("hourly_services", self.hourly_services());
    }
    // [society] Bins, camps and crime at :40.
    if tick % 60 == 40 {
        crate::prof!("hourly_society", self.hourly_society());
    }
    if tick % 60 == 45 {
        crate::prof!("hourly_vehicles", self.hourly_vehicles());
    }
    // [social] Outings, group nights, gigs and protests at :25.
    if tick % 60 == 25 {
        self.hourly_social();
    }
    }

    /// A vehicle no system claimed reached the end of its route. Wreck tow
    /// trucks are home at their yard (or off the map), takeover outsiders
    /// and through cars from older saves are at their exit: they leave the
    /// world. Anything else is not deleted where it stands (that used to
    /// make vehicles vanish mid-street): it drives off the map
    /// (`retire_vehicle`).
    pub(super) fn unclaimed_arrival(&mut self, car_id: CarId) {
        let Some(car) = self.traffic.cars.get(car_id) else { return };
        let gang = car.drive.gang.is_some();
        let tow = self.driving.tows.iter().find(|t| t.truck == car_id).map(|t| t.yard);
        let home = match tow {
            Some(Some(yard)) => self.buildings.contains_key(yard),
            Some(None) => true,
            None => false,
        };
        if home || gang || self.at_map_edge(car_id) {
            self.traffic.remove_car(car_id);
        } else {
            self.retire_vehicle(car_id);
        }
    }

    pub(super) fn step_traffic(&mut self) {
        // [fire-scene] What stands at the roadside for the traffic to pass.
        self.set_traffic_scenes();
        // [rail-crossing] Lights and barriers for the trains coming.
        self.step_level_crossings();
        let arrived = self.traffic.step(self.config.traffic_seconds_per_tick, self.config.traffic_substeps);
        // [tolls] Before anyone arriving is taken off the road.
        self.settle_tolls();
        for car_id in arrived {
            // [emergency] A fugitive got where it was running: on it runs.
            if self.chase_car_arrived(car_id) {
                continue;
            }
            // [land] A convoy's escort or police car at the end of its run.
            if self.convoy_car_arrived(car_id) {
                continue;
            }
            // Fire engines, police cars and ambulances: services handle them.
            if self.services.unit_of_car(car_id).is_some() {
                self.unit_car_arrived(car_id);
                continue;
            }
            if self.traffic.cars.get(car_id).is_some_and(|c| c.bus.is_some()) {
                self.bus_arrived(car_id);
                continue;
            }
            // [society] Garbage trucks.
            if self.society_car_arrived(car_id) {
                continue;
            }
            let Some(car) = self.traffic.cars.get(car_id) else { continue };
            // Lorries deliver and head back to a truck lot, their business
            // or out of town (`world/industry`).
            if let Some(_) = car.cargo {
                self.lorry_arrived(car_id);
                continue;
            }
            // [carlife] Private cars park (or go on looking), trucks and
            // vans head back to base, cars sold or emigrating leave.
            // [games] An away fan's car at the ground.
            if self.fan_car_arrived(car_id) {
                continue;
            }
            if self.vehicle_arrived(car_id) {
                continue;
            }
            self.unclaimed_arrival(car_id);
        }
        // Keep citizen positions in sync with their cars for inspection.
        for (cid, car) in self.traffic.cars.iter() {
            // [carlife] Only someone actually in it (not a driver who got
            // out while it was taken elsewhere).
            if let Some(c) = self.citizens.get_mut(car.citizen).filter(|c| matches!(c.activity, Activity::Driving { car, .. } if car == cid)) {
                c.pos = car.pos;
                c.heading = car.heading;
            }
            if let Some(bus) = &car.bus {
                for &p in &bus.passengers {
                    if let Some(c) = self.citizens.get_mut(p) {
                        c.pos = car.pos;
                        c.prev_pos = car.pos;
                        c.heading = car.heading;
                    }
                }
            }
        }
    }
}
