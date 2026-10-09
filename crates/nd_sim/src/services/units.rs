//! Units (patrol cars, engines, ambulances) and the people in custody or
//! in hospital.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitKind {
    FireEngine,
    PoliceCar,
    Ambulance,
}

impl UnitKind {
    pub fn label(self) -> &'static str {
        match self {
            UnitKind::FireEngine => "fire engine",
            UnitKind::PoliceCar => "police car",
            UnitKind::Ambulance => "ambulance",
        }
    }

    pub fn call_sign(self) -> &'static str {
        match self {
            UnitKind::FireEngine => "Engine",
            UnitKind::PoliceCar => "Car",
            UnitKind::Ambulance => "Medic",
        }
    }

    /// Workers it takes to put one on the road.
    pub fn crew(self) -> usize {
        match self {
            UnitKind::FireEngine => 3,
            UnitKind::PoliceCar | UnitKind::Ambulance => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitState {
    /// In the station.
    Idle,
    Responding,
    OnScene,
    Transporting,
    Returning,
    /// Police on patrol, ambulances on standby: crewed and free, out in the
    /// district (driving to the next beat, or parked on it).
    Patrol,
    /// [emergency] Chasing a suspect's car (`Services::chases`).
    Pursuing,
}

impl UnitState {
    pub fn label(self) -> &'static str {
        match self {
            UnitState::Idle => "in station",
            UnitState::Patrol => "on patrol",
            UnitState::Responding => "responding",
            UnitState::OnScene => "on scene",
            UnitState::Transporting => "transporting",
            UnitState::Returning => "returning",
            UnitState::Pursuing => "in pursuit",
        }
    }
}

/// One vehicle and its crew.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub kind: UnitKind,
    pub station: BuildingId,
    /// Number at its station, from 1.
    pub number: u8,
    pub state: UnitState,
    /// The car on the road while driving.
    pub car: Option<CarId>,
    pub incident: Option<u32>,
    /// Where it stands when not driving (on scene, at the hospital door).
    pub pos: Vec2,
    pub heading: f32,
    /// Hospital it is driving a patient to.
    pub hospital: Option<BuildingId>,
    /// On patrol, parked on a beat: when it moves on.
    #[serde(default)]
    pub patrol_until: u64,
    /// [services] Its station is gone: it is driving off the map, where it
    /// leaves the world (it takes no calls any more).
    #[serde(default)]
    pub leaving: bool,
    /// [fire-scene] A fire engine packing up on scene after its fire ended
    /// (`incident` still names it): it leaves at this tick. 0 otherwise.
    #[serde(default)]
    pub packing_until: u64,
}

impl Unit {
    /// On a job: responding, on scene, transporting. Units in the station,
    /// on patrol or driving home are free for the next call.
    pub fn busy(&self) -> bool {
        matches!(self.state, UnitState::Responding | UnitState::OnScene | UnitState::Transporting | UnitState::Pursuing)
    }

    /// Out of the station (so a crew is aboard).
    pub fn out(&self) -> bool {
        self.state != UnitState::Idle
    }

    /// Lights and sirens: on the way to a job or carrying a patient.
    pub fn lights(&self) -> bool {
        matches!(self.state, UnitState::Responding | UnitState::Transporting | UnitState::OnScene | UnitState::Pursuing)
    }

    /// [fire-scene] Packing up on scene after the fire ended.
    pub fn packing(&self) -> bool {
        self.state == UnitState::OnScene && self.packing_until > 0
    }
}

/// Someone in custody until `until`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Custody {
    pub citizen: CitizenId,
    /// The jail, or the police station holding them.
    pub at: BuildingId,
    pub jailed: bool,
    pub until: u64,
}

/// A patient in a hospital bed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Admission {
    pub citizen: CitizenId,
    pub hospital: BuildingId,
    pub until: u64,
}
