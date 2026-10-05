//! Power: plants and the allocation of supply to buildings.

use super::*;

impl World {
    /// Place a power plant on a lot, paying from the treasury. Whatever grew
    /// there is bought out and pulled down: the city outranks the market.
    pub fn build_power(&mut self, lot_id: LotId, kind: PowerPlantKind) -> Result<BuildingId, String> {
        let lot = self.lots.get(lot_id).ok_or("no such lot")?;
        if let Some(b) = lot.building.and_then(|b| self.buildings.get(b)) {
            if Self::is_civic_structure(b) {
                return Err(format!("{} stands there; bulldoze it first", b.name));
            }
        }
        if lot.landlocked {
            return Err("that land has no road access".into());
        }
        if self.under_facility(lot.center) {
            return Err("that ground belongs to a station or yard".into());
        }
        if !kind.allowed_on(lot.land_use) {
            return Err(format!("a {} cannot go on {} land", kind.label(), lot.land_use.label().to_lowercase()));
        }
        if lot.area() < kind.min_area() {
            return Err(format!("too small: a {} needs {:.0} m²", kind.label(), kind.min_area()));
        }
        self.power_site(lot_id, kind)?;
        if self.economy.treasury < kind.build_cost() {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, kind.label(), kind.build_cost()));
        }
        self.take_over_lot(lot_id, kind.label())?;
        self.economy.debit(Flow::Construction, kind.build_cost());
        let name = format!("{} {}", match kind {
            PowerPlantKind::Coal => "Blackridge",
            PowerPlantKind::Solar => "Sunfield",
            PowerPlantKind::Wind => "Westerly",
            PowerPlantKind::Nuclear => "Calder",
            PowerPlantKind::Hydro => "Millrace",
            PowerPlantKind::Tidal => "Tiderace",
            PowerPlantKind::Geothermal => "Hotwell",
        }, self.rng.random_range(1..9));
        let id = self.place_special(lot_id, name, kind.jobs(), kind.jobs() / 2, |b| b.plant = Some(kind));
        self.allocate_power();
        Ok(id)
    }

    /// Can a plant of `kind` stand on this lot? `Ok` carries the share of
    /// nameplate the site supports: hydro scales with the current, geothermal
    /// with the hot spot's heat, everything else 1. The Buildings tool calls
    /// this to show where the water and hot-ground plants may go.
    pub fn power_site(&self, lot_id: LotId, kind: PowerPlantKind) -> Result<f32, String> {
        use crate::power::PlantSite;
        let lot = self.lots.get(lot_id).ok_or("no such lot")?;
        match kind.site() {
            PlantSite::Anywhere => Ok(1.0),
            PlantSite::River => {
                let (_, current, _) = self.landscape.river_beside(&lot.polygon, 30.0).ok_or("a hydro plant needs a river or stream beside the lot")?;
                if current < crate::landscape::MIN_HYDRO_CURRENT {
                    return Err(format!("the water here is too slow ({current:.1} m/s); hydro needs {:.1} m/s", crate::landscape::MIN_HYDRO_CURRENT));
                }
                Ok((current / 1.6).clamp(0.5, 1.4))
            }
            PlantSite::Coast => {
                if self.landscape.on_coast(&lot.polygon, 40.0) {
                    Ok(1.0)
                } else {
                    Err("a tidal plant must stand on the sea shore".into())
                }
            }
            PlantSite::HotSpot => self.landscape.hot_spot_at(lot.center).map(|h| h.heat).ok_or_else(|| "no hot ground under this lot: look for the steaming vents".to_string()),
        }
    }

    /// Share the hour's supply out: plants first, then what the regional
    /// grid will sell; the rich are served first, so cheap districts brown
    /// out first. Buildings whose power state changes are redrawn.
    pub(super) fn allocate_power(&mut self) {
        let day = self.clock.day_fraction();
        let mut supply: f32 = 0.0;
        let mut plants = 0;
        for b in self.buildings.values() {
            if let Some(kind) = b.plant {
                if !b.is_derelict() {
                    let site = self.power_site(b.lot, kind).unwrap_or(0.0);
                    supply += kind.capacity_mw() * kind.availability(&self.weather, day) * site;
                    plants += 1;
                }
            }
        }
        let demand: f32 = self.buildings.values().map(demand_mw).sum::<f32>() + 0.0; // sum of nothing is -0.0
        let shortfall = (demand - supply).max(0.0);
        let imported = shortfall.min(GRID_IMPORT_MW);
        self.economy.power.import_cost_today += imported * GRID_IMPORT_PRICE;
        let mut remaining = supply + imported;
        let mut order: Vec<(BuildingId, f32)> = self.buildings.iter().map(|(id, b)| (id, demand_mw(b))).collect();
        order.sort_by(|a, b| {
            let ta = self.buildings[a.0].tier;
            let tb = self.buildings[b.0].tier;
            tb.cmp(&ta).then(b.1.total_cmp(&a.1))
        });
        let mut unpowered = 0;
        let mut changed = Vec::new();
        for (id, need) in order {
            let b = &mut self.buildings[id];
            let powered = if need <= 0.0 {
                1.0
            } else if remaining >= need {
                remaining -= need;
                1.0
            } else {
                let p = remaining / need;
                remaining = 0.0;
                p
            };
            let before = (b.powered * 2.0).round();
            b.powered = powered;
            if powered < 0.6 {
                b.unpowered_hours_today = b.unpowered_hours_today.saturating_add(1);
                unpowered += 1;
            }
            if (powered * 2.0).round() != before {
                changed.push(id);
            }
        }
        for id in changed {
            self.events.push(SimEvent::BuildingChanged(id));
        }
        let p = &mut self.economy.power;
        p.supply_mw = supply;
        p.demand_mw = demand;
        p.imported_mw = imported;
        p.served = if demand > 0.0 { ((supply + imported) / demand).min(1.0) } else { 1.0 };
        p.plants = plants;
        p.unpowered_buildings = unpowered;
        if p.served < 0.98 {
            p.brownout_hours_today += 1;
        }
        // People in the dark notice.
        let tick = self.clock.tick;
        let ids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in ids {
            let Some(inside) = self.citizens[cid].inside else { continue };
            let powered = self.buildings.get(inside).map_or(1.0, |b| b.powered);
            if powered < 0.6 {
                let name = self.buildings[inside].name.clone();
                let c = &mut self.citizens[cid];
                c.mood = (c.mood - 0.012).max(0.0);
                if self.rng.random_bool(0.04) {
                    c.thought = "The power is out again.".to_string();
                    c.remember(tick, format!("Sat in the dark at {name}. No power."));
                }
            }
        }
    }
}
