//! [parks] (D5) The city's parks as places that are kept or let go.
//!
//! - **One park, one name.** Park lots side by side are one park (as
//!   `park::plan_parks` lays them out): the oldest park building of the
//!   group holds the park's state, and every lot of it carries its name.
//! - **Run down.** Visitors drop litter that a trash service clears; a park
//!   people sleep rough in, or by, and one on cheap land the police seldom
//!   reach draws dealers. Litter, campers and dealers wear a park down: a
//!   run-down park lifts the land round it less (it sours it, past a
//!   point), and people would rather sit somewhere else.
//! - **Clean-up.** The player pays a crew to clear the litter, move the
//!   dealers on and find the campers somewhere else to go: the park is kept
//!   again, and dealers stay away for a while ([`CLEAN_GRACE_DAYS`]).
//! - **Nature preserves.** A park can be left wild: meadow and woodland
//!   with a trail, no lawns, rides or courts. It drops little litter and
//!   never draws dealers; it lifts the land round it a little less than a
//!   kept park, and its trees are a boon for the air.
//!
//! Pure data here; the world side is `world/parks.rs`.

use crate::buildings::BuildingId;
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// A park with `kept` under this is run down.
pub const RUN_DOWN_BELOW: f32 = 0.5;
/// Litter a day per visitor-hour on a park's every 50 m² of ground.
pub const LITTER_PER_VISIT: f32 = 0.01;
/// Litter cleared a day with a trash depot in reach, and without.
pub const LITTER_CLEARED: f32 = 0.12;
pub const LITTER_CLEARED_UNSERVED: f32 = 0.03;
/// Land value under which a park can draw dealers (with little police).
pub const DEALER_LV: f32 = 0.42;
/// Most dealers in one park.
pub const MAX_DEALERS: u8 = 4;
/// Campers this close to a park's lots count as camping in it, metres.
pub const CAMP_REACH: f32 = 30.0;
/// Days dealers keep away after a clean-up.
pub const CLEAN_GRACE_DAYS: u64 = 10;
/// What a clean-up costs: a crew call-out, and by the park's size and how
/// bad it is.
pub const CLEANUP_BASE: f32 = 300.0;
pub const CLEANUP_PER_M2: f32 = 0.3;

/// How one park is doing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParkState {
    pub name: String,
    /// The other park buildings of a park of several lots (this one, the
    /// anchor, holds the state), and all its lots.
    pub members: Vec<BuildingId>,
    pub lots: Vec<LotId>,
    pub area: f32,
    /// Litter on the ground, `0..1`.
    pub trash: f32,
    /// Dealers working it now.
    pub dealers: u8,
    /// People camping in or by it (at the last count), and homeless people
    /// whiling the day away in it (most at once today).
    pub campers: u16,
    pub homeless_today: u16,
    pub homeless: u16,
    /// How well kept it is, `0..1` (1: spotless).
    pub kept: f32,
    /// Visitor-hours today and yesterday.
    pub visits_today: u32,
    pub visits_yesterday: u32,
    /// Left wild: a nature preserve.
    pub preserve: bool,
    /// Tick of the last clean-up, and how many there have been.
    pub cleaned: Option<u64>,
    pub cleanups: u32,
}

impl Default for ParkState {
    fn default() -> Self {
        Self { name: String::new(), members: Vec::new(), lots: Vec::new(), area: 0.0, trash: 0.0, dealers: 0, campers: 0, homeless_today: 0, homeless: 0, kept: 1.0, visits_today: 0, visits_yesterday: 0, preserve: false, cleaned: None, cleanups: 0 }
    }
}

impl ParkState {
    pub fn run_down(&self) -> bool {
        self.kept < RUN_DOWN_BELOW
    }

    /// What wears it down, in words ("litter, 2 dealers, 5 campers").
    pub fn troubles(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.trash >= 0.25 {
            v.push(if self.trash >= 0.6 { "litter everywhere".to_string() } else { "litter".to_string() });
        }
        if self.dealers > 0 {
            v.push(format!("{} dealer{}", self.dealers, if self.dealers == 1 { "" } else { "s" }));
        }
        if self.campers > 0 {
            v.push(format!("{} camping", self.campers));
        }
        if self.homeless > 0 {
            v.push(format!("{} homeless by day", self.homeless));
        }
        v
    }

    /// How kept a park is with this much litter, dealers and campers.
    pub fn kept_for(trash: f32, dealers: u8, campers: u16, homeless: u16) -> f32 {
        let people = (campers as f32 + 0.5 * homeless as f32) / 8.0;
        (1.0 - 0.45 * trash - 0.3 * (dealers as f32 / 3.0).min(1.0) - 0.25 * people.min(1.0)).clamp(0.0, 1.0)
    }

    /// What a clean-up of it costs now.
    pub fn cleanup_cost(&self) -> f32 {
        let bad = 0.3 + self.trash + 0.2 * self.dealers as f32 + 0.05 * (self.campers as f32).min(10.0);
        (CLEANUP_BASE + self.area * CLEANUP_PER_M2 * bad).round()
    }
}

/// The saved state of the parks (`World::parks`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Parks {
    /// Parks by the park building that holds each one's state.
    pub parks: BTreeMap<BuildingId, ParkState>,
    /// Points inside the parks left as nature preserves (a park whose lots
    /// hold one is planned wild, whatever its lots become).
    pub preserves: Vec<Vec2>,
    /// Bumped when a park's look changes (litter, dealers, preserve).
    pub version: u64,
    /// Member building to the building holding its park's state.
    #[serde(skip)]
    pub anchor_of: HashMap<BuildingId, BuildingId>,
}

impl Parks {
    /// The park a park building is part of: its state's holder.
    pub fn anchor(&self, b: BuildingId) -> Option<BuildingId> {
        if self.parks.contains_key(&b) {
            return Some(b);
        }
        match self.anchor_of.get(&b) {
            Some(a) => Some(*a),
            None if self.anchor_of.is_empty() => self.parks.iter().find(|(_, s)| s.members.contains(&b)).map(|(a, _)| *a),
            None => None,
        }
    }

    pub fn of(&self, b: BuildingId) -> Option<&ParkState> {
        self.parks.get(&self.anchor(b)?)
    }

    pub(crate) fn reindex(&mut self) {
        self.anchor_of.clear();
        for (a, s) in &self.parks {
            for m in &s.members {
                self.anchor_of.insert(*m, *a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn litter_dealers_and_campers_wear_a_park_down() {
        assert_eq!(ParkState::kept_for(0.0, 0, 0, 0), 1.0);
        assert!(ParkState::kept_for(0.8, 0, 0, 0) > RUN_DOWN_BELOW, "litter alone does not ruin it");
        assert!(ParkState::kept_for(0.8, 2, 4, 0) < RUN_DOWN_BELOW);
        let s = ParkState { area: 2000.0, trash: 0.8, dealers: 2, ..Default::default() };
        assert!(s.cleanup_cost() > CLEANUP_BASE && s.troubles().len() == 2);
    }
}
