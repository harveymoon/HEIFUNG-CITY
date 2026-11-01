//! [cyclists] Bikes and the rules of the road. A cyclist rides the road
//! shoulder (`route_path`), so along a street they are traffic: at a
//! signalised junction they stop before the crosswalk while their approach
//! has red, and anywhere they give way to people on a crossing they are
//! about to ride over. Where a bike leaves a footpath straight across a
//! street it crosses like a walker: the walk light, or a gap in traffic at
//! a zebra, and drivers stop for it while it is on the zebra.
//!
//! Temperament works as for walkers (`pedestrians::jaywalk_chance`): the
//! rebellious, the drunk and the late sometimes run a red when the cross
//! traffic leaves a gap they like; the cautious, the old and children never
//! do. Decisions draw from `hash01`, never the world's random stream.
//!
//! The crossings a rider meets are found geometrically from the walk
//! graph's crossings (`Pedestrians::cross_grid`), so the bike's path needs
//! no extra state and a reload changes nothing but who is mid-wait.

use super::pedestrians::Feet;
use super::*;
use crate::citizens::driver_traits;
use crate::pedestrians::{gap_needed, hash01, jaywalk_chance, Rider, LAWFUL_GAP};
use crate::walk::{CrossingKind, CROSSWALK_HALF};

/// How far beyond this tick's ride a cyclist looks for a crossing, metres.
const LOOK_AHEAD: f32 = 6.0;
/// A cyclist waiting at a red or for a walker stops this far short of the
/// zebra's near edge.
const STOP_SHORT: f32 = 1.0;
/// Riding along a street when the bike's heading is within about 45
/// degrees of it; across it when more than 60 degrees off.
const ALONG: f32 = 0.7;
const ACROSS: f32 = 0.5;
/// [r1-roads] Most metres more than the street's width a ride round a ring
/// of footbridge decks may take (`bike_over_footbridges`).
const RING_DETOUR: f32 = 90.0;
/// [r4-hunger] Ticks a rider gives way to a zebra that never empties
/// before wheeling through among the walkers.
const ZEBRA_PATIENCE: u64 = 5;
/// A bold rider rolls the dice on a red once per this many ticks.
const RED_ROLL_TICKS: u64 = 5;
/// Salt of a rider's red-light dice, apart from a walker's.
const RED_SALT: u64 = 0xb1cc;

/// What a rider found ahead: stop `at` metres along the ride, waiting at
/// crossing `cr`.
struct Stop {
    at: f32,
    cr: u32,
}

/// `p` hit along the ray `o + d * t`, `t` in `[0, reach]`, by the segment
/// `a..b` (with a little slack at its ends): `t` if so.
fn ray_hits(o: Vec2, d: Vec2, reach: f32, a: Vec2, b: Vec2) -> Option<f32> {
    let e = b - a;
    let denom = d.perp_dot(e);
    if denom.abs() < 1e-6 {
        return None;
    }
    let w = a - o;
    let t = w.perp_dot(e) / denom;
    let u = w.perp_dot(d) / denom;
    (t >= -CROSSWALK_HALF - 0.5 && t <= reach && (-0.15..=1.15).contains(&u)).then_some(t)
}

impl World {
    /// [walk-crossings] A bike's path that goes straight over a street
    /// where a footbridge replaced the crossing goes over the bridge
    /// instead: up the stairs at the near end (wheeling the bike, at a
    /// walk: `step_rider`), over the deck and down the far side. The bridge's
    /// points are the walk graph's own, so the rider is drawn up there.
    /// [r1-roads] A bridge joined at a corner deck to the next one (a ring
    /// of decks round a junction, no stairs at that end) is ridden as the
    /// walkers go: over the ring and down the nearest stairs, by the walk
    /// graph (it used to be left out, and the bikes crossed at grade under
    /// it). A path already over a bridge is left as it is, so this may run
    /// again on the rest of a ride (`step_rider`, after the bridges change).
    pub(super) fn bike_over_footbridges(&self, path: &mut Vec<Vec2>) {
        if self.walk.footbridges.is_empty() || path.len() < 2 {
            return;
        }
        // Arc length at each vertex.
        let mut arc = vec![0.0f32; path.len()];
        for i in 1..path.len() {
            arc[i] = arc[i - 1] + path[i - 1].distance(path[i]);
        }
        // (arc position, half the street's width with its pavements, the
        // bridge's points in the order ridden)
        let mut hits: Vec<(f32, f32, Vec<Vec2>)> = Vec::new();
        // [r5-fixes] The point on the leg ridden `p`..`q` abreast of `x`.
        let abreast = |x: Vec2, p: Vec2, q: Vec2| crate::geom::project_on_segment(x, p, q).1;
        for f in &self.walk.footbridges {
            if path.iter().any(|p| f.top.contains(p)) {
                continue;
            }
            let Some(c) = self.walk.crossings.get(f.crossing as usize) else { continue };
            if !c.closed || self.roads.try_segment(c.segment).is_none() {
                continue;
            }
            let (s0, s1) = self.roads.segment_points(c.segment);
            let sdir = (s1 - s0).normalize_or_zero();
            let (a, b) = (self.walk.nodes[c.a as usize].pos, self.walk.nodes[c.b as usize].pos);
            let half = a.distance(b) * 0.5 + 1.0;
            for i in 1..path.len() {
                let (p, q) = (path[i - 1], path[i]);
                let len = p.distance(q);
                if len < 0.3 {
                    continue;
                }
                let d = (q - p) / len;
                if d.dot(sdir).abs() > ACROSS {
                    continue;
                }
                let (t, at) = crate::geom::project_on_segment(c.mid, p, q);
                if at.distance(c.mid) > 3.0 {
                    continue;
                }
                let s = arc[i - 1] + t * len;
                if hits.iter().any(|h| (h.0 - s).abs() < half) {
                    break;
                }
                // The end on the side the bike comes from first.
                let near_a = p.distance(a) <= p.distance(b);
                let pts = if f.joined[0] || f.joined[1] {
                    // Round the ring of decks as a walker goes, from this
                    // kerb to the other: never more than a short way round.
                    let (from, to) = if near_a { (c.a, c.b) } else { (c.b, c.a) };
                    let Some(nodes) = self.walk.route_nodes(from, to) else { break };
                    let pts: Vec<Vec2> = nodes.iter().map(|&n| self.walk.nodes[n as usize].pos).collect();
                    let walked: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
                    let raised = nodes.iter().any(|&n| self.walk.nodes[n as usize].level > 0.0);
                    if !raised || walked > half * 8.0 + RING_DETOUR {
                        break;
                    }
                    pts
                } else if near_a {
                    // [r5-stairs] Up and down every point of the stairs
                    // (a switchback's landing, a spiral's turns).
                    f.over_from(0)
                } else {
                    f.over_from(1)
                };
                // [r5-fixes] Harvey's flag 2 (Delacroix Boulevard): the
                // ride goes on along its own leg to abreast of the bridge's
                // first foot and steps across to it there, and back onto
                // the leg abreast of its last. Cut from the leg's far start
                // straight to the foot, a 235 m leg along the boulevard
                // became a diagonal 15-20 m out over the grass.
                let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { break };
                let mut ride = Vec::with_capacity(pts.len() + 2);
                let on = abreast(first, p, q);
                if on.distance(first) > 1.0 && on.distance(p) > 1.0 {
                    ride.push(on);
                }
                ride.extend_from_slice(&pts);
                let off = abreast(last, p, q);
                if off.distance(last) > 1.0 && off.distance(q) > 1.0 {
                    ride.push(off);
                }
                hits.push((s, half, ride));
                break;
            }
        }
        if hits.is_empty() {
            return;
        }
        hits.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out: Vec<Vec2> = Vec::with_capacity(path.len() + hits.len() * 4);
        let mut h = 0;
        for (i, &v) in path.iter().enumerate() {
            // Bridges before this vertex (and the street under them) first.
            while h < hits.len() && hits[h].0 - hits[h].1 <= arc[i] {
                out.extend_from_slice(&hits[h].2);
                h += 1;
            }
            // A vertex out in the street a bridge spans is not ridden.
            if hits[..h].iter().any(|x| (arc[i] - x.0).abs() < x.1) && i + 1 < path.len() && i > 0 {
                continue;
            }
            out.push(v);
        }
        for x in &hits[h..] {
            out.extend_from_slice(&x.2);
        }
        *path = out;
    }

    /// Do vehicles coming in on `approach` of intersection `k` have the
    /// green to go straight or right (a left arrow alone is not a green for
    /// a bike going on)? Read from the connector lanes the signals set.
    pub(super) fn approach_green(&self, k: usize, approach: usize) -> bool {
        let Some(inter) = self.traffic.net.intersections.get(k) else { return true };
        if !inter.signalized || inter.phases.is_empty() {
            return true;
        }
        let mut any = false;
        for &(lane, ap) in &inter.connectors {
            if ap != approach {
                continue;
            }
            let l = &self.traffic.net.lanes[lane as usize];
            if l.turn >= 0 {
                any = true;
                if l.green {
                    return true;
                }
            }
        }
        // An approach with no straight or right movement: follow its
        // phase rather than wait forever.
        !any && inter.phases.get(inter.phase).is_some_and(|g| g.contains(&approach))
    }

    /// [r4-hunger] No light faces `approach` of intersection `k`: no lane
    /// comes into the junction on it (a one-way street out of it) and no
    /// phase serves it, so it is never green. A rider coming in that way
    /// (on the pavement, against the one-way) waited for that green for
    /// good: 2,200 cyclists stood at one such T on Harvey's day-62 city,
    /// starved and left.
    pub(super) fn approach_unlit(&self, k: usize, approach: usize) -> bool {
        let Some(inter) = self.traffic.net.intersections.get(k) else { return false };
        inter.signalized && !inter.phases.is_empty() && !inter.phases.iter().any(|g| g.contains(&approach)) && !inter.connectors.iter().any(|&(_, ap)| ap == approach)
    }

    /// Someone on foot on (or stepping onto) crossing `cr` last tick.
    ///
    /// [walk-networks] Only walkers: a rider gives way to people on a
    /// crossing, not to other bikes. Counting riders locked a roundabout in
    /// Harvey's city for days: bikes riding round the ring stood in the
    /// mouth of an arm (on its crosswalk, so they held it) giving way to
    /// bikes standing on the ring's crossing, who in turn gave way to them;
    /// hundreds piled up at three spots and the cars (sirens too) stopped
    /// for the bikes "in the road" for good.
    fn crossing_taken(&self, cr: u32, id: CitizenId) -> bool {
        let occ = &self.peds.occupants;
        let from = occ.partition_point(|x| x.0 < cr);
        occ[from..].iter().take_while(|x| x.0 == cr).any(|x| x.1 != id && self.citizens.get(x.1).is_some_and(|c| c.mode == TravelMode::Walk))
    }

    /// Seconds before cross traffic reaches junction `node`: the nearest
    /// moving vehicle over the crosswalks of every arm but `arm`'s own.
    fn cross_traffic_gap(&self, node: Option<NodeId>, arm: SegmentId) -> f32 {
        let Some(node) = node else { return f32::INFINITY };
        let mut gap = f32::INFINITY;
        let at = self.roads.node(node).pos;
        for k in self.peds.cross_grid.in_box(at - Vec2::splat(40.0), at + Vec2::splat(40.0)) {
            let k = k as usize;
            let Some(c) = self.walk.crossings.get(k) else { continue };
            if c.node == Some(node) && c.segment != arm {
                if let Some(info) = self.peds.crossings.get(k) {
                    for &(l, s) in &info.lanes {
                        gap = gap.min(self.traffic.time_to_crossing(l, s, CROSSWALK_HALF * 2.0 + 0.6));
                    }
                }
            }
        }
        gap
    }

    /// How likely this rider is to run a light right now.
    fn rider_boldness(&self, id: CitizenId, r: &Rider, late: bool) -> f32 {
        let c = &self.citizens[id];
        jaywalk_chance(c.age, r.caution, r.hedonism, c.drunk, late)
    }

    /// The first crossing ahead of a rider at `pos` heading `dir` (within
    /// `reach`) that they must stop for, if any; crossings they ride over
    /// lawfully as a walker would are claimed in `feet` so drivers stop.
    ///
    /// [bridge-riders] Only the crossings of streets at the rider's own
    /// level (`leg`, the leg ridden, says how high that is): riding an
    /// elevated path or a viaduct over a street, its crossings are nothing
    /// to the rider, nor the rider anything to its drivers.
    #[allow(clippy::too_many_arguments)]
    fn rider_stop(&mut self, id: CitizenId, r: &mut Rider, pos: Vec2, dir: Vec2, reach: f32, late: bool, leg: (Vec2, Vec2), feet: &mut Feet) -> Option<Stop> {
        let tick = self.clock.tick;
        let seed = self.citizens[id].seed;
        let end = pos + dir * reach;
        // A crossing's box spans the street between its kerbs, so a ray that
        // meets it (riding along) or runs down it (riding across) comes
        // within a few metres of the box.
        let pad = Vec2::splat(3.5);
        let mut near = self.peds.cross_grid.in_box(pos.min(end) - pad, pos.max(end) + pad);
        if !near.is_empty() {
            if let Some(z) = self.leg_way_height(leg.0, leg.1, pos) {
                near.retain(|&cr| !self.crossing_apart_from(cr, z));
            }
        }
        // [review] Per junction, the crosswalk best aligned with the ride
        // is the one over the arm the rider comes in on: only its light
        // is theirs. At a five-arm junction a neighbouring arm's crosswalk
        // can lie within 45 degrees of the ride too; obeying both lights
        // (never green together) held every rider there for good.
        let mut own_arm: Vec<(NodeId, f32)> = Vec::new();
        for &cr in &near {
            let Some(crossing) = self.walk.crossings.get(cr as usize) else { continue };
            let Some(n) = crossing.node else { continue };
            if crossing.kind == CrossingKind::Jay || self.roads.segments.len() <= crossing.segment.0 as usize {
                continue;
            }
            let (s0, s1) = self.roads.segment_points(crossing.segment);
            let along = dir.dot((s1 - s0).normalize_or_zero()).abs();
            let (a, b) = (self.walk.nodes[crossing.a as usize].pos, self.walk.nodes[crossing.b as usize].pos);
            if along < ALONG || ray_hits(pos, dir, reach, a, b).is_none() {
                continue;
            }
            match own_arm.iter_mut().find(|(m, _)| *m == n) {
                Some(e) => e.1 = e.1.max(along),
                None => own_arm.push((n, along)),
            }
        }
        let mut best: Option<Stop> = None;
        for cr in near {
            let Some(crossing) = self.walk.crossings.get(cr as usize) else { continue };
            if crossing.kind == CrossingKind::Jay || self.roads.segments.len() <= crossing.segment.0 as usize {
                continue;
            }
            let (s0, s1) = self.roads.segment_points(crossing.segment);
            let sdir = (s1 - s0).normalize_or_zero();
            let along = dir.dot(sdir).abs();
            let (a, b) = (self.walk.nodes[crossing.a as usize].pos, self.walk.nodes[crossing.b as usize].pos);
            let signal = self.peds.crossings.get(cr as usize).and_then(|i| i.signal);
            let stop = if along >= ALONG {
                // Riding along the street over this crosswalk.
                let Some(t) = ray_hits(pos, dir, reach, a, b) else { continue };
                let near_edge = t - CROSSWALK_HALF;
                if near_edge < -0.2 {
                    // Already on it: ride on.
                    continue;
                }
                let at = (near_edge - STOP_SHORT).max(0.0);
                // Into the junction, with lights: the approach's signal.
                let entering = crossing.node.is_some_and(|n| (self.roads.node(n).pos - crossing.mid).dot(dir) > 0.0);
                let own = crossing.node.is_none_or(|n| own_arm.iter().any(|(m, best)| *m == n && along >= *best));
                let entering = entering && own;
                // [r4-hunger] In on an arm no light faces: no green comes,
                // so in a gap in the cross traffic, as where there are no
                // lights.
                let unlit = entering && signal.is_some_and(|(k, ap)| self.approach_unlit(k as usize, ap as usize));
                let red = match signal {
                    Some((k, ap)) if entering && !unlit && !self.approach_green(k as usize, ap as usize) => Some((k, ap)),
                    _ => None,
                };
                if unlit && self.cross_traffic_gap(crossing.node, crossing.segment) < gap_needed(r.caution, late) {
                    Some(Stop { at, cr })
                } else if let Some((k, ap)) = red {
                    // Call the bike's phase: a detector would see it.
                    if let Some(inter) = self.traffic.net.intersections.get(k as usize) {
                        if let Some(g) = inter.phases.get(inter.phase).filter(|g| !g.contains(&(ap as usize))) {
                            if let Some(&other) = g.first() {
                                if let Some(x) = feet.calls.get_mut(k as usize).and_then(|v| v.get_mut(other)) {
                                    *x = true;
                                }
                            }
                        }
                    }
                    let since = match r.waiting {
                        Some((c, t)) if c == cr => t,
                        _ => tick,
                    };
                    let p = self.rider_boldness(id, r, late);
                    let bold = p > 0.0 && hash01(seed, since + (tick - since) / RED_ROLL_TICKS, cr as u64 ^ RED_SALT) < p;
                    let gap = self.cross_traffic_gap(crossing.node, crossing.segment);
                    if bold && gap >= gap_needed(r.caution, late) && !self.crossing_taken(cr, id) {
                        if r.ran != Some(cr) {
                            r.ran = Some(cr);
                            self.peds.red_lights_run_today += 1;
                            self.peds.red_lights_run_total += 1;
                        }
                        continue;
                    }
                    if r.waiting.is_none_or(|(c, _)| c != cr) {
                        r.waiting = Some((cr, tick));
                    }
                    Some(Stop { at, cr })
                } else if self.crossing_taken(cr, id) {
                    // Somebody is on the zebra: give way. [r4-hunger] Not
                    // for good: a zebra in front of a busy grocer never
                    // emptied and a hundred riders stood at it all day, so
                    // after a few minutes the rider wheels through among
                    // the walkers.
                    let since = match r.waiting {
                        Some((c, t)) if c == cr => t,
                        _ => {
                            r.waiting = Some((cr, tick));
                            tick
                        }
                    };
                    (tick.saturating_sub(since) < ZEBRA_PATIENCE).then_some(Stop { at, cr })
                } else {
                    None
                }
            } else if along <= ACROSS {
                // Straight over the street on a footpath's line: as a walker.
                let to_mid = crossing.mid - pos;
                let t_mid = to_mid.dot(dir);
                if to_mid.perp_dot(dir).abs() > 3.0 || t_mid < -12.0 || t_mid > reach + 12.0 {
                    continue;
                }
                let hw = self.roads.segment(crossing.segment).half_width();
                let entry = t_mid - hw;
                if entry > reach || t_mid + hw < 0.0 {
                    continue;
                }
                if entry <= 0.3 {
                    // On the street now: drivers see a rider on the zebra.
                    feet.occupied.push((cr, id));
                    continue;
                }
                let lawful = match signal {
                    Some((k, ap)) => self.traffic.peds.walk_shown(k as usize, ap as usize),
                    None => true,
                };
                let gap = self.crossing_gap(cr, 0.5);
                let go = if lawful {
                    gap >= LAWFUL_GAP
                } else {
                    let p = self.rider_boldness(id, r, late);
                    let bold = p > 0.0 && hash01(seed, tick / RED_ROLL_TICKS, cr as u64 ^ RED_SALT) < p;
                    if bold && gap >= gap_needed(r.caution, late) && r.ran != Some(cr) {
                        r.ran = Some(cr);
                        self.peds.red_lights_run_today += 1;
                        self.peds.red_lights_run_total += 1;
                    }
                    bold && gap >= gap_needed(r.caution, late)
                };
                if let Some((k, ap)) = signal {
                    if let Some(x) = feet.calls.get_mut(k as usize).and_then(|v| v.get_mut(ap as usize)) {
                        *x = true;
                    }
                }
                if go {
                    if entry < reach {
                        feet.occupied.push((cr, id));
                    }
                    None
                } else {
                    Some(Stop { at: (entry - 0.5).max(0.0), cr })
                }
            } else {
                None
            };
            if let Some(s) = stop {
                if best.as_ref().is_none_or(|b| s.at < b.at) {
                    best = Some(s);
                }
            }
        }
        best
    }

    /// One tick of riding for `id` (by bike, `Activity::Traveling`): true
    /// once the path is done.
    pub(super) fn step_rider(&mut self, id: CitizenId, feet: &mut Feet) -> bool {
        crate::prof_scope!("step_rider");
        let speed = self.config.bike_speed;
        let seed = {
            let c = &mut self.citizens[id];
            c.prev_pos = c.pos;
            c.prev_heading = c.heading;
            c.seed
        };
        let late = match &self.citizens[id].activity {
            Activity::Traveling { purpose, .. } => self.late_for_work(id, *purpose),
            _ => false,
        };
        let mut act = std::mem::replace(&mut self.citizens[id].activity, Activity::Idle);
        let Activity::Traveling { path, idx, .. } = &mut act else {
            self.citizens[id].activity = act;
            return false;
        };
        let mut r = self.peds.riders.get(&id).copied().unwrap_or_else(|| {
            let (caution, hedonism) = driver_traits(seed);
            Rider { waiting: None, ran: None, caution, hedonism, bridges: 0 }
        });
        let mut pos = self.citizens[id].pos;
        let mut heading = self.citizens[id].heading;
        let mut budget = speed;
        let mut stopped: Option<u32> = None;
        let mut rail_wait: Option<usize> = None;
        let mut guard = 0;
        // [walk-crossings] Up a footbridge's stairs and over its deck the
        // bike is wheeled, at a walk.
        let mut wheeled = false;
        // [r1-roads] (section C) A footbridge put up (or a ring of decks
        // joined) since the ride was planned: the rest of it goes over, as
        // a walker re-plans (riders used to cross at grade under bridges
        // built after they set off).
        let walk_key = self.walk.version + 1;
        if r.bridges != walk_key {
            r.bridges = walk_key;
            if !self.walk.footbridges.is_empty() && *idx < path.len() {
                let mut rest: Vec<Vec2> = std::iter::once(pos).chain(path[*idx..].iter().copied()).collect();
                let n = rest.len();
                self.bike_over_footbridges(&mut rest);
                if rest.len() != n {
                    path.truncate(*idx);
                    path.extend_from_slice(&rest[1..]);
                }
            }
        }
        // Crossings matter only once the walk graph and lanes are matched.
        let rules = !self.peds.cross_grid.is_empty() && self.peds.crossings.len() == self.walk.crossings.len();
        while budget > 1e-4 && *idx < path.len() && guard < 64 {
            guard += 1;
            let target = path[*idx];
            let d = pos.distance(target);
            let from = if *idx > 0 { path[*idx - 1] } else { pos };
            if !wheeled && !self.walk.raised.is_empty() && (self.walk.level_at_node(from) > 0.0 || self.walk.level_at_node(target) > 0.0) {
                wheeled = true;
                budget *= self.config.walk_speed / speed.max(1e-3);
            }
            if d > 1e-3 {
                let dir = (target - pos) / d;
                heading = dir.y.atan2(dir.x);
                // [walk-networks] Down in a tunnel the crossings and level
                // crossings of the ways above are nothing to the rider (a
                // bike under a roundabout gave way to its crossings and,
                // standing there, held its traffic up).
                // [bridge-riders] Nor up on a footbridge (its stairs, its
                // deck, a ring of decks): Harvey's flag, the cars under a
                // footbridge braked for the bikes wheeled over it, which the
                // deck, running a step beside the old crosswalk, put "on"
                // that crosswalk ("everyone is taking the footpath").
                let below = self.leg_in_tunnel(from, target, pos, true);
                let off_street = below || self.leg_raised(from, target);
                if rules && !off_street {
                    let reach = d.min(budget + LOOK_AHEAD);
                    if let Some(s) = self.rider_stop(id, &mut r, pos, dir, reach, late, (from, target), feet) {
                        if s.at <= budget {
                            pos += dir * s.at.min(d);
                            stopped = Some(s.cr);
                            break;
                        }
                    }
                }
                // [rail-crossing] A level crossing with its lights on.
                if let Some((k, stop)) = self.rail_stop(pos, pos + dir * d.min(budget)).filter(|_| !off_street) {
                    pos = stop;
                    rail_wait = Some(k);
                    break;
                }
                if d <= budget {
                    if let Some(k) = self.walk.footbridge_deck(from, target) {
                        self.peds.uses.entry(k).or_default().bridge_bikes += 1;
                    }
                    pos = target;
                    *idx += 1;
                    budget -= d;
                } else {
                    pos += dir * budget;
                    budget = 0.0;
                }
            } else {
                pos = target;
                *idx += 1;
            }
        }
        let arrived = *idx >= path.len();
        self.citizens[id].activity = act;
        {
            let c = &mut self.citizens[id];
            c.pos = pos;
            c.heading = heading;
        }
        if let Some(k) = rail_wait {
            self.note_rail_wait(k);
            feet.riders_waiting += 1;
        }
        if stopped.is_some() {
            feet.riders_waiting += 1;
        } else {
            r.waiting = None;
        }
        if arrived {
            self.peds.riders.remove(&id);
        } else {
            self.peds.riders.insert(id, r);
        }
        arrived
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;
    use crate::pedestrians::CAUTIOUS;

    fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
    }

    fn run(w: &mut World, ticks: usize) {
        let mut minds = IdleMind;
        for _ in 0..ticks {
            w.tick(&mut minds);
        }
    }

    /// A signalised crossroads with a few homes, everyone indoors and
    /// deciding nothing, so only the riders we set off move.
    fn town() -> World {
        town_with(&[])
    }

    /// `town` with extra arms from the junction out to each of `arms`.
    fn town_with(arms: &[Vec2]) -> World {
        let mut roads = vec![(vec![Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1), (vec![Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1)];
        roads.extend(arms.iter().map(|&a| (vec![Vec2::ZERO, a], 1, 1)));
        town_of(&roads)
    }

    /// `town` on these roads (points, lanes forward, lanes back).
    fn town_of(roads: &[(Vec<Vec2>, u8, u8)]) -> World {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        for (pts, f, b) in roads {
            w.add_road(pts, *f, *b);
        }
        w.add_zone(square(10.0, 10.0, 180.0), LandUse::Residential);
        run(&mut w, 8 * 60);
        let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in sites {
            w.finish_construction(id);
        }
        let mut guard = 0;
        while w.citizens.len() < 4 && guard < 24 {
            run(&mut w, 60);
            guard += 1;
        }
        assert!(w.citizens.len() >= 4, "people moved in: {}", w.citizens.len());
        for c in w.citizens.values_mut() {
            if let Some(h) = c.home {
                c.activity = Activity::AtHome;
                c.inside = Some(h);
            }
            c.next_decision_tick = u64::MAX;
        }
        run(&mut w, 1);
        w
    }

    fn seeds(f: impl Fn(f32, f32) -> bool, n: usize) -> Vec<u64> {
        (1u64..)
            .filter(|&s| {
                let (c, h) = driver_traits(s);
                f(c, h)
            })
            .take(n)
            .collect()
    }

    /// A signalised crosswalk across an arm of the crossroads: crossing,
    /// intersection, approach, the junction's centre, and the unit vector
    /// from the junction out along the arm.
    fn arm(w: &World) -> (u32, usize, usize, Vec2, Vec2) {
        let (cr, info) = w.peds.crossings.iter().enumerate().find(|(_, i)| i.signal.is_some()).expect("a signalised crosswalk");
        let (k, ap) = info.signal.unwrap();
        let c = &w.walk.crossings[cr];
        let centre = w.roads.node(c.node.unwrap()).pos;
        let out = (c.mid - centre).normalize();
        (cr as u32, k as usize, ap as usize, centre, out)
    }

    /// A bike path on the shoulder along the arm: `from` metres out to
    /// `to` metres out (negative: past the junction), riding toward `to`.
    fn shoulder(w: &World, centre: Vec2, out: Vec2, from: f32, to: f32) -> Vec<Vec2> {
        let d = if to < from { -out } else { out };
        let right = Vec2::new(-d.y, d.x);
        let sid = w.walk.crossings.iter().find(|c| c.node.is_some_and(|n| w.roads.node(n).pos.distance(centre) < 1.0) && (c.mid - centre).normalize().dot(out) > 0.9).map(|c| c.segment).unwrap();
        let off = right * (w.roads.segment(sid).half_width() - crate::roads::SHOULDER * 0.5);
        vec![centre + out * from + off, centre + out * to + off]
    }

    fn rider(w: &mut World, id: CitizenId, seed: u64, path: Vec<Vec2>) {
        w.peds.riders.remove(&id);
        let c = &mut w.citizens[id];
        c.seed = seed;
        c.age = 30;
        c.drunk = 0.0;
        c.inside = None;
        c.mode = TravelMode::Bike;
        c.pos = path[0];
        c.prev_pos = path[0];
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
        c.next_decision_tick = u64::MAX;
    }

    /// Run until the approach has just turned red.
    fn until_red(w: &mut World, k: usize, ap: usize) {
        let mut guard = 0;
        while !w.approach_green(k, ap) && guard < 400 {
            run(w, 1);
            guard += 1;
        }
        while w.approach_green(k, ap) && guard < 800 {
            run(w, 1);
            guard += 1;
        }
        assert!(!w.approach_green(k, ap), "the light turned red");
    }

    /// How far out along the arm a point is.
    fn out_of(p: Vec2, centre: Vec2, out: Vec2) -> f32 {
        (p - centre).dot(out)
    }

    #[test]
    fn a_cyclist_stops_at_the_red_before_the_crosswalk_and_goes_on_green() {
        let mut w = town();
        let (cr, k, ap, centre, out) = arm(&w);
        let mid_out = out_of(w.walk.crossings[cr as usize].mid, centre, out);
        until_red(&mut w, k, ap);
        let id = w.citizens.keys().next().unwrap();
        let path = shoulder(&w, centre, out, mid_out + CROSSWALK_HALF + 6.0, -80.0);
        rider(&mut w, id, seeds(|c, _| c > 0.85, 1)[0], path);
        let (mut waited, mut passed_on) = (0, None);
        for _ in 0..300 {
            let green_before = w.approach_green(k, ap);
            run(&mut w, 1);
            let at = out_of(w.citizens[id].pos, centre, out);
            if w.peds.riders.get(&id).is_some_and(|r| r.waiting.is_some()) {
                waited += 1;
                assert!(at > mid_out + CROSSWALK_HALF, "waits short of the zebra: {at} m out, zebra at {mid_out}");
            }
            if passed_on.is_none() && at < mid_out {
                passed_on = Some(green_before || w.approach_green(k, ap));
            }
            if !matches!(w.citizens[id].activity, Activity::Traveling { .. }) {
                break;
            }
        }
        assert!(waited >= 1, "had to wait at the red");
        assert_eq!(passed_on, Some(true), "went over on the green");
        assert_eq!(w.peds.red_lights_run_total, 0, "a cautious rider never runs the red");
        assert!(!matches!(w.citizens[id].activity, Activity::Traveling { .. }), "arrived");
    }

    /// [r4-hunger] Coming into a signalised junction along a one-way
    /// street out of it (on its pavement), no light faces the rider: no
    /// lane comes in that way and no phase serves it. They used to wait for
    /// that arm's green, which never comes; 2,200 riders stood at such a T
    /// in Harvey's day-62 city until they starved and left. Now they go in
    /// a gap in the cross traffic.
    #[test]
    fn a_cyclist_coming_in_against_a_one_way_is_not_held_for_good() {
        let mut w = town_of(&[
            (vec![Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1),
            (vec![Vec2::new(0.0, -300.0), Vec2::ZERO], 1, 1),
            (vec![Vec2::ZERO, Vec2::new(0.0, 300.0)], 1, 0),
        ]);
        let (cr, k, ap) = w
            .peds
            .crossings
            .iter()
            .enumerate()
            .find_map(|(cr, i)| i.signal.filter(|&(k, ap)| w.approach_unlit(k as usize, ap as usize)).map(|(k, ap)| (cr as u32, k as usize, ap as usize)))
            .expect("a signalised junction with an arm no light faces (the one-way out)");
        assert!(!(0..400).any(|_| {
            run(&mut w, 1);
            w.approach_green(k, ap)
        }), "that arm is never green");
        let c = &w.walk.crossings[cr as usize];
        let centre = w.roads.node(c.node.unwrap()).pos;
        let out = (c.mid - centre).normalize();
        let mid_out = out_of(c.mid, centre, out);
        let id = w.citizens.keys().next().unwrap();
        let path = shoulder(&w, centre, out, mid_out + CROSSWALK_HALF + 6.0, -80.0);
        rider(&mut w, id, seeds(|c, _| c > 0.85, 1)[0], path);
        for _ in 0..600 {
            run(&mut w, 1);
            if !matches!(w.citizens[id].activity, Activity::Traveling { .. }) {
                break;
            }
        }
        assert!(out_of(w.citizens[id].pos, centre, out) < -60.0 || !matches!(w.citizens[id].activity, Activity::Traveling { .. }), "rode through the junction: {} m out", out_of(w.citizens[id].pos, centre, out));
        assert_eq!(w.peds.red_lights_run_total, 0, "and ran no light doing it");
    }

    #[test]
    fn a_cyclist_gives_way_to_a_walker_on_the_crosswalk() {
        let mut w = town();
        let (cr, k, ap, centre, out) = arm(&w);
        let mid_out = out_of(w.walk.crossings[cr as usize].mid, centre, out);
        // Leaving the junction along this arm (no light applies that way):
        // over the zebra while someone is on it.
        let mut ids = w.citizens.keys();
        let (id, walker) = (ids.next().unwrap(), ids.next().unwrap());
        let _ = (k, ap);
        let path = shoulder(&w, centre, out, mid_out - 6.0, mid_out + 60.0);
        rider(&mut w, id, seeds(|c, _| c < 0.1, 1)[0], path);
        for _ in 0..4 {
            w.peds.occupants = vec![(cr, walker)];
            run(&mut w, 1);
            let at = out_of(w.citizens[id].pos, centre, out);
            assert!(at < mid_out - CROSSWALK_HALF, "held short of the occupied zebra: {at} m out, zebra at {mid_out}");
        }
        run(&mut w, 3);
        assert!(out_of(w.citizens[id].pos, centre, out) > mid_out + CROSSWALK_HALF || !matches!(w.citizens[id].activity, Activity::Traveling { .. }), "rode on once the zebra was clear");
    }

    /// [walk-networks] A rider gives way to people on a crossing, not to
    /// another bike standing on it: two bikes each waiting for the other's
    /// crossing locked a roundabout in Harvey's city for days.
    #[test]
    fn a_cyclist_does_not_wait_for_another_bike_on_the_crosswalk() {
        let mut w = town();
        let (cr, _, _, centre, out) = arm(&w);
        let mid_out = out_of(w.walk.crossings[cr as usize].mid, centre, out);
        let mut ids = w.citizens.keys();
        let (id, other) = (ids.next().unwrap(), ids.next().unwrap());
        let path = shoulder(&w, centre, out, mid_out - 6.0, mid_out + 60.0);
        rider(&mut w, id, seeds(|c, _| c < 0.1, 1)[0], path);
        // The other is a cyclist standing on the zebra.
        let at = w.walk.crossings[cr as usize].mid;
        {
            let o = &mut w.citizens[other];
            o.mode = TravelMode::Bike;
            o.inside = None;
            o.pos = at;
            o.prev_pos = at;
            o.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path: vec![at], idx: 0 };
        }
        let mut passed = false;
        for _ in 0..6 {
            w.peds.occupants = vec![(cr, other)];
            w.citizens[other].mode = TravelMode::Bike;
            run(&mut w, 1);
            passed |= out_of(w.citizens[id].pos, centre, out) > mid_out + CROSSWALK_HALF || !matches!(w.citizens[id].activity, Activity::Traveling { .. });
        }
        assert!(passed, "rode over a zebra only a bike stood on");
    }

    /// [review] At a five-arm junction a neighbouring arm's crosswalk lies
    /// within 45 degrees of a rider coming in; the rider obeyed both lights,
    /// which are never green together, and waited for good (a thousand
    /// riders held at one junction starved and left town).
    #[test]
    fn a_cyclist_gets_through_a_five_arm_junction_from_every_arm() {
        let diag = Vec2::from_angle(35f32.to_radians()) * 300.0;
        let mut w = town_with(&[diag]);
        let node = w.walk.crossings.iter().filter_map(|c| c.node).find(|n| w.roads.node(*n).pos.length() < 1.0).expect("the junction");
        let centre = w.roads.node(node).pos;
        let arms: Vec<(u32, Vec2)> = w.walk.crossings.iter().enumerate().filter(|(i, c)| c.node == Some(node) && w.peds.crossings.get(*i).is_some_and(|x| x.signal.is_some())).map(|(i, c)| (i as u32, (c.mid - centre).normalize())).collect();
        assert_eq!(arms.len(), 5, "five signalised crosswalks");
        let id = w.citizens.keys().next().unwrap();
        let seed = seeds(|c, _| c > CAUTIOUS, 1)[0];
        for (cr, out) in arms {
            let mid_out = out_of(w.walk.crossings[cr as usize].mid, centre, out);
            // In along this arm and straight on past the centre.
            let path = shoulder(&w, centre, out, mid_out + CROSSWALK_HALF + 6.0, -12.0);
            rider(&mut w, id, seed, path);
            let mut ticks = 0;
            while matches!(w.citizens[id].activity, Activity::Traveling { .. }) && ticks < 600 {
                run(&mut w, 1);
                ticks += 1;
            }
            assert!(!matches!(w.citizens[id].activity, Activity::Traveling { .. }), "a rider in along {out:?} got through within ten minutes (held at {:?})", w.citizens[id].pos);
            w.citizens[id].activity = Activity::Idle;
        }
        assert_eq!(w.peds.red_lights_run_total, 0, "lawfully");
    }

    #[test]
    fn cautious_cyclists_never_run_reds_and_rebels_sometimes_do() {
        let trials = |seeds: &[u64]| {
            let mut w = town();
            let (cr, k, ap, centre, out) = arm(&w);
            let mid_out = out_of(w.walk.crossings[cr as usize].mid, centre, out);
            let id = w.citizens.keys().next().unwrap();
            for &seed in seeds {
                until_red(&mut w, k, ap);
                let path = shoulder(&w, centre, out, mid_out + 8.0, -80.0);
        rider(&mut w, id, seed, path);
                run(&mut w, 3);
                w.citizens[id].activity = Activity::Idle;
                w.peds.riders.remove(&id);
            }
            w.peds.red_lights_run_total
        };
        assert_eq!(trials(&seeds(|c, _| c > CAUTIOUS, 20)), 0, "the cautious wait");
        let bold = trials(&seeds(|c, h| c < 0.15 && h > 0.8, 30));
        assert!(bold >= 2 && bold < 30, "rebels sometimes run a red on an empty junction: {bold} of 30");
    }
}
