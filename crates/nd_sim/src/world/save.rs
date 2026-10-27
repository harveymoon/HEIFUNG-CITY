//! Saving and loading: the RON save format, and what a load rebuilds,
//! migrates and checks (`after_load`, `validate`).

use super::*;

/// [saves] Gzip's first two bytes: a compressed save starts with them, RON
/// text never does.
pub const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// [saves] Whether a save's bytes are compressed.
pub fn is_compressed(bytes: &[u8]) -> bool {
    bytes.starts_with(&GZIP_MAGIC)
}

/// [saves] A save's RON text: gunzipped if compressed, else as it is. The
/// text is sized from gzip's length trailer up front, so a big save grows no
/// doubling buffers on the way.
pub fn save_text(bytes: &[u8]) -> std::io::Result<String> {
    use std::io::Read;
    if !is_compressed(bytes) {
        return String::from_utf8(bytes.to_vec()).map_err(std::io::Error::other);
    }
    // The trailer holds the length mod 2^32: a hint only, and never more
    // than RON packs into (a damaged file must not ask for gigabytes).
    let hint = bytes.len().checked_sub(4).map_or(0, |at| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize);
    let mut text = String::with_capacity(hint.min(bytes.len().saturating_mul(64)) + 1);
    flate2::read::MultiGzDecoder::new(bytes).read_to_string(&mut text)?;
    Ok(text)
}

impl World {
    /// The RON format: pretty RON, nested six deep.
    pub(super) fn ron_config() -> ron::ser::PrettyConfig {
        ron::ser::PrettyConfig::default().depth_limit(6)
    }

    /// [saves] The format of the game's save files: [`Self::ron_config`]
    /// one item a line but not indented. Indentation was half of a big
    /// city's save (124 of its 239 MB), all of it to write, compress, hold
    /// in memory while loading and parse. Indented saves load the same.
    fn save_config() -> ron::ser::PrettyConfig {
        Self::ron_config().indentor("")
    }

    /// [saves] The save file's RON ([`Self::save_config`]) as a string, as
    /// [`World::write_ron_gz`] compresses it. [`World::to_ron`] stays
    /// indented for reading by eye and for tests that cut fields out.
    pub fn to_save_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, Self::save_config())
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, Self::ron_config())
    }

    /// [memory] The save, byte for byte as [`World::to_ron`], streamed into
    /// `out` (wrap a file in a `BufWriter`). A big city's save runs past
    /// 128 MiB, and building it as one `String` first needs a single 256 MiB
    /// block when the string doubles: the "memory allocation of 268435456
    /// bytes failed" crash.
    pub fn write_ron(&self, out: impl std::io::Write) -> Result<(), ron::Error> {
        ron::Options::default().to_io_writer_pretty(out, self, Self::ron_config())
    }

    /// [saves] The save compressed: [`World::to_save_ron`]'s RON through gzip
    /// at its fastest level (a big city's 240 MB of RON comes to 40 MB,
    /// in 1.2 s against 0.8 s plain). [`World::from_save_bytes`] reads it and the plain RON alike.
    pub fn write_ron_gz(&self, out: impl std::io::Write) -> std::io::Result<()> {
        // RON writes in small pieces: buffered before the compressor, which
        // pays per call (unbuffered it took thirty times as long).
        let mut buf = std::io::BufWriter::with_capacity(1 << 20, flate2::write::GzEncoder::new(out, flate2::Compression::fast()));
        ron::Options::default().to_io_writer_pretty(&mut buf, self, Self::save_config()).map_err(std::io::Error::other)?;
        buf.into_inner().map_err(|e| e.into_error())?.finish()?.flush()
    }

    /// [saves] A world from a save file's bytes: gzip-compressed RON
    /// ([`World::write_ron_gz`]) or plain RON (every older save).
    pub fn from_save_bytes(bytes: &[u8]) -> Result<Self, String> {
        let text = save_text(bytes).map_err(|e| e.to_string())?;
        Self::from_ron(&text).map_err(|e| e.to_string())
    }

    /// [saves] [`World::from_save_bytes`] of a file. The compressed bytes
    /// are dropped before the RON is parsed, so the peak is the RON text and
    /// the world, as for a plain save.
    pub fn read_save(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let text = {
            let bytes = std::fs::read(path.as_ref()).map_err(|e| e.to_string())?;
            save_text(&bytes).map_err(|e| e.to_string())?
        };
        Self::from_ron(&text).map_err(|e| e.to_string())
    }

    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        let mut w: World = ron::from_str(text)?;
        w.after_load();
        Ok(w)
    }

    /// Rebuild derived state that is not serialised, and make a save from
    /// any earlier version safe to run.
    pub(super) fn after_load(&mut self) {
        // Speeds follow the time scale, so a save from before a retune
        // walks at today's pace.
        self.config.derive_speeds();
        self.config.landscape = self.landscape.params.clone();
        // [grid-weather] A save from before the climates keeps its weather
        // but learns which climate it is in.
        self.weather.climate = self.landscape.params.climate;
        self.validate();
        self.migrate_through_cars();
        // [pt-visual] Generic names that do not fit the business (F38).
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for id in ids {
            self.fit_business_name(id);
        }
        // [people] Saves from before households: one per citizen. Detected
        // per citizen (no household), so no version bump is needed.
        self.migrate_households();
        // [stories] Districts from before stories take the story their
        // dials are (or stay custom).
        self.districts.adopt_stories();
        // A parking lot the player placed before the flag existed is one of
        // the city's own: roads stop at it instead of buying it out.
        let city_lots: Vec<BuildingId> = self.vehicles.city_lots.keys().copied().collect();
        for id in city_lots {
            if let Some(b) = self.buildings.get_mut(id) {
                b.city_parking = true;
            }
        }
        self.version = SAVE_VERSION;
        // Ring pieces and slip roads are flagged from the network's shape;
        // a save from before the flags has them unset.
        self.roads.classify();
        self.rebuild_road_components();
        self.streets.relink(&self.roads, self.config.seed);
        self.rebuild_ground();
        self.traffic.rebuild(&self.roads);
        self.apply_tolls();
        // [events] Worn streets are rough again for the traffic.
        self.apply_road_wear();
        // Park layouts are saved with their lots (the benches people sit on
        // must survive a load), so they are only planned again when a save
        // from before parks were serialised has park lots without one.
        if self.lots.values().any(|l| l.land_use == LandUse::Park && l.polygon.len() >= 3 && l.park.is_none()) {
            let links: Vec<Vec2> = self.path_links.links.iter().map(|l| l.pos).collect();
            crate::park::plan_parks_wild(&mut self.lots, &self.land_value, &links, &self.parks.preserves);
            self.plan_attraction_grounds();
        }
        // [K10] Grounds side by side in a save from before they were joined:
        // their walks run out to the seam now (a no-op once they do).
        self.join_neighbouring_grounds();
        // [free-placement] A save from before city ground: the lots under
        // the city's buildings leave their zones (nothing moves).
        self.settle_city_ground(&[], false);
        // [tools] Footbridges and push buttons on the crossings; [park-paths]
        // paths joined to the parks.
        self.build_walk_graph();
        // [tools] Columns for raised roads from a save without them.
        self.pillars_after_load();
        for (_, car) in self.traffic.reattach_cars() {
            self.strand_car(car);
        }
        self.vehicles_after_load();
        // [services] Which car is which unit's is not saved: without it the
        // first unit car to arrive after a load went unclaimed.
        self.services.by_car = self.services.units.iter().filter_map(|(id, u)| u.car.map(|c| (c, id))).collect();
        // [services] Docks retiered by the daily update before `is_special`
        // knew them (an older save): their jobs back.
        for b in self.buildings.values_mut() {
            if let Some(kind) = b.port.filter(|_| b.construction.is_none()) {
                b.jobs = kind.jobs();
            }
        }
        self.transit.refresh_legs(&self.traffic.net);
        let on_trains: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::OnTrain { .. })).map(|(id, _)| id).collect();
        for id in on_trains {
            let pos = self.citizens[id].pos;
            self.citizens[id].transit = None;
            self.strand_driver(id, pos);
        }
        // [ped-highway] People a save left standing on a highway or ramp
        // walk to the pavement.
        self.rescue_stranded();
        self.refresh_facility_cache();
        // [land] A save from before the land market: a cold pressure grid
        // (it warms on the first hourly sample); stale references dropped.
        if !self.estate.pressure.ready() {
            self.estate.pressure = crate::estate::Pressure::new(self.grid_half(crate::estate::PRESSURE_CELL));
        }
        // [sweep-sim] A save from before the grids covered far islands:
        // grown (cells keep their place; land value refills on its next
        // pass, pressure warms again).
        self.grow_grids();
        self.prune_estate();
        if self.config.social && self.social_concern.is_none() {
            self.social_concern = self.concern_of_the_week();
        }
        self.subway_after_load();
        self.seats_after_load();
        // [dirty-industry] A save from before the dirty plants were kept
        // (or a stale entry): read them off the buildings.
        if self.dirty.is_empty() || self.dirty.iter().any(|d| !self.buildings.contains_key(d.building)) {
            self.refresh_dirty();
        }
        // [pt-growth] (F8) Civic halls are two to five storeys now (what
        // their styles draw): an older save's taller ones are cut down to
        // that, their staff kept about as they were by the grander tier's
        // staffing per floor.
        for b in self.buildings.values_mut() {
            if b.land_use == LandUse::Civic && b.is_grown() && b.service.is_none() && b.plot.tower.is_none() && b.construction.is_none() && b.floors > 5 {
                b.floors = 5;
                b.recompute_capacity();
            }
        }
        // [pt-growth] A save from before the rescaled land value reads 1.0
        // almost everywhere: recompute it now, not two hours in.
        if self.land_value.raw.is_empty() && !self.buildings.is_empty() {
            self.update_land_value();
        }
        // [tenants] A save from before units: every building that can be
        // let in parts is cut into units, its business keeping all it
        // needs for its staff (nobody laid off), the rest to let.
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for id in ids {
            self.plan_units(id, true);
        }
        self.sync_tenants();
        self.update_economy();
        // A save from before named projects: number and name the one it has.
        self.name_active_project();
        self.recut_parcels();
        // [waterfront] frontage on the water, for a save from before
        self.mark_shore_lots();
        self.plan_changed();
        self.events.push(SimEvent::PlanChanged);
        // [ledger] Every balance into its account (an older save's money
        // fields become the opening balances).
        self.open_ledger();
    }

    /// Drop every reference to something that is not there: a hand-edited
    /// or truncated save must never index a missing building or citizen.
    pub(super) fn validate(&mut self) {
        let tick = self.clock.tick;
        // Lots and buildings must agree about who stands where.
        for l in self.lots.values_mut() {
            if l.building.is_some_and(|b| !self.buildings.contains_key(b)) {
                l.building = None;
            }
        }
        let building_ids: std::collections::HashSet<BuildingId> = self.buildings.keys().collect();
        for b in self.buildings.values_mut() {
            b.residents.retain(|c| self.citizens.contains_key(*c));
            b.workers.retain(|c| self.citizens.contains_key(*c));
            b.rail_inbound.retain(|c| building_ids.contains(&c.to));
        }
        self.rail.orders.retain(|o| building_ids.contains(&o.cargo.to));
        // [freight] Saves from before freight rode the trains kept rail
        // orders on the yard, waiting for any train to call: they are out of
        // town now, first in the queue for the next freight train.
        let mut legacy: Vec<crate::rail::RailOrder> = Vec::new();
        for (_, b) in self.buildings.iter_mut() {
            legacy.extend(std::mem::take(&mut b.rail_inbound).into_iter().map(|cargo| crate::rail::RailOrder { cargo, since: tick }));
        }
        self.rail.orders.extend(legacy);
        for f in self.facilities.values_mut() {
            if f.building.is_some_and(|b| !self.buildings.contains_key(b)) {
                f.building = None;
            }
            if f.lot.is_some_and(|l| !self.lots.contains_key(l)) {
                f.lot = None;
            }
        }
        // [people] Households and citizens must agree about who belongs where.
        for h in self.households.values_mut() {
            h.members.retain(|c| self.citizens.contains_key(*c));
            if h.home.is_some_and(|b| !self.buildings.contains_key(b)) {
                h.home = None;
            }
        }
        self.households.retain(|_, h| !h.members.is_empty());
        for b in self.buildings.values_mut() {
            b.households.retain(|h| self.households.contains_key(*h));
        }
        for c in self.citizens.values_mut() {
            if c.household.is_some_and(|h| !self.households.contains_key(h)) {
                c.household = None;
            }
        }
        let buildings = &self.buildings;
        let routes = &self.transit.routes;
        for c in self.citizens.values_mut() {
            let mut lost = false;
            for slot in [&mut c.home, &mut c.work, &mut c.inside, &mut c.parked_at] {
                if slot.is_some_and(|b| !buildings.contains_key(b)) {
                    *slot = None;
                    lost = true;
                }
            }
            let trip_ok = match c.transit.map(|t| t.leg) {
                None => true,
                Some(TransitLeg::Bus { route, board, alight }) => routes.get(route).is_some_and(|r| board < r.stops.len() && alight < r.stops.len()),
                Some(TransitLeg::Rail { board, alight }) => buildings.contains_key(board) && buildings.contains_key(alight),
                Some(TransitLeg::Air { from, to }) => buildings.contains_key(from) && buildings.contains_key(to),
                Some(TransitLeg::Subway { line, board, alight }) => self.subway.lines.get(line).is_some_and(|l| l.index_of(board).is_some() && l.index_of(alight).is_some()),
                Some(TransitLeg::Taxi) => true,
            } && c.transit.is_none_or(|t| buildings.contains_key(t.target));
            if !trip_ok {
                c.transit = None;
                lost = true;
            }
            let target_gone = match &c.activity {
                Activity::Visiting(b) | Activity::Sitting { park: b, .. } => !buildings.contains_key(*b),
                Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } => !buildings.contains_key(*b),
                Activity::AtHome => c.home.is_none(),
                Activity::Working => c.work.is_none(),
                Activity::Waiting { .. } | Activity::Riding { .. } | Activity::OnTrain { .. } => c.transit.is_none(),
                Activity::Flying { drone } => !self.air.drones.contains_key(*drone),
                // Trains are not saved: `subway_after_load` sets riders down.
                Activity::OnMetro { .. } => c.transit.is_none(),
                _ => false,
            };
            if lost || target_gone {
                c.activity = Activity::Idle;
                c.inside = None;
                c.next_decision_tick = tick + 1;
            }
        }
        // Vehicles whose ends no longer exist are not deleted where they
        // stand: a lorry bound for a building that is gone drives on when
        // it gets there (`lorry_arrived` looks every building up), and a bus
        // whose line is gone lets its riders off (`validate` above already
        // set them to decide again) and heads for its depot when it reaches
        // the end of this leg. Riders who no longer exist get off.
        for car in self.traffic.cars.values_mut() {
            if let Some(bus) = car.bus.as_mut() {
                if !routes.contains_key(bus.route) {
                    bus.passengers.clear();
                    bus.to_depot = true;
                    bus.next_stop = usize::MAX;
                }
                bus.passengers.retain(|p| self.citizens.contains_key(*p));
            }
        }
    }

    /// [services] Saves from before `Errand::Through` marked through traffic
    /// only by having no driver, owner, cargo or bus, which also matched
    /// service units, tow and garbage trucks and takeover outsiders. Those
    /// tracked by their own system are left as they are; the rest are
    /// through cars.
    pub(super) fn migrate_through_cars(&mut self) {
        use crate::vehicle::Errand;
        let mut tracked: std::collections::HashSet<CarId> = self.services.units.values().filter_map(|u| u.car).collect();
        tracked.extend(self.society.trash.trucks.values().filter_map(|t| t.car));
        tracked.extend(self.driving.tows.iter().map(|t| t.truck));
        let citizens = &self.citizens;
        for (id, car) in self.traffic.cars.iter_mut() {
            let legacy = car.errand == Errand::Trip && !citizens.contains_key(car.citizen) && car.cargo.is_none() && car.bus.is_none() && car.owned.is_none() && !car.siren && car.drive.gang.is_none();
            if legacy && !tracked.contains(&id) {
                car.errand = Errand::Through;
            }
        }
    }
}

#[cfg(test)]
mod save_format_tests {
    use super::*;

    /// [saves] A compressed save holds the unindented RON and loads; a
    /// plain (indented, older) one loads too; a cut-off file is an error,
    /// not a panic.
    #[test]
    fn compressed_saves_round_trip() {
        let w = World::new(WorldConfig::default());
        let mut buf = Vec::new();
        w.write_ron_gz(&mut buf).unwrap();
        assert!(is_compressed(&buf));
        assert_eq!(save_text(&buf).unwrap(), w.to_save_ron().unwrap());
        assert!(buf.len() * 4 < w.to_ron().unwrap().len());
        let back = World::from_save_bytes(&buf).expect("compressed loads");
        assert_eq!(back.clock.tick, w.clock.tick);
        assert_eq!(back.lots.len(), w.lots.len());
        let plain = World::from_save_bytes(w.to_ron().unwrap().as_bytes()).expect("plain loads");
        assert_eq!(plain.to_save_ron().unwrap(), back.to_save_ron().unwrap());
        assert!(World::from_save_bytes(&buf[..buf.len() / 2]).is_err());
    }
}
