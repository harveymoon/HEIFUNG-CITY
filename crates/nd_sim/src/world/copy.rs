//! [r16-speed] The sim thread's copy of the world, in parallel.
//!
//! The game's sim thread copies the whole city after every batch of ticks
//! and on every rebase (`nd_app::sim_thread`): ~100 ms at 17,000 people and
//! ~290 ms at 38,000, on one core, which capped the top speed. The copy is
//! the same exact clone as before (`versioned::exact_clone`: versions and
//! caches kept), with the biggest parts cloned on three scoped threads
//! beside the rest.
//!
//! Every field is listed (no `..`), so a field added to `World` and not
//! here fails to compile instead of being left out of the copy.

use super::World;
use crate::versioned::exact_clone;
use std::time::Instant;

impl World {
    /// An exact copy (as `exact_clone(|| world.clone())`), made on four
    /// threads. Falls back to the plain clone if a thread cannot start.
    pub fn exact_copy(&self) -> World {
        let World { version, config, clock, rng, master, project, parked_projects, next_project, plan_warnings, implement_refused, parcels, parcel_cut, planned, parcel_lots, next_gesture, plan_version, plan_cache, wages_paid, town_shops, passers, roads, zones, lots, buildings, citizens, households, traffic, walk, weather, transit, rail, level_crossings, facilities, economy, services, land_value, land_value_work, events, next_seed, road_component, drive_component, connectivity, walk_networks, deferred_intents, stations, yards, halts, seller_index, seller_index_hour, available, groceries_for_sale, meals_for_sale, gyms_open, landscape, ports, fishing, road_z, road_inner, relief, ground, bridge_metres, bots, driving, journeys, peds, waiting_cache, cable_waiting, crowds_cache, seats, society, vehicles, enforcement, air, contamination, foot_wear, lot_memory, land_tile_grounds, cleanups, industry_bias, commercial_bias, places, concerns, districts, tourism, subway, cable, tolls, taxis, cargo_hubs, airports, rides, streets, pillars, pillar_gaps, undercroft, crosswalks, estate, skyline, pressure, scene, identity, street_events, road_wear, dirty, road_log, social, social_concern, committee, shore, path_links, corps, megablocks, freight_queue, inbound_unanswered, food_need, food_trucks, games, airlifts, attractions, dams, parks, insight, travel, containers, talks, builders, goods, builder_sites } = self;
        std::thread::scope(|s| {
            let spawn = |name: &str| std::thread::Builder::new().name(name.into()).stack_size(64 << 20);
            let a = spawn("nd-copy-a").spawn_scoped(s, || exact_clone(|| (citizens.clone(),)));
            let b = spawn("nd-copy-b").spawn_scoped(s, || exact_clone(|| (insight.clone(), travel.clone(), households.clone(), ground.clone(), lots.clone(), zones.clone())));
            let c = spawn("nd-copy-c").spawn_scoped(s, || exact_clone(|| (walk.clone(), traffic.clone(), economy.clone(), buildings.clone(), parcels.clone(), goods.clone())));
            let here = exact_clone(|| (version.clone(), config.clone(), clock.clone(), rng.clone(), master.clone(), project.clone(), parked_projects.clone(), next_project.clone(), plan_warnings.clone(), implement_refused.clone(), parcel_cut.clone(), planned.clone(), parcel_lots.clone(), next_gesture.clone(), plan_version.clone(), plan_cache.clone(), wages_paid.clone(), town_shops.clone(), passers.clone(), roads.clone(), weather.clone(), transit.clone(), rail.clone(), level_crossings.clone(), facilities.clone(), services.clone(), land_value.clone(), land_value_work.clone(), events.clone(), next_seed.clone(), road_component.clone(), drive_component.clone(), connectivity.clone(), walk_networks.clone(), deferred_intents.clone(), stations.clone(), yards.clone(), halts.clone(), seller_index.clone(), seller_index_hour.clone(), available.clone(), groceries_for_sale.clone(), meals_for_sale.clone(), gyms_open.clone(), landscape.clone(), ports.clone(), fishing.clone(), road_z.clone(), road_inner.clone(), relief.clone(), bridge_metres.clone(), bots.clone(), driving.clone(), journeys.clone(), peds.clone(), waiting_cache.clone(), cable_waiting.clone(), crowds_cache.clone(), seats.clone(), society.clone(), vehicles.clone(), enforcement.clone(), air.clone(), contamination.clone(), foot_wear.clone(), lot_memory.clone(), land_tile_grounds.clone(), cleanups.clone(), industry_bias.clone(), commercial_bias.clone(), places.clone(), concerns.clone(), districts.clone(), tourism.clone(), subway.clone(), cable.clone(), tolls.clone(), taxis.clone(), cargo_hubs.clone(), airports.clone(), rides.clone(), streets.clone(), pillars.clone(), pillar_gaps.clone(), undercroft.clone(), crosswalks.clone(), estate.clone(), skyline.clone(), pressure.clone(), scene.clone(), identity.clone(), street_events.clone(), road_wear.clone(), dirty.clone(), road_log.clone(), social.clone(), social_concern.clone(), committee.clone(), shore.clone(), path_links.clone(), corps.clone(), megablocks.clone(), freight_queue.clone(), inbound_unanswered.clone(), food_need.clone(), food_trucks.clone(), games.clone(), airlifts.clone(), attractions.clone(), dams.clone(), parks.clone(), containers.clone(), talks.clone(), builders.clone(), builder_sites.clone()));
            let (Ok(a), Ok(b), Ok(c)) = (a, b, c) else { return exact_clone(|| self.clone()) };
            let (Ok(a), Ok(b), Ok(c)) = (a.join(), b.join(), c.join()) else { return exact_clone(|| self.clone()) };
            let (citizens,) = a;
            let (insight, travel, households, ground, lots, zones) = b;
            let (walk, traffic, economy, buildings, parcels, goods) = c;
            let (version, config, clock, rng, master, project, parked_projects, next_project, plan_warnings, implement_refused, parcel_cut, planned, parcel_lots, next_gesture, plan_version, plan_cache, wages_paid, town_shops, passers, roads, weather, transit, rail, level_crossings, facilities, services, land_value, land_value_work, events, next_seed, road_component, drive_component, connectivity, walk_networks, deferred_intents, stations, yards, halts, seller_index, seller_index_hour, available, groceries_for_sale, meals_for_sale, gyms_open, landscape, ports, fishing, road_z, road_inner, relief, bridge_metres, bots, driving, journeys, peds, waiting_cache, cable_waiting, crowds_cache, seats, society, vehicles, enforcement, air, contamination, foot_wear, lot_memory, land_tile_grounds, cleanups, industry_bias, commercial_bias, places, concerns, districts, tourism, subway, cable, tolls, taxis, cargo_hubs, airports, rides, streets, pillars, pillar_gaps, undercroft, crosswalks, estate, skyline, pressure, scene, identity, street_events, road_wear, dirty, road_log, social, social_concern, committee, shore, path_links, corps, megablocks, freight_queue, inbound_unanswered, food_need, food_trucks, games, airlifts, attractions, dams, parks, containers, talks, builders, builder_sites) = here;
            World { version, config, clock, rng, master, project, parked_projects, next_project, plan_warnings, implement_refused, parcels, parcel_cut, planned, parcel_lots, next_gesture, plan_version, plan_cache, wages_paid, town_shops, passers, roads, zones, lots, buildings, citizens, households, traffic, walk, weather, transit, rail, level_crossings, facilities, economy, services, land_value, land_value_work, events, next_seed, road_component, drive_component, connectivity, walk_networks, deferred_intents, stations, yards, halts, seller_index, seller_index_hour, available, groceries_for_sale, meals_for_sale, gyms_open, landscape, ports, fishing, road_z, road_inner, relief, ground, bridge_metres, bots, driving, journeys, peds, waiting_cache, cable_waiting, crowds_cache, seats, society, vehicles, enforcement, air, contamination, foot_wear, lot_memory, land_tile_grounds, cleanups, industry_bias, commercial_bias, places, concerns, districts, tourism, subway, cable, tolls, taxis, cargo_hubs, airports, rides, streets, pillars, pillar_gaps, undercroft, crosswalks, estate, skyline, pressure, scene, identity, street_events, road_wear, dirty, road_log, social, social_concern, committee, shore, path_links, corps, megablocks, freight_queue, inbound_unanswered, food_need, food_trucks, games, airlifts, attractions, dams, parks, insight, travel, containers, talks, builders, goods, builder_sites }
        })
    }

    /// Ms to clone each top-level field once (exactly as the sim thread
    /// does, inside `exact_clone`), biggest first (`clone_probe`).
    pub fn clone_cost(&self) -> Vec<(&'static str, f64)> {
        let mut out = Vec::new();
        exact_clone(|| {
            macro_rules! time {
                ($($f:ident)*) => {$(
                    let t = Instant::now();
                    let c = self.$f.clone();
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    std::hint::black_box(&c);
                    out.push((stringify!($f), ms));
                )*};
            }
            time!(version config clock rng master project parked_projects next_project plan_warnings implement_refused parcels parcel_cut planned parcel_lots next_gesture plan_version plan_cache wages_paid town_shops passers roads zones lots buildings citizens households traffic walk weather transit rail level_crossings facilities economy services land_value land_value_work events next_seed road_component drive_component connectivity walk_networks deferred_intents stations yards halts seller_index seller_index_hour available groceries_for_sale meals_for_sale gyms_open landscape ports fishing road_z road_inner relief ground bridge_metres bots driving journeys peds waiting_cache cable_waiting crowds_cache seats society vehicles enforcement air contamination foot_wear lot_memory land_tile_grounds cleanups industry_bias commercial_bias places concerns districts tourism subway cable tolls taxis cargo_hubs airports rides streets pillars pillar_gaps undercroft crosswalks estate skyline pressure scene identity street_events road_wear dirty road_log social social_concern committee shore path_links corps megablocks freight_queue inbound_unanswered food_need food_trucks games airlifts attractions dams parks insight travel containers talks builders goods builder_sites);
        });
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }
}

impl World {
    /// [r16-speed] Ms to clone each heap-holding part of every citizen, and
    /// how much each part holds (items over all citizens).
    pub fn citizen_clone_cost(&self) -> Vec<(&'static str, f64, usize)> {
        let mut out = Vec::new();
        crate::versioned::exact_clone(|| {
            macro_rules! time {
                ($($f:ident => $n:expr),*) => {$(
                    let t = Instant::now();
                    let c: Vec<_> = self.citizens.values().map(|c| c.$f.clone()).collect();
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    let n: usize = self.citizens.values().map($n).sum();
                    std::hint::black_box(&c);
                    out.push((stringify!($f), ms, n));
                )*};
            }
            time!(name => |_| 1, memories => |c| c.memories.len(), places => |c| c.places.places.len() + c.places.acquaintances.len(), thought => |_| 1, trail => |c| c.trail.len(), trail_stops => |_| 1, pending_footfall => |c| c.pending_footfall.len(), activity => |_| 1, commute => |_| 1, look => |_| 1, transit => |_| 1, to_car => |_| 1);
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::mind::IdleMind;
    use crate::{World, WorldConfig};

    /// The parallel copy is the exact clone: the same city (fingerprint),
    /// the same versions, and both go on to the same city.
    #[test]
    fn the_parallel_copy_is_the_exact_clone() {
        let mut w = World::new(WorldConfig::default());
        w.drain_events();
        crate::scenarios::demo_district(&mut w);
        let mut minds = IdleMind;
        for _ in 0..(2 * 1440) {
            w.tick(&mut minds);
        }
        assert!(!w.citizens.is_empty());
        let mut plain = crate::versioned::exact_clone(|| w.clone());
        let mut par = w.exact_copy();
        assert_eq!(plain.fingerprint().unwrap(), par.fingerprint().unwrap());
        assert_eq!(par.fingerprint().unwrap(), w.fingerprint().unwrap());
        for (a, b) in [(plain.roads.version(), par.roads.version()), (plain.lots.version(), par.lots.version()), (plain.buildings.version(), par.buildings.version()), (plain.landscape.version(), par.landscape.version()), (plain.ground.version(), par.ground.version()), (plain.land_value.version(), par.land_value.version())] {
            assert_eq!(a, b, "versions kept");
        }
        assert_eq!(par.roads.version(), w.roads.version());
        let (mut m1, mut m2) = (IdleMind, IdleMind);
        for _ in 0..300 {
            plain.tick(&mut m1);
            par.tick(&mut m2);
        }
        assert_eq!(plain.fingerprint().unwrap(), par.fingerprint().unwrap(), "both copies go on the same way");
    }
}
