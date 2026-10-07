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

    /// Someone other than `id` on (or stepping onto) crossing `cr` last tick.
    fn crossing_taken(&self, cr: u32, id: CitizenId) -> bool {
        let occ = &self.peds.occupants;
        let from = occ.partition_point(|x| x.0 < cr);
        occ[from..].iter().take_while(|x| x.0 == cr).any(|x| x.1 != id)
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
    #[allow(clippy::too_many_arguments)]
    fn rider_stop(&mut self, id: CitizenId, r: &mut Rider, pos: Vec2, dir: Vec2, reach: f32, late: bool, feet: &mut Feet) -> Option<Stop> {
        let tick = self.clock.tick;
        let seed = self.citizens[id].seed;
        let end = pos + dir * reach;
        // A crossing's box spans the street between its kerbs, so a ray that
        // meets it (riding along) or runs down it (riding across) comes
        // within a few metres of the box.
        let pad = Vec2::splat(3.5);
        let near = self.peds.cross_grid.in_box(pos.min(end) - pad, pos.max(end) + pad);
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
                let red = match signal {
                    Some((k, ap)) if entering && !self.approach_green(k as usize, ap as usize) => Some((k, ap)),
                    _ => None,
                };
                if let Some((k, ap)) = red {
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
                    // Somebody is on the zebra: give way.
                    Some(Stop { at, cr })
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
            Rider { waiting: None, ran: None, caution, hedonism }
        });
        let mut pos = self.citizens[id].pos;
        let mut heading = self.citizens[id].heading;
        let mut budget = speed;
        let mut stopped: Option<u32> = None;
        let mut guard = 0;
        // Crossings matter only once the walk graph and lanes are matched.
        let rules = !self.peds.cross_grid.is_empty() && self.peds.crossings.len() == self.walk.crossings.len();
        while budget > 1e-4 && *idx < path.len() && guard < 64 {
            guard += 1;
            let target = path[*idx];
            let d = pos.distance(target);
            if d > 1e-3 {
                let dir = (target - pos) / d;
                heading = dir.y.atan2(dir.x);
                if rules {
                    let reach = d.min(budget + LOOK_AHEAD);
                    if let Some(s) = self.rider_stop(id, &mut r, pos, dir, reach, late, feet) {
                        if s.at <= budget {
                            pos += dir * s.at.min(d);
                            stopped = Some(s.cr);
                            break;
                        }
                    }
                }
                if d <= budget {
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
        let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
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
