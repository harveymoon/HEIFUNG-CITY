//! The subway (metro): stations whose entrance is a kiosk on a city lot and
//! whose platform lies underground, joined by lines of twin tunnels that
//! run straight under the city and ignore the street grid. Trains run every
//! line end to end in both directions on a headway, calling at every
//! station; riders walk to an entrance, wait on the platform, ride and
//! walk out at the far end (`world/subway.rs` does the moving).
//!
//! The tunnels are not part of the road network on purpose: track there
//! would be joined to the surface railway's searches, its leaf nodes would
//! become entry points for intercity trains, and every edit would rebuild
//! parcels and lanes. A metro line is its own polyline at [`DEPTH`] below
//! the ground, laid with the railway's own easing (`rail::smooth_track`),
//! and its trains are the railway's own [`Train`] records (the same poses,
//! lengths and stop lists) kept in a separate map.

use crate::buildings::BuildingId;
use crate::rail::{path_length, point_along, Train, TrainKind};
use crate::roads::RoadNetwork;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct MetroLineId;
    pub struct MetroTrainId;
}

/// Metres below the ground the tunnels and platforms lie: deep enough to
/// pass under road tunnels (`roads::TUNNEL_LEVEL` and below) and cuttings.
pub const DEPTH: f32 = 16.0;
/// Building an entrance and its platform box.
pub const STATION_COST: f32 = 15_000.0;
pub const STATION_UPKEEP_PER_DAY: f32 = 140.0;
pub const STATION_JOBS: u16 = 4;
/// Smallest lot an entrance plaza fits on.
pub const STATION_MIN_AREA: f32 = 120.0;
/// Stations closer than this would share a platform.
pub const MIN_SPACING: f32 = 160.0;
/// Boring twin tunnels, per metre of line, paid when the line is laid.
pub const TUNNEL_COST_PER_M: f32 = 22.0;
/// Keeping the tunnels dry, lit and ventilated, per km per day.
pub const TUNNEL_UPKEEP_PER_KM: f32 = 90.0;
/// Each train run costs drivers and power.
pub const RUN_COST: f32 = 6.0;
/// The flat fare, cheaper than the intercity train, dearer than the bus.
pub const FARE: f32 = 2.5;
/// How far people walk to an entrance; a Central Station draws from further.
pub const REACH: f32 = 320.0;
pub const CENTRAL_REACH: f32 = 450.0;
/// The shortest trip worth going underground for.
pub const TRIP_MIN: f32 = 420.0;
/// Cruising speed between stations, m/s.
pub const SPEED: f32 = 22.0;
/// Seconds a train stands at a platform.
pub const DWELL_SECONDS: f32 = 16.0;
/// Cars in a metro set and people it carries.
pub const CARS: u8 = 4;
pub const CAPACITY: usize = 240;
pub const CAR_LEN: f32 = 16.0;
/// Minutes between trains each way, by default and at the limits the
/// player may set.
pub const DEFAULT_HEADWAY: u32 = 10;
pub const MIN_HEADWAY: u32 = 4;
pub const MAX_HEADWAY: u32 = 60;
/// Service hours, sim minutes of the day: 05:00 to midnight.
pub const SERVICE_START: u32 = 5 * 60;
pub const SERVICE_END: u32 = 24 * 60;
/// A line's two tunnels lie this far either side of its centreline.
pub const TRACK_OFFSET: f32 = 2.2;
/// Land value round an entrance: a metro stop puts a block on the map.
pub const LAND_VALUE_RADIUS: f32 = 240.0;
pub const LAND_VALUE_LIFT: f32 = 0.16;
/// Half the straight a line keeps through each station.
pub const PLATFORM_STRAIGHT: f32 = 45.0;
/// Station names, handed out by place (`World::metro_station_name`).
pub const STATION_NAMES: [&str; 32] = [
    "Lantern Square", "Neon Gate", "Old Market", "Chrome Row", "Signal Hill", "Ninth Ward", "Glass Bridge", "Ironworks",
    "Canal Street", "Arcade", "Tower Row", "Dragon Yard", "Pagoda Steps", "Rain Street", "Static Park", "Midnight Square",
    "Silk Lane", "Foundry", "Harbour Light", "Kiln Street", "Paper Moon", "Copper Hill", "Junction Row", "Cable Yard",
    "Night Bazaar", "Steam Alley", "Lotus Court", "Wirefield", "Ash Gardens", "Mirror Lake", "Circuit Plaza", "Long Stair",
];
/// Line colours (indices into the app's line palette), handed out in turn.
pub const LINE_COLOURS: u8 = 8;

/// One metro line: the stations it calls at in order and the tunnel
/// between them. Trains run first to last and last to first.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetroLine {
    pub name: String,
    pub color: u8,
    pub stations: Vec<BuildingId>,
    /// Centreline of the tunnel from the first station to the last.
    pub path: Vec<Vec2>,
    /// Where along `path` each station's platform is (same order as
    /// `stations`, increasing).
    pub stops: Vec<f32>,
    /// Minutes between trains in each direction.
    #[serde(default = "default_headway")]
    pub headway: u32,
    /// Tick the last pair of trains left the termini.
    #[serde(default)]
    pub last_dispatch: u64,
    #[serde(default)]
    pub riders_today: u32,
    #[serde(default)]
    pub riders_yesterday: u32,
    #[serde(default)]
    pub runs_today: u32,
}

fn default_headway() -> u32 {
    DEFAULT_HEADWAY
}

impl MetroLine {
    pub fn length(&self) -> f32 {
        path_length(&self.path)
    }

    pub fn index_of(&self, b: BuildingId) -> Option<usize> {
        self.stations.iter().position(|&s| s == b)
    }

    /// Metres ridden from one station to another, either way.
    pub fn ride_length(&self, board: usize, alight: usize) -> f32 {
        match (self.stops.get(board), self.stops.get(alight)) {
            (Some(a), Some(b)) => (a - b).abs(),
            _ => 0.0,
        }
    }

    /// Whether a train running `forward` (first station to last) takes a
    /// rider from `board` on to `alight`.
    pub fn serves(&self, forward: bool, board: BuildingId, alight: BuildingId) -> bool {
        match (self.index_of(board), self.index_of(alight)) {
            (Some(a), Some(b)) => if forward { b > a } else { b < a },
            _ => false,
        }
    }

    /// Minutes of clock a ride takes: running time plus the dwell at each
    /// station called at on the way.
    pub fn ride_minutes(&self, board: usize, alight: usize, seconds_per_tick: f32) -> f32 {
        let secs = seconds_per_tick.max(0.1);
        let calls = board.abs_diff(alight) as f32;
        self.ride_length(board, alight) / (SPEED * secs) + calls * DWELL_SECONDS / secs
    }

    /// A run of this line as a railway [`Train`]: the tunnel's polyline,
    /// one way or the other, with its calls.
    pub fn run(&self, forward: bool, color: u8) -> Train {
        let total = self.length();
        let (poly, stops): (Vec<Vec2>, Vec<(BuildingId, f32)>) = if forward {
            (self.path.clone(), self.stations.iter().copied().zip(self.stops.iter().copied()).collect())
        } else {
            (self.path.iter().rev().copied().collect(), self.stations.iter().rev().copied().zip(self.stops.iter().rev().map(|s| total - s)).collect())
        };
        Train { kind: TrainKind::Passenger, cars: CARS, nodes: Vec::new(), poly, s: 0.0, prev_s: 0.0, speed: SPEED, stops, next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo: Vec::new(), color, held: false, branch: None }
    }
}

/// A train running a line, one way.
#[derive(Clone, Debug)]
pub struct MetroTrain {
    pub line: MetroLineId,
    /// First station to last.
    pub forward: bool,
    /// Position, calls and riders, as for a surface train. Its polyline is
    /// the line's centreline; the renderer offsets it onto the tunnel of
    /// its direction.
    pub run: Train,
}

impl MetroTrain {
    /// Where the head of the train is now.
    pub fn head(&self) -> Vec2 {
        point_along(&self.run.poly, self.run.s).0
    }

    /// Centre and heading of car `k`, at head position `s`, on the tunnel
    /// of its direction (right-hand running).
    pub fn car_pose_at(&self, k: usize, s: f32) -> (Vec2, f32) {
        let back = k as f32 * (CAR_LEN + crate::rail::COUPLING);
        let (a, _) = point_along(&self.run.poly, s - back);
        let (b, _) = point_along(&self.run.poly, s - back - CAR_LEN);
        let d = (a - b).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        ((a + b) * 0.5 + right * TRACK_OFFSET, d.y.atan2(d.x))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SubwayStats {
    pub stations: u32,
    pub lines: u32,
    pub trains: u32,
    /// People on the platforms and on the trains, now.
    pub waiting: u32,
    pub riding: u32,
    pub riders_today: u32,
    pub riders_yesterday: u32,
    pub fares_today: f32,
    pub fares_yesterday: f32,
    pub upkeep_today: f32,
    pub runs_today: u32,
    /// Metres of line (centreline; each has two bores).
    pub tunnel_m: f32,
}

/// Every metro line and the trains on them. Stations are buildings with
/// `Building::metro` set; boardings per station live here, keyed by
/// building, so a station's record goes when its building does.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Subway {
    #[serde(default)]
    pub lines: SlotMap<MetroLineId, MetroLine>,
    /// Trains are not saved: like the railway's they are back on the next
    /// headway after a load, and their riders are set down at a platform.
    #[serde(skip)]
    pub trains: SlotMap<MetroTrainId, MetroTrain>,
    #[serde(default)]
    pub stats: SubwayStats,
    /// People who boarded at each station today and yesterday.
    #[serde(default)]
    pub boardings: Vec<(BuildingId, u32, u32)>,
    /// Bumped on every change to the stations or lines, so the renderer
    /// redraws the tunnels only then.
    #[serde(skip)]
    pub version: u64,
    #[serde(default)]
    pub next_line: u32,
}

/// A ride the planner found: the line, where to board and alight, and
/// which way the train must be going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetroPlan {
    pub line: MetroLineId,
    pub board: BuildingId,
    pub alight: BuildingId,
    /// The entrance to walk to, and the one walked out of.
    pub entrance: Vec2,
    pub exit: Vec2,
    pub minutes_riding: f32,
}

impl Subway {
    pub fn in_service(minute_of_day: u32) -> bool {
        (SERVICE_START..SERVICE_END).contains(&minute_of_day)
    }

    /// Lines calling at `station`.
    pub fn lines_at(&self, station: BuildingId) -> impl Iterator<Item = (MetroLineId, &MetroLine)> {
        self.lines.iter().filter(move |(_, l)| l.stations.contains(&station))
    }

    pub fn note_boarding(&mut self, station: BuildingId, n: u32) {
        match self.boardings.iter_mut().find(|(b, _, _)| *b == station) {
            Some(e) => e.1 += n,
            None => self.boardings.push((station, n, 0)),
        }
    }

    pub fn boardings_at(&self, station: BuildingId) -> (u32, u32) {
        self.boardings.iter().find(|(b, _, _)| *b == station).map_or((0, 0), |e| (e.1, e.2))
    }

    /// The day ends: today's counts become yesterday's.
    pub fn roll_day(&mut self) {
        for l in self.lines.values_mut() {
            l.riders_yesterday = l.riders_today;
            l.riders_today = 0;
            l.runs_today = 0;
        }
        for e in &mut self.boardings {
            e.2 = e.1;
            e.1 = 0;
        }
        let s = &mut self.stats;
        s.riders_yesterday = s.riders_today;
        s.riders_today = 0;
        s.fares_yesterday = s.fares_today;
        s.fares_today = 0.0;
        s.runs_today = 0;
    }

    /// The best single-line ride for a walk from `from` to `to`: an
    /// entrance within reach of each end on the same line (`entrance` gives
    /// each station's entrance and how far people walk to it), the walks
    /// short enough that riding saves real distance. Ties go to the
    /// shorter walk, so the plan is the same whatever the map's order.
    pub fn plan_trip(&self, from: Vec2, to: Vec2, seconds_per_tick: f32, walk_per_tick: f32, entrance: impl Fn(BuildingId) -> Option<(Vec2, f32)>) -> Option<MetroPlan> {
        let direct = from.distance(to);
        if direct < TRIP_MIN {
            return None;
        }
        let walk = walk_per_tick.max(0.1);
        let mut best: Option<(f32, MetroPlan)> = None;
        for (lid, line) in &self.lines {
            if line.stations.len() < 2 || line.stops.len() != line.stations.len() {
                continue;
            }
            let ends: Vec<(usize, Vec2, f32, f32)> = line.stations.iter().enumerate().filter_map(|(i, &s)| entrance(s).map(|(p, reach)| (i, p, reach, 0.0))).collect();
            for &(i, pi, ri, _) in &ends {
                let d_in = from.distance(pi);
                if d_in > ri {
                    continue;
                }
                for &(j, pj, rj, _) in &ends {
                    if i == j {
                        continue;
                    }
                    let d_out = pj.distance(to);
                    if d_out > rj || d_in + d_out + 150.0 > direct {
                        continue;
                    }
                    let riding = line.ride_minutes(i, j, seconds_per_tick);
                    let cost = (d_in + d_out) / walk + riding;
                    let plan = MetroPlan { line: lid, board: line.stations[i], alight: line.stations[j], entrance: pi, exit: pj, minutes_riding: riding };
                    if best.as_ref().is_none_or(|(c, _)| cost < *c - 1e-3) {
                        best = Some((cost, plan));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }
}

/// Lay a line's tunnel through its stations: the railway's easing (no
/// corner tighter than a train takes, long legs straight), then where along
/// it each station's platform lies, found in order so a line that doubles
/// back near itself still puts each platform on its own pass.
pub fn lay_tunnel(stations: &[Vec2]) -> (Vec<Vec2>, Vec<f32>) {
    if stations.len() < 2 {
        return (stations.to_vec(), vec![0.0; stations.len()]);
    }
    // Each platform is a straight: the line runs through a station along
    // the chord from the station before to the one after, and bends only
    // between stations, so the easing never pulls a tunnel off a platform.
    let n = stations.len();
    let mut ctrl: Vec<Vec2> = Vec::with_capacity(n * 3);
    for (k, &p) in stations.iter().enumerate() {
        let prev = if k > 0 { stations[k - 1] } else { p };
        let next = if k + 1 < n { stations[k + 1] } else { p };
        let dir = (next - prev).normalize_or_zero();
        let room = |q: Vec2| (q.distance(p) / 3.0).min(PLATFORM_STRAIGHT);
        if k > 0 {
            ctrl.push(p - dir * room(prev));
        }
        ctrl.push(p);
        if k + 1 < n {
            ctrl.push(p + dir * room(next));
        }
    }
    ctrl.dedup_by(|a, b| a.distance(*b) < 0.5);
    let path = crate::rail::smooth_track(&ctrl, &RoadNetwork::default());
    let mut stops = Vec::with_capacity(stations.len());
    let mut from = 0.0f32;
    for (k, p) in stations.iter().enumerate() {
        let s = if k == 0 {
            0.0
        } else if k + 1 == stations.len() {
            path_length(&path)
        } else {
            project_from(&path, *p, from)
        };
        stops.push(s);
        from = s + 1.0;
    }
    (path, stops)
}

/// The arc position on `path` nearest `p`, at or after `from`.
fn project_from(path: &[Vec2], p: Vec2, from: f32) -> f32 {
    let mut acc = 0.0;
    let mut best = (f32::INFINITY, from);
    for w in path.windows(2) {
        let len = w[0].distance(w[1]);
        if acc + len >= from && len > 0.0 {
            let (t, q) = crate::roads::project_on_segment(p, w[0], w[1]);
            let s = (acc + t * len).max(from);
            let q = if s > acc + t * len { point_along(path, s).0 } else { q };
            let d = q.distance(p);
            if d < best.0 {
                best = (d, s);
            }
        }
        acc += len;
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::KeyData;

    fn bid(n: u64) -> BuildingId {
        BuildingId::from(KeyData::from_ffi(n | (1 << 32)))
    }

    fn line(sub: &mut Subway, pts: &[Vec2], first: u64) -> MetroLineId {
        let (path, stops) = lay_tunnel(pts);
        let stations = (0..pts.len() as u64).map(|i| bid(first + i)).collect();
        sub.lines.insert(MetroLine { name: "Line 1".into(), color: 0, stations, path, stops, headway: DEFAULT_HEADWAY, last_dispatch: 0, riders_today: 0, riders_yesterday: 0, runs_today: 0 })
    }

    #[test]
    fn a_tunnel_runs_through_its_stations_with_platforms_in_order() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(600.0, 0.0), Vec2::new(600.0, 700.0), Vec2::new(1400.0, 900.0)];
        let (path, stops) = lay_tunnel(&pts);
        assert!(path.len() > 8, "smoothed into a curve");
        assert_eq!(stops.len(), 4);
        assert!(stops.windows(2).all(|w| w[1] > w[0] + 100.0), "platforms in order along the line: {stops:?}");
        // Ends are exact; a station on a corner is passed close by.
        assert!(point_along(&path, stops[0]).0.distance(pts[0]) < 0.5);
        assert!(point_along(&path, stops[3]).0.distance(pts[3]) < 0.5);
        for k in 1..3 {
            let d = point_along(&path, stops[k]).0.distance(pts[k]);
            assert!(d < 3.0, "platform {k} on the line: {d:.1} m off");
        }
        assert!(crate::rail::sharpest_turn(&path) < 0.6, "no corner a train cannot take");
        // Degenerate lines do not panic.
        assert_eq!(lay_tunnel(&[Vec2::ZERO]).1, vec![0.0]);
        assert!(lay_tunnel(&[]).0.is_empty());
    }

    #[test]
    fn runs_go_both_ways_with_calls_in_running_order() {
        let mut sub = Subway::default();
        let id = line(&mut sub, &[Vec2::ZERO, Vec2::new(500.0, 0.0), Vec2::new(1000.0, 0.0)], 1);
        let l = &sub.lines[id];
        let fwd = l.run(true, 0);
        let back = l.run(false, 0);
        assert_eq!(fwd.stops.iter().map(|s| s.0).collect::<Vec<_>>(), vec![bid(1), bid(2), bid(3)]);
        assert_eq!(back.stops.iter().map(|s| s.0).collect::<Vec<_>>(), vec![bid(3), bid(2), bid(1)]);
        assert!(back.stops.windows(2).all(|w| w[1].1 > w[0].1), "reversed calls still increase along the run");
        assert!((back.stops[1].1 - 500.0).abs() < 1.0);
        assert!(point_along(&back.poly, 0.0).0.distance(Vec2::new(1000.0, 0.0)) < 0.5, "the return run starts at the far end");
        assert!(l.serves(true, bid(1), bid(3)) && !l.serves(true, bid(3), bid(1)));
        assert!(l.serves(false, bid(3), bid(2)) && !l.serves(false, bid(1), bid(2)));
        assert!(!l.serves(true, bid(1), bid(9)), "a station not on the line");
        // Cars sit on the right-hand tunnel of their direction.
        let t = MetroTrain { line: id, forward: true, run: { let mut r = fwd.clone(); r.s = 200.0; r } };
        let (p, h) = t.car_pose_at(0, 200.0);
        assert!((p.y - TRACK_OFFSET).abs() < 0.01 && h.abs() < 0.01, "eastbound runs on the south bore: {p:?}");
        let tb = MetroTrain { line: id, forward: false, run: { let mut r = back.clone(); r.s = 200.0; r } };
        assert!((tb.car_pose_at(0, 200.0).0.y + TRACK_OFFSET).abs() < 0.01, "westbound on the north bore");
    }

    #[test]
    fn trips_ride_between_the_entrances_nearest_each_end() {
        let mut sub = Subway::default();
        let pts = [Vec2::ZERO, Vec2::new(800.0, 0.0), Vec2::new(1600.0, 0.0)];
        let id = line(&mut sub, &pts, 1);
        let at = |b: BuildingId| -> Option<(Vec2, f32)> { (1..=3).find(|i| bid(*i) == b).map(|i| (pts[i as usize - 1], REACH)) };
        let walk = 1.4 * 4.0;
        let p = sub.plan_trip(Vec2::new(50.0, 100.0), Vec2::new(1550.0, -80.0), 4.0, walk, at).expect("a ride end to end");
        assert_eq!((p.line, p.board, p.alight), (id, bid(1), bid(3)));
        // The other way round boards at the far end.
        let back = sub.plan_trip(Vec2::new(1550.0, -80.0), Vec2::new(820.0, 60.0), 4.0, walk, at).unwrap();
        assert_eq!((back.board, back.alight), (bid(3), bid(2)));
        // Too short, out of reach, or a walk the ride would not shorten.
        assert!(sub.plan_trip(Vec2::new(0.0, 10.0), Vec2::new(300.0, 0.0), 4.0, walk, at).is_none(), "under the trip minimum");
        assert!(sub.plan_trip(Vec2::new(0.0, 900.0), Vec2::new(1600.0, 900.0), 4.0, walk, at).is_none(), "no entrance within reach");
        assert!(sub.plan_trip(Vec2::new(400.0, 300.0), Vec2::new(400.0, -300.0), 4.0, walk, at).is_none(), "walking to the stations is no shorter");
        // A Central Station's longer reach draws riders from further off.
        let far = Vec2::new(0.0, 420.0);
        assert!(sub.plan_trip(far, Vec2::new(1600.0, 50.0), 4.0, walk, at).is_none());
        let wide = |b: BuildingId| at(b).map(|(p, _)| (p, if b == bid(1) { CENTRAL_REACH } else { REACH }));
        assert_eq!(sub.plan_trip(far, Vec2::new(1600.0, 50.0), 4.0, walk, wide).map(|p| p.board), Some(bid(1)));
    }

    #[test]
    fn boardings_roll_over_at_midnight() {
        let mut sub = Subway::default();
        let id = line(&mut sub, &[Vec2::ZERO, Vec2::new(900.0, 0.0)], 1);
        sub.note_boarding(bid(1), 3);
        sub.note_boarding(bid(1), 2);
        sub.lines[id].riders_today = 5;
        sub.stats.riders_today = 5;
        assert_eq!(sub.boardings_at(bid(1)), (5, 0));
        sub.roll_day();
        assert_eq!(sub.boardings_at(bid(1)), (0, 5));
        assert_eq!((sub.lines[id].riders_today, sub.lines[id].riders_yesterday), (0, 5));
        assert_eq!((sub.stats.riders_today, sub.stats.riders_yesterday), (0, 5));
        assert_eq!(sub.boardings_at(bid(7)), (0, 0));
    }

    #[test]
    fn ride_minutes_count_running_and_dwells() {
        let mut sub = Subway::default();
        let id = line(&mut sub, &[Vec2::ZERO, Vec2::new(880.0, 0.0), Vec2::new(1760.0, 0.0)], 1);
        let l = &sub.lines[id];
        let m = l.ride_minutes(0, 2, 4.0);
        let expect = 1760.0 / (SPEED * 4.0) + 2.0 * DWELL_SECONDS / 4.0;
        assert!((m - expect).abs() < 0.2, "{m} vs {expect}");
        assert_eq!(l.ride_minutes(1, 1, 4.0), 0.0);
    }
}
