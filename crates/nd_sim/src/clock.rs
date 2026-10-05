use serde::{Deserialize, Serialize};

pub const MINUTES_PER_DAY: u64 = 24 * 60;

/// Simulation clock. One tick is one simulated minute.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimClock {
    pub tick: u64,
}

impl SimClock {
    pub fn starting_at(day: u64, hour: u64, minute: u64) -> Self {
        Self { tick: day * MINUTES_PER_DAY + hour * 60 + minute }
    }

    pub fn advance(&mut self) {
        self.tick += 1;
    }

    pub fn day(&self) -> u64 {
        self.tick / MINUTES_PER_DAY
    }

    pub fn minute_of_day(&self) -> u64 {
        self.tick % MINUTES_PER_DAY
    }

    pub fn hour(&self) -> u64 {
        self.minute_of_day() / 60
    }

    pub fn minute(&self) -> u64 {
        self.minute_of_day() % 60
    }

    /// Fraction of the day in `0..1`, midnight is 0.
    pub fn day_fraction(&self) -> f32 {
        self.minute_of_day() as f32 / MINUTES_PER_DAY as f32
    }

    pub fn is_night(&self) -> bool {
        let h = self.hour();
        h >= 23 || h < 6
    }

    pub fn is_work_hours(&self) -> bool {
        (8..17).contains(&self.hour())
    }

    pub fn is_evening(&self) -> bool {
        (17..23).contains(&self.hour())
    }

    pub fn format_hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour(), self.minute())
    }
}
