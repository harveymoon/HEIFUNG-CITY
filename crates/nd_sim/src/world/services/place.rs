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
    }

    /// Put down `what` on a lot (rail pieces at the lot's centre).
    pub fn place_on_lot(&mut self, what: Placeable, lot: LotId) -> Result<BuildingId, String> {
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
        // Whatever grew there is bought out; another civic structure is not.
        if let Some(b) = lot.building.and_then(|b| self.buildings.get(b)) {
            if Self::is_civic_structure(b) {
                return Some(format!("{} stands there; bulldoze it first", b.name));
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
            return Some(format!("that is park land: unzone it before putting a {} there", what.label()));
        }
        // [plant-groups] A small lot may join the solar or wind farm next door.
        // [plant-scale] A site on unzoned ground (no lot yet) too.
        let joins = matches!(what, Placeable::Plant(k) if lot.area() < k.min_area() && self.small_plant_site_joins(k, &lot.polygon, lot_id).is_some());
        if lot.area() < what.min_area() && !joins {
            return Some(format!("too small: a {} needs {:.0} m² (this lot is {:.0})", what.label(), what.min_area(), lot.area()));
        }
        // [plant-scale] A heavy plant is not put on a strip along the kerb.
        let thick = crate::zoning::site_thickness(&lot.polygon);
        if thick < what.min_thickness() {
            return Some(format!("too narrow: a {} needs ground about {:.0} m deep and wide (this is a strip {:.0} m across)", what.label(), what.min_thickness() * 2.0, thick * 2.0));
        }
        if let Placeable::Plant(k) = what {
            if let Err(e) = self.power_site_on(lot, k) {
                return Some(e);
            }
        }
        let needs_street = matches!(what, Placeable::BusDepot | Placeable::ParkingLot(_) | Placeable::CargoHub) || matches!(what, Placeable::Service(k, _) if k.unit_kind().is_some() || k == ServiceKind::Stadium || k == ServiceKind::TrashDepot || k == ServiceKind::ParkingEnforcement || k == ServiceKind::University);
        if needs_street && !lot.car_access() {
            return Some(format!("a {} needs a street out front", what.label()));
        }
        // [clarity] Solar and wind cost what the lot they fill makes them.
        // [plant-scale] Coal, nuclear and geothermal by their site too.
        let cost = match what {
            Placeable::Plant(k) => k.cost_at(k.size_scale(lot.area())),
            _ => what.cost(),
        };
        if self.economy.treasury < cost {
            return Some(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, what.label(), cost));
        }
        None
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
