//! [megablocks] Mega-blocks: a mega-corporation's flagship, a self-contained
//! arcology on a block of plots it has bought up. A podium of street halls
//! (shops, a market hall of food counters, car decks) under offices, with
//! residential towers rising from it, sky gardens on the setbacks and bridges
//! between the towers. It houses thousands; its residents can live, work,
//! shop and relax without going out on the street (`world/megablocks.rs`).
//!
//! The record lives on the building (`estate::Plot::megablock`), the
//! city-wide tallies on the `World` (`MegaBlockStats`). Everything is a
//! hash of the block's seed: no draws from the world's stream. Saved with
//! serde defaults: older saves load with none.

use crate::estate::OwnerId;
use crate::hash::{hash01, mix};
use serde::{Deserialize, Serialize};

// ── tuning ───────────────────────────────────────────────────────────────────

/// People a town needs before a corp builds one.
pub const MEGABLOCK_MIN_POP: usize = 5000;
/// Days a corp must have traded as one, and holdings it must have.
pub const MEGABLOCK_CORP_MIN_DAYS: u32 = 10;
pub const MEGABLOCK_MIN_HOLDINGS: usize = 8;
/// Chance a day a corp that may build one breaks ground (one rising at a
/// time across the city).
pub const MEGABLOCK_DAILY_CHANCE: f32 = 0.15;
/// The site: at least this big (m²), aiming for this, at most this many
/// plots, compact (short side at least `MIN_SIDE` m and `MIN_ASPECT` of the
/// long side, the hull at most `HULL_SLACK` of the plots' area).
pub const MEGABLOCK_MIN_AREA: f32 = 4500.0;
pub const MEGABLOCK_TARGET_AREA: f32 = 9000.0;
pub const MEGABLOCK_MAX_LOTS: usize = 32;
pub const MEGABLOCK_MIN_SIDE: f32 = 48.0;
pub const MEGABLOCK_MIN_ASPECT: f32 = 0.42;
pub const MEGABLOCK_HULL_SLACK: f32 = 1.45;
/// The plots it buys hold buildings no taller than this.
pub const MEGABLOCK_BUY_MAX_FLOORS: u8 = 24;
/// Calendar days of work at full pace (four stages, a delivery each).
pub const MEGABLOCK_BUILD_DAYS: f32 = 40.0;
/// Podium storeys: two of street halls (shops), a market hall, offices.
pub const PODIUM_FLOORS: u8 = 6;
/// Tower storeys above the podium: the tallest tower, by the height limit.
pub const TOWER_MIN_FLOORS: u8 = 22;
pub const TOWER_MAX_FLOORS: u8 = 44;
/// Homes a tower storey holds (small flats round a core).
pub const HOMES_PER_TOWER_FLOOR: f32 = 10.0;
pub const MAX_HOMES: u16 = 2400;
/// A sky garden every this many tower storeys (and one on every roof).
pub const GARDEN_EVERY: u8 = 12;
/// Shop units in the street halls, counters in the market hall, office
/// suites in the podium.
pub const MIN_SHOPS: usize = 12;
pub const MAX_SHOPS: usize = 32;
pub const MARKET_COUNTERS: usize = 6;
pub const MIN_OFFICES: usize = 4;
pub const MAX_OFFICES: usize = 10;
/// Shop floor area per job, office floor area per job (m²).
pub const SHOP_M2_PER_JOB: f32 = 45.0;
pub const OFFICE_M2_PER_JOB: f32 = 18.0;
/// Units let a day across the city's mega-blocks (on top of the streets').
pub const LETS_PER_DAY: usize = 5;
/// Residents working outside who take a free job inside, a day per block.
pub const JOB_MOVES_PER_DAY: usize = 30;
/// Chance a resident at home who fancies a park goes up to the sky garden
/// instead of out.
pub const GARDEN_CHANCE: f32 = 0.8;
/// Utility the corp's brand adds to its own shops (and those in its block)
/// while its mega-block stands.
pub const BRAND_PULL: f32 = 0.12;
/// The land round it: worth more out to `LIFT_RADIUS` (m), shaded close in.
pub const LIFT_RADIUS: f32 = 340.0;
pub const LIFT_WEIGHT: f32 = 0.07;
pub const SHADE_RADIUS: f32 = 110.0;
pub const SHADE_WEIGHT: f32 = 0.05;
/// Car decks in the podium: spaces per home.
pub const PARKING_PER_HOME: f32 = 0.2;

/// The four stages of the build, one per quarter of the work (each waits
/// for its delivery of materials, `buildings::Construction::stage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Piling,
    Podium,
    Towers,
    FitOut,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Piling, Stage::Podium, Stage::Towers, Stage::FitOut];

    pub fn of_progress(p: f32) -> Stage {
        Stage::ALL[((p.clamp(0.0, 0.9999) / 0.25) as usize).min(3)]
    }

    pub fn key(self) -> &'static str {
        match self {
            Stage::Piling => "piling",
            Stage::Podium => "podium",
            Stage::Towers => "towers",
            Stage::FitOut => "fit_out",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Piling => "clearing the block, piling the raft",
            Stage::Podium => "the podium: street halls, market, car decks",
            Stage::Towers => "the towers rising",
            Stage::FitOut => "fit-out: flats, offices, sky gardens",
        }
    }
}

/// A mega-block: what it is and how it is doing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MegaBlock {
    /// Its own seed (the building's changes when it is rebuilt).
    pub seed: u64,
    /// The mega-corp that built it (its owner record).
    pub corp: OwnerId,
    /// Towers on the podium, and the tallest's storeys above it.
    pub towers: u8,
    pub tower_floors: u8,
    pub podium_floors: u8,
    pub homes: u16,
    pub shops: u8,
    pub offices: u8,
    pub started_day: u32,
    #[serde(default)]
    pub opened_day: Option<u32>,
    /// Trips today (and yesterday) that never left the block: home to a
    /// job, a shop, a bar inside; residents' trips out to the streets; visits
    /// to the sky gardens.
    #[serde(default)]
    pub inside_today: u32,
    #[serde(default)]
    pub out_today: u32,
    #[serde(default)]
    pub gardens_today: u32,
    #[serde(default)]
    pub inside_yesterday: u32,
    #[serde(default)]
    pub out_yesterday: u32,
    #[serde(default)]
    pub gardens_yesterday: u32,
    #[serde(default)]
    pub inside_total: u64,
    /// Residents who swapped a job outside for one inside.
    #[serde(default)]
    pub moved_in_jobs_total: u32,
}

impl MegaBlock {
    /// Plan one on a site of `area` m² whose tallest tower may have
    /// `max_tower_floors` storeys over the podium.
    pub fn plan(seed: u64, corp: OwnerId, area: f32, max_tower_floors: u8, day: u32) -> MegaBlock {
        let towers = ((area / 2000.0).round() as u8).clamp(3, 6);
        let want = TOWER_MIN_FLOORS + ((TOWER_MAX_FLOORS - TOWER_MIN_FLOORS) as f32 * (0.55 + 0.45 * hash01(seed, 0x3E6A, 1))) as u8;
        let tower_floors = want.min(max_tower_floors).max(12);
        let mut mb = MegaBlock {
            seed,
            corp,
            towers,
            tower_floors,
            podium_floors: PODIUM_FLOORS,
            homes: 0,
            shops: ((area / 320.0).round() as usize).clamp(MIN_SHOPS, MAX_SHOPS) as u8,
            offices: (towers as usize * 2).clamp(MIN_OFFICES, MAX_OFFICES) as u8,
            started_day: day,
            opened_day: None,
            inside_today: 0,
            out_today: 0,
            gardens_today: 0,
            inside_yesterday: 0,
            out_yesterday: 0,
            gardens_yesterday: 0,
            inside_total: 0,
            moved_in_jobs_total: 0,
        };
        let floors: f32 = (0..towers).map(|i| mb.floors_of(i) as f32).sum();
        mb.homes = ((floors * HOMES_PER_TOWER_FLOOR) as u16).min(MAX_HOMES);
        mb
    }

    /// Storeys of tower `i` above the podium: the first is the tallest, the
    /// others step down by hash (a cluster, not a row of equals).
    pub fn floors_of(&self, i: u8) -> u8 {
        if i == 0 {
            return self.tower_floors;
        }
        let k = 0.55 + 0.4 * hash01(self.seed, 0x70E5, i as u64);
        ((self.tower_floors as f32 * k) as u8).max(10)
    }

    /// Every storey of the building: the podium and the tallest tower.
    pub fn total_floors(&self) -> u8 {
        self.podium_floors.saturating_add(self.tower_floors)
    }

    /// Sky gardens: one every [`GARDEN_EVERY`] storeys up each tower, and
    /// every roof (the podium roof's park counted once).
    pub fn gardens(&self) -> u32 {
        1 + (0..self.towers).map(|i| 1 + (self.floors_of(i) / GARDEN_EVERY) as u32).sum::<u32>()
    }

    /// Sky bridges between towers: one per neighbouring pair, by hash at
    /// one or two heights.
    pub fn bridges(&self) -> u32 {
        (1..self.towers).map(|i| 1 + (mix(self.seed, 0xB21D ^ i as u64) % 2) as u32).sum()
    }

    /// Roll the day's tallies into yesterday's.
    pub fn roll_day(&mut self) {
        self.inside_yesterday = self.inside_today;
        self.out_yesterday = self.out_today;
        self.gardens_yesterday = self.gardens_today;
        self.inside_today = 0;
        self.out_today = 0;
        self.gardens_today = 0;
    }

    /// Share of yesterday's trips by its residents and workers that stayed
    /// inside (0 with none).
    pub fn inside_share(&self) -> f32 {
        let all = self.inside_yesterday + self.out_yesterday;
        if all == 0 { 0.0 } else { self.inside_yesterday as f32 / all as f32 }
    }
}

/// Jobs a unit of `area` m² holds: a shop or market counter, or offices.
pub fn unit_jobs(shopfront: bool, area: f32) -> u16 {
    if shopfront {
        ((area / SHOP_M2_PER_JOB).round() as u16).clamp(3, 24)
    } else {
        ((area / OFFICE_M2_PER_JOB).round() as u16).clamp(8, 140)
    }
}

/// The units of a mega-block whose podium storey is `plate` m², all to
/// let: shops along the two street halls (ground and first floor), the
/// market hall's food counters (second floor), office suites over them
/// (third floor to the podium roof).
pub fn layout(m: &MegaBlock, plate: f32) -> Vec<crate::buildings::Unit> {
    use crate::buildings::{Occupant, Unit, UnitRole};
    let unit = |lo: u8, hi: u8, share: f32, shopfront: bool, role: UnitRole| Unit { lo, hi, share, area: plate * share * (hi - lo + 1) as f32, shopfront, jobs: 0, occupant: Occupant::Vacant, vacant_since: 0, role, sign: None };
    let mut v = Vec::new();
    // The halls: shops take 70% of each of the two storeys (the rest is the
    // halls, the lifts and the car ramps).
    let n = m.shops.max(1) as usize;
    let below = n.div_ceil(2);
    for (f, k) in [(0u8, below), (1u8, n - below)] {
        for _ in 0..k {
            v.push(unit(f, f, 0.7 / k.max(1) as f32, true, UnitRole::Shop));
        }
    }
    // The market hall: food counters round the tables.
    for _ in 0..MARKET_COUNTERS {
        v.push(unit(2, 2, 0.04, true, UnitRole::FoodCourt));
    }
    // Office suites over it, each a share of the upper podium storeys.
    let top = m.podium_floors.saturating_sub(1).max(3);
    let k = m.offices.max(1) as usize;
    for _ in 0..k {
        v.push(unit(3, top, 0.8 / k as f32, false, UnitRole::Shop));
    }
    v
}

/// City-wide tallies (`World::megablocks`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MegaBlockStats {
    pub started_total: u32,
    pub opened_total: u32,
    pub inside_trips_total: u64,
    pub garden_visits_total: u64,
    pub jobs_moved_inside_total: u32,
    /// Why the last attempt to start one did not (for the API).
    pub last_refusal: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_houses_thousands_within_its_caps() {
        for seed in 0..40u64 {
            for area in [5500.0f32, 9000.0, 16000.0] {
                let m = MegaBlock::plan(seed, OwnerId::default(), area, 44, 0);
                assert!((3..=6).contains(&m.towers));
                assert!(m.homes >= 600 && m.homes <= MAX_HOMES, "{seed} {area}: {} homes", m.homes);
                assert!((MIN_SHOPS..=MAX_SHOPS).contains(&(m.shops as usize)));
                assert!(m.gardens() > m.towers as u32);
                assert!((0..m.towers).all(|i| m.floors_of(i) <= m.tower_floors && m.floors_of(i) >= 10));
            }
        }
        // A height limit holds the towers down.
        let low = MegaBlock::plan(3, OwnerId::default(), 9000.0, 14, 0);
        assert_eq!(low.tower_floors, 14);
    }

    #[test]
    fn stages_follow_the_quarters() {
        assert_eq!(Stage::of_progress(0.0), Stage::Piling);
        assert_eq!(Stage::of_progress(0.3), Stage::Podium);
        assert_eq!(Stage::of_progress(0.6), Stage::Towers);
        assert_eq!(Stage::of_progress(0.99), Stage::FitOut);
        assert_eq!(Stage::of_progress(1.0), Stage::FitOut);
    }
}
