//! [r17-goods] The hospitals' medicine and the nuclear plants' fuel
//! (`crate::goods`): two supply lines outside the businesses' recipes.

use super::*;
use crate::goods::{self as gd, NuclearFuel};
use crate::market::IMPORT_MARKUP;
use crate::traffic::Cargo;
use crate::power::PowerPlantKind;
use crate::services::ServiceKind;

/// The colour code of a convoy's vehicles (a lorry's colour index).
const CONVOY_COLOR: u8 = 7;

impl World {
    /// Every hour: nuclear plants needing fuel send for a convoy; at
    /// `MEDICINE_ORDER_HOUR` the hospitals order medicine; every hour they
    /// use some.
    pub(super) fn goods_hourly(&mut self) {
        self.nuclear_fuel_hourly();
        self.hospitals_use_medicine();
        if self.clock.hour() == gd::MEDICINE_ORDER_HOUR {
            self.hospital_supplies();
        }
    }

    /// At midnight: the day's counts roll; loads on their way that never
    /// came are written down a little at a time.
    pub(super) fn goods_daily(&mut self) {
        self.goods.yesterday = std::mem::take(&mut self.goods.today);
        for v in self.goods.medicine_inbound.values_mut() {
            *v *= 0.7;
        }
        let live = |w: &World, id: &BuildingId| w.buildings.get(*id).is_some_and(|b| b.service.is_some_and(|s| s.kind == ServiceKind::Health));
        let gone: Vec<BuildingId> = self.goods.medicine.keys().chain(self.goods.medicine_inbound.keys()).copied().filter(|id| !live(self, id)).collect();
        for id in gone {
            self.goods.medicine.remove(&id);
            self.goods.medicine_inbound.remove(&id);
        }
        let plants: std::collections::BTreeSet<LotId> = self.buildings.values().filter(|b| b.plant == Some(PowerPlantKind::Nuclear)).map(|b| b.lot).collect();
        self.goods.nuclear.retain(|l, _| plants.contains(l));
    }

    // ── hospitals ───────────────────────────────────────────────────────

    /// The working hospitals: id, centre, beds, patients in a bed now.
    fn hospitals(&self) -> Vec<(BuildingId, Vec2, u32, u32)> {
        self.buildings
            .iter()
            .filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::Health).map(|s| (id, b, s)))
            .filter(|(_, b, _)| !b.is_derelict() && b.operating())
            .filter_map(|(id, b, s)| {
                let patients = self.services.admissions.iter().filter(|a| a.hospital == id).count() as u32;
                self.lots.get(b.lot).map(|l| (id, l.center, s.kind.capacity(s.tier), patients))
            })
            .collect()
    }

    /// A hospital's medicine on its shelves. One the book has not seen (an
    /// old save, a new hospital) starts with its full keep.
    pub fn hospital_medicine(&self, h: BuildingId) -> Option<f32> {
        self.goods.medicine.get(&h).copied()
    }

    /// Whether a hospital has run out of medicine.
    pub fn hospital_short(&self, h: BuildingId) -> bool {
        self.goods.medicine.get(&h).is_some_and(|m| *m <= 0.01)
    }

    /// The wards' hour of medicine.
    fn hospitals_use_medicine(&mut self) {
        let mut short = 0;
        for (id, _, beds, patients) in self.hospitals() {
            let per_day = gd::medicine_per_day(beds, patients);
            let stock = self.goods.medicine.entry(id).or_insert(per_day * gd::MEDICINE_KEEP_DAYS);
            let use_now = (per_day / 24.0).min(*stock);
            *stock -= use_now;
            self.goods.today.medicine_used += use_now;
            if *stock <= 0.01 {
                short += 1;
            }
        }
        self.goods.today.hospitals_short = self.goods.today.hospitals_short.max(short);
    }

    /// Each hospital short of its keep orders medicine: from the cheapest
    /// drug maker in town with stock (its lorry brings it), else imported.
    /// The city pays.
    pub(super) fn hospital_supplies(&mut self) {
        for (id, at, beds, patients) in self.hospitals() {
            let per_day = gd::medicine_per_day(beds, patients);
            let stock = *self.goods.medicine.entry(id).or_insert(per_day * gd::MEDICINE_KEEP_DAYS);
            let inbound = self.goods.medicine_inbound.get(&id).copied().unwrap_or(0.0);
            if stock + inbound >= per_day * gd::MEDICINE_REORDER_DAYS {
                continue;
            }
            let want = (per_day * gd::MEDICINE_KEEP_DAYS - stock - inbound).max(1.0).ceil();
            let r = Resource::Medicine;
            if let Some((seller, price)) = self.best_seller(r, at, want, id) {
                let got = self.buildings[seller].business.as_mut().map_or(0.0, |z| z.take(r, want));
                if got > 0.0 {
                    let cost = got * price;
                    let paid = self.city_pays(Account::Business(seller), Flow::ServiceUpkeep, cost, Some(Item::Named("medicine for the hospitals".into())));
                    if let Some(z) = self.buildings[seller].business.as_mut() {
                        z.revenue_today += paid;
                        z.deals += 1;
                    }
                    self.economy.market.deals_today += 1;
                    *self.goods.medicine_inbound.entry(id).or_insert(0.0) += got;
                    self.goods.today.medicine_local += got;
                    self.ship(seller, id, r, got, cost);
                    continue;
                }
            }
            let cost = want * r.base_price() * IMPORT_MARKUP;
            self.city_pays(Account::Outside, Flow::ServiceUpkeep, cost, Some(Item::Named("medicine for the hospitals (imported)".into())));
            self.economy.taxes.note_import(r, want);
            self.economy.market.imports_today += cost;
            *self.goods.medicine_inbound.entry(id).or_insert(0.0) += want;
            self.goods.today.medicine_imported += want;
            self.import(id, r, want, cost);
        }
    }

    /// A load for a hospital or a nuclear plant arrived (`World::deliver`):
    /// onto its shelves, or into its reactor. True if it was one.
    pub(super) fn goods_receive(&mut self, cargo: &Cargo) -> bool {
        match cargo.resource {
            Resource::Medicine if self.buildings.get(cargo.to).is_some_and(|b| b.business.is_none() && b.service.is_some_and(|s| s.kind == ServiceKind::Health)) => {
                *self.goods.medicine.entry(cargo.to).or_insert(0.0) += cargo.amount;
                if let Some(v) = self.goods.medicine_inbound.get_mut(&cargo.to) {
                    *v = (*v - cargo.amount).max(0.0);
                }
                true
            }
            Resource::NuclearFuel => {
                // The escorts carry nothing (amount 0); the flask the core.
                if cargo.amount > 0.0 {
                    if let Some(lot) = self.buildings.get(cargo.to).filter(|b| b.plant == Some(PowerPlantKind::Nuclear)).map(|b| b.lot) {
                        let now = self.clock.tick;
                        let f = self.goods.nuclear.entry(lot).or_default();
                        f.until = f.until.max(now) + gd::FUEL_DAYS * 1440;
                        f.convoy = None;
                        f.loads += 1;
                        self.goods.today.convoys_arrived += 1;
                        self.refresh_power();
                    }
                }
                true
            }
            _ => false,
        }
    }

    // ── nuclear fuel ────────────────────────────────────────────────────

    /// [r17-goods] Whether a nuclear plant has fuel in it (every other
    /// plant needs none). A plant the book has not seen (from an old save)
    /// counts as fuelled until its first hour here gives it a core.
    pub fn plant_fuelled(&self, b: &Building) -> bool {
        b.plant != Some(PowerPlantKind::Nuclear) || self.goods.nuclear.get(&b.lot).is_none_or(|f| f.fuelled(self.clock.tick))
    }

    /// The fuel of the nuclear plant on `lot`, if it is one.
    pub fn nuclear_fuel(&self, lot: LotId) -> Option<NuclearFuel> {
        self.goods.nuclear.get(&lot).copied()
    }

    /// A new nuclear plant starts with no fuel: it waits for its convoy.
    pub(super) fn nuclear_plant_built(&mut self, lot: LotId) {
        self.goods.nuclear.insert(lot, NuclearFuel::default());
    }

    /// Every hour: a plant from an old save is given its core; a plant
    /// whose core runs out within `FUEL_LEAD_DAYS` (or is empty) sends for
    /// a convoy, unless one is under way.
    fn nuclear_fuel_hourly(&mut self) {
        let now = self.clock.tick;
        let plants: Vec<(BuildingId, LotId)> = self.buildings.iter().filter(|(_, b)| b.plant == Some(PowerPlantKind::Nuclear) && !b.is_derelict() && b.construction.is_none()).map(|(id, b)| (id, b.lot)).collect();
        for (id, lot) in plants {
            let f = *self.goods.nuclear.entry(lot).or_insert(NuclearFuel { until: now + gd::FUEL_DAYS * 1440, convoy: None, loads: 1 });
            if let Some(sent) = f.convoy {
                if now < sent + gd::CONVOY_RETRY_HOURS * 60 {
                    continue;
                }
            }
            if f.until > now + gd::FUEL_LEAD_DAYS * 1440 {
                continue;
            }
            self.send_fuel_convoy(id, lot);
        }
    }

    /// The convoy: an escort, the flask carrier, an escort, from the city
    /// edge to the plant's gate. The city pays for the load as it is sent.
    /// With no road in, the fuel comes the way an island's goods do, or
    /// (nothing else left) is handed over at the gate.
    fn send_fuel_convoy(&mut self, plant: BuildingId, lot: LotId) {
        let now = self.clock.tick;
        let name = self.buildings[plant].name.clone();
        self.city_pays(Account::Outside, Flow::PlantUpkeep, gd::FUEL_LOAD_PRICE, Some(Item::Named(format!("nuclear fuel for {name}"))));
        self.goods.today.convoys_sent += 1;
        if let Some(f) = self.goods.nuclear.get_mut(&lot) {
            f.convoy = Some(now);
        }
        let load = Cargo { from: BuildingId::default(), to: plant, resource: Resource::NuclearFuel, amount: 1.0, price: gd::FUEL_LOAD_PRICE };
        let escort = Cargo { amount: 0.0, price: 0.0, ..load };
        let route = (|| {
            let entry = self.entry_node_const()?;
            let node = self.roads.node(entry);
            let seg = *node.segments.iter().find(|&&s| self.roads.segment(s).alive)?;
            let (to_seg, to_pt) = self.truck_stop(plant)?;
            self.traffic.net.find_route(seg, node.pos, to_seg, to_pt)
        })()
        .or_else(|| self.site_route_from_edge(plant));
        let Some((route, s0, s1)) = route else {
            self.deliver(load);
            return;
        };
        for (k, (cargo, body)) in [(escort, crate::vehicle::BodyType::Suv), (load, crate::vehicle::BodyType::FlaskCarrier), (escort, crate::vehicle::BodyType::Suv)].into_iter().enumerate() {
            let id = self.traffic.spawn_car(CitizenId::default(), Some(cargo), route.clone(), s0, s1, CONVOY_COLOR);
            let mut m = crate::vehicle::VehicleModel::new(crate::vehicle::VehicleClass::Mid, body);
            m.livery = crate::vehicle::Livery::Escort;
            m.paint = k as u8;
            self.traffic.cars[id].model = m;
        }
    }
}
