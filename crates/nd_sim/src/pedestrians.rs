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
pub use crate::hash::hash01;

/// One walker's state on the move. Saved with `Pedestrians::walkers`, so
/// every field defaults (`#[serde(default)]`): add new ones the same way.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
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

impl Default for Walker {
    /// A walker with no state yet: the path key and the room are worked
    /// out afresh (`usize::MAX`), at the standard pace.
    fn default() -> Self {
        Walker { key: (usize::MAX, 0, 0), base: Vec2::ZERO, side: 0.0, keep: 0.0, crossing: None, waiting: None, bold: false, against: false, caution: 0.5, hedonism: 0.5, pace: 1.0, room: (-SIDESTEP_MAX, SIDESTEP_MAX), room_leg: usize::MAX }
    }
}

/// [cyclists] A cyclist's state at the crossings (`world/riders.rs`).
/// Saved, so every field defaults.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Rider {
    /// The crossing they are waiting at and since when.
    pub waiting: Option<(u32, u64)>,
    /// The last light they ran, counted once.
    pub ran: Option<u32>,
    pub caution: f32,
    pub hedonism: f32,
}

impl Default for Rider {
    fn default() -> Self {
        Rider { waiting: None, ran: None, caution: 0.5, hedonism: 0.5 }
    }
}

/// What a crossing is to the lane network, worked out when roads change.
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
    /// [tools] Push-button crossings' lights, by crossing (reset when the
    /// walk graph is rebuilt).
    #[serde(skip)]
    pub buttons: BTreeMap<u32, ButtonLight>,
    /// Walk graph version `buttons` belongs to.
    #[serde(skip)]
    pub buttons_key: u64,
    /// [tools] Button presses ever, and red lights they gave traffic.
    pub button_presses_total: u64,
    pub button_reds_total: u64,
    /// [walk-crossings] How each crossing has been used since the walk
    /// graph was built (or the survey reset): by walk-graph crossing index,
    /// a footbridge counted under the crossing it spans. Not saved; the
    /// `walk_crossings` survey and the tests read it.
    #[serde(skip)]
    pub uses: BTreeMap<u32, CrossingUse>,
    /// Walk graph version `uses` belongs to.
    #[serde(skip)]
    pub uses_key: u64,
    /// [walk-crossings] Walkers who found the crossing ahead of them gone
    /// (a footbridge or the player took it away) and planned again, ever.
    pub replans_total: u64,
    /// [walk-crossings] Jaywalkers who gave up waiting for a gap in the
    /// traffic and went round by a crossing instead, ever.
    pub jay_give_ups_total: u64,
}

/// [walk-crossings] One crossing's use since the survey began.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CrossingUse {
    /// Walkers who stepped out over it at grade.
    pub at_grade: u32,
    /// Of those, against the light or mid-block (jaywalking).
    pub against: u32,
    /// Walkers over its footbridge's deck.
    pub bridge: u32,
    /// Cyclists over its footbridge (wheeling the bike).
    pub bridge_bikes: u32,
    /// Walkers who planned again on finding it shut.
    pub replans: u32,
}

/// [tools] Seconds from a press to the red for traffic (an amber), the
/// red's length, and the least green traffic then gets before the next red.
pub const BUTTON_DELAY: f32 = 3.0;
pub const BUTTON_RED: f32 = 12.0;
pub const BUTTON_MIN_GREEN: f32 = 20.0;

/// [tools] A push-button crossing's light for traffic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ButtonLight {
    /// Green for traffic, nobody waiting.
    #[default]
    Idle,
    /// Pressed: red for traffic in so many seconds.
    Pressed(f32),
    /// Red for traffic (walk for walkers) for so many seconds more.
    Red(f32),
    /// Green for traffic for at least so many seconds more; `true` if
    /// someone pressed meanwhile.
    Green(f32, bool),
}

impl ButtonLight {
    /// Someone presses the button.
    pub fn press(self) -> Self {
        match self {
            ButtonLight::Idle => ButtonLight::Pressed(BUTTON_DELAY),
            ButtonLight::Green(t, _) => ButtonLight::Green(t, true),
            other => other,
        }
    }

    /// Advance by `dt` seconds; true when the red starts now.
    pub fn step(&mut self, dt: f32) -> bool {
        match *self {
            ButtonLight::Idle => false,
            ButtonLight::Pressed(t) if t - dt <= 0.0 => {
                *self = ButtonLight::Red(BUTTON_RED);
                true
            }
            ButtonLight::Pressed(t) => {
                *self = ButtonLight::Pressed(t - dt);
                false
            }
            ButtonLight::Red(t) if t - dt <= 0.0 => {
                *self = ButtonLight::Green(BUTTON_MIN_GREEN, false);
                false
            }
            ButtonLight::Red(t) => {
                *self = ButtonLight::Red(t - dt);
                false
            }
            ButtonLight::Green(t, pending) if t - dt <= 0.0 => {
                *self = if pending { ButtonLight::Pressed(BUTTON_DELAY) } else { ButtonLight::Idle };
                false
            }
            ButtonLight::Green(t, pending) => {
                *self = ButtonLight::Green(t - dt, pending);
                false
            }
        }
    }

    /// Red for traffic: walkers may cross.
    pub fn walk(self) -> bool {
        matches!(self, ButtonLight::Red(t) if t > 2.0)
    }

    pub fn red(self) -> bool {
        matches!(self, ButtonLight::Red(_))
    }

    pub fn label(self) -> &'static str {
        match self {
            ButtonLight::Idle => "green (nobody waiting)",
            ButtonLight::Pressed(_) => "pressed: amber",
            ButtonLight::Red(_) => "red: walk",
            ButtonLight::Green(..) => "green",
        }
    }
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

    /// Walkers and riders saved before a field existed still load, at the
    /// defaults (the standard pace, room worked out afresh).
    #[test]
    fn walkers_and_riders_from_older_saves_load() {
        let w: Walker = ron::from_str("(key: (3, 1, 2), base: (1.0, 2.0), side: 0.5, caution: 0.2)").expect("walker loads");
        assert_eq!((w.key, w.caution, w.pace, w.room_leg), ((3, 1, 2), 0.2, 1.0, usize::MAX));
        let r: Rider = ron::from_str("(ran: Some(4))").expect("rider loads");
        assert_eq!((r.ran, r.waiting), (Some(4), None));
    }
}
