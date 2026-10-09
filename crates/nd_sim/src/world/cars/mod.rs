//! [carlife] Private cars as things with a place, and the vehicles that
//! come back to base.
//!
//! Every private car is an `OwnedCar` in `World::vehicles`: parked (at its
//! owner's home, a building's garage, a parking business, a kerb slot, or
//! squeezed onto the street when nothing was free) or on the road as a car
//! of the traffic model (`Car::owned`). Cars come into the city only when a
//! household drives in with one or someone buys one (at a dealer in town,
//! delivered home; or out of town, driven in from the map edge), and leave
//! it only when sold (driven to the dealer), scrapped, or driven out of town
//! by an emigrating owner (`CarEvent`). A trip starts at the car's space:
//! the driver walks to it, the car pulls out of the kerb into a gap, and at
//! the far end it parks: the destination's own spaces, else a kerb slot or a
//! paid lot nearby (whichever costs the driver less in walking and money),
//! else it circles the blocks around looking, and after a few tries squeezes
//! in anywhere. Searching, fees and slow trips are felt (`Commute`) and
//! weigh on how the driver travels next time.
//!
//! Trucks and vans belong to the business that sent them (`Errand::Haul`):
//! after the job they drive back and park in its fleet
//! (`Vehicles::fleet_parked`), or leave town when they came from elsewhere.

use super::*;
use crate::citizens::{ToCar, TripMode};
use crate::traffic::LaneId;
use rand::seq::SliceRandom;
use crate::vehicle::{CarEvent, CarPlace, Errand, OwnedCar, ParkKind, ParkSpot, VehicleId, VehicleModel, BIKE_PRICE};

mod kerbs;
mod parking;
mod vehicle_routes;
mod parking_hunt;
mod car_market;
mod commute;

/// A parked car this close to where a trip starts is walked to and driven.
pub(super) const CAR_REACH: f32 = 320.0;
/// No kerb parking this close to either end of a street (the junction).
const KERB_MARGIN: f32 = 12.0;
/// [services] Seconds a vehicle with no road off the map waits at the kerb
/// before it looks for one again (`retire_vehicle`).
pub(super) const RETIRE_RETRY_SECONDS: f32 = 300.0;
/// Length of a kerb slot.
const KERB_PITCH: f32 = 6.5;
/// How far from the destination a driver will park at a kerb.
const KERB_WALK: f32 = 170.0;
/// And at a parking business.
const LOT_WALK: f32 = 180.0;
/// Streets a driver circles through looking for a space.
const SEARCH_RADIUS: f32 = 240.0;
/// Places checked before parking anywhere at all.
const MAX_TRIES: u8 = 4;
/// [parking] Rounds after which the driver takes any free slot at all.
const GIVE_UP: u8 = 10;
/// [parking] Ticks (sim minutes) of hunting after which a driver looks
/// further and at banned kerbs, however few places it has tried.
const HUNT_DESPERATE: u64 = 20;
/// [parking] Ticks of hunting after which a driver stops looking and
/// leaves the car where it can: a banned kerb nearby, a free one further
/// off, else squeezed in where it stands.
pub(super) const HUNT_GIVE_UP: u64 = 60;
/// [parking] Seconds a worn-out hunter stands in a queue before it leaves
/// the car where it is (`World::hunters_give_up`).
const HUNT_STUCK: f32 = 120.0;
/// Walking the last stretch from the car below this is just getting out.
const GET_OUT: f32 = 20.0;

/// A free kerb slot: segment, side, slot, where it stands, heading.
#[derive(Clone, Copy, Debug)]
pub(super) struct KerbSlot {
    pub segment: SegmentId,
    pub side: u8,
    pub slot: u16,
    pub pos: Vec2,
    pub heading: f32,
}

impl World {
    // ── keeping things straight ─────────────────────────────────────────

    /// After a load: give saves from before persistent cars a car for every
    /// owner (parked at home, or with them on the road), make the building
    /// counts match the cars in them, and rebuild the kerb index.
    pub(super) fn vehicles_after_load(&mut self) {
        // Records whose owner is gone and who are not driving away.
        let orphans: Vec<VehicleId> = self
            .vehicles
            .cars
            .iter()
            .filter(|(id, c)| match c.place {
                CarPlace::Parked(_) => !self.citizens.get(c.owner).is_some_and(|o| o.car == Some(*id)) && !self.spare_kept(c),
                CarPlace::Driving(cid) => !self.traffic.cars.get(cid).is_some_and(|t| t.owned.is_some()),
            })
            .map(|(id, _)| id)
            .collect();
        for vid in orphans {
            let place = self.vehicles.cars[vid].place;
            match place {
                CarPlace::Driving(_) => {
                    // Its traffic car was dropped: park it where its owner is.
                    let owner = self.vehicles.cars[vid].owner;
                    match self.citizens.get(owner).filter(|c| c.car == Some(vid)).map(|c| c.pos) {
                        Some(p) => self.park_stranded(vid, p),
                        None => {
                            self.vehicles.cars.remove(vid);
                            self.vehicles.note(CarEvent::Scrapped);
                        }
                    }
                }
                CarPlace::Parked(_) => {
                    self.vehicles.cars.remove(vid);
                    self.vehicles.note(CarEvent::Scrapped);
                }
            }
        }
        for c in self.citizens.values_mut() {
            if c.car.is_some_and(|v| !self.vehicles.cars.contains_key(v)) {
                c.car = None;
            }
        }
        // Building counts follow the cars in them.
        for b in self.buildings.values_mut() {
            b.parking_used = 0;
        }
        for car in self.vehicles.cars.values() {
            if let Some(s) = car.spot().filter(|s| s.kind.uses_building()) {
                if let Some(b) = s.building.and_then(|b| self.buildings.get_mut(b)) {
                    b.parking_used = b.parking_used.saturating_add(1);
                }
            }
        }
        self.refresh_no_kerb();
        self.vehicles.reindex_kerbs();
        // Old saves: owners without a car record get one where they are.
        let ids: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.owns_car && c.car.is_none()).map(|(id, _)| id).collect();
        for id in ids {
            let model = {
                let c = &mut self.citizens[id];
                *c.vehicle.get_or_insert_with(|| VehicleModel::fallback(c.seed, c.income, c.age))
            };
            let driving = match self.citizens[id].activity {
                Activity::Driving { car, .. } => self.traffic.cars.contains_key(car).then_some(car),
                _ => None,
            };
            match driving {
                Some(car) => {
                    let vid = self.new_car(id, model, CarPlace::Driving(car), CarEvent::Migrated);
                    self.traffic.cars[car].owned = Some(vid);
                }
                None => {
                    let home = self.citizens[id].home;
                    let near = home.and_then(|h| self.building_entrance(h)).unwrap_or(self.citizens[id].pos);
                    let spot = self.place_near(near, home).unwrap_or(ParkSpot { kind: ParkKind::Overflow, pos: near, slot: u16::MAX, ..Default::default() });
                    let vid = self.new_car(id, model, CarPlace::Driving(CarId::default()), CarEvent::Migrated);
                    self.park_at(vid, spot);
                }
            }
        }
        for c in self.citizens.values_mut() {
            c.owns_car = c.car.is_some();
        }
        // [parking] Street cars from older saves without a slot of their
        // own (stacked on one point) get one now.
        self.refresh_kerbs();
    }

    /// Hourly: cars parked at a building that has gone are moved to the
    /// street outside; cars still on a dealer's forecourt are delivered;
    /// fleets of businesses that are gone are sold off.
    pub(super) fn hourly_vehicles(&mut self) {
        self.refresh_no_kerb();
        let lost: Vec<(VehicleId, Vec2)> = self
            .vehicles
            .cars
            .iter()
            .filter_map(|(id, c)| c.spot().map(|s| (id, *s)))
            .filter(|(_, s)| s.kind.uses_building() || s.kind == ParkKind::Dealer)
            .filter(|(_, s)| !s.building.is_some_and(|b| self.buildings.contains_key(b)))
            .map(|(id, s)| (id, s.pos))
            .collect();
        for (vid, pos) in lost {
            self.unpark(vid);
            self.vehicles.cars[vid].place = CarPlace::Driving(CarId::default());
            let spot = self.place_near(pos, None).unwrap_or(ParkSpot { kind: ParkKind::Overflow, pos, slot: u16::MAX, ..Default::default() });
            self.park_at(vid, spot);
        }
        let waiting: Vec<VehicleId> = self.vehicles.cars.iter().filter(|(_, c)| c.spot().is_some_and(|s| s.kind == ParkKind::Dealer)).map(|(id, _)| id).collect();
        for vid in waiting {
            self.deliver_car(vid);
        }
        let buildings = &self.buildings;
        self.vehicles.fleet_parked.retain(|b, n| *n > 0 && buildings.contains_key(*b));
        // A private car on the road whose record says otherwise, or the
        // other way round, is put right (never deleted).
        let stale: Vec<VehicleId> = self.vehicles.cars.iter().filter(|(id, c)| matches!(c.place, CarPlace::Driving(cid) if self.traffic.cars.get(cid).is_none_or(|t| t.owned != Some(*id)))).map(|(id, _)| id).collect();
        for vid in stale {
            let owner = self.vehicles.cars[vid].owner;
            let pos = self.citizens.get(owner).map(|c| c.pos).unwrap_or_default();
            self.park_stranded(vid, pos);
        }
    }

    /// Midnight: yesterday's counts, memories fade.
    pub(super) fn daily_vehicles(&mut self) {
        self.daily_city_lots();
        let s = &mut self.vehicles.stats;
        s.trips_yesterday = s.trips_today;
        s.trips_today = Default::default();
        s.bought_today = 0;
        s.sold_today = 0;
        s.bikes_bought_today = 0;
        s.bikes_sold_today = 0;
        s.circled_today = 0;
        s.search_seconds_today = 0.0;
        s.parked_today = Default::default();
        s.fleet_returned_today = 0;
        for c in self.citizens.values_mut() {
            c.commute.fade();
        }
    }
}

/// [economy] A car dealership sells to buyers this far away; a bike shop
/// to riders this far. Beyond, the old shop down the road will do.
pub const CAR_DEALER_REACH: f32 = 4000.0;
pub const BIKE_SHOP_REACH: f32 = 2500.0;

pub(super) fn id_hash(id: CitizenId) -> u64 {
    slotmap::Key::data(&id).as_ffi()
}

/// How a citizen names a vehicle, for their memories.
pub(super) fn model_label(m: &VehicleModel) -> String {
    let old = if m.class == crate::vehicle::VehicleClass::Cheap { "beat-up " } else { "" };
    let flash = if m.tuned { "souped-up " } else { "" };
    format!("{old}{flash}{}", m.body.label().replace('_', " "))
}

#[cfg(test)]
impl World {
    /// Tests: give a citizen a car parked near `near` (a purchase).
    pub(super) fn give_car_parked(&mut self, id: CitizenId, near: Vec2) -> VehicleId {
        let model = VehicleModel::fallback(self.citizens[id].seed, self.citizens[id].income, self.citizens[id].age);
        let home = self.citizens[id].home;
        let spot = self.place_near(near, home).expect("a street to park on");
        let vid = self.new_car(id, model, CarPlace::Driving(CarId::default()), CarEvent::Bought);
        self.park_at(vid, spot);
        vid
    }
}
