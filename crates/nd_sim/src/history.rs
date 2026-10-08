//! The city's recent past: a daily and an hourly ring of headline numbers,
//! kept in the save so the expanded info windows can chart income and
//! spending by flow, population, power by plant kind and ridership by line
//! over the days, and the remote API can hand the same series out.
//!
//! Why in the sim and not the app: a chart that starts empty after every
//! load is little use for "where has the money been going this month", and
//! the numbers that need closing at midnight (the ledger, the day's riders,
//! the day's energy) are only whole inside the tick that rolls them over.
//! The rings are bounded (`DAYS_KEPT`, `HOURS_KEPT`), so a long game's save
//! grows by a fixed amount, and everything is `serde(default)` so older
//! saves load with an empty history.
//!
//! Recording is a pure read of the world plus a push, at fixed ticks
//! (`World::tick`): it draws no random numbers and changes no state the
//! simulation reads, so it cannot disturb determinism.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::buildings::BuildingId;
use crate::economy::{Flow, FLOWS};
use crate::transit::RouteId;

/// Days kept in the daily ring: a year of play.
pub const DAYS_KEPT: usize = 365;
/// Hours kept in the hourly ring: three days, enough to see the solar day
/// and the commuter peaks repeat.
pub const HOURS_KEPT: usize = 72;
/// Days kept in each plant's and battery's own ring: a season, enough to
/// see what a plant earns its upkeep with, without a year per plant in the
/// save.
pub const PLANT_DAYS_KEPT: usize = 90;

/// One closed day. Counters ("riders", "imports", the ledger) are the
/// day's totals; levels ("population", "treasury") are as the day closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayRecord {
    /// The 1-based day this record closes.
    pub day: u32,
    /// Treasury after the midnight upkeep was paid.
    pub treasury: f32,
    /// The day's ledger row by `Flow::index`. A `Vec`, not an array, so
    /// records saved before a new flow existed still load (read through
    /// `flow`, which treats missing entries as zero).
    pub ledger: Vec<f32>,
    /// Income tax by the payer's class (low, middle, high).
    pub income_tax_by_class: [f32; 3],
    /// Business tax by the land use of the business (`LandUse::index`).
    pub business_tax_by_use: [f32; 6],
    pub population: u32,
    pub households: u32,
    pub working_age: u32,
    /// Citizens with a job.
    pub employed: u32,
    pub unemployed: u32,
    pub unhoused: u32,
    pub waiting_immigrants: u32,
    /// Citizens per income class.
    pub classes: [u32; 3],
    pub homes: u32,
    pub homes_filled: u32,
    pub jobs: u32,
    pub jobs_filled: u32,
    /// Finished buildings and building sites.
    pub buildings: u32,
    pub sites: u32,
    pub avg_land_value: f32,
    pub gini: f32,
    pub avg_rent: [f32; 3],
    /// Development demand per land use (`LandUse::index`).
    pub demand: [f32; 6],
    pub businesses: u32,
    pub imports: f32,
    pub exports: f32,
    /// Building materials bought from outside.
    pub materials: f32,
    pub deals: u32,
    /// The day's energy by plant kind (`PowerPlantKind::index`), MWh.
    pub supply_mwh: [f32; 7],
    pub demand_mwh: f32,
    pub imported_mwh: f32,
    /// Sold to the regional grid, MWh.
    pub exported_mwh: f32,
    /// Into and out of the batteries (grid side), MWh.
    pub battery_in_mwh: f32,
    pub battery_out_mwh: f32,
    /// [batteries] What the batteries saved the city that day.
    pub battery_savings: f32,
    pub peak_demand_mw: f32,
    pub brownout_hours: u32,
    pub bus_riders: u32,
    pub rail_riders: u32,
    pub ferry_riders: u32,
    pub air_riders: u32,
    /// Riders per bus line.
    pub route_riders: Vec<(RouteId, u32)>,
    /// 911 calls by kind (fire, crime, medical), and those resolved.
    pub incidents: [u32; 3],
    pub resolved: [u32; 3],
    pub unanswered: u32,
    pub out_of_town_calls: u32,
    /// Mean minutes from call to arrival.
    pub avg_response_min: f32,
}

impl DayRecord {
    /// The day's total under `flow` (zero for a flow newer than the record).
    pub fn flow(&self, flow: Flow) -> f32 {
        self.ledger.get(flow.index()).copied().unwrap_or(0.0)
    }

    pub fn income(&self) -> f32 {
        self.ledger.iter().filter(|x| **x > 0.0).sum()
    }

    /// Spending, as a positive number.
    pub fn spending(&self) -> f32 {
        -self.ledger.iter().filter(|x| **x < 0.0).sum::<f32>()
    }

    pub fn net(&self) -> f32 {
        self.ledger.iter().sum()
    }

    pub fn riders_on(&self, route: RouteId) -> u32 {
        self.route_riders.iter().find(|(r, _)| *r == route).map_or(0, |(_, n)| *n)
    }

    pub fn supply_total_mwh(&self) -> f32 {
        self.supply_mwh.iter().sum()
    }
}

/// One hour, sampled on the hour after power is shared out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HourRecord {
    pub day: u32,
    pub hour: u8,
    pub treasury: f32,
    pub population: u32,
    pub supply_mw: f32,
    /// Supply by plant kind (`PowerPlantKind::index`).
    pub supply_by_kind: [f32; 7],
    pub demand_mw: f32,
    pub imported_mw: f32,
    /// Sold to the regional grid.
    pub exported_mw: f32,
    /// Surplus nobody took: plants turned down.
    pub curtailed_mw: f32,
    /// Into and out of the batteries (grid side), and what they held at
    /// the end of the hour against what they could.
    pub battery_in_mw: f32,
    pub battery_out_mw: f32,
    pub battery_charge_mwh: f32,
    pub battery_capacity_mwh: f32,
    /// [batteries] What they saved this hour.
    pub battery_savings: f32,
    pub served: f32,
    /// [grid-weather] The regional grid's import price this hour, per MWh,
    /// and the air temperature, °C.
    pub import_price: f32,
    pub temp_c: f32,
    pub bus_waiting: u32,
    pub bus_riding: u32,
    pub rail_riding: u32,
    pub cars: u32,
}

/// Running sums for the day in progress, folded from the hourly samples.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayAcc {
    pub supply_mwh: [f32; 7],
    pub demand_mwh: f32,
    pub imported_mwh: f32,
    pub exported_mwh: f32,
    pub battery_in_mwh: f32,
    pub battery_out_mwh: f32,
    pub battery_savings: f32,
    pub peak_demand_mw: f32,
}

/// One plant's or battery's own record, for the Power window's table and
/// per-plant charts: its output hour by hour (a battery's flow, positive
/// delivering, negative charging, and its charge), and its energy per day.
/// Kept by building and dropped when the building goes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlantLog {
    pub id: BuildingId,
    /// MW each hour, the last [`HOURS_KEPT`], oldest first.
    pub hours: VecDeque<f32>,
    /// A battery's stored MWh at the end of each hour (empty for a plant).
    pub level: VecDeque<f32>,
    /// MWh each closed day (a battery: delivered), the last
    /// [`PLANT_DAYS_KEPT`].
    pub days: VecDeque<f32>,
    /// MWh so far today.
    pub today_mwh: f32,
}

impl PlantLog {
    /// Mean output over the last `n` hours (fewer if it is newer), MW.
    pub fn mean_mw(&self, n: usize) -> f32 {
        let k = self.hours.len().min(n);
        if k == 0 {
            return 0.0;
        }
        self.hours.iter().skip(self.hours.len() - k).sum::<f32>() / k as f32
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub days: VecDeque<DayRecord>,
    pub hours: VecDeque<HourRecord>,
    pub acc: DayAcc,
    /// Every standing plant's and battery's own record, in the order they
    /// were first recorded.
    pub plants: Vec<PlantLog>,
}

impl History {
    /// Add an hour: into the ring, and into the day's energy sums.
    pub fn push_hour(&mut self, h: HourRecord) {
        for (a, x) in self.acc.supply_mwh.iter_mut().zip(h.supply_by_kind) {
            *a += x;
        }
        self.acc.demand_mwh += h.demand_mw;
        self.acc.imported_mwh += h.imported_mw;
        self.acc.exported_mwh += h.exported_mw;
        self.acc.battery_in_mwh += h.battery_in_mw;
        self.acc.battery_out_mwh += h.battery_out_mw;
        self.acc.battery_savings += h.battery_savings;
        self.acc.peak_demand_mw = self.acc.peak_demand_mw.max(h.demand_mw);
        self.hours.push_back(h);
        while self.hours.len() > HOURS_KEPT {
            self.hours.pop_front();
        }
    }

    /// Close a day: the energy sums go into the record and start again.
    pub fn push_day(&mut self, mut d: DayRecord) {
        let acc = std::mem::take(&mut self.acc);
        d.supply_mwh = acc.supply_mwh;
        d.demand_mwh = acc.demand_mwh;
        d.imported_mwh = acc.imported_mwh;
        d.exported_mwh = acc.exported_mwh;
        d.battery_in_mwh = acc.battery_in_mwh;
        d.battery_out_mwh = acc.battery_out_mwh;
        d.battery_savings = acc.battery_savings;
        d.peak_demand_mw = acc.peak_demand_mw;
        for p in &mut self.plants {
            p.days.push_back(std::mem::take(&mut p.today_mwh));
            while p.days.len() > PLANT_DAYS_KEPT {
                p.days.pop_front();
            }
        }
        if d.ledger.len() < FLOWS {
            d.ledger.resize(FLOWS, 0.0);
        }
        self.days.push_back(d);
        while self.days.len() > DAYS_KEPT {
            self.days.pop_front();
        }
    }

    /// Add an hour to a plant's or battery's record (`level` for a battery's
    /// charge), starting one for a building seen for the first time. A
    /// battery's day counts what it delivered.
    pub fn push_plant_hour(&mut self, id: BuildingId, mw: f32, level: Option<f32>) {
        let i = match self.plants.iter().position(|p| p.id == id) {
            Some(i) => i,
            None => {
                self.plants.push(PlantLog { id, ..Default::default() });
                self.plants.len() - 1
            }
        };
        let p = &mut self.plants[i];
        p.hours.push_back(mw);
        while p.hours.len() > HOURS_KEPT {
            p.hours.pop_front();
        }
        if let Some(l) = level {
            p.level.push_back(l);
            while p.level.len() > HOURS_KEPT {
                p.level.pop_front();
            }
        }
        p.today_mwh += mw.max(0.0);
    }

    /// Drop the records of buildings that are no longer plants or batteries.
    pub fn retain_plants(&mut self, keep: impl Fn(BuildingId) -> bool) {
        self.plants.retain(|p| keep(p.id));
    }

    pub fn plant(&self, id: BuildingId) -> Option<&PlantLog> {
        self.plants.iter().find(|p| p.id == id)
    }

    /// The last `n` days (all of them if fewer), oldest first.
    pub fn last_days(&self, n: usize) -> impl Iterator<Item = &DayRecord> {
        self.days.iter().skip(self.days.len().saturating_sub(n))
    }

    /// The last `n` hours, oldest first.
    pub fn last_hours(&self, n: usize) -> impl Iterator<Item = &HourRecord> {
        self.hours.iter().skip(self.hours.len().saturating_sub(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_stay_bounded() {
        let mut h = History::default();
        for d in 0..(DAYS_KEPT as u32 + 40) {
            h.push_day(DayRecord { day: d + 1, ..Default::default() });
        }
        for i in 0..(HOURS_KEPT as u32 * 3) {
            h.push_hour(HourRecord { day: i / 24 + 1, hour: (i % 24) as u8, ..Default::default() });
        }
        assert_eq!(h.days.len(), DAYS_KEPT);
        assert_eq!(h.hours.len(), HOURS_KEPT);
        assert_eq!(h.days.front().unwrap().day, 41, "the oldest days fall off the front");
        assert_eq!(h.last_days(7).count(), 7);
        assert_eq!(h.last_days(7).next().unwrap().day, DAYS_KEPT as u32 + 40 - 6);
    }

    #[test]
    fn a_day_takes_the_hours_energy_and_starts_afresh() {
        let mut h = History::default();
        for hour in 0..24u8 {
            let mut s = [0.0; 7];
            s[0] = 100.0;
            h.push_hour(HourRecord { hour, supply_by_kind: s, demand_mw: if hour == 18 { 140.0 } else { 90.0 }, imported_mw: 2.0, ..Default::default() });
        }
        h.push_day(DayRecord { day: 1, ledger: vec![5.0], ..Default::default() });
        let d = h.days.back().unwrap();
        assert_eq!(d.supply_mwh[0], 2400.0);
        assert_eq!(d.demand_mwh, 23.0 * 90.0 + 140.0);
        assert_eq!(d.imported_mwh, 48.0);
        assert_eq!(d.peak_demand_mw, 140.0);
        assert_eq!(d.ledger.len(), FLOWS, "short rows are padded to every flow");
        assert_eq!(h.acc, DayAcc::default());
    }

    #[test]
    fn a_record_from_an_older_save_loads() {
        // Only a few fields, and a ledger row shorter than today's flows.
        let old = "(days: [(day: 3, treasury: 1200.0, ledger: [10.0, -4.0])], hours: [])";
        let h: History = ron::from_str(old).unwrap();
        let d = &h.days[0];
        assert_eq!(d.day, 3);
        assert_eq!(d.flow(Flow::IncomeTax), 10.0);
        assert_eq!(d.flow(Flow::ServiceUpkeep), 0.0);
        assert_eq!(d.net(), 6.0);
        assert_eq!((d.income(), d.spending()), (10.0, 4.0));
        let back: History = ron::from_str(&ron::to_string(&h).unwrap()).unwrap();
        assert_eq!(back.days, h.days);
    }
}
