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
    /// On a train.
    OnTrain {
        train: crate::rail::TrainId,
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
            Activity::OnTrain { .. } => "on the train",
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
        matches!(self, Activity::Traveling { .. } | Activity::Driving { .. } | Activity::Riding { .. } | Activity::OnTrain { .. })
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
/// Points kept in the breadcrumb trail drawn behind a selected citizen.
pub const MAX_TRAIL: usize = 300;
/// A citizen moved at least this far since the last point to earn another.
pub const TRAIL_STEP: f32 = 0.5;
/// A jump longer than this (arriving by train, a respawn) is not a walk:
/// the trail starts over rather than drawing a line across the map.
const TRAIL_BREAK: f32 = 150.0;

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
    /// Where the citizen has been lately, oldest first, for drawing the
    /// route so far. Rebuilt as they move; never saved.
    #[serde(skip)]
    pub trail: Vec<Vec2>,
}

impl Citizen {
    /// Record the current position if the citizen has moved on since the
    /// last point. Called once per tick after every system has moved people.
    pub fn push_trail(&mut self) {
        if let Some(&last) = self.trail.last() {
            let d = last.distance(self.pos);
            if d < TRAIL_STEP {
                return;
            }
            if d > TRAIL_BREAK {
                self.trail.clear();
            }
        }
        self.trail.push(self.pos);
        if self.trail.len() > MAX_TRAIL {
            self.trail.remove(0);
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn citizen_at(pos: Vec2) -> Citizen {
        Citizen {
            name: "Test".into(),
            seed: 1,
            home: None,
            work: None,
            pos,
            heading: 0.0,
            prev_pos: pos,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            owns_car: false,
            owns_bike: false,
            parked_at: None,
            transit: None,
            activity: Activity::Idle,
            memories: Vec::new(),
            thought: String::new(),
            spotlight: false,
            next_decision_tick: 0,
            arrived_tick: 0,
            inside: None,
            mood: 0.5,
            income: 0.0,
            savings: 0.0,
            rent_stress_days: 0,
            satiety: 1.0,
            groceries: 0.0,
            hungry_days: 0,
            trail: Vec::new(),
        }
    }

    #[test]
    fn trail_records_movement_only() {
        let mut c = citizen_at(Vec2::ZERO);
        c.push_trail();
        c.push_trail();
        assert_eq!(c.trail.len(), 1, "standing still adds nothing after the first point");
        c.pos = Vec2::new(0.2, 0.0);
        c.push_trail();
        assert_eq!(c.trail.len(), 1, "a shuffle under half a metre is not a step");
        c.pos = Vec2::new(1.0, 0.0);
        c.push_trail();
        assert_eq!(c.trail.len(), 2);
    }

    #[test]
    fn trail_is_capped_and_breaks_on_a_jump() {
        let mut c = citizen_at(Vec2::ZERO);
        for i in 0..(MAX_TRAIL + 50) {
            c.pos = Vec2::new(i as f32, 0.0);
            c.push_trail();
        }
        assert_eq!(c.trail.len(), MAX_TRAIL);
        assert_eq!(c.trail[0], Vec2::new(50.0, 0.0), "the oldest points are dropped first");
        c.pos = Vec2::new(5000.0, 0.0);
        c.push_trail();
        assert_eq!(c.trail, vec![Vec2::new(5000.0, 0.0)], "a teleport starts a fresh trail");
    }
}
