//! The landscape's hooks into the world: road heights and bridges, lots on
//! water or steep ground, the built ground, the highway and its through
//! traffic, and the docks with their ships.

use super::*;
use crate::landscape::{BRIDGE_CLEARANCE, HIGHWAY_HALF_GAP, HIGHWAY_LANES, HIGHWAY_Y, UNDERPASS_CLEARANCE, WATER_LEVEL};
use crate::ports::{self, Port, PortId, PortKind, Ship, ShipKind};
use crate::zoning::WaterMask;

/// Price of a metre of new bridge deck.
pub const BRIDGE_COST_PER_M: f32 = 60.0;
/// Steepest grade a street keeps; rail and paths have their own.
pub const STREET_GRADE: f32 = 0.07;
pub const RAIL_GRADE: f32 = 0.03;
pub const PATH_GRADE: f32 = 0.10;
/// At most this many through cars on the highway at once.
pub const MAX_THROUGH_CARS: usize = 100;
/// Chance per tick that a through car joins each carriageway.
pub const THROUGH_CHANCE: f64 = 0.45;
/// How far a bridge deck reaches onto each bank.
const ABUTMENT: f32 = 5.0;

/// A way of `mine` kind at level `mine_level` meeting one of `theirs` kind
/// at `their_level`, where one of them is the railway: refused (with the
/// reason) unless they pass one over the other or meet at grade, within a
/// metre. A level crossing has to be level: the track cannot follow a road
/// down a portal ramp or up to a deck.
fn rail_crossing_refusal(mine: RoadKind, theirs: RoadKind, highway: bool, mine_level: f32, their_highway: bool, their_level: f32) -> Option<String> {
    if (mine == RoadKind::Rail) == (theirs == RoadKind::Rail) {
        return None;
    }
    let gap = mine_level - their_level;
    if crate::roads::grade_separated(highway, mine_level, their_highway, their_level) || gap.abs() < 1.0 {
        return None;
    }
    let need = crate::roads::GRADE_SEPARATION - gap.abs();
    let (what, other) = if mine == RoadKind::Rail { ("the track", "the road") } else { ("the road", "the railway") };
    let (way, verb) = if gap < 0.0 { ("below", "lower") } else { ("above", "raise") };
    Some(format!(
        "{what} meets {other} {:.1} m {way} it: a level crossing must be at grade. {} {what} {need:.1} m more there to pass {} it, or start the ramp past the crossing",
        gap.abs(),
        if verb == "lower" { "Lower" } else { "Raise" },
        if gap < 0.0 { "under" } else { "over" }
    ))
}

/// Spacing of the inner height samples along a long street or path.
pub const INNER_STEP: f32 = 30.0;

/// Heights inside long streets and paths, between their nodes, so a street
/// drawn as one long leg over a hill follows the land instead of running
/// dead straight from end to end (and the ground under it is not cut and
/// filled by metres to that straight line).
///
/// A street's vertices decide where its parcels are cut (parcel keys are
/// their centres and frontages), so the samples cannot be new road nodes as
/// the highway's and the track's are (`sample_heights`): they are kept per
/// segment as `(t, z)` in rising `t`, every `INNER_STEP` or so, and read
/// through [`crate::landscape::way_height`] by everything that sets things
/// on a way (cars, lots, the built ground, the street mesh). Nothing about
/// the network or the parcels changes.
///
/// Each sample takes the land's height there, held within the way's grade
/// of both ends and of its neighbours (forward and back passes). Ways off
/// the ground (decks, tunnels, cuttings, any level the player set), the
/// highway and track (which have real vertices), and ways in a grade
/// crossing (whose underpass is worked out on the straight line) keep the
/// straight line, as does the flat map. An empty list is the straight line.
fn inner_heights(land: &crate::landscape::Landscape, roads: &crate::roads::RoadNetwork, z: &[f32], floor: f32, crossings: &[crate::roads::Crossing]) -> Vec<Vec<(f32, f32)>> {
    let mut out = vec![Vec::new(); roads.segments.len()];
    if land.is_flat() {
        return out;
    }
    let crossed: std::collections::BTreeSet<u32> = crossings.iter().flat_map(|c| [c.upper.0 .0, c.lower.0 .0]).collect();
    let edge = crate::landscape::LAND_HALF;
    for (sid, s) in roads.alive_segments() {
        if s.bridge || s.viaduct || s.tunnel || s.is_rail() || s.is_highway() || s.ring || crossed.contains(&sid.0) {
            continue;
        }
        if roads.node(s.a).level != 0.0 || roads.node(s.b).level != 0.0 {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let len = a.distance(b);
        if len < INNER_STEP * 1.5 {
            continue;
        }
        let (za, zb) = (z.get(s.a.0 as usize).copied().unwrap_or(0.0), z.get(s.b.0 as usize).copied().unwrap_or(0.0));
        let g = way_grade(s.kind);
        let m = (len / INNER_STEP).round().max(2.0) as usize;
        let ds = len / m as f32;
        let mut h: Vec<f32> = (1..m)
            .map(|k| {
                let (t, sk) = (k as f32 / m as f32, k as f32 * ds);
                let p = a.lerp(b, t).clamp(Vec2::splat(-edge), Vec2::splat(edge));
                let lo = (za - g * sk).max(zb - g * (len - sk));
                let hi = (za + g * sk).min(zb + g * (len - sk));
                let line = za + (zb - za) * t;
                if lo > hi { line } else { land.height(p).max(floor).clamp(lo, hi) }
            })
            .collect();
        // Neighbour to neighbour within the grade, both ways.
        let mut prev = za;
        for v in h.iter_mut() {
            *v = v.clamp(prev - g * ds, prev + g * ds);
            prev = *v;
        }
        let mut next = zb;
        for v in h.iter_mut().rev() {
            *v = v.clamp(next - g * ds, next + g * ds);
            next = *v;
        }
        let samples: Vec<(f32, f32)> = h.into_iter().enumerate().map(|(k, v)| ((k + 1) as f32 / m as f32, v)).collect();
        if samples.iter().any(|&(t, v)| (v - (za + (zb - za) * t)).abs() > 0.05) {
            out[sid.0 as usize] = samples;
        }
    }
    out
}

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
/// node below it (the approach to a cutting), then the high end of anything
/// too steep is lowered (a cutting), never below its `least` (a bridge end
/// keeps its deck), so a way climbing to the hills at the map edge cuts
/// into them rather than carrying the basin up on an embankment as high as
/// the hills; last the low end of anything still too steep is raised (fill
/// and embankment, the approaches to a deck). Fixed nodes never move.
fn ease_heights(z: &mut [f32], fixed: &[bool], least: &[f32], segs: &[(usize, usize, f32, f32)]) {
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
            if z[a] > z[b] + max + 1e-3 && !fixed[a] && z[a] > least[a] + 1e-3 {
                z[a] = (z[b] + max).max(least[a]);
                changed = true;
            } else if z[b] > z[a] + max + 1e-3 && !fixed[b] && z[b] > least[b] + 1e-3 {
                z[b] = (z[a] + max).max(least[b]);
                changed = true;
            }
        }
        if !changed {
            break;
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

    /// Mark lots on ground too steep to level (a cliff, or more rise than
    /// one terrace takes) or in the water unusable. Ordinary hillsides are
    /// graded: the lot's pad is levelled to its street (`Ground`).
    pub(super) fn grade_lots(&mut self) {
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
        self.ground = crate::landscape::Ground::build(&self.landscape, &self.roads, &self.road_z, &self.road_inner, &self.lots, self.ground_half());
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

    /// Pay for bridge deck laid since the last rebuild.
    pub(super) fn charge_bridges(&mut self) {
        // Only deck on the map is paid for: the ways run on elsewhere at
        // elsewhere's expense.
        let edge = crate::landscape::LAND_HALF;
        let metres: f32 = self.roads.alive_segments().filter(|(_, s)| s.bridge).map(|(id, _)| {
            let (a, b) = self.roads.segment_points(id);
            let inside = (0..8).filter(|k| {
                let p = a.lerp(b, (*k as f32 + 0.5) / 8.0);
                p.x.abs() <= edge && p.y.abs() <= edge
            }).count();
            a.distance(b) * inside as f32 / 8.0
        }).sum();
        let new = metres - self.bridge_metres;
        if new > 1.0 {
            self.economy.debit_item(Flow::Construction, new * BRIDGE_COST_PER_M, Item::Named("bridges".into()));
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
    fn ground_or_water(&self, p: Vec2) -> f32 {
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

    // ── the highway ─────────────────────────────────────────────────────

    /// Lay the through highway: two three-lane carriageways across the map
    /// at `HIGHWAY_Y`, bridging whatever water lies across them. Traffic
    /// drives on the right, so the eastbound carriageway is the north one.
    pub fn lay_highway(&mut self) -> [GestureId; 2] {
        let had_project = self.project.take();
        // Edge to edge of the map; built, both ends run on out of sight
        // (`landscape::reach_elsewhere`): traffic comes from elsewhere.
        let x = crate::landscape::MAP_EDGE;
        let east = self.add_gesture(Gesture::Road { points: vec![Vec2::new(-x, HIGHWAY_Y + HIGHWAY_HALF_GAP), Vec2::new(x, HIGHWAY_Y + HIGHWAY_HALF_GAP)], lanes_fwd: HIGHWAY_LANES, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        let west = self.add_gesture(Gesture::Road { points: vec![Vec2::new(x, HIGHWAY_Y - HIGHWAY_HALF_GAP), Vec2::new(-x, HIGHWAY_Y - HIGHWAY_HALF_GAP)], lanes_fwd: HIGHWAY_LANES, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        self.implement();
        self.project = had_project;
        [east, west]
    }

    /// Join the street end at `from` (south of the highway) to the highway:
    /// a street north to a junction short of it, an off-ramp from the
    /// westbound carriageway and an on-ramp back onto it, built at once.
    /// False when there is no highway there (nothing is laid then). Scripts
    /// and tests use it; the player draws their own ramps.
    pub fn link_to_highway(&mut self, from: Vec2) -> bool {
        let Some(gestures) = self.highway_link(from) else { return false };
        let had_project = self.project.take();
        for g in gestures {
            self.add_gesture(g);
        }
        self.implement();
        self.project = had_project;
        true
    }

    /// The gestures `link_to_highway` lays for a street end at `from`, for
    /// a caller that adds them to a project of its own; None when there is
    /// no highway there or `from` is too close to it.
    pub fn highway_link(&self, from: Vec2) -> Option<Vec<Gesture>> {
        let y = HIGHWAY_Y - HIGHWAY_HALF_GAP;
        let x = from.x;
        let there = self.roads.alive_segments().any(|(id, s)| {
            let (a, b) = self.roads.segment_points(id);
            s.is_highway() && crate::geom::project_on_segment(Vec2::new(x, y), a, b).1.distance(Vec2::new(x, y)) < 1.0
        });
        if !there || from.y > y - 60.0 {
            return None;
        }
        let j = Vec2::new(x, y - 50.0);
        let street = |points: Vec<Vec2>, fwd: u8, bwd: u8| Gesture::Road { points, lanes_fwd: fwd, lanes_bwd: bwd, curved: false, kind: RoadKind::Street, elevation: Vec::new() };
        // Westbound traffic comes from the east: off on that side, back on
        // on the other.
        Some(vec![
            street(vec![from, j], 1, 1),
            street(vec![Vec2::new(x + 220.0, y), Vec2::new(x + 70.0, y - 30.0), j], 1, 0),
            street(vec![j, Vec2::new(x - 70.0, y - 30.0), Vec2::new(x - 220.0, y)], 1, 0),
        ])
    }

    /// Ends of highway carriageways out of sight beyond the map: where
    /// traffic comes in from elsewhere (`entries`) and leaves (`exits`),
    /// with the end segment. A highway stub ending on the map is neither.
    pub fn highway_ends(&self) -> (Vec<(NodeId, SegmentId)>, Vec<(NodeId, SegmentId)>) {
        let mut entries = Vec::new();
        let mut exits = Vec::new();
        for n in self.roads.leaf_nodes() {
            if !crate::landscape::is_elsewhere(self.roads.node(n).pos) {
                continue;
            }
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

    /// Is this car passing through from elsewhere to elsewhere? Only cars
    /// made as through traffic carry `Errand::Through`: a test on "no
    /// driver, no owner, no cargo" also caught fire engines, tow and garbage
    /// trucks and takeover outsiders, which then ate the through-traffic cap
    /// and were re-spawned as through cars on a rebuild while their own
    /// system recovered them too (a duplicate).
    pub fn is_through_car(car: &crate::traffic::Car) -> bool {
        car.errand == crate::vehicle::Errand::Through && car.cargo.is_none() && car.bus.is_none()
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
            self.traffic.cars[id].errand = crate::vehicle::Errand::Through;
            live += 1;
        }
    }

    /// Put a through car back on the highway after the roads were rebuilt
    /// (every car is taken off then): on the lane under it, at the same
    /// place and speed, bound for the far exit. Nothing vanishes on the
    /// highway because the player built a ramp. False when it is not on a
    /// highway any more or cannot get out from there.
    pub(super) fn resume_through_car(&mut self, car: &crate::traffic::Car) -> bool {
        let Some((sid, _, q)) = self.roads.nearest_where(car.pos, |s| s.is_highway()) else { return false };
        if q.distance(car.pos) > 8.0 {
            return false;
        }
        let Some((lane, s, _)) = self.traffic.net.nearest_lane_on(sid, car.pos) else { return false };
        let (_, mut exits) = self.highway_ends();
        exits.sort_by(|a, b| self.roads.node(b.0).pos.distance(car.pos).total_cmp(&self.roads.node(a.0).pos.distance(car.pos)));
        for (en, esid) in exits {
            let end = self.roads.node(en).pos;
            let Some((goal, s1, _)) = self.traffic.net.nearest_lane_on(esid, end) else { continue };
            // On the last piece already: it drives on in its own lane.
            let (goal, s1) = if esid == sid { (lane, self.traffic.net.lanes[lane as usize].project(end).0) } else { (goal, s1) };
            // Already on the last lane at its very end: it leaves from there.
            let s = if goal == lane { s.min(s1 - 0.5).max(0.0) } else { s };
            if let Some((route, s0, s1)) = self.traffic.net.route_lanes(lane, s, goal, s1) {
                let id = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, car.color);
                let c = &mut self.traffic.cars[id];
                c.model = car.model;
                c.errand = crate::vehicle::Errand::Through;
                c.v = car.v;
                c.prev_pos = car.prev_pos;
                c.prev_heading = car.prev_heading;
                return true;
            }
        }
        false
    }

    /// Road ends people and lorries can arrive by: the far ends of ways to
    /// elsewhere (out of sight past the map edge) a car can drive away
    /// from, on a network that reaches the city's lots. Before any lot
    /// exists, every such end. A dead end on the map is never one: nothing
    /// appears or vanishes where the player can see it.
    pub(super) fn city_leaves(&self) -> Vec<NodeId> {
        let open = self.config.dead_ends_lead_elsewhere;
        let leaves: Vec<NodeId> = self.roads.leaf_nodes().into_iter().filter(|&n| open || crate::landscape::is_elsewhere(self.roads.node(n).pos)).collect();
        let comps: std::collections::HashSet<u32> = self.lots.values().filter(|l| self.zones.contains_key(l.zone)).filter_map(|l| self.road_component(l.access_node)).collect();
        let drivable = |n: NodeId| {
            self.roads.node(n).segments.iter().filter(|&&s| self.roads.segment(s).alive).all(|&s| {
                let seg = self.roads.segment(s);
                !seg.is_street() || (seg.a == n && seg.lanes_fwd > 0) || (seg.b == n && seg.lanes_bwd > 0) || seg.lanes() == 0
            })
        };
        let keep: Vec<NodeId> = leaves.iter().copied().filter(|&n| drivable(n) && (comps.is_empty() || self.road_component(n).is_some_and(|c| comps.contains(&c)))).collect();
        if keep.is_empty() && open { leaves } else { keep }
    }

    /// Can newcomers get in: a road from elsewhere (the highway, a road
    /// off the map edge) reaching the city's lots, or a working station?
    pub fn has_way_in(&self) -> bool {
        if !self.stations.is_empty() {
            return true;
        }
        let comps: std::collections::HashSet<u32> = self.lots.values().filter(|l| self.zones.contains_key(l.zone)).filter_map(|l| self.road_component(l.access_node)).collect();
        !comps.is_empty() && !self.city_leaves().is_empty()
    }

    /// [dock-access] The road network a dock or station joins: the road
    /// components that hold zoned lots (a city at the other end) or reach
    /// off the map (the highway, a road out to elsewhere). A street in an
    /// empty field is neither.
    pub(super) fn network_components(&self) -> std::collections::HashSet<u32> {
        let mut comps: std::collections::HashSet<u32> = self.lots.values().filter(|l| self.zones.contains_key(l.zone) && l.zone != ZoneId::default()).filter_map(|l| self.road_component(l.access_node)).collect();
        comps.extend(self.roads.leaf_nodes().into_iter().filter(|&n| crate::landscape::is_elsewhere(self.roads.node(n).pos)).filter_map(|n| self.road_component(n)));
        comps
    }

    /// [dock-access] For a dock, station or yard: whether its street
    /// reaches the road network (`network_components`). None for any other
    /// building.
    pub fn road_access(&self, b: BuildingId) -> Option<bool> {
        if let Some(p) = self.ports.port_of(b) {
            return Some(p.road_connected);
        }
        self.buildings.get(b).and_then(|x| x.rail.as_ref()).map(|l| l.road_connected)
    }

    /// [dock-access] What the player is told about a dock or station off
    /// the roads, for the inspector, the alerts and the API.
    pub fn access_problem(&self, b: BuildingId) -> Option<String> {
        if self.road_access(b)? {
            return None;
        }
        let joined = self.stub_ends_of(b).is_empty();
        let what = if self.ports.port_of(b).is_some_and(|p| p.kind == PortKind::FerryTerminal) {
            "ferries sail, but carry no one until people can get to the pier"
        } else if self.ports.port_of(b).is_some() {
            "nothing lands or is sold until lorries can reach it"
        } else if self.buildings.get(b).and_then(|x| x.rail.as_ref()).is_some_and(|l| l.kind.is_passenger() && l.rail_connected) {
            "trains call, but nobody gets on or off until it is on the roads"
        } else {
            "it opens once it is on the roads"
        };
        Some(if joined {
            format!("not connected to the road network: its street does not reach the city or a way out of town yet; {what}")
        } else {
            format!("not connected to the road network: draw a road to the marked end of its street stub; {what}")
        })
    }

    /// [dock-access] The free ends of a dock's or station's street stub
    /// that a road can be drawn onto: dead ends of the street it laid (a
    /// dock's access road: its far end only). Empty once joined.
    pub fn stub_ends_of(&self, b: BuildingId) -> Vec<Vec2> {
        let ends: Vec<Vec2> = if let Some(p) = self.ports.port_of(b) {
            match p.stub {
                Some(s) => vec![s[1]],
                None => p.front_line().to_vec(),
            }
        } else if let Some(f) = self.facilities.values().find(|f| f.building == Some(b)) {
            f.road_line().to_vec()
        } else {
            return Vec::new();
        };
        ends.into_iter()
            .filter_map(|e| self.roads.node_near(e, 2.5))
            .filter(|&n| self.roads.node(n).segments.iter().filter(|&&s| self.roads.segment(s).alive).count() == 1)
            .map(|n| self.roads.node(n).pos)
            .collect()
    }

    /// [dock-access] Docks and stations off the roads with the free ends
    /// of their stubs, for the "connect here" markers.
    pub fn unjoined_stubs(&self) -> Vec<(BuildingId, Vec<Vec2>)> {
        let docks = self.ports.ports.values().filter(|p| !p.road_connected).filter_map(|p| p.building);
        let rail = self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| !l.road_connected)).map(|(id, _)| id);
        docks.chain(rail).filter(|b| self.buildings.contains_key(*b)).map(|b| (b, self.stub_ends_of(b))).collect()
    }

    /// [dock-access] The Build tool's verdict for a dock or a rail piece at
    /// `p`: what placing it there would do (`Ok`), or why it cannot go
    /// (`Err`). None for anything else. Neither needs a road: a dock needs
    /// the water's edge, a station or yard nothing but room; what is not
    /// there yet is laid as stubs to join later.
    pub fn piece_verdict(&self, what: crate::services::Placeable, p: Vec2) -> Option<Result<String, String>> {
        use crate::services::Placeable;
        let cost = what.cost();
        let afford = |v: Result<String, String>| if self.economy.treasury < cost { Err(format!("the treasury has {:.0}; it costs {:.0}", self.economy.treasury, cost)) } else { v };
        match what {
            Placeable::Dock(kind) => Some(afford(self.plan_port(kind, p).map(|port| {
                let probe = port.front_mid() - port.seaward * rail::ROAD_GAP;
                if self.roads.nearest_street(probe).is_some_and(|(_, _, q)| q.distance(probe) <= rail::ROAD_EDGE_FUSE) {
                    "on the water's edge, fronting the street here".to_string()
                } else {
                    let ferry = if kind == PortKind::FerryTerminal { " (its ferries sail meanwhile, carrying no one)" } else { "" };
                    format!("on the water's edge; no road needed: it lays a short access road inland. Draw a road to its marked end to put it on the road network{ferry}")
                }
            }))),
            Placeable::Rail(kind) => {
                let (f, rail_fused, road_fused) = rail::Facility::plan(kind, p, &self.roads);
                Some(afford(match self.rail_piece_problem(&f) {
                    Some(why) => Err(why),
                    None => Ok(match (rail_fused, road_fused) {
                        (true, true) => "on the track, fronting the street".to_string(),
                        (true, false) => "on the track; no road needed: it lays a street stub. Draw a road to its marked end to connect it (trains call meanwhile; nobody gets on or off)".to_string(),
                        (false, true) => "by the street; it lays a rail stub: lay track onto it".to_string(),
                        (false, false) => "in open ground; it lays a rail stub and a street stub to join to the track and the roads".to_string(),
                    }),
                }))
            }
            _ => None,
        }
    }

    // ── docks ───────────────────────────────────────────────────────────

    /// Where a dock of `kind` would go for a click at `p`, or why not.
    pub fn plan_port(&self, kind: PortKind, p: Vec2) -> Result<Port, String> {
        let port = Port::plan(kind, p, &self.landscape)?;
        // [ferries] A ferry terminal's boats run to towns off the map, so
        // its water must reach the edge (a lake that does not is no good).
        if kind == PortKind::FerryTerminal && self.landscape.water_routes_out(port.berth).is_empty() {
            return Err("ferries need water that reaches the edge of the map: build the terminal on a river or the sea".into());
        }
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
        self.economy.debit_item(Flow::Construction, kind.cost(), Item::Named(kind.label().to_string()));
        let probe = port.front_mid() - port.seaward * rail::ROAD_GAP;
        let fused = self.roads.nearest_street(probe).is_some_and(|(_, _, q)| q.distance(probe) <= rail::ROAD_EDGE_FUSE);
        // [dock-access] No road needed: a dock not set down by a street gets
        // a short access road of its own from its gate inland, a dead end
        // the player draws a road onto to join it to the network.
        let mut port = port;
        if !fused {
            port.stub = Some(self.dock_stub(&port));
        }
        let stub = port.stub;
        let pid = self.ports.ports.insert(port);
        let had_project = self.project.take();
        if let Some(stub) = stub {
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
                // [economy]
                PortKind::MarinaSmall => format!("{n} Marina"),
                PortKind::MarinaLarge => format!("{n} Yacht Club"),
                PortKind::FishingPortSmall => format!("{n} Fish Quay"),
                PortKind::FishingPortLarge => format!("{n} Fishing Harbour"),
            }
        };
        let id = self.place_special(lot_id, name, kind.jobs(), kind.jobs() / 3, |b| b.port = Some(kind));
        // [dock-access] A dock stands on its own footprint and is drawn as
        // itself from the start: what it bought out is cleared at once,
        // never shown as the old building (in the zone's lot shape) coming down.
        self.buildings[id].demolition = None;
        // [economy] A marina or fishing port is run by an operator on the
        // city's quay (`world/waterfront.rs`).
        self.lease_dock(id, kind, false);
        self.ports.ports[pid].building = Some(id);
        // [ferries] Its towns off the map, across the edges its water reaches.
        if kind == PortKind::FerryTerminal {
            self.link_remote_terminals(pid);
        }
        self.ports.ports[pid].last_sailing = self.clock.tick.saturating_sub(ports::FREIGHT_HEADWAY);
        self.refresh_port_links();
        self.events.push(SimEvent::LotsChanged);
        Ok(id)
    }

    /// [dock-access] The access road for a dock: from just off the middle
    /// of its landward side, straight inland if that is clear, else turned
    /// up to 60 degrees either way, else shorter. Clear means dry ground,
    /// not too steep, off every other dock's and station's footprint and
    /// off the lots of standing buildings (all but those the dock takes
    /// over). Falls back to the shortest straight one.
    pub fn dock_stub(&self, port: &Port) -> [Vec2; 2] {
        let gate = port.front_mid() - port.seaward * ports::STUB_GATE;
        let inland = -port.seaward;
        let own = port.footprint();
        let blocked_lots: Vec<Vec<Vec2>> = self
            .buildings
            .values()
            .filter_map(|b| self.lots.get(b.lot))
            .filter(|l| !(point_in_polygon(l.center, &own) || polygons_overlap(&own, &l.polygon)))
            .map(|l| l.polygon.clone())
            .collect();
        let others: Vec<Vec<Vec2>> = self.facilities.values().map(|f| f.footprint()).chain(self.ports.ports.values().map(|p| p.footprint())).collect();
        let clear = |end: Vec2| {
            let n = (gate.distance(end) / 3.0).ceil().max(1.0) as usize;
            (0..=n).all(|i| {
                let q = gate.lerp(end, i as f32 / n as f32);
                !self.landscape.is_water(q) && !others.iter().any(|fp| point_in_polygon(q, fp)) && !blocked_lots.iter().any(|lp| point_in_polygon(q, lp)) && (i == 0 || !point_in_polygon(q, &own))
            }) && self.landscape.slope_over(&[gate, end]) <= crate::landscape::MAX_LOT_SLOPE * 1.5
        };
        let turn = |deg: f32| {
            let (s, c) = deg.to_radians().sin_cos();
            Vec2::new(inland.x * c - inland.y * s, inland.x * s + inland.y * c)
        };
        for len in [ports::STUB_LENGTH, ports::STUB_LENGTH * 0.7, ports::STUB_MIN] {
            for deg in [0.0f32, 25.0, -25.0, 45.0, -45.0, 60.0, -60.0] {
                let end = gate + turn(deg) * len;
                if clear(end) {
                    return [gate, end];
                }
            }
        }
        [gate, gate + inland * ports::STUB_MIN]
    }

    /// [dock-access] Pull down a dock and its pier: its ships and the goods
    /// and people waiting there go, and the zoned lots under its footprint
    /// come back from the plan. Its street stays.
    pub(super) fn bulldoze_dock(&mut self, bid: BuildingId) -> Result<String, String> {
        let pid = self.ports.ports.iter().find(|(_, p)| p.building == Some(bid)).map(|(id, _)| id).ok_or("no such dock")?;
        let name = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        self.ports.ports.remove(pid);
        self.ports.ships.retain(|_, s| s.port != bid && s.to != Some(bid));
        let stranded: Vec<Cargo> = self.ports.inbound.iter().filter(|(p, _)| *p == bid).map(|(_, c)| *c).collect();
        self.ports.inbound.retain(|(p, _)| *p != bid);
        self.ports.boarding.remove(&bid);
        self.demolish(bid);
        let had_project = self.project.take();
        self.rebuild_from_master();
        self.project = had_project;
        for c in stranded {
            self.dispatch_truck(super::market::TruckOrigin::CityEdge, c, 6);
        }
        self.events.push(SimEvent::LotsChanged);
        Ok(format!("{name} bulldozed with its pier"))
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
            land_use: p.kind.land_use(),
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
        let comps = self.network_components();
        let ids: Vec<PortId> = self.ports.ports.keys().collect();
        for id in ids {
            let (lot, berth) = (self.ports.ports[id].lot, self.ports.ports[id].berth);
            let road = lot.and_then(|l| self.lots.get(l)).and_then(|l| self.road_component(l.access_node)).is_some_and(|c| comps.contains(&c));
            let out = self.landscape.water_route(berth, None).is_some();
            let p = &mut self.ports.ports[id];
            p.road_connected = road;
            p.reaches_elsewhere = out;
            // [ferries] A terminal from a save before remote towns gets its own.
            if p.kind == PortKind::FerryTerminal && p.remote.is_empty() && out {
                self.link_remote_terminals(id);
            }
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
        let mut returned = Vec::new();
        for id in ids {
            let ship = &mut self.ports.ships[id];
            ship.prev_s = ship.s;
            if ship.dwell > 0 {
                ship.dwell -= 1;
                continue;
            }
            let total = crate::geom::path_length(&ship.path);
            if ship.home {
                // [ferries] Unloaded at its own pier after a run: done.
                done.push(id);
                continue;
            }
            // [ferries] Out of sight beyond the map edge the run is hurried
            // along, so the next town is hours away, not half a day.
            let off_map = ship.remote.is_some() && {
                let p = ship.pose().0;
                p.x.abs().max(p.y.abs()) > crate::landscape::MAP_EDGE + 400.0
            };
            ship.s += ship.speed * if off_map { ports::OFF_MAP_SPEEDUP } else { 1.0 };
            if ship.remote.is_some() && ship.docked && ship.s >= total {
                // Back from the town off the map: tie up and let them off.
                ship.s = total;
                ship.home = true;
                ship.dwell = ports::FERRY_UNLOAD;
                returned.push(id);
                continue;
            }
            if !ship.docked && ship.s >= ship.berth_s {
                ship.s = ship.berth_s;
                ship.docked = true;
                ship.dwell = match ship.kind {
                    ShipKind::Freighter => ports::FREIGHTER_DWELL,
                    ShipKind::Ferry => ports::FERRY_DWELL,
                    // [economy] out on the fishing ground, or at anchor
                    ShipKind::FishingBoat | ShipKind::Yacht | ShipKind::Sailboat => crate::world::waterfront::boat_dwell(ship.kind),
                };
                docked.push(id);
            } else if ship.s >= total {
                done.push(id);
            }
        }
        for id in docked {
            self.ship_docked(id);
        }
        for id in returned {
            let (port, town, passengers) = {
                let s = &self.ports.ships[id];
                (s.port, s.remote.unwrap_or(0), s.passengers)
            };
            let _ = passengers;
            self.ferry_returned(port, town);
        }
        for id in done {
            if let Some(ship) = self.ports.ships.remove(id) {
                // [economy] A trawler back at the quay lands its catch.
                self.boat_home(&ship);
            }
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
        // [economy] Marina and fishing boats do not count against the ships.
        let big_ships = self.ports.ships.values().filter(|s| matches!(s.kind, ShipKind::Freighter | ShipKind::Ferry)).count();
        for (pid, port) in &ports_now {
            if !matches!(port.kind, PortKind::ShippingPort | PortKind::FerryTerminal) {
                continue;
            }
            // [dock-access] A ferry terminal's boats run on the water alone.
            if !port.sails() || big_ships >= ports::MAX_SHIPS {
                continue;
            }
            let Some(b) = port.building else { continue };
            if self.buildings.get(b).is_none_or(|x| x.is_derelict()) {
                continue;
            }
            // [ferries] A terminal runs two boats, one away while the next
            // sails, but never two at its pier at once.
            let mine: Vec<&Ship> = self.ports.ships.values().filter(|s| s.port == b).collect();
            let at_pier = mine.iter().any(|s| s.pose().0.distance(port.berth) < 40.0);
            let busy = match port.kind {
                PortKind::ShippingPort => !mine.is_empty(),
                PortKind::FerryTerminal => at_pier || mine.len() >= ports::FERRIES_PER_TERMINAL,
                _ => true,
            };
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
                    self.ports.ships.insert(Ship { kind: ShipKind::Freighter, path, s: 0.0, prev_s: 0.0, speed, dwell: 0, port: b, to: None, cargo, passengers: 0, docked: false, berth_s, color, remote: None, home: false });
                    self.ports.ports[*pid].last_sailing = tick;
                }
                // [economy] Boats are sent by `World::step_boats`.
                PortKind::MarinaSmall | PortKind::MarinaLarge | PortKind::FishingPortSmall | PortKind::FishingPortLarge => {}
                PortKind::FerryTerminal => {
                    if !(ports::FERRY_SERVICE_START..ports::FERRY_SERVICE_END).contains(&minute) || tick.saturating_sub(port.last_sailing) < ports::FERRY_HEADWAY {
                        continue;
                    }
                    // [ferries] Take turns: a run to a town off the map,
                    // then one to the next terminal in town, and so on;
                    // with only one kind, that kind every time.
                    let others: Vec<(BuildingId, Vec2)> = ports_now.iter().filter(|(oid, o)| oid != pid && o.kind == PortKind::FerryTerminal && o.sails()).filter_map(|(_, o)| o.building.map(|ob| (ob, o.berth))).collect();
                    let turn = port.sailings as usize;
                    let remote_turn = !port.remote.is_empty() && (others.is_empty() || turn % 2 == 0);
                    let mut chosen: Option<(Option<BuildingId>, Option<u8>, Vec<Vec2>)> = None;
                    if remote_turn {
                        let k = (turn / 2) % port.remote.len();
                        let edge = port.remote[k].edge;
                        chosen = self.landscape.water_routes_out(port.berth).into_iter().find(|(e, _)| *e == edge).map(|(_, r)| (None, Some(k as u8), r));
                    }
                    if chosen.is_none() {
                        let start = turn / 2;
                        for k in 0..others.len() {
                            let (ob, ob_berth) = others[(start + k) % others.len()];
                            if let Some(route) = self.landscape.water_route(port.berth, Some(ob_berth)) {
                                chosen = Some((Some(ob), None, route));
                                break;
                            }
                        }
                    }
                    let Some((to, remote, route)) = chosen else { continue };
                    let berth_s = crate::geom::path_length(&route);
                    let mut path = route.clone();
                    path.extend(route.iter().rev().skip(1).copied());
                    let speed = self.config.metres_per_tick(ports::FERRY_SPEED);
                    // Locals hopping across town, and on a run off the map
                    // whoever is waiting at the pier to leave.
                    // [dock-access] Nobody can get to or away from a pier
                    // whose terminal is off the roads: those boats sail empty.
                    let reachable = |x: BuildingId| ports_now.iter().any(|(_, o)| o.building == Some(x) && o.works());
                    let mut riders = if reachable(b) && to.is_none_or(reachable) { self.ferry_riders(b, to) } else { 0 };
                    if remote.is_some() {
                        let leaving = self.ports.boarding.remove(&b).unwrap_or(0);
                        riders += leaving;
                        self.ports.stats.remote_sailings_today += 1;
                    }
                    let riders = riders.min(ports::FERRY_CAPACITY);
                    let fares = riders as f32 * ports::FERRY_FARE;
                    // Ferry fares are public transport income, booked with the bus fares.
                    self.economy.credit_item(Flow::BusFares, fares, Item::Building(b));
                    self.ports.stats.ferry_riders_today += riders;
                    self.ports.stats.ferry_fares_today += fares;
                    self.ports.count_line_riders(b, to, remote, riders);
                    self.ports.ships.insert(Ship { kind: ShipKind::Ferry, path, s: 0.0, prev_s: 0.0, speed, dwell: ports::FERRY_DWELL, port: b, to, cargo: Vec::new(), passengers: riders, docked: false, berth_s, color: 0, remote, home: false });
                    self.ports.ports[*pid].last_sailing = tick;
                    self.ports.ports[*pid].sailings += 1;
                }
            }
        }
    }

    /// People living near either end of a sailing who take the ferry.
    fn ferry_riders(&mut self, from: BuildingId, to: Option<BuildingId>) -> u32 {
        let ends: Vec<Vec2> = [Some(from), to].into_iter().flatten().filter_map(|b| self.building_entrance(b)).collect();
        let near = self.citizens.values().filter(|c| c.home.and_then(|h| self.building_entrance(h)).is_some_and(|p| ends.iter().any(|e| e.distance(p) < 500.0))).count() as u32;
        // Out to elsewhere draws a few more locals (visitors, commuters
        // and day trippers are counted as they board).
        let base = if to.is_none() { 4 } else { 2 };
        (near / 12 + base + self.rng.random_range(0..6)).min(ports::FERRY_CAPACITY)
    }

    fn ship_docked(&mut self, id: crate::ports::ShipId) {
        let Some(ship) = self.ports.ships.get_mut(id) else { return };
        if ship.kind == ShipKind::Freighter {
            let port = ship.port;
            let cargo = std::mem::take(&mut ship.cargo);
            self.ports.stats.freight_landed_today += cargo.len() as u32;
            for c in cargo {
                // Into a cargo hub on the quay, else a lorry.
                self.unload_freight(port, c);
            }
        }
    }

    /// Daily books for the docks: upkeep and fresh counters.
    pub(super) fn daily_ports(&mut self) {
        let docks: Vec<(Option<BuildingId>, PortKind, f32)> = self.ports.ports.values().map(|p| (p.building, p.kind, p.kind.upkeep_per_day())).collect();
        for (b, kind, upkeep) in docks {
            let item = b.map_or_else(|| Item::Named(kind.label().to_string()), Item::Building);
            self.economy.debit_item(Flow::TransitUpkeep, upkeep, item);
        }
        let s = &mut self.ports.stats;
        s.freight_landed_today = 0;
        s.ferry_riders_today = 0;
        s.ferry_fares_today = 0.0;
        s.remote_sailings_today = 0;
        s.visitors_in_today = 0;
        s.visitors_out_today = 0;
        s.commuters_in_today = 0;
        s.commuters_out_today = 0;
        s.day_trips_today = 0;
        s.day_trippers_back_today = 0;
        self.ports.commuters_today.clear();
        self.ports.trips_today.clear();
        self.ports.line_riders.clear();
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
