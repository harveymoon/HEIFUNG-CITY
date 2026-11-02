//! The tick: the midnight work (`daily`), the staggered periodic jobs
//! (`periodic`), then every system's step in a fixed order; and the traffic
//! step with its arrivals handed to the systems that own them.

use super::*;

impl World {
    pub fn tick(&mut self, minds: &mut dyn MindProvider) {
        // [park-paths] A path whose park went since the last tick is cut back.
        crate::prof!("settle_path_links", self.settle_path_links());
        // [under-construction] Park walkways and gates shut while their
        // building is a site.
        crate::prof!("refresh_closed_grounds", self.refresh_closed_grounds());
        self.clock.advance();
        let tick = self.clock.tick;
        // [ledger] Every move this tick is stamped with it.
        self.economy.bank.now = tick;
        if self.clock.minute_of_day() == 0 {
            self.daily();
        }
        self.periodic(tick);
        // [pt-walk] Stranded areas, when the networks changed (and hourly).
        crate::prof!("step_connectivity", self.step_connectivity());
        // [megacorp] Students to class and out at night.
        crate::prof!("step_campus", self.step_campus());
        // [parking] Drivers worn out by the hunt and stuck in a queue leave
        // the car where it stands.
        if tick % 5 == 2 {
            crate::prof!("hunters_give_up", self.hunters_give_up());
        }
        // [parking] Hunters turn into a lot with room they are passing.
        if tick % 2 == 1 {
            crate::prof!("hunters_spot_lots", self.hunters_spot_lots());
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
        crate::prof!("step_traffic", self.step_traffic());
        // [road-inspect] The vehicles that drove onto each piece, counted.
        crate::prof!("log_road_traffic", self.log_road_traffic());
        // [land] Billionaires' convoys form and follow.
        crate::prof!("step_convoys", self.step_convoys());
        crate::prof!("step_air", self.step_air());
        crate::prof!("step_airports", self.step_airports());
        crate::prof!("step_taxis", self.step_taxis());
        // [ped-highway] Nobody stands about on a highway or ramp.
        if tick % kerbside::STRANDED_SWEEP_EVERY == 7 {
            crate::prof!("rescue_stranded", self.rescue_stranded());
            // [walk-crossings] Nor in a street's lanes.
            crate::prof!("clear_carriageways", self.clear_carriageways());
        }
        crate::prof!("step_driving", self.step_driving());
        crate::prof!("step_journeys", self.step_journeys());
        // [events] Street races and warehouse raves.
        crate::prof!("step_nights", self.step_nights());
        if tick % crate::road_wear::SAMPLE_EVERY == 3 {
            crate::prof!("sample_road_load", self.sample_road_load());
        }
        crate::prof!("step_enforcement", self.step_enforcement());
        crate::prof!("step_services", self.step_services());
        // [society] Trash trucks.
        crate::prof!("step_society", self.step_society());
        crate::prof!("step_through_traffic", self.step_through_traffic());
        crate::prof!("step_transit", self.step_transit());
        crate::prof!("step_rail", self.step_rail());
        crate::prof!("step_subway", self.step_subway());
        // [cable] Gondolas and aerial tramways.
        crate::prof!("step_cable", self.step_cable());
        crate::prof!("step_ships", self.step_ships());
        // [economy] marina boats out on the water
        crate::prof!("step_boats", self.step_boats());
        // [fishing] trawlers out to the grounds and home
        crate::prof!("step_fishing", self.step_fishing());
        // [ferries] Day trips out, and anyone stranded abroad.
        crate::prof!("step_ferry_people", self.step_ferry_people());
        // [ferry-islands] The last boat gone, and riders left aboard.
        crate::prof!("step_ferry_piers", self.step_ferry_piers());
        crate::prof!("step_cargo_hubs", self.step_cargo_hubs());
        // [delivery-hours] Lorries due out now.
        crate::prof!("step_freight_queue", self.step_freight_queue());
        // [r8] Container lorries in the depots' bays, the carriers' moves.
        crate::prof!("step_box_yards", self.step_box_yards());
        // [games] Away fans driving in to a game, and home after.
        crate::prof!("step_games", self.step_games());
        // [airlift] Stuck vehicles lifted off the road.
        crate::prof!("step_airlifts", self.step_airlifts());
        // [watchdog] No service vehicle stands for long on a carriageway.
        crate::prof!("step_watchdog", self.step_watchdog());
        if self.clock.minute_of_day() == 0 {
            self.bots.delivered_today = 0;
            self.bots.van_orders_today = 0;
        }
        if tick % crate::bots::ROUND_TICKS == 11 {
            crate::prof!("dispatch_deliveries", self.dispatch_deliveries());
        }
        crate::prof!("step_bots", self.step_bots());
        // [tourism] Arrivals on the hour, visitors' days, departures.
        crate::prof!("step_tourism", self.step_tourism());
        crate::prof!("step_citizens", self.step_citizens(minds));
        // [passengers] With their drivers, in or out of the car.
        crate::prof!("step_passengers", self.step_passengers());
        // [social] Who gets talking, and friends calling round.
        crate::prof!("step_social", self.step_social());
        // [concerns] Happiness on the hour and at midnight.
        crate::prof!("step_concerns", self.step_concerns());
        // [insight] Everything has moved: the history takes it in.
        crate::prof!("step_insight", self.step_insight());
        // Every system that moves people has run: lay the breadcrumbs.
        for c in self.citizens.values_mut() {
            c.push_trail();
            // [pt-walk] An "o" on the trail wherever they stopped.
            if c.inside.is_some() || matches!(c.activity, Activity::Sitting { .. } | Activity::Waiting { .. }) {
                c.mark_stop();
            }
        }
        // [ledger] The debug check: every account against its mirror.
        if self.economy.bank.check {
            self.check_ledger_tick();
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
        // [cable] Stations, rope and cabins, before the books close.
        crate::prof!("daily_cable", self.daily_cable());
        // [r4-timetable] Extra runs paid for, then the timelines roll.
        crate::prof!("daily_timetables", self.daily_timetables());
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
        // [K34] The scrap yards cut up, the box owners' round.
        crate::prof!("daily_containers", self.daily_containers());
        self.industry_daily();
        self.daily_places();
        // [social] ties fade, friends help, groups form and make things
        crate::prof!("daily_social", self.daily_social());
        self.rides.stats.roll();
        // [benches] seats and bench-neighbour tallies
        self.daily_benches();
        // [r5-layered] bays under the decks
        self.daily_undercroft();
        // [districts] facelifts; [tourism] hotel nights and the books.
        crate::prof!("daily_districts", self.daily_districts());
        // [stories] late licences, noise, heritage grants
        self.daily_stories();
        crate::prof!("daily_tourism", self.daily_tourism());
        crate::prof!("daily_airports", self.daily_airports());
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
        // [megablocks] Their arcologies: tallies, jobs inside, new projects.
        crate::prof!("daily_megablocks", self.daily_megablocks());
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
        // [tenants] tenants stand as their buildings do
        self.sync_tenants();
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
    // [K34] Shipping containers: lorries and trains gone, stack developers.
    if tick % 60 == 17 {
        crate::prof!("hourly_containers", self.hourly_containers());
        // [r8] The forwarders send their lorries round the firms.
        crate::prof!("forwarder_rounds", self.forwarder_rounds());
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
    // [r5-layered] Bays under the decks: found again, camps, stallholders.
    if tick % 60 == 29 {
        crate::prof!("hourly_undercroft", self.hourly_undercroft());
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
            // [airport] Passengers' cars and cabs to and from the terminal.
            if self.airport_car_arrived(car_id) {
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
