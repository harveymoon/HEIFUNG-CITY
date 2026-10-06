//! The landscape's hooks into the world: road heights and bridges, lots on
//! water or steep ground, the built ground, the highway and its through
//! traffic, and the docks with their ships.

use super::*;
use crate::landscape::{BRIDGE_CLEARANCE, HIGHWAY_HALF_GAP, HIGHWAY_LANES, HIGHWAY_Y, MAX_LOT_SLOPE, UNDERPASS_CLEARANCE, WATER_LEVEL};
use crate::ports::{self, Port, PortId, PortKind, Ship, ShipKind};
use crate::zoning::WaterMask;

/// Price of a metre of new bridge deck.
pub const BRIDGE_COST_PER_M: f32 = 60.0;
/// Steepest grade a street keeps; rail and paths have their own.
pub const STREET_GRADE: f32 = 0.07;
pub const RAIL_GRADE: f32 = 0.03;
pub const PATH_GRADE: f32 = 0.10;
/// At most this many through cars on the highway at once.
pub const MAX_THROUGH_CARS: usize = 36;
/// Chance per tick that a through car joins each carriageway.
pub const THROUGH_CHANCE: f64 = 0.45;
/// How far a bridge deck reaches onto each bank.
const ABUTMENT: f32 = 5.0;

/// Steepest grade a way of this kind keeps.
pub fn way_grade(kind: RoadKind) -> f32 {
    match kind {
        RoadKind::Rail => RAIL_GRADE,
        RoadKind::Path => PATH_GRADE,
        RoadKind::Street => STREET_GRADE,
    }
}

/// Ease node heights `z` to the grades of `segs` (a, b, length, grade):
/// first every free node is brought down to within its grade of a fixed
/// node below it (the approach to a cutting), then the low end of anything
/// still too steep is raised (fill and embankment). Fixed nodes never move.
fn ease_heights(z: &mut [f32], fixed: &[bool], segs: &[(usize, usize, f32, f32)]) {
    let n = z.len();
    if fixed.iter().any(|f| *f) {
        let mut cap: Vec<f32> = (0..n).map(|k| if fixed[k] { z[k] } else { f32::INFINITY }).collect();
        for _ in 0..200 {
            let mut changed = false;
            for &(a, b, len, grade) in segs {
                let max = len * grade;
                if !fixed[b] && cap[a] + max < cap[b] - 1e-3 {
                    cap[b] = cap[a] + max;
                    changed = true;
                }
                if !fixed[a] && cap[b] + max < cap[a] - 1e-3 {
                    cap[a] = cap[b] + max;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for k in 0..n {
            if !fixed[k] {
                z[k] = z[k].min(cap[k]);
            }
        }
    }
    for _ in 0..200 {
        let mut changed = false;
        for &(a, b, len, grade) in segs {
            let max = len * grade;
            if z[a] > z[b] + max + 1e-3 && !fixed[b] {
                z[b] = z[a] - max;
                changed = true;
            } else if z[b] > z[a] + max + 1e-3 && !fixed[a] {
                z[a] = z[b] - max;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

impl World {
    /// Insert a vertex wherever a polyline crosses a waterline, set back a
    /// few metres onto the bank, so the stretch over the water becomes its
    /// own segments: the bridge.
    pub(super) fn split_at_water(&self, points: &[Vec2]) -> Vec<Vec2> {
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
    pub(super) fn mark_bridges(&mut self) {
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
    pub(super) fn water_mask(&self) -> WaterMask {
        if self.landscape.is_flat() {
            return WaterMask::default();
        }
        WaterMask::new(&self.landscape.water_polygons())
    }

    /// Mark lots on ground too steep to level (or in the water) unusable.
    pub(super) fn grade_lots(&mut self) {
        if self.landscape.is_flat() {
            return;
        }
        let land = &self.landscape;
        for l in self.lots.values_mut() {
            if l.landlocked {
                continue;
            }
            let mut pts = l.polygon.clone();
            pts.push(l.center);
            if land.is_water(l.center) || land.slope_over(&pts) > MAX_LOT_SLOPE {
                l.landlocked = true;
            }
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
    pub(super) fn rebuild_ground(&mut self) {
        let land = &self.landscape;
        let flat = land.is_flat();
        let floor = if flat { 0.0 } else { crate::landscape::BANK_HEIGHT };
        let roads = &self.roads;
        let mut z: Vec<f32> = roads.nodes.iter().map(|nd| land.height(nd.pos).max(floor) + nd.level).collect();
        let mut fixed: Vec<bool> = roads.nodes.iter().map(|nd| nd.level != 0.0).collect();
        if !flat {
            // Bridge ends stand at deck height; the approaches ramp up to them.
            for (_, seg) in roads.alive_segments() {
                if seg.bridge && !seg.viaduct {
                    let deck = WATER_LEVEL + BRIDGE_CLEARANCE;
                    for k in [seg.a.0 as usize, seg.b.0 as usize] {
                        z[k] = z[k].max(deck);
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
            ease_heights(&mut z, &fixed, &segs);
            let mut lifted = false;
            for c in &crossings {
                let (zu, zl) = (seg_z(&z, c.upper), seg_z(&z, c.lower));
                // Clear, either way up (a street on a hill over the highway).
                if zu - zl >= UNDERPASS_CLEARANCE - 0.01 || zl - zu >= UNDERPASS_CLEARANCE - 0.01 {
                    continue;
                }
                let s = roads.segment(c.upper.0);
                let vertex = if c.upper.1 < 0.02 { Some(s.a.0 as usize) } else if c.upper.1 > 0.98 { Some(s.b.0 as usize) } else { None };
                // Only a way at grade is lifted: one the player set at a
                // level keeps it (the tool warns about it).
                if let Some(k) = vertex.filter(|&k| !fixed[k]) {
                    z[k] = zl + UNDERPASS_CLEARANCE;
                    fixed[k] = true;
                    lifted = true;
                }
            }
            if !lifted {
                break;
            }
        }
        self.relief = !flat || z.iter().any(|h| h.abs() > 1e-3) || self.roads.alive_segments().any(|(_, s)| s.bridge);
        self.road_z = z;
        self.ground = crate::landscape::Ground::build(&self.landscape, &self.roads, &self.road_z, &self.lots, self.config.half_extent + 200.0);
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
        for i in 0..points.len() - 1 {
            let len = points[i].distance(points[i + 1]);
            if len < 0.5 {
                continue;
            }
            let g = (z(i + 1) - z(i)).abs() / len;
            if g > grade + 1e-3 && (lv(i) != 0.0 || lv(i + 1) != 0.0) {
                let need = (z(i + 1) - z(i)).abs() / grade;
                out.push(format!("too steep between points {} and {}: {:.0}% over {:.0} m (at most {:.0}%; needs {:.0} m)", i + 1, i + 2, g * 100.0, len, grade * 100.0, need));
            }
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
                if !crate::roads::grade_separated(highway, mine, s.is_highway(), theirs) && gap >= 1.0 {
                    out.push(format!("meets a road {gap:.0} m below or above it at grade: raise or lower it {:.0} m more to pass over or under", crate::roads::GRADE_SEPARATION - gap));
                } else if crate::roads::grade_separated(highway, mine, s.is_highway(), theirs) && gap > 0.0 && gap < UNDERPASS_CLEARANCE && (mine != 0.0 && theirs != 0.0) {
                    out.push(format!("only {gap:.1} m clear of the road it crosses (needs {UNDERPASS_CLEARANCE} m)"));
                }
            }
        }
        out.dedup();
        out
    }

    /// Pay for bridge deck laid since the last rebuild.
    pub(super) fn charge_bridges(&mut self) {
        let metres: f32 = self.roads.alive_segments().filter(|(_, s)| s.bridge).map(|(id, _)| {
            let (a, b) = self.roads.segment_points(id);
            a.distance(b)
        }).sum();
        let new = metres - self.bridge_metres;
        if new > 1.0 {
            self.economy.debit(Flow::Construction, new * BRIDGE_COST_PER_M);
        }
        self.bridge_metres = metres;
    }

    /// Metres of bridge a road along `points` would need, and the cost.
    pub fn bridge_cost(&self, points: &[Vec2]) -> (f32, f32) {
        let mut wet = 0.0;
        for w in points.windows(2) {
            let len = w[0].distance(w[1]);
            let steps = (len / 4.0).ceil().max(1.0) as usize;
            for k in 0..steps {
                if self.landscape.is_water(w[0].lerp(w[1], (k as f32 + 0.5) / steps as f32)) {
                    wet += len / steps as f32;
                }
            }
        }
        let m = if wet > 0.0 { wet + ABUTMENT * 2.0 } else { 0.0 };
        (m, m * BRIDGE_COST_PER_M)
    }

    /// Height of the road at a point on a segment.
    pub fn road_height(&self, sid: SegmentId, t: f32) -> f32 {
        let Some(seg) = self.roads.try_segment(sid) else { return 0.0 };
        let za = self.road_z.get(seg.a.0 as usize).copied().unwrap_or(0.0);
        let zb = self.road_z.get(seg.b.0 as usize).copied().unwrap_or(0.0);
        za + (zb - za) * t.clamp(0.0, 1.0)
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
        self.ground_height(p).max(if self.landscape.is_flat() { f32::MIN } else { WATER_LEVEL })
    }

    /// The surface at `p` nearest the height `hint`: the ground or any deck
    /// over it. Something moving along keeps to the level it was on, so a
    /// walker on the street under a viaduct stays under it.
    pub fn surface_height_near(&self, p: Vec2, hint: f32) -> f32 {
        if !self.relief {
            return 0.0;
        }
        let ground = self.ground_height(p).max(if self.landscape.is_flat() { f32::MIN } else { WATER_LEVEL });
        self.ground.decks_at(p).into_iter().fold(ground, |best, z| if (z - hint).abs() < (best - hint).abs() { z } else { best })
    }

    /// Height a vehicle drives at: the height of the way it is on (or, in
    /// a junction, the way it turns onto), so traffic on a viaduct stays up
    /// and traffic under it stays down whatever passes over where.
    pub fn car_height(&self, car: &crate::traffic::Car) -> f32 {
        if !self.relief {
            return 0.0;
        }
        let p = car.pos;
        let lanes = &self.traffic.net.lanes;
        let segment_of = |lane: crate::traffic::LaneId| match lanes.get(lane as usize).map(|l| &l.kind) {
            Some(crate::traffic::LaneKind::Road { segment, .. }) => Some(*segment),
            _ => None,
        };
        let here = car.route.get(car.idx).copied();
        let seg = here.and_then(segment_of).or_else(|| {
            here.and_then(|l| lanes.get(l as usize)).and_then(|l| l.next.first().copied()).and_then(segment_of)
        });
        if let Some((sid, s)) = seg.and_then(|sid| self.roads.try_segment(sid).map(|s| (sid, s))) {
            let (a, b) = self.roads.segment_points(sid);
            let (t, q) = crate::geom::project_on_segment(p, a, b);
            if q.distance(p) <= s.half_width() + 8.0 {
                return self.road_height(sid, t);
            }
        }
        self.surface_height_near(p, self.ground_height(p))
    }

    /// The pad a lot is levelled to.
    pub fn lot_pad(&self, lot: LotId) -> f32 {
        if let Some(&h) = self.ground.pads.get(&lot) {
            return h;
        }
        self.lots.get(lot).map_or(0.0, |l| crate::landscape::lot_pad(&self.roads, &self.road_z, l, &self.landscape))
    }

    // ── the highway ─────────────────────────────────────────────────────

    /// Lay the through highway: two three-lane carriageways across the map
    /// at `HIGHWAY_Y`, bridging whatever water lies across them. Traffic
    /// drives on the right, so the eastbound carriageway is the north one.
    pub fn lay_highway(&mut self) -> [GestureId; 2] {
        let had_project = self.project.take();
        // Edge to edge of the drawn land: traffic comes from off the map.
        let x = crate::landscape::LAND_HALF - 40.0;
        let east = self.add_gesture(Gesture::Road { points: vec![Vec2::new(-x, HIGHWAY_Y + HIGHWAY_HALF_GAP), Vec2::new(x, HIGHWAY_Y + HIGHWAY_HALF_GAP)], lanes_fwd: HIGHWAY_LANES, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        let west = self.add_gesture(Gesture::Road { points: vec![Vec2::new(x, HIGHWAY_Y - HIGHWAY_HALF_GAP), Vec2::new(-x, HIGHWAY_Y - HIGHWAY_HALF_GAP)], lanes_fwd: HIGHWAY_LANES, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        self.implement();
        self.project = had_project;
        [east, west]
    }

    /// Ends of highway carriageways: where traffic comes in from elsewhere
    /// (`entries`) and leaves (`exits`), with the end segment.
    pub fn highway_ends(&self) -> (Vec<(NodeId, SegmentId)>, Vec<(NodeId, SegmentId)>) {
        let mut entries = Vec::new();
        let mut exits = Vec::new();
        for n in self.roads.leaf_nodes() {
            let Some(&sid) = self.roads.node(n).segments.iter().find(|&&s| self.roads.segment(s).alive) else { continue };
            let seg = self.roads.segment(sid);
            if !seg.is_highway() {
                continue;
            }
            // A one-way leaving the node is an entry, arriving an exit.
            let leaves = (seg.a == n && seg.lanes_fwd > 0) || (seg.b == n && seg.lanes_bwd > 0);
            if leaves {
                entries.push((n, sid));
            } else {
                exits.push((n, sid));
            }
        }
        (entries, exits)
    }

    /// Is this car passing through from elsewhere to elsewhere?
    pub fn is_through_car(car: &crate::traffic::Car) -> bool {
        // [carlife] Not a private car driven by the dealer or leaving town,
        // nor a van or lorry on its way back to base.
        car.citizen == CitizenId::default() && car.cargo.is_none() && car.bus.is_none() && car.owned.is_none() && car.errand == crate::vehicle::Errand::Trip
    }

    pub fn through_cars(&self) -> usize {
        self.traffic.cars.values().filter(|c| Self::is_through_car(c)).count()
    }

    /// Traffic from elsewhere: cars join the highway at a map edge and
    /// drive to the far end, whatever the city does.
    pub(super) fn step_through_traffic(&mut self) {
        let (entries, exits) = self.highway_ends();
        if entries.is_empty() || exits.is_empty() {
            return;
        }
        let mut live = self.through_cars();
        for (n, sid) in entries {
            if live >= MAX_THROUGH_CARS || !self.rng.random_bool(THROUGH_CHANCE) {
                continue;
            }
            let start = self.roads.node(n).pos;
            // The far end: the exit farthest from where it came in.
            let Some(&(en, esid)) = exits.iter().max_by(|a, b| self.roads.node(a.0).pos.distance(start).total_cmp(&self.roads.node(b.0).pos.distance(start))) else { continue };
            let end = self.roads.node(en).pos;
            let Some((start_lane, s0, _)) = self.traffic.net.nearest_lane_on(sid, start) else { continue };
            // Spread over the lanes.
            let lanes: Vec<crate::traffic::LaneId> = self.traffic.net.segment_lanes.get(&sid).cloned().unwrap_or_default();
            let start_lane = lanes.choose(&mut self.rng).copied().unwrap_or(start_lane);
            if self.traffic.net.lanes[start_lane as usize].cars.first().is_some_and(|&c| self.traffic.cars[c].s < 12.0) {
                continue;
            }
            let Some((goal, s1, _)) = self.traffic.net.nearest_lane_on(esid, end) else { continue };
            let s0 = s0.min(1.0);
            let Some((route, s0, s1)) = self.traffic.net.route_lanes(start_lane, s0, goal, s1) else { continue };
            let color = self.rng.random_range(0..8);
            let id = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, color);
            self.traffic.cars[id].model = crate::vehicle::VehicleModel::through(&mut crate::vehicle::model_rng(self.clock.tick, slotmap::Key::data(&id).as_ffi()));
            live += 1;
        }
    }

    /// Road ends people and lorries can arrive by: dead ends a car can drive
    /// away from, on a network that reaches the city's lots. Before any lot
    /// exists, every leaf.
    pub(super) fn city_leaves(&self) -> Vec<NodeId> {
        let leaves = self.roads.leaf_nodes();
        let comps: std::collections::HashSet<u32> = self.lots.values().filter(|l| self.zones.contains_key(l.zone)).filter_map(|l| self.road_component(l.access_node)).collect();
        let drivable = |n: NodeId| {
            self.roads.node(n).segments.iter().filter(|&&s| self.roads.segment(s).alive).all(|&s| {
                let seg = self.roads.segment(s);
                !seg.is_street() || (seg.a == n && seg.lanes_fwd > 0) || (seg.b == n && seg.lanes_bwd > 0) || seg.lanes() == 0
            })
        };
        let keep: Vec<NodeId> = leaves.iter().copied().filter(|&n| drivable(n) && (comps.is_empty() || self.road_component(n).is_some_and(|c| comps.contains(&c)))).collect();
        if keep.is_empty() { leaves } else { keep }
    }

    // ── docks ───────────────────────────────────────────────────────────

    /// Where a dock of `kind` would go for a click at `p`, or why not.
    pub fn plan_port(&self, kind: PortKind, p: Vec2) -> Result<Port, String> {
        let port = Port::plan(kind, p, &self.landscape)?;
        let fp = port.footprint();
        // Grown buildings under it are bought out on placement; a civic
        // structure there stops it.
        if let Some(b) = self.buildings_under(&fp).into_iter().filter_map(|(id, _)| self.buildings.get(id)).find(|b| Self::is_civic_structure(b)) {
            return Err(format!("{} stands there; bulldoze it first", b.name));
        }
        if self.facilities.values().any(|f| polygons_overlap(&f.footprint(), &fp)) || self.ports.ports.values().any(|o| polygons_overlap(&o.footprint(), &fp) || o.center.distance(port.center) < kind.size().0 * 0.6) {
            return Err("too close to another dock or station".into());
        }
        Ok(port)
    }

    /// Build a shipping port or ferry terminal at the water's edge near
    /// `p`: it gets a pier, a lot and building of its own, and a street stub
    /// along its landward side unless a street is already there.
    pub fn place_port(&mut self, kind: PortKind, p: Vec2) -> Result<BuildingId, String> {
        let port = self.plan_port(kind, p)?;
        if self.economy.treasury < kind.cost() {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, kind.label(), kind.cost()));
        }
        for (_, lot) in self.buildings_under(&port.footprint()) {
            self.take_over_lot(lot, kind.label())?;
        }
        self.economy.debit(Flow::Construction, kind.cost());
        let probe = port.front_mid() - port.seaward * rail::ROAD_GAP;
        let fused = self.roads.nearest_street(probe).is_some_and(|(_, _, q)| q.distance(probe) <= rail::ROAD_EDGE_FUSE);
        let stub = port.road_line();
        let pid = self.ports.ports.insert(port);
        let had_project = self.project.take();
        if !fused {
            self.add_gesture(Gesture::Road { points: stub.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        }
        if self.project.is_some() {
            self.implement();
        } else {
            self.rebuild_from_master();
        }
        self.project = had_project;
        let Some(lot_id) = self.ports.ports[pid].lot else {
            self.ports.ports.remove(pid);
            return Err("the dock found no street to front".into());
        };
        let name = {
            const NAMES: [&str; 12] = ["Kessler", "Morrow", "Saltmarsh", "Greyhaven", "Tideway", "Ironbank", "Halcyon", "Wharfside", "Neon Bay", "Driftmoor", "Calloway", "Ashgate"];
            let n = *NAMES.choose(&mut self.rng).unwrap();
            match kind {
                PortKind::ShippingPort => format!("Port of {n}"),
                PortKind::FerryTerminal => format!("{n} Ferry"),
            }
        };
        let id = self.place_special(lot_id, name, kind.jobs(), kind.jobs() / 3, |b| b.port = Some(kind));
        self.ports.ports[pid].building = Some(id);
        self.ports.ports[pid].last_sailing = self.clock.tick.saturating_sub(ports::FREIGHT_HEADWAY);
        self.refresh_port_links();
        self.events.push(SimEvent::LotsChanged);
        Ok(id)
    }

    /// Buildings whose lots lie under or across a footprint.
    fn buildings_under(&self, fp: &[Vec2]) -> Vec<(BuildingId, LotId)> {
        self.buildings.iter().filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, fp) || polygons_overlap(fp, &l.polygon))).map(|(id, b)| (id, b.lot)).collect()
    }

    /// The lot a dock stands on: its footprint, fronting the nearest street.
    pub(super) fn port_lot(&self, pid: PortId) -> Option<Lot> {
        let p = &self.ports.ports[pid];
        let corners = p.corners();
        let probe = p.front_mid() - p.seaward * rail::ROAD_GAP;
        let (front_segment, front_t, car_point) = self.roads.nearest_street(probe)?;
        Some(Lot {
            zone: ZoneId::default(),
            land_use: if p.kind == PortKind::ShippingPort { LandUse::Industrial } else { LandUse::Civic },
            polygon: corners.to_vec(),
            corners,
            front_edge: [corners[0], corners[1]],
            center: p.center,
            front_dir: -p.seaward,
            front_segment,
            front_t,
            access_node: self.roads.nearest_endpoint(front_segment, front_t),
            building: None,
            landlocked: false,
            footfall_today: 0,
            footfall_hour: 0,
            car_segment: Some(front_segment),
            car_point,
            park: None,
        })
    }

    /// Recompute whether each dock's street reaches the city and ships can
    /// reach its berth from off the map. Drops docks whose building is gone.
    pub(super) fn refresh_port_links(&mut self) {
        let gone: Vec<PortId> = self.ports.ports.iter().filter(|(_, p)| p.building.is_some_and(|b| !self.buildings.contains_key(b))).map(|(id, _)| id).collect();
        for id in gone {
            self.ports.ports.remove(id);
        }
        let comps: std::collections::HashSet<u32> = self.lots.values().filter(|l| self.zones.contains_key(l.zone)).filter_map(|l| self.road_component(l.access_node)).collect();
        let ids: Vec<PortId> = self.ports.ports.keys().collect();
        for id in ids {
            let (lot, berth) = (self.ports.ports[id].lot, self.ports.ports[id].berth);
            let road = lot.and_then(|l| self.lots.get(l)).and_then(|l| self.road_component(l.access_node)).is_some_and(|c| comps.contains(&c));
            let out = self.landscape.water_route(berth, None).is_some();
            let p = &mut self.ports.ports[id];
            p.road_connected = road;
            p.reaches_elsewhere = out;
        }
        // Goods waiting at a dock that no longer works go by road.
        let working: Vec<BuildingId> = self.ports.working(PortKind::ShippingPort);
        let stranded: Vec<Cargo> = {
            let (keep, lost): (Vec<_>, Vec<_>) = std::mem::take(&mut self.ports.inbound).into_iter().partition(|(b, _)| working.contains(b));
            self.ports.inbound = keep;
            lost.into_iter().map(|(_, c)| c).collect()
        };
        for c in stranded {
            self.dispatch_truck(super::market::TruckOrigin::CityEdge, c, 6);
        }
    }

    /// The working shipping port nearest a building, for imports by sea.
    pub(super) fn nearest_port(&self, to: BuildingId) -> Option<BuildingId> {
        let p = self.building_entrance(to)?;
        self.ports.working(PortKind::ShippingPort).into_iter().filter_map(|b| self.building_entrance(b).map(|q| (b, q.distance(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(b, _)| b)
    }

    /// Order goods by sea: the buyer pays the cheaper sea price and the load
    /// waits at the port for the next freighter. False when there is no port.
    pub(super) fn import_by_sea(&mut self, to: BuildingId, cargo: Cargo) -> bool {
        let Some(port) = self.nearest_port(to) else { return false };
        let refund = cargo.price * (1.0 - ports::SHIP_IMPORT_FACTOR);
        if let Some(bz) = self.buildings.get_mut(to).and_then(|b| b.business.as_mut()) {
            bz.money += refund;
            bz.spend_today -= refund;
        }
        self.economy.market.imports_today -= refund;
        self.ports.inbound.push((port, Cargo { from: port, price: cargo.price * ports::SHIP_IMPORT_FACTOR, ..cargo }));
        true
    }

    /// Export income multiplier from docks.
    pub(super) fn sea_export_bonus(&self) -> f32 {
        if self.ports.working(PortKind::ShippingPort).is_empty() { 1.0 } else { ports::SHIP_EXPORT_BONUS }
    }

    /// Sail ships, dispatch new ones on their headways, land freight and
    /// ferry passengers.
    pub(super) fn step_ships(&mut self) {
        if self.ports.ports.is_empty() && self.ports.ships.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        if tick % 10 == 3 {
            self.dispatch_ships();
        }
        let ids: Vec<crate::ports::ShipId> = self.ports.ships.keys().collect();
        let mut docked = Vec::new();
        let mut done = Vec::new();
        for id in ids {
            let ship = &mut self.ports.ships[id];
            ship.prev_s = ship.s;
            if ship.dwell > 0 {
                ship.dwell -= 1;
                continue;
            }
            let total = crate::geom::path_length(&ship.path);
            ship.s += ship.speed;
            if !ship.docked && ship.s >= ship.berth_s {
                ship.s = ship.berth_s;
                ship.docked = true;
                ship.dwell = match ship.kind {
                    ShipKind::Freighter => ports::FREIGHTER_DWELL,
                    ShipKind::Ferry => ports::FERRY_DWELL,
                };
                docked.push(id);
            } else if ship.s >= total {
                done.push(id);
            }
        }
        for id in docked {
            self.ship_docked(id);
        }
        for id in done {
            self.ports.ships.remove(id);
        }
        self.ports.stats.ships_out = self.ports.ships.len() as u32;
    }

    fn dispatch_ships(&mut self) {
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day() as u32;
        let ports_now: Vec<(PortId, Port)> = self.ports.ports.iter().map(|(id, p)| (id, p.clone())).collect();
        self.ports.stats.ports = ports_now.iter().filter(|(_, p)| p.kind == PortKind::ShippingPort).count() as u32;
        self.ports.stats.terminals = ports_now.iter().filter(|(_, p)| p.kind == PortKind::FerryTerminal).count() as u32;
        self.ports.stats.waiting_cargo = self.ports.inbound.len() as u32;
        for (pid, port) in &ports_now {
            if !port.works() || self.ports.ships.len() >= ports::MAX_SHIPS {
                continue;
            }
            let Some(b) = port.building else { continue };
            if self.buildings.get(b).is_none_or(|x| x.is_derelict()) {
                continue;
            }
            let busy = self.ports.ships.values().any(|s| s.port == b);
            if busy {
                continue;
            }
            match port.kind {
                PortKind::ShippingPort => {
                    let waiting = self.ports.inbound.iter().any(|(p, _)| *p == b);
                    let due = tick.saturating_sub(port.last_sailing) >= ports::FREIGHT_HEADWAY && (waiting || tick.saturating_sub(port.last_sailing) >= ports::FREIGHT_HEADWAY * 2);
                    if !due {
                        continue;
                    }
                    let Some(out) = self.landscape.water_route(port.berth, None) else { continue };
                    // In from elsewhere, to the berth, and back out.
                    let mut path: Vec<Vec2> = out.iter().rev().copied().collect();
                    let berth_s = crate::geom::path_length(&path);
                    path.extend(out.iter().skip(1).copied());
                    let cargo: Vec<Cargo> = {
                        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.ports.inbound).into_iter().partition(|(p, _)| *p == b);
                        self.ports.inbound = rest;
                        mine.into_iter().map(|(_, c)| c).collect()
                    };
                    let speed = self.config.metres_per_tick(ports::FREIGHTER_SPEED);
                    let color = self.rng.random_range(0..6);
                    self.ports.ships.insert(Ship { kind: ShipKind::Freighter, path, s: 0.0, prev_s: 0.0, speed, dwell: 0, port: b, to: None, cargo, passengers: 0, docked: false, berth_s, color });
                    self.ports.ports[*pid].last_sailing = tick;
                }
                PortKind::FerryTerminal => {
                    if !(ports::FERRY_SERVICE_START..ports::FERRY_SERVICE_END).contains(&minute) || tick.saturating_sub(port.last_sailing) < ports::FERRY_HEADWAY {
                        continue;
                    }
                    // To the next terminal on connected water, else out to elsewhere.
                    let others: Vec<(BuildingId, Vec2)> = ports_now.iter().filter(|(oid, o)| oid != pid && o.kind == PortKind::FerryTerminal && o.works()).filter_map(|(_, o)| o.building.map(|ob| (ob, o.berth))).collect();
                    let mut chosen: Option<(Option<BuildingId>, Vec<Vec2>)> = None;
                    let start = (tick / ports::FERRY_HEADWAY) as usize;
                    for k in 0..others.len() {
                        let (ob, ob_berth) = others[(start + k) % others.len()];
                        if let Some(route) = self.landscape.water_route(port.berth, Some(ob_berth)) {
                            chosen = Some((Some(ob), route));
                            break;
                        }
                    }
                    if chosen.is_none() && port.reaches_elsewhere {
                        chosen = self.landscape.water_route(port.berth, None).map(|r| (None, r));
                    }
                    let Some((to, route)) = chosen else { continue };
                    let berth_s = crate::geom::path_length(&route);
                    let mut path = route.clone();
                    path.extend(route.iter().rev().skip(1).copied());
                    let speed = self.config.metres_per_tick(ports::FERRY_SPEED);
                    let riders = self.ferry_riders(b, to);
                    let fares = riders as f32 * ports::FERRY_FARE;
                    // Ferry fares are public transport income, booked with the bus fares.
                    self.economy.credit(Flow::BusFares, fares);
                    self.ports.stats.ferry_riders_today += riders;
                    self.ports.stats.ferry_fares_today += fares;
                    self.ports.ships.insert(Ship { kind: ShipKind::Ferry, path, s: 0.0, prev_s: 0.0, speed, dwell: ports::FERRY_DWELL, port: b, to, cargo: Vec::new(), passengers: riders, docked: false, berth_s, color: 0 });
                    self.ports.ports[*pid].last_sailing = tick;
                }
            }
        }
    }

    /// People living near either end of a sailing who take the ferry.
    fn ferry_riders(&mut self, from: BuildingId, to: Option<BuildingId>) -> u32 {
        let ends: Vec<Vec2> = [Some(from), to].into_iter().flatten().filter_map(|b| self.building_entrance(b)).collect();
        let near = self.citizens.values().filter(|c| c.home.and_then(|h| self.building_entrance(h)).is_some_and(|p| ends.iter().any(|e| e.distance(p) < 500.0))).count() as u32;
        // Out to elsewhere draws visitors and commuters from off the map.
        let base = if to.is_none() { 8 } else { 2 };
        (near / 12 + base + self.rng.random_range(0..6)).min(ports::FERRY_CAPACITY)
    }

    fn ship_docked(&mut self, id: crate::ports::ShipId) {
        let Some(ship) = self.ports.ships.get_mut(id) else { return };
        if ship.kind == ShipKind::Freighter {
            let port = ship.port;
            let cargo = std::mem::take(&mut ship.cargo);
            self.ports.stats.freight_landed_today += cargo.len() as u32;
            for c in cargo {
                self.dispatch_truck(super::market::TruckOrigin::Building(port), c, 6);
            }
        }
    }

    /// Daily books for the docks: upkeep and fresh counters.
    pub(super) fn daily_ports(&mut self) {
        let upkeep: f32 = self.ports.ports.values().map(|p| p.kind.upkeep_per_day()).sum();
        if upkeep > 0.0 {
            self.economy.debit(Flow::TransitUpkeep, upkeep);
        }
        let s = &mut self.ports.stats;
        s.freight_landed_today = 0;
        s.ferry_riders_today = 0;
        s.ferry_fares_today = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::{LandscapeParams, LandscapePreset, WaterKind};
    use crate::mind::IdleMind;

    fn valley(seed: u64) -> World {
        World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, seed), ..WorldConfig::default() })
    }

    /// A point on the main river inside the basin, its flow direction, the
    /// unit vector from it towards the middle of the map, and its width.
    fn river_spot(w: &World) -> (Vec2, Vec2, Vec2, f32) {
        let r = &w.landscape.rivers[0];
        let p = *r.points.iter().min_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap();
        let (seg, _, _) = r.nearest(p);
        let flow = r.flow(seg);
        let mut n = Vec2::new(-flow.y, flow.x);
        if n.dot(-p) < 0.0 {
            n = -n;
        }
        (p, flow, n, r.width)
    }

    #[test]
    fn worlds_from_the_same_landscape_are_the_same() {
        let (a, b) = (valley(9), valley(9));
        assert_eq!(a.landscape.rivers.len(), b.landscape.rivers.len());
        assert_eq!(a.landscape.rivers[0].points, b.landscape.rivers[0].points);
        assert_eq!(a.road_z, b.road_z);
        for k in 0..20 {
            let p = Vec2::new(k as f32 * 90.0 - 900.0, (k * 7 % 11) as f32 * 150.0 - 800.0);
            assert_eq!(a.ground_height(p), b.ground_height(p));
        }
        assert_ne!(valley(10).landscape.rivers[0].points, a.landscape.rivers[0].points);
    }

    #[test]
    fn no_lots_on_water_and_bridges_where_roads_cross_it() {
        let mut w = valley(3);
        let (p, flow, n, width) = river_spot(&w);
        let treasury = w.economy.treasury;
        // A road across the river, and a zone straddling it.
        let a = p + n * 160.0;
        let b = p - n * 160.0;
        w.add_road(&[a, b], 1, 1);
        let bridges: Vec<SegmentId> = w.roads.alive_segments().filter(|(_, s)| s.bridge && s.lanes() == 2).map(|(id, _)| id).collect();
        assert_eq!(bridges.len(), 1, "one bridge span over the river");
        let (b0, b1) = w.roads.segment_points(bridges[0]);
        assert!(b0.distance(b1) >= width && b0.distance(b1) < width + 30.0, "the span is the river plus abutments: {}", b0.distance(b1));
        assert!(w.road_height(bridges[0], 0.5) >= WATER_LEVEL + BRIDGE_CLEARANCE - 0.01, "the deck clears the water");
        assert!(w.surface_height(p) > 5.0, "a car on the bridge is on the deck");
        assert!(w.economy.treasury < treasury - width * BRIDGE_COST_PER_M * 0.9, "the bridge was paid for");
        let (m, cost) = w.bridge_cost(&[a, b]);
        assert!(m >= width && cost > 0.0);
        // Gentle grades: no street segment steeper than its limit.
        for (id, s) in w.roads.alive_segments().filter(|(_, s)| s.is_street()) {
            let (pa, pb) = w.roads.segment_points(id);
            let dz = (w.road_z[s.a.0 as usize] - w.road_z[s.b.0 as usize]).abs();
            assert!(dz <= pa.distance(pb) * STREET_GRADE + 0.05, "grade on {id:?}");
        }
        let zone = vec![p + n * 150.0 - flow * 120.0, p + n * 150.0 + flow * 120.0, p - n * 150.0 + flow * 120.0, p - n * 150.0 - flow * 120.0];
        w.add_zone(zone, LandUse::Residential);
        assert!(w.lots.values().filter(|l| l.usable()).count() >= 6, "lots on both banks");
        for l in w.lots.values() {
            assert!(!w.landscape.is_water(l.center), "lot centre in the river at {:?}", l.center);
            let wet = l.polygon.iter().filter(|q| w.landscape.water_at(**q).is_some_and(|h| h.kind == WaterKind::River) && w.landscape.rivers[0].nearest(**q).2 < width * 0.5 - 2.0).count();
            assert_eq!(wet, 0, "lot outline reaches into the river");
        }
        // Lots sit on level pads at their street's height.
        let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
        let pad = w.lot_pad(lot);
        assert!((w.ground_height(w.lots[lot].center) - pad).abs() < 0.3, "the ground under a lot is its pad");
    }

    #[test]
    fn the_highway_is_there_from_the_start_with_through_traffic_and_ramps_join_it() {
        let mut w = valley(4);
        let hw: Vec<&crate::roads::Segment> = w.roads.alive_segments().map(|(_, s)| s).filter(|s| s.is_highway()).collect();
        assert!(!hw.is_empty(), "highway carriageways exist");
        assert!(hw.iter().any(|s| s.bridge), "the highway bridges the river");
        let (entries, exits) = w.highway_ends();
        assert_eq!((entries.len(), exits.len()), (2, 2), "two carriageways, each in from and out to elsewhere");
        let mut minds = IdleMind;
        let mut seen = 0;
        for _ in 0..60 {
            w.tick(&mut minds);
            seen = seen.max(w.through_cars());
        }
        assert!(seen >= 4, "through traffic flows: {seen}");
        assert!(w.traffic.cars.values().any(|c| World::is_through_car(c) && c.v > 15.0), "at highway speed");
        // A street crossing the highway passes over it: no junction.
        w.add_road(&[Vec2::new(60.0, 300.0), Vec2::new(60.0, 540.0)], 1, 1);
        let crossing = w.roads.nodes.iter().any(|n| n.pos.distance(Vec2::new(60.0, HIGHWAY_Y + HIGHWAY_HALF_GAP)) < 2.0 && n.segments.len() >= 3);
        assert!(!crossing, "streets fly over the highway");
        // An on-ramp: a one-way lane from the street dropped onto the eastbound carriageway.
        w.add_road(&[Vec2::new(60.0, 470.0), Vec2::new(150.0, HIGHWAY_Y + HIGHWAY_HALF_GAP + 4.0)], 1, 0);
        let merge = w.roads.node_near(Vec2::new(150.0, HIGHWAY_Y + HIGHWAY_HALF_GAP), 6.0).expect("the ramp end joined the carriageway");
        assert_eq!(w.roads.node(merge).segments.iter().filter(|&&s| w.roads.segment(s).alive).count(), 3);
        let inter = w.traffic.net.intersections.iter().find(|i| i.node == merge).expect("a merge");
        assert!(!inter.signalized, "ramps merge without lights");
        let from = w.roads.node_near(Vec2::new(60.0, 300.0), 2.0).unwrap();
        let (_, exits) = w.highway_ends();
        assert!(exits.iter().any(|&(e, _)| w.roads.path(from, e).is_some()), "the city reaches elsewhere by the ramp");
    }

    #[test]
    fn hydro_only_beside_a_river_with_a_current() {
        let mut w = valley(6);
        w.economy.treasury = 500_000.0;
        let (p, flow, n, width) = river_spot(&w);
        w.landscape.rivers[0].current = 1.5;
        let road_at = p + n * (width * 0.5 + 45.0);
        w.add_road(&[road_at - flow * 200.0, road_at + flow * 200.0], 1, 1);
        let zone = vec![p + n * (width * 0.5 + 2.0) - flow * 150.0, p + n * (width * 0.5 + 2.0) + flow * 150.0, p + n * (width * 0.5 + 90.0) + flow * 150.0, p + n * (width * 0.5 + 90.0) - flow * 150.0];
        w.add_zone(zone, LandUse::Industrial);
        // A lot far from any water.
        w.add_road(&[Vec2::new(-100.0, -100.0), Vec2::new(100.0, -100.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-90.0, -90.0), Vec2::new(90.0, -90.0), Vec2::new(90.0, -20.0), Vec2::new(-90.0, -20.0)], LandUse::Industrial);
        let dry = w.lots.iter().filter(|(_, l)| l.usable() && l.center.length() < 150.0 && l.area() >= 300.0).map(|(id, _)| id).next().expect("a dry lot");
        assert!(w.power_site(dry, PowerPlantKind::Hydro).is_err());
        assert!(w.build_power(dry, PowerPlantKind::Hydro).is_err(), "no hydro away from water");
        assert!(w.build_power(dry, PowerPlantKind::Tidal).is_err(), "no tide in a valley");
        let near_river = |w: &World| {
            w.lots
                .iter()
                .filter(|(_, l)| l.usable() && l.area() >= 300.0 && l.land_use == LandUse::Industrial && l.center.distance(p) < 200.0)
                .min_by(|a, b| w.landscape.rivers[0].nearest(a.1.center).2.total_cmp(&w.landscape.rivers[0].nearest(b.1.center).2))
                .map(|(id, _)| id)
        };
        let by_river = near_river(&w).expect("a riverside lot after the rebuild");
        assert!(w.power_site(by_river, PowerPlantKind::Hydro).is_ok(), "{:?}", w.power_site(by_river, PowerPlantKind::Hydro));
        let plant = w.build_power(by_river, PowerPlantKind::Hydro).expect("hydro by the river");
        assert_eq!(w.buildings[plant].plant, Some(PowerPlantKind::Hydro));
        w.allocate_power();
        assert!(w.economy.power.supply_mw > 50.0, "the river turns the turbines: {}", w.economy.power.supply_mw);
        // Slow water will not do.
        w.landscape.rivers[0].current = 0.3;
        let other = w.lots.iter().filter(|(id, l)| *id != by_river && l.usable() && l.building.is_none() && l.area() >= 300.0 && w.landscape.river_beside(&l.polygon, 30.0).is_some()).map(|(id, _)| id).next();
        if let Some(other) = other {
            assert!(w.build_power(other, PowerPlantKind::Hydro).unwrap_err().contains("too slow"));
        }
        // Geothermal only over a hot spot.
        assert!(w.power_site(dry, PowerPlantKind::Geothermal).is_err() || w.landscape.hot_spot_at(w.lots[dry].center).is_some());
    }

    #[test]
    fn docks_go_on_the_shore_build_a_pier_and_ships_call() {
        let mut w = valley(8);
        w.economy.treasury = 500_000.0;
        let (p, flow, n, width) = river_spot(&w);
        // Away from the water: refused.
        assert!(w.place_port(PortKind::ShippingPort, Vec2::new(0.0, 0.0)).is_err());
        // A city street and some homes nearby so the dock connects.
        let road_at = p + n * (width * 0.5 + 110.0);
        w.add_road(&[road_at - flow * 300.0, road_at + flow * 300.0], 1, 1);
        w.add_zone(vec![road_at + n * 10.0 - flow * 100.0, road_at + n * 10.0 + flow * 100.0, road_at + n * 80.0 + flow * 100.0, road_at + n * 80.0 - flow * 100.0], LandUse::Residential);
        let click = p + n * (width * 0.5 + 20.0);
        let id = w.place_port(PortKind::ShippingPort, click).expect("a port on the river bank");
        let dock = w.ports.port_of(id).unwrap().clone();
        assert!(w.landscape.is_water(dock.pier[1]), "the pier reaches the water");
        assert!(!w.landscape.is_water(dock.center), "the yard is on land");
        // The stub joins the city street.
        let [s0, _] = dock.road_line();
        w.add_road(&[s0, road_at - flow * 300.0], 1, 1);
        assert!(w.ports.port_of(id).unwrap().reaches_elsewhere, "the river runs off the map");
        assert!(w.ports.port_of(id).unwrap().road_connected, "the dock's street reaches the city");
        // A ferry terminal on the same bank a little way along.
        let ferry = w.place_port(PortKind::FerryTerminal, click + flow * 170.0);
        // Wherever it lands, a second dock never overlaps the first.
        if let Ok(f) = ferry {
            let fp = w.ports.port_of(f).unwrap().footprint();
            assert!(!polygons_overlap(&fp, &dock.footprint()));
        }
        // A lake is no place for a shipping port.
        if let Some(lake) = w.landscape.lakes.first() {
            let edge = lake.outline[0] + (lake.outline[0] - lake.center).normalize() * 20.0;
            if let Err(e) = w.plan_port(PortKind::ShippingPort, edge) {
                assert!(e.contains("lake") || e.contains("edge") || e.contains("ragged") || e.contains("steep"), "{e}");
            }
        }
        // A freighter calls.
        let mut minds = IdleMind;
        let mut sailed = false;
        for _ in 0..600 {
            w.tick(&mut minds);
            if w.ports.ships.values().any(|s| s.kind == ShipKind::Freighter) {
                sailed = true;
                break;
            }
        }
        assert!(sailed, "a freighter sails for the port");
        let ship = w.ports.ships.values().find(|s| s.kind == ShipKind::Freighter).unwrap();
        let (start, _) = ship.pose_at(0.0);
        assert!(start.x.abs() > crate::landscape::LAND_HALF - 100.0 || start.y.abs() > crate::landscape::LAND_HALF - 100.0, "it comes in from off the map: {start:?}");
        assert!(ship.pose_at(ship.berth_s).0.distance(dock.berth) < 1.0, "it ties up at the berth");
    }

    #[test]
    fn landscapes_and_docks_survive_a_save_and_old_saves_load_flat() {
        let mut w = valley(12);
        w.economy.treasury = 500_000.0;
        let (p, _, n, width) = river_spot(&w);
        let _ = w.place_port(PortKind::FerryTerminal, p + n * (width * 0.5 + 18.0));
        let text = w.to_ron().unwrap();
        let back = World::from_ron(&text).unwrap();
        assert_eq!(back.landscape.params, w.landscape.params);
        assert_eq!(back.landscape.rivers[0].points, w.landscape.rivers[0].points);
        assert_eq!(back.landscape.lakes.len(), w.landscape.lakes.len());
        assert_eq!(back.ports.ports.len(), w.ports.ports.len());
        assert_eq!(back.road_z.len(), back.roads.nodes.len(), "road heights rebuilt on load");
        assert_eq!(back.config.landscape, back.landscape.params);
        let q = p + n * 300.0;
        assert!((back.ground_height(q) - w.ground_height(q)).abs() < 1e-3);
        // A save from before landscapes: no landscape field at all.
        let mut old = World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::default() });
        old.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        let text = old.to_ron().unwrap();
        let start = text.find("\n    landscape: (").expect("landscape field") + 1;
        let mut depth = 0;
        let mut end = start;
        for (i, ch) in text[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let stripped = format!("{}{}", &text[..start], text[end..].trim_start_matches(','));
        let loaded = World::from_ron(&stripped).expect("an old save loads");
        assert!(loaded.landscape.is_flat());
        assert_eq!(loaded.ground_height(Vec2::new(20.0, 30.0)), 0.0);
        assert_eq!(loaded.surface_height(Vec2::new(20.0, 0.0)), 0.0);
    }
}
