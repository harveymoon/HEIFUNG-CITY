//! [carlife] Parking and unparking: the spots a car stands in, kept valid
//! when the streets change.

use super::*;

/// [r2-traffic] Which band of fullness a car park is in: the cars drawn on
/// it (baked into its mesh, `parking_visual`) are redrawn when it changes.
/// Every car counts while few stand there, then every 1/24 of the bays:
/// in eighths a 109-bay lot "2/109 taken" was still drawn empty (K44).
pub(in crate::world) fn parking_band(used: u16, capacity: u16) -> u32 {
    let (used, cap) = (used as u32, capacity.max(1) as u32);
    if used <= 12 {
        used
    } else {
        13 + used * 24 / cap
    }
}

impl World {
    /// Put a car in a space: the building's count, the kerb index, the
    /// owner's `parked_at`.
    pub(in crate::world) fn park_at(&mut self, vid: VehicleId, spot: ParkSpot) {
        let Some(car) = self.vehicles.cars.get_mut(vid) else { return };
        car.place = CarPlace::Parked(spot);
        let owner = car.owner;
        if spot.kind.uses_building() {
            if let Some(b) = spot.building.and_then(|b| self.buildings.get_mut(b)) {
                let before = parking_band(b.parking_used, b.parking_capacity);
                b.parking_used = b.parking_used.saturating_add(1);
                if b.is_parking() && before != parking_band(b.parking_used, b.parking_capacity) {
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
    pub(in crate::world) fn unpark(&mut self, vid: VehicleId) -> Option<ParkSpot> {
        let spot = *self.vehicles.cars.get(vid)?.spot()?;
        if spot.kind.uses_building() {
            if let Some(b) = spot.building.and_then(|b| self.buildings.get_mut(b)) {
                let before = parking_band(b.parking_used, b.parking_capacity);
                b.parking_used = b.parking_used.saturating_sub(1);
                if b.is_parking() && before != parking_band(b.parking_used, b.parking_capacity) {
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
    pub(in crate::world) fn building_spot(&self, kind: ParkKind, b: BuildingId) -> Option<ParkSpot> {
        let lot = self.building_lot(b)?;
        let pos = if lot.car_segment.is_some() { lot.car_point } else { lot.entrance() };
        Some(ParkSpot { kind, building: Some(b), segment: lot.car_segment, side: 0, slot: 0, pos, heading: 0.0, since: self.clock.tick, ..Default::default() })
    }

    /// A kerb slot as a parking spot: legal, or `Overflow` (parked where
    /// it is not allowed) when the street bans parking.
    pub(in crate::world) fn kerb_spot(&self, k: KerbSlot) -> ParkSpot {
        let kind = if self.kerb_allowed(k.segment) { ParkKind::Kerb } else { ParkKind::Overflow };
        ParkSpot { kind, building: None, segment: Some(k.segment), side: k.side, slot: k.slot, pos: k.pos, heading: k.heading, since: self.clock.tick, ..Default::default() }
    }

    /// [parking] Nowhere legal: the nearest free slot on a street where
    /// parking is banned, else a legal one further off. Always a slot of
    /// its own (it used to be the nearest kerb point, so cars that gave up
    /// near the same place stood on top of each other).
    pub(super) fn overflow_spot(&self, p: Vec2) -> Option<ParkSpot> {
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
    pub(in crate::world) fn place_near(&self, p: Vec2, home: Option<BuildingId>) -> Option<ParkSpot> {
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

    /// A car taken off the road by the world (roads rebuilt, a bad save):
    /// it parks where it stood.
    pub(in crate::world) fn park_stranded(&mut self, vid: VehicleId, pos: Vec2) {
        let Some(car) = self.vehicles.cars.get(vid) else { return };
        // [boot] A clamped car is only shifted along the kerb, clamp and all.
        let booted = car.spot().is_some_and(|s| s.booted);
        if car.spot().is_some() {
            self.unpark(vid);
        }
        let home = self.citizens.get(self.vehicles.cars[vid].owner).and_then(|c| c.home);
        let near = if booted { self.boot_spot(pos) } else { self.place_near(pos, home) };
        let mut spot = near.unwrap_or_else(|| self.squeezed_spot(pos));
        spot.booted = booted;
        self.park_at(vid, spot);
    }

    /// [boot] Where a car pulls over to be clamped: a free kerb slot close
    /// by (legal or not), else the street's edge right here, off the lanes.
    pub(in crate::world) fn boot_spot(&self, pos: Vec2) -> Option<ParkSpot> {
        let k = self.free_kerb_near(pos, 60.0).or_else(|| self.free_illegal_slots_near(pos, 60.0, 1).into_iter().next())?;
        Some(self.kerb_spot(k))
    }

    /// Roads changed: kerb slots on streets that are gone or reshaped are
    /// re-found near where the car stands.
    pub(in crate::world) fn refresh_kerbs(&mut self) {
        self.refresh_no_kerb();
        self.remap_kerb_slots();
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

    /// [r2-traffic] Roads rebuilt renumber their pieces: a car whose kerb
    /// slot is still there, under another id or number, keeps it (found by
    /// where it stands); then the kerb index is rebuilt. Done before any
    /// car is re-parked: with the stale ids the index marked slots taken on
    /// whichever streets now had those numbers, and cars were moved, slot
    /// free by slot, onto the empty kerbs of a road just built (K41).
    pub(in crate::world) fn remap_kerb_slots(&mut self) {
        let stale: Vec<(VehicleId, ParkSpot)> = self.vehicles.cars.iter().filter_map(|(id, c)| c.spot().filter(|s| s.kind.on_street() && s.slot != u16::MAX).map(|s| (id, *s))).filter(|(_, s)| !self.street_spot_valid(s)).collect();
        for (vid, s) in stale {
            if let Some((seg, side, slot)) = self.slot_at(s.pos) {
                if let Some(CarPlace::Parked(sp)) = self.vehicles.cars.get_mut(vid).map(|c| &mut c.place) {
                    (sp.segment, sp.side, sp.slot) = (Some(seg), side, slot);
                }
            }
        }
        self.vehicles.reindex_kerbs();
    }

    /// [r2-traffic] The kerb slot whose place is `p` (within a metre).
    fn slot_at(&self, p: Vec2) -> Option<(SegmentId, u8, u16)> {
        let segs: Vec<SegmentId> = self.roads.segments_in_box(p - Vec2::splat(12.0), p + Vec2::splat(12.0)).collect();
        for sid in segs {
            let count = self.slot_count(sid);
            if count == 0 {
                continue;
            }
            let (a, b) = self.roads.segment_points(sid);
            let len = a.distance(b).max(0.01);
            let d = (b - a) / len;
            let along = (p - a).dot(d);
            for side in 0..2u8 {
                if !self.kerb_sides(sid)[side as usize] {
                    continue;
                }
                let at = if side == 0 { along } else { len - along };
                let k = ((at - KERB_MARGIN) / KERB_PITCH - 0.5).round();
                if k < 0.0 || k >= count as f32 {
                    continue;
                }
                let slot = k as u16;
                if self.slot_pose(sid, side, slot).0.distance(p) < 1.0 {
                    return Some((sid, side, slot));
                }
            }
        }
        None
    }

    /// Does a car on the street still stand where its slot is?
    pub(super) fn street_spot_valid(&self, s: &ParkSpot) -> bool {
        // [parking] Slot-less spots (saves from before every street car had
        // a slot of its own) are re-found too.
        let Some(seg) = s.segment.filter(|seg| self.kerb_physical(*seg)) else { return false };
        s.slot != u16::MAX && s.slot < self.slot_count(seg) && self.kerb_sides(seg)[s.side.min(1) as usize] && self.slot_pose(seg, s.side, s.slot).0.distance(s.pos) < 1.0
    }
}
