//! Heights: water crossings, lot grading, the ground graded round the roads,
//! and how high things stand on it.

use super::*;

/// [stable-parcels] How far a lot's street may move up or down before its
/// pad follows, m.
const PAD_HOLD: f32 = 0.3;
/// [stable-parcels] The same for the ground under the player's buildings.
const CITY_PAD_HOLD: f32 = 1.0;

impl World {
    /// Insert a vertex wherever a polyline crosses a waterline, set back a
    /// few metres onto the bank, so the stretch over the water becomes its
    /// own segments: the bridge.
    pub(in crate::world) fn split_at_water(&self, points: &[Vec2]) -> Vec<Vec2> {
        if self.landscape.is_flat() || points.len() < 2 {
            return points.to_vec();
        }
        let mut out = vec![points[0]];
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.distance(b);
            if len < 1.0 {
                out.push(b);
                continue;
            }
            let dir = (b - a) / len;
            let steps = (len / 3.0).ceil() as usize;
            let mut wet = self.landscape.is_water(a);
            for k in 1..=steps {
                let t = (k as f32 / steps as f32) * len;
                let p = a + dir * t;
                let now = self.landscape.is_water(p);
                if now != wet {
                    // Entering: the abutment before the water; leaving: after it.
                    let s = if now { (t - 3.0 - ABUTMENT).max(0.0) } else { (t + ABUTMENT).min(len) };
                    let q = a + dir * s;
                    if out.last().is_none_or(|l| l.distance(q) > 2.0) && q.distance(b) > 2.0 {
                        out.push(q);
                    }
                    wet = now;
                }
            }
            out.push(b);
        }
        out
    }

    /// Flag every segment whose middle is over water, and every segment
    /// the player drew well above the ground (a viaduct: a deck too).
    pub(in crate::world) fn mark_bridges(&mut self) {
        let flat = self.landscape.is_flat();
        let ids: Vec<(SegmentId, Vec2, Vec2)> = self.roads.alive_segments().map(|(id, _)| (id, self.roads.segment_points(id).0, self.roads.segment_points(id).1)).collect();
        for (id, a, b) in ids {
            let wet = !flat && [0.25, 0.5, 0.75].iter().filter(|&&t| self.landscape.is_water(a.lerp(b, t))).count() >= 2;
            let seg = self.roads.segment(id);
            let (la, lb) = (self.roads.node(seg.a).level, self.roads.node(seg.b).level);
            let viaduct = la.max(lb) >= crate::roads::VIADUCT_LEVEL;
            // Under the water a tunnel is still a tunnel.
            let tunnel = la.max(lb) <= crate::roads::TUNNEL_LEVEL;
            let s = &mut self.roads.segments[id.0 as usize];
            s.bridge = (wet && !tunnel) || viaduct;
            s.viaduct = viaduct && !wet;
            s.tunnel = tunnel;
        }
    }

    /// Rivers, lakes and the sea, ready for cutting parcels.
    pub(in crate::world) fn water_mask(&self) -> WaterMask {
        if self.landscape.is_flat() {
            return WaterMask::default();
        }
        WaterMask::new(&self.landscape.water_polygons())
    }

    /// Mark lots on ground too steep to level (a cliff, or more rise than
    /// one terrace takes) or in the water unusable. Ordinary hillsides are
    /// graded: the lot's pad is levelled to its street (`Ground`).
    pub(in crate::world) fn grade_lots(&mut self) {
        if self.landscape.is_flat() {
            return;
        }
        let land = &self.landscape;
        for l in self.lots.values_mut() {
            if l.landlocked {
                continue;
            }
            if land.lot_ground_problem(&l.polygon, l.center).is_some() {
                l.landlocked = true;
            }
        }
    }

    /// [island-lots] Half-side of the square parcels are cut in: the
    /// configured buildable square (`half_extent`), grown on a landscape to
    /// take in every street and path drawn past it (an island off the
    /// coast) with its parcels, in steps of 100 m, up to the edge of the map.
    /// Derived from the roads alone, so it is the same after a load.
    pub fn parcel_half(&self) -> f32 {
        let base = self.config.half_extent;
        if self.landscape.is_flat() {
            return base;
        }
        let edge = crate::landscape::LAND_HALF;
        let mut r = base;
        for (id, s) in self.roads.alive_segments() {
            if s.is_rail() || s.fronts_nothing() {
                continue;
            }
            let (a, b) = self.roads.segment_points(id);
            for p in [a, b] {
                let c = p.x.abs().max(p.y.abs());
                if c < edge {
                    r = r.max(c + crate::zoning::PARCEL_REACH + s.half_width() + 10.0);
                }
            }
        }
        ((r / 100.0).ceil() * 100.0).min(edge - 100.0).max(base)
    }

    /// [island-lots] Half-side of the built-ground raster: the parcels'
    /// square and 200 m round it (as it always was round `half_extent`),
    /// so every lot's pad is levelled into the ground.
    pub fn ground_half(&self) -> f32 {
        (self.parcel_half() + 200.0).min(crate::landscape::LAND_HALF)
    }

    /// [sweep-sim] Half extent of the land-value and estate-pressure grids
    /// with `cell`-sized cells: the old `half_extent + 200` on a flat map,
    /// out to the whole buildable land otherwise (lots on far islands, see
    /// [`parcel_half`](Self::parcel_half)). Grown by whole cells so the
    /// cells of the old grid stay where they were. Fixed per map, so the
    /// grids are not resized as roads reach further out.
    pub fn grid_half(&self, cell: f32) -> f32 {
        let base = self.config.half_extent + 200.0;
        let reach = crate::landscape::LAND_HALF;
        if self.landscape.is_flat() || reach <= base || cell <= 0.0 {
            return base;
        }
        base + ((reach - base) / cell).ceil() * cell
    }

    /// [sweep-sim] Grow an old save's land-value and pressure grids to
    /// [`grid_half`](Self::grid_half).
    pub(in crate::world) fn grow_grids(&mut self) {
        let lv = self.grid_half(self.land_value.cell);
        if lv > self.land_value.half + 1.0 && !self.land_value.grow_to(lv) {
            self.land_value.set(crate::land_value::LandValue::new(lv, self.land_value.cell));
        }
        let pr = self.grid_half(crate::estate::PRESSURE_CELL);
        if pr > self.estate.pressure.half + 1.0 && !self.estate.pressure.grow_to(pr) {
            self.estate.pressure = crate::estate::Pressure::new(pr);
        }
    }

    /// Heights of the road nodes, then the built ground from them.
    ///
    /// Every node starts on the ground plus the level it was drawn at. Nodes
    /// drawn off the ground (a viaduct, a cutting) are held there; the rest
    /// are eased so no way is steeper than its grade: lowered towards a
    /// cutting next to them, raised otherwise (fill and embankment). Where
    /// two ways cross grade-separated and the upper is not clear of the
    /// lower by `UNDERPASS_CLEARANCE` (the highway at grade with a street,
    /// say), the upper way's vertex over the crossing is lifted until it is,
    /// and its approaches ramp up to it.
    pub(in crate::world) fn rebuild_ground(&mut self) {
        let land = &self.landscape;
        let flat = land.is_flat();
        let floor = if flat { 0.0 } else { crate::landscape::BANK_HEIGHT };
        let roads = &self.roads;
        // Out past the map a way to elsewhere keeps the level it had at
        // the edge: the far hills are cut and filled for it.
        let edge = crate::landscape::LAND_HALF;
        let mut z: Vec<f32> = roads.nodes.iter().map(|nd| land.height(nd.pos.clamp(Vec2::splat(-edge), Vec2::splat(edge))).max(floor) + nd.level).collect();
        let mut fixed: Vec<bool> = roads.nodes.iter().map(|nd| nd.level != 0.0).collect();
        let mut least = vec![f32::NEG_INFINITY; z.len()];
        if !flat {
            // Bridge ends stand at deck height; the approaches ramp up to them.
            for (_, seg) in roads.alive_segments() {
                if seg.bridge && !seg.viaduct {
                    let deck = WATER_LEVEL + BRIDGE_CLEARANCE;
                    for k in [seg.a.0 as usize, seg.b.0 as usize] {
                        z[k] = z[k].max(deck);
                        least[k] = deck;
                    }
                }
            }
        }
        let segs: Vec<(usize, usize, f32, f32)> = roads
            .alive_segments()
            .map(|(id, s)| {
                let (a, b) = roads.segment_points(id);
                (s.a.0 as usize, s.b.0 as usize, a.distance(b), way_grade(s.kind))
            })
            .collect();
        let crossings = roads.grade_crossings();
        let seg_z = |z: &[f32], (sid, t): (SegmentId, f32)| {
            let s = roads.segment(sid);
            z[s.a.0 as usize] + (z[s.b.0 as usize] - z[s.a.0 as usize]) * t
        };
        for _ in 0..6 {
            ease_heights(&mut z, &fixed, &least, &segs);
            let mut lifted = false;
            for c in &crossings {
                let (zu, zl) = (seg_z(&z, c.upper), seg_z(&z, c.lower));
                // Clear, either way up: the other way may pass over where the
                // land carries it (a street on a hill over the highway in its
                // valley), but not on a long fill that merely spans the gap.
                let on_land = zl <= land.height(c.at).max(floor) + 1.0;
                if zu - zl >= UNDERPASS_CLEARANCE - 0.01 || (on_land && zl - zu >= UNDERPASS_CLEARANCE - 0.01) {
                    continue;
                }
                let s = roads.segment(c.upper.0);
                let vertex = if c.upper.1 < 0.02 { Some(s.a.0 as usize) } else if c.upper.1 > 0.98 { Some(s.b.0 as usize) } else { None };
                // Only a way at grade is lifted: one the player set at a
                // level keeps it (the tool warns about it).
                if let Some(k) = vertex.filter(|&k| !fixed[k]) {
                    z[k] = zl + UNDERPASS_CLEARANCE + 0.01;
                    fixed[k] = true;
                    lifted = true;
                }
            }
            if !lifted {
                break;
            }
        }
        // A ramp that has to climb well above the land (to the highway's
        // embankment, onto a deck) stands on piers rather than on earth.
        let ramps: Vec<SegmentId> = self
            .roads
            .alive_segments()
            .filter(|(_, s)| !s.bridge && !s.tunnel && s.ramp)
            .filter(|(_, s)| [s.a, s.b].iter().any(|&n| z[n.0 as usize] - (land.height(self.roads.node(n).pos).max(floor)) >= crate::roads::VIADUCT_LEVEL))
            .map(|(id, _)| id)
            .collect();
        for id in ramps {
            let s = &mut self.roads.segments[id.0 as usize];
            s.bridge = true;
            s.viaduct = true;
        }
        self.relief = !flat || z.iter().any(|h| h.abs() > 1e-3) || self.roads.alive_segments().any(|(_, s)| s.bridge);
        self.road_z = z;
        self.road_inner = inner_heights(&self.landscape, &self.roads, &self.road_z, floor, &crossings);
        // [stable-parcels] The player's buildings' ground is levelled last.
        let mut locked: Vec<LotId> = self.lots.iter().filter(|(_, l)| self.is_city_ground(l)).map(|(id, _)| id).collect();
        locked.sort();
        // [F16] A dock's yard is levelled at the waterside (its own ground's
        // height, at least the bank): levelled to its street, a marina at
        // the foot of a hill stood metres up in the air over its boats.
        let mut pad_at: HashMap<LotId, f32> = HashMap::new();
        if !self.landscape.is_flat() {
            for l in self.ports.ports.values().filter_map(|p| p.lot) {
                let Some(lot) = self.lots.get(l) else { continue };
                let mean = lot.polygon.iter().map(|c| self.landscape.height(*c)).sum::<f32>() / lot.polygon.len().max(1) as f32;
                pad_at.insert(l, mean.max(crate::landscape::BANK_HEIGHT));
            }
        }
        // [stable-parcels] A lot kept through the works keeps its pad while
        // its street's height moves by no more than `PAD_HOLD` (the heights
        // are eased over the whole network, so a road laid far off shifts
        // them by millimetres): nothing far from the works rises or sinks.
        for (id, lot) in self.lots.iter() {
            if pad_at.contains_key(&id) {
                continue;
            }
            let Some(&old) = self.ground.pads.get(&id) else { continue };
            let now = crate::landscape::lot_pad(&self.roads, &self.road_z, &self.road_inner, lot, &self.landscape);
            // The player's own buildings hold theirs further: a road
            // regraded nearby does not lift or sink a plant.
            let hold = if self.is_city_ground(lot) { CITY_PAD_HOLD } else { PAD_HOLD };
            if (now - old).abs() <= hold {
                pad_at.insert(id, old);
            }
        }
        self.ground.set(crate::landscape::Ground::build_with(&self.landscape, &self.roads, &self.road_z, &self.road_inner, &self.lots, self.ground_half(), &locked, &pad_at));
    }

    /// Legs (by their first point) of a road drawn along `points` at
    /// `levels` that rise or fall faster than its grade, counting only legs
    /// with an end off the ground (the land's own slopes are eased as ever).
    pub fn steep_legs(&self, points: &[Vec2], levels: &[f32], kind: RoadKind) -> Vec<usize> {
        let land = &self.landscape;
        let floor = if land.is_flat() { 0.0 } else { crate::landscape::BANK_HEIGHT };
        let lv = |i: usize| levels.get(i).copied().unwrap_or(0.0);
        let z = |i: usize| land.height(points[i]).max(floor) + lv(i);
        let grade = way_grade(kind);
        (0..points.len().saturating_sub(1))
            .filter(|&i| {
                let len = points[i].distance(points[i + 1]);
                len >= 0.5 && (lv(i) != 0.0 || lv(i + 1) != 0.0) && (z(i + 1) - z(i)).abs() / len > grade + 1e-3
            })
            .collect()
    }

    /// Where the drawn road (`points` with a level each, of `kind`) would be
    /// steeper than its grade allows, or cross another way at an awkward
    /// height: one warning per problem, for the Road tool and the API.
    /// Steep legs are still built: the end on the ground is raised (or
    /// lowered) until the grade holds, which takes ground beside it.
    pub fn road_grade_warnings(&self, points: &[Vec2], levels: &[f32], kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> Vec<String> {
        let mut out = Vec::new();
        if points.len() < 2 {
            return out;
        }
        let land = &self.landscape;
        let floor = if land.is_flat() { 0.0 } else { crate::landscape::BANK_HEIGHT };
        let lv = |i: usize| levels.get(i).copied().unwrap_or(0.0);
        let z = |i: usize| land.height(points[i]).max(floor) + lv(i);
        let grade = way_grade(kind);
        for i in self.steep_legs(points, levels, kind) {
            let len = points[i].distance(points[i + 1]);
            let g = (z(i + 1) - z(i)).abs() / len;
            let need = (z(i + 1) - z(i)).abs() / grade;
            out.push(format!("too steep between points {} and {}: {:.0}% over {:.0} m (at most {:.0}%; needs {:.0} m)", i + 1, i + 2, g * 100.0, len, grade * 100.0, need));
        }
        // Crossings at a height that is neither a junction nor a flyover.
        let highway = crate::roads::is_highway_spec(kind, lanes_fwd, lanes_bwd);
        for i in 0..points.len() - 1 {
            let (a, b) = (points[i], points[i + 1]);
            for sid in self.roads.segments_in_box(a.min(b), a.max(b)) {
                let (p0, p1) = self.roads.segment_points(sid);
                let Some((t, u)) = crate::roads::segment_intersection(a, b, p0, p1) else { continue };
                let s = self.roads.segment(sid);
                let mine = lv(i) + (lv(i + 1) - lv(i)) * t;
                let theirs = self.roads.segment_level(sid, u);
                let gap = (mine - theirs).abs();
                // Off-grade railway crossings are refused: said once below.
                if rail_crossing_refusal(kind, s.kind, highway, mine, s.is_highway(), theirs).is_some() {
                    continue;
                } else if !crate::roads::grade_separated(highway, mine, s.is_highway(), theirs) && gap >= 1.0 {
                    out.push(format!("meets a road {gap:.0} m below or above it at grade: raise or lower it {:.0} m more to pass over or under", crate::roads::GRADE_SEPARATION - gap));
                } else if crate::roads::grade_separated(highway, mine, s.is_highway(), theirs) && gap > 0.0 && gap < UNDERPASS_CLEARANCE && (mine != 0.0 && theirs != 0.0) {
                    out.push(format!("only {gap:.1} m clear of the road it crosses (needs {UNDERPASS_CLEARANCE} m)"));
                }
            }
        }
        out.extend(self.road_refusal(points, levels, kind, lanes_fwd, lanes_bwd));
        out.dedup();
        out
    }

    /// Why a road drawn along `points` at `levels` cannot be built, if it
    /// cannot: it meets the railway at a height that is neither at grade
    /// nor clear of it. Such a crossing used to be built as a level
    /// crossing at the road's height: the junction node took the road's
    /// level, so a portal ramp halfway down to a tunnel dragged the track
    /// down into its cutting (and a ramp up to a viaduct hoisted it).
    /// The Road tool and the API refuse it instead, saying what would work.
    pub fn road_refusal(&self, points: &[Vec2], levels: &[f32], kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> Option<String> {
        let lv = |i: usize| levels.get(i).copied().filter(|l| l.is_finite()).unwrap_or(0.0);
        let highway = crate::roads::is_highway_spec(kind, lanes_fwd, lanes_bwd);
        for i in 0..points.len().saturating_sub(1) {
            let (a, b) = (points[i], points[i + 1]);
            for sid in self.roads.segments_in_box(a.min(b), a.max(b)) {
                let (p0, p1) = self.roads.segment_points(sid);
                // Closed: the track's height samples are vertices, and a
                // crossing exactly at one is still a crossing.
                let Some((t, u)) = crate::roads::segment_intersection_closed(a, b, p0, p1, 1e-3) else { continue };
                let s = self.roads.segment(sid);
                let mine = lv(i) + (lv(i + 1) - lv(i)) * t;
                if let Some(why) = rail_crossing_refusal(kind, s.kind, highway, mine, s.is_highway(), self.roads.segment_level(sid, u)) {
                    return Some(why);
                }
            }
        }
        None
    }

    /// Height of the road at a point on a segment.
    pub fn road_height(&self, sid: SegmentId, t: f32) -> f32 {
        crate::landscape::way_height(&self.roads, &self.road_z, &self.road_inner, sid, t)
    }

    /// The ground as built at a point: graded under roads and lots, the
    /// natural terrain elsewhere (the bed under open water).
    pub fn ground_height(&self, p: Vec2) -> f32 {
        self.ground.height(&self.landscape, p)
    }

    /// What something standing or driving at `p` stands on: a deck (over
    /// the water, a viaduct, the upper way of an underpass), else the built
    /// ground. Street furniture and the camera are set on this.
    pub fn surface_height(&self, p: Vec2) -> f32 {
        if !self.relief {
            return 0.0;
        }
        if let Some(z) = self.ground.deck_at(p) {
            return z;
        }
        self.ground_or_water(p)
    }

    /// The built ground, or the water's surface where there is water.
    pub(super) fn ground_or_water(&self, p: Vec2) -> f32 {
        let g = self.ground_height(p);
        if !self.landscape.is_flat() && self.landscape.is_water(p) { g.max(WATER_LEVEL) } else { g }
    }

    /// The surface at `p` nearest the height `hint`: the ground or any deck
    /// over it. Something moving along keeps to the level it was on, so a
    /// walker on the street under a viaduct stays under it.
    pub fn surface_height_near(&self, p: Vec2, hint: f32) -> f32 {
        if !self.relief {
            return 0.0;
        }
        let ground = self.ground_or_water(p);
        self.ground.decks_at(p).into_iter().fold(ground, |best, z| if (z - hint).abs() < (best - hint).abs() { z } else { best })
    }

    /// Height a vehicle drives at: the height of the way it is on (or, in
    /// a junction, the way it turns onto), so traffic on a viaduct stays up
    /// and traffic under it stays down whatever passes over where.
    pub fn car_height(&self, car: &crate::traffic::Car) -> f32 {
        self.car_height_at(car, car.pos)
    }

    /// [render] Is the vehicle in a tunnel: on a tunnel's lane, or in a
    /// junction between tunnels? The depth view draws it with the
    /// Underground layer and hides it above ground, whatever its height: a
    /// wreck near a portal, where the cutting is graded down to the road,
    /// used to show above ground and vanish in the Underground view (which
    /// went by height alone).
    pub fn car_in_tunnel(&self, car: &crate::traffic::Car) -> bool {
        let lanes = &self.traffic.net.lanes;
        let tunnel = |lane: crate::traffic::LaneId| match lanes.get(lane as usize).map(|l| &l.kind) {
            Some(crate::traffic::LaneKind::Road { segment, .. }) => Some(self.roads.try_segment(*segment).is_some_and(|s| s.tunnel)),
            _ => None,
        };
        let Some(&here) = car.route.get(car.idx) else { return false };
        match tunnel(here) {
            Some(t) => t,
            // A connector: in the tunnel when the ways on both sides are.
            None => {
                let before = car.idx.checked_sub(1).and_then(|k| car.route.get(k)).copied().and_then(tunnel);
                let after = car.route.get(car.idx + 1).copied().and_then(tunnel);
                before.unwrap_or(false) && after.unwrap_or(before.unwrap_or(false))
            }
        }
    }

    /// The height of the road under a car drawn at `p` somewhere along the
    /// last tick: the road of whichever lane it was on there (the lanes it
    /// passed this tick, and the one ahead), the nearest that `p` lies on.
    /// The renderer interpolates the car's position through the tick, so
    /// its height must follow that position: lifting it to where it ends
    /// the tick (`car_height`) made cars hop up or down a step every tick
    /// on a grade and float or sink by metres on the highway's hills.
    pub fn car_height_at(&self, car: &crate::traffic::Car, p: Vec2) -> f32 {
        if !self.relief {
            return 0.0;
        }
        let lanes = &self.traffic.net.lanes;
        let segment_of = |lane: crate::traffic::LaneId| match lanes.get(lane as usize).map(|l| &l.kind) {
            Some(crate::traffic::LaneKind::Road { segment, .. }) => Some(*segment),
            _ => None,
        };
        // Lanes the car may have been on during the tick: a few back from
        // where it is, and the lane after a connector (a connector has no
        // segment of its own).
        let lo = car.idx.saturating_sub(4);
        let hi = (car.idx + 1).min(car.route.len().saturating_sub(1));
        let mut best: Option<(f32, f32)> = None;
        for k in (lo..=hi.max(lo)).rev() {
            let Some(&lane) = car.route.get(k) else { continue };
            let seg = segment_of(lane).or_else(|| lanes.get(lane as usize).and_then(|l| l.next.first().copied()).and_then(segment_of));
            let Some((sid, s)) = seg.and_then(|sid| self.roads.try_segment(sid).map(|s| (sid, s))) else { continue };
            let (a, b) = self.roads.segment_points(sid);
            let (t, q) = crate::geom::project_on_segment(p, a, b);
            let d = q.distance(p);
            if d <= s.half_width() + 8.0 && best.is_none_or(|(bd, _)| d < bd - 0.01) {
                best = Some((d, self.road_height(sid, t)));
            }
        }
        if let Some((_, h)) = best {
            return h;
        }
        self.surface_height_near(p, self.ground_height(p))
    }

    /// The pad a lot is levelled to.
    pub fn lot_pad(&self, lot: LotId) -> f32 {
        if let Some(&h) = self.ground.pads.get(&lot) {
            return h;
        }
        self.lots.get(lot).map_or(0.0, |l| crate::landscape::lot_pad(&self.roads, &self.road_z, &self.road_inner, l, &self.landscape))
    }
}
