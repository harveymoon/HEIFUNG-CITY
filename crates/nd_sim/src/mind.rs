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
    /// [places] Head out to find somewhere new of this use near by
    /// ("looking for a drink"); wanders when there is nowhere new.
    Explore(LandUse),
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
    /// Child, adult or retiree.
    pub stage: crate::citizens::LifeStage,
    /// Alertness in `0..1`; below `citizens::TIRED` it is time for bed.
    pub wakefulness: f32,
    /// Asleep at home.
    pub asleep: bool,
    pub at_home: bool,
    /// The workplace is open and this hour is in the citizen's shift.
    pub on_shift: bool,
    /// People sharing the home, the citizen included.
    pub household_size: usize,
    /// The household pantry is under half its target: someone should shop.
    pub pantry_low: bool,
    /// [places] A regular spot for a night out that is open now.
    pub regular_spot: Option<&'a str>,
    /// [places] Places for a night out known (and not sworn off).
    pub places_known: usize,
}

pub trait MindProvider {
    /// Decide what the citizen should do now. `None` means "still thinking".
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision>;

    /// Asynchronous answers that became ready since the last call.
    fn poll(&mut self) -> Vec<(CitizenId, Decision)> {
        Vec::new()
    }

    /// The world was replaced (a save loaded, a new game): forget every
    /// request still out, so a late answer about a citizen of the old world
    /// is not applied to whoever reuses that id in the new one.
    fn reset(&mut self) {}

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
