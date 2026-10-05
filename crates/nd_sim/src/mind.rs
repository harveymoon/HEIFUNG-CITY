//! The interface between the simulation and whatever decides what a citizen
//! does next. Rule-based today, LLM-backed for spotlight citizens later. The
//! simulation never blocks on a mind: `decide` returns `None` when an answer is
//! still pending and the citizen simply carries on.

use crate::citizens::{Citizen, CitizenId};
use crate::clock::SimClock;
use crate::zoning::LandUse;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Stay,
    GoHome,
    GoWork,
    Visit(LandUse),
    /// Buy a retail resource at the nearest well-stocked shop.
    Shop(crate::market::Resource),
    Wander,
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub intent: Intent,
    /// What the citizen is thinking, shown in the inspector. For an LLM mind
    /// this is the model's text.
    pub thought: String,
    /// Optional new memory to record alongside the decision.
    pub memory: Option<String>,
}

pub struct MindContext<'a> {
    pub id: CitizenId,
    pub citizen: &'a Citizen,
    pub clock: &'a SimClock,
    pub has_home: bool,
    pub has_work: bool,
    pub home_name: Option<&'a str>,
    pub work_name: Option<&'a str>,
    /// Land uses that exist in the city, so minds only ask for places that exist.
    pub available: [bool; 6],
    pub population: u32,
    /// Hunger satisfaction in `0..1`.
    pub satiety: f32,
    /// Meals at home.
    pub groceries: f32,
    /// Whether any shop sells groceries right now.
    pub groceries_for_sale: bool,
    pub meals_for_sale: bool,
}

pub trait MindProvider {
    /// Decide what the citizen should do now. `None` means "still thinking".
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision>;

    /// Asynchronous answers that became ready since the last call.
    fn poll(&mut self) -> Vec<(CitizenId, Decision)> {
        Vec::new()
    }

    fn name(&self) -> &'static str;
}

/// A mind that does nothing; useful for tests and headless runs.
pub struct IdleMind;

impl MindProvider for IdleMind {
    fn decide(&mut self, _ctx: &MindContext) -> Option<Decision> {
        Some(Decision { intent: Intent::Stay, thought: String::new(), memory: None })
    }

    fn name(&self) -> &'static str {
        "idle"
    }
}
