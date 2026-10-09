//! Grid batteries: their kinds and state, the charge modes and the price
//! bands time-of-use charging follows.

use super::*;

/// Share of the energy drawn into a battery that is stored, and of the
/// stored energy that reaches the grid again: about 90% round trip, as for
/// a lithium-iron pack with its inverters and cooling.
pub const BATTERY_CHARGE_EFF: f32 = 0.95;

pub const BATTERY_DISCHARGE_EFF: f32 = 0.95;

/// Grid battery packs the player places under Buildings, Power. They make
/// no power: they soak up surplus (before it is exported) and give it back
/// in a shortfall (before power is imported), so a solar noon can carry
/// into the evening peak and a calm hour need not go to the regional grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BatteryKind {
    Small,
    Medium,
    Large,
}

impl BatteryKind {
    pub const ALL: [BatteryKind; 3] = [BatteryKind::Small, BatteryKind::Medium, BatteryKind::Large];

    pub fn label(self) -> &'static str {
        match self {
            BatteryKind::Small => "small battery pack",
            BatteryKind::Medium => "battery pack",
            BatteryKind::Large => "battery farm",
        }
    }

    /// The remote API's name for it.
    pub fn key(self) -> &'static str {
        match self {
            BatteryKind::Small => "battery_small",
            BatteryKind::Medium => "battery_medium",
            BatteryKind::Large => "battery_large",
        }
    }

    pub fn from_key(s: &str) -> Option<BatteryKind> {
        match s {
            "battery_small" | "small_battery" => Some(BatteryKind::Small),
            "battery_medium" | "battery" | "medium_battery" => Some(BatteryKind::Medium),
            "battery_large" | "large_battery" | "battery_farm" => Some(BatteryKind::Large),
            _ => None,
        }
    }

    /// Energy it holds when full, MWh.
    pub fn capacity_mwh(self) -> f32 {
        match self {
            BatteryKind::Small => 20.0,
            BatteryKind::Medium => 80.0,
            BatteryKind::Large => 240.0,
        }
    }

    /// Fastest it charges or discharges, MW at the grid side: four hours
    /// from full to empty at full power for the packs, three for the farm.
    pub fn max_mw(self) -> f32 {
        match self {
            BatteryKind::Small => 5.0,
            BatteryKind::Medium => 20.0,
            BatteryKind::Large => 80.0,
        }
    }

    pub fn build_cost(self) -> f32 {
        match self {
            BatteryKind::Small => 6_000.0,
            BatteryKind::Medium => 20_000.0,
            BatteryKind::Large => 55_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            BatteryKind::Small => 30.0,
            BatteryKind::Medium => 90.0,
            BatteryKind::Large => 240.0,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            BatteryKind::Small => 1,
            BatteryKind::Medium => 2,
            BatteryKind::Large => 4,
        }
    }

    pub fn min_area(self) -> f32 {
        match self {
            BatteryKind::Small => 150.0,
            BatteryKind::Medium => 350.0,
            BatteryKind::Large => 800.0,
        }
    }

    /// Anywhere but parks: a fenced yard of cabinets sits as well beside
    /// homes as beside a factory.
    pub fn allowed_on(self, lu: LandUse) -> bool {
        lu != LandUse::Park
    }

    /// Hum of the inverters and the cooling fans at the fence, dB(A).
    pub fn noise_db(self) -> f32 {
        match self {
            BatteryKind::Small => 40.0,
            BatteryKind::Medium => 44.0,
            BatteryKind::Large => 48.0,
        }
    }
}

/// A battery pack's state, on its building.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Battery {
    pub kind: BatteryKind,
    /// Energy stored, MWh, `0..=capacity`.
    #[serde(default)]
    pub charge_mwh: f32,
    /// This hour's flow at the grid side, MW: positive discharging into the
    /// grid, negative charging from it.
    #[serde(default)]
    pub flow_mw: f32,
    /// [batteries] Its share of what the batteries saved today and
    /// yesterday (by its share of the hour's flow).
    #[serde(default)]
    pub savings_today: f32,
    #[serde(default)]
    pub savings_yesterday: f32,
}

impl Battery {
    pub fn new(kind: BatteryKind) -> Self {
        Self { kind, charge_mwh: 0.0, flow_mw: 0.0, savings_today: 0.0, savings_yesterday: 0.0 }
    }

    /// Share full, `0..1`.
    pub fn level(&self) -> f32 {
        (self.charge_mwh / self.kind.capacity_mwh()).clamp(0.0, 1.0)
    }

    /// What its status lights show, coarsely: fifths of charge (rounded
    /// up) and whether it is charging (-1), holding (0) or delivering (1).
    /// The renderer redraws the pack when this changes.
    pub fn status(&self) -> (u8, i8) {
        let fifths = ((self.level() * 5.0) - 1e-3).ceil().max(0.0) as u8;
        let flow = if self.flow_mw < -0.05 { -1 } else if self.flow_mw > 0.05 { 1 } else { 0 };
        (fifths, flow)
    }

    /// Most it can take from the grid this hour, MW.
    pub fn room_mw(&self) -> f32 {
        ((self.kind.capacity_mwh() - self.charge_mwh).max(0.0) / BATTERY_CHARGE_EFF).min(self.kind.max_mw())
    }

    /// Most it can deliver to the grid this hour, MW.
    pub fn available_mw(&self) -> f32 {
        (self.charge_mwh.max(0.0) * BATTERY_DISCHARGE_EFF).min(self.kind.max_mw())
    }

    /// Run one hour at `flow` MW (grid side; positive discharging): the
    /// stored energy moves by the flow less the conversion losses.
    pub fn run_hour(&mut self, flow: f32) {
        self.flow_mw = flow;
        let stored = if flow < 0.0 { -flow * BATTERY_CHARGE_EFF } else { -flow / BATTERY_DISCHARGE_EFF };
        self.charge_mwh = (self.charge_mwh + stored).clamp(0.0, self.kind.capacity_mwh());
    }
}

/// [batteries] How many of the day's hours count as cheap (the batteries
/// buy from the regional grid) and as dear (they deliver first).
pub const ARBITRAGE_HOURS: usize = 6;

/// Buying cheap to give back dear must clear the round-trip losses and
/// this margin on top, or the batteries do not buy from the grid at all.
pub const ARBITRAGE_MARGIN: f32 = 0.9;

/// [batteries] What the batteries do with the grid this hour, read off the
/// day's price curve ([`PriceBands::mode`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatteryMode {
    /// A cheap hour: take surplus, and buy from the regional grid to fill up.
    Charge,
    /// A middle hour: take surplus, keep the charge for the dear hours (give
    /// only what keeps the lights on past the import line).
    #[default]
    Hold,
    /// A dear hour: deliver before anything is imported.
    Deliver,
}

impl BatteryMode {
    pub fn label(self) -> &'static str {
        match self {
            BatteryMode::Charge => "cheap hour: charging from the grid",
            BatteryMode::Hold => "middle hour: holding for the peak",
            BatteryMode::Deliver => "dear hour: delivering first",
        }
    }
}

/// [batteries] The day's price thresholds: an hour at or under `cheap`
/// (the [`ARBITRAGE_HOURS`]th cheapest of the day) is for charging, one at
/// or over `dear` (the [`ARBITRAGE_HOURS`]th dearest) is for delivering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceBands {
    pub cheap: f32,
    pub dear: f32,
}

impl PriceBands {
    /// The bands of a day's hourly prices (any length; 24 in the game).
    pub fn of(prices: &[f32]) -> Self {
        if prices.is_empty() {
            return Self::default();
        }
        let mut s = prices.to_vec();
        s.sort_by(f32::total_cmp);
        let k = ARBITRAGE_HOURS.min(s.len()).max(1);
        Self { cheap: s[k - 1], dear: s[s.len() - k] }
    }

    /// Whether power bought in the cheap hours is worth giving back in the
    /// dear ones, after the round trip's losses and a margin.
    pub fn worth_storing(&self) -> bool {
        self.cheap < self.dear * BATTERY_CHARGE_EFF * BATTERY_DISCHARGE_EFF * ARBITRAGE_MARGIN
    }

    /// The rule for an hour at `price`.
    pub fn mode(&self, price: f32) -> BatteryMode {
        if price >= self.dear && self.dear > self.cheap {
            BatteryMode::Deliver
        } else if price <= self.cheap && self.worth_storing() {
            BatteryMode::Charge
        } else if self.dear <= self.cheap {
            // A flat day: nothing to play, the old merit order.
            BatteryMode::Deliver
        } else {
            BatteryMode::Hold
        }
    }
}
