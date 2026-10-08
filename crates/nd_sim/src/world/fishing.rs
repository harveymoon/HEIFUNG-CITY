//! [fishing] Stepping the fishing fleets (`crate::fishing`): fleets kept
//! in step with the fishing ports, grounds found, boats sent out in the
//! morning, trawling, hauling, home in the afternoon and the catch landed
//! into the port's stock. Nothing here draws a random number: the day's
//! plan for each boat is a hash of the port, the day and the boat.

use super::*;
use crate::fishing::{self as fish, Fleet, Trawler, TrawlerPhase};
use crate::ports::{Port, PortKind, ShipKind};

/// A trawler's day as the port sees it, for the inspector and the API.
#[derive(Clone, Debug, PartialEq)]
pub struct BoatReport {
    pub name: String,
    pub phase: TrawlerPhase,
    pub pos: Vec2,
    pub ground: Option<u8>,
    pub hold: f32,
    pub catch_today: f32,
    pub fished_min: u32,
    pub home_by: Option<u32>,
}

fn port_seed(b: BuildingId) -> u64 {
    slotmap::Key::data(&b).as_ffi()
}

impl World {
    /// What the docks are, for finding grounds again when one changes.
    fn fishing_key(&self) -> u64 {
        let mut k = 0x5EA5_u64;
        for p in self.ports.ports.values() {
            let b = p.building.map_or(0, port_seed);
            k = fish::hash(k, b ^ (p.kind as u64) << 56 ^ (p.berth.x.to_bits() as u64) << 20 ^ p.berth.y.to_bits() as u64);
        }
        k
    }

    /// Shipping lanes and ferry routes fishing grounds keep clear of.
    fn shipping_lanes(&self) -> Vec<Vec<Vec2>> {
        let mut out = Vec::new();
        let ferries: Vec<Vec2> = self.ports.ports.values().filter(|p| p.kind == PortKind::FerryTerminal).map(|p| p.berth).collect();
        for p in self.ports.ports.values() {
            match p.kind {
                PortKind::ShippingPort => out.extend(self.landscape.water_route(p.berth, None)),
                PortKind::FerryTerminal => {
                    out.extend(self.landscape.water_routes_out(p.berth).into_iter().map(|(_, r)| r));
                    for q in &ferries {
                        if q.distance(p.berth) > 1.0 {
                            out.extend(self.landscape.water_route(p.berth, Some(*q)));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Fleets for the fishing ports there are (a boat per berth), and
    /// their grounds; dropped with their port.
    pub(super) fn refresh_fleets(&mut self) {
        let key = self.fishing_key();
        if key == self.fishing.key {
            return;
        }
        self.fishing.key = key;
        let ports: Vec<(BuildingId, Port)> = self.ports.ports.values().filter(|p| p.kind.is_fishing()).filter_map(|p| p.building.map(|b| (b, p.clone()))).collect();
        self.fishing.fleets.retain(|b, _| ports.iter().any(|(pb, _)| pb == b));
        if ports.is_empty() {
            return;
        }
        let grid = fish::WaterGrid::build(&self.landscape);
        let lanes = self.shipping_lanes();
        for (b, port) in ports {
            let land = &self.landscape;
            let moorings = fish::quay_moorings(&port, land);
            let offing = fish::offing(&port, land);
            let docks: Vec<Vec2> = self.ports.ports.values().filter(|p| p.building != Some(b)).map(|p| p.berth).collect();
            let grounds = fish::find_grounds(land, &grid, offing, &lanes, &docks);
            let fleet = self.fishing.fleets.entry(b).or_default();
            let n = moorings.len().max(1);
            if fleet.kind != Some(port.kind) {
                fleet.boats.clear();
            }
            fleet.boats.truncate(n);
            while fleet.boats.len() < n {
                let slot = fleet.boats.len();
                fleet.boats.push(Trawler { name: fish::boat_name(port_seed(b), slot), slot: slot as u8, ..Default::default() });
            }
            fleet.kind = Some(port.kind);
            fleet.moorings = moorings;
            fleet.offing = offing;
            fleet.grounds = grounds;
        }
    }

    /// How fouled the water at `p` is by dirty industry, `0..1`: the
    /// contamination that reached it and the plants nearby.
    pub fn water_fouling(&self, p: Vec2) -> f32 {
        let near = self.dirt_exposure_at(p, crate::industry::Neighbour::Industry(crate::industry::IndustryKind::Fishing), None);
        self.contamination.at(p).max(near).clamp(0.0, 1.0)
    }

    /// Fish a boat of a port with `jobs` and `boats` brings in per minute
    /// fished on an ordinary ground: about what the port's recipe used to
    /// make round the clock, spread over its boats' fishing time.
    pub fn catch_rate(jobs: u16, boats: usize) -> f32 {
        let per = BusinessKind::FishingPort.recipe().outputs.first().map_or(3.0, |o| o.1);
        let days_out = 1.0 - 1.0 / fish::REST_DAY_IN as f32;
        per * jobs as f32 * crate::market::JOB_RATE * 24.0 / (boats.max(1) as f32 * days_out * fish::NOMINAL_FISH_MIN)
    }

    /// A port has boats that fish real grounds (else it makes its fish the
    /// old way, by the hour).
    pub fn port_fishes_grounds(&self, b: BuildingId) -> bool {
        self.fishing.fleets.get(&b).is_some_and(|f| !f.grounds.is_empty())
    }

    /// Every tick: the fleets keep up with the docks, boats go out, fish
    /// and come home.
    pub(super) fn step_fishing(&mut self) {
        if self.fishing.fleets.is_empty() && !self.ports.ports.values().any(|p| p.kind.is_fishing()) {
            return;
        }
        self.refresh_fleets();
        let minute = self.clock.minute_of_day() as u32;
        let day = self.clock.day();
        if minute == 0 {
            for f in self.fishing.fleets.values_mut() {
                f.landed_yesterday = f.landed_today;
                f.landed_today = 0.0;
                f.trips_today = 0;
                f.kept_in = None;
                for b in &mut f.boats {
                    b.catch_today = 0.0;
                }
            }
        }
        let storm = self.weather.spell == crate::weather::Spell::Storm;
        let steam = self.config.metres_per_tick(fish::STEAM_MPS);
        let trawl = self.config.metres_per_tick(fish::TRAWL_MPS);
        let ids: Vec<BuildingId> = self.fishing.fleets.keys().copied().collect();
        let mut at_sea = 0;
        for b in ids {
            let Some(port) = self.ports.port_of(b).cloned() else { continue };
            let Some(bld) = self.buildings.get(b) else { continue };
            let open = port.works() && bld.construction.is_none() && bld.business.as_ref().is_some_and(|z| !z.closed && z.kind == BusinessKind::FishingPort);
            let staffing = bld.staffing();
            let jobs = bld.jobs;
            let mut fleet = std::mem::take(self.fishing.fleets.get_mut(&b).expect("listed"));
            let rate = World::catch_rate(jobs, fleet.boats.len());
            let crewed = ((fleet.boats.len() as f32) * staffing).round() as usize;
            let seed = port_seed(b);
            let mut landed = 0.0f32;
            let grounds = std::mem::take(&mut fleet.grounds);
            for i in 0..fleet.boats.len() {
                let mooring = fleet.mooring(fleet.boats[i].slot).0;
                let boat = &mut fleet.boats[i];
                boat.prev_s = boat.s;
                let h = fish::hash(seed ^ day.wrapping_mul(0x51ED), i as u64);
                match boat.phase {
                    TrawlerPhase::InPort => {
                        if !open || boat.trip_day == Some(day) || grounds.is_empty() {
                            continue;
                        }
                        let depart = fish::DEPART_FROM + (h % fish::DEPART_SPREAD as u64) as u32;
                        if minute < depart || minute >= fish::LAST_DEPARTURE {
                            continue;
                        }
                        if storm {
                            fleet.kept_in = Some("a storm: the boats stay in port".into());
                            continue;
                        }
                        if i >= crewed || (h >> 20) % fish::REST_DAY_IN == 0 {
                            continue;
                        }
                        let g = ((h >> 8) % grounds.len() as u64) as usize;
                        let ground = &grounds[g];
                        let mut path = fish::leave_quay(&port, mooring, fleet.offing, &self.landscape);
                        path.extend(ground.route.iter().skip(1).copied());
                        path.dedup_by(|a, b| a.distance(*b) < 0.5);
                        boat.out = path.clone();
                        boat.path = path;
                        boat.s = 0.0;
                        boat.prev_s = 0.0;
                        boat.phase = TrawlerPhase::Outbound;
                        boat.ground = Some(g as u8);
                        boat.spot = Some((ground.center, ground.radius));
                        boat.trip_day = Some(day);
                        boat.home_by = fish::HOME_FROM + ((h >> 32) % fish::HOME_SPREAD as u64) as u32;
                        boat.rate = rate * ground.richness * (1.0 - fish::FOUL_HIT * self.water_fouling(ground.center));
                        boat.fished = 0;
                        boat.sweeps = 0;
                        boat.hold = 0.0;
                        fleet.trips_today += 1;
                    }
                    TrawlerPhase::Outbound => {
                        boat.s += steam;
                        if boat.s >= fish::path_len(&boat.path) {
                            let (c, r) = boat.spot.unwrap_or((boat.pose_at(boat.s).0, 20.0));
                            boat.path = fish::sweep(&self.landscape, c, r, (h % 628) as f32 / 100.0);
                            boat.s = 0.0;
                            boat.prev_s = 0.0;
                            boat.phase = TrawlerPhase::Fishing;
                        }
                    }
                    TrawlerPhase::Fishing | TrawlerPhase::Hauling => {
                        boat.fished += 1;
                        boat.hold += boat.rate;
                        if boat.phase == TrawlerPhase::Hauling {
                            boat.left = boat.left.saturating_sub(1);
                            if boat.left == 0 {
                                boat.phase = TrawlerPhase::Fishing;
                            }
                        } else {
                            boat.s += trawl;
                            let len = fish::path_len(&boat.path);
                            if boat.s >= len {
                                // Another sweep, turned from the last.
                                boat.sweeps += 1;
                                let (c, r) = boat.spot.unwrap_or((boat.pose_at(len).0, 20.0));
                                boat.path = fish::sweep(&self.landscape, c, r, (h % 628) as f32 / 100.0 + boat.sweeps as f32 * 0.9);
                                boat.s = (boat.s - len).min(1.0);
                                boat.prev_s = boat.s;
                            }
                            if boat.fished % fish::HAUL_EVERY == 0 {
                                boat.phase = TrawlerPhase::Hauling;
                                boat.left = fish::HAUL_MIN;
                            }
                        }
                        // Time to head in: home by its hour, at once in a
                        // storm, or when the port has shut.
                        let pos = boat.pose_at(boat.s).0;
                        let (c, _) = boat.spot.unwrap_or((pos, 0.0));
                        let way = pos.distance(c) + fish::path_len(&boat.out);
                        let travel = (way / steam.max(0.1)).ceil() as u32;
                        if storm || !open || minute + travel + 5 >= boat.home_by || minute >= fish::CURFEW {
                            let mut path = vec![pos, c];
                            path.extend(boat.out.iter().rev().copied());
                            path.dedup_by(|a, b| a.distance(*b) < 0.5);
                            boat.path = path;
                            boat.s = 0.0;
                            boat.prev_s = 0.0;
                            boat.phase = TrawlerPhase::Inbound;
                        }
                    }
                    TrawlerPhase::Inbound => {
                        boat.s += steam;
                        if boat.s >= fish::path_len(&boat.path) {
                            boat.s = fish::path_len(&boat.path);
                            boat.phase = TrawlerPhase::Unloading;
                            boat.left = fish::UNLOAD_MIN;
                            landed += boat.hold;
                            boat.catch_today += boat.hold;
                            boat.hold = 0.0;
                        }
                    }
                    TrawlerPhase::Unloading => {
                        boat.left = boat.left.saturating_sub(1);
                        if boat.left == 0 {
                            boat.phase = TrawlerPhase::InPort;
                            boat.path.clear();
                            boat.out.clear();
                            boat.s = 0.0;
                            boat.prev_s = 0.0;
                            boat.spot = None;
                        }
                    }
                }
            }
            fleet.grounds = grounds;
            fleet.landed_today += landed;
            at_sea += fleet.at_sea();
            *self.fishing.fleets.get_mut(&b).expect("listed") = fleet;
            if landed > 0.0 {
                self.land_catch(b, landed);
            }
        }
        let marina = self.ports.ships.values().filter(|s| matches!(s.kind, ShipKind::Yacht | ShipKind::Sailboat)).count();
        self.scene.boats_out = (marina + at_sea) as u32;
    }

    /// Boxes of fish onto the quay: into the port's stock (no more than
    /// its cold store holds) and the day's figures.
    fn land_catch(&mut self, b: BuildingId, amount: f32) {
        self.scene.fish_landed_today += amount;
        let Some(bld) = self.buildings.get_mut(b) else { return };
        let jobs = bld.jobs;
        let Some(z) = bld.business.as_mut() else { return };
        let per = BusinessKind::FishingPort.recipe().outputs.first().map_or(3.0, |o| o.1);
        let cap = per * jobs as f32 * crate::market::JOB_RATE * 96.0;
        let room = (cap - z.stock_of(Resource::Fish)).max(0.0);
        z.add(Resource::Fish, amount.min(room));
    }

    /// A fishing port's boats, for the inspector and the API.
    pub fn fishing_boats(&self, b: BuildingId) -> Vec<BoatReport> {
        let Some(f) = self.fishing.fleets.get(&b) else { return Vec::new() };
        f.boats
            .iter()
            .enumerate()
            .map(|(k, t)| BoatReport {
                name: t.name.clone(),
                phase: t.phase,
                pos: f.boat_pose(k, 1.0).0,
                ground: t.ground.filter(|_| t.phase.at_sea()),
                hold: t.hold,
                catch_today: t.catch_today,
                fished_min: t.fished,
                home_by: t.phase.at_sea().then_some(t.home_by),
            })
            .collect()
    }

    /// The fleet of a fishing port.
    pub fn fleet(&self, b: BuildingId) -> Option<&Fleet> {
        self.fishing.fleets.get(&b)
    }
}
