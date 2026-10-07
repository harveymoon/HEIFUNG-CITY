//! Power: plants and the allocation of supply to buildings.

use super::*;

/// [grid-weather] One hour of [`World::grid_day_ahead`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridHour {
    pub hour: u8,
    pub temp_c: f32,
    pub demand_mw: f32,
    pub import_price: f32,
    pub export_price: f32,
}

/// What a row of the plant table is: a plant of a kind or a battery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridAsset {
    Plant(PowerPlantKind),
    Battery(BatteryKind),
}

/// One plant or battery as the Power window's table and the remote API's
/// power section show it: what it puts out, what it costs, what it does to
/// the air and the ears. Read from the world and its history, so the UI
/// and the API cannot disagree.
#[derive(Clone, Debug)]
pub struct PlantRow {
    pub id: BuildingId,
    pub name: String,
    pub asset: GridAsset,
    /// Output now, MW (a battery: its flow, positive delivering, negative
    /// charging).
    pub now_mw: f32,
    /// Nameplate (a battery: its fastest rate), MW.
    pub capacity_mw: f32,
    /// Mean output over the last 24 recorded hours, MW (a battery: what it
    /// delivered).
    pub mean_mw: f32,
    pub upkeep_per_day: f32,
    /// Upkeep spread over the energy of the last 24 hours, per MWh: what
    /// the city pays for each MWh it gets from this plant. `None` when it
    /// made (or gave back) next to nothing.
    pub cost_per_mwh: Option<f32>,
    /// CO2 over a day at the last 24 hours' rate, tonnes.
    pub co2_t_per_day: f32,
    /// Smog at the fence now, `0..1` (the kind's figure times how hard it runs).
    pub air: f32,
    /// Noise at the fence while running, dB(A); `None` while it stands idle.
    pub noise_db: Option<f32>,
    /// Land-value cone: radius, weight (`PowerPlantKind::pollution`).
    pub nuisance: (f32, f32),
    /// A battery's charge and capacity, MWh.
    pub charge_mwh: f32,
    pub capacity_mwh: f32,
    pub derelict: bool,
    /// Still clearing the lot or being built: not on the grid yet.
    pub pending: bool,
    /// [plant-groups] The plants this row stands for (just `id`, or every
    /// member of a Sunfield or Wind Park), their lots and their lot area.
    pub members: Vec<BuildingId>,
    pub lots: Vec<LotId>,
    pub area_m2: f32,
}

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
        if lot.area() < kind.min_area() && self.small_plant_lot_joins(kind, lot_id).is_none() {
            return Err(format!("too small: a {} needs {:.0} m²", kind.label(), kind.min_area()));
        }
        self.power_site(lot_id, kind)?;
        // [clarity] Solar and wind are priced by the lot they fill.
        let cost = kind.build_cost() * kind.size_scale(lot.area());
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; a {} here costs {:.0}", self.economy.treasury, kind.label(), cost));
        }
        self.take_over_lot(lot_id, kind.label())?;
        self.economy.debit_item(Flow::Construction, cost, Item::Named(kind.label().to_string()));
        let name = format!("{} {}", match kind {
            PowerPlantKind::Coal => "Blackridge",
            PowerPlantKind::Solar => "Solar Array",
            PowerPlantKind::Wind => "Westerly",
            PowerPlantKind::Nuclear => "Calder",
            PowerPlantKind::Hydro => "Millrace",
            PowerPlantKind::Tidal => "Tiderace",
            PowerPlantKind::Geothermal => "Hotwell",
        }, self.rng.random_range(1..9));
        let id = self.place_special(lot_id, name, kind.jobs(), kind.jobs() / 2, |b| b.plant = Some(kind));
        self.clear_for_light_works(id, kind.merges_with_neighbours());
        self.refresh_power();
        Ok(id)
    }

    /// Place a grid battery on a lot, paying from the treasury; it starts
    /// empty and fills from the next surplus. Like a plant, it buys out
    /// whatever grew there and works once the old building is cleared.
    pub fn build_battery(&mut self, lot_id: LotId, kind: BatteryKind) -> Result<BuildingId, String> {
        let what = crate::services::Placeable::Battery(kind);
        if let Some(problem) = self.placement_problem(what, lot_id) {
            return Err(problem);
        }
        self.take_over_lot(lot_id, kind.label())?;
        self.economy.debit_item(Flow::Construction, kind.build_cost(), Item::Named(kind.label().to_string()));
        let name = format!("{} {}", ["Ampere Cell", "Volta Bank", "Gridvault"][kind as usize], self.rng.random_range(1..9));
        let id = self.place_special(lot_id, name, kind.jobs(), 0, |b| b.battery = Some(Battery::new(kind)));
        self.clear_for_light_works(id, true);
        self.refresh_power();
        Ok(id)
    }

    /// Solar fields, wind farms and battery yards are light works: posts,
    /// masts and cabinets on a levelled yard. What stood on the lot is
    /// cleared before they go in, at once, instead of standing as a
    /// demolition site for a day; that site drew the old building where the
    /// player had just bought a solar farm (it read as "a green building")
    /// while the plant already fed the grid. Heavy plants keep the clearing.
    fn clear_for_light_works(&mut self, id: BuildingId, light: bool) {
        if light && self.buildings.get(id).is_some_and(|b| b.demolition.is_some()) {
            self.finish_demolition(id);
        }
    }

    /// [clarity] How much of its kind's nameplate a plant is: its lot's
    /// area over the reference for solar and wind, 1 for the others.
    pub fn plant_scale(&self, b: &Building) -> f32 {
        let Some(k) = b.plant else { return 1.0 };
        self.lots.get(b.lot).map_or(1.0, |l| k.size_scale(l.area()))
    }

    /// [clarity] A plant's own nameplate output (scaled by its lot).
    pub fn plant_capacity_mw(&self, b: &Building) -> f32 {
        // [plant-groups] times the shared-connection bonus of its group.
        b.plant.map_or(0.0, |k| k.capacity_mw() * self.plant_scale(b) * self.plant_group_bonus(b))
    }

    /// [clarity] A plant's own daily upkeep (scaled by its lot).
    pub fn plant_upkeep(&self, b: &Building) -> f32 {
        b.plant.map_or(0.0, |k| k.upkeep_per_day() * self.plant_scale(b))
    }

    /// [clarity] What a plant of `kind` on `lot` would cost to build.
    pub fn plant_cost_on(&self, kind: PowerPlantKind, lot: LotId) -> f32 {
        kind.build_cost() * self.lots.get(lot).map_or(1.0, |l| kind.size_scale(l.area()))
    }

    /// Every plant and battery as a table row, plants in kind order then
    /// batteries, each group by name.
    pub fn plant_rows(&self) -> Vec<PlantRow> {
        let mut rows: Vec<PlantRow> = Vec::new();
        for (id, b) in self.buildings.iter() {
            let log = self.economy.history.plant(id);
            let mean = log.map_or(0.0, |l| l.mean_mw(24));
            let pending = b.construction.is_some();
            if let Some(k) = b.plant {
                let now = self.plant_dispatched_mw(b);
                let mean = if log.is_some_and(|l| !l.hours.is_empty()) { mean } else { now };
                rows.push(PlantRow {
                    id,
                    name: b.name.clone(),
                    asset: GridAsset::Plant(k),
                    now_mw: now,
                    capacity_mw: self.plant_capacity_mw(b),
                    mean_mw: mean,
                    upkeep_per_day: self.plant_upkeep(b),
                    cost_per_mwh: (mean > 0.05).then(|| self.plant_upkeep(b) / (mean * 24.0)),
                    co2_t_per_day: mean * 24.0 * k.co2_t_per_mwh(),
                    air: k.air_index() * (now / self.plant_capacity_mw(b).max(0.01)).clamp(0.0, 1.0),
                    noise_db: (now > 0.0).then(|| k.noise_db()),
                    nuisance: k.pollution(),
                    charge_mwh: 0.0,
                    capacity_mwh: 0.0,
                    derelict: b.is_derelict(),
                    pending,
                    members: vec![id],
                    lots: vec![b.lot],
                    area_m2: self.lots.get(b.lot).map_or(0.0, |l| l.area()),
                });
            } else if let Some(bat) = &b.battery {
                let k = bat.kind;
                let given = log.map_or(0.0, |l| { let n = l.hours.len().min(24); l.hours.iter().skip(l.hours.len() - n).map(|x| x.max(0.0)).sum::<f32>() });
                rows.push(PlantRow {
                    id,
                    name: b.name.clone(),
                    asset: GridAsset::Battery(k),
                    now_mw: bat.flow_mw,
                    capacity_mw: k.max_mw(),
                    mean_mw: given / 24.0,
                    upkeep_per_day: k.upkeep_per_day(),
                    cost_per_mwh: (given > 1.0).then(|| k.upkeep_per_day() / given),
                    co2_t_per_day: 0.0,
                    air: 0.0,
                    noise_db: (bat.flow_mw.abs() > 0.05).then(|| k.noise_db()),
                    nuisance: (0.0, 0.0),
                    charge_mwh: bat.charge_mwh,
                    capacity_mwh: k.capacity_mwh(),
                    derelict: b.is_derelict(),
                    pending: pending || b.demolition.is_some(),
                    members: vec![id],
                    lots: vec![b.lot],
                    area_m2: self.lots.get(b.lot).map_or(0.0, |l| l.area()),
                });
            }
        }
        let mut rows = self.fold_group_rows(rows);
        let order = |a: &GridAsset| match a {
            GridAsset::Plant(k) => k.index(),
            GridAsset::Battery(k) => 100 + *k as usize,
        };
        rows.sort_by(|a, b| order(&a.asset).cmp(&order(&b.asset)).then(a.name.cmp(&b.name)));
        rows
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

    /// What a plant puts on the grid this hour: nameplate, times what the
    /// weather and the hour allow, times what its site supports. Zero for
    /// a derelict plant or any other building.
    pub fn plant_output_mw(&self, b: &Building) -> f32 {
        let Some(kind) = b.plant else { return 0.0 };
        // Derelict, or the lot still being cleared for it.
        if b.is_derelict() || b.demolition.is_some() {
            return 0.0;
        }
        let site = self.power_site(b.lot, kind).unwrap_or(0.0);
        self.plant_capacity_mw(b) * self.plant_availability(b) * site
    }

    /// [grid-weather] The wind and sun a plant at `p` would get: the map's
    /// wind field and sun factor there ([`crate::weather::SiteResource`]).
    pub fn site_resource_at(&self, p: Vec2) -> crate::weather::SiteResource {
        crate::weather::SiteResource::at(&self.landscape, p)
    }

    /// [grid-weather] What a plant's site gives it: the resource at its
    /// lot's centre for wind and solar, open ground for the rest.
    pub fn plant_resource(&self, b: &Building) -> crate::weather::SiteResource {
        match b.plant {
            Some(PowerPlantKind::Wind | PowerPlantKind::Solar) => self.lots.get(b.lot).map_or_else(Default::default, |l| self.site_resource_at(l.center)),
            _ => Default::default(),
        }
    }

    /// [grid-weather] Share of a plant's nameplate the weather, the hour and
    /// its site allow right now (before the hydro and geothermal site
    /// factor of [`Self::power_site`]).
    pub fn plant_availability(&self, b: &Building) -> f32 {
        let Some(kind) = b.plant else { return 0.0 };
        kind.availability(&self.weather, self.clock.day_fraction(), self.plant_resource(b))
    }

    /// [grid-weather] Share of nameplate a plant of `kind` on a lot at `p`
    /// would make right now, and on an ordinary day's average (for the
    /// placement preview): wind on the map's wind there, solar on the sun.
    pub fn kind_availability_at(&self, kind: PowerPlantKind, p: Vec2) -> f32 {
        kind.availability(&self.weather, self.clock.day_fraction(), self.site_resource_at(p))
    }

    /// [grid-weather] The hour of the day now, `0..24`, as the grid reads it.
    pub fn grid_hour(&self) -> f32 {
        self.clock.day_fraction() * 24.0
    }

    /// [grid-weather] The next 24 hours on the grid from the top of this
    /// hour, if the weather holds (the temperature follows the day's swing):
    /// the city's demand and the regional prices. For the Power window's
    /// sparkline and the remote API.
    pub fn grid_day_ahead(&self) -> Vec<GridHour> {
        let base = crate::power::DemandBase::of(self.buildings.values());
        let now = self.clock.hour() as u32;
        (0..24)
            .map(|k| {
                let hour = (now + k) % 24;
                let h = hour as f32;
                let temp = if k == 0 { self.weather.temp_c } else { self.weather.temp_at(h) };
                GridHour { hour: hour as u8, temp_c: temp, demand_mw: base.at(h, temp), import_price: crate::power::import_price(h, temp), export_price: crate::power::export_price(h, temp) }
            })
            .collect()
    }

    /// What a plant actually ran at this hour: its output less its share of
    /// any curtailment (surplus with nowhere to go; every plant turns down
    /// by the same share). This is what the plant table and its record show.
    pub fn plant_dispatched_mw(&self, b: &Building) -> f32 {
        let p = &self.economy.power;
        let share = if p.supply_mw > 0.0 { ((p.supply_mw - p.curtailed_mw) / p.supply_mw).clamp(0.0, 1.0) } else { 1.0 };
        self.plant_output_mw(b) * share
    }

    /// Whether a battery on this building takes part in the grid: built,
    /// not still clearing the lot, not run down.
    fn battery_online(b: &Building) -> bool {
        b.battery.is_some() && b.construction.is_none() && b.demolition.is_none() && !b.is_derelict()
    }

    /// The hour on the grid: [`Self::dispatch_power`] with the batteries
    /// run, the money booked and the dark hours counted.
    pub(super) fn allocate_power(&mut self) {
        self.dispatch_power(true);
    }

    /// The grid redone in the middle of an hour (a plant or battery was
    /// placed): who is powered is updated, but no battery moves energy, no
    /// money changes hands and no dark hour is counted; the hour is booked
    /// once, on the hour.
    pub(super) fn refresh_power(&mut self) {
        self.dispatch_power(false);
    }

    /// Share the hour's supply out. The plants' output meets demand; a
    /// surplus charges the batteries, then is sold to the regional grid up
    /// to its export line; a shortfall drains the batteries, then is bought
    /// from the regional grid up to its import line ([`crate::power::dispatch`]).
    /// What is left is shared highest tier first, so cheap districts brown
    /// out first. Buildings whose power state changes are redrawn.
    fn dispatch_power(&mut self, hourly: bool) {
        let mut supply: f32 = 0.0;
        let mut plants = 0;
        let mut by_kind = [0.0f32; 7];
        for b in self.buildings.values() {
            if let Some(kind) = b.plant {
                if !b.is_derelict() {
                    let mw = self.plant_output_mw(b);
                    supply += mw;
                    by_kind[kind.index()] += mw;
                    plants += 1;
                }
            }
        }
        self.economy.power.supply_by_kind = by_kind;
        // [grid-weather] Demand follows the hour and the weather.
        let (hour, temp) = (self.grid_hour(), self.weather.temp_c);
        let demand: f32 = self.buildings.values().map(|b| crate::power::demand_at(b, hour, temp)).sum::<f32>() + 0.0; // sum of nothing is -0.0
        let (import_price, export_price) = (crate::power::import_price(hour, temp), crate::power::export_price(hour, temp));
        // Batteries, in slot order (deterministic); each takes or gives the
        // same share of its own headroom.
        let mut batteries = 0;
        let (mut room, mut available, mut capacity) = (0.0f32, 0.0f32, 0.0f32);
        for b in self.buildings.values().filter(|b| Self::battery_online(b)) {
            let bat = b.battery.as_ref().unwrap();
            room += bat.room_mw();
            available += bat.available_mw();
            capacity += bat.kind.capacity_mwh();
            batteries += 1;
        }
        let d = crate::power::dispatch(supply, demand, room, available);
        let mut stored = 0.0f32;
        let mut relit = Vec::new();
        for (id, b) in self.buildings.iter_mut() {
            let shown = b.battery.as_ref().map(|bat| bat.status());
            if !Self::battery_online(b) {
                if let Some(bat) = b.battery.as_mut() {
                    bat.flow_mw = 0.0;
                }
                if shown != b.battery.as_ref().map(|bat| bat.status()) {
                    relit.push(id);
                }
                continue;
            }
            let bat = b.battery.as_mut().unwrap();
            let flow = if d.charge_mw > 0.0 {
                -bat.room_mw() * d.charge_share
            } else {
                bat.available_mw() * d.discharge_share
            };
            if hourly {
                bat.run_hour(flow);
            } else {
                bat.flow_mw = flow;
            }
            stored += bat.charge_mwh;
            // The status lights show fifths of charge and the flow: redraw
            // when they change, not every hour.
            if shown != Some(bat.status()) {
                relit.push(id);
            }
        }
        for id in relit {
            self.events.push(SimEvent::BuildingChanged(id));
        }
        let imported = d.imported_mw;
        if hourly {
            self.economy.power.import_cost_today += imported * import_price;
            self.economy.power.export_income_today += d.exported_mw * export_price;
        }
        {
            let p = &mut self.economy.power;
            p.import_price = import_price;
            p.export_price = export_price;
            p.exported_mw = d.exported_mw;
            p.curtailed_mw = d.curtailed_mw;
            p.batteries = batteries;
            p.battery_capacity_mwh = capacity;
            p.battery_charge_mwh = stored + 0.0;
            p.battery_in_mw = d.charge_mw;
            p.battery_out_mw = d.discharge_mw;
        }
        // A surplus serves everyone (the rest went to the batteries and the
        // export line); a shortfall shares what came in. The thousandth of a
        // megawatt keeps rounding from browning out the last building.
        let delivered = supply + d.discharge_mw + imported;
        let mut remaining = delivered.min(demand + 1e-3);
        let mut order: Vec<(BuildingId, f32)> = self.buildings.iter().map(|(id, b)| (id, crate::power::demand_at(b, hour, temp))).collect();
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
                if hourly {
                    b.unpowered_hours_today = b.unpowered_hours_today.saturating_add(1);
                }
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
        p.served = if demand > 0.0 { (delivered / demand).min(1.0) } else { 1.0 };
        p.plants = plants;
        p.unpowered_buildings = unpowered;
        if !hourly {
            return;
        }
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
                // [concerns] Every dark hour counts (once a day each).
                self.concerns.said(tick, cid, c, crate::concerns::Concern::Power, 0.4, format!("Sat in the dark at {name}. No power."));
                if self.rng.random_bool(0.04) {
                    c.thought = "The power is out again.".to_string();
                    c.remember(tick, format!("Sat in the dark at {name}. No power."));
                }
            }
        }
    }

    /// The lots of the plant field `id` belongs to: `id`'s own lot plus every
    /// finished plant of the same kind reachable through parcels that
    /// touch ([`super::plant_groups::lots_touch`]: an edge, or a lot line a
    /// couple of metres wide, never a road), sorted by lot id (so
    /// the first is the same for every member: the field's anchor). Only
    /// kinds that [merge](PowerPlantKind::merges_with_neighbours) form
    /// fields; for anything else, a site under construction or demolition,
    /// or a plant with no neighbour, the result is just its own lot (empty
    /// for a building that does not exist).
    pub fn plant_cluster(&self, id: BuildingId) -> Vec<LotId> {
        let Some(b) = self.buildings.get(id) else { return Vec::new() };
        let finished = |b: &Building| b.construction.is_none() && b.demolition.is_none();
        let kind = match b.plant {
            Some(k) if k.merges_with_neighbours() && finished(b) => k,
            _ => return vec![b.lot],
        };
        let candidates: Vec<LotId> = self.buildings.values().filter(|o| o.plant == Some(kind) && finished(o) && self.lots.contains_key(o.lot)).map(|o| o.lot).collect();
        let mut field = vec![b.lot];
        let mut next = 0;
        while next < field.len() {
            let here = &self.lots[field[next]].polygon;
            next += 1;
            for &l in &candidates {
                if !field.contains(&l) && super::plant_groups::lots_touch(here, &self.lots[l].polygon, &self.roads) {
                    field.push(l);
                }
            }
        }
        field.sort();
        field
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zoning::LandUse;

    fn square(x0: f32, y0: f32, s: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s)]
    }

    /// A town with homes, a coal plant and a battery: the coal plant's
    /// surplus charges the battery, the rest is exported and paid for at
    /// midnight; with the plant run down the battery carries the town before
    /// anything is imported. Per-plant records follow, all of it survives a
    /// save and load, and a save without any of it loads.
    #[test]
    fn batteries_soak_up_surplus_exports_pay_and_shortfalls_drain_first() {
        let mut w = World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
        w.economy.treasury = 1e6;
        let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
        let res: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).collect();
        let big = ind.iter().copied().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).unwrap();
        let coal = w.build_power(big, PowerPlantKind::Coal).expect("coal plant");
        let spot = res.iter().copied().find(|&l| w.lots[l].area() >= BatteryKind::Medium.min_area()).expect("a lot for a battery");
        let bat = w.build_battery(spot, BatteryKind::Medium).expect("battery placed");
        assert!(w.build_battery(spot, BatteryKind::Small).unwrap_err().contains("bulldoze"), "a battery is the city's own");
        assert_eq!(demand_mw(&w.buildings[bat]), 0.0);
        for &l in res.iter().filter(|&&l| l != spot).take(4) {
            w.build_on(l);
        }
        let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in ids {
            w.finish_construction(id);
        }
        w.buildings[bat].demolition = None;
        assert_eq!(w.economy.power.export_income_today, 0.0, "placing books no hour");
        // An hour of surplus: the battery takes its fill rate, the rest goes out.
        w.allocate_power();
        let p = w.economy.power.clone();
        assert_eq!(p.batteries, 1);
        assert!(p.supply_mw > p.demand_mw);
        assert!((p.battery_in_mw - BatteryKind::Medium.max_mw()).abs() < 1e-3, "{p:?}");
        assert!(p.exported_mw > 0.0 && p.imported_mw == 0.0 && p.served >= 1.0);
        let charge = w.buildings[bat].battery.as_ref().unwrap().charge_mwh;
        assert!((charge - BatteryKind::Medium.max_mw() * crate::power::BATTERY_CHARGE_EFF).abs() < 1e-3);
        assert!(p.export_price > 0.0 && p.export_price < p.import_price);
        assert!((p.export_income_today - p.exported_mw * p.export_price).abs() < 1e-2);
        // Midnight pays the exports into the ledger.
        w.daily_update();
        assert!((w.economy.ledger.yesterday_of(Flow::GridExport) - p.export_income_today).abs() < 1e-2, "the day closed with the export income on its books");
        assert_eq!(w.economy.power.export_income_today, 0.0);
        // The plant runs down: the battery carries the load before any import.
        w.buildings[coal].condition = 0.1;
        w.allocate_power();
        let p = w.economy.power.clone();
        assert!(p.battery_out_mw > 0.0, "{p:?}");
        assert_eq!(p.exported_mw, 0.0);
        if p.demand_mw > p.battery_out_mw + 1e-3 {
            assert!(p.imported_mw > 0.0, "the rest is imported");
        } else {
            assert_eq!(p.imported_mw, 0.0, "nothing bought while the battery covers it");
        }
        assert!(w.buildings[bat].battery.as_ref().unwrap().charge_mwh < charge);
        // Records per plant and battery.
        w.record_hour();
        let log = w.economy.history.plant(bat).expect("the battery has a record");
        assert_eq!(log.level.len(), 1);
        assert!(log.hours[0] > 0.0, "discharging shows as output");
        assert!(w.economy.history.plant(coal).is_some());
        let h = w.economy.history.hours.back().unwrap();
        assert!(h.battery_out_mw > 0.0 && h.battery_capacity_mwh == BatteryKind::Medium.capacity_mwh());
        // Save and load keep it; a save without the battery field loads.
        let ron = w.to_ron().unwrap();
        let back = World::from_ron(&ron).unwrap();
        assert_eq!(back.buildings[bat].battery, w.buildings[bat].battery);
        assert_eq!(back.economy.history.plants, w.economy.history.plants);
        let old = ron.replace("battery: None,", "");
        let old = World::from_ron(&old).expect("a save without the battery field loads");
        assert_eq!(old.buildings.values().filter(|b| b.battery.is_some()).count(), 1);
    }
}
