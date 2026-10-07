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
    // ── kerbs ───────────────────────────────────────────────────────────

    /// Can cars park along this street? Not on highways, ramps, bridges or
    /// avenues (two lanes or more each way are clearways), nor on shopping
    /// and nightlife streets, whose kerbs are for loading: drivers going
    /// there use the lots.
    pub(super) fn kerb_allowed(&self, sid: SegmentId) -> bool {
        self.roads.try_segment(sid).is_some_and(|s| s.alive && s.is_street() && !s.fronts_nothing() && !s.bridge && s.lanes() > 0 && s.lanes_fwd < 2 && s.lanes_bwd < 2)
            && !self.vehicles.no_kerb.1.contains(&sid)
            && !self.on_ring(sid)
    }

    /// Is this segment part of a roundabout's ring? Rings are one-way and
    /// often one lane, which the street rule above would otherwise let
    /// cars park along: parked cars then sat in the ring itself.
    pub(super) fn on_ring(&self, sid: SegmentId) -> bool {
        let Some(seg) = self.roads.try_segment(sid) else { return false };
        let (a, b) = (self.roads.node(seg.a).pos, self.roads.node(seg.b).pos);
        let mid = (a + b) * 0.5;
        self.roads.rings.iter().any(|r| {
            let near = |p: Vec2| (p.distance(r.center) - r.radius).abs() < 3.0;
            // A chord's midpoint sits inside the circle by r(1 - cos(θ/2)).
            let sag = (a.distance(b) * 0.5).powi(2) / (2.0 * r.radius.max(1.0));
            near(a) && near(b) && (mid.distance(r.center) - (r.radius - sag)).abs() < 3.0
        })
    }

    /// Refresh which streets have no kerb parking, when the plan or the
    /// roads changed.
    pub(super) fn refresh_no_kerb(&mut self) {
        let key = (self.plan_version, self.roads.version);
        if self.vehicles.no_kerb.0 == Some(key) {
            return;
        }
        let set = self.lots.values().filter(|l| matches!(l.land_use, LandUse::Commercial | LandUse::Entertainment)).map(|l| l.front_segment).collect();
        self.vehicles.no_kerb = (Some(key), set);
    }

    /// Kerb slots along one side of a street.
    pub(super) fn kerb_slots(&self, sid: SegmentId) -> u16 {
        if !self.kerb_allowed(sid) {
            return 0;
        }
        self.slot_count(sid)
    }

    /// [parking] Can a car physically stand along this street, legal or
    /// not: every street but highways, ramps, tunnels, bridges and
    /// roundabout rings (where a parked car would sit in the traffic, or
    /// be drawn on the ground over the bore).
    pub(super) fn kerb_physical(&self, sid: SegmentId) -> bool {
        self.roads.try_segment(sid).is_some_and(|s| s.alive && s.is_street() && !s.fronts_nothing() && !s.bridge && s.lanes() > 0) && !self.on_ring(sid)
    }

    /// Slots along one side of a street, legal or not.
    pub(super) fn slot_count(&self, sid: SegmentId) -> u16 {
        if !self.kerb_physical(sid) {
            return 0;
        }
        let (a, b) = self.roads.segment_points(sid);
        ((a.distance(b) - 2.0 * KERB_MARGIN) / KERB_PITCH).floor().max(0.0) as u16
    }

    /// The kerb sides of a street cars can pull into: 0 to the right of
    /// a→b (cars driving a→b), 1 to the right of b→a.
    fn kerb_sides(&self, sid: SegmentId) -> [bool; 2] {
        let s = self.roads.segment(sid);
        [s.lanes_fwd > 0, s.lanes_bwd > 0]
    }

    /// Where a car parked `along` metres from the start of its side stands.
    fn kerb_pose(&self, sid: SegmentId, side: u8, along: f32) -> (Vec2, f32) {
        let (a, b) = self.roads.segment_points(sid);
        let len = a.distance(b).max(0.01);
        let d = (b - a) / len;
        let (p, dir) = if side == 0 { (a + d * along, d) } else { (b - d * along, -d) };
        let right = Vec2::new(-dir.y, dir.x);
        let off = self.roads.segment(sid).half_width() - 1.15;
        (p + right * off, dir.y.atan2(dir.x))
    }

    fn slot_pose(&self, sid: SegmentId, side: u8, slot: u16) -> (Vec2, f32) {
        self.kerb_pose(sid, side, KERB_MARGIN + (slot as f32 + 0.5) * KERB_PITCH)
    }

    /// The free kerb slot nearest `q`, within `r`.
    pub(super) fn free_kerb_near(&self, q: Vec2, r: f32) -> Option<KerbSlot> {
        self.free_kerbs_near(q, r, 1).into_iter().next()
    }

    /// Up to `n` free kerb slots within `r` of `q`, nearest first, at most
    /// one per side of a street (the nearest there).
    pub(super) fn free_kerbs_near(&self, q: Vec2, r: f32, n: usize) -> Vec<KerbSlot> {
        self.free_slots_near(q, r, n, true)
    }

    /// [parking] Free slots on streets where parking is banned (shop
    /// fronts, clearways): a driver who found nothing legal stops there, in
    /// a slot of its own, and risks the ticket and the tow truck.
    pub(super) fn free_illegal_slots_near(&self, q: Vec2, r: f32, n: usize) -> Vec<KerbSlot> {
        self.free_slots_near(q, r, n, false)
    }

    /// Free slots, legal (`legal`) or on streets where parking is banned,
    /// within `r` of `q`, nearest first, one per side of a street. Both
    /// kinds share the kerb index, so no two cars ever stand in one slot.
    fn free_slots_near(&self, q: Vec2, r: f32, n: usize, legal: bool) -> Vec<KerbSlot> {
        let mut found: Vec<(f32, KerbSlot)> = Vec::new();
        let segs: Vec<SegmentId> = self.roads.segments_in_box(q - Vec2::splat(r), q + Vec2::splat(r)).collect();
        for sid in segs {
            if self.kerb_allowed(sid) != legal {
                continue;
            }
            let count = self.slot_count(sid);
            if count == 0 {
                continue;
            }
            let taken = self.vehicles.kerbs.get(&sid);
            let sides = self.kerb_sides(sid);
            for side in 0..2u8 {
                if !sides[side as usize] {
                    continue;
                }
                let mut best: Option<(f32, KerbSlot)> = None;
                for slot in 0..count {
                    if taken.is_some_and(|t| t.contains_key(&(side, slot))) {
                        continue;
                    }
                    let (pos, heading) = self.slot_pose(sid, side, slot);
                    let d = pos.distance(q);
                    if d <= r && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, KerbSlot { segment: sid, side, slot, pos, heading }));
                    }
                }
                found.extend(best);
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().take(n).map(|(_, k)| k).collect()
    }

    /// [parking] Legal kerb slots along one side of a street (for the API).
    pub fn kerb_slot_count(&self, sid: SegmentId) -> u16 {
        self.kerb_slots(sid)
    }

    /// Kerb slots in the city and how many are taken.
    pub fn kerb_occupancy(&self) -> (u32, u32) {
        let total: u32 = self.roads.alive_segments().map(|(sid, s)| self.kerb_slots(sid) as u32 * (s.lanes_fwd > 0) as u32 + self.kerb_slots(sid) as u32 * (s.lanes_bwd > 0) as u32).sum();
        let used: u32 = self.vehicles.kerbs.values().map(|m| m.len() as u32).sum();
        (total, used)
    }

    // ── parking and unparking ───────────────────────────────────────────

    /// Put a car in a space: the building's count, the kerb index, the
    /// owner's `parked_at`.
    pub(super) fn park_at(&mut self, vid: VehicleId, spot: ParkSpot) {
        let Some(car) = self.vehicles.cars.get_mut(vid) else { return };
        car.place = CarPlace::Parked(spot);
        let owner = car.owner;
        if spot.kind.uses_building() {
            if let Some(b) = spot.building.and_then(|b| self.buildings.get_mut(b)) {
                let before = b.parking_used * 8 / b.parking_capacity.max(1);
                b.parking_used = b.parking_used.saturating_add(1);
                if b.is_parking() && before != b.parking_used * 8 / b.parking_capacity.max(1) {
                    self.events.push(SimEvent::BuildingChanged(spot.building.unwrap()));
                }
            }
        }
        if spot.kind.on_street() && spot.slot != u16::MAX {
            if let Some(seg) = spot.segment {
                self.vehicles.kerbs.entry(seg).or_default().insert((spot.side, spot.slot), vid);
            }
        }
        if let Some(c) = self.citizens.get_mut(owner).filter(|c| c.car == Some(vid)) {
            c.parked_at = if spot.kind.uses_building() { spot.building } else { None };
        }
    }

    /// Take a car out of its space (it is about to drive, or to go).
    pub(super) fn unpark(&mut self, vid: VehicleId) -> Option<ParkSpot> {
        let spot = *self.vehicles.cars.get(vid)?.spot()?;
        if spot.kind.uses_building() {
            if let Some(b) = spot.building.and_then(|b| self.buildings.get_mut(b)) {
                let before = b.parking_used * 8 / b.parking_capacity.max(1);
                b.parking_used = b.parking_used.saturating_sub(1);
                if b.is_parking() && before != b.parking_used * 8 / b.parking_capacity.max(1) {
                    self.events.push(SimEvent::BuildingChanged(spot.building.unwrap()));
                }
            }
        }
        if spot.kind.on_street() {
            if let Some(m) = spot.segment.and_then(|s| self.vehicles.kerbs.get_mut(&s)) {
                if m.get(&(spot.side, spot.slot)) == Some(&vid) {
                    m.remove(&(spot.side, spot.slot));
                }
            }
        }
        let owner = self.vehicles.cars[vid].owner;
        if let Some(c) = self.citizens.get_mut(owner).filter(|c| c.car == Some(vid)) {
            c.parked_at = None;
        }
        Some(spot)
    }

    /// A spot in a building's spaces (home, garage, lot, forecourt).
    pub(super) fn building_spot(&self, kind: ParkKind, b: BuildingId) -> Option<ParkSpot> {
        let lot = self.building_lot(b)?;
        let pos = if lot.car_segment.is_some() { lot.car_point } else { lot.entrance() };
        Some(ParkSpot { kind, building: Some(b), segment: lot.car_segment, side: 0, slot: 0, pos, heading: 0.0, since: self.clock.tick, ..Default::default() })
    }

    /// A kerb slot as a parking spot: legal, or `Overflow` (parked where
    /// it is not allowed) when the street bans parking.
    pub(super) fn kerb_spot(&self, k: KerbSlot) -> ParkSpot {
        let kind = if self.kerb_allowed(k.segment) { ParkKind::Kerb } else { ParkKind::Overflow };
        ParkSpot { kind, building: None, segment: Some(k.segment), side: k.side, slot: k.slot, pos: k.pos, heading: k.heading, since: self.clock.tick, ..Default::default() }
    }

    /// [parking] Nowhere legal: the nearest free slot on a street where
    /// parking is banned, else a legal one further off. Always a slot of
    /// its own (it used to be the nearest kerb point, so cars that gave up
    /// near the same place stood on top of each other).
    fn overflow_spot(&self, p: Vec2) -> Option<ParkSpot> {
        let k = self
            .free_illegal_slots_near(p, KERB_WALK, 1)
            .into_iter()
            .next()
            .or_else(|| self.free_kerbs_near(p, 700.0, 1).into_iter().next())
            .or_else(|| self.free_illegal_slots_near(p, 700.0, 1).into_iter().next())?;
        Some(self.kerb_spot(k))
    }

    /// The best place to leave a car near `p` with nobody to pay: the
    /// owner's home if `home` has room, a kerb slot, else the street.
    pub(super) fn place_near(&self, p: Vec2, home: Option<BuildingId>) -> Option<ParkSpot> {
        if let Some(h) = home.filter(|h| self.buildings.get(*h).is_some_and(|b| b.parking_free() > 0)) {
            if self.building_lot(h).is_some_and(|l| l.entrance().distance(p) < 60.0) {
                return self.building_spot(ParkKind::Home, h);
            }
        }
        if let Some(k) = self.free_kerb_near(p, KERB_WALK) {
            return Some(self.kerb_spot(k));
        }
        self.overflow_spot(p)
    }

    // ── driving off ─────────────────────────────────────────────────────

    /// Where a parked car joins the road: its street and the point beside it.
    pub(super) fn spot_exit(&self, spot: &ParkSpot) -> Option<(SegmentId, Vec2)> {
        if spot.kind.on_street() {
            if let Some(s) = spot.segment.filter(|s| self.kerb_allowed(*s)) {
                return Some((s, spot.pos));
            }
        } else if let Some(stop) = spot.building.and_then(|b| self.truck_stop(b)) {
            return Some(stop);
        }
        self.roads.nearest_street(spot.pos).map(|(s, _, q)| (s, q))
    }

    /// Drive a parked car to `to` on `to_seg`: it pulls out of its space
    /// into the first lane. `None` (the car stays put) without a route.
    pub(super) fn drive_from_spot(&mut self, vid: VehicleId, to_seg: SegmentId, to_pt: Vec2, driver: CitizenId, errand: Errand) -> Option<CarId> {
        let spot = *self.vehicles.cars.get(vid)?.spot()?;
        let (fseg, fpt) = self.spot_exit(&spot)?;
        let (route, s0, s1) = self.route_any_lane(fseg, fpt, to_seg, to_pt)?;
        self.launch(vid, spot, route, s0, s1, driver, errand)
    }

    /// [parking] A route from a point beside a street: from the nearest
    /// lane, else from any lane of the street (pulling out the other way:
    /// the goal may lie behind on the near lane with no turning place).
    pub(super) fn route_any_lane(&self, fseg: SegmentId, fpt: Vec2, to_seg: SegmentId, to_pt: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        if let Some(r) = self.traffic.net.find_route(fseg, fpt, to_seg, to_pt) {
            return Some(r);
        }
        let (goal, s_end, _) = self.traffic.net.nearest_lane_on(to_seg, to_pt)?;
        let lanes = self.traffic.net.segment_lanes.get(&fseg)?.clone();
        lanes.into_iter().find_map(|l| {
            let (s, _) = self.traffic.net.lanes[l as usize].project(fpt);
            self.traffic.net.route_lanes(l, s, goal, s_end)
        })
    }

    /// Drive a parked car off the map (sold out of town, emigrating).
    pub(super) fn drive_out_of_town(&mut self, vid: VehicleId, errand: Errand) -> Option<CarId> {
        let spot = *self.vehicles.cars.get(vid)?.spot()?;
        let (fseg, fpt) = self.spot_exit(&spot)?;
        let (start, s0, _) = self.traffic.net.nearest_lane_on(fseg, fpt)?;
        let (route, s0, s1) = self.edge_exits(spot.pos).into_iter().find_map(|(goal, s_end)| self.traffic.net.route_lanes(start, s0, goal, s_end))?;
        self.launch(vid, spot, route, s0, s1, CitizenId::default(), errand)
    }

    pub(super) fn launch(&mut self, vid: VehicleId, spot: ParkSpot, route: Vec<LaneId>, s0: f32, s1: f32, driver: CitizenId, errand: Errand) -> Option<CarId> {
        // [boot] Clamped: it stays where it is until the tow truck comes.
        if spot.booted {
            return None;
        }
        self.unpark(vid);
        let (model, color) = {
            let c = &self.vehicles.cars[vid];
            (c.model, c.color)
        };
        let car = self.traffic.spawn_car(driver, None, route, s0, s1, color);
        let c = &mut self.traffic.cars[car];
        c.model = model;
        c.owned = Some(vid);
        c.errand = errand;
        // Drawn pulling out from the kerb or the driveway, not appearing in
        // the lane.
        let kerb_lane = self.traffic.net.lanes.get(c.route[0] as usize).is_some_and(|l| l.right.is_none());
        let off = if spot.kind.on_street() { spot.pos.distance(c.pos).min(2.6) } else if kerb_lane { 2.0 } else { 0.0 };
        c.lateral = c.lateral.max(off);
        self.vehicles.cars[vid].place = CarPlace::Driving(car);
        Some(car)
    }

    /// Exits off the map for a car near `from`, nearest first: highway
    /// carriageways leaving the map, then road ends at the city's edge.
    /// Each is a lane and where on it the car is gone.
    pub(super) fn edge_exits(&self, from: Vec2) -> Vec<(LaneId, f32)> {
        let (_, mut ends) = self.highway_ends();
        let near = |a: &(NodeId, SegmentId), b: &(NodeId, SegmentId)| self.roads.node(a.0).pos.distance(from).total_cmp(&self.roads.node(b.0).pos.distance(from));
        ends.sort_by(near);
        ends.truncate(3);
        let mut leaves: Vec<(NodeId, SegmentId)> = self
            .city_leaves()
            .into_iter()
            .filter_map(|n| self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|s| (n, s)))
            .collect();
        let max = leaves.iter().map(|(n, _)| self.roads.node(*n).pos.length()).fold(0.0, f32::max);
        leaves.retain(|(n, _)| self.roads.node(*n).pos.length() >= max * 0.6);
        leaves.sort_by(near);
        ends.extend(leaves.into_iter().take(3));
        ends.into_iter()
            .filter_map(|(n, sid)| {
                let at = self.roads.node(n).pos;
                let lanes = self.traffic.net.segment_lanes.get(&sid)?;
                let lane = *lanes.iter().min_by(|a, b| self.traffic.net.lanes[**a as usize].end().distance(at).total_cmp(&self.traffic.net.lanes[**b as usize].end().distance(at)))?;
                Some((lane, (self.traffic.net.lanes[lane as usize].length - 1.0).max(0.0)))
            })
            .collect()
    }

    /// [services] `edge_exits`, then every other road end leading off the
    /// map (the nearer ends `edge_exits` leaves to walkers), nearest first:
    /// for a tow that must get away from wherever a wreck lies.
    pub(super) fn any_exits(&self, from: Vec2) -> Vec<(LaneId, f32)> {
        let mut out = self.edge_exits(from);
        let mut leaves: Vec<NodeId> = self.city_leaves();
        leaves.sort_by(|a, b| self.roads.node(*a).pos.distance(from).total_cmp(&self.roads.node(*b).pos.distance(from)).then(a.0.cmp(&b.0)));
        for n in leaves {
            let at = self.roads.node(n).pos;
            let Some(sid) = self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()) else { continue };
            let Some(lanes) = self.traffic.net.segment_lanes.get(&sid) else { continue };
            let Some(&lane) = lanes.iter().min_by(|a, b| self.traffic.net.lanes[**a as usize].end().distance(at).total_cmp(&self.traffic.net.lanes[**b as usize].end().distance(at))) else { continue };
            if !out.iter().any(|&(l, _)| l == lane) {
                out.push((lane, (self.traffic.net.lanes[lane as usize].length - 1.0).max(0.0)));
            }
        }
        out
    }

    /// Send a car on the road off the map by the first exit it can reach.
    pub(super) fn redirect_out(&mut self, car: CarId) -> bool {
        let pos = self.traffic.cars[car].pos;
        self.edge_exits(pos).into_iter().any(|(goal, s_end)| self.traffic.redirect(car, goal, s_end))
    }

    /// [services] A city vehicle with nowhere left to go in town (its
    /// station, office, depot or line is gone, or no road leads back to
    /// it) drives off the map by the nearest exit it can reach, as
    /// `Errand::Out`, and leaves the world there (`out_arrived`). With no
    /// road out from where it stands it pulls in and tries again in a
    /// while. Vehicles only ever leave the world at the map edge or into a
    /// parking place or base: deleting one mid-street made fire engines,
    /// buses and tow trucks blink out in front of the player.
    pub(super) fn retire_vehicle(&mut self, car: CarId) {
        let Some(c) = self.traffic.cars.get_mut(car) else { return };
        c.siren = false;
        c.errand = Errand::Out;
        if !self.redirect_out(car) {
            self.traffic.pull_in(car, RETIRE_RETRY_SECONDS);
        }
    }

    /// [services] An `Errand::Out` vehicle at the end of its route: gone
    /// at the map edge, else (a vehicle that could not route out and held)
    /// it tries again from here.
    pub(super) fn out_arrived(&mut self, car: CarId) {
        if self.at_map_edge(car) {
            self.traffic.remove_car(car);
        } else {
            self.retire_vehicle(car);
        }
    }

    /// [services] Has `car` stopped where a route to `pt` on `seg` ends (the
    /// lane `nearest_lane_on` gives for it, at that place), or within
    /// `reach` metres of `pt`? Asked before a vehicle is taken off the road
    /// "at" a base, so one re-aimed elsewhere drives on instead.
    pub(super) fn car_at_stop(&self, car: CarId, seg: SegmentId, pt: Vec2, reach: f32) -> bool {
        let Some(c) = self.traffic.cars.get(car) else { return true };
        if c.pos.distance(pt) < reach {
            return true;
        }
        self.traffic.net.nearest_lane_on(seg, pt).is_some_and(|(goal, s_end, _)| c.route.last() == Some(&goal) && (c.s - s_end).abs() < 3.0)
    }

    /// [services] A route off the map for a vehicle standing at `from` (a
    /// unit parked on scene, a truck at a kerb): from the street beside it
    /// to the nearest exit it can reach.
    pub(super) fn route_out_from(&self, from: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        let (seg, _, q) = self.roads.nearest_street(from)?;
        let (start, s0, _) = self.traffic.net.nearest_lane_on(seg, q)?;
        self.edge_exits(from).into_iter().find_map(|(goal, s_end)| self.traffic.net.route_lanes(start, s0, goal, s_end))
    }

    /// A route onto the map from out of town to `to` on `to_seg`: in on a
    /// highway carriageway entering the map when one leads there, else
    /// along a road from the city's edge.
    pub(super) fn route_from_edge(&mut self, to_seg: SegmentId, to_pt: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        let (goal, s_end, _) = self.traffic.net.nearest_lane_on(to_seg, to_pt)?;
        self.route_from_edge_to(goal, s_end)
    }

    /// `route_from_edge` to a given lane: `s_end` metres along `goal`.
    pub(super) fn route_from_edge_to(&mut self, goal: LaneId, s_end: f32) -> Option<(Vec<LaneId>, f32, f32)> {
        let (mut highway, _) = self.highway_ends();
        highway.shuffle(&mut self.rng);
        let mut leaves: Vec<(NodeId, SegmentId)> = self
            .city_leaves()
            .into_iter()
            .filter_map(|n| self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|s| (n, s)))
            .collect();
        // The far ends of the city's roads, as people walking in use.
        let max = leaves.iter().map(|(n, _)| self.roads.node(*n).pos.length()).fold(0.0, f32::max);
        leaves.retain(|(n, _)| self.roads.node(*n).pos.length() >= max * 0.6);
        leaves.shuffle(&mut self.rng);
        for (n, sid) in highway.into_iter().take(3).chain(leaves.into_iter().take(3)) {
            if let Some(r) = self.route_in_from(n, sid, goal, s_end) {
                return Some(r);
            }
        }
        // [services] Then any road end leading off the map, nearest first:
        // a recovery truck must get to a wreck however it can.
        let to = self.traffic.net.lanes.get(goal as usize)?.point_at(s_end).0;
        let mut all: Vec<(NodeId, SegmentId)> = self
            .city_leaves()
            .into_iter()
            .filter_map(|n| self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|s| (n, s)))
            .collect();
        all.sort_by(|a, b| self.roads.node(a.0).pos.distance(to).total_cmp(&self.roads.node(b.0).pos.distance(to)).then(a.0 .0.cmp(&b.0 .0)));
        all.into_iter().find_map(|(n, sid)| self.route_in_from(n, sid, goal, s_end))
    }

    /// A route in from the road end `n` (segment `sid`) to `s_end` on `goal`.
    fn route_in_from(&self, n: NodeId, sid: SegmentId, goal: LaneId, s_end: f32) -> Option<(Vec<LaneId>, f32, f32)> {
        let at = self.roads.node(n).pos;
        let lanes = self.traffic.net.segment_lanes.get(&sid)?;
        let &lane = lanes.iter().min_by(|a, b| self.traffic.net.lanes[**a as usize].start().distance(at).total_cmp(&self.traffic.net.lanes[**b as usize].start().distance(at)))?;
        let s0 = 1.0f32.min(self.traffic.net.lanes[lane as usize].length * 0.5);
        self.traffic.net.route_lanes(lane, s0, goal, s_end)
    }

    /// The citizen's car, if it is parked within reach of `near`.
    pub(super) fn car_ready(&self, id: CitizenId, near: Vec2) -> Option<(VehicleId, Vec2)> {
        // `owns_car` mirrors `car`; a test or a tool clearing it means "no car".
        let c = self.citizens.get(id).filter(|c| c.owns_car)?;
        let vid = c.car?;
        let spot = self.vehicles.cars.get(vid)?.spot()?;
        // [boot] A clamped car goes nowhere.
        (!matches!(spot.kind, ParkKind::Dealer | ParkKind::Hooked | ParkKind::Pound) && !spot.booted && spot.pos.distance(near) <= CAR_REACH).then_some((vid, spot.pos))
    }

    /// Get in and drive to `target`. False when the car cannot get there
    /// (no route); the citizen is then left as they were.
    pub(super) fn start_drive(&mut self, id: CitizenId, vid: VehicleId, target: BuildingId, purpose: Purpose) -> bool {
        crate::prof_scope!("start_drive");
        let Some(lot) = self.building_lot(target) else { return false };
        let Some((ct_seg, ct_pt)) = lot.car_segment.map(|s| (s, lot.car_point)) else { return false };
        let spot = self.vehicles.cars.get(vid).and_then(|c| c.spot().copied());
        if spot.is_some_and(|s| s.kind == ParkKind::Hooked) {
            return false;
        }
        let Some(car) = self.drive_from_spot(vid, ct_seg, ct_pt, id, Errand::Trip) else { return false };
        // [parking] Fetching it from the pound: pay to get it out.
        if let Some(s) = spot.filter(|s| s.kind == ParkKind::Pound) {
            self.pay_release(id, s);
        }
        self.leave_building(id);
        let (pos, heading, model) = {
            let c = &self.traffic.cars[car];
            (c.pos, c.heading, c.model)
        };
        let c = &mut self.citizens[id];
        c.pos = pos;
        c.prev_pos = pos;
        c.heading = heading;
        c.vehicle = Some(model);
        c.to_car = None;
        c.commute.drive_since = self.clock.tick.max(1);
        c.activity = Activity::Driving { car, to: Some(target), purpose };
        true
    }

    /// Walk to the parked car first, then drive (`arrive` picks it up).
    pub(super) fn walk_to_car(&mut self, id: CitizenId, vid: VehicleId, at: Vec2, target: BuildingId, purpose: Purpose, from: Vec2) {
        crate::prof_scope!("walk_to_car");
        let exit = self.leave_building(id);
        let start = exit.unwrap_or(from);
        let mut path = match self.walk.route(start, at) {
            Some((pts, _)) => pts,
            None => vec![start],
        };
        path.push(at);
        let c = &mut self.citizens[id];
        c.pos = start;
        c.prev_pos = start;
        c.mode = TravelMode::Walk;
        c.to_car = Some(ToCar { car: vid, to: target, purpose });
        c.activity = Activity::Traveling { to: Some(target), purpose, path, idx: 0 };
    }

    // ── arriving ────────────────────────────────────────────────────────

    /// A vehicle of ours reached the end of its route. True when handled
    /// (anything but a through car, a bus or a service unit).
    pub(super) fn vehicle_arrived(&mut self, car_id: CarId) -> bool {
        let Some(car) = self.traffic.cars.get(car_id) else { return true };
        let (errand, owned, cargo, citizen) = (car.errand, car.owned, car.cargo, car.citizen);
        if let Some(_) = cargo {
            // Lorries are `world/industry`'s: deliver, unload at the kerb,
            // then a truck lot, their business or the map edge.
            self.lorry_arrived(car_id);
            return true;
        }
        match errand {
            Errand::Haul { base } => {
                self.send_back(car_id, base);
                true
            }
            Errand::Return(b) => {
                self.traffic.remove_car(car_id);
                if self.buildings.contains_key(b) {
                    *self.vehicles.fleet_parked.entry(b).or_insert(0) += 1;
                    self.vehicles.stats.fleet_returned_today += 1;
                }
                true
            }
            Errand::Leave | Errand::Sale(_) => {
                self.traffic.remove_car(car_id);
                if let Some(vid) = owned {
                    self.drop_car(vid, if errand == Errand::Leave { CarEvent::Emigrated } else { CarEvent::Sold });
                }
                true
            }
            Errand::Patrol(office) => {
                self.patrol_arrived(car_id, office);
                true
            }
            Errand::Tow(job) => {
                self.tow_truck_arrived(car_id, job);
                true
            }
            Errand::Recover(wreck) => {
                self.recovery_truck_arrived(car_id, wreck);
                true
            }
            Errand::Taxi(cab) => {
                self.cab_arrived(car_id, cab);
                true
            }
            Errand::Out => {
                self.out_arrived(car_id);
                true
            }
            // Its route ends at a highway exit off the map.
            Errand::Through => {
                self.traffic.remove_car(car_id);
                true
            }
            Errand::Deliver | Errand::Trip | Errand::Search { .. } | Errand::ToLot(_) | Errand::ToKerb => {
                if owned.is_some() || self.citizens.contains_key(citizen) {
                    self.private_arrived(car_id);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// A truck or van done with its job drives back to `base`, or out of
    /// town. With no road to either it holds at the kerb and tries again
    /// (`retire_vehicle`): it no longer vanishes where it stands.
    fn send_back(&mut self, car_id: CarId, base: Option<BuildingId>) {
        if let Some(b) = base.filter(|b| self.buildings.contains_key(*b)) {
            if let Some((seg, pt)) = self.truck_stop(b) {
                if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, pt) {
                    if self.traffic.redirect(car_id, goal, s_end) {
                        self.traffic.cars[car_id].errand = Errand::Return(b);
                        self.traffic.pull_in(car_id, 45.0);
                        return;
                    }
                }
            }
        }
        if self.redirect_out(car_id) {
            self.traffic.cars[car_id].errand = Errand::Leave;
            return;
        }
        self.retire_vehicle(car_id);
    }

    /// The business a truck or van belongs to and returns to: the sender,
    /// when it is a working business with a street. Hook for fleet lots: a
    /// business that keeps its lorries elsewhere can answer that here.
    pub(super) fn truck_base(&self, from: Option<BuildingId>) -> Option<BuildingId> {
        let b = from?;
        let bld = self.buildings.get(b)?;
        (bld.business.is_some() && bld.construction.is_none() && self.building_lot(b).is_some_and(|l| l.car_segment.is_some())).then_some(b)
    }

    /// A truck or van was just put on the road by `from`: it takes one of
    /// the business's parked vehicles (or the business buys one) and will
    /// come back when done.
    pub(crate) fn truck_dispatched(&mut self, car: CarId, from: Option<BuildingId>) {
        let base = self.truck_base(from);
        if let Some(b) = base {
            match self.vehicles.fleet_parked.get_mut(&b) {
                Some(n) if *n > 0 => *n -= 1,
                _ => self.vehicles.stats.fleet_bought_total += 1,
            }
        }
        if let Some(c) = self.traffic.cars.get_mut(car) {
            c.errand = Errand::Haul { base };
        }
    }

    /// A private car at the end of its route: park it, or go on looking.
    fn private_arrived(&mut self, car_id: CarId) {
        // [passengers] A stop on the way (a child's school): let them out
        // and drive on.
        if self.traffic.cars.get(car_id).is_some_and(|c| c.errand == Errand::Trip) && self.drop_off(car_id) {
            return;
        }
        let (vid, driver, errand, pos) = {
            let c = &self.traffic.cars[car_id];
            (c.owned, c.citizen, c.errand, c.pos)
        };
        let Some(vid) = vid.filter(|v| self.vehicles.cars.contains_key(*v)) else {
            // A car with no record (a save from before cars were kept):
            // the driver gets out; the car parks where it is under them.
            self.traffic.remove_car(car_id);
            self.walk_from_car(driver, pos);
            return;
        };
        let owner = self.vehicles.cars[vid].owner;
        let home = self.citizens.get(owner).and_then(|c| c.home);
        let dest = if errand == Errand::Deliver {
            home
        } else {
            match self.citizens.get(driver).map(|c| &c.activity) {
                Some(Activity::Driving { to, .. }) => *to,
                _ => None,
            }
        };
        let target = dest.and_then(|b| self.building_entrance(b)).unwrap_or(pos);
        // First arrival: the destination's own spaces.
        if matches!(errand, Errand::Trip | Errand::Deliver) {
            if let Some(b) = dest {
                let own = self.buildings.get(b).is_some_and(|bld| !bld.is_parking() && bld.parking_free() > 0);
                if own {
                    let kind = if Some(b) == home { ParkKind::Home } else { ParkKind::Building };
                    if let Some(spot) = self.building_spot(kind, b) {
                        self.finish_park(car_id, vid, spot, target, 0);
                        return;
                    }
                }
            }
        }
        let tries = match errand {
            Errand::Search { tries } => tries,
            Errand::ToLot(_) | Errand::ToKerb => 1,
            _ => 0,
        };
        if let Errand::ToLot(b) = errand {
            if self.lot_open(b) {
                if let Some(spot) = self.building_spot(ParkKind::Lot, b) {
                    self.pay_lot(driver, b);
                    self.finish_park(car_id, vid, spot, target, tries);
                    return;
                }
            }
        }
        if errand == Errand::ToKerb {
            if let Some(k) = self.free_kerb_near(pos, 16.0).or_else(|| self.free_illegal_slots_near(pos, 16.0, 1).into_iter().next()) {
                let spot = self.kerb_spot(k);
                self.finish_park(car_id, vid, spot, target, tries);
                return;
            }
        }
        self.look_for_space(car_id, vid, driver, target, tries);
    }

    /// [parking] A driver who has hunted for a space past `HUNT_GIVE_UP`
    /// and is standing in a queue (`HUNT_STUCK` seconds) does not wait to
    /// reach the street it was making for: it leaves the car at the kerb
    /// where it is (a banned kerb nearby, a free one, else squeezed in).
    /// Hunters going round the same blocks filled a dead-end street and
    /// its junction solid in a player's city, each waiting for the next,
    /// for hours; every one that gives up leaves the gap the rest need.
    pub(super) fn hunters_give_up(&mut self) {
        let tick = self.clock.tick;
        let stuck: Vec<CarId> = self
            .traffic
            .cars
            .iter()
            .filter(|(_, c)| matches!(c.errand, Errand::Search { .. } | Errand::ToKerb | Errand::ToLot(_)) && !c.entering && c.hold <= 0.0 && !c.crashed() && c.v < 0.5 && c.stalled > HUNT_STUCK)
            .filter(|(_, c)| self.citizens.get(c.citizen).is_some_and(|z| z.commute.search_since > 0 && tick.saturating_sub(z.commute.search_since) > HUNT_GIVE_UP))
            .map(|(id, _)| id)
            .collect();
        for id in stuck {
            if let Some(c) = self.traffic.cars.get_mut(id) {
                // Not at the lot or the slot it was making for: a search
                // from here.
                c.errand = Errand::Search { tries: MAX_TRIES };
                self.private_arrived(id);
            }
        }
    }

    pub(super) fn lot_open(&self, b: BuildingId) -> bool {
        self.buildings.get(b).is_some_and(|bld| bld.is_parking() && bld.parking_free() > 0 && bld.business.as_ref().is_some_and(|z| !z.closed))
    }

    /// Money per minute of a citizen's time, as they see it: the poor count
    /// every coin. Minute-equivalents per unit of money.
    pub(super) fn money_weight(&self, id: CitizenId) -> f32 {
        let income = self.citizens.get(id).map_or(60.0, |c| c.income);
        (300.0 / income.max(25.0)).clamp(1.0, 12.0)
    }

    /// No space where the car stopped: weigh a free kerb slot against a
    /// paid lot near the destination and drive to the better; with neither,
    /// circle the streets around, and after `MAX_TRIES` park anywhere.
    fn look_for_space(&mut self, car_id: CarId, vid: VehicleId, driver: CitizenId, target: Vec2, tries: u8) {
        let pos = self.traffic.cars[car_id].pos;
        if tries > 0 || self.citizens.get(driver).is_some_and(|c| c.commute.search_since == 0) {
            // The search clock starts the first time the driver has to look.
            if let Some(c) = self.citizens.get_mut(driver) {
                if c.commute.search_since == 0 {
                    c.commute.search_since = self.clock.tick.max(1);
                }
            }
        }
        // Candidates: the nearest free kerb slots and parking businesses,
        // each costed as the driver sees it: the drive there (by the route
        // it would really take, round the block if need be), the walk back
        // and the fee.
        let walk = self.config.walk_speed.max(0.1);
        let drive = 9.0 * self.config.traffic_seconds_per_tick;
        let money_w = self.money_weight(driver);
        let mut options: Vec<(f32, Vec<LaneId>, f32, Errand, Option<KerbSlot>)> = Vec::new();
        // [parking] How long this driver has been at it. A driver sent to a
        // kerb slot or a lot that was taken on arrival starts that leg at
        // one try again, so only the clock tells how long the hunt has run.
        let hunted = self.citizens.get(driver).filter(|c| c.commute.search_since > 0).map_or(0, |c| self.clock.tick.saturating_sub(c.commute.search_since));
        // After a few fruitless rounds (or a while at it) the driver looks
        // further, and at slots where parking is banned (a fine, maybe the
        // tow truck).
        let desperate = tries >= MAX_TRIES || hunted > HUNT_DESPERATE;
        // [parking] Hours of circling help nobody: past `HUNT_GIVE_UP` the
        // driver stops looking and leaves the car wherever it can.
        let worn_out = hunted > HUNT_GIVE_UP;
        let reach = if desperate { KERB_WALK * 2.0 } else { KERB_WALK };
        let mut slots: Vec<(KerbSlot, f32)> = if worn_out { Vec::new() } else { self.free_kerbs_near(target, reach, 5).into_iter().map(|k| (k, 0.0)).collect() };
        if desperate && !worn_out {
            slots.extend(self.free_illegal_slots_near(target, KERB_WALK, 3).into_iter().map(|k| (k, 12.0)));
        }
        for (k, risk) in slots {
            let walk_min = k.pos.distance(target) / walk + risk;
            if k.pos.distance(pos) < 14.0 {
                // Right here: pull in.
                options.push((walk_min, Vec::new(), 0.0, Errand::ToKerb, Some(k)));
                continue;
            }
            let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(k.segment, k.pos) else { continue };
            let Some(route) = self.traffic.route_from_here(car_id, goal, s_end) else { continue };
            let m = self.traffic.route_metres(car_id, &route, s_end);
            options.push((walk_min + m / drive, route, s_end, Errand::ToKerb, None));
        }
        let mut lots: Vec<(f32, BuildingId, f32)> = self
            .buildings
            .iter()
            .filter(|(id, _)| self.lot_open(*id))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.center.distance(target), id, b.parking_price)))
            .filter(|(d, _, _)| *d <= LOT_WALK && !worn_out)
            .collect();
        lots.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (d, b, fee) in lots.into_iter().take(3) {
            let Some((seg, pt)) = self.truck_stop(b) else { continue };
            let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { continue };
            let Some(route) = self.traffic.route_from_here(car_id, goal, s_end) else { continue };
            let m = self.traffic.route_metres(car_id, &route, s_end);
            options.push((d / walk + m / drive + fee * money_w, route, s_end, Errand::ToLot(b), None));
        }
        let best = options.into_iter().min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, route, s_end, errand, here)) = best {
            if let Some(k) = here {
                let spot = self.kerb_spot(k);
                self.finish_park(car_id, vid, spot, target, tries);
                return;
            }
            if self.traffic.follow(car_id, route, s_end) {
                self.traffic.cars[car_id].errand = errand;
                return;
            }
        }
        // Nothing free in sight (or no way to it from here: lanes never
        // lead back onto the one the car is on): drive round the block
        // and look again.
        if tries < GIVE_UP && !worn_out {
            let r = if desperate { SEARCH_RADIUS * 2.0 } else { SEARCH_RADIUS };
            let mut segs: Vec<SegmentId> = self.roads.segments_in_box(target - Vec2::splat(r), target + Vec2::splat(r)).filter(|&s| self.roads.try_segment(s).is_some_and(|g| g.is_street() && !g.is_highway() && g.lanes() > 0)).collect();
            segs.sort_by_key(|s| s.0);
            if tries == 0 {
                self.vehicles.stats.circled_today += 1;
                self.economy.parking_street_today += 1;
                self.economy.parking_shortage_at = Some(target);
            }
            for _ in 0..4 {
                let Some(&sid) = segs.choose(&mut self.rng) else { break };
                let (a, b) = self.roads.segment_points(sid);
                let mid = a.lerp(b, 0.5);
                if mid.distance(pos) < 30.0 {
                    continue;
                }
                if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(sid, mid) {
                    if self.traffic.redirect(car_id, goal, s_end) {
                        self.traffic.cars[car_id].errand = Errand::Search { tries: tries + 1 };
                        return;
                    }
                }
            }
        }
        // Gave up: onto the street, wherever.
        if tries == 0 {
            self.economy.parking_street_today += 1;
            self.economy.parking_shortage_at = Some(target);
        }
        match self.overflow_spot(pos) {
            Some(spot) => self.finish_park(car_id, vid, spot, target, tries.max(1)),
            None if !worn_out => {
                // Not one free slot within reach: keep driving round.
                if let Some(c) = self.traffic.cars.get_mut(car_id) {
                    c.errand = Errand::Search { tries: MAX_TRIES };
                }
                self.circle(car_id, target);
            }
            None => {
                // An hour of it and not one slot free within 700 m: the car
                // is left where it stands, squeezed in at the kerb (it used
                // to drive round for good: drivers hunting two to ten hours
                // for a space, through the jam they were part of).
                let spot = ParkSpot { kind: ParkKind::Overflow, pos, slot: u16::MAX, since: self.clock.tick, ..Default::default() };
                self.finish_park(car_id, vid, spot, target, tries.max(1));
            }
        }
    }

    /// [parking] Drive round the block towards some street near `target`.
    fn circle(&mut self, car_id: CarId, target: Vec2) {
        let r = SEARCH_RADIUS * 2.0;
        let mut segs: Vec<SegmentId> = self.roads.segments_in_box(target - Vec2::splat(r), target + Vec2::splat(r)).filter(|&s| self.roads.try_segment(s).is_some_and(|g| g.is_street() && !g.is_highway() && g.lanes() > 0)).collect();
        segs.sort_by_key(|s| s.0);
        for _ in 0..6 {
            let Some(&sid) = segs.choose(&mut self.rng) else { break };
            let (a, b) = self.roads.segment_points(sid);
            if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(sid, a.lerp(b, 0.5)) {
                if self.traffic.redirect(car_id, goal, s_end) {
                    return;
                }
            }
        }
    }

    /// Pay a parking business for a space.
    pub(super) fn pay_lot(&mut self, driver: CitizenId, b: BuildingId) {
        let tick = self.clock.tick;
        let (fee, name) = {
            let bld = &self.buildings[b];
            (bld.parking_price, bld.name.clone())
        };
        if self.vehicles.city_lots.contains_key(&b) {
            // [parking] A lot the city runs: the fee goes to the treasury.
            self.economy.credit_item(Flow::ParkingFees, fee, Item::Building(b));
            self.enforcement.stats.city_lot_fees_today += fee;
            if let Some(bz) = self.buildings[b].business.as_mut() {
                bz.deals += 1;
            }
        } else if let Some(bz) = self.buildings[b].business.as_mut() {
            bz.money += fee;
            bz.revenue_today += fee;
            bz.deals += 1;
        }
        self.economy.parking_fees_today += fee;
        let roll = self.rng.random_bool(0.05);
        if let Some(c) = self.citizens.get_mut(driver) {
            c.savings -= fee;
            c.commute.fee += fee;
            if roll {
                c.remember(tick, format!("Paid {fee:.0} to park at {name}."));
            }
        }
    }

    /// The car is in its space: off the road, into the record; the driver
    /// gets out and walks the rest.
    fn finish_park(&mut self, car_id: CarId, vid: VehicleId, spot: ParkSpot, target: Vec2, tries: u8) {
        let Some(car) = self.traffic.remove_car(car_id) else { return };
        self.park_at(vid, spot);
        let slot = match spot.kind {
            ParkKind::Home | ParkKind::Building | ParkKind::Dealer | ParkKind::Hooked | ParkKind::Pound => 0,
            ParkKind::Lot => 1,
            ParkKind::Kerb => 2,
            ParkKind::Overflow => 3,
        };
        self.vehicles.stats.parked_today[slot] += 1;
        let tick = self.clock.tick;
        if car.errand == Errand::Deliver {
            let owner = self.vehicles.cars[vid].owner;
            if let Some(c) = self.citizens.get_mut(owner) {
                c.remember(tick, "The new wheels were delivered to the door.");
            }
            return;
        }
        let driver = car.citizen;
        let secs = self.config.traffic_seconds_per_tick;
        let roll = self.rng.random_bool(0.15);
        let Some(c) = self.citizens.get_mut(driver) else { return };
        if c.commute.search_since > 0 {
            let s = tick.saturating_sub(c.commute.search_since) as f32 * secs;
            c.commute.searched += s;
            c.commute.search_since = 0;
            self.vehicles.stats.search_seconds_today += s;
        }
        if c.commute.drive_since > 0 {
            c.commute.drive_minutes = tick.saturating_sub(c.commute.drive_since) as f32;
            c.commute.drive_since = 0;
        }
        // [rage] How the drive went to their head; the anger stays in the car.
        c.commute.rage = c.commute.rage.max(car.drive.anger_peak);
        if car.drive.expected_min > 0.0 {
            c.commute.expected_drive = car.drive.expected_min;
        }
        if spot.kind == ParkKind::Overflow {
            c.mood = (c.mood - 0.03).max(0.0);
            if roll {
                c.remember(tick, "Circled for ages. Nowhere to park, so I left it on the street.");
            }
            self.concerns.said(tick, driver, c, crate::concerns::Concern::Parking, 0.4, "Circled for ages. Nowhere to park, so I left it on the street.");
        } else if tries >= 2 && roll {
            c.remember(tick, "Went round the block twice before I found a space.");
            self.concerns.heard(driver, c, crate::concerns::Concern::Parking, 0.2);
        }
        let _ = target;
        self.walk_from_car(driver, spot.pos);
    }

    /// Out of the car at `from`: walk to the destination, or be there.
    pub(super) fn walk_from_car(&mut self, id: CitizenId, from: Vec2) {
        let Some(c) = self.citizens.get(id) else { return };
        let (to, purpose) = match &c.activity {
            Activity::Driving { to, purpose, .. } => (*to, *purpose),
            _ => return,
        };
        let Some(entrance) = to.and_then(|b| self.building_entrance(b)) else {
            let c = &mut self.citizens[id];
            c.pos = from;
            self.arrive(id);
            return;
        };
        if from.distance(entrance) < GET_OUT {
            let c = &mut self.citizens[id];
            c.pos = from;
            self.arrive(id);
            return;
        }
        let mut path = match self.walk.route(from, entrance) {
            Some((pts, _)) => pts,
            None => vec![from],
        };
        path.push(entrance);
        let c = &mut self.citizens[id];
        c.pos = from;
        c.mode = TravelMode::Walk;
        c.commute.walk_after = from.distance(entrance);
        c.activity = Activity::Traveling { to, purpose, path, idx: 0 };
    }

    // ── coming and going ────────────────────────────────────────────────

    pub(super) fn new_car(&mut self, owner: CitizenId, model: VehicleModel, place: CarPlace, event: CarEvent) -> VehicleId {
        let color = self.rng.random_range(0..8u8);
        let vid = self.vehicles.cars.insert(OwnedCar { owner, model, place, color, since: self.clock.tick });
        self.vehicles.note(event);
        if let Some(c) = self.citizens.get_mut(owner) {
            c.car = Some(vid);
            c.owns_car = true;
            c.vehicle = Some(model);
        }
        vid
    }

    /// A car leaves the city for good: sold, scrapped or driven away.
    pub(super) fn drop_car(&mut self, vid: VehicleId, event: CarEvent) {
        if self.vehicles.cars.get(vid).and_then(|c| c.spot()).is_some() {
            self.unpark(vid);
        }
        let Some(car) = self.vehicles.cars.remove(vid) else { return };
        self.vehicles.note(event);
        if let Some(c) = self.citizens.get_mut(car.owner).filter(|c| c.car == Some(vid)) {
            c.car = None;
            c.owns_car = false;
            c.parked_at = None;
        }
    }

    /// A newcomer drives in from out of town with their car, to `home`.
    /// False when there is no road in; they come without it.
    pub(super) fn arrive_by_car(&mut self, id: CitizenId, model: VehicleModel, home: BuildingId) -> bool {
        let Some(lot) = self.building_lot(home) else { return false };
        let Some((ct_seg, ct_pt)) = lot.car_segment.map(|s| (s, lot.car_point)) else { return false };
        let Some((route, s0, s1)) = self.route_from_edge(ct_seg, ct_pt) else { return false };
        let color = self.rng.random_range(0..8u8);
        let car = self.traffic.spawn_car(id, None, route, s0, s1, color);
        let vid = self.new_car(id, model, CarPlace::Driving(car), CarEvent::ArrivedWithHousehold);
        let (pos, heading) = {
            let c = &mut self.traffic.cars[car];
            c.model = model;
            c.owned = Some(vid);
            (c.pos, c.heading)
        };
        let tick = self.clock.tick;
        let c = &mut self.citizens[id];
        c.pos = pos;
        c.prev_pos = pos;
        c.heading = heading;
        c.activity = Activity::Driving { car, to: Some(home), purpose: Purpose::Home };
        c.commute.mode = Some(TripMode::Car);
        c.commute.started = tick;
        c.commute.distance = pos.distance(ct_pt);
        true
    }

    /// The car dealer for a buyer at `p`: an open shop selling goods with a
    /// street in front (the gadget shops double as dealers), else any open
    /// shop. `None` when the town has none: cars come from out of town.
    pub(super) fn dealer_near(&self, p: Vec2) -> Option<BuildingId> {
        self.specialist_near(p, BusinessKind::CarDealer, CAR_DEALER_REACH).or_else(|| self.any_shop_near(p))
    }

    /// [economy] Where a bike is bought: a bike shop in reach, else as a
    /// car (a goods or gadget shop doubles).
    pub(super) fn bike_shop_near(&self, p: Vec2) -> Option<BuildingId> {
        self.specialist_near(p, BusinessKind::BikeShop, BIKE_SHOP_REACH).or_else(|| self.any_shop_near(p))
    }

    /// [economy] The nearest open business of `kind` with a street in
    /// front within `reach`.
    fn specialist_near(&self, p: Vec2, kind: BusinessKind, reach: f32) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.construction.is_none() && b.business.as_ref().is_some_and(|z| z.kind == kind && !z.closed))
            .filter_map(|(id, b)| self.lots.get(b.lot).filter(|l| l.car_access()).map(|l| (l.center.distance(p), id)))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, id)| id)
    }

    fn any_shop_near(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Commercial && b.construction.is_none() && b.port.is_none() && b.business.as_ref().is_some_and(|z| !z.closed && !b.is_parking()))
            .filter(|(id, _)| self.building_lot(*id).is_some_and(|l| l.car_access()))
            .filter_map(|(id, b)| {
                let goods = b.business.as_ref().is_some_and(|z| z.sells(Resource::Goods) || z.sells(Resource::Electronics));
                self.lots.get(b.lot).map(|l| (l.center.distance(p) + if goods { 0.0 } else { 600.0 }, id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    /// Buy a car: at a dealer in town (it is delivered home from the
    /// forecourt), else from out of town (driven in from the map edge).
    /// Returns what was paid, or `None` when it could not be done.
    pub(super) fn buy_car(&mut self, id: CitizenId, model: VehicleModel, price: f32) -> Option<f32> {
        let c = self.citizens.get(id)?;
        if c.car.is_some() {
            return None;
        }
        let home = c.home?;
        let at = self.building_entrance(home)?;
        let tick = self.clock.tick;
        match self.dealer_near(at) {
            Some(dealer) => {
                let spot = self.building_spot(ParkKind::Dealer, dealer)?;
                let vid = self.new_car(id, model, CarPlace::Parked(spot), CarEvent::Bought);
                // The forecourt is not a parking space anyone counts.
                let name = self.buildings[dealer].name.clone();
                if let Some(bz) = self.buildings[dealer].business.as_mut() {
                    // The dealer's margin; the rest goes to the maker out of town.
                    bz.money += price * 0.25;
                    bz.revenue_today += price * 0.25;
                    bz.deals += 1;
                }
                self.deliver_car(vid);
                self.vehicles.stats.bought_today += 1;
                self.citizens[id].remember(tick, format!("Bought a {} at {name}.", model_label(&model)));
                Some(price)
            }
            None => {
                let lot = self.building_lot(home)?;
                let (ct_seg, ct_pt) = lot.car_segment.map(|s| (s, lot.car_point))?;
                let (route, s0, s1) = self.route_from_edge(ct_seg, ct_pt)?;
                let color = self.rng.random_range(0..8u8);
                let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, color);
                let vid = self.new_car(id, model, CarPlace::Driving(car), CarEvent::BoughtOutOfTown);
                let c = &mut self.traffic.cars[car];
                c.model = model;
                c.owned = Some(vid);
                c.errand = Errand::Deliver;
                self.vehicles.stats.bought_today += 1;
                self.citizens[id].remember(tick, format!("Ordered a {} from out of town. It's being driven in.", model_label(&model)));
                Some(price)
            }
        }
    }

    /// Drive a car waiting on a dealer's forecourt to its owner's home.
    pub(super) fn deliver_car(&mut self, vid: VehicleId) {
        let Some(owner) = self.vehicles.cars.get(vid).map(|c| c.owner) else { return };
        let Some(home) = self.citizens.get(owner).and_then(|c| c.home) else { return };
        let Some(lot) = self.building_lot(home) else { return };
        let Some((seg, pt)) = lot.car_segment.map(|s| (s, lot.car_point)) else { return };
        self.drive_from_spot(vid, seg, pt, CitizenId::default(), Errand::Deliver);
    }

    /// Sell the citizen's car: the dealer's driver fetches it from where it
    /// is parked (out of town without a dealer). A car on the road is not
    /// sold. Returns the money got.
    pub(super) fn sell_car(&mut self, id: CitizenId, reason: &str) -> Option<f32> {
        let vid = self.citizens.get(id)?.car?;
        let car = self.vehicles.cars.get(vid)?;
        let spot = *car.spot()?;
        // [parking] Not while it is on a tow truck or in the pound.
        if matches!(spot.kind, ParkKind::Hooked | ParkKind::Pound) || spot.booted {
            return None;
        }
        let worth = (car.model.price() * 0.5).min(400.0);
        let tick = self.clock.tick;
        let dealer = self.dealer_near(spot.pos);
        let driven = match dealer {
            Some(d) => self.truck_stop(d).and_then(|(seg, pt)| self.drive_from_spot(vid, seg, pt, CitizenId::default(), Errand::Sale(d))),
            None => self.drive_out_of_town(vid, Errand::Sale(BuildingId::default())),
        };
        let paid = if driven.is_some() {
            if let Some(bz) = dealer.and_then(|d| self.buildings.get_mut(d)).and_then(|b| b.business.as_mut()) {
                bz.deals += 1;
            }
            worth
        } else {
            // Nobody can drive it away: it goes for scrap where it stands.
            self.drop_car(vid, CarEvent::Scrapped);
            worth * 0.2
        };
        // The owner is done with it either way.
        let c = &mut self.citizens[id];
        c.car = None;
        c.owns_car = false;
        c.vehicle = None;
        c.parked_at = None;
        c.remember(tick, reason.to_string());
        self.vehicles.stats.sold_today += 1;
        Some(paid)
    }

    /// The owner is leaving town: the car goes with them, off the map.
    pub(super) fn car_emigrates(&mut self, id: CitizenId) {
        let Some(vid) = self.citizens.get(id).and_then(|c| c.car) else { return };
        let Some(car) = self.vehicles.cars.get(vid) else { return };
        match car.place {
            // [parking] On a tow truck or in a pound out of town: left
            // behind, it leaves the record with its owner.
            CarPlace::Parked(s) if s.kind == ParkKind::Hooked || (s.kind == ParkKind::Pound && s.building.is_none()) => {
                self.drop_car(vid, CarEvent::Emigrated);
            }
            // [boot] Clamped: left for the tow truck; unclaimed at the
            // pound, it is auctioned and driven away.
            CarPlace::Parked(s) if s.booted => {}
            CarPlace::Parked(_) => {
                if self.drive_out_of_town(vid, Errand::Leave).is_none() {
                    // No road out: the car leaves the record with its owner.
                    self.drop_car(vid, CarEvent::Emigrated);
                }
            }
            CarPlace::Driving(cid) => {
                let ok = self.traffic.cars.contains_key(cid) && self.redirect_out(cid);
                if ok {
                    let c = &mut self.traffic.cars[cid];
                    c.errand = Errand::Leave;
                    c.citizen = CitizenId::default();
                } else {
                    self.traffic.remove_car(cid);
                    self.drop_car(vid, CarEvent::Emigrated);
                }
            }
        }
        if let Some(c) = self.citizens.get_mut(id) {
            c.car = None;
            c.owns_car = false;
        }
    }

    /// Buy a bike at a shop near home (goods or gadgets), or online from
    /// out of town. Returns the price.
    pub(super) fn buy_bike(&mut self, id: CitizenId, reason: &str) -> f32 {
        let tick = self.clock.tick;
        let at = self.citizens[id].home.and_then(|h| self.building_entrance(h)).unwrap_or(self.citizens[id].pos);
        let shop = self.bike_shop_near(at);
        let name = match shop {
            Some(s) => {
                if let Some(bz) = self.buildings[s].business.as_mut() {
                    bz.money += BIKE_PRICE * 0.4;
                    bz.revenue_today += BIKE_PRICE * 0.4;
                    bz.deals += 1;
                }
                self.buildings[s].name.clone()
            }
            None => "a shop out of town".to_string(),
        };
        let c = &mut self.citizens[id];
        c.owns_bike = true;
        c.remember(tick, format!("{reason} Bought a bike at {name}."));
        self.vehicles.stats.bikes_bought_today += 1;
        BIKE_PRICE
    }

    // ── keeping things straight ─────────────────────────────────────────

    /// A car taken off the road by the world (roads rebuilt, a bad save):
    /// it parks where it stood.
    pub(super) fn park_stranded(&mut self, vid: VehicleId, pos: Vec2) {
        let Some(car) = self.vehicles.cars.get(vid) else { return };
        // [boot] A clamped car is only shifted along the kerb, clamp and all.
        let booted = car.spot().is_some_and(|s| s.booted);
        if car.spot().is_some() {
            self.unpark(vid);
        }
        let home = self.citizens.get(self.vehicles.cars[vid].owner).and_then(|c| c.home);
        let near = if booted { self.boot_spot(pos) } else { self.place_near(pos, home) };
        let mut spot = near.unwrap_or(ParkSpot { kind: ParkKind::Overflow, pos, slot: u16::MAX, since: self.clock.tick, ..Default::default() });
        spot.booted = booted;
        self.park_at(vid, spot);
    }

    /// [boot] Where a car pulls over to be clamped: a free kerb slot close
    /// by (legal or not), else the street's edge right here, off the lanes.
    pub(super) fn boot_spot(&self, pos: Vec2) -> Option<ParkSpot> {
        let k = self.free_kerb_near(pos, 60.0).or_else(|| self.free_illegal_slots_near(pos, 60.0, 1).into_iter().next())?;
        Some(self.kerb_spot(k))
    }

    /// Roads changed: kerb slots on streets that are gone or reshaped are
    /// re-found near where the car stands.
    pub(super) fn refresh_kerbs(&mut self) {
        self.refresh_no_kerb();
        self.vehicles.reindex_kerbs();
        let moved: Vec<(VehicleId, Vec2)> = self
            .vehicles
            .cars
            .iter()
            .filter_map(|(id, c)| c.spot().filter(|s| s.kind.on_street()).map(|s| (id, *s)))
            .filter(|(_, s)| !self.street_spot_valid(s))
            .map(|(id, s)| (id, s.pos))
            .collect();
        for (vid, pos) in moved {
            self.park_stranded(vid, pos);
        }
        self.vehicles.reindex_kerbs();
    }

    /// Does a car on the street still stand where its slot is?
    fn street_spot_valid(&self, s: &ParkSpot) -> bool {
        // [parking] Slot-less spots (saves from before every street car had
        // a slot of its own) are re-found too.
        let Some(seg) = s.segment.filter(|seg| self.kerb_physical(*seg)) else { return false };
        s.slot != u16::MAX && s.slot < self.slot_count(seg) && self.kerb_sides(seg)[s.side.min(1) as usize] && self.slot_pose(seg, s.side, s.slot).0.distance(s.pos) < 1.0
    }

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
                CarPlace::Parked(_) => !self.citizens.get(c.owner).is_some_and(|o| o.car == Some(*id)),
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

    // ── how it felt ─────────────────────────────────────────────────────

    /// The trip under way began: note its mode and when.
    pub(super) fn begin_trip(&mut self, id: CitizenId, mode: TripMode, distance: f32) {
        crate::prof_scope!("begin_trip");
        let tick = self.clock.tick;
        let c = &mut self.citizens[id];
        c.commute.mode = Some(mode);
        c.commute.started = tick;
        c.commute.distance = distance;
        c.commute.searched = 0.0;
        c.commute.search_since = 0;
        c.commute.fee = 0.0;
        c.commute.toll = 0.0;
        c.commute.drive_since = 0;
        c.commute.drive_minutes = 0.0;
        c.commute.walk_after = 0.0;
        c.commute.rage = 0.0;
        c.commute.expected_drive = 0.0;
        self.vehicles.stats.trips_today[mode.index()] += 1;
    }

    /// Riders who waited for hours and gave up: transit let them down.
    pub(super) fn transit_failed(&mut self, id: CitizenId) {
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get_mut(id) else { return };
        let mode = c.commute.mode.filter(|m| matches!(m, TripMode::Bus | TripMode::Train | TripMode::Subway)).unwrap_or(TripMode::Bus);
        c.commute.feel(mode, 3.0);
        c.remember(tick, match mode {
            TripMode::Train => "Waited hours for a train that never came.",
            TripMode::Subway => "Stood on the metro platform for hours. Nothing came.",
            _ => "Waited hours for a bus that never came.",
        });
        self.concerns.heard(id, c, crate::concerns::Concern::Commute, 0.5);
    }

    /// The trip is over: how bad was it? Slow trips hurt by mode (a long
    /// walk, a slow ride, a jam), and so do searching for a space and the
    /// fee. Memories of the worst.
    pub(super) fn end_trip(&mut self, id: CitizenId) {
        let tick = self.clock.tick;
        let secs = self.config.traffic_seconds_per_tick;
        let (walk, bike) = (self.config.walk_speed.max(0.1), self.config.bike_speed.max(0.1));
        let Some(c) = self.citizens.get_mut(id) else { return };
        let Some(mode) = c.commute.mode.take() else { return };
        let minutes = tick.saturating_sub(c.commute.started) as f32;
        let d = c.commute.distance;
        let frail = matches!(c.stage(), crate::citizens::LifeStage::Child | crate::citizens::LifeStage::Retiree);
        // Minutes over what the trip "should" take, in bad half-hours.
        let pain = match mode {
            TripMode::Walk => (minutes - 25.0).max(0.0) / 30.0 * if frail { 1.4 } else { 1.0 },
            TripMode::Bike => (minutes - 35.0).max(0.0) / 30.0 + (d / bike - 60.0).max(0.0) / 60.0,
            TripMode::Bus | TripMode::Train | TripMode::Subway => (minutes - (d / (walk * 4.0) + 20.0)).max(0.0) / 30.0,
            // [taxis] The wait at the kerb and the ride against a drive.
            TripMode::Taxi => (minutes - (d * 1.3 / (9.0 * secs) + 12.0)).max(0.0) / 30.0,
            TripMode::Car => {
                // The drive against free flow (about 9 m/s door to door);
                // the hunt for a space, the fee and the walk from the car
                // are the parking's share.
                let free = d * 1.3 / (9.0 * secs) + 4.0;
                let driving = if c.commute.drive_minutes > 0.0 { c.commute.drive_minutes - c.commute.searched / secs } else { minutes };
                let jam = (driving - free * 2.0 - 5.0).max(0.0) / 30.0;
                let search = c.commute.searched / secs / 20.0;
                let fee = c.commute.fee / 6.0;
                let walk_back = (c.commute.walk_after - 80.0).max(0.0) / 200.0;
                let parking = search + fee + walk_back;
                c.commute.parking_pain += (parking - c.commute.parking_pain) * 0.4;
                // [tolls] Tolls hurt like a fee, and are expected next time.
                let toll = c.commute.toll;
                c.commute.toll_avg += (toll - c.commute.toll_avg) * 0.4;
                // [rage] Losing your temper makes a bad drive feel worse.
                jam + parking + toll / 6.0 + c.commute.rage * 0.8
            }
        };
        c.commute.feel(mode, pain);
        c.commute.last_minutes = minutes as u32;
        // [economy] A long walk: a bike would have helped (bike shops).
        if mode == TripMode::Walk && minutes >= 25.0 {
            self.pressure.long_walks_today += 1;
        }
        let Some(c) = self.citizens.get_mut(id) else { return };
        if mode == TripMode::Car && c.commute.rage >= 0.5 {
            // [rage] Always remembered: it is a story they will tell.
            let expected = c.commute.expected_drive.max(1.0);
            c.remember(tick, format!("Lost my temper in traffic: {minutes:.0} minutes for what should be a {expected:.0}-minute drive."));
            self.concerns.heard(id, c, crate::concerns::Concern::Commute, 0.3);
        } else if pain >= 1.5 && (tick + id_hash(id)) % 3 == 0 {
            let text = match mode {
                TripMode::Car if c.commute.searched / secs > 15.0 => format!("{:.0} minutes hunting for a parking space. Driving here is a chore.", c.commute.searched / secs),
                TripMode::Car => format!("{minutes:.0} minutes stuck in traffic. There has to be a better way."),
                TripMode::Walk => format!("{minutes:.0} minutes on foot. My feet are done."),
                TripMode::Bike => format!("{minutes:.0} minutes in the saddle. Too far to ride."),
                TripMode::Bus | TripMode::Train | TripMode::Subway => format!("{minutes:.0} minutes on transit for that. Crawling."),
                TripMode::Taxi => format!("{minutes:.0} minutes for a cab ride. Waited forever at the kerb."),
            };
            c.remember(tick, text);
            let k = if mode == TripMode::Car && c.commute.searched / secs > 15.0 { crate::concerns::Concern::Parking } else { crate::concerns::Concern::Commute };
            self.concerns.heard(id, c, k, (pain / 4.0).clamp(0.3, 0.8));
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
