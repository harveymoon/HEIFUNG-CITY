//! [carlife] Kerbs: where cars may stand on a street, and the free slots.

use super::*;

/// [r11-parking] Squeeze places (a car left with no slot of its own, at a
/// corner of the block or along a street too short for slots) are kerb
/// "slots" numbered from here in the kerb index, so two such cars never
/// stand on one place (two of them stacked at one corner were drawn as one
/// car z-fighting with the other: Harvey's "scooter that looks like a car").
pub(in crate::world) const SQUEEZE_SLOT: u16 = 0xF000;
/// [r11-parking] A squeeze place's centre this far from the end of its
/// street: just short of the first kerb slot, in the corner zone.
const SQUEEZE_AT: f32 = KERB_MARGIN - KERB_PITCH * 0.5;
/// [r11-parking] Half a parked car's length and width (a long car), and how
/// far from a node a kerb place is checked against the other ways there.
const PARKED_HALF_LEN: f32 = 2.4;
const PARKED_HALF_WIDTH: f32 = 1.0;
const KERB_CLEAR_REACH: f32 = 80.0;
/// [r11-parking] Squeeze places looked for this far round a car that has to
/// be put somewhere (taken off the road, clamped, an old save's car).
pub(in crate::world) const SQUEEZE_REACH: [f32; 3] = [60.0, 250.0, 1000.0];

impl World {
    /// Can cars park along this street? Not on highways, ramps, bridges or
    /// avenues (two lanes or more each way are clearways), nor on shopping
    /// and nightlife streets, whose kerbs are for loading: drivers going
    /// there use the lots.
    pub(in crate::world) fn kerb_allowed(&self, sid: SegmentId) -> bool {
        self.roads.try_segment(sid).is_some_and(|s| s.lanes_fwd < 2 && s.lanes_bwd < 2)
            && self.kerb_physical(sid)
            && !self.vehicles.no_kerb.1.contains(&sid)
    }

    /// [r11-parking] Legal kerb parking along this street (the road
    /// inspector's "kerb parking"; it used to read the street's kind alone,
    /// so a roundabout's ring said yes).
    pub fn kerb_parking(&self, sid: SegmentId) -> bool {
        self.kerb_allowed(sid)
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
    /// [r11-parking] Never a ring piece or a slip road, whatever else the
    /// player set the road to (`ring` and `ramp` are read off the network's
    /// shape as well as the circle test).
    pub(in crate::world) fn kerb_physical(&self, sid: SegmentId) -> bool {
        self.roads.try_segment(sid).is_some_and(|s| s.alive && s.is_street() && !s.fronts_nothing() && !s.bridge && !s.tunnel && !s.ring && !s.ramp && s.lanes() > 0) && !self.on_ring(sid)
    }

    /// [r11-parking] A car standing at `pos` (facing `heading`) on the kerb
    /// line of `sid` is clear of every other way meeting it at either end:
    /// its body is outside their carriageways. Near a junction, and all
    /// along a sharp fork, the kerb line of one street runs into the lanes
    /// of the next (a car in slot 0 of an avenue stood 2.5 m from the
    /// centre of the avenue forking off it, in the traffic). A way going on
    /// nearly straight from the same node is the same street.
    pub(in crate::world) fn kerb_clear(&self, sid: SegmentId, pos: Vec2, heading: f32) -> bool {
        let Some(seg) = self.roads.try_segment(sid) else { return false };
        let dir = Vec2::new(heading.cos(), heading.sin());
        let body = [pos, pos + dir * PARKED_HALF_LEN, pos - dir * PARKED_HALF_LEN];
        for (n, other_end) in [(seg.a, seg.b), (seg.b, seg.a)] {
            let at = self.roads.node(n).pos;
            if at.distance(pos) > KERB_CLEAR_REACH {
                continue;
            }
            let out = (self.roads.node(other_end).pos - at).normalize_or_zero();
            for &o in &self.roads.node(n).segments {
                let Some(s) = self.roads.try_segment(o).filter(|_| o != sid) else { continue };
                if s.kind == RoadKind::Path {
                    continue;
                }
                let (a, b) = self.roads.segment_points(o);
                let away = if s.a == n { b - a } else { a - b }.normalize_or_zero();
                if away.dot(out) < -0.94 {
                    continue;
                }
                let room = s.half_width() + PARKED_HALF_WIDTH;
                if body.iter().any(|&p| p.distance(crate::geom::project_on_segment(p, a, b).1) < room) {
                    return false;
                }
            }
        }
        true
    }

    /// [r11-parking] Squeeze places along one side of a street: the two
    /// corners of a street with kerb slots, or every pitch of one too short
    /// for slots.
    pub(in crate::world) fn squeeze_count(&self, sid: SegmentId) -> u16 {
        if !self.kerb_physical(sid) {
            return 0;
        }
        let (a, b) = self.roads.segment_points(sid);
        let len = a.distance(b);
        if self.slot_count(sid) > 0 {
            2
        } else if len >= 2.0 * SQUEEZE_AT {
            ((len - 2.0 * SQUEEZE_AT) / KERB_PITCH).floor() as u16 + 1
        } else {
            0
        }
    }

    /// [r11-parking] Where squeeze place `k` of a side stands.
    pub(in crate::world) fn squeeze_pose(&self, sid: SegmentId, side: u8, k: u16) -> (Vec2, f32) {
        let (a, b) = self.roads.segment_points(sid);
        let len = a.distance(b);
        let along = if self.slot_count(sid) > 0 && k == 1 { len - SQUEEZE_AT } else { SQUEEZE_AT + k as f32 * KERB_PITCH };
        self.kerb_pose(sid, side, along)
    }

    /// [r11-parking] The free squeeze place nearest `q` within `r`, clear
    /// of the other ways at its corner.
    pub(in crate::world) fn free_squeeze_near(&self, q: Vec2, r: f32) -> Option<KerbSlot> {
        let mut best: Option<(f32, KerbSlot)> = None;
        let mut segs: Vec<SegmentId> = self.roads.segments_in_box(q - Vec2::splat(r), q + Vec2::splat(r)).collect();
        segs.sort_by_key(|s| s.0);
        for sid in segs {
            let count = self.squeeze_count(sid);
            let taken = self.vehicles.kerbs.get(&sid);
            let sides = self.kerb_sides(sid);
            for side in 0..2u8 {
                if !sides[side as usize] {
                    continue;
                }
                for k in 0..count {
                    let slot = SQUEEZE_SLOT + k;
                    if taken.is_some_and(|t| t.contains_key(&(side, slot))) || self.food_trucks.holds(sid, side, slot) {
                        continue;
                    }
                    let (pos, heading) = self.squeeze_pose(sid, side, k);
                    let d = pos.distance(q);
                    if d <= r && best.as_ref().is_none_or(|(bd, _)| d < *bd) && self.kerb_clear(sid, pos, heading) {
                        best = Some((d, KerbSlot { segment: sid, side, slot, pos, heading }));
                    }
                }
            }
        }
        best.map(|(_, k)| k)
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
    /// [r11-parking] A free squeeze place (a corner clear of the junction,
    /// one car to a place) as near as there is one. Only with none within a
    /// kilometre, the kerb line of the nearest street a car may stand along
    /// (any distance off), away from its ends. It used to be the nearest
    /// street within 40 m at the point nearest `p`: by a junction that is
    /// the junction itself, so a car given up by a roundabout stood in the
    /// ring 0.4 m off its centre line, cars given up at one corner stood on
    /// each other, and cars by an interchange (no street within 40 m) were
    /// left in their lane.
    pub(in crate::world) fn squeezed_spot(&self, p: Vec2) -> ParkSpot {
        let tick = self.clock.tick;
        if let Some(k) = SQUEEZE_REACH.iter().find_map(|&r| self.free_squeeze_near(p, r)) {
            return ParkSpot { kind: ParkKind::Overflow, segment: Some(k.segment), side: k.side, slot: k.slot, pos: k.pos, heading: k.heading, since: tick, ..Default::default() };
        }
        let near = self.roads.nearest_where(p, |s| s.is_street() && !s.fronts_nothing() && !s.ramp && !s.bridge && !s.tunnel && !s.ring && s.lanes() > 0);
        let Some((sid, _, _)) = near else { return ParkSpot { kind: ParkKind::Overflow, pos: p, slot: u16::MAX, since: tick, ..Default::default() } };
        let (a, b) = self.roads.segment_points(sid);
        let len = a.distance(b).max(0.01);
        let d = (b - a) / len;
        let along = if len > 2.0 * SQUEEZE_AT { (p - a).dot(d).clamp(SQUEEZE_AT, len - SQUEEZE_AT) } else { len * 0.5 };
        let sides = self.kerb_sides(sid);
        let right = Vec2::new(-d.y, d.x);
        let side = if ((p - a).dot(right) >= 0.0 && sides[0]) || !sides[1] { 0 } else { 1 };
        let (pos, heading) = self.kerb_pose(sid, side, if side == 0 { along } else { len - along });
        ParkSpot { kind: ParkKind::Overflow, segment: Some(sid), side, slot: u16::MAX, pos, heading, since: tick, ..Default::default() }
    }

    pub(in crate::world) fn slot_pose(&self, sid: SegmentId, side: u8, slot: u16) -> (Vec2, f32) {
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
                    // [r14-foodtrucks] Nor a slot a food truck stands in
                    // (or is heading for).
                    if taken.is_some_and(|t| t.contains_key(&(side, slot))) || self.food_trucks.holds(sid, side, slot) {
                        continue;
                    }
                    let (pos, heading) = self.slot_pose(sid, side, slot);
                    let d = pos.distance(q);
                    // [r11-parking] Never a slot that runs into the lanes
                    // of a way meeting this street.
                    if d <= r && best.as_ref().is_none_or(|(bd, _)| d < *bd) && self.kerb_clear(sid, pos, heading) {
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
