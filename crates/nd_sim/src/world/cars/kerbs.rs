//! [carlife] Kerbs: where cars may stand on a street, and the free slots.

use super::*;

impl World {
    /// Can cars park along this street? Not on highways, ramps, bridges or
    /// avenues (two lanes or more each way are clearways), nor on shopping
    /// and nightlife streets, whose kerbs are for loading: drivers going
    /// there use the lots.
    pub(in crate::world) fn kerb_allowed(&self, sid: SegmentId) -> bool {
        self.roads.try_segment(sid).is_some_and(|s| s.alive && s.is_street() && !s.fronts_nothing() && !s.bridge && s.lanes() > 0 && s.lanes_fwd < 2 && s.lanes_bwd < 2)
            && !self.vehicles.no_kerb.1.contains(&sid)
            && !self.on_ring(sid)
    }

    /// Is this segment part of a roundabout's ring? Rings are one-way and
    /// often one lane, which the street rule above would otherwise let
    /// cars park along: parked cars then sat in the ring itself.
    pub(in crate::world) fn on_ring(&self, sid: SegmentId) -> bool {
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
    pub(in crate::world) fn refresh_no_kerb(&mut self) {
        let key = (self.plan_version, self.roads.version);
        if self.vehicles.no_kerb.0 == Some(key) {
            return;
        }
        let set = self.lots.values().filter(|l| matches!(l.land_use, LandUse::Commercial | LandUse::Entertainment)).map(|l| l.front_segment).collect();
        self.vehicles.no_kerb = (Some(key), set);
    }

    /// Kerb slots along one side of a street.
    pub(in crate::world) fn kerb_slots(&self, sid: SegmentId) -> u16 {
        if !self.kerb_allowed(sid) {
            return 0;
        }
        self.slot_count(sid)
    }

    /// [parking] Can a car physically stand along this street, legal or
    /// not: every street but highways, ramps, tunnels, bridges and
    /// roundabout rings (where a parked car would sit in the traffic, or
    /// be drawn on the ground over the bore).
    pub(in crate::world) fn kerb_physical(&self, sid: SegmentId) -> bool {
        self.roads.try_segment(sid).is_some_and(|s| s.alive && s.is_street() && !s.fronts_nothing() && !s.bridge && s.lanes() > 0) && !self.on_ring(sid)
    }

    /// Slots along one side of a street, legal or not.
    pub(in crate::world) fn slot_count(&self, sid: SegmentId) -> u16 {
        if !self.kerb_physical(sid) {
            return 0;
        }
        let (a, b) = self.roads.segment_points(sid);
        ((a.distance(b) - 2.0 * KERB_MARGIN) / KERB_PITCH).floor().max(0.0) as u16
    }

    /// The kerb sides of a street cars can pull into: 0 to the right of
    /// a→b (cars driving a→b), 1 to the right of b→a.
    pub(super) fn kerb_sides(&self, sid: SegmentId) -> [bool; 2] {
        let s = self.roads.segment(sid);
        [s.lanes_fwd > 0, s.lanes_bwd > 0]
    }

    /// Where a car parked `along` metres from the start of its side stands.
    pub(super) fn kerb_pose(&self, sid: SegmentId, side: u8, along: f32) -> (Vec2, f32) {
        let (a, b) = self.roads.segment_points(sid);
        let len = a.distance(b).max(0.01);
        let d = (b - a) / len;
        let (p, dir) = if side == 0 { (a + d * along, d) } else { (b - d * along, -d) };
        let right = Vec2::new(-dir.y, dir.x);
        let off = self.roads.segment(sid).half_width() - 1.15;
        (p + right * off, dir.y.atan2(dir.x))
    }

    /// [r2-traffic] Where a car left with no slot of its own stands: on
    /// the kerb line of the nearest street it can stand along, on the side
    /// nearer `p`, facing that side's traffic. These spots used to keep the
    /// car's place in its lane, so it was drawn in the middle of the road
    /// with the traffic driving through it (K32, K35).
    pub(in crate::world) fn squeezed_spot(&self, p: Vec2) -> ParkSpot {
        let tick = self.clock.tick;
        let near = self.roads.nearest_where(p, |s| s.is_street() && !s.is_highway() && !s.ramp && !s.bridge && !s.tunnel && !s.ring && s.lanes() > 0).filter(|(_, _, q)| q.distance(p) < 40.0);
        let Some((sid, _, _)) = near else { return ParkSpot { kind: ParkKind::Overflow, pos: p, slot: u16::MAX, since: tick, ..Default::default() } };
        let (a, b) = self.roads.segment_points(sid);
        let len = a.distance(b).max(0.01);
        let d = (b - a) / len;
        let along = (p - a).dot(d).clamp(0.0, len);
        let sides = self.kerb_sides(sid);
        let right = Vec2::new(-d.y, d.x);
        let side = if ((p - a).dot(right) >= 0.0 && sides[0]) || !sides[1] { 0 } else { 1 };
        let (pos, heading) = self.kerb_pose(sid, side, if side == 0 { along } else { len - along });
        ParkSpot { kind: ParkKind::Overflow, segment: Some(sid), side, slot: u16::MAX, pos, heading, since: tick, ..Default::default() }
    }

    pub(super) fn slot_pose(&self, sid: SegmentId, side: u8, slot: u16) -> (Vec2, f32) {
        self.kerb_pose(sid, side, KERB_MARGIN + (slot as f32 + 0.5) * KERB_PITCH)
    }

    /// The free kerb slot nearest `q`, within `r`.
    pub(in crate::world) fn free_kerb_near(&self, q: Vec2, r: f32) -> Option<KerbSlot> {
        self.free_kerbs_near(q, r, 1).into_iter().next()
    }

    /// Up to `n` free kerb slots within `r` of `q`, nearest first, at most
    /// one per side of a street (the nearest there).
    pub(in crate::world) fn free_kerbs_near(&self, q: Vec2, r: f32, n: usize) -> Vec<KerbSlot> {
        self.free_slots_near(q, r, n, true)
    }

    /// [parking] Free slots on streets where parking is banned (shop
    /// fronts, clearways): a driver who found nothing legal stops there, in
    /// a slot of its own, and risks the ticket and the tow truck.
    pub(in crate::world) fn free_illegal_slots_near(&self, q: Vec2, r: f32, n: usize) -> Vec<KerbSlot> {
        self.free_slots_near(q, r, n, false)
    }

    /// Free slots, legal (`legal`) or on streets where parking is banned,
    /// within `r` of `q`, nearest first, one per side of a street. Both
    /// kinds share the kerb index, so no two cars ever stand in one slot.
    pub(super) fn free_slots_near(&self, q: Vec2, r: f32, n: usize, legal: bool) -> Vec<KerbSlot> {
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
}
