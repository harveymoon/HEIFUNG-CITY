//! [road-inspect] What the road inspector reads about a street piece that
//! the rest of the sim does not keep: the vehicles that drove onto it hour
//! by hour over the last three days (a small ring of `u16` counts per
//! piece, made on the first vehicle, so quiet pieces cost nothing), and
//! its wear history (the wear at each midnight, the wear a day measured
//! from it, and the day it was last resurfaced).
//!
//! The figures are kept per segment index with each piece's midpoint, and
//! follow the pieces through rebuilds the way `road_wear` does
//! (`RoadLog::remap`). Display only: nothing here feeds back into the
//! traffic or the wear. No random numbers.

use crate::roads::RoadNetwork;
use crate::seg_match::{Matcher, OldPiece};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Hours of traffic kept per piece (three days).
pub const HOURS: usize = 72;

/// Vehicles onto a piece per hour, for the last `HOURS` hours: a ring
/// indexed by the absolute hour (`tick / 60`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HourCounts {
    /// The absolute hour of the newest slot written.
    pub hour: u32,
    /// Slot `h % HOURS` holds hour `h`; empty until the first vehicle.
    pub counts: Vec<u16>,
}

impl HourCounts {
    /// `n` more vehicles in absolute hour `hour`. The hours skipped since
    /// the last write are cleared; a count tops out at `u16::MAX`; an hour
    /// already rolled out of the ring is dropped.
    pub fn add(&mut self, hour: u32, n: u16) {
        if self.counts.len() != HOURS {
            self.counts = vec![0; HOURS];
            self.hour = hour;
        }
        if hour > self.hour {
            let gap = (hour - self.hour) as usize;
            if gap >= HOURS {
                self.counts.iter_mut().for_each(|c| *c = 0);
            } else {
                for h in self.hour + 1..=hour {
                    self.counts[h as usize % HOURS] = 0;
                }
            }
            self.hour = hour;
        }
        if hour as usize + HOURS <= self.hour as usize {
            return;
        }
        let k = hour as usize % HOURS;
        self.counts[k] = self.counts[k].saturating_add(n);
    }

    /// The count for absolute hour `h` (0 for hours never written or
    /// rolled out).
    pub fn at(&self, h: u32) -> u16 {
        if self.counts.len() != HOURS || h > self.hour || h as usize + HOURS <= self.hour as usize {
            return 0;
        }
        self.counts[h as usize % HOURS]
    }

    /// The `n` hours (at most `HOURS`) ending with `now`, oldest first.
    pub fn series(&self, now: u32, n: usize) -> Vec<u16> {
        let n = n.min(HOURS) as u32;
        (0..n).map(|k| now.checked_sub(n - 1 - k).map_or(0, |h| self.at(h))).collect()
    }

    /// Vehicles over the hours `from..=to`.
    pub fn sum(&self, from: u32, to: u32) -> u32 {
        (from..=to).map(|h| self.at(h) as u32).sum()
    }
}

/// One piece's log.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SegLog {
    /// The piece's midpoint when laid out, to follow it through rebuilds.
    pub mid: Vec2,
    /// Its two ends then (`None` in saves from before they were kept:
    /// those match by midpoint).
    pub ends: Option<(Vec2, Vec2)>,
    pub traffic: HourCounts,
    /// Wear at the last midnight it was read.
    pub wear_midnight: Option<f32>,
    /// Wear gained over the last whole day it was not repaired.
    pub wear_rate: Option<f32>,
    /// Day (0-based) it was last resurfaced or patched, and whether all the
    /// way (wear back to nothing).
    pub repaired_day: Option<u32>,
    pub repaired_whole: bool,
    /// Resurfaced by the player's "Repair now" rather than the crews.
    pub repaired_by_order: bool,
}

/// The log of every piece, by segment index.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RoadLog {
    pub segs: Vec<SegLog>,
    /// `RoadNetwork::version` `segs` was laid out for.
    pub roads_version: u64,
}

impl Default for RoadLog {
    fn default() -> Self {
        Self { segs: Vec::new(), roads_version: u64::MAX }
    }
}

fn ends_of(roads: &RoadNetwork, i: usize) -> (Vec2, Vec2) {
    let s = &roads.segments[i];
    (roads.node(s.a).pos, roads.node(s.b).pos)
}

impl RoadLog {
    /// Lay the log out for the network as it is now: unchanged pieces keep
    /// theirs, a changed one takes the log of the old piece it continues
    /// (`seg_match`: its midpoint on that old piece; a street cut in two
    /// keeps its history on both halves), new road starts empty.
    pub fn remap(&mut self, roads: &RoadNetwork) {
        let n = roads.segments.len();
        if self.roads_version == roads.version && self.segs.len() == n {
            return;
        }
        let old = std::mem::take(&mut self.segs);
        let empty = |l: &SegLog| *l == SegLog { mid: l.mid, ends: l.ends, ..Default::default() };
        let matcher = Matcher::new(old.iter().enumerate().filter(|(_, l)| !empty(l)).map(|(k, l)| (k, OldPiece { mid: l.mid, ends: l.ends })));
        let mut segs = vec![SegLog::default(); n];
        for (i, slot) in segs.iter_mut().enumerate() {
            if !roads.segments[i].alive {
                continue;
            }
            let (a, b) = ends_of(roads, i);
            let m = (a + b) * 0.5;
            let same = old.get(i).is_some_and(|o| o.mid.distance(m) < 0.01 && o.ends.is_none_or(|(oa, ob)| oa.distance(a) < 0.01 && ob.distance(b) < 0.01));
            let found = if same { Some(i) } else { matcher.find(a, b) };
            if let Some(k) = found {
                *slot = old[k].clone();
            }
            slot.mid = m;
            slot.ends = Some((a, b));
        }
        self.segs = segs;
        self.roads_version = roads.version;
    }

    pub fn get(&self, i: usize) -> Option<&SegLog> {
        self.segs.get(i)
    }

    /// Midnight: read each piece's wear (`wear_of`); a piece worn less
    /// than at the last midnight was resurfaced (or patched) today, else
    /// what it gained is its wear a day. (A piece repaired by order has no
    /// reading to compare until the next midnight.)
    pub fn note_midnight(&mut self, day: u32, wear_of: impl Fn(usize) -> Option<f32>) {
        for (i, l) in self.segs.iter_mut().enumerate() {
            let Some(w) = wear_of(i) else { continue };
            if let Some(prev) = l.wear_midnight {
                if w < prev - 1e-4 {
                    l.repaired_day = Some(day);
                    l.repaired_whole = w < 1e-4;
                    l.repaired_by_order = false;
                } else {
                    l.wear_rate = Some((w - prev).max(0.0));
                }
            }
            l.wear_midnight = Some(w);
        }
    }
}

/// The wear bands a piece passes through, with the label it takes on
/// reaching each (`road_wear::label`).
pub const STEPS: [(f32, &str); 3] = [(crate::road_wear::REPAIR_AT, "fair"), (crate::road_wear::WORN, "worn"), (crate::road_wear::CRUMBLING, "crumbling")];

/// The next condition a piece worn `wear` reaches at `rate` a day, and in
/// how many days (`None` past the last step or when it does not wear).
pub fn next_step(wear: f32, rate: f32) -> Option<(&'static str, f32)> {
    let (at, label) = STEPS.iter().copied().find(|(at, _)| wear < *at)?;
    (rate > 1e-6).then(|| (label, (at - wear) / rate))
}

/// What is on a piece right now (`World::road_now`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadNow {
    pub cars: u32,
    pub lorries: u32,
    pub buses: u32,
    /// Police, fire, ambulances and city vehicles with a siren on.
    pub sirens: u32,
    pub bikes: u32,
    pub walkers: u32,
    /// Mean speed of the vehicles on it, and the mean limit of its lanes
    /// (with the rough surface's slowdown: what drivers would keep), m/s.
    pub mean_speed: f32,
    pub limit: f32,
    /// Vehicles standing (under 2 m/s) back from the end of a lane, all
    /// lanes, and the longest such queue, metres.
    pub queued: u32,
    pub queue_m: f32,
    /// Lane-metres, for density.
    pub lane_m: f32,
}

impl RoadNow {
    pub fn vehicles(&self) -> u32 {
        self.cars + self.lorries + self.buses + self.sirens
    }

    /// Vehicles per lane-kilometre.
    pub fn density(&self) -> f32 {
        if self.lane_m > 0.0 { self.vehicles() as f32 * 1000.0 / self.lane_m } else { 0.0 }
    }

    /// How the traffic moves: "empty", "free-flowing", "busy", "slow" or
    /// "jammed" (by the mean speed against what the road allows, and the
    /// queue).
    pub fn congestion(&self) -> &'static str {
        if self.vehicles() == 0 {
            return "empty";
        }
        let r = if self.limit > 0.0 { self.mean_speed / self.limit } else { 1.0 };
        let queue_share = self.queued as f32 / self.vehicles() as f32;
        if r < 0.25 && queue_share > 0.5 && self.density() > 40.0 {
            "jammed"
        } else if r < 0.45 {
            "slow"
        } else if r < 0.75 {
            "busy"
        } else {
            "free-flowing"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hourly_counts_add_up_per_hour() {
        let mut h = HourCounts::default();
        assert_eq!(h.at(10), 0);
        h.add(10, 1);
        h.add(10, 2);
        h.add(11, 5);
        assert_eq!((h.at(9), h.at(10), h.at(11), h.at(12)), (0, 3, 5, 0));
        assert_eq!(h.series(12, 4), vec![0, 3, 5, 0]);
        assert_eq!(h.sum(10, 11), 8);
        // A late add for an hour still in the ring lands there.
        h.add(10, 1);
        assert_eq!(h.at(10), 4);
    }

    #[test]
    fn hourly_counts_roll_over_after_three_days() {
        let mut h = HourCounts::default();
        for hour in 0..HOURS as u32 {
            h.add(hour, hour as u16 + 1);
        }
        assert_eq!(h.series(HOURS as u32 - 1, HOURS).iter().map(|&c| c as u32).sum::<u32>(), (1..=HOURS as u32).sum::<u32>());
        // Hour 72 reuses hour 0's slot: hour 0 is gone, hour 1 still kept.
        h.add(HOURS as u32, 7);
        assert_eq!((h.at(0), h.at(1), h.at(HOURS as u32)), (0, 2, 7));
        // A quiet stretch clears the hours skipped, not just the new one.
        h.add(HOURS as u32 + 5, 1);
        assert_eq!((h.at(HOURS as u32 + 1), h.at(HOURS as u32 + 4), h.at(HOURS as u32)), (0, 0, 7));
        assert_eq!(h.at(5), 0, "hour 5 rolled out");
        // Over a whole ring's length idle: only the new hour counts.
        h.add(1000, 3);
        assert_eq!(h.counts.iter().map(|&c| c as u32).sum::<u32>(), 3);
        assert_eq!(h.series(1000, 3), vec![0, 0, 3]);
        // An hour already rolled out is dropped, not written over a new one.
        h.add(1000 - HOURS as u32, 9);
        assert_eq!(h.counts.iter().map(|&c| c as u32).sum::<u32>(), 3);
        // Reading ahead of the last write: the hours since were quiet.
        assert_eq!(h.series(1002, 3), vec![3, 0, 0]);
    }

    #[test]
    fn hourly_counts_cap_at_u16() {
        let mut h = HourCounts::default();
        h.add(3, u16::MAX - 1);
        h.add(3, 5);
        assert_eq!(h.at(3), u16::MAX);
        h.add(4, 1);
        assert_eq!(h.at(4), 1);
        assert_eq!(h.counts.len(), HOURS, "a fixed ring: 72 u16s a piece");
    }

    #[test]
    fn next_condition_step() {
        assert_eq!(next_step(0.1, 0.01).map(|(l, d)| (l, d.round())), Some(("fair", 20.0)));
        assert_eq!(next_step(0.6, 0.02).map(|(l, d)| (l, d.round())), Some(("crumbling", 10.0)));
        assert_eq!(next_step(0.9, 0.02), None);
        assert_eq!(next_step(0.1, 0.0), None);
    }

    #[test]
    fn midnight_notes_rate_and_repairs() {
        let mut log = RoadLog { segs: vec![SegLog::default(); 2], roads_version: 0 };
        let wear = std::cell::Cell::new([0.2f32, 0.5]);
        log.note_midnight(0, |i| Some(wear.get()[i]));
        wear.set([0.21, 0.0]);
        log.note_midnight(1, |i| Some(wear.get()[i]));
        assert!((log.segs[0].wear_rate.unwrap() - 0.01).abs() < 1e-5);
        assert_eq!((log.segs[1].repaired_day, log.segs[1].repaired_whole), (Some(1), true));
        assert_eq!(log.segs[1].wear_rate, None);
    }

    /// A street cut in two keeps its traffic history on both halves.
    #[test]
    fn a_split_street_keeps_its_history_on_both_halves() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        let mut log = RoadLog::default();
        log.remap(&roads);
        log.segs[0].traffic.add(5, 7);
        log.segs[0].repaired_day = Some(3);
        roads.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        log.remap(&roads);
        let mut halves = 0;
        for (i, l) in log.segs.iter().enumerate() {
            if !roads.segments[i].alive {
                continue;
            }
            let (a, b) = l.ends.expect("ends kept");
            if (a.y - b.y).abs() < 0.1 {
                halves += 1;
                assert_eq!((l.traffic.at(5), l.repaired_day), (7, Some(3)), "half {i}");
            } else {
                assert_eq!(l.traffic.at(5), 0, "the crossing street is new");
            }
        }
        assert_eq!(halves, 2);
    }
}
