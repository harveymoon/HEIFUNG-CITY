//! [cable] Aerial cable cars: gondola lifts and aerial tramways.
//!
//! A cable line runs station to station in straight spans and may turn at
//! a station (an angle station). Its pylons are placed by [`lay_line`]
//! along each span, as tall as the ground and what stands below need: the
//! cable sags between supports (a parabola, `Spec::sag` of the span at its
//! middle), and the cabins hanging under it must clear roofs, roads, water
//! and the ground by [`CLEARANCE`]. Where no pylon can stand or a span is
//! too long or too steep the line is refused with the reason.
//!
//! Two kinds ([`CableKind`]):
//! - a **gondola** is a loop of small cabins on a moving rope: they crawl
//!   through every station (detached from the rope), so riders board any
//!   cabin passing, waits are short and the capacity is high; spans are
//!   short (pylons every few hundred metres) and it may turn at stations.
//! - an **aerial tramway** shuttles two big cabins between two stations,
//!   one up while the other comes down. It hangs spans of up to two
//!   kilometres (across a lake, up a cliff) and climbs steeper, but each
//!   cabin waits at the station and riders wait for it.
//!
//! The moving parts are pure functions of the line and its `phase`
//! ([`CableLine::cabin_pose`]), so the simulation (`world/cable.rs`, who is
//! aboard which cabin, boarding as cabins pass) and the renderer agree
//! without saving any cabin.
//!
//! Stations stand on a lot they take (a ground station), on the roof of a
//! tall building (a rooftop station, the building stays) or on open ground
//! (a hillside, a park), and are reached on foot at their entrance.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::timetable::Timeline;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct CableStationId;
    pub struct CableLineId;
}

/// Metres between the bottom of a cabin and whatever is below it, at the
/// least, along a span.
pub const CLEARANCE: f32 = 4.0;
/// Over open water the cabins clear a mast or a ferry's funnel.
pub const WATER_CLEARANCE: f32 = 9.0;
/// Over a road, a lorry.
pub const ROAD_CLEARANCE: f32 = 5.0;
/// Over open ground, trees and lamp posts.
pub const GROUND_CLEARANCE: f32 = 6.0;
/// Stations closer than this would share a platform.
pub const MIN_STATION_GAP: f32 = 110.0;
/// No pylon within this many metres of a station's middle (the station's
/// own structure carries the rope there).
pub const PYLON_STATION_GAP: f32 = 24.0;
/// Pylons stand at least this far apart.
pub const MIN_PYLON_GAP: f32 = 45.0;
/// Pylon positions tried along a span, this far apart.
pub const PYLON_STEP: f32 = 5.0;
/// The profile under a span is sampled this often.
pub const SAMPLE_STEP: f32 = 4.0;
/// Platform floor above the ground at a ground station; above the roof at
/// a rooftop one.
pub const GROUND_FLOOR: f32 = 1.2;
pub const ROOF_FLOOR: f32 = 0.6;
/// A building at least this tall takes a station on its roof (lower ones
/// are bought out and the station stands on the lot).
pub const ROOF_MIN_HEIGHT: f32 = 14.0;
/// How far people walk to a station.
pub const REACH: f32 = 340.0;
/// The shortest trip worth riding.
pub const TRIP_MIN: f32 = 260.0;
/// An open-ground station needs a way to walk in from this near.
pub const LINK_REACH: f32 = 90.0;
/// Land value round a station, and the hum of its bullwheel next door.
pub const LAND_VALUE_RADIUS: f32 = 200.0;
pub const LAND_VALUE_LIFT: f32 = 0.08;
pub const NOISE_RADIUS: f32 = 45.0;
pub const NOISE: f32 = 0.03;
/// [cable] The view from a station well above the town: up to this much
/// more land value round it.
pub const VIEW_LIFT: f32 = 0.08;
/// Service hours, minutes of the day: 06:00 to 23:00 by default.
pub const OPEN: u32 = 6 * 60;
pub const CLOSE: u32 = 23 * 60;
/// Minutes after closing a rider still waiting gives up.
pub const CLOSING_GRACE: u32 = 15;
/// The longest a line's fare may be set.
pub const MAX_FARE: f32 = 25.0;
/// Line colours, handed out in turn (indices into the app's palette).
pub const LINE_COLOURS: u8 = 8;
/// The most cable lines and stations a city may build.
pub const MAX_LINES: usize = 24;

/// Station names, by place, the first free one (`World::cable_station_name`).
pub const STATION_NAMES: [&str; 24] = [
    "Summit", "Skyline Deck", "Ridge Terrace", "Cloud Gate", "Lookout", "Upper Market", "Harbour Rise", "Kite Hill",
    "Wire Garden", "Eagle Steps", "Crown Point", "High Bazaar", "Vista Row", "Mist Platform", "Sunset Ledge", "Bellwheel Yard",
    "Tower Landing", "Cliff Street", "Lantern Peak", "Cable Plaza", "Overlook", "River Crossing", "Pine Ledge", "North Anchor",
];

/// The kinds of cable line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CableKind {
    /// A loop of small cabins on a moving rope.
    #[default]
    Gondola,
    /// Two big cabins shuttling to and fro.
    Tramway,
}

/// What a kind of line can do and costs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    /// Rope speed (gondola) or cruising speed (tramway), m/s.
    pub speed: f32,
    /// People a cabin holds.
    pub seats: u32,
    /// The longest span between two supports, m.
    pub max_span: f32,
    /// Pylon heights, ground to rope, m.
    pub min_pylon: f32,
    pub max_pylon: f32,
    /// How far the rope sags at mid-span, as a share of the span.
    pub sag: f32,
    /// The steepest a span may climb (rise over run).
    pub max_grade: f32,
    /// Metres from the rope down to the cabin's floor.
    pub hanger: f32,
    /// Cabin height and length (for clearance and drawing), m.
    pub cabin_h: f32,
    pub cabin_len: f32,
    /// Metres between the two ropes (up and down, or the two tracks).
    pub gauge: f32,
    /// Half a station's length along the line, m.
    pub station_half: f32,
    /// Seconds a gondola cabin takes to crawl through a station, or a
    /// tramway cabin stands at one.
    pub station_seconds: f32,
    pub fare: f32,
    pub station_cost: f32,
    /// Rope and line works per metre, and a pylon (plus per metre of it).
    pub cost_per_m: f32,
    pub pylon_cost: f32,
    pub pylon_cost_per_m: f32,
    /// Per day: each station (its staff's wages and the drive), each km of
    /// rope, each cabin.
    pub station_upkeep: f32,
    pub upkeep_per_km: f32,
    pub cabin_upkeep: f32,
    /// Staff at each station (paid in its upkeep).
    pub staff: u16,
    pub max_stations: usize,
    pub max_length: f32,
    /// Gondola: metres of rope between cabins, by default and at the limits.
    pub spacing: f32,
    pub min_spacing: f32,
    pub max_spacing: f32,
}

pub const GONDOLA: Spec = Spec {
    speed: 6.0,
    seats: 10,
    max_span: 380.0,
    min_pylon: 12.0,
    max_pylon: 58.0,
    sag: 0.03,
    max_grade: 1.0,
    hanger: 4.6,
    cabin_h: 2.4,
    cabin_len: 2.3,
    gauge: 6.0,
    station_half: 13.0,
    station_seconds: 20.0,
    fare: 3.0,
    station_cost: 16_000.0,
    cost_per_m: 26.0,
    pylon_cost: 2_200.0,
    pylon_cost_per_m: 70.0,
    station_upkeep: 150.0,
    upkeep_per_km: 110.0,
    cabin_upkeep: 2.0,
    staff: 4,
    max_stations: 8,
    max_length: 6_000.0,
    spacing: 42.0,
    min_spacing: 24.0,
    max_spacing: 160.0,
};

pub const TRAMWAY: Spec = Spec {
    speed: 12.0,
    seats: 100,
    max_span: 2_100.0,
    min_pylon: 18.0,
    max_pylon: 96.0,
    sag: 0.045,
    max_grade: 1.5,
    hanger: 7.4,
    cabin_h: 3.0,
    cabin_len: 7.6,
    gauge: 9.0,
    station_half: 15.0,
    station_seconds: 28.0,
    fare: 4.0,
    station_cost: 26_000.0,
    cost_per_m: 38.0,
    pylon_cost: 9_000.0,
    pylon_cost_per_m: 160.0,
    station_upkeep: 220.0,
    upkeep_per_km: 130.0,
    cabin_upkeep: 60.0,
    staff: 6,
    max_stations: 2,
    max_length: 3_600.0,
    spacing: 0.0,
    min_spacing: 0.0,
    max_spacing: 0.0,
};

impl CableKind {
    pub const ALL: [CableKind; 2] = [CableKind::Gondola, CableKind::Tramway];

    pub fn spec(self) -> &'static Spec {
        match self {
            CableKind::Gondola => &GONDOLA,
            CableKind::Tramway => &TRAMWAY,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CableKind::Gondola => "gondola",
            CableKind::Tramway => "tramway",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CableKind::Gondola => "gondola",
            CableKind::Tramway => "aerial tramway",
        }
    }

    pub fn parse(s: &str) -> Option<CableKind> {
        match s.to_ascii_lowercase().as_str() {
            "gondola" | "gondola_lift" | "cable_car" | "cablecar" | "telecabine" => Some(CableKind::Gondola),
            "tramway" | "aerial_tramway" | "tram" | "aerial_tram" | "shuttle" => Some(CableKind::Tramway),
            _ => None,
        }
    }
}

/// One station: a platform at `floor`, reached at `entrance`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CableStation {
    pub name: String,
    pub pos: Vec2,
    /// Height of the platform floor (absolute, m).
    pub floor: f32,
    /// Height of the ground under it (the street below a roof).
    pub ground: f32,
    /// On a building's roof (the building stays and keeps its use).
    #[serde(default)]
    pub roof: bool,
    /// The lot's station building (a ground station on a lot) or the
    /// building whose roof it is on; none on open ground.
    #[serde(default)]
    pub building: Option<BuildingId>,
    /// Where people walk in, on the walk network (or the host building's door).
    pub entrance: Vec2,
    #[serde(default)]
    pub boardings_today: u32,
    #[serde(default)]
    pub boardings_yesterday: u32,
    #[serde(default)]
    pub built_tick: u64,
}

/// What holds the rope up: a station or a pylon.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub pos: Vec2,
    /// Metres along the line from its first station.
    pub s: f32,
    /// The ground at its foot and the rope at its top (absolute, m).
    pub base: f32,
    pub top: f32,
    /// Which of the line's stations it is (`None`: a pylon).
    pub station: Option<u16>,
}

impl Support {
    pub fn height(&self) -> f32 {
        self.top - self.base
    }
}

/// Someone in a cabin, getting off at station `alight` (an index into the
/// line's stations).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aboard {
    pub citizen: CitizenId,
    pub cabin: u32,
    pub alight: u16,
}

/// One line: its stations in order, its supports and its cabins.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CableLine {
    pub name: String,
    pub kind: CableKind,
    pub color: u8,
    pub stations: Vec<CableStationId>,
    /// Every support from the first station to the last, stations included.
    pub supports: Vec<Support>,
    /// Where along the line each station is.
    pub stops: Vec<f32>,
    /// Cabins on the rope (a gondola's loop; a tramway always has two).
    pub cabins: u32,
    #[serde(default = "default_open")]
    pub open: u32,
    #[serde(default = "default_close")]
    pub close: u32,
    pub fare: f32,
    /// Gondola: where cabin 0 is round the loop (virtual metres, see
    /// [`CableLine::loop_layout`]). Tramway: ticks into the shuttle's cycle.
    #[serde(default)]
    pub phase: f32,
    /// The rope is moving.
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub riders_today: u32,
    #[serde(default)]
    pub riders_yesterday: u32,
    /// Riders a full cabin left on the platform today and yesterday.
    #[serde(default)]
    pub left_behind_today: u32,
    #[serde(default)]
    pub left_behind_yesterday: u32,
    /// Seats that passed through a station while the line ran today, and
    /// those taken, for "capacity used".
    #[serde(default)]
    pub seats_today: u32,
    #[serde(default)]
    pub seats_yesterday: u32,
    #[serde(default)]
    pub seated_today: u32,
    #[serde(default)]
    pub seated_yesterday: u32,
    /// Fares taken from visitors riding for the view.
    #[serde(default)]
    pub sightseers_today: u32,
    #[serde(default)]
    pub sightseers_yesterday: u32,
    /// `0..1`: how scenic a ride it is (height, water, a hilltop at the end).
    #[serde(default)]
    pub scenic: f32,
    /// What it cost to build.
    #[serde(default)]
    pub cost: f32,
    #[serde(default)]
    pub built_tick: u64,
    #[serde(default)]
    pub timeline: Timeline,
    /// Who is aboard. Not saved: after a load riders are set down at a
    /// station and go on from there (`World::cable_after_load`).
    #[serde(skip)]
    pub aboard: Vec<Aboard>,
}

fn default_open() -> u32 {
    OPEN
}

fn default_close() -> u32 {
    CLOSE
}

/// A cabin's place: where it is (rope side included), how high its floor
/// is, which way it faces, and whether it is in a station (which one, by
/// index) or out on a span.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CabinPose {
    pub pos: Vec2,
    pub z: f32,
    pub heading: f32,
    pub station: Option<u16>,
    /// Going from the first station towards the last.
    pub forward: bool,
    /// Moving at full speed (out on a span), `0..1`: how fast relative to it.
    pub pace: f32,
}

/// One piece of a gondola's loop, in virtual metres (a station counts as
/// the metres the rope would run while a cabin crawls through it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoopPart {
    /// Through station `i` the way the cabins are going (`forward`); a
    /// terminal (`turn`) is round its bullwheel.
    Station { i: u16, forward: bool, turn: bool },
    /// Out on the line from station `i` to `i + 1` (forward) or back.
    Span { i: u16, forward: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoopLayout {
    /// Each part, where it starts round the loop and how long it is.
    pub parts: Vec<(LoopPart, f32, f32)>,
    pub length: f32,
}

impl LoopLayout {
    /// The part at loop position `u` and how far into it.
    pub fn at(&self, u: f32) -> (LoopPart, f32, f32) {
        let u = u.rem_euclid(self.length.max(1.0));
        let k = self.parts.partition_point(|p| p.1 <= u).saturating_sub(1);
        let (part, start, len) = self.parts[k.min(self.parts.len() - 1)];
        (part, (u - start).max(0.0), len)
    }
}

/// The line's horizontal path: station to station.
pub fn path_of(points: &[Vec2]) -> Vec<f32> {
    let mut s = Vec::with_capacity(points.len());
    let mut acc = 0.0;
    for (k, p) in points.iter().enumerate() {
        if k > 0 {
            acc += points[k - 1].distance(*p);
        }
        s.push(acc);
    }
    s
}

impl CableLine {
    pub fn spec(&self) -> &'static Spec {
        self.kind.spec()
    }

    pub fn length(&self) -> f32 {
        self.stops.last().copied().unwrap_or(0.0)
    }

    pub fn index_of(&self, s: CableStationId) -> Option<usize> {
        self.stations.iter().position(|&x| x == s)
    }

    pub fn pylons(&self) -> impl Iterator<Item = &Support> {
        self.supports.iter().filter(|s| s.station.is_none())
    }

    /// The station supports in order (their positions).
    fn station_points(&self) -> Vec<Vec2> {
        self.supports.iter().filter(|s| s.station.is_some()).map(|s| s.pos).collect()
    }

    /// In service at minute `m` of the day.
    pub fn open_at(&self, m: u32) -> bool {
        let m = m % 1440;
        if self.open <= self.close {
            (self.open..self.close).contains(&m)
        } else {
            m >= self.open || m < self.close
        }
    }

    /// Closed for the night at least `grace` minutes ago.
    pub fn closed_for(&self, m: u32, grace: u32) -> bool {
        !self.open_at(m) && (m % 1440 + 1440 - self.close) % 1440 >= grace
    }

    /// The point `s` metres along the line, `side` metres to the right of
    /// the way forward, and the way forward there.
    pub fn point_at(&self, s: f32, side: f32) -> (Vec2, Vec2) {
        let pts = self.station_points();
        if pts.len() < 2 {
            return (pts.first().copied().unwrap_or_default(), Vec2::X);
        }
        let s = s.clamp(0.0, self.length());
        let k = self.stops.partition_point(|x| *x <= s).clamp(1, self.stops.len() - 1) - 1;
        let (a, b) = (pts[k], pts[k + 1]);
        let len = (self.stops[k + 1] - self.stops[k]).max(0.01);
        let dir = (b - a).normalize_or_zero();
        let right = Vec2::new(-dir.y, dir.x);
        (a.lerp(b, ((s - self.stops[k]) / len).clamp(0.0, 1.0)) + right * side, dir)
    }

    /// Height of the rope `s` metres along the line: sagging between each
    /// pair of supports.
    pub fn rope_z(&self, s: f32) -> f32 {
        rope_z(&self.supports, self.spec().sag, s)
    }

    /// The loop a gondola's cabins go round: through the first station
    /// round its bullwheel, out along every span and through every station
    /// to the last, round it, and back.
    pub fn loop_layout(&self) -> LoopLayout {
        let spec = self.spec();
        let crawl = spec.speed * spec.station_seconds;
        let n = self.stations.len();
        let mut parts = Vec::with_capacity(4 * n);
        let mut at = 0.0;
        let mut push = |p: LoopPart, len: f32, parts: &mut Vec<(LoopPart, f32, f32)>| {
            parts.push((p, at, len));
            at += len;
        };
        if n < 2 {
            return LoopLayout { parts: vec![(LoopPart::Station { i: 0, forward: true, turn: true }, 0.0, crawl)], length: crawl };
        }
        push(LoopPart::Station { i: 0, forward: true, turn: true }, crawl, &mut parts);
        for i in 0..n - 1 {
            let span = (self.stops[i + 1] - self.stops[i] - 2.0 * spec.station_half).max(1.0);
            push(LoopPart::Span { i: i as u16, forward: true }, span, &mut parts);
            let last = i + 1 == n - 1;
            push(LoopPart::Station { i: (i + 1) as u16, forward: true, turn: last }, crawl, &mut parts);
        }
        for i in (0..n - 1).rev() {
            let span = (self.stops[i + 1] - self.stops[i] - 2.0 * spec.station_half).max(1.0);
            push(LoopPart::Span { i: i as u16, forward: false }, span, &mut parts);
            if i > 0 {
                push(LoopPart::Station { i: i as u16, forward: false, turn: false }, crawl, &mut parts);
            }
        }
        LoopLayout { parts, length: at }
    }

    /// Gondola: metres of rope between cabins.
    pub fn spacing(&self) -> f32 {
        if self.kind != CableKind::Gondola {
            return 0.0;
        }
        self.loop_layout().length / self.cabins.max(1) as f32
    }

    /// Where cabin `k` is round the loop now (gondola), virtual metres.
    pub fn cabin_u(&self, k: u32, layout: &LoopLayout) -> f32 {
        (self.phase + k as f32 * layout.length / self.cabins.max(1) as f32).rem_euclid(layout.length.max(1.0))
    }

    /// The tramway's cycle: ticks standing at a station, ticks under way,
    /// and the whole round trip.
    pub fn shuttle_ticks(&self, seconds_per_tick: f32) -> (f32, f32, f32) {
        let spec = self.spec();
        let secs = seconds_per_tick.max(0.1);
        let dwell = spec.station_seconds / secs;
        // Easing in and out costs a little over the cruise.
        let run = (self.length() - 2.0 * spec.station_half).max(1.0) / (spec.speed * secs) * 1.15 + 1.0;
        (dwell, run, 2.0 * (dwell + run))
    }

    /// Where a cabin is now (`phase` as it stands), for the sim and the
    /// renderer alike. `seconds_per_tick` is the world's
    /// `traffic_seconds_per_tick` (for a tramway's cycle).
    pub fn cabin_pose(&self, k: u32, seconds_per_tick: f32) -> CabinPose {
        match self.kind {
            CableKind::Gondola => {
                let layout = self.loop_layout();
                self.loop_pose(&layout, self.cabin_u(k, &layout))
            }
            CableKind::Tramway => self.shuttle_pose(k, self.phase, seconds_per_tick),
        }
    }

    /// A gondola cabin's pose at loop position `u`.
    pub fn loop_pose(&self, layout: &LoopLayout, u: f32) -> CabinPose {
        let spec = self.spec();
        let g = spec.gauge * 0.5;
        let (part, into, len) = layout.at(u);
        let t = (into / len.max(0.01)).clamp(0.0, 1.0);
        match part {
            LoopPart::Span { i, forward } => {
                let (a, b) = (self.stops[i as usize] + spec.station_half, self.stops[i as usize + 1] - spec.station_half);
                let s = if forward { a + (b - a) * t } else { b - (b - a) * t };
                let side = if forward { g } else { -g };
                let (p, dir) = self.point_at(s, side);
                let d = if forward { dir } else { -dir };
                CabinPose { pos: p, z: self.rope_z(s) - spec.hanger, heading: d.y.atan2(d.x), station: None, forward, pace: 1.0 }
            }
            LoopPart::Station { i, forward, turn } => self.station_pose(i, forward, turn, t),
        }
    }

    /// A cabin crawling through station `i`, `t` of the way through: in
    /// along the platform, round the bullwheel at a terminal, out again.
    fn station_pose(&self, i: u16, forward: bool, turn: bool, t: f32) -> CabinPose {
        let spec = self.spec();
        let g = spec.gauge * 0.5;
        let s0 = self.stops[i as usize];
        let h = spec.station_half;
        let floor = self.supports.iter().find(|x| x.station == Some(i)).map_or(0.0, |x| x.top - spec.hanger);
        // Slow in the middle of the station, at rope speed at its mouths.
        let pace = 0.25 + 0.75 * (2.0 * t - 1.0).abs();
        if !turn {
            let (from, to) = if forward { (s0 - h, s0 + h) } else { (s0 + h, s0 - h) };
            let side = if forward { g } else { -g };
            // Along the chord between the mouths (an angle station turns
            // inside its hall).
            let (a, b) = (self.point_at(from, side).0, self.point_at(to, side).0);
            let d = (b - a).normalize_or_zero();
            return CabinPose { pos: a.lerp(b, smooth(t)), z: floor, heading: d.y.atan2(d.x), station: Some(i), forward, pace };
        }
        // A terminal: in along the platform on one side, round the
        // bullwheel at the back, out along the other side. `out` points
        // from the station into the line; cabins leave on its right.
        let pts = self.station_points();
        let first = i == 0;
        let me = pts[i as usize];
        let other = if first { pts.get(1) } else { pts.get(i as usize - 1) }.copied().unwrap_or(me + Vec2::X);
        let out = (other - me).normalize_or_zero();
        let right = Vec2::new(-out.y, out.x);
        let wheel = me - out * (h * 0.45);
        let mouth = me + out * h;
        let straight = mouth.distance(wheel);
        let arc = std::f32::consts::PI * g;
        let total = 2.0 * straight + arc;
        let d = t * total;
        let (p, dir) = if d < straight {
            let k = d / straight;
            ((mouth - right * g).lerp(wheel - right * g, k), -out)
        } else if d < straight + arc {
            // Round the back of the wheel, from the left side to the right.
            let a = (d - straight) / g;
            let v = -right * a.cos() - out * a.sin();
            let tangent = right * a.sin() - out * a.cos();
            (wheel + v * g, tangent)
        } else {
            let k = (d - straight - arc) / straight;
            ((wheel + right * g).lerp(mouth + right * g, k), out)
        };
        CabinPose { pos: p, z: floor, heading: dir.y.atan2(dir.x), station: Some(i), forward: first, pace }
    }

    /// A tramway cabin `k` (0 or 1) at cycle time `phase` (ticks).
    pub fn shuttle_pose(&self, k: u32, phase: f32, seconds_per_tick: f32) -> CabinPose {
        let spec = self.spec();
        let (dwell, run, period) = self.shuttle_ticks(seconds_per_tick);
        let t = (phase + if k % 2 == 1 { period * 0.5 } else { 0.0 }).rem_euclid(period);
        // Each cabin keeps its own track: cabin 0 on the right going out.
        let side = if k % 2 == 0 { spec.gauge * 0.5 } else { -spec.gauge * 0.5 };
        let n = self.stops.len();
        if n < 2 {
            return CabinPose { pos: self.supports.first().map_or(Vec2::ZERO, |s| s.pos), z: 0.0, heading: 0.0, station: Some(0), forward: true, pace: 0.0 };
        }
        let (first, last) = (0u16, (n - 1) as u16);
        // Both run the same cycle half a cycle apart: cabin 0 starts at
        // the first station, cabin 1 (half way round) at the last.
        let (home, away) = (first, last);
        let floor = |i: u16| self.supports.iter().find(|x| x.station == Some(i)).map_or(0.0, |x| x.top - spec.hanger);
        let s_of = |i: u16| if i == 0 { spec.station_half * 0.3 } else { self.length() - spec.station_half * 0.3 };
        let leg = |from: u16, to: u16, x: f32| {
            let e = smooth(x);
            let (a, b) = (s_of(from), s_of(to));
            let s = a + (b - a) * e;
            let fwd = to > from;
            let (p, dir) = self.point_at(s, side);
            let d = if fwd { dir } else { -dir };
            // Easing out of the station: the floor eases onto the rope.
            let mouth = spec.station_half * 1.4;
            let near = ((s - a).abs().min((b - s).abs()) / mouth).clamp(0.0, 1.0);
            let rope = self.rope_z(s) - spec.hanger;
            let plat = if (s - a).abs() < (b - s).abs() { floor(from) } else { floor(to) };
            let z = plat + (rope - plat) * smooth(near);
            let pace = (4.0 * x * (1.0 - x)).clamp(0.0, 1.0).sqrt();
            CabinPose { pos: p, z, heading: d.y.atan2(d.x), station: None, forward: fwd, pace }
        };
        let at = |i: u16| {
            let (p, dir) = self.point_at(s_of(i), side);
            let d = if i == 0 { dir } else { -dir };
            CabinPose { pos: p, z: floor(i), heading: d.y.atan2(d.x), station: Some(i), forward: i == 0, pace: 0.0 }
        };
        if t < dwell {
            at(home)
        } else if t < dwell + run {
            leg(home, away, (t - dwell) / run)
        } else if t < 2.0 * dwell + run {
            at(away)
        } else {
            leg(away, home, (t - 2.0 * dwell - run) / run)
        }
    }

    /// Minutes of clock a ride from station `a` to `b` takes (indices).
    pub fn ride_minutes(&self, a: usize, b: usize, seconds_per_tick: f32) -> f32 {
        let spec = self.spec();
        let secs = seconds_per_tick.max(0.1);
        let dist = (self.stops.get(b).copied().unwrap_or(0.0) - self.stops.get(a).copied().unwrap_or(0.0)).abs();
        match self.kind {
            CableKind::Gondola => {
                let stations = a.abs_diff(b) as f32;
                dist / (spec.speed * secs) + stations * spec.station_seconds / secs
            }
            CableKind::Tramway => self.shuttle_ticks(secs).1,
        }
    }

    /// Minutes a rider expects to wait for a cabin (with nobody ahead).
    pub fn expected_wait(&self, seconds_per_tick: f32) -> f32 {
        let secs = seconds_per_tick.max(0.1);
        match self.kind {
            CableKind::Gondola => (self.spacing() / (self.spec().speed * secs) * 0.5).max(0.5),
            CableKind::Tramway => self.shuttle_ticks(secs).2 * 0.25 + self.shuttle_ticks(secs).0 * 0.5,
        }
    }

    /// Minutes between cabins leaving a station in one direction.
    pub fn interval(&self, seconds_per_tick: f32) -> f32 {
        let secs = seconds_per_tick.max(0.1);
        match self.kind {
            CableKind::Gondola => self.spacing() / (self.spec().speed * secs),
            CableKind::Tramway => self.shuttle_ticks(secs).2,
        }
    }

    /// People an hour the line carries each way at most.
    pub fn capacity_per_hour(&self, seconds_per_tick: f32) -> f32 {
        let iv = self.interval(seconds_per_tick).max(0.1);
        60.0 / iv * self.spec().seats as f32
    }

    /// The rope moves this far (gondola, virtual metres) or the cycle this
    /// many ticks (tramway) in one tick.
    pub fn advance_per_tick(&self, seconds_per_tick: f32) -> f32 {
        match self.kind {
            CableKind::Gondola => self.spec().speed * seconds_per_tick.max(0.1),
            CableKind::Tramway => 1.0,
        }
    }

    /// Riders in cabin `k` now.
    pub fn in_cabin(&self, k: u32) -> usize {
        self.aboard.iter().filter(|a| a.cabin == k).count()
    }

    /// Upkeep a day: stations and their staff, rope, cabins.
    pub fn upkeep_per_day(&self) -> f32 {
        let spec = self.spec();
        self.stations.len() as f32 * spec.station_upkeep + self.length() / 1000.0 * spec.upkeep_per_km + self.cabins as f32 * spec.cabin_upkeep
    }

    pub fn roll_day(&mut self) {
        self.riders_yesterday = std::mem::take(&mut self.riders_today);
        self.left_behind_yesterday = std::mem::take(&mut self.left_behind_today);
        self.seats_yesterday = std::mem::take(&mut self.seats_today);
        self.seated_yesterday = std::mem::take(&mut self.seated_today);
        self.sightseers_yesterday = std::mem::take(&mut self.sightseers_today);
    }

    /// Share of the seats that passed through stations taken, yesterday
    /// (today while there is no yesterday yet).
    pub fn capacity_used(&self) -> f32 {
        let (seats, seated) = if self.seats_yesterday > 0 { (self.seats_yesterday, self.seated_yesterday) } else { (self.seats_today, self.seated_today) };
        if seats == 0 {
            0.0
        } else {
            seated as f32 / seats as f32
        }
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The rope's height `s` metres along a line hung on `supports` (in order
/// of `s`): a parabola between each pair, dipping `sag` of the span.
pub fn rope_z(supports: &[Support], sag: f32, s: f32) -> f32 {
    if supports.is_empty() {
        return 0.0;
    }
    let k = supports.partition_point(|x| x.s <= s);
    if k == 0 {
        return supports[0].top;
    }
    if k >= supports.len() {
        return supports[supports.len() - 1].top;
    }
    let (a, b) = (supports[k - 1], supports[k]);
    let span = (b.s - a.s).max(0.01);
    let t = ((s - a.s) / span).clamp(0.0, 1.0);
    a.top + (b.top - a.top) * t - 4.0 * sag * span * t * (1.0 - t)
}

/// What stands under the line at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Below {
    Ground,
    Water,
    Road,
    Building(BuildingId),
    Rail,
}

/// What the placement needs to know of the city (`World` gives it; tests
/// give a toy).
pub trait Terrain {
    /// The ground at `p` (absolute, m).
    fn ground(&self, p: Vec2) -> f32;
    /// The top of whatever is highest at `p` and what it is.
    fn top(&self, p: Vec2) -> (f32, Below);
    /// Why a pylon cannot stand at `p` (`None`: it can).
    fn pylon_refusal(&self, p: Vec2) -> Option<&'static str>;
    /// A name for what is below, for messages.
    fn name(&self, b: Below) -> String;
}

/// The rope must be this far above the top of what is there.
fn needed_over(spec: &Spec, top: f32, below: Below, ramp: f32) -> f32 {
    let margin = ramp
        * match below {
        Below::Water => WATER_CLEARANCE,
        Below::Road | Below::Rail => ROAD_CLEARANCE,
        Below::Ground => GROUND_CLEARANCE,
            Below::Building(_) => CLEARANCE,
        };
    // Never into a roof, whatever the ramp.
    let floor = if matches!(below, Below::Building(_)) { 1.0 } else { 0.0 };
    top + margin.max(floor) + spec.hanger
}

/// Metres beyond a station's mouth over which the clearance rises to the
/// full: cabins leave a station low, over its own forecourt, and climb.
pub const STATION_RAMP: f32 = 45.0;

/// One span laid: the pylons between two stations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpanLaid {
    pub pylons: Vec<Support>,
    /// The highest the cabins pass over the ground on this span, m.
    pub max_above_ground: f32,
    /// Share of the span over water.
    pub over_water: f32,
}

/// Lay the pylons on the span from `a` (rope at `za`) to `b` (rope at
/// `zb`), `s0` metres along the line at `a`. Each pylon goes as far on as
/// it can (fewer pylons) without growing much taller; the rope sagging
/// between supports clears what is below. Refused with the reason when no
/// pylon can stand where one is needed, one would be too tall, or the
/// climb too steep.
pub fn lay_span(a: Vec2, za: f32, b: Vec2, zb: f32, s0: f32, spec: &Spec, land: &dyn Terrain, names: (&str, &str)) -> Result<SpanLaid, String> {
    let len = a.distance(b);
    if len < MIN_STATION_GAP {
        return Err(format!("{} and {} are only {:.0} m apart (stations need {:.0} m)", names.0, names.1, len, MIN_STATION_GAP));
    }
    let dir = (b - a) / len;
    let n = (len / SAMPLE_STEP).ceil().max(2.0) as usize;
    let skip = spec.station_half + 6.0;
    // What the rope must clear along the span.
    let mut req = vec![f32::NEG_INFINITY; n + 1];
    let mut what = vec![Below::Ground; n + 1];
    let mut water = 0usize;
    let mut ground_at = vec![0.0f32; n + 1];
    for k in 0..=n {
        let s = len * k as f32 / n as f32;
        let p = a + dir * s;
        let (top, below) = land.top(p);
        ground_at[k] = land.ground(p);
        if below == Below::Water {
            water += 1;
        }
        what[k] = below;
        if s > skip && s < len - skip {
            let near = (s - skip).min(len - skip - s);
            req[k] = needed_over(spec, top, below, (near / STATION_RAMP).clamp(0.0, 1.0));
        }
    }
    let s_of = |k: usize| len * k as f32 / n as f32;
    let k_of = |s: f32| ((s / len) * n as f32).round().clamp(0.0, n as f32) as usize;
    let grade_ok = |z0: f32, z1: f32, run: f32| (z1 - z0).abs() / run.max(1.0) <= spec.max_grade;
    // Does the rope hang from (sa, za) to (sb, zb) clear of everything?
    let clear = |sa: f32, za: f32, sb: f32, zb: f32| -> bool {
        let span = sb - sa;
        if span <= 0.0 || span > spec.max_span + 0.01 || !grade_ok(za, zb, span) {
            return false;
        }
        let (ka, kb) = (k_of(sa), k_of(sb));
        for k in ka + 1..kb {
            let r = req[k];
            if !r.is_finite() {
                continue;
            }
            let t = (s_of(k) - sa) / span;
            let z = za + (zb - za) * t - 4.0 * spec.sag * span * t * (1.0 - t);
            if z < r - 0.05 {
                return false;
            }
        }
        true
    };
    // Every place a pylon might stand, at a few heights each (as low as
    // what stands at its foot allows, up to the tallest it may be).
    const HEIGHTS: usize = 10;
    let step = if spec.max_span > 1000.0 { 15.0 } else { PYLON_STEP };
    // (s, top, ground); the stations first and last.
    let mut nodes: Vec<(f32, f32, f32)> = vec![(0.0, za, ground_at[0])];
    let mut refused: Vec<(f32, &'static str)> = Vec::new();
    let mut c = PYLON_STATION_GAP;
    while c <= len - PYLON_STATION_GAP {
        let p = a + dir * c;
        match land.pylon_refusal(p) {
            Some(why) => refused.push((c, why)),
            None => {
                let g = land.ground(p);
                let (k0, k1) = (k_of(c - 6.0), k_of(c + 6.0));
                let here = (k0..=k1).map(|k| req[k]).filter(|r| r.is_finite()).fold(f32::NEG_INFINITY, f32::max);
                let lo = (g + spec.min_pylon).max(here + 0.5);
                let hi = g + spec.max_pylon;
                if lo <= hi {
                    for h in 0..HEIGHTS {
                        nodes.push((c, lo + (hi - lo) * h as f32 / (HEIGHTS - 1) as f32, g));
                    }
                } else {
                    refused.push((c, "what stands beside it is taller than a pylon"));
                }
            }
        }
        c += step;
    }
    let end = nodes.len();
    nodes.push((len, zb, ground_at[n]));
    // The cheapest way along: few pylons, low ones (a pylon costs as much
    // as twelve metres of height).
    let pylon_cost = |i: usize| if i == end { 0.0 } else { 120.0 + 10.0 * (nodes[i].1 - nodes[i].2) };
    let mut cost = vec![f32::INFINITY; nodes.len()];
    let mut prev = vec![usize::MAX; nodes.len()];
    cost[0] = 0.0;
    for i in 1..nodes.len() {
        let (si, zi, _) = nodes[i];
        let mut best = (f32::INFINITY, usize::MAX);
        let mut j = i;
        while j > 0 {
            j -= 1;
            let (sj, zj, _) = nodes[j];
            if si - sj > spec.max_span {
                break;
            }
            // Pylons stand apart (a station's neighbour is held off by the gap).
            if j != 0 && i != end && si - sj < MIN_PYLON_GAP {
                continue;
            }
            if !cost[j].is_finite() || cost[j] >= best.0 {
                continue;
            }
            if clear(sj, zj, si, zi) {
                best = (cost[j], j);
            }
        }
        if best.1 != usize::MAX {
            cost[i] = best.0 + pylon_cost(i);
            prev[i] = best.1;
        }
    }
    if !cost[end].is_finite() {
        // How far it got, and what stopped it there.
        let reach = (0..end).filter(|&i| cost[i].is_finite()).map(|i| nodes[i].0).fold(0.0f32, f32::max);
        let (from, to) = (reach + if reach > 0.0 { MIN_PYLON_GAP } else { PYLON_STATION_GAP }, (reach + spec.max_span).min(len - PYLON_STATION_GAP));
        let standing = nodes.iter().skip(1).take(end - 1).any(|nd| nd.0 > reach && nd.0 <= to);
        let kind = spec_label(spec);
        if !standing {
            let mut why: Vec<(&str, u32)> = Vec::new();
            for (s, r) in &refused {
                if *s >= from - 1.0 && *s <= to + 1.0 {
                    match why.iter_mut().find(|w| w.0 == *r) {
                        Some(w) => w.1 += 1,
                        None => why.push((r, 1)),
                    }
                }
            }
            why.sort_by(|x, y| y.1.cmp(&x.1));
            let why: Vec<&str> = why.iter().map(|w| w.0).collect();
            let further = if spec.max_stations > 2 { " an aerial tramway spans further, or" } else { "" };
            return Err(format!(
                "no pylon can stand between {:.0} m and {:.0} m out of {} ({}): a {kind} hangs at most {:.0} m between supports;{further} add a station",
                from.min(to),
                to,
                names.0,
                if why.is_empty() { "nowhere to stand".to_string() } else { why.join(", ") },
                spec.max_span
            ));
        }
        // Sites, but none tall enough (or the climb too steep).
        let (k0, k1) = (k_of(reach), k_of((reach + spec.max_span).min(len)));
        let worst = (k0..=k1).filter(|&k| req[k].is_finite()).max_by(|&x, &y| (req[x] - ground_at[x]).total_cmp(&(req[y] - ground_at[y])));
        let rise = (ground_at[k1] - ground_at[k0]).abs() / (s_of(k1) - s_of(k0)).max(1.0);
        if rise > spec.max_grade * 0.8 {
            return Err(format!("the climb from {} to {} is too steep for a {kind} (at most {:.0}%)", names.0, names.1, spec.max_grade * 100.0));
        }
        let (what_name, at, h) = match worst {
            Some(k) => (land.name(what[k]), s_of(k), req[k] - ground_at[k] + 4.0 * spec.sag * spec.max_span * 0.25),
            None => (land.name(Below::Ground), reach, spec.max_pylon + 1.0),
        };
        return Err(format!(
            "clearing {what_name} {at:.0} m out of {} needs a pylon over {:.0} m tall (about {h:.0} m; a {kind} pylon is at most {:.0} m): put a station on a roof or a hill, or go round it",
            names.0,
            spec.max_pylon,
            spec.max_pylon
        ));
    }
    // The way back from the far station.
    let mut path = vec![end];
    while let Some(&i) = path.last() {
        if i == 0 {
            break;
        }
        path.push(prev[i]);
    }
    path.reverse();
    let mut chosen: Vec<(f32, f32, f32)> = path.iter().map(|&i| nodes[i]).collect();
    // Each pylon down as far as its neighbours let it go.
    for _ in 0..2 {
        for q in 1..chosen.len() - 1 {
            let (s, top, g) = chosen[q];
            let (k0, k1) = (k_of(s - 6.0), k_of(s + 6.0));
            let here = (k0..=k1).map(|k| req[k]).filter(|r| r.is_finite()).fold(f32::NEG_INFINITY, f32::max);
            let floor = (g + spec.min_pylon).max(here + 0.5);
            let (pa, pb) = (chosen[q - 1], chosen[q + 1]);
            let ok = |z: f32| clear(pa.0, pa.1, s, z) && clear(s, z, pb.0, pb.1);
            let (mut lo, mut hi) = (floor, top);
            if ok(lo) {
                hi = lo;
            } else {
                for _ in 0..12 {
                    let mid = (lo + hi) * 0.5;
                    if ok(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
            }
            chosen[q].1 = hi;
        }
    }
    let pylons: Vec<Support> = chosen[1..chosen.len() - 1].iter().map(|&(s, top, g)| Support { pos: a + dir * s, s: s0 + s, base: g, top, station: None }).collect();
    // How high the ride is: the cabin floor over the ground, sampled.
    let mut all: Vec<Support> = Vec::with_capacity(pylons.len() + 2);
    all.push(Support { pos: a, s: 0.0, base: ground_at[0], top: za, station: Some(0) });
    all.extend(pylons.iter().map(|p| Support { s: p.s - s0, ..*p }));
    all.push(Support { pos: b, s: len, base: ground_at[n], top: zb, station: Some(1) });
    let mut max_above = 0.0f32;
    for k in 0..=n {
        let z = rope_z(&all, spec.sag, s_of(k)) - spec.hanger;
        max_above = max_above.max(z - ground_at[k]);
    }
    Ok(SpanLaid { pylons, max_above_ground: max_above, over_water: water as f32 / (n + 1) as f32 })
}

fn spec_label(spec: &Spec) -> &'static str {
    if spec.max_stations == 2 {
        "aerial tramway"
    } else {
        "gondola"
    }
}

/// A line laid through its stations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineLaid {
    pub supports: Vec<Support>,
    pub stops: Vec<f32>,
    pub max_above_ground: f32,
    pub over_water: f32,
}

/// Lay a line of `kind` through stations at `points`, their platform
/// floors at `floors` (absolute): the rope at each station `hanger` above
/// its floor, pylons along every span. `names` names the stations for the
/// messages.
pub fn lay_line(kind: CableKind, points: &[Vec2], floors: &[f32], land: &dyn Terrain, names: &[String]) -> Result<LineLaid, String> {
    let spec = kind.spec();
    if points.len() < 2 {
        return Err("a line needs two stations".into());
    }
    if points.len() > spec.max_stations {
        return Err(match kind {
            CableKind::Tramway => "an aerial tramway runs between two stations only (a gondola calls at more)".into(),
            CableKind::Gondola => format!("a gondola calls at {} stations at most", spec.max_stations),
        });
    }
    let stops = path_of(points);
    let total = stops.last().copied().unwrap_or(0.0);
    if total > spec.max_length {
        return Err(format!("{:.1} km of line is longer than a {} runs ({:.1} km)", total / 1000.0, kind.label(), spec.max_length / 1000.0));
    }
    // An angle station turns the line, but not back on itself.
    for k in 1..points.len().saturating_sub(1) {
        let (d0, d1) = ((points[k] - points[k - 1]).normalize_or_zero(), (points[k + 1] - points[k]).normalize_or_zero());
        if d0.dot(d1) < -0.5 {
            return Err(format!("the line turns back too sharply at {} (more than 120 degrees)", names.get(k).map_or("a station", |s| s.as_str())));
        }
    }
    let mut supports = Vec::new();
    let mut max_above = 0.0f32;
    let mut water = 0.0;
    for k in 0..points.len() - 1 {
        let za = floors[k] + spec.hanger;
        let zb = floors[k + 1] + spec.hanger;
        let (na, nb) = (names.get(k).map_or("one station", |s| s.as_str()), names.get(k + 1).map_or("the next", |s| s.as_str()));
        let laid = lay_span(points[k], za, points[k + 1], zb, stops[k], spec, land, (na, nb))?;
        supports.push(Support { pos: points[k], s: stops[k], base: land.ground(points[k]), top: za, station: Some(k as u16) });
        supports.extend(laid.pylons);
        max_above = max_above.max(laid.max_above_ground);
        water += laid.over_water * (stops[k + 1] - stops[k]);
    }
    let last = points.len() - 1;
    supports.push(Support { pos: points[last], s: stops[last], base: land.ground(points[last]), top: floors[last] + spec.hanger, station: Some(last as u16) });
    Ok(LineLaid { supports, stops, max_above_ground: max_above, over_water: water / total.max(1.0) })
}

/// What it costs to build a line hung on `supports` (stations not counted).
pub fn works_cost(spec: &Spec, supports: &[Support], length: f32) -> f32 {
    let pylons: f32 = supports.iter().filter(|s| s.station.is_none()).map(|s| spec.pylon_cost + spec.pylon_cost_per_m * s.height()).sum();
    length * spec.cost_per_m + pylons
}

/// Cabins a new gondola line gets at the default spacing.
pub fn default_cabins(kind: CableKind, loop_len: f32) -> u32 {
    match kind {
        CableKind::Gondola => (loop_len / GONDOLA.spacing).round().max(4.0) as u32,
        CableKind::Tramway => 2,
    }
}

/// The cabin count a gondola may run on a loop this long.
pub fn cabin_limits(kind: CableKind, loop_len: f32) -> (u32, u32) {
    match kind {
        CableKind::Gondola => ((loop_len / GONDOLA.max_spacing).ceil().max(2.0) as u32, (loop_len / GONDOLA.min_spacing).floor().max(2.0) as u32),
        CableKind::Tramway => (2, 2),
    }
}

/// How scenic a ride is (`0..1`): high over the town, out over water, up
/// to a summit well above the rest (`summit`: metres the highest station
/// stands over the lowest).
pub fn scenic_score(max_above_ground: f32, over_water: f32, summit: f32) -> f32 {
    let height = (max_above_ground / 45.0).clamp(0.0, 1.0);
    let water = (over_water * 2.5).clamp(0.0, 1.0);
    let hill = (summit / 60.0).clamp(0.0, 1.0);
    (0.4 * height + 0.25 * water + 0.35 * hill).clamp(0.0, 1.0)
}

/// What the line tool shows before a line is laid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CablePreview {
    pub kind: CableKind,
    pub stations: usize,
    /// Stations that would be built (the rest exist already).
    pub new_stations: usize,
    pub supports: Vec<Support>,
    pub length_m: f32,
    pub pylons: usize,
    pub tallest_pylon_m: f32,
    pub max_above_ground_m: f32,
    pub over_water: f32,
    pub ride_min: f32,
    pub cabins: u32,
    pub interval_min: f32,
    pub capacity_per_hour: f32,
    pub build_cost: f32,
    pub upkeep_per_day: f32,
    pub scenic: f32,
    /// Why it cannot be built (the rest is what could be worked out).
    pub refusal: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CableStats {
    pub lines: u32,
    pub stations: u32,
    pub cabins: u32,
    pub pylons: u32,
    pub waiting: u32,
    pub riding: u32,
    pub riders_today: u32,
    pub riders_yesterday: u32,
    pub fares_today: f32,
    pub fares_yesterday: f32,
    pub upkeep_yesterday: f32,
    pub rope_m: f32,
}

/// Every cable line and station.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CableWays {
    #[serde(default)]
    pub stations: SlotMap<CableStationId, CableStation>,
    #[serde(default)]
    pub lines: SlotMap<CableLineId, CableLine>,
    #[serde(default)]
    pub stats: CableStats,
    #[serde(default)]
    pub next_line: u32,
    /// Bumped on every change to the stations or lines (the renderer
    /// redraws the static parts only then).
    #[serde(skip)]
    pub version: u64,
    /// [cable] How tall a building may grow under a line, per lot (m over
    /// the lot's ground); rebuilt with the lines.
    #[serde(skip)]
    pub corridor: std::collections::HashMap<crate::zoning::LotId, f32>,
}

/// A ride the planner found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CablePlan {
    pub line: CableLineId,
    pub board: CableStationId,
    pub alight: CableStationId,
    pub entrance: Vec2,
    pub exit: Vec2,
    pub minutes_riding: f32,
    pub minutes_waiting: f32,
}

impl CableWays {
    pub fn lines_at(&self, s: CableStationId) -> impl Iterator<Item = (CableLineId, &CableLine)> {
        self.lines.iter().filter(move |(_, l)| l.stations.contains(&s))
    }

    pub fn station_near(&self, p: Vec2, radius: f32) -> Option<CableStationId> {
        self.stations.iter().map(|(id, s)| (id, s.pos.distance(p).min(s.entrance.distance(p) + 6.0))).filter(|(_, d)| *d <= radius).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    /// The best single-line ride from `from` to `to`: a station within
    /// reach of each end, the walks short enough that riding saves real
    /// distance. `open` says whether the line still runs when the rider
    /// gets to the station (the line and the metres walked to it); `queue`
    /// how many wait at a station already (for the wait).
    #[allow(clippy::too_many_arguments)]
    pub fn plan_trip(&self, from: Vec2, to: Vec2, seconds_per_tick: f32, walk_per_tick: f32, open: impl Fn(&CableLine, f32) -> bool, queue: impl Fn(CableStationId) -> u32) -> Option<CablePlan> {
        let direct = from.distance(to);
        if direct < TRIP_MIN {
            return None;
        }
        let walk = walk_per_tick.max(0.1);
        let mut best: Option<(f32, CablePlan)> = None;
        for (lid, line) in &self.lines {
            if line.stations.len() < 2 || line.stops.len() != line.stations.len() {
                continue;
            }
            let ends: Vec<(usize, Vec2)> = line.stations.iter().enumerate().filter_map(|(i, &s)| self.stations.get(s).map(|st| (i, st.entrance))).collect();
            for &(i, pi) in &ends {
                let d_in = from.distance(pi);
                if d_in > REACH {
                    continue;
                }
                if !open(line, d_in) {
                    break;
                }
                for &(j, pj) in &ends {
                    if i == j {
                        continue;
                    }
                    let d_out = pj.distance(to);
                    if d_out > REACH || d_in + d_out + 120.0 > direct {
                        continue;
                    }
                    let riding = line.ride_minutes(i, j, seconds_per_tick);
                    let q = queue(line.stations[i]) as f32;
                    let wait = line.expected_wait(seconds_per_tick) + (q / line.spec().seats as f32).floor() * line.interval(seconds_per_tick);
                    let cost = (d_in + d_out) / walk + riding + wait;
                    let plan = CablePlan { line: lid, board: line.stations[i], alight: line.stations[j], entrance: pi, exit: pj, minutes_riding: riding, minutes_waiting: wait };
                    if best.as_ref().is_none_or(|(c, _)| cost < *c - 1e-3) {
                        best = Some((cost, plan));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// The day ends: today's counts become yesterday's.
    pub fn roll_day(&mut self) {
        for l in self.lines.values_mut() {
            l.roll_day();
            l.timeline.roll();
        }
        for s in self.stations.values_mut() {
            s.boardings_yesterday = std::mem::take(&mut s.boardings_today);
        }
        let s = &mut self.stats;
        s.riders_yesterday = std::mem::take(&mut s.riders_today);
        s.fares_yesterday = std::mem::take(&mut s.fares_today);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat ground at 0, with boxes standing on it, a strip of water and
    /// spots no pylon may use.
    struct Toy {
        boxes: Vec<(Vec2, Vec2, f32)>,
        water: Option<(f32, f32)>,
        hill: Option<(f32, f32, f32)>,
    }

    impl Toy {
        fn flat() -> Toy {
            Toy { boxes: Vec::new(), water: None, hill: None }
        }
    }

    impl Terrain for Toy {
        fn ground(&self, p: Vec2) -> f32 {
            match self.hill {
                // A ramp from x0 (0 m) to x1 (h m), level beyond.
                Some((x0, x1, h)) => ((p.x - x0) / (x1 - x0)).clamp(0.0, 1.0) * h,
                None => 0.0,
            }
        }
        fn top(&self, p: Vec2) -> (f32, Below) {
            if let Some((x0, x1)) = self.water {
                if (x0..x1).contains(&p.x) {
                    return (0.0, Below::Water);
                }
            }
            for (k, (lo, hi, h)) in self.boxes.iter().enumerate() {
                if p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y {
                    return (self.ground(p) + h, Below::Building(slotmap::KeyData::from_ffi(k as u64 + 1).into()));
                }
            }
            (self.ground(p), Below::Ground)
        }
        fn pylon_refusal(&self, p: Vec2) -> Option<&'static str> {
            match self.top(p).1 {
                Below::Water => Some("water"),
                Below::Building(_) => Some("a building"),
                _ => None,
            }
        }
        fn name(&self, b: Below) -> String {
            match b {
                Below::Water => "the river".into(),
                Below::Building(_) => "a tower".into(),
                _ => "the ground".into(),
            }
        }
    }

    fn names() -> Vec<String> {
        vec!["A".into(), "B".into(), "C".into()]
    }

    fn check_clear(laid: &LineLaid, spec: &Spec, land: &Toy) {
        let total = laid.stops.last().copied().unwrap();
        let mut s = spec.station_half + 7.0;
        while s < total - spec.station_half - 7.0 {
            let p = laid.supports.first().unwrap().pos.lerp(laid.supports.last().unwrap().pos, s / total);
            let (top, below) = land.top(p);
            let cabin_bottom = rope_z(&laid.supports, spec.sag, s) - spec.hanger;
            // Full clearance once clear of the stations' ramps.
            let skip = spec.station_half + 6.0;
            let ramp = (((s - skip).min(total - skip - s)) / STATION_RAMP).clamp(0.0, 1.0);
            let full = if below == Below::Water { WATER_CLEARANCE } else if matches!(below, Below::Building(_)) { CLEARANCE } else { GROUND_CLEARANCE };
            assert!(cabin_bottom + 0.05 >= top + full * ramp - 0.6, "cabin at {s:.0} m ({cabin_bottom:.1}) over {below:?} at {top:.1}");
            s += 2.0;
        }
        for w in laid.supports.windows(2) {
            assert!(w[1].s - w[0].s <= spec.max_span + 0.01, "span {:.0} too long", w[1].s - w[0].s);
        }
        for p in laid.supports.iter().filter(|s| s.station.is_none()) {
            assert!(p.height() <= spec.max_pylon + 0.01 && p.height() >= spec.min_pylon - 0.01, "pylon {:.1} m", p.height());
            assert!(land.pylon_refusal(p.pos).is_none(), "pylon at {:?} stands on something", p.pos);
        }
    }

    #[test]
    fn a_short_flat_gondola_hangs_between_its_stations_and_a_long_one_gets_pylons() {
        let land = Toy::flat();
        let short = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(300.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &land, &names()).unwrap();
        // Low stations: the rope needs pylons to clear the trees even over
        // open ground, but never more than the span limit asks.
        check_clear(&short, &GONDOLA, &land);
        let long = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(1500.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &land, &names()).unwrap();
        check_clear(&long, &GONDOLA, &land);
        let pylons = long.supports.iter().filter(|s| s.station.is_none()).count();
        assert!(pylons >= (1500.0 / GONDOLA.max_span) as usize, "{pylons} pylons over 1.5 km");
        assert!(pylons <= 12, "{pylons} pylons over 1.5 km of open ground is too many");
    }

    #[test]
    fn pylons_grow_to_clear_a_tall_building_and_stand_beside_it() {
        let mut land = Toy::flat();
        land.boxes.push((Vec2::new(380.0, -30.0), Vec2::new(440.0, 30.0), 40.0));
        let laid = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(800.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &land, &names()).unwrap();
        check_clear(&laid, &GONDOLA, &land);
        let tallest = laid.supports.iter().filter(|s| s.station.is_none()).map(|s| s.height()).fold(0.0, f32::max);
        assert!(tallest > 40.0, "a pylon tall enough to clear the 40 m block: {tallest:.0}");
        // The same line over open ground needs only short pylons.
        let open = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(800.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &Toy::flat(), &names()).unwrap();
        let low = open.supports.iter().filter(|s| s.station.is_none()).map(|s| s.height()).fold(0.0, f32::max);
        assert!(low < tallest - 10.0, "open ground {low:.0} m against {tallest:.0} m over the block");
    }

    #[test]
    fn a_wide_river_refuses_a_gondola_with_the_reason_and_a_tramway_spans_it() {
        let mut land = Toy::flat();
        land.water = Some((150.0, 900.0));
        let err = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(1050.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &land, &names()).unwrap_err();
        assert!(err.contains("no pylon can stand") && err.contains("water"), "{err}");
        let tram = lay_line(CableKind::Tramway, &[Vec2::ZERO, Vec2::new(1050.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &land, &names()).unwrap();
        check_clear(&tram, &TRAMWAY, &land);
        assert!(tram.over_water > 0.6, "{}", tram.over_water);
    }

    #[test]
    fn a_narrow_river_is_crossed_by_a_gondola_with_pylons_on_the_banks() {
        let mut land = Toy::flat();
        land.water = Some((300.0, 560.0));
        let laid = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(900.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &land, &names()).unwrap();
        check_clear(&laid, &GONDOLA, &land);
    }

    #[test]
    fn a_block_too_tall_to_clear_is_refused_with_a_pylon_height() {
        let mut land = Toy::flat();
        // 150 m tall and 500 m deep: no gondola pylon reaches over it.
        land.boxes.push((Vec2::new(200.0, -60.0), Vec2::new(700.0, 60.0), 150.0));
        let err = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(900.0, 0.0)], &[GROUND_FLOOR, GROUND_FLOOR], &land, &names()).unwrap_err();
        assert!(err.contains("pylon"), "{err}");
    }

    #[test]
    fn a_gondola_climbs_a_hill_and_a_cliff_is_too_steep() {
        let mut land = Toy::flat();
        land.hill = Some((200.0, 700.0, 120.0));
        let laid = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(900.0, 0.0)], &[GROUND_FLOOR, 120.0 + GROUND_FLOOR], &land, &names()).unwrap();
        check_clear(&laid, &GONDOLA, &land);
        // 200 m up in 130 m along: steeper than a gondola climbs.
        let err = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(130.0, 0.0)], &[GROUND_FLOOR, 200.0], &Toy::flat(), &names()).unwrap_err();
        assert!(err.contains("steep") || err.contains("pylon"), "{err}");
    }

    #[test]
    fn lines_are_refused_for_shape_kind_and_length() {
        let land = Toy::flat();
        let three = [Vec2::ZERO, Vec2::new(400.0, 0.0), Vec2::new(800.0, 0.0)];
        assert!(lay_line(CableKind::Tramway, &three, &[1.0; 3], &land, &names()).unwrap_err().contains("two stations"));
        let back = [Vec2::ZERO, Vec2::new(400.0, 0.0), Vec2::new(30.0, 40.0)];
        assert!(lay_line(CableKind::Gondola, &back, &[1.0; 3], &land, &names()).unwrap_err().contains("turns back"));
        assert!(lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(60.0, 0.0)], &[1.0; 2], &land, &names()).unwrap_err().contains("apart"));
        assert!(lay_line(CableKind::Tramway, &[Vec2::ZERO, Vec2::new(5000.0, 0.0)], &[1.0; 2], &land, &names()).unwrap_err().contains("longer"));
        // An angle station turning 60 degrees is fine.
        let bend = [Vec2::ZERO, Vec2::new(400.0, 0.0), Vec2::new(600.0, 340.0)];
        assert!(lay_line(CableKind::Gondola, &bend, &[1.0; 3], &land, &names()).is_ok());
    }

    fn line_of(kind: CableKind, laid: LineLaid, n: usize) -> CableLine {
        let mut keys: SlotMap<CableStationId, ()> = SlotMap::with_key();
        let stations = (0..n).map(|_| keys.insert(())).collect();
        let mut l = CableLine {
            name: "L".into(),
            kind,
            color: 0,
            stations,
            supports: laid.supports,
            stops: laid.stops,
            cabins: 0,
            open: OPEN,
            close: CLOSE,
            fare: kind.spec().fare,
            phase: 0.0,
            running: true,
            riders_today: 0,
            riders_yesterday: 0,
            left_behind_today: 0,
            left_behind_yesterday: 0,
            seats_today: 0,
            seats_yesterday: 0,
            seated_today: 0,
            seated_yesterday: 0,
            sightseers_today: 0,
            sightseers_yesterday: 0,
            scenic: 0.0,
            cost: 0.0,
            built_tick: 0,
            timeline: Timeline::default(),
            aboard: Vec::new(),
        };
        l.cabins = default_cabins(kind, l.loop_layout().length);
        l
    }

    #[test]
    fn gondola_cabins_go_round_the_loop_through_every_station_and_stay_spaced() {
        let laid = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(500.0, 0.0), Vec2::new(900.0, 200.0)], &[GROUND_FLOOR; 3], &Toy::flat(), &names()).unwrap();
        let mut l = line_of(CableKind::Gondola, laid, 3);
        let layout = l.loop_layout();
        // Three stations: both terminals once, the middle one each way.
        let stations = layout.parts.iter().filter(|p| matches!(p.0, LoopPart::Station { .. })).count();
        assert_eq!(stations, 4);
        assert!((layout.length - (2.0 * (l.length() - 4.0 * GONDOLA.station_half) + 4.0 * GONDOLA.speed * GONDOLA.station_seconds)).abs() < 1.0);
        // Out on a span the cabins are on the rope, under it, spaced.
        let mut last: Option<Vec2> = None;
        for step in 0..(layout.length / 3.0) as usize {
            l.phase = step as f32 * 3.0;
            let p = l.cabin_pose(0, 4.0);
            if p.station.is_none() {
                assert!(p.z > 0.0, "a cabin in the air");
            }
            if let Some(q) = last {
                assert!(p.pos.distance(q) < 15.0, "cabins move smoothly: {:?} to {:?}", q, p.pos);
            }
            last = Some(p.pos);
        }
        assert!(l.capacity_per_hour(4.0) > 300.0, "{}", l.capacity_per_hour(4.0));
        assert!(l.expected_wait(4.0) < 2.0, "{}", l.expected_wait(4.0));
    }

    #[test]
    fn tramway_cabins_shuttle_and_pass_each_other_mid_span() {
        let laid = lay_line(CableKind::Tramway, &[Vec2::ZERO, Vec2::new(1200.0, 0.0)], &[GROUND_FLOOR; 2], &Toy::flat(), &names()).unwrap();
        let mut l = line_of(CableKind::Tramway, laid, 2);
        assert_eq!(l.cabins, 2);
        let (dwell, run, period) = l.shuttle_ticks(4.0);
        l.phase = 0.0;
        assert_eq!(l.cabin_pose(0, 4.0).station, Some(0));
        assert_eq!(l.cabin_pose(1, 4.0).station, Some(1));
        l.phase = dwell + run * 0.5;
        let (a, b) = (l.cabin_pose(0, 4.0), l.cabin_pose(1, 4.0));
        assert!(a.station.is_none() && b.station.is_none());
        assert!((a.pos.x - b.pos.x).abs() < 30.0, "they pass mid-span: {:?} {:?}", a.pos, b.pos);
        assert!((a.pos.y - b.pos.y).abs() > TRAMWAY.gauge * 0.9, "on their own tracks");
        l.phase = dwell + run + 0.5;
        assert_eq!(l.cabin_pose(0, 4.0).station, Some(1));
        assert!(period > 2.0 * run);
        assert!(l.expected_wait(4.0) > l.ride_minutes(0, 1, 4.0) * 0.3);
    }

    #[test]
    fn the_planner_rides_where_it_saves_and_not_for_a_short_hop() {
        let laid = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(1000.0, 0.0)], &[GROUND_FLOOR; 2], &Toy::flat(), &names()).unwrap();
        let mut ways = CableWays::default();
        let mut l = line_of(CableKind::Gondola, laid, 0);
        let a = ways.stations.insert(CableStation { name: "A".into(), pos: Vec2::ZERO, floor: 1.0, ground: 0.0, roof: false, building: None, entrance: Vec2::new(0.0, 10.0), boardings_today: 0, boardings_yesterday: 0, built_tick: 0 });
        let b = ways.stations.insert(CableStation { name: "B".into(), pos: Vec2::new(1000.0, 0.0), floor: 1.0, ground: 0.0, roof: false, building: None, entrance: Vec2::new(1000.0, 10.0), boardings_today: 0, boardings_yesterday: 0, built_tick: 0 });
        l.stations = vec![a, b];
        let id = ways.lines.insert(l);
        let p = ways.plan_trip(Vec2::new(-50.0, 40.0), Vec2::new(1060.0, 30.0), 4.0, 5.6, |_, _| true, |_| 0).expect("a ride");
        assert_eq!((p.line, p.board, p.alight), (id, a, b));
        assert!(ways.plan_trip(Vec2::new(-50.0, 40.0), Vec2::new(150.0, 30.0), 4.0, 5.6, |_, _| true, |_| 0).is_none(), "too short to ride");
        assert!(ways.plan_trip(Vec2::new(-50.0, 40.0), Vec2::new(1060.0, 30.0), 4.0, 5.6, |_, _| false, |_| 0).is_none(), "closed");
        // A long queue makes the wait longer.
        let q = ways.plan_trip(Vec2::new(-50.0, 40.0), Vec2::new(1060.0, 30.0), 4.0, 5.6, |_, _| true, |_| 45).unwrap();
        assert!(q.minutes_waiting > p.minutes_waiting + 1.0);
    }

    #[test]
    fn service_hours_wrap_and_close() {
        let laid = lay_line(CableKind::Gondola, &[Vec2::ZERO, Vec2::new(500.0, 0.0)], &[GROUND_FLOOR; 2], &Toy::flat(), &names()).unwrap();
        let mut l = line_of(CableKind::Gondola, laid, 2);
        assert!(l.open_at(12 * 60) && !l.open_at(3 * 60));
        assert!(!l.closed_for(23 * 60 + 5, CLOSING_GRACE) && l.closed_for(23 * 60 + 20, CLOSING_GRACE));
        l.open = 20 * 60;
        l.close = 2 * 60;
        assert!(l.open_at(23 * 60) && l.open_at(60) && !l.open_at(12 * 60));
    }
}
