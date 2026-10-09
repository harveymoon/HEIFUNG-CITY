//! [chases] Police chases: when one starts, how it is lost or won.

use super::*;

/// Share of suspects about to be picked up who have a vehicle within reach
/// and run for it instead.
pub const CHASE_CHANCE: f32 = 0.5;

/// How far from the scene the suspect's own parked car (or motorbike) may
/// be for them to get to it, metres.
pub const CHASE_REACH: f32 = 700.0;

/// Centre to centre (m) at which a police car touches the car it chases:
/// tag, the suspect stops and gets out.
pub const TAG_REACH: f32 = 4.6;

/// Further apart than this (m) the police are losing them; that many
/// minutes running and they are gone. A chase is given up after
/// `CHASE_MAX` minutes in any case.
pub const CHASE_LOSE_DIST: f32 = 280.0;

pub const CHASE_LOSE_TICKS: u32 = 6;

pub const CHASE_MAX: u64 = 60;

/// Minutes between the fugitive's fresh looks for somewhere away from the
/// police (also on reaching where it was heading).
pub const FLEE_REPLAN: u64 = 8;

/// How far off a fugitive heads (metres from where it is).
pub const FLEE_RANGE: (f32, f32) = (250.0, 1200.0);

/// How a chase ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChaseResult {
    /// The police car touched it: the suspect stopped, got out, arrested.
    Tagged,
    /// Out of sight for too long, or the chase went on too long.
    Lost,
    /// Called off: the unit or the car went (a rebuild, a sale, a wreck).
    CalledOff,
}

impl ChaseResult {
    pub fn label(self) -> &'static str {
        match self {
            ChaseResult::Tagged => "tagged and arrested",
            ChaseResult::Lost => "lost them",
            ChaseResult::CalledOff => "called off",
        }
    }
}

/// A chase as kept on the incident's record.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChaseRecord {
    pub started: u64,
    pub ended: Option<u64>,
    /// What the suspect drove ("an old motorcycle").
    pub vehicle: String,
    pub result: Option<ChaseResult>,
    /// Metres the fugitive drove, and the closest the police got after the
    /// start (m).
    pub metres: f32,
    pub closest: f32,
}

/// A chase under way: the police unit after the suspect's car.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chase {
    /// The incident (a crime) it is part of.
    pub incident: u32,
    pub unit: UnitId,
    pub suspect: CitizenId,
    /// The fugitive's car on the road, and the vehicle it is, if theirs.
    pub car: CarId,
    #[serde(default)]
    pub vehicle: Option<crate::vehicle::VehicleId>,
    pub started: u64,
    /// Minutes in a row the police have been further off than
    /// `CHASE_LOSE_DIST`.
    #[serde(default)]
    pub out_of_touch: u32,
    /// When the fugitive last chose where to run.
    #[serde(default)]
    pub planned: u64,
    /// Where the fugitive was last tick (for the distance driven).
    #[serde(default)]
    pub last: Vec2,
}
