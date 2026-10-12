//! Putting the city's buildings down: the Buildings tool's placement, site
//! checks, service stations and their units, and what a building was placed as.

use super::*;

impl World {
    /// Put down anything the Buildings tool offers at `p`: a rail piece
    /// where clicked, everything else on the lot under `p` or, on unzoned
    /// ground, on a lot made for the parcels there ([free-placement]).
    pub fn place_building(&mut self, what: Placeable, p: Vec2) -> Result<BuildingId, String> {
        if let Placeable::Rail(f) = what {
            return self.place_rail_facility(f, p);
        }
        if let Placeable::Dock(k) = what {
            return self.place_port(k, p);
        }
        if let Placeable::Toll(_) = what {
            return Err("a toll booth goes on a road, not a lot (World::place_toll)".into());
        }
        let id = self.place_on_site(what, p)?;
        self.placed_grounds(what);
        Ok(id)
    }

    /// [park-paths] A stadium, plaza, campus or station just put down
    /// shows its gates to the Path tool at once.
    pub(super) fn placed_grounds(&mut self, what: Placeable) {
        use crate::rail::RailFacility;
        if matches!(what, Placeable::Service(ServiceKind::Stadium | ServiceKind::Monument | ServiceKind::School, _) | Placeable::Rail(RailFacility::Station | RailFacility::Central)) {
            self.build_walk_graph();
        }
        // [attractions] A landmark's forecourt shows its gates; a park,
        // zoo, garden or pier lays out its walkways and benches.
        if let Placeable::Service(k, _) = what {
            if k.is_landmark() {
                if k.has_grounds() {
                    self.plan_attraction_grounds();
                }
                self.build_walk_graph();
            }
        }
    }

    /// Put down `what` on a lot (rail pieces at the lot's centre).
    pub fn place_on_lot(&mut self, what: Placeable, lot: LotId) -> Result<BuildingId, String> {
        // [big-sites] Too small on its own: the neighbours it needs join it.
        let lot = match self.joined_site(what, lot) {
            Some((joined, outline)) => {
                let site = super::super::placement::PlacementSite { lot: Some(lot), parcels: Vec::new(), outline, leaves_zone: None, joined };
                if let Some(why) = self.site_problem(what, &site) {
                    return Err(why);
                }
                self.join_site_lots(&site, what)?
            }
            _ => lot,
        };
        let id = self.place_on_lot_inner(what, lot)?;
        self.placed_grounds(what);
        Ok(id)
    }

    pub(super) fn place_on_lot_inner(&mut self, what: Placeable, lot: LotId) -> Result<BuildingId, String> {
        match what {
            Placeable::Plant(k) => self.build_power(lot, k),
            Placeable::Battery(k) => self.build_battery(lot, k),
            Placeable::BusDepot => self.build_depot(lot),
            Placeable::Service(k, t) => self.build_service(lot, k, t),
            Placeable::Rail(f) => {
                let c = self.lots.get(lot).ok_or("no such lot")?.center;
                self.place_rail_facility(f, c)
            }
            Placeable::Dock(k) => {
                let c = self.lots.get(lot).ok_or("no such lot")?.center;
                self.place_port(k, c)
            }
            Placeable::ParkingLot(size) => self.build_city_parking(lot, size),
            Placeable::MetroStation => self.build_metro_station(lot),
            Placeable::Toll(_) => Err("a toll booth goes on a road, not a lot (World::place_toll)".into()),
            Placeable::CargoHub => self.build_cargo_hub(lot),
        }
    }

    /// Why `what` cannot go on `lot`, for the placement preview; None if it can.
    pub fn placement_problem(&self, what: Placeable, lot_id: LotId) -> Option<String> {
        let Some(lot) = self.lots.get(lot_id) else { return Some("no such lot".into()) };
        self.lot_problem(what, lot, Some(lot_id))
    }

    /// [free-placement] [`World::placement_problem`] for a lot that may not
    /// exist yet (`lot_id` None: one about to be made on unzoned ground).
    pub(in crate::world) fn lot_problem(&self, what: Placeable, lot: &Lot, lot_id: Option<LotId>) -> Option<String> {
        if matches!(what, Placeable::Rail(_) | Placeable::Dock(_)) {
            return None;
        }
        if matches!(what, Placeable::Toll(_)) {
            return Some("a toll booth goes on a road".into());
        }
        // [hq] One headquarters per city for the police and the fire brigade.
        if let Some(why) = self.hq_problem(what) {
            return Some(why);
        }
        // Whatever grew there is bought out; another civic structure is not.
        if let Some(b) = lot.building.and_then(|b| self.buildings.get(b)) {
            if Self::is_civic_structure(b) {
                return Some(format!("{} stands there; bulldoze it first", b.name));
            }
        }
        // [attractions] One per city, said first: whatever the ground, a
        // second one is no use.
        if let Placeable::Service(k, _) = what {
            if let Some(b) = k.is_landmark().then(|| self.landmark(k)).flatten().and_then(|b| self.buildings.get(b)) {
                return Some(format!("one per city: {} is the city's {} already", b.name, k.label()));
            }
        }
        if lot.landlocked {
            return Some("that land has no road access".into());
        }
        if self.under_facility(lot.center) {
            return Some("that ground belongs to a station or yard".into());
        }
        // [free-placement] Zoning is no bar: only a park is kept.
        if !what.allowed_on(lot.land_use) {
            return Some(format!("that is park land: unzone it before putting {} there", crate::names::a(what.label())));
        }
        // [plant-groups] A small lot may join the solar or wind farm next door.
        // [plant-scale] A site on unzoned ground (no lot yet) too.
        let joins = matches!(what, Placeable::Plant(k) if lot.area() < k.min_area() && self.small_plant_site_joins(k, &lot.polygon, lot_id).is_some());
        if lot.area() < what.min_area() && !joins {
            return Some(format!("too small: {} needs {:.0} m² (this lot is {:.0})", crate::names::a(what.label()), what.min_area(), lot.area()));
        }
        // [plant-scale] A heavy plant is not put on a strip along the kerb.
        let thick = crate::zoning::site_thickness(&lot.polygon);
        if thick < what.min_thickness() {
            return Some(format!("too narrow: {} needs ground about {:.0} m deep and wide (this is a strip {:.0} m across)", crate::names::a(what.label()), what.min_thickness() * 2.0, thick * 2.0));
        }
        if let Placeable::Plant(k) = what {
            if let Err(e) = self.power_site_on(lot, k) {
                return Some(e);
            }
        }
        // [attractions] One per city; the pier on the sea shore, the
        // observatory on high ground.
        if let Placeable::Service(k, _) = what {
            if k.is_landmark() {
                if let Some(why) = self.landmark_problem(k, lot) {
                    return Some(why);
                }
            }
        }
        let needs_street = matches!(what, Placeable::BusDepot | Placeable::ParkingLot(_) | Placeable::CargoHub) || matches!(what, Placeable::Service(k, _) if k.unit_kind().is_some() || k == ServiceKind::Stadium || k == ServiceKind::TrashDepot || k == ServiceKind::ParkingEnforcement || k == ServiceKind::University || k.is_landmark());
        if needs_street && !lot.car_access() {
            return Some(format!("{} needs a street out front", crate::names::a(what.label())));
        }
        // [clarity] Solar and wind cost what the lot they fill makes them.
        // [plant-scale] Coal, nuclear and geothermal by their site too.
        let cost = match what {
            Placeable::Plant(k) => k.cost_at(k.size_scale(lot.area())),
            _ => what.cost(),
        };
        if self.economy.treasury < cost {
            return Some(format!("the treasury has {:.0}; {} costs {:.0}", self.economy.treasury, crate::names::a(what.label()), cost));
        }
        None
    }

    /// [hq] The police and the fire brigade each run one headquarters (the
    /// top tier) per city; posts and stations as many as wanted. Why a
    /// second cannot go down, naming the one there is; None otherwise.
    pub fn hq_problem(&self, what: Placeable) -> Option<String> {
        let Placeable::Service(kind @ (ServiceKind::Police | ServiceKind::Fire), Tier::High) = what else { return None };
        let (_, b) = self.buildings.iter().find(|(_, b)| b.service.is_some_and(|s| s.kind == kind && s.tier == Tier::High))?;
        let what = if kind == ServiceKind::Police { "police" } else { "fire" };
        Some(format!("the city has its {what} headquarters already ({}): only one per city; build a {what} station instead", b.name))
    }

    /// Build a service or attraction on a vacant lot, paid from the treasury.
    pub fn build_service(&mut self, lot_id: LotId, kind: ServiceKind, tier: Tier) -> Result<BuildingId, String> {
        let what = Placeable::Service(kind, tier);
        if let Some(problem) = self.placement_problem(what, lot_id) {
            return Err(problem);
        }
        self.take_over_lot(lot_id, kind.tier_label(tier))?;
        self.economy.debit_item(Flow::Construction, what.cost(), Item::Named(kind.tier_label(tier).to_string()));
        let n = self.srng().random_range(0..1000u32);
        let name = kind.name(tier, n);
        let floors = service_floors(kind, tier);
        let parking = match kind {
            ServiceKind::Stadium => [20, 60, 140][tier.index()],
            ServiceKind::Health | ServiceKind::School => [4, 10, 24][tier.index()],
            ServiceKind::University => [10, 24, 50][tier.index()],
            // [attractions] Visitors' car parks.
            ServiceKind::GrandPark | ServiceKind::Zoo | ServiceKind::ConcertHall => 60,
            // (D4) Coaches and cars by the hundred.
            ServiceKind::AmusementPark | ServiceKind::ConventionCentre | ServiceKind::CasinoResort => 90,
            ServiceKind::Pier => 40,
            k if k.is_landmark() => 30,
            _ => [2, 4, 8][tier.index()],
        };
        let id = self.place_special(lot_id, name, kind.jobs(tier), parking, |b| {
            b.service = Some(ServiceSite { kind, tier });
            b.tier = tier;
            b.floors = floors;
        });
        self.services.rubble.retain(|r| r.0 != lot_id);
        // [megacorp] A university founds its campus: an owner, a brand.
        if kind == ServiceKind::University {
            self.found_campus(id);
        }
        // [attractions] The pier's deck, the books.
        if kind.is_landmark() {
            self.landmark_placed(id, kind);
        }
        self.ensure_units(id);
        self.refresh_coverage();
        Ok(id)
    }

    /// Give a station the vehicles its tier runs.
    pub(super) fn ensure_units(&mut self, station: BuildingId) {
        let Some((kind, tier, pos)) = self.buildings.get(station).and_then(|b| b.service.map(|s| (s.kind, s.tier, self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.car_point)))) else { return };
        let Some(uk) = kind.unit_kind() else { return };
        let have = self.services.units.values().filter(|u| u.station == station).count();
        for k in have..kind.units(tier) {
            self.services.units.insert(Unit { kind: uk, station, number: k as u8 + 1, state: UnitState::Idle, car: None, incident: None, pos, heading: 0.0, hospital: None, patrol_until: 0, leaving: false, packing_until: 0 });
        }
    }

    /// Vehicles a station can put on the road with the staff it has.
    pub fn crewed_units(&self, station: BuildingId) -> usize {
        let Some(b) = self.buildings.get(station) else { return 0 };
        let Some(s) = b.service else { return 0 };
        let Some(uk) = s.kind.unit_kind() else { return 0 };
        if b.is_derelict() || !b.operating() {
            return 0;
        }
        s.kind.units(s.tier).min(b.workers.len() / uk.crew())
    }

    /// [build-highlight] What a building was put down as with the Build
    /// window (a service at its tier, a plant, a depot...), or None for
    /// anything that grew on its own.
    pub fn placed_as(&self, id: BuildingId) -> Option<Placeable> {
        let b = self.buildings.get(id)?;
        if let Some(s) = b.service {
            return Some(Placeable::Service(s.kind, s.tier));
        }
        if let Some(k) = b.plant {
            return Some(Placeable::Plant(k));
        }
        if let Some(bat) = &b.battery {
            return Some(Placeable::Battery(bat.kind));
        }
        if self.cargo_hubs.hubs.contains_key(&id) {
            return Some(Placeable::CargoHub);
        }
        if let Some(r) = &b.rail {
            return Some(Placeable::Rail(r.kind));
        }
        if let Some(k) = b.port {
            return Some(Placeable::Dock(k));
        }
        if b.metro {
            return Some(Placeable::MetroStation);
        }
        if b.depot {
            return Some(Placeable::BusDepot);
        }
        if let Some(c) = self.vehicles.city_lots.get(&id) {
            return Some(Placeable::ParkingLot(c.size));
        }
        None
    }
}
