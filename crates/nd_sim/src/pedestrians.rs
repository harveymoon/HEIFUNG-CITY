//! [pedestrians] People on foot and the rules of the road.
//!
//! Walkers cross streets only at crossings (`walk::Crossing`). At a
//! signalised junction they wait at the kerb, in a small cluster, for the
//! walk light (`traffic::PedTraffic`); at a zebra or an unmarked crossing
//! they step out once nothing is too close to stop, and drivers give way.
//! Some people bend the rules: the rebellious (careless and
//! pleasure-seeking), the drunk and those late for work sometimes cross
//! against the light, or straight over mid-block, when they judge the gap
//! big enough. The cautious, the old and children always wait. On the
//! pavement everyone keeps to the right, keeps a little personal space,
//! steps aside for people coming the other way and falls in behind the
//! slower ones; in a crowd they stand closer.
//!
//! [emergency] Crowds are not queues: people walk at their own pace and
//! spread across the width they have (a pavement, a park path, a plaza),
//! pass slower walkers on the left where there is room and only fall in
//! behind when boxed in; in a crush they step off the line onto whatever
//! is beside it. On a crossing with the walk light on they do not wait for
//! a jam that is standing (or crawling) across it: they weave between the
//! stopped cars.
//!
//! The movement itself lives in `world/pedestrians.rs`.

use crate::citizens::{rebelliousness, CitizenId, DRUNK};
use crate::traffic::LaneId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Caution at or above which someone never jaywalks.
pub const CAUTIOUS: f32 = 0.65;
/// The old and children never jaywalk either.
pub const OLD: u8 = 65;
pub const CHILD: u8 = 13;
/// Space people keep around themselves on a pavement, metres; in a crowd
/// (more than `CROWD` others within a few metres) they stand closer.
pub const PERSONAL_SPACE: f32 = 0.75;
pub const CROWD_SPACE: f32 = 0.5;
pub const CROWD: usize = 8;
/// How far right of a path's centre line people walk, metres, and the most
/// they step aside to pass someone.
pub const KEEP_RIGHT_MIN: f32 = 0.35;
pub const KEEP_RIGHT_MAX: f32 = 0.75;
pub const SIDESTEP_MAX: f32 = 1.2;
/// [emergency] Where people like to walk, right of the centre line, spread
/// wider than `KEEP_RIGHT_*` so a crowd walks abreast rather than in single
/// file; scaled up on wide paths and plazas.
pub const KEEP_SPREAD: (f32, f32) = (-0.2, 1.05);
/// Walking pace as a share of the standard, by person.
pub const PACE: (f32, f32) = (0.86, 1.14);
/// Half-width either side of the line a walker may use away from any
/// street (lot paths, parks, plazas), and on a crossing.
pub const OPEN_ROOM: f32 = 2.6;
/// Room kept from the kerb and the building line on a pavement.
pub const KERB_MARGIN: f32 = 0.35;
/// In a crush, people spill this far beyond the path's usual width.
pub const CRUSH_SPILL: f32 = 0.9;
/// Cars crawling slower than this (m/s) across a crossing with the walk
/// light on are a jam to weave through, not traffic to wait for.
pub const JAM_CRAWL: f32 = 2.0;
/// Seconds of clear road a lawful walker wants before stepping onto a
/// crossing: drivers further off than that can stop for them.
pub const LAWFUL_GAP: f32 = 1.6;
/// A conflict faster than this (m/s) can hurt; the chance grows with speed.
pub const HURT_SPEED: f32 = 4.0;

/// How likely this person is to bend the crossing rules on a given
/// crossing, `0..1`.
pub fn jaywalk_chance(age: u8, caution: f32, hedonism: f32, drunk: f32, late: bool) -> f32 {
    if caution >= CAUTIOUS || age >= OLD || age < CHILD {
        return 0.0;
    }
    let mut p = ((rebelliousness(caution, hedonism) - 0.55) / 0.35).clamp(0.0, 1.0) * 0.5;
    if drunk >= DRUNK {
        p += 0.3;
    }
    if late {
        p += 0.25;
    }
    p.min(0.9)
}

/// Seconds of clear road a jaywalker wants before going: the careful want
/// more, someone late less.
pub fn gap_needed(caution: f32, late: bool) -> f32 {
    (3.0 + 4.0 * caution) * if late { 0.75 } else { 1.0 }
}

/// A stable `0..1` from a few numbers, for decisions that must not draw
/// from (and so shift) the world's random stream.
pub fn hash01(a: u64, b: u64, c: u64) -> f32 {
    let mut z = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.rotate_left(21).wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ c.rotate_left(42).wrapping_mul(0x94D0_49BB_1331_11EB);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// One walker's state on the move (never saved: a loaded walker starts
/// afresh from where they stand).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Walker {
    /// The path this state belongs to: length and its ends' bits.
    pub key: (usize, u32, u32),
    /// Position on the path's centre line; the figure is drawn `side` to
    /// the right of it.
    pub base: Vec2,
    pub side: f32,
    /// Where they like to walk, right of the centre line.
    pub keep: f32,
    /// On the crossing of the leg that ends at this path index.
    pub crossing: Option<(u32, usize)>,
    /// Waiting at the kerb for that crossing since this tick.
    pub waiting: Option<(u32, usize, u64)>,
    /// Will jaywalk the crossing they are waiting at (decided once per wait).
    pub bold: bool,
    /// On a crossing against the light, or mid-block.
    pub against: bool,
    /// Personality, from the seed: caution, hedonism.
    pub caution: f32,
    pub hedonism: f32,
    /// [emergency] Own walking pace (share of the standard).
    pub pace: f32,
    /// [emergency] Room either side of the path line on the current leg
    /// (m, - left, + right), and the leg it was found for.
    pub room: (f32, f32),
    pub room_leg: usize,
}

/// What a crossing is to the lane network, worked out when roads change.
/// [cyclists] A cyclist's state at the crossings (`world/riders.rs`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rider {
    /// The crossing they are waiting at and since when.
    pub waiting: Option<(u32, u64)>,
    /// The last light they ran, counted once.
    pub ran: Option<u32>,
    pub caution: f32,
    pub hedonism: f32,
}

#[derive(Clone, Debug, Default)]
pub struct CrossingInfo {
    /// Signalised: the intersection and the approach it crosses.
    pub signal: Option<(u32, u32)>,
    /// Lanes over it, with where on each the crossing begins.
    pub lanes: Vec<(LaneId, f32)>,
}

/// A pedestrian hit or nearly hit, for the record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PedIncident {
    pub tick: u64,
    pub pos: Vec2,
    pub citizen: CitizenId,
    pub speed: f32,
    pub hurt: bool,
    /// Crossing against the light or mid-block.
    pub jaywalking: bool,
}

/// The city's pedestrians: the day's counts and the walkers' state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pedestrians {
    /// Crossings started against the light or mid-block today.
    pub jaywalkers_today: u32,
    pub near_misses_today: u32,
    pub collisions_today: u32,
    pub jaywalks_total: u64,
    pub near_misses_total: u64,
    pub collisions_total: u64,
    pub recent: Vec<PedIncident>,
    /// Walkers standing at a kerb waiting to cross, last tick.
    #[serde(skip)]
    pub waiting_now: u32,
    /// Walkers' state on the pavement (side, crossing, waiting), saved so
    /// a game loaded mid-walk carries on exactly (REVIEW2 L7); keyed in
    /// order so the save is the same every time.
    pub walkers: BTreeMap<CitizenId, Walker>,
    /// Per `WalkGraph::crossings` entry.
    #[serde(skip)]
    pub crossings: Vec<CrossingInfo>,
    /// (walk graph version, lane network version) `crossings` was built for.
    #[serde(skip)]
    pub key: (u64, u64),
    /// Who was on which crossing last tick, by crossing.
    #[serde(skip)]
    pub occupants: Vec<(u32, CitizenId)>,
    /// [cyclists] Red lights (and walk lights) cyclists rode against.
    pub red_lights_run_today: u32,
    pub red_lights_run_total: u64,
    /// [cyclists] Riders stopped at a light or for a walker this tick.
    #[serde(skip)]
    pub riders_waiting_now: u32,
    pub riders: BTreeMap<CitizenId, Rider>,
    /// Grid over the walk graph's crossings (their two ends), rebuilt with
    /// `crossings`, for a cyclist to find the ones ahead.
    #[serde(skip)]
    pub cross_grid: crate::geom::Grid,
}

/// Incidents kept for the API.
pub const KEEP_INCIDENTS: usize = 40;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cautious_old_and_young_never_jaywalk_and_rebels_might() {
        assert_eq!(jaywalk_chance(30, 0.8, 1.0, 1.0, true), 0.0);
        assert_eq!(jaywalk_chance(70, 0.1, 1.0, 0.0, false), 0.0);
        assert_eq!(jaywalk_chance(9, 0.0, 1.0, 0.0, true), 0.0);
        let rebel = jaywalk_chance(22, 0.05, 0.95, 0.0, false);
        assert!(rebel > 0.3, "{rebel}");
        assert!(jaywalk_chance(22, 0.05, 0.95, 0.6, true) > rebel);
        assert_eq!(jaywalk_chance(40, 0.5, 0.3, 0.0, false), 0.0, "an ordinary sober person on time waits");
    }
}
