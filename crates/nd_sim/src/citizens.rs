use crate::buildings::{BuildingId, Tier};
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
    Shop(crate::market::Resource),
    Wander,
    /// Walking to a park bench.
    Sit,
    /// Walking to a bus stop.
    ToStop,
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
    /// On a park bench, in the open.
    Sitting {
        at: Vec2,
        park: BuildingId,
    },
    /// At a bus stop, waiting for the line.
    Waiting {
        at: Vec2,
    },
    /// On a bus.
    Riding {
        bus: CarId,
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
            Activity::Sitting { .. } => "sitting in the park",
            Activity::Waiting { .. } => "waiting for the bus",
            Activity::Riding { .. } => "on the bus",
            Activity::Traveling { purpose, .. } | Activity::Driving { purpose, .. } => match purpose {
                Purpose::Home => "heading home",
                Purpose::Work => "commuting",
                Purpose::Visit(_) => "going out",
                Purpose::Shop(_) => "shopping",
                Purpose::Wander => "wandering",
                Purpose::Sit => "heading for a bench",
                Purpose::ToStop => "walking to the bus stop",
            },
        }
    }

    pub fn is_traveling(&self) -> bool {
        matches!(self, Activity::Traveling { .. } | Activity::Driving { .. } | Activity::Riding { .. })
    }
}

/// How a citizen gets around on the current trip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TravelMode {
    #[default]
    Walk,
    Bike,
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
    /// Position and heading at the previous tick, for smooth rendering.
    #[serde(default)]
    pub prev_pos: Vec2,
    #[serde(default)]
    pub prev_heading: f32,
    #[serde(default)]
    pub mode: TravelMode,
    #[serde(default)]
    pub owns_car: bool,
    #[serde(default)]
    pub owns_bike: bool,
    /// Where the car is parked while the citizen is inside somewhere.
    #[serde(default)]
    pub parked_at: Option<BuildingId>,
    /// The bus trip under way, if any.
    #[serde(default)]
    pub transit: Option<crate::transit::TransitTrip>,
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
    /// Daily income from work (or gig money when unemployed).
    pub income: f32,
    pub savings: f32,
    /// Consecutive days where rent ate too much of the income.
    pub rent_stress_days: u8,
    /// Hunger satisfaction in `0..1`; drops every hour, restored by eating.
    pub satiety: f32,
    /// Meals stocked at home.
    pub groceries: f32,
    /// Days without a proper meal.
    pub hungry_days: u8,
}

impl Citizen {
    pub fn remember(&mut self, tick: u64, text: impl Into<String>) {
        self.memories.push(Memory { tick, text: text.into() });
        if self.memories.len() > MAX_MEMORIES {
            self.memories.remove(0);
        }
    }

    pub fn class(&self) -> Tier {
        Tier::for_income(self.income)
    }

    pub fn is_visible(&self) -> bool {
        matches!(self.activity, Activity::Traveling { .. } | Activity::Idle | Activity::Sitting { .. } | Activity::Waiting { .. })
    }
}
