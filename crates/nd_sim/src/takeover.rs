//! Street takeovers: the data and the geometry. On some nights a crew of
//! locals with tuned cars and Akira bikes, joined by outsiders driving in
//! off the highway, meets at an intersection. They arrive in a pack running
//! lights, park in a ring that closes the junction to everyone else, and the
//! best cars spin donuts in the middle in their own tyre smoke while a
//! crowd gathers round. Somebody calls the police; when a car gets there
//! the show breaks up (a few arrests if they are quick), otherwise it burns
//! out after an hour or so. Then they leave in convoy.
//!
//! The night itself is run by `World::step_takeovers` (`world/driving.rs`):
//! this file holds what it remembers and where the cars go.

use crate::citizens::CitizenId;
use crate::roads::NodeId;
use crate::traffic::CarId;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Chance a night has a takeover, when the city has a crew for one.
pub const NIGHT_CHANCE: f64 = 0.3;
/// Locals and outsiders in one, at most.
pub const MAX_RESIDENTS: usize = 6;
pub const OUTSIDERS: (u32, u32) = (4, 9);
/// Ticks the crew waits for stragglers before the show starts anyway, and
/// how long a show runs if nobody stops it.
pub const GATHER_TICKS: u64 = 50;
pub const SHOW_TICKS: (u64, u64) = (45, 80);
/// Spectators at most, and how far they come from.
pub const CROWD_MAX: usize = 28;
pub const CROWD_REACH: f32 = 450.0;
/// A car this close to the meet joins the show.
pub const JOIN_REACH: f32 = 75.0;
/// Chance the police make an arrest of each local still in the ring.
pub const ARREST_CHANCE: f32 = 0.35;
pub const MAX_ARRESTS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// Crews driving in.
    Gathering,
    /// The junction is held: ring, donuts, crowd.
    Show,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Gathering => "gathering",
            Stage::Show => "show",
        }
    }
}

/// One takeover under way.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Takeover {
    pub id: u32,
    pub node: NodeId,
    pub centre: Vec2,
    /// Radius of the ring of parked cars (the junction plus a margin).
    pub radius: f32,
    /// A building nearby, to name the place.
    pub place: String,
    pub stage: Stage,
    pub started: u64,
    pub show_from: Option<u64>,
    pub ends_at: Option<u64>,
    /// Every car in it (locals' and outsiders').
    pub cars: Vec<CarId>,
    /// The locals driving.
    pub residents: Vec<CitizenId>,
    pub outsiders: u32,
    /// Outsiders still to drive in, and where from.
    pub to_spawn: u32,
    pub entry: Option<NodeId>,
    /// Where the outsiders leave by.
    pub exit: Option<NodeId>,
    /// Spectators.
    pub crowd: Vec<CitizenId>,
    /// The 911 call about it.
    pub incident: Option<u32>,
    pub arrests: u32,
    /// Ring slots and donut spots handed out so far.
    pub ring_used: usize,
    pub donuts: usize,
}

impl Takeover {
    /// Cars in it at all.
    pub fn participants(&self) -> usize {
        self.cars.len()
    }
}

/// How one ended, for the record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: u32,
    pub place: String,
    pub centre: Vec2,
    pub started: u64,
    pub ended: u64,
    pub participants: usize,
    pub residents: usize,
    pub outsiders: u32,
    pub crowd: usize,
    pub arrests: u32,
    /// Broken up by the police (else it ran its course).
    pub police: bool,
}

/// Everything the city remembers about takeovers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Takeovers {
    pub active: Option<Takeover>,
    pub next_id: u32,
    /// The tick tonight's takeover starts, if one is planned.
    pub planned: Option<u64>,
    /// Day the last plan was rolled for.
    pub rolled_day: u64,
    pub history: Vec<Record>,
    pub total: u32,
}

/// Spots for the ring of parked cars: `n` round a circle, each parked
/// across the circle's tangent with its nose out (so the crowd behind sees
/// the headlights sweep), a little wobble so it reads as people, not a
/// formation. Returns (position, heading).
pub fn ring_slot(centre: Vec2, radius: f32, k: usize, n: usize) -> (Vec2, f32) {
    let n = n.max(6);
    let a = k as f32 / n as f32 * std::f32::consts::TAU + 0.3 * ((k * 7) % 5) as f32 / 5.0;
    let dir = Vec2::new(a.cos(), a.sin());
    let p = centre + dir * (radius + 0.6 * ((k * 3) % 4) as f32 / 4.0);
    // Nose pointing in, angled a bit along the ring.
    let face = (-dir).y.atan2((-dir).x) + 0.35 * if k % 2 == 0 { 1.0 } else { -1.0 };
    (p, face)
}

/// Centres for the donut cars inside the ring.
pub fn donut_spot(centre: Vec2, k: usize) -> (Vec2, f32) {
    let offsets = [Vec2::new(0.0, 0.0), Vec2::new(5.5, 2.0), Vec2::new(-5.0, -2.5), Vec2::new(1.5, -6.0)];
    let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
    (centre + offsets[k % offsets.len()], dir)
}

/// Where spectator `k` stands: just outside the ring.
pub fn crowd_spot(centre: Vec2, radius: f32, k: usize, jitter: f32) -> Vec2 {
    let a = k as f32 * 2.39996 + jitter;
    centre + Vec2::new(a.cos(), a.sin()) * (radius + 4.0 + (k % 4) as f32 * 1.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_slots_surround_the_centre_and_face_it() {
        let c = Vec2::new(10.0, -5.0);
        for k in 0..8 {
            let (p, face) = ring_slot(c, 14.0, k, 8);
            let d = p.distance(c);
            assert!((14.0..=15.0).contains(&d), "slot {k} at {d}");
            let nose = Vec2::new(face.cos(), face.sin());
            assert!(nose.dot((c - p).normalize()) > 0.8, "slot {k} faces in");
        }
        let spots: Vec<Vec2> = (0..8).map(|k| ring_slot(c, 14.0, k, 8).0).collect();
        for i in 0..8 {
            for j in i + 1..8 {
                assert!(spots[i].distance(spots[j]) > 4.0, "slots {i} and {j} overlap");
            }
        }
    }
}
