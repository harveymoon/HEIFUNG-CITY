//! [tools] The Intersection editor's side of the world: what a junction
//! (or a lone crosswalk) offers to edit (`IntersectionView`), and the
//! edits: the junction's rule (signals, greens, the left arrow, banned
//! lefts; `Traffic::set_junction_rule`), lane permissions (the player's
//! lane arrows), and crosswalk upgrades (push button, footbridge), paid
//! for and put on the walk graph at once.

use super::*;
use crate::junctions::{CrossingUpgrade, JunctionRule};
use crate::walk::CrossingKind;

/// One street arriving at the junction.
#[derive(Clone, Debug)]
pub struct ApproachView {
    pub segment: SegmentId,
    /// Unit direction the street leaves the junction in.
    pub dir: Vec2,
    /// The street's name.
    pub name: String,
    /// Index into the lane network's approaches (lanes arriving with
    /// arrows), None for a one-way street leaving the junction.
    pub approach: Option<usize>,
    /// Arrows per arriving lane, leftmost first.
    pub arrows: Vec<u8>,
    /// Movements the junction offers from here (after banned lefts).
    pub exits: u8,
    /// Arrows set by the player.
    pub fixed: bool,
    /// Left turns forbidden from here by the player.
    pub no_left: bool,
    /// The junction's shape offers a left from here at all.
    pub left_possible: bool,
    /// Cars queued on its lanes.
    pub queued: usize,
    /// [pt-ux] Its arriving lanes, leftmost first (as `arrows`): where each
    /// meets the junction and the moves out of it, for the editor's
    /// on-road arrows.
    pub lanes: Vec<LaneView>,
}

/// [pt-ux] One lane arriving at a junction.
#[derive(Clone, Debug)]
pub struct LaneView {
    /// Its stop line (the lane's end).
    pub end: Vec2,
    /// Unit direction of travel there.
    pub dir: Vec2,
    /// Every move the lane network has out of it.
    pub moves: Vec<MoveView>,
}

/// [pt-ux] A move from a lane through the junction: its arrow bit
/// (`LEFT`, `STRAIGHT`, `RIGHT`), whether the lane's arrows allow it, and
/// the way it drives (through the junction and a little into the exit).
#[derive(Clone, Debug)]
pub struct MoveView {
    pub bit: u8,
    pub permitted: bool,
    pub path: Vec<Vec2>,
}

/// One signal phase: the approaches (indices into `approaches`) that share
/// a green, and its length.
#[derive(Clone, Debug)]
pub struct PhaseView {
    pub approaches: Vec<usize>,
    pub green: f32,
    pub running: bool,
}

/// One crosswalk.
#[derive(Clone, Debug)]
pub struct CrossingView {
    /// Index into the walk graph's crossings.
    pub index: u32,
    pub mid: Vec2,
    /// Unit direction of the street it crosses.
    pub dir: Vec2,
    /// Its two kerb ends.
    pub ends: [Vec2; 2],
    pub upgrade: CrossingUpgrade,
    /// Across an arm of a signalised junction: it has walk lights already.
    pub signalised: bool,
    /// The street crossed is a highway or the like (no footbridge stairs
    /// on a hard shoulder).
    pub kind: CrossingKind,
    /// For a push-button crossing: its light now.
    pub light: Option<crate::pedestrians::ButtonLight>,
    /// Its footbridge, if it has one: the walkway's ends, the stairs' feet
    /// and the width.
    pub footbridge: Option<([Vec2; 2], [Vec2; 2], f32)>,
    /// [footbridge] Under a footbridge: the crosswalk kept at grade (the
    /// player's choice) rather than replaced by the bridge.
    pub kept: bool,
    /// [footbridge] Replaced by its footbridge: no paint, no walk light,
    /// nobody crosses here at grade.
    pub closed: bool,
}

/// What the Intersection editor shows and edits.
#[derive(Clone, Debug)]
pub struct IntersectionView {
    /// The junction, None for a lone crosswalk.
    pub node: Option<NodeId>,
    pub at: Vec2,
    /// Index into the lane network's intersections.
    pub intersection: Option<usize>,
    pub radius: f32,
    pub signalized: bool,
    pub roundabout: bool,
    pub rule: JunctionRule,
    pub approaches: Vec<ApproachView>,
    pub phases: Vec<PhaseView>,
    pub crossings: Vec<CrossingView>,
}

impl World {
    /// The junction (three or more streets) whose middle is within `r` of
    /// `p`, nearest first.
    pub fn junction_at(&self, p: Vec2, r: f32) -> Option<usize> {
        let net = &self.traffic.net;
        net.intersections
            .iter()
            .enumerate()
            .filter(|(_, i)| i.approaches >= 3)
            .filter_map(|(k, i)| self.roads.nodes.get(i.node.0 as usize).map(|n| (k, n.pos.distance(p), i.radius)))
            .filter(|&(_, d, rad)| d <= r.max(rad + 2.0))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, ..)| k)
    }

    /// The crosswalk (junction or zebra, not a jaywalk) within `r` of `p`.
    pub fn crosswalk_at(&self, p: Vec2, r: f32) -> Option<u32> {
        let nodes = &self.walk.nodes;
        self.walk
            .crossings
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind != CrossingKind::Jay)
            .map(|(k, c)| {
                let (a, b) = (nodes[c.a as usize].pos, nodes[c.b as usize].pos);
                (k as u32, crate::roads::project_on_segment(p, a, b).1.distance(p))
            })
            .filter(|&(_, d)| d <= r)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// The editor's view of whatever is at `p`: a junction (with its
    /// crosswalks), else a lone crosswalk.
    pub fn intersection_view(&self, p: Vec2) -> Option<IntersectionView> {
        if let Some(cr) = self.crosswalk_at(p, 3.0) {
            // A crosswalk of a junction opens the junction.
            if let Some(n) = self.walk.crossings[cr as usize].node {
                if let Some(k) = self.traffic.net.node_intersection.get(n.0 as usize).copied().filter(|&k| k != usize::MAX && self.traffic.net.intersections[k].approaches >= 3) {
                    return Some(self.junction_view(k));
                }
            }
            if self.junction_at(p, 0.0).is_none() {
                return Some(self.crosswalk_view(cr));
            }
        }
        self.junction_at(p, 0.0).map(|k| self.junction_view(k))
    }

    fn crossing_view(&self, k: u32) -> Option<CrossingView> {
        let c = self.walk.crossings.get(k as usize)?;
        self.roads.try_segment(c.segment)?;
        let (s0, s1) = self.roads.segment_points(c.segment);
        let dir = (s1 - s0).normalize_or_zero();
        let nodes = &self.walk.nodes;
        let upgrade = if c.bridged {
            CrossingUpgrade::Footbridge
        } else if c.button {
            CrossingUpgrade::Button
        } else {
            self.crosswalks.find(c.mid, dir).map_or(CrossingUpgrade::None, |r| r.upgrade)
        };
        let signalised = self.peds.crossings.get(k as usize).is_some_and(|i| i.signal.is_some());
        Some(CrossingView {
            index: k,
            mid: c.mid,
            dir,
            ends: [nodes[c.a as usize].pos, nodes[c.b as usize].pos],
            upgrade,
            signalised,
            kind: c.kind,
            light: c.button.then(|| self.button_light(k)),
            footbridge: self.walk.footbridges.iter().find(|f| f.crossing == k).map(|f| (f.top, f.foot, f.width)),
            kept: c.bridged && !c.closed,
            closed: c.closed,
        })
    }

    /// [footbridge] Keep crosswalk `k` at grade under its footbridge, or
    /// let the bridge replace it (the default). Free either way.
    pub fn set_crosswalk_kept(&mut self, k: u32, keep: bool) -> Result<(), String> {
        let v = self.crossing_view(k).ok_or("no such crosswalk")?;
        if v.upgrade != CrossingUpgrade::Footbridge {
            return Err("only a crosswalk under a footbridge can be kept or removed".into());
        }
        if v.kept == keep {
            return Ok(());
        }
        if !self.crosswalks.set_keep_crosswalk(v.mid, v.dir, keep) {
            return Err("no footbridge rule there".into());
        }
        self.rebuild_walk();
        self.peds.buttons.clear();
        self.refresh_crossings_now();
        self.events.push(SimEvent::RoadsChanged);
        Ok(())
    }

    /// [footbridge] Middles of the crosswalks replaced by footbridges: the
    /// street painter leaves their zebras (and walk lights) out.
    pub fn closed_crosswalks(&self) -> Vec<Vec2> {
        self.walk.crossings.iter().filter(|c| c.closed).map(|c| c.mid).collect()
    }

    fn crosswalk_view(&self, k: u32) -> IntersectionView {
        let c = &self.walk.crossings[k as usize];
        IntersectionView {
            node: None,
            at: c.mid,
            intersection: None,
            radius: 6.0,
            signalized: false,
            roundabout: false,
            rule: JunctionRule::new(c.mid),
            approaches: Vec::new(),
            phases: Vec::new(),
            crossings: self.crossing_view(k).into_iter().collect(),
        }
    }

    /// The editor's view of intersection `k`.
    pub fn junction_view(&self, k: usize) -> IntersectionView {
        let net = &self.traffic.net;
        let inter = &net.intersections[k];
        let at = self.roads.node(inter.node).pos;
        let rule = self.traffic.junction_rule(at).cloned().unwrap_or_else(|| JunctionRule::new(at));
        let dirs = net.approach_dirs(&self.roads, k);
        let approaches: Vec<ApproachView> = dirs
            .iter()
            .map(|&(sid, dir)| {
                let ai = net.approaches.iter().position(|a| a.node == inter.node && a.segment == sid);
                let ap = ai.map(|i| &net.approaches[i]);
                // Left possible: some connector from its lanes turns left
                // (whether permitted or not).
                let left_possible = ap.is_some_and(|a| a.lanes.iter().any(|&l| net.lanes[l as usize].next.iter().any(|&c| net.lanes[c as usize].turn < 0)));
                ApproachView {
                    segment: sid,
                    dir,
                    name: self.streets.of(sid).map(|st| st.name.clone()).unwrap_or_else(|| format!("Street {}", sid.0)),
                    approach: ai,
                    arrows: ap.map(|a| a.arrows.clone()).unwrap_or_default(),
                    exits: ap.map_or(0, |a| a.exits),
                    fixed: ap.is_some_and(|a| a.fixed),
                    no_left: rule.left_forbidden(dir),
                    left_possible,
                    queued: ap.map_or(0, |a| a.lanes.iter().map(|&l| net.lanes[l as usize].cars.len()).sum()),
                    lanes: ap.map(|a| a.lanes.iter().map(|&l| lane_view(net, l)).collect()).unwrap_or_default(),
                }
            })
            .collect();
        let phases = inter.phases.iter().enumerate().map(|(p, g)| PhaseView { approaches: g.clone(), green: inter.green_of(p), running: inter.signalized && p == inter.phase }).collect();
        let crossings = self.walk.crossings.iter().enumerate().filter(|(_, c)| c.node == Some(inter.node) && c.kind != CrossingKind::Jay).filter_map(|(i, _)| self.crossing_view(i as u32)).collect();
        IntersectionView {
            node: Some(inter.node),
            at,
            intersection: Some(k),
            radius: inter.radius,
            signalized: inter.signalized,
            roundabout: self.roads.ring_of_node(inter.node).is_some(),
            rule,
            approaches,
            phases,
            crossings,
        }
    }

    /// Set a junction's rule (signals, greens, arrow, banned lefts) and
    /// apply it now.
    pub fn set_junction_rule(&mut self, rule: JunctionRule) -> Result<(), String> {
        let k = self.junction_at(rule.at, 2.0).ok_or("no junction there")?;
        if self.roads.ring_of_node(self.traffic.net.intersections[k].node).is_some() {
            return Err("a roundabout's junctions give way by its own rules".into());
        }
        self.traffic.set_junction_rule(&self.roads, rule);
        self.refresh_crossings_now();
        self.events.push(SimEvent::RoadsChanged);
        Ok(())
    }

    /// Lane permissions on one approach of junction `k` (leftmost lane
    /// first), or back to the defaults with None.
    pub fn set_lane_permissions(&mut self, k: usize, segment: SegmentId, arrows: Option<Vec<u8>>) -> Result<(), String> {
        let node = self.traffic.net.intersections.get(k).ok_or("no such junction")?.node;
        self.traffic.set_lane_use(segment, node, arrows)
    }

    /// Upgrade crosswalk `k` (or take its upgrade away), paying for it.
    pub fn set_crossing_upgrade(&mut self, k: u32, upgrade: CrossingUpgrade) -> Result<(), String> {
        let v = self.crossing_view(k).ok_or("no such crosswalk")?;
        if v.upgrade == upgrade {
            return Ok(());
        }
        match upgrade {
            CrossingUpgrade::Button if v.signalised => return Err("this crosswalk already has walk lights with the junction's signals".into()),
            CrossingUpgrade::Button | CrossingUpgrade::Footbridge if v.kind == CrossingKind::Informal => return Err("only a marked crosswalk can be upgraded".into()),
            _ => {}
        }
        // Two footbridges may not stand in each other's way (at a corner
        // where two arms' stairs would meet).
        if upgrade == CrossingUpgrade::Footbridge {
            if let Some((top, foot)) = self.walk.footbridge_shape(k, &self.roads) {
                let mine = [(top[0], top[1]), (top[0], foot[0]), (top[1], foot[1])];
                let clear = crate::junctions::FOOTBRIDGE_WIDTH + 0.5;
                for f in self.walk.footbridges.iter().filter(|f| f.crossing != k) {
                    let theirs = [(f.top[0], f.top[1]), (f.top[0], f.foot[0]), (f.top[1], f.foot[1])];
                    let close = mine.iter().any(|&(a, b)| theirs.iter().any(|&(c, d)| crate::geom::segment_distance_between(a, b, c, d) < clear));
                    if close {
                        return Err("it would stand in the way of the footbridge next to it".into());
                    }
                }
            }
        }
        let cost = upgrade.cost();
        if cost > 0.0 && self.economy.treasury < cost {
            return Err(format!("not enough money: a {} costs {cost:.0}", upgrade.label()));
        }
        if cost > 0.0 {
            self.economy.debit_item(Flow::Construction, cost, Item::Named(format!("{}s", upgrade.label())));
        }
        self.crosswalks.set(v.mid, v.dir, upgrade);
        self.rebuild_walk();
        self.peds.buttons.clear();
        self.refresh_crossings_now();
        self.events.push(SimEvent::RoadsChanged);
        Ok(())
    }

    /// Dev: send a citizen out on foot from `from` (default: where they
    /// are) to `to`, by the walk graph (raised walkways included). Returns
    /// the number of points on the route.
    pub fn dev_walk(&mut self, id: CitizenId, from: Option<Vec2>, to: Vec2) -> Result<usize, String> {
        let c = self.citizens.get(id).ok_or("no such citizen")?;
        let start = from.unwrap_or(c.pos);
        let (path, _) = self.walk.route(start, to).ok_or("no walking route there")?;
        let n = path.len();
        self.peds.walkers.remove(&id);
        let c = &mut self.citizens[id];
        c.inside = None;
        c.mode = crate::citizens::TravelMode::Walk;
        c.pos = path[0];
        c.prev_pos = path[0];
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
        c.next_decision_tick = self.clock.tick + 120;
        Ok(n)
    }

    /// [footbridge] The height (absolute, as `road_height`) of the raised
    /// way a walker or rider at `pos` is going along: an elevated path or
    /// street, a viaduct, a bridge over the water, a cutting. None at grade
    /// (and in a flat city), where they stand on the ground as before.
    ///
    /// Harvey's flag: people on an elevated path were drawn on the ground
    /// (on the water) beside it. They were lifted onto whatever surface was
    /// nearest their last height, so whoever came onto the path where it
    /// was already up, or stepped half a metre off its narrow deck,
    /// stayed down. Now the leg they walk names the way: a raised way
    /// running along the leg, with them on its corridor.
    pub fn walker_way_height(&self, c: &Citizen, pos: Vec2) -> Option<f32> {
        let Activity::Traveling { path, idx, .. } = &c.activity else { return None };
        let to = *path.get(*idx)?;
        let from = if *idx > 0 { path[*idx - 1] } else { pos };
        self.way_height_on_leg(from, to, pos)
    }

    /// [footbridge] `walker_way_height` for someone at `pos` on the leg
    /// from `from` to `to`.
    pub fn way_height_on_leg(&self, from: Vec2, to: Vec2, pos: Vec2) -> Option<f32> {
        if !self.relief {
            return None;
        }
        let leg = to - from;
        let len = leg.length();
        let dir = (len > 0.5).then(|| leg / len);
        let mut best: Option<(f32, f32)> = None;
        for sid in self.roads.segments_near(pos, 12.0) {
            let s = self.roads.segment(sid);
            if s.is_rail() || s.tunnel {
                continue;
            }
            let (na, nb) = (self.roads.node(s.a), self.roads.node(s.b));
            if !(s.bridge || s.viaduct || na.level.abs() > 0.3 || nb.level.abs() > 0.3) {
                continue;
            }
            let (a, b) = self.roads.segment_points(sid);
            let along = (b - a).normalize_or_zero();
            // Along the way, not across it (a street under a viaduct).
            if dir.is_some_and(|d| d.dot(along).abs() < 0.7) {
                continue;
            }
            let reach = s.half_width() + if s.is_street() { crate::zoning::SIDEWALK } else { 0.0 } + 1.5;
            let (t, q) = crate::geom::project_on_segment(pos, a, b);
            let d = q.distance(pos);
            if d > reach {
                continue;
            }
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, self.road_height(sid, t)));
            }
        }
        best.map(|(_, z)| z)
    }

    /// How high a walker stands above the ground: up the stairs and on the
    /// deck of a raised walkway, 0 elsewhere.
    pub fn walker_level(&self, c: &Citizen, pos: Vec2) -> f32 {
        if self.walk.raised.is_empty() {
            return 0.0;
        }
        match &c.activity {
            Activity::Traveling { path, idx, .. } if *idx > 0 && *idx < path.len() => self.walk.level_on_leg(path[*idx - 1], path[*idx], pos),
            _ => 0.0,
        }
    }
}

/// [pt-ux] Lane `l`'s end and its moves, for the editor.
fn lane_view(net: &crate::traffic::LaneNetwork, l: crate::traffic::LaneId) -> LaneView {
    let lane = &net.lanes[l as usize];
    let n = lane.points.len();
    let end = lane.points.last().copied().unwrap_or_default();
    let dir = if n >= 2 { (lane.points[n - 1] - lane.points[n - 2]).normalize_or_zero() } else { Vec2::ZERO };
    let moves = lane
        .next
        .iter()
        .map(|&c| {
            let con = &net.lanes[c as usize];
            let mut path = con.points.clone();
            // A few metres into the street it leads to, for the direction.
            if let Some(&out) = con.next.first() {
                let exit = &net.lanes[out as usize];
                let mut run = 0.0;
                for w in exit.points.windows(2) {
                    path.push(w[1]);
                    run += w[0].distance(w[1]);
                    if run > 8.0 {
                        break;
                    }
                }
            }
            MoveView { bit: crate::traffic::turn_bit(con.turn), permitted: con.permitted, path }
        })
        .collect();
    LaneView { end, dir, moves }
}
