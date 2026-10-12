//! Toll booths: a special the player sets down on a road. It sits across
//! the carriageway at one point of one road piece; every vehicle passing it
//! slows down through the booth and pays the toll into the treasury, and
//! drivers planning a route count the toll against the way (so some go
//! round). The booth has a single row of kiosks across the lanes; the plaza
//! is the big version, the road flaring out to six booths splayed across
//! (drawn wider; the sim keeps the road's own lanes, so a plaza passes
//! traffic faster rather than adding capacity).
//!
//! The booths are saved by position; their road piece is found again on
//! every road rebuild (`World::apply_tolls`), and a booth whose road went
//! is removed, as bus stops are. The traffic model carries what it needs on
//! the lanes (`traffic::LaneToll`).

use crate::roads::{RoadNetwork, SegmentId};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Metres of detour a driver will take to save one unit of money: the
/// routing weight of a toll (a toll of 3 is worth about 360 m of driving).
pub const TOLL_AVERSION_M: f32 = 120.0;
/// Lorries pay this many times the toll.
pub const TRUCK_FACTOR: f32 = 2.0;
/// Vehicles brake for the booth from this far ahead...
pub const APPROACH_M: f32 = 60.0;
/// ...and keep the booth speed until this far past it.
pub const EXIT_M: f32 = 6.0;
/// A booth snaps to a road within this distance of the click.
pub const SNAP_M: f32 = 15.0;
/// Booths closer than this to each other are refused.
pub const SPACING_M: f32 = 40.0;
/// Highest toll the player may set.
pub const MAX_PRICE: f32 = 50.0;
/// [toll-fit] A toll plaza's cost: a fixed part, and a kiosk per lane.
pub const TOLL_BASE_COST: f32 = 1_500.0;
pub const TOLL_LANE_COST: f32 = 1_250.0;
/// [toll-fit] Its upkeep a day: a fixed part, and a kiosk per lane.
pub const TOLL_BASE_UPKEEP: f32 = 15.0;
pub const TOLL_LANE_UPKEEP: f32 = 22.5;
/// [toll-inspect] Hours of passages each booth keeps for its chart.
pub const HOURS_KEPT: usize = 72;
/// [toll-inspect] A car counts as queueing for a booth when it is this far
/// or less before the line and slower than `QUEUE_SPEED`.
pub const QUEUE_M: f32 = 150.0;
pub const QUEUE_SPEED: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TollSize {
    /// A row of kiosks across the lanes under a canopy.
    #[default]
    Booth,
    /// A toll plaza: the road flares out to six booths splayed across.
    Plaza,
}

impl TollSize {
    pub const ALL: [TollSize; 2] = [TollSize::Booth, TollSize::Plaza];
    /// [toll-fit] What the Build window offers: one toll plaza, a kiosk for
    /// every lane of the road it is put on, priced by the road. (The old
    /// six-booth plaza is kept for saves that have one.)
    pub const OFFERED: [TollSize; 1] = [TollSize::Booth];

    /// [toll-fit] What a plaza across a road of `lanes` lanes costs to
    /// build: a kiosk and its canopy for every lane.
    pub fn cost_for(self, lanes: u8) -> f32 {
        match self {
            TollSize::Booth => TOLL_BASE_COST + TOLL_LANE_COST * lanes.max(1) as f32,
            TollSize::Plaza => self.cost(),
        }
    }

    /// [toll-fit] Upkeep a day across a road of `lanes` lanes.
    pub fn upkeep_for(self, lanes: u8) -> f32 {
        match self {
            TollSize::Booth => TOLL_BASE_UPKEEP + TOLL_LANE_UPKEEP * lanes.max(1) as f32,
            TollSize::Plaza => self.upkeep_per_day(),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TollSize::Booth => "toll_booth",
            TollSize::Plaza => "toll_plaza",
        }
    }

    pub fn from_key(s: &str) -> Option<TollSize> {
        match s.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "toll" | "toll_booth" | "tollbooth" | "booth" => Some(TollSize::Booth),
            "toll_plaza" | "tollplaza" | "plaza" | "toll_big" | "toll_large" => Some(TollSize::Plaza),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TollSize::Booth => "toll booth",
            TollSize::Plaza => "toll plaza",
        }
    }

    pub fn menu_label(self) -> &'static str {
        match self {
            TollSize::Booth => "Toll plaza (fits the road)",
            TollSize::Plaza => "Toll plaza (6 booths)",
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            TollSize::Booth => 4_000.0,
            TollSize::Plaza => 15_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            TollSize::Booth => 60.0,
            TollSize::Plaza => 200.0,
        }
    }

    /// The toll it opens with.
    pub fn default_price(self) -> f32 {
        match self {
            TollSize::Booth => 2.0,
            TollSize::Plaza => 3.0,
        }
    }

    /// Speed through the booth, m/s: a plaza's six lanes let cars roll
    /// through faster than a single row of kiosks.
    pub fn speed(self) -> f32 {
        match self {
            TollSize::Booth => 2.5,
            TollSize::Plaza => 4.5,
        }
    }

    /// Clear road it needs either side of the booth before a junction.
    pub fn clearance(self) -> f32 {
        match self {
            TollSize::Booth => 8.0,
            TollSize::Plaza => 24.0,
        }
    }

    /// How far the plaza's flare reaches either side of the booth line, and
    /// its width there (the booth: the road's own width).
    pub fn flare(self) -> f32 {
        match self {
            TollSize::Booth => 0.0,
            TollSize::Plaza => 22.0,
        }
    }

    /// Kiosks across the road (a plaza always has six lanes through).
    pub fn booths(self, lanes: u8) -> u8 {
        match self {
            TollSize::Booth => lanes.max(1),
            TollSize::Plaza => 6,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TollBooth {
    /// Stable id (never reused), 1 and up; 0 means "no booth" on a car.
    pub id: u32,
    pub size: TollSize,
    /// On the road centre line.
    pub pos: Vec2,
    pub price: f32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cars_today: u32,
    #[serde(default)]
    pub revenue_today: f32,
    #[serde(default)]
    pub cars_yesterday: u32,
    #[serde(default)]
    pub revenue_yesterday: f32,
    #[serde(default)]
    pub cars_total: u64,
    #[serde(default)]
    pub revenue_total: f32,
    /// [toll-inspect] Lorries through (they pay `TRUCK_FACTOR` times), and
    /// vehicles let through free (buses, emergency and city vehicles):
    /// both are part of `cars_*`.
    #[serde(default)]
    pub trucks_today: u32,
    #[serde(default)]
    pub trucks_yesterday: u32,
    #[serde(default)]
    pub trucks_total: u64,
    #[serde(default)]
    pub free_today: u32,
    #[serde(default)]
    pub free_yesterday: u32,
    #[serde(default)]
    pub free_total: u64,
    /// [toll-inspect] Drivers who set off on a way round this booth that
    /// they would have driven through were there no toll.
    #[serde(default)]
    pub avoided_today: u32,
    #[serde(default)]
    pub avoided_yesterday: u32,
    #[serde(default)]
    pub avoided_total: u64,
    /// [toll-inspect] The hour under way, and the last `HOURS_KEPT` hours,
    /// oldest first.
    #[serde(default)]
    pub hour: TollHour,
    #[serde(default)]
    pub hourly: VecDeque<TollHour>,
    /// The road piece it stands on and the piece's direction (a to b);
    /// found again on every road rebuild.
    #[serde(skip)]
    pub segment: Option<SegmentId>,
    #[serde(skip)]
    pub dir: Vec2,
    /// [toll-fit] Lanes its kiosks were built (and paid) for: widening the
    /// road adds kiosks, and the road works pay for them. 0 in a save from
    /// before (taken as the road it stands on now).
    #[serde(default)]
    pub paid_lanes: u8,
}

/// [toll-inspect] One hour of passages at a booth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TollHour {
    /// Game day (1 and up) and hour of day the counts are for.
    pub day: u32,
    pub hour: u8,
    /// Every vehicle through, lorries and free ones included.
    pub cars: u32,
    pub trucks: u32,
    pub free: u32,
    pub avoided: u32,
    pub revenue: f32,
}

impl TollBooth {
    /// A new booth with empty books.
    pub fn new(id: u32, size: TollSize, pos: Vec2, name: String) -> Self {
        Self {
            id,
            size,
            pos,
            price: size.default_price(),
            name,
            cars_today: 0,
            revenue_today: 0.0,
            cars_yesterday: 0,
            revenue_yesterday: 0.0,
            cars_total: 0,
            revenue_total: 0.0,
            trucks_today: 0,
            trucks_yesterday: 0,
            trucks_total: 0,
            free_today: 0,
            free_yesterday: 0,
            free_total: 0,
            avoided_today: 0,
            avoided_yesterday: 0,
            avoided_total: 0,
            hour: TollHour::default(),
            hourly: VecDeque::new(),
            segment: None,
            dir: Vec2::X,
            paid_lanes: 0,
        }
    }

    /// [toll-inspect] Count a vehicle through the booth: `paid` is what it
    /// was charged (nothing when it went free).
    pub fn count_pass(&mut self, truck: bool, free: bool, paid: f32) {
        self.cars_today += 1;
        self.cars_total += 1;
        self.hour.cars += 1;
        if free {
            self.free_today += 1;
            self.free_total += 1;
            self.hour.free += 1;
            return;
        }
        if truck {
            self.trucks_today += 1;
            self.trucks_total += 1;
            self.hour.trucks += 1;
        }
        self.revenue_today += paid;
        self.revenue_total += paid;
        self.hour.revenue += paid;
    }

    /// [toll-inspect] Count a driver who went round it.
    pub fn count_avoided(&mut self) {
        self.avoided_today += 1;
        self.avoided_total += 1;
        self.hour.avoided += 1;
    }

    /// [toll-inspect] What it takes in a day less its upkeep, on
    /// yesterday's takings (today's so far on the first day).
    pub fn net_per_day(&self) -> f32 {
        let take = if self.cars_yesterday > 0 || self.revenue_yesterday > 0.0 { self.revenue_yesterday } else { self.revenue_today };
        take - self.upkeep_per_day()
    }

    /// [toll-fit] Its upkeep a day: a kiosk for every lane it was built for.
    pub fn upkeep_per_day(&self) -> f32 {
        if self.paid_lanes == 0 { self.size.upkeep_per_day() } else { self.size.upkeep_for(self.paid_lanes) }
    }
}

/// Every toll booth, and the day's passages.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tolls {
    pub booths: Vec<TollBooth>,
    pub next_id: u32,
    /// Bumped whenever a booth is added, removed, moved or repriced, so
    /// the renderer redraws.
    #[serde(skip)]
    pub version: u64,
    /// Vehicles that paid today, and those let through free (buses and
    /// emergency vehicles); cars that avoided a booth are not counted.
    pub paid_today: u32,
    pub free_today: u32,
    /// Lorries' share of the take today.
    pub trucks_today: u32,
}

impl Tolls {
    pub fn get(&self, id: u32) -> Option<&TollBooth> {
        self.booths.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut TollBooth> {
        self.booths.iter_mut().find(|b| b.id == id)
    }

    /// The booth nearest `p` within `r` metres.
    pub fn nearest(&self, p: Vec2, r: f32) -> Option<&TollBooth> {
        self.booths.iter().filter(|b| b.pos.distance(p) <= r).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p)))
    }

    pub fn revenue_today(&self) -> f32 {
        self.booths.iter().map(|b| b.revenue_today).sum()
    }

    pub fn upkeep_per_day(&self) -> f32 {
        self.booths.iter().map(|b| b.upkeep_per_day()).sum()
    }

    /// Close the day's counts.
    pub fn roll(&mut self) {
        for b in &mut self.booths {
            b.cars_yesterday = std::mem::take(&mut b.cars_today);
            b.revenue_yesterday = std::mem::take(&mut b.revenue_today);
            b.trucks_yesterday = std::mem::take(&mut b.trucks_today);
            b.free_yesterday = std::mem::take(&mut b.free_today);
            b.avoided_yesterday = std::mem::take(&mut b.avoided_today);
        }
        self.paid_today = 0;
        self.free_today = 0;
        self.trucks_today = 0;
    }

    /// [toll-inspect] Close the hour at every booth: `day` (1 and up) and
    /// `hour` are those the hour's counts belong to.
    pub fn close_hour(&mut self, day: u32, hour: u8) {
        for b in &mut self.booths {
            let mut h = std::mem::take(&mut b.hour);
            h.day = day;
            h.hour = hour;
            b.hourly.push_back(h);
            while b.hourly.len() > HOURS_KEPT {
                b.hourly.pop_front();
            }
        }
    }
}

/// Where a booth of `size` clicked at `p` would stand: the road piece, the
/// point on its centre line and the piece's direction; or why it cannot.
/// Streets and highways only (not paths, track or tunnels). The point is
/// slid along the piece to keep `TollSize::clearance` from its ends, so a
/// click near a junction still lands; a piece too short for that refuses.
pub fn site(roads: &RoadNetwork, p: Vec2, size: TollSize) -> Result<(SegmentId, Vec2, Vec2), String> {
    let (sid, _, q) = roads.nearest_where(p, |s| s.is_street() && !s.tunnel).ok_or("there is no road to put a toll on")?;
    if q.distance(p) > SNAP_M {
        return Err("click on a road: a toll booth sits across the carriageway".into());
    }
    let (a, b) = roads.segment_points(sid);
    let len = a.distance(b);
    let clear = size.clearance();
    if len < clear * 2.0 + 2.0 {
        return Err(format!("this stretch of road is {len:.0} m between junctions or bends; {} needs a straight run of {:.0} m", crate::names::a(size.label()), clear * 2.0 + 2.0));
    }
    let dir = (b - a) / len.max(1e-3);
    let along = (q - a).dot(dir).clamp(clear, len - clear);
    Ok((sid, a + dir * along, dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roads::RoadKind;

    fn road(len: f32) -> RoadNetwork {
        let mut r = RoadNetwork::default();
        r.add_polyline_kind(&[Vec2::new(0.0, 0.0), Vec2::new(len, 0.0)], 1, 1, RoadKind::Street);
        r
    }

    #[test]
    fn a_booth_lands_on_the_centre_line_clear_of_the_ends() {
        let r = road(200.0);
        let (_, at, dir) = site(&r, Vec2::new(100.0, 4.0), TollSize::Booth).unwrap();
        assert!((at - Vec2::new(100.0, 0.0)).length() < 0.01);
        assert!((dir.x.abs() - 1.0).abs() < 1e-4);
        let (_, at, _) = site(&r, Vec2::new(2.0, 0.0), TollSize::Plaza).unwrap();
        assert!(at.x >= TollSize::Plaza.clearance() - 0.01, "slid clear of the junction: {at}");
        assert!(site(&r, Vec2::new(100.0, 40.0), TollSize::Booth).is_err(), "off the road");
        assert!(site(&road(30.0), Vec2::new(15.0, 0.0), TollSize::Plaza).is_err(), "too short for a plaza");
        assert!(site(&road(30.0), Vec2::new(15.0, 0.0), TollSize::Booth).is_ok());
    }

    #[test]
    fn sizes_round_trip_and_the_books_roll() {
        for s in TollSize::ALL {
            assert_eq!(TollSize::from_key(s.key()), Some(s));
        }
        let mut t = Tolls::default();
        t.booths.push(TollBooth::new(1, TollSize::Booth, Vec2::ZERO, String::new()));
        for _ in 0..3 {
            t.booths[0].count_pass(false, false, 2.0);
        }
        t.booths[0].count_pass(true, false, 4.0);
        t.booths[0].count_pass(false, true, 0.0);
        t.booths[0].count_avoided();
        t.paid_today = 4;
        t.roll();
        let b = &t.booths[0];
        assert_eq!((b.cars_yesterday, b.trucks_yesterday, b.free_yesterday, b.avoided_yesterday), (5, 1, 1, 1));
        assert!((b.revenue_yesterday - 10.0).abs() < 1e-4);
        assert_eq!((b.cars_today, b.trucks_today, b.free_today, b.avoided_today), (0, 0, 0, 0));
        assert_eq!(b.revenue_today, 0.0);
        assert_eq!((b.cars_total, b.trucks_total, b.free_total, b.avoided_total), (5, 1, 1, 1));
        assert!((b.net_per_day() - (10.0 - TollSize::Booth.upkeep_per_day())).abs() < 1e-4);
        assert_eq!(t.paid_today, 0);
        assert_eq!(t.nearest(Vec2::new(3.0, 0.0), 5.0).map(|b| b.id), Some(1));
    }
}
