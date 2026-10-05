use crate::buildings::BuildingId;
use crate::traffic::CarId;
use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

new_key_type! {
    pub struct CitizenId;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Purpose {
    Home,
    Work,
    Visit(LandUse),
    Wander,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Activity {
    /// Outside the city, not yet rendered.
    Arriving,
    AtHome,
    Working,
    Visiting(BuildingId),
    /// Standing around outside (no home, or wandering).
    Idle,
    Traveling {
        to: Option<BuildingId>,
        purpose: Purpose,
        path: Vec<Vec2>,
        idx: usize,
    },
    /// In a car on the lane network.
    Driving {
        car: CarId,
        to: Option<BuildingId>,
        purpose: Purpose,
    },
}

impl Activity {
    pub fn label(&self) -> &'static str {
        match self {
            Activity::Arriving => "arriving",
            Activity::AtHome => "at home",
            Activity::Working => "working",
            Activity::Visiting(_) => "out",
            Activity::Idle => "loitering",
            Activity::Traveling { purpose, .. } | Activity::Driving { purpose, .. } => match purpose {
                Purpose::Home => "heading home",
                Purpose::Work => "commuting",
                Purpose::Visit(_) => "going out",
                Purpose::Wander => "wandering",
            },
        }
    }

    pub fn is_traveling(&self) -> bool {
        matches!(self, Activity::Traveling { .. } | Activity::Driving { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub tick: u64,
    pub text: String,
}

pub const MAX_MEMORIES: usize = 24;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Citizen {
    pub name: String,
    pub seed: u64,
    pub home: Option<BuildingId>,
    pub work: Option<BuildingId>,
    pub pos: Vec2,
    pub heading: f32,
    pub activity: Activity,
    pub memories: Vec<Memory>,
    pub thought: String,
    /// Spotlight citizens may be driven by an expensive mind (an LLM).
    pub spotlight: bool,
    pub next_decision_tick: u64,
    pub arrived_tick: u64,
    /// Where the citizen is right now, if inside a building.
    pub inside: Option<BuildingId>,
    /// Satisfaction in `0..1`; feeds the economy and future happiness systems.
    pub mood: f32,
}

impl Citizen {
    pub fn remember(&mut self, tick: u64, text: impl Into<String>) {
        self.memories.push(Memory { tick, text: text.into() });
        if self.memories.len() > MAX_MEMORIES {
            self.memories.remove(0);
        }
    }

    pub fn is_visible(&self) -> bool {
        matches!(self.activity, Activity::Traveling { .. } | Activity::Idle)
    }
}
