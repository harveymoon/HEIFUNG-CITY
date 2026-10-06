//! Industry on the land: which sub-zone a new industrial lot takes, the
//! player's bias for an area, the ground industry fouls and the land value
//! it costs, the logistics trade, and the truck and bus lots where idle
//! fleets park.
//!
//! Hooks elsewhere (kept to one line each):
//! - `develop.rs`: `build_on` asks [`World::pick_industrial_business`];
//!   `splat_building` calls [`World::splat_industry`]; strip 3 calls
//!   [`World::splat_contamination`]; rents and tiers read
//!   [`World::lot_value`] (land value as the lot's land use feels it).
//! - `mod.rs`: `tick` runs [`World::industry_hourly`] after the hourly
//!   economy and [`World::industry_daily`] after the daily update; arrived
//!   trucks go through [`World::truck_arrived`].
//! - `market.rs`: `ship` sends the truck from [`World::fleet_origin`].
//! - `buses.rs`: a depot's room grows by [`World::depot_overflow`].

use super::*;
use crate::ground::{self, GroundSample};
use crate::industry::{self, IndustryBias, IndustryKind, IndustrySite};
use crate::ports::PortKind;

/// A truck that has delivered parks at a truck and bus lot with room
/// within this distance, else drives back to the business it came from.
pub const FLEET_REACH: f32 = 700.0;
/// ... if that business is within this distance; otherwise it is gone.
pub const RETURN_REACH: f32 = 1500.0;
/// A business within this distance of a lot with parked trucks sends its
/// loads out in one of those trucks.
pub const FLEET_SHARE_REACH: f32 = 320.0;
/// Ground per truck bay, square metres.
pub const FLEET_BAY_M2: f32 = 90.0;
/// A bus depot may park its overflow at truck and bus lots this close.
pub const DEPOT_OVERFLOW_REACH: f32 = 450.0;
/// Handling fees a container yard or fleet lot earns per filled job-hour.
pub const HANDLING_PER_JOB_HOUR: f32 = 2.0;
/// Fee per parked truck per hour.
pub const BAY_FEE_PER_HOUR: f32 = 0.5;
/// Contamination this strong takes this much land value (and scales down).
pub const CONTAMINATION_VALUE_PENALTY: f32 = 0.6;
/// The most area biases kept; the oldest go first.
pub const MAX_INDUSTRY_BIASES: usize = 256;

/// Where the player sets an industry bias: one lot, or an area.
#[derive(Clone, Debug, PartialEq)]
pub enum IndustryArea {
    Lot(LotId),
    Polygon(Vec<Vec2>),
}

fn near(d: f32, full: f32, none: f32) -> f32 {
    1.0 - ((d - full) / (none - full)).clamp(0.0, 1.0)
}

/// An empty truck on its way to park: it carries nothing to its own lot.
pub fn is_return_trip(c: &Cargo) -> bool {
    c.amount == 0.0 && c.price == 0.0 && c.from == c.to
}

impl World {
    // ── the ground ──────────────────────────────────────────────────────────

    /// The static ground (fertility, oil, rock, sand, water) at a point.
    pub fn ground_at(&self, p: Vec2) -> GroundSample {
        ground::sample(&self.landscape, p)
    }

    /// The ground over a lot, averaged over its outline.
    pub fn lot_ground(&self, lot: LotId) -> Option<GroundSample> {
        let l = self.lots.get(lot)?;
        Some(ground::sample_area(&self.landscape, l.center, &l.polygon))
    }

    /// Land value as the lot's own land use feels it: homes by a power plant
    /// lose more than the grid shows, industry less.
    pub fn lot_value(&self, lot: LotId) -> f32 {
        self.lots.get(lot).map_or(0.3, |l| self.land_value.for_use(l.center, l.land_use))
    }

    // ── the player's bias ───────────────────────────────────────────────────

    /// The bias covering `p`: `Some(Some(kind))` forces a kind, `Some(None)`
    /// says "let the ground decide" over an older bias, `None` no bias.
    pub fn industry_bias_at(&self, p: Vec2) -> Option<Option<IndustryKind>> {
        self.industry_bias.iter().rev().find(|b| b.polygon.len() >= 3 && point_in_polygon(p, &b.polygon)).map(|b| b.kind)
    }

    /// Set (or with `None` clear) the industry sub-zone of one lot or of
    /// every lot in an area. New industrial buildings there take the kind
    /// whatever the ground says; standing industry of another kind retools
    /// on the spot (stock dropped, money kept). The bias belongs to the
    /// ground, not to the parcel, so it survives the lots being recut.
    /// Returns the number of buildings that retooled.
    pub fn set_industry_bias(&mut self, area: IndustryArea, kind: Option<IndustryKind>) -> Result<usize, String> {
        let polygon = match area {
            IndustryArea::Lot(l) => self.lots.get(l).ok_or("no such lot")?.polygon.clone(),
            IndustryArea::Polygon(p) => p,
        };
        if polygon.len() < 3 {
            return Err("an industry area needs at least three corners".into());
        }
        self.industry_bias.push(IndustryBias { polygon: polygon.clone(), kind });
        if self.industry_bias.len() > MAX_INDUSTRY_BIASES {
            let extra = self.industry_bias.len() - MAX_INDUSTRY_BIASES;
            self.industry_bias.drain(..extra);
        }
        let Some(kind) = kind else { return Ok(0) };
        let ids: Vec<BuildingId> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Industrial && b.plant.is_none() && !b.depot && b.rail.is_none() && b.port.is_none())
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind.industry() != Some(kind) && z.kind != BusinessKind::Parking))
            .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &polygon)))
            .map(|(id, _)| id)
            .collect();
        let mut n = 0;
        for id in ids {
            let lot = self.buildings[id].lot;
            let Some(site) = self.industry_site(lot) else { continue };
            let roll = self.rng.random::<f32>();
            let Some(bk) = industry::pick_business(&industry::business_weights(kind, &site), roll) else { continue };
            let b = &mut self.buildings[id];
            let jobs = b.jobs;
            let old = b.business.take();
            let mut bz = Business::new(bk, jobs);
            if let Some(old) = old {
                bz.money = old.money;
                bz.closed = old.closed;
                bz.broke_days = old.broke_days;
            }
            b.business = Some(bz);
            b.floors = b.floors.min(kind.max_floors()).max(1);
            b.seed = b.seed.wrapping_add(1);
            if b.construction.is_none() {
                b.recompute_capacity();
            }
            n += 1;
            self.events.push(SimEvent::BuildingChanged(id));
        }
        Ok(n)
    }

    /// The industry sub-zone of a building, from its business.
    pub fn industry_of(&self, b: BuildingId) -> Option<IndustryKind> {
        self.buildings.get(b)?.business.as_ref()?.kind.industry()
    }

    // ── choosing ────────────────────────────────────────────────────────────

    /// What the industry choice weighs on a lot.
    pub fn industry_site(&self, lot_id: LotId) -> Option<IndustrySite> {
        let lot = self.lots.get(lot_id)?;
        let c = lot.center;
        let nearest = |ids: &mut dyn Iterator<Item = BuildingId>| -> f32 { ids.filter_map(|b| self.building_lot(b)).map(|l| l.center.distance(c)).fold(f32::INFINITY, f32::min) };
        let ports = self.ports.working(PortKind::ShippingPort);
        let port = near(nearest(&mut ports.iter().copied()), 80.0, 420.0);
        let yard = near(nearest(&mut self.yards.iter().copied()), 80.0, 360.0);
        let depot = near(nearest(&mut self.buildings.iter().filter(|(_, b)| b.depot && !b.is_derelict()).map(|(id, _)| id)), 60.0, 350.0);
        let mut hw = f32::INFINITY;
        for (sid, seg) in self.roads.alive_segments() {
            if seg.is_highway() {
                let (a, b) = self.roads.segment_points(sid);
                hw = hw.min(crate::geom::project_on_segment(c, a, b).1.distance(c));
            }
        }
        let highway = near(hw, 30.0, 200.0);
        let mut neighbours = Vec::new();
        let mut fleet_lot_near = false;
        for b in self.buildings.values() {
            if b.lot == lot_id {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            let d = l.center.distance(c);
            let kind = match (&b.business, b.plant) {
                (Some(z), _) => z.kind.industry(),
                // Coal smoke and reactor fears read as heavy industry.
                (None, Some(PowerPlantKind::Coal)) | (None, Some(PowerPlantKind::Nuclear)) => Some(IndustryKind::Refinery),
                _ => None,
            };
            if b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::FleetLot) && d < 300.0 {
                fleet_lot_near = true;
            }
            if let Some(k) = kind {
                if d < industry::NEIGHBOUR_RADIUS {
                    neighbours.push((k, d));
                }
            }
        }
        neighbours.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        Some(IndustrySite {
            ground: ground::sample_area(&self.landscape, c, &lot.polygon),
            contamination: self.contamination.over(c, &lot.polygon),
            land_value: self.land_value.at(c),
            port,
            yard,
            highway,
            depot,
            neighbours,
            fleet_lot_near,
        })
    }

    /// The weight of each industry kind on a lot, in `IndustryKind::ALL`
    /// order, with the player's bias applied (a forced kind takes it all).
    pub fn industry_weights(&self, lot_id: LotId) -> Option<[f32; 7]> {
        let site = self.industry_site(lot_id)?;
        let mut w = industry::weights(&site);
        if let Some(Some(k)) = self.industry_bias_at(self.lots[lot_id].center) {
            w = [0.0; 7];
            w[k.index()] = 1.0;
        }
        Some(w)
    }

    /// The business a new industrial building on `lot_id` hosts.
    pub(super) fn pick_industrial_business(&mut self, lot_id: LotId) -> Option<BusinessKind> {
        let site = self.industry_site(lot_id)?;
        let forced = self.industry_bias_at(self.lots[lot_id].center).flatten();
        let roll = self.rng.random::<f32>();
        let kind = forced.unwrap_or_else(|| industry::pick(&industry::weights(&site), roll));
        let roll = self.rng.random::<f32>();
        industry::pick_business(&industry::business_weights(kind, &site), roll)
    }

    // ── land value ──────────────────────────────────────────────────────────

    /// An industrial building's pull on the land: a nuisance by its kind
    /// (homes feel it most) and, where land is cheap, the lift of the jobs.
    pub(super) fn splat_industry(b: &mut LandValueBuilder, bld: &Building, c: Vec2) {
        if bld.plant.is_some() {
            // Plants carry their own figures (`PowerPlantKind::pollution`).
            return;
        }
        let kind = bld.business.as_ref().and_then(|z| z.kind.industry());
        let (r, w) = kind.map_or((220.0, 0.3), |k| k.nuisance());
        b.splat_nuisance(c, r, w);
        if let Some(k) = kind {
            let open = bld.construction.is_none() && bld.business.as_ref().is_some_and(|z| !z.closed);
            if open {
                let (r, w) = k.lift();
                b.splat_lift(c, r, w);
            }
        }
    }

    /// Fouled ground is worth less to everyone.
    pub(super) fn splat_contamination(&self, b: &mut LandValueBuilder) {
        let c = &self.contamination;
        for (&i, &v) in &c.cells {
            if v < 0.02 {
                continue;
            }
            let (x, y) = (i as usize % c.n, i as usize / c.n);
            b.splat(c.cell_center(x, y), c.cell * 2.0, -CONTAMINATION_VALUE_PENALTY * v);
        }
    }

    // ── the hourly round ────────────────────────────────────────────────────

    /// Every hour after production: wells, refineries and coal plants foul
    /// the ground; farms yield by their soil and lose to contamination;
    /// container yards and fleet lots burn fuel and earn their fees.
    pub(super) fn industry_hourly(&mut self) {
        let ports: Vec<Vec2> = self.ports.working(PortKind::ShippingPort).into_iter().filter_map(|b| self.building_lot(b).map(|l| l.center)).collect();
        let yards: Vec<Vec2> = self.yards.iter().filter_map(|b| self.building_lot(*b).map(|l| l.center)).collect();
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for id in ids {
            let b = &self.buildings[id];
            if b.construction.is_some() || b.is_derelict() {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let (center, outline) = (lot.center, lot.polygon.clone());
            if let Some((r, a)) = b.plant.and_then(|k| k.ground_spill()) {
                self.contamination.add(center, r, a);
                continue;
            }
            let Some(bz) = &b.business else { continue };
            if bz.closed {
                continue;
            }
            let kind = bz.kind;
            let jobs = b.jobs;
            let staffing = if jobs == 0 { 0.0 } else { (b.workers.len() as f32 / jobs as f32).clamp(0.0, 1.0) * b.powered.clamp(0.0, 1.0) };
            if let Some((r, a)) = kind.industry().and_then(|k| k.spill()) {
                if staffing > 0.0 {
                    self.contamination.add(center, r, a * staffing);
                }
            }
            let cap = jobs as f32 * staffing * crate::market::JOB_RATE;
            match kind {
                // Field crops follow the soil; indoor farms only mind poison.
                BusinessKind::GrainFarm | BusinessKind::VertFarm | BusinessKind::Fishery => {
                    let contam = self.contamination.over(center, &outline);
                    let factor = match kind {
                        BusinessKind::GrainFarm => {
                            let g = ground::sample_area(&self.landscape, center, &outline);
                            (0.6 + 0.8 * g.fertility) * (1.0 - 0.9 * contam)
                        }
                        _ => 1.0 - 0.5 * contam,
                    };
                    if (factor - 1.0).abs() > 0.01 && cap > 0.0 {
                        let bz = self.buildings[id].business.as_mut().unwrap();
                        for (res, per) in kind.recipe().outputs {
                            let delta = per * cap * (factor - 1.0);
                            if delta < 0.0 {
                                bz.take(*res, -delta);
                            } else {
                                let ceiling = per * jobs as f32 * crate::market::JOB_RATE * 96.0;
                                let room = (ceiling - bz.stock_of(*res)).max(0.0);
                                bz.add(*res, delta.min(room));
                            }
                        }
                    }
                }
                BusinessKind::ContainerYard | BusinessKind::FleetLot => {
                    let per = kind.recipe().inputs.iter().find(|(r, _)| *r == Resource::Fuel).map_or(0.0, |x| x.1);
                    let need = per * cap;
                    let port = ports.iter().map(|p| near(p.distance(center), 80.0, 420.0)).fold(0.0, f32::max);
                    let yard = yards.iter().map(|p| near(p.distance(center), 80.0, 360.0)).fold(0.0, f32::max);
                    let bz = self.buildings[id].business.as_mut().unwrap();
                    let got = if need > 0.0 { bz.take(Resource::Fuel, need) } else { 0.0 };
                    let fuelled = if need > 0.0 { got / need } else { 1.0 };
                    let mut fee = HANDLING_PER_JOB_HOUR * jobs as f32 * staffing * (0.5 + 0.5 * fuelled) * (1.0 + 0.5 * port + 0.3 * yard);
                    if kind == BusinessKind::FleetLot {
                        fee += BAY_FEE_PER_HOUR * bz.fleet_trucks as f32;
                    }
                    bz.money += fee;
                    bz.revenue_today += fee;
                }
                _ => {}
            }
        }
    }

    /// Once a day: contamination seeps and fades.
    pub(super) fn industry_daily(&mut self) {
        self.contamination.daily();
    }

    // ── truck and bus lots ──────────────────────────────────────────────────

    /// Bays at a truck and bus lot: one per `FLEET_BAY_M2` of ground.
    pub fn fleet_bays_for(&self, lot: LotId) -> u16 {
        self.lots.get(lot).map_or(0, |l| (l.area() / FLEET_BAY_M2).clamp(4.0, 40.0) as u16)
    }

    /// Bays at a building that is a truck and bus lot, else zero.
    pub fn fleet_capacity(&self, b: BuildingId) -> u16 {
        match self.buildings.get(b) {
            Some(bld) if bld.business.as_ref().is_some_and(|z| z.kind == BusinessKind::FleetLot && !z.closed) && bld.construction.is_none() => self.fleet_bays_for(bld.lot),
            _ => 0,
        }
    }

    /// Trucks parked at a lot plus those on their way there to park.
    pub fn fleet_parked(&self, b: BuildingId) -> u16 {
        self.buildings.get(b).and_then(|x| x.business.as_ref()).map_or(0, |z| z.fleet_trucks)
    }

    fn fleet_inbound(&self) -> HashMap<BuildingId, u16> {
        let mut m: HashMap<BuildingId, u16> = HashMap::new();
        for car in self.traffic.cars.values() {
            if let Some(c) = car.cargo.as_ref().filter(|c| is_return_trip(c)) {
                *m.entry(c.to).or_default() += 1;
            }
        }
        m
    }

    /// The nearest truck and bus lot with a free bay within `reach` of `p`.
    pub fn fleet_lot_with_room(&self, p: Vec2, reach: f32) -> Option<BuildingId> {
        let inbound = self.fleet_inbound();
        self.buildings
            .iter()
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::FleetLot))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance(p))))
            .filter(|(id, d)| *d <= reach && self.fleet_parked(*id) + inbound.get(id).copied().unwrap_or(0) < self.fleet_capacity(*id))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(id, _)| id)
    }

    /// Room a depot's buses have at truck and bus lots nearby, beyond the
    /// depot's own: a quarter of each lot's bays.
    pub fn depot_overflow(&self, depot: BuildingId) -> u32 {
        let Some(at) = self.building_lot(depot).map(|l| l.center) else { return 0 };
        self.buildings
            .iter()
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::FleetLot))
            .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| l.center.distance(at) <= DEPOT_OVERFLOW_REACH))
            .map(|(id, _)| (self.fleet_capacity(id) as u32 / 4).max(if self.fleet_capacity(id) > 0 { 2 } else { 0 }))
            .sum()
    }

    fn park_truck(&mut self, lot_building: BuildingId) {
        let cap = self.fleet_capacity(lot_building);
        let Some(bz) = self.buildings.get_mut(lot_building).and_then(|b| b.business.as_mut()) else { return };
        if bz.kind != BusinessKind::FleetLot || bz.fleet_trucks >= cap {
            return;
        }
        let before = fleet_bucket(bz.fleet_trucks, cap);
        bz.fleet_trucks += 1;
        if fleet_bucket(bz.fleet_trucks, cap) != before {
            self.events.push(SimEvent::BuildingChanged(lot_building));
        }
    }

    /// Where a truck carrying a load from `from` sets off: a truck and bus
    /// lot near the seller with a lorry parked takes the job (the lorry
    /// leaves the lot), else the seller's own yard.
    pub(super) fn fleet_origin(&mut self, from: BuildingId) -> BuildingId {
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            return from;
        }
        let Some(at) = self.building_lot(from).map(|l| l.center) else { return from };
        let lot = self
            .buildings
            .iter()
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::FleetLot && z.fleet_trucks > 0))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance(at))))
            .filter(|(_, d)| *d <= FLEET_SHARE_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(id, _)| id);
        let Some(lot) = lot else { return from };
        let cap = self.fleet_capacity(lot);
        let bz = self.buildings[lot].business.as_mut().unwrap();
        let before = fleet_bucket(bz.fleet_trucks, cap);
        bz.fleet_trucks -= 1;
        if fleet_bucket(bz.fleet_trucks, cap) != before {
            self.events.push(SimEvent::BuildingChanged(lot));
        }
        lot
    }

    /// A truck reached the end of its trip: an empty one parks (or is home
    /// and done); a loaded one delivers and then heads for a truck lot with
    /// room nearby, else back to the business that sent it.
    pub(super) fn truck_arrived(&mut self, cargo: Cargo) {
        if is_return_trip(&cargo) {
            self.park_truck(cargo.to);
            return;
        }
        let at = cargo.to;
        let home = cargo.from;
        let resource = cargo.resource;
        self.deliver(cargo);
        self.send_truck_back(at, home, resource);
    }

    fn send_truck_back(&mut self, at: BuildingId, home: BuildingId, resource: Resource) {
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            return;
        }
        let Some(pos) = self.building_lot(at).map(|l| l.center) else { return };
        let dest = self.fleet_lot_with_room(pos, FLEET_REACH).or_else(|| {
            let h = self.buildings.get(home)?;
            let ok = home != at && h.construction.is_none() && !h.is_derelict() && self.lots.get(h.lot).is_some_and(|l| l.center.distance(pos) <= RETURN_REACH);
            ok.then_some(home)
        });
        let Some(dest) = dest else { return };
        if dest == at {
            self.park_truck(dest);
            return;
        }
        let route = (|| {
            let (a_seg, a_pt) = self.truck_stop(at)?;
            let (b_seg, b_pt) = self.truck_stop(dest)?;
            self.traffic.net.find_route(a_seg, a_pt, b_seg, b_pt)
        })();
        match route {
            Some((lanes, s0, s1)) if !lanes.is_empty() => {
                let cargo = Cargo { from: dest, to: dest, resource, amount: 0.0, price: 0.0 };
                self.traffic.spawn_car(CitizenId::default(), Some(cargo), lanes, s0, s1, 7);
            }
            // No road: a lorry for a lot next door simply rolls in.
            _ => self.park_truck(dest),
        }
    }
}

/// Occupancy in quarters, so a lot is redrawn only when it visibly changes.
fn fleet_bucket(n: u16, cap: u16) -> u8 {
    if cap == 0 {
        0
    } else {
        ((n as f32 / cap as f32) * 4.0).round() as u8
    }
}

#[cfg(test)]
#[path = "industry_tests.rs"]
mod tests;
