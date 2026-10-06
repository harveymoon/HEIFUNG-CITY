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
            TollSize::Booth => "Toll booth",
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
    /// The road piece it stands on and the piece's direction (a to b);
    /// found again on every road rebuild.
    #[serde(skip)]
    pub segment: Option<SegmentId>,
    #[serde(skip)]
    pub dir: Vec2,
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
        self.booths.iter().map(|b| b.size.upkeep_per_day()).sum()
    }

    /// Close the day's counts.
    pub fn roll(&mut self) {
        for b in &mut self.booths {
            b.cars_yesterday = std::mem::take(&mut b.cars_today);
            b.revenue_yesterday = std::mem::take(&mut b.revenue_today);
        }
        self.paid_today = 0;
        self.free_today = 0;
        self.trucks_today = 0;
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
        return Err(format!("this stretch of road is {len:.0} m between junctions or bends; a {} needs a straight run of {:.0} m", size.label(), clear * 2.0 + 2.0));
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
        t.booths.push(TollBooth { id: 1, size: TollSize::Booth, pos: Vec2::ZERO, price: 2.0, name: String::new(), cars_today: 4, revenue_today: 8.0, cars_yesterday: 0, revenue_yesterday: 0.0, cars_total: 4, revenue_total: 8.0, segment: None, dir: Vec2::X });
        t.paid_today = 4;
        t.roll();
        assert_eq!(t.booths[0].cars_yesterday, 4);
        assert_eq!(t.booths[0].revenue_today, 0.0);
        assert_eq!(t.paid_today, 0);
        assert_eq!(t.nearest(Vec2::new(3.0, 0.0), 5.0).map(|b| b.id), Some(1));
    }
}
