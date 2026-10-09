//! [road-inspect] Street pieces as things to inspect: which piece a click
//! picks (the one under the pointer within its half width; at a crossing
//! of two levels the upper one, or in the Underground view the one below
//! ground), the gold outline along both edges of the selected piece with
//! the rest of its run between junctions traced faintly, the selection
//! following its piece through road rebuilds, and the words for what a
//! piece is (kind, level, lanes). The card itself is `ui::road`; the remote
//! API's read-out `remote::road`.

use bevy::prelude::*;
use nd_sim::roads::{RoadKind, Segment};
use nd_sim::streets::StreetClass;
use nd_sim::{SegmentId, Vec2 as SVec2, World};

use crate::sim::Sim;
use crate::overlay::shapes::GOLD;
use crate::tools::{Depth, Selection};

/// A click this far outside a piece's half width still picks it.
const SLACK: f32 = 0.6;

/// The piece a click at `p` lands on, if any: not inside a junction (those
/// belong to the junction), the upper of two crossing levels, or in the
/// Underground view only what lies below ground.
pub fn pick(w: &World, p: SVec2, depth: Depth) -> Option<SegmentId> {
    let under = depth == Depth::Underground;
    if !under && w.junction_at(p, 0.0).is_some() {
        return None;
    }
    let reach = SVec2::splat(16.0);
    w.roads
        .segments_in_box(p - reach, p + reach)
        .filter_map(|sid| {
            let s = w.roads.segment(sid);
            let (a, b) = w.roads.segment_points(sid);
            let (t, q) = nd_sim::roads::project_on_segment(p, a, b);
            let d = q.distance(p);
            if d > s.half_width() + SLACK {
                return None;
            }
            let h = w.road_height(sid, t);
            let below = s.tunnel || h < crate::render::UNDERGROUND_BELOW;
            if below != under {
                return None;
            }
            Some((h, d, sid))
        })
        // The upper level first, then the nearest centre line.
        .max_by(|x, y| x.0.total_cmp(&y.0).then(y.1.total_cmp(&x.1)).then(y.2.cmp(&x.2)))
        .map(|(_, _, sid)| sid)
}

/// The selected piece outlined in gold along both edges (and across its
/// ends), at its own height; the rest of its run between junctions traced
/// faintly so the street it belongs to shows.
pub fn draw(gizmos: &mut Gizmos, w: &World, sid: SegmentId) {
    if w.roads.try_segment(sid).is_none_or(|s| !s.alive) {
        return;
    }
    for other in w.road_run(sid) {
        if other != sid {
            edges(gizmos, w, other, 0.0, GOLD.with_alpha(0.3), false);
        }
    }
    edges(gizmos, w, sid, 0.4, GOLD, true);
}

fn edges(gizmos: &mut Gizmos, w: &World, sid: SegmentId, out: f32, colour: Color, caps: bool) {
    let s = w.roads.segment(sid);
    let (a, b) = w.roads.segment_points(sid);
    let len = a.distance(b);
    if len < 0.01 {
        return;
    }
    let dir = (b - a) / len;
    let side = SVec2::new(-dir.y, dir.x) * (s.half_width() + out);
    let n = ((len / 6.0).ceil() as usize).clamp(1, 64);
    let at = |t: f32, k: f32| {
        let p = a + (b - a) * t + side * k;
        Vec3::new(p.x, w.road_height(sid, t) + 0.45, p.y)
    };
    for k in [-1.0f32, 1.0] {
        gizmos.linestrip((0..=n).map(|i| at(i as f32 / n as f32, k)), colour);
    }
    if caps {
        for t in [0.0f32, 1.0] {
            gizmos.line(at(t, -1.0), at(t, 1.0), colour);
        }
    }
}

/// The live piece whose midpoint is within a few metres of `mid`: where a
/// selected piece went when the network was rebuilt (new segment numbers).
pub fn repoint(w: &World, mid: SVec2) -> Option<SegmentId> {
    w.roads
        .nearest_on_network(mid)
        .filter(|&(n, _, _)| {
            let (a, b) = w.roads.segment_points(n);
            ((a + b) * 0.5).distance(mid) < 6.0
        })
        .map(|(n, _, _)| n)
}

/// The midpoint of a live piece.
fn midpoint(w: &World, sid: SegmentId) -> Option<SVec2> {
    w.roads.try_segment(sid).filter(|s| s.alive)?;
    let (a, b) = w.roads.segment_points(sid);
    Some((a + b) * 0.5)
}

/// [fix/review-app] Run `f` (an implement) and keep a road selection on
/// its piece through the rebuild it causes, at once: the road card drawn
/// later in the same egui pass would otherwise find the old piece gone and
/// drop the selection before `keep_on_rebuild` could re-point it (the
/// toolbar's Implement lost it; Ctrl+Enter, in Input, did not).
pub fn keeping_road<R>(w: &mut World, selection: &mut Selection, f: impl FnOnce(&mut World) -> R) -> R {
    let mid = match *selection {
        Selection::Road(sid) => midpoint(w, sid),
        _ => None,
    };
    let r = f(w);
    if let (Some(mid), Selection::Road(sid)) = (mid, *selection) {
        if midpoint(w, sid).is_none_or(|m| m.distance(mid) > 0.01) {
            *selection = repoint(w, mid).map_or(Selection::None, Selection::Road);
        }
    }
    r
}

/// Keeps a road selection on its piece when the network is rebuilt (new
/// segment numbers): the piece whose midpoint is nearest the selected
/// one's, else the selection is dropped.
pub fn keep_on_rebuild(sim: Res<Sim>, mut selection: ResMut<Selection>, mut last: Local<Option<(SegmentId, SVec2, u64)>>) {
    let w = &sim.world;
    let Selection::Road(sid) = *selection else {
        *last = None;
        return;
    };
    let version = w.roads.version;
    match *last {
        Some((s, mid, v)) if s == sid && v != version => match repoint(w, mid) {
            Some(n) => *selection = Selection::Road(n),
            None => {
                *selection = Selection::None;
                *last = None;
                return;
            }
        },
        _ => {}
    }
    let Selection::Road(sid) = *selection else { return };
    match midpoint(w, sid) {
        Some(mid) => *last = Some((sid, mid, version)),
        None => {
            *selection = Selection::None;
            *last = None;
        }
    }
}

/// What kind of way a piece is: "street", "avenue", "highway", "ramp",
/// "roundabout", "path" or "rail".
pub fn kind(w: &World, sid: SegmentId) -> &'static str {
    let Some(s) = w.roads.try_segment(sid) else { return "road" };
    match s.kind {
        RoadKind::Path => "path",
        RoadKind::Rail => "rail",
        RoadKind::Street if s.ramp => "ramp",
        RoadKind::Street if s.is_highway() => "highway",
        RoadKind::Street if s.ring => "roundabout",
        RoadKind::Street => match w.streets.of(sid).map(|st| st.class) {
            Some(StreetClass::Avenue) => "avenue",
            Some(StreetClass::Highway) => "highway",
            _ if s.lanes() >= 4 => "avenue",
            _ => "street",
        },
    }
}

/// The piece's level: "tunnel", "viaduct", "bridge", "cutting" or "at grade".
pub fn level(w: &World, s: &Segment) -> &'static str {
    if s.tunnel {
        "tunnel"
    } else if s.viaduct {
        "viaduct"
    } else if s.bridge {
        "bridge"
    } else if w.roads.node(s.a).level < 0.0 || w.roads.node(s.b).level < 0.0 {
        "cutting"
    } else if w.roads.node(s.a).level > 0.0 || w.roads.node(s.b).level > 0.0 {
        "embankment"
    } else {
        "at grade"
    }
}

/// The name the card leads with: the street's, else what it is.
pub fn name(w: &World, sid: SegmentId) -> String {
    w.streets.of(sid).map_or_else(|| format!("Unnamed {}", kind(w, sid)), |s| s.name.clone())
}

/// Everything the road card shows about one piece, gathered once for the
/// inspector and the remote API alike.
#[derive(Clone, Debug)]
pub struct Card {
    pub name: String,
    pub kind: &'static str,
    pub level: &'static str,
    pub lanes_fwd: u8,
    pub lanes_bwd: u8,
    pub one_way: bool,
    /// The fastest lane's limit, m/s (0 for track).
    pub limit: f32,
    pub length: f32,
    /// The run between junctions: its pieces, where this one is in it, and
    /// its length.
    pub run: Vec<SegmentId>,
    pub run_index: usize,
    pub run_length: f32,
    pub now: nd_sim::road_traffic::RoadNow,
    /// Vehicles onto it per hour, oldest first, the last the hour under way.
    pub hours: Vec<u16>,
    /// Absolute hour (tick / 60) of the last entry of `hours`.
    pub hour_now: u32,
    pub decay: Option<Decay>,
    pub tolls: Vec<(u32, String, f32)>,
    /// Crosswalks over it: (junction arms, zebras, push-button signals,
    /// footbridges).
    pub crossings: (u32, u32, u32, u32),
    /// Bus stops on it: the lines' names.
    pub bus_stops: Vec<String>,
    /// Cars parked at its kerbs, and whether kerb parking is allowed.
    pub kerb_parked: u32,
    pub kerb_parking: bool,
}

/// The wear side of the card (pieces the city's crews look after).
#[derive(Clone, Debug)]
pub struct Decay {
    pub wear: f32,
    pub label: &'static str,
    pub speed_factor: f32,
    pub crash_factor: f32,
    pub rate: f32,
    pub rate_measured: bool,
    /// The next condition and the days to it.
    pub next: Option<(&'static str, f32)>,
    pub repair_cost: f32,
    pub land_value: f32,
    /// Place in the crews' queue (1 first), its length, and the piece's priority there.
    pub queue_place: Option<usize>,
    pub queue_len: usize,
    pub priority: f32,
    pub repaired_day: Option<u32>,
    pub repaired_whole: bool,
    pub repaired_by_order: bool,
    pub funding: f32,
}

impl Card {
    pub fn of(w: &World, sid: SegmentId) -> Option<Card> {
        use nd_sim::road_wear::{self, lane_metres, wears};
        let s = w.roads.try_segment(sid).filter(|s| s.alive)?;
        let (a, b) = w.roads.segment_points(sid);
        let i = sid.0 as usize;
        let limit = w.traffic.net.segment_lanes.get(&sid).map_or(0.0, |ls| ls.iter().map(|&l| w.traffic.net.lanes[l as usize].speed_limit).fold(0.0, f32::max));
        let run = w.road_run(sid);
        let run_index = run.iter().position(|&r| r == sid).unwrap_or(0);
        let run_length = run.iter().map(|&r| {
            let (p, q) = w.roads.segment_points(r);
            p.distance(q)
        }).sum();
        let hour_now = (w.clock.tick / 60) as u32;
        let hours = w.road_hours(sid).map_or_else(|| vec![0; nd_sim::road_traffic::HOURS], |l| l.traffic.series(hour_now, nd_sim::road_traffic::HOURS));
        let decay = wears(&w.roads, i).then(|| {
            let wear = w.road_wear.wear_of(i);
            let rough = road_wear::roughness(wear);
            let (rate, rate_measured) = w.road_wear_rate(sid);
            let queue = w.repair_queue();
            let place = queue.iter().position(|j| j.segment == sid);
            let land_value = w.land_value.at((a + b) * 0.5);
            let log = w.road_hours(sid);
            Decay {
                wear,
                label: road_wear::label(wear),
                speed_factor: road_wear::slow_factor(rough),
                crash_factor: road_wear::crash_factor(rough),
                rate,
                rate_measured,
                next: nd_sim::road_traffic::next_step(wear, rate),
                repair_cost: road_wear::repair_cost(lane_metres(&w.roads, i), wear),
                land_value,
                queue_place: place.map(|p| p + 1),
                queue_len: queue.len(),
                priority: road_wear::priority(wear, land_value),
                repaired_day: log.and_then(|l| l.repaired_day),
                repaired_whole: log.is_some_and(|l| l.repaired_whole),
                repaired_by_order: log.is_some_and(|l| l.repaired_by_order),
                funding: w.road_wear.funding,
            }
        });
        let tolls = w.tolls.booths.iter().filter(|t| t.segment == Some(sid)).map(|t| (t.id, t.name.clone(), t.price)).collect();
        let mut crossings = (0, 0, 0, 0);
        // [footbridge] A zebra where a footpath meets the street sits on the
        // node between two pieces and is listed with one of them: it counts
        // on both (Harvey's footbridge there showed on neither card's
        // neighbour). A crosswalk a footbridge replaced counts only as the
        // footbridge.
        let on_piece = |c: &&nd_sim::walk::Crossing| c.segment == sid || (c.kind == nd_sim::walk::CrossingKind::Zebra && (c.node == Some(s.a) || c.node == Some(s.b)));
        for c in w.walk.crossings.iter().filter(on_piece) {
            match c.kind {
                _ if c.closed => {}
                nd_sim::walk::CrossingKind::Junction => crossings.0 += 1,
                nd_sim::walk::CrossingKind::Zebra => crossings.1 += 1,
                _ => {}
            }
            crossings.2 += u32::from(c.button);
            crossings.3 += u32::from(c.bridged);
        }
        let mut bus_stops: Vec<String> = w.transit.routes.values().filter(|r| r.stops.iter().any(|st| st.segment == sid)).map(|r| r.name.clone()).collect();
        bus_stops.sort();
        bus_stops.dedup();
        Some(Card {
            name: name(w, sid),
            kind: kind(w, sid),
            level: level(w, s),
            lanes_fwd: s.lanes_fwd,
            lanes_bwd: s.lanes_bwd,
            one_way: s.is_street() && (s.lanes_fwd == 0 || s.lanes_bwd == 0),
            limit,
            length: a.distance(b),
            run,
            run_index,
            run_length,
            now: w.road_now(sid),
            hours,
            hour_now,
            decay,
            tolls,
            crossings,
            bus_stops,
            kerb_parked: w.vehicles.kerbs.get(&sid).map_or(0, |k| k.len() as u32),
            kerb_parking: s.is_street() && !s.is_motorway() && !s.tunnel && !w.vehicles.no_kerb.1.contains(&sid),
        })
    }

    /// Vehicles onto it this hour so far, the last whole hour, today and
    /// yesterday (from the hourly counts).
    pub fn flows(&self) -> (u32, u32, u32, u32) {
        let n = self.hours.len();
        let h = |k: usize| self.hours.get(n.wrapping_sub(1 + k)).copied().unwrap_or(0) as u32;
        let hour_of_day = (self.hour_now % 24) as usize;
        let today: u32 = (0..=hour_of_day).map(h).sum();
        let yesterday: u32 = (hour_of_day + 1..hour_of_day + 25).map(h).sum();
        (h(0), h(1), today, yesterday)
    }

    /// The busiest whole hour kept: (absolute hour, vehicles).
    pub fn peak(&self) -> Option<(u32, u16)> {
        let n = self.hours.len() as u32;
        self.hours.iter().enumerate().take(self.hours.len().saturating_sub(1)).max_by_key(|(k, &c)| (c, *k)).filter(|(_, &c)| c > 0).map(|(k, &c)| (self.hour_now + 1 + k as u32 - n, c))
    }
}
