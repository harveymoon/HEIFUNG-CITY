//! [dirty-industry] Dirty industry and its neighbours: mines, oil wells,
//! refineries and chemical works, smelters, factory farms and coal plants.
//!
//! - The dirty plants are read off the buildings every hour into
//!   `World::dirty` ([`World::refresh_dirty`]): where each is, how dirty
//!   it is right now and how far its neighbours feel it.
//! - Every neighbour suffers by the compatibility table
//!   (`industry::sensitivity`): farms, fishing, homes, shops and clean tech
//!   hate a dirty plant next door, manufacturing and logistics shrug, the
//!   dirty kinds tolerate each other. What one suffers is its *exposure*,
//!   `0..1` ([`World::building_exposure`]).
//! - Exposure costs land value and so rent and tier (`World::lot_value`,
//!   [`ADJACENCY_VALUE_HIT`]), food output ([`ADJACENCY_OUTPUT_HIT`], in
//!   `World::industry_hourly`), and the chance homes and shops are built
//!   next door at all (`develop`, [`ADJACENCY_BUILD_HIT`]); new industry
//!   picks its kind away from it (`industry::weights`).
//! - Residents of exposed or poisoned homes lose a little mood each day and
//!   some say so (Noise & pollution), [`World::daily_dirt`]; parks slowly
//!   clean the ground they stand on, and the player can pay to have
//!   ground cleaned up ([`World::remediate`]).
//!
//! Nothing here draws a random number.

use super::*;
use crate::industry::{self, DirtySource, IndustryKind, Neighbour};

/// Land value a lot loses at full exposure to dirty industry.
pub const ADJACENCY_VALUE_HIT: f32 = 0.3;
/// Share of a farm's or fishery's output lost at full exposure.
pub const ADJACENCY_OUTPUT_HIT: f32 = 0.5;
/// How much less likely homes and shops are built at full exposure.
pub const ADJACENCY_BUILD_HIT: f32 = 0.85;
/// Share of the contamination under a park that its plants and soil take
/// out each day, on top of the ground's own slow recovery.
pub const PARK_CLEANUP_PER_DAY: f32 = 0.03;
/// What cleaning up one cell's worth of full contamination costs.
pub const CLEANUP_COST: f32 = 450.0;
/// The widest area one clean-up order covers, metres.
pub const CLEANUP_MAX_RADIUS: f32 = 300.0;
/// A home this exposed (or on ground this contaminated) feels it.
pub const HOME_EXPOSED: f32 = 0.12;
/// Mood a resident loses a day at full exposure.
pub const DIRT_MOOD: f32 = 0.04;
/// Homes heard from per day about dirty neighbours, at most.
const DIRT_VOICES: usize = 16;
/// A coal plant's dirtiness as a neighbour (it reads as a refinery).
pub const COAL_DIRTINESS: f32 = 0.7;

/// A dirty plant as one neighbour sees it, for the inspector.
#[derive(Clone, Debug, PartialEq)]
pub struct DirtyNeighbour {
    pub source: BuildingId,
    pub name: String,
    /// What it is ("smelter", "coal plant").
    pub what: String,
    pub distance: f32,
    /// What it alone does to the neighbour, `0..1`.
    pub hit: f32,
}

impl World {
    /// Read the dirty plants off the buildings: every business of a dirty
    /// kind (shut ones still scar the place, at half) and every coal plant.
    pub fn refresh_dirty(&mut self) {
        let mut out = Vec::new();
        for (id, b) in self.buildings.iter() {
            if b.construction.is_some() {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            let (as_kind, d, activity) = if b.plant == Some(crate::power::PowerPlantKind::Coal) {
                (IndustryKind::Refinery, COAL_DIRTINESS, if b.is_derelict() { 0.4 } else { 1.0 })
            } else if let Some(z) = &b.business {
                let Some(k) = z.kind.industry() else { continue };
                let d = z.kind.dirtiness();
                if d < industry::DIRTY {
                    continue;
                }
                let staffing = if b.jobs == 0 { 0.0 } else { (b.workers.len() as f32 / b.jobs as f32).clamp(0.0, 1.0) };
                let activity = if z.closed || b.is_derelict() { 0.5 } else { 0.6 + 0.4 * staffing };
                (k, d, activity)
            } else {
                continue;
            };
            out.push(DirtySource { building: id, pos: l.center, as_kind, level: d * activity, reach: industry::dirt_reach(d) });
        }
        self.dirty = out;
    }

    /// Who the building is as a neighbour: its industry kind, else its
    /// land use.
    pub fn neighbour_use(&self, b: BuildingId) -> Option<Neighbour> {
        let bld = self.buildings.get(b)?;
        Some(match bld.business.as_ref().and_then(|z| z.kind.industry()) {
            Some(k) => Neighbour::Industry(k),
            None => Neighbour::Land(bld.land_use),
        })
    }

    /// Each dirty plant's hit on `who` at `p` (not counting `exclude`):
    /// index into `World::dirty`, the hit and the distance.
    fn dirt_hits(&self, p: Vec2, who: Neighbour, exclude: Option<BuildingId>) -> Vec<(usize, f32, f32)> {
        self.dirty
            .iter()
            .enumerate()
            .filter(|(_, s)| Some(s.building) != exclude)
            .filter_map(|(i, s)| {
                let d = s.pos.distance(p);
                let h = industry::hit(s.level, s.reach, industry::sensitivity(who, s.as_kind), d);
                (h > 0.0).then_some((i, h, d))
            })
            .collect()
    }

    /// How exposed `who` at `p` is to the dirty plants around, `0..1`.
    pub fn dirt_exposure_at(&self, p: Vec2, who: Neighbour, exclude: Option<BuildingId>) -> f32 {
        if self.dirty.is_empty() {
            return 0.0;
        }
        industry::combine(self.dirt_hits(p, who, exclude).into_iter().map(|h| h.1))
    }

    /// How exposed a building is to the dirty plants around it.
    pub fn building_exposure(&self, b: BuildingId) -> f32 {
        let (Some(who), Some(l)) = (self.neighbour_use(b), self.building_lot(b)) else { return 0.0 };
        self.dirt_exposure_at(l.center, who, Some(b))
    }

    /// The dirty plants a building suffers from, worst first.
    pub fn unhappy_neighbours(&self, b: BuildingId) -> Vec<DirtyNeighbour> {
        let (Some(who), Some(l)) = (self.neighbour_use(b), self.building_lot(b)) else { return Vec::new() };
        let mut v: Vec<DirtyNeighbour> = self
            .dirt_hits(l.center, who, Some(b))
            .into_iter()
            .filter_map(|(i, hit, distance)| {
                let s = &self.dirty[i];
                let src = self.buildings.get(s.building)?;
                let what = match (&src.business, src.plant) {
                    (Some(z), _) => z.kind.label().to_string(),
                    (None, Some(k)) => k.label().to_string(),
                    _ => "plant".to_string(),
                };
                Some(DirtyNeighbour { source: s.building, name: src.name.clone(), what, distance, hit })
            })
            .collect();
        v.sort_by(|a, b| b.hit.total_cmp(&a.hit).then(a.distance.total_cmp(&b.distance)));
        v
    }

    /// What the dirty neighbours cost a building, in words: "food output
    /// −30%, land value −0.09" (`None` when it does not suffer).
    pub fn adjacency_cost(&self, b: BuildingId) -> Option<String> {
        let e = self.building_exposure(b);
        if e < 0.01 {
            return None;
        }
        let mut parts = Vec::new();
        if self.buildings.get(b).and_then(|x| x.business.as_ref()).is_some_and(|z| feels_in_output(z.kind)) {
            parts.push(format!("food output −{:.0}%", 100.0 * ADJACENCY_OUTPUT_HIT * e));
        }
        parts.push(format!("land value −{:.2}", ADJACENCY_VALUE_HIT * e));
        Some(parts.join(", "))
    }

    /// The output factor of a business from its dirty neighbours (1: none).
    pub(super) fn adjacency_output_factor(&self, b: BuildingId) -> f32 {
        match self.buildings.get(b).and_then(|x| x.business.as_ref()) {
            Some(z) if feels_in_output(z.kind) => 1.0 - ADJACENCY_OUTPUT_HIT * self.building_exposure(b),
            _ => 1.0,
        }
    }

    /// Once a day: parks clean the ground under them a little, and the
    /// residents of exposed or poisoned homes lose some mood; a few say so.
    pub(super) fn daily_dirt(&mut self) {
        // Parks.
        if !self.contamination.cells.is_empty() {
            let parks: Vec<(Vec2, f32)> = self
                .buildings
                .values()
                .filter(|b| b.land_use == LandUse::Park && b.construction.is_none())
                .filter_map(|b| self.lots.get(b.lot).map(|l| (l.center, l.polygon.iter().map(|q| q.distance(l.center)).fold(0.0, f32::max))))
                .collect();
            for (c, r) in parks {
                self.contamination.scale_within(c, r, 1.0 - PARK_CLEANUP_PER_DAY);
            }
        }
        // Homes.
        if self.dirty.is_empty() && self.contamination.cells.is_empty() {
            return;
        }
        let day = self.clock.tick / 1440;
        let mut homes: Vec<(BuildingId, f32, Option<DirtyNeighbour>, f32)> = Vec::new();
        for (id, b) in self.buildings.iter() {
            if b.land_use != LandUse::Residential || b.construction.is_some() || b.residents.is_empty() {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            let e = self.dirt_exposure_at(l.center, Neighbour::Land(LandUse::Residential), Some(id));
            let soil = self.contamination.at(l.center);
            let felt = industry::combine([e, soil]);
            if felt < HOME_EXPOSED {
                continue;
            }
            let worst = if e > 0.0 { self.unhappy_neighbours(id).into_iter().next() } else { None };
            homes.push((id, felt, worst, soil));
        }
        // A different few speak up each day.
        homes.sort_by_key(|(id, ..)| (slotmap::Key::data(id).as_ffi() ^ day.wrapping_mul(0x9E37_79B9)).wrapping_mul(0xBF58_476D_1CE4_E5B9));
        let tick = self.clock.tick;
        let mut said = 0;
        for (id, felt, worst, soil) in homes {
            let residents = self.buildings[id].residents.clone();
            for &c in &residents {
                if let Some(x) = self.citizens.get_mut(c) {
                    x.mood = (x.mood - DIRT_MOOD * felt).max(0.0);
                }
            }
            if said >= DIRT_VOICES {
                continue;
            }
            let Some(&p) = residents.first() else { continue };
            let text = match &worst {
                Some(n) if n.hit >= soil => format!("{} is {:.0} m from my door. The kids cough all winter and nothing grows out back.", n.name, n.distance),
                _ => "The soil round here is poison. You can't grow a tomato and the tap water tastes of metal.".to_string(),
            };
            let Some(cz) = self.citizens.get(p) else { continue };
            self.concerns.said(tick, p, cz, crate::concerns::Concern::Noise, (0.25 + 0.5 * felt).min(0.9), text);
            said += 1;
        }
    }

    /// Clean up the ground within `radius` of `center`: the contamination
    /// there is dug out and hauled off, paid from the treasury at
    /// [`CLEANUP_COST`] per cell of full contamination. Returns the cost
    /// and the contamination removed (in cells).
    pub fn remediate(&mut self, center: Vec2, radius: f32) -> Result<(f32, f32), String> {
        let radius = radius.clamp(self.contamination.cell * 0.5, CLEANUP_MAX_RADIUS);
        let amount = self.contamination.within(center, radius);
        if amount < 0.01 {
            return Err("the ground there is clean".into());
        }
        let cost = amount * CLEANUP_COST;
        if self.economy.treasury < cost {
            return Err(format!("cleaning up costs {cost:.0}; the treasury has {:.0}", self.economy.treasury));
        }
        self.contamination.scale_within(center, radius, 0.0);
        self.economy.debit_item(Flow::Construction, cost, Item::Named(format!("ground clean-up, {:.0} m round ({:.0}, {:.0})", radius, center.x, center.y)));
        Ok((cost, amount))
    }
}

/// Farms, fisheries and the food chain feel dirty neighbours in their output.
fn feels_in_output(k: BusinessKind) -> bool {
    k.is_food() && matches!(k.industry(), Some(IndustryKind::Farming | IndustryKind::Fishing))
}

#[cfg(test)]
#[path = "dirt_tests.rs"]
mod tests;
