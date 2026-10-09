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

    /// Minutes of working time (the [`is_work_hours`](Self::is_work_hours)
    /// span, 8:00 to 17:00) since the clock began. It runs with the clock
    /// through the working day and stands still from knocking-off time to
    /// the next morning, so anything driven by it (a tower crane slewing
    /// between lifts) works by day and rests by night, carrying on in the
    /// morning from where it stopped.
    pub fn work_minutes(&self) -> u64 {
        const START: u64 = 8 * 60;
        const END: u64 = 17 * 60;
        self.day() * (END - START) + self.minute_of_day().clamp(START, END) - START
    }

    pub fn is_evening(&self) -> bool {
        (17..23).contains(&self.hour())
    }

    pub fn format_hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour(), self.minute())
    }
}

/// Opening hours on the clock, `open..close`; a close earlier than the open
/// runs past midnight, and equal hours mean round the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hours {
    pub open: u8,
    pub close: u8,
}

impl Hours {
    pub const ALWAYS: Hours = Hours { open: 0, close: 0 };

    pub const fn new(open: u8, close: u8) -> Self {
        Self { open, close }
    }

    pub fn always(self) -> bool {
        self.open == self.close
    }

    /// Is the hour `h` (0..24) inside?
    pub fn contains(self, h: u8) -> bool {
        if self.always() {
            true
        } else if self.open < self.close {
            (self.open..self.close).contains(&h)
        } else {
            h >= self.open || h < self.close
        }
    }

    /// Hours open per day.
    pub fn span(self) -> u8 {
        if self.always() {
            24
        } else {
            (self.close + 24 - self.open) % 24
        }
    }

    /// The shift a worker with `seed` works: the whole day when it is ten
    /// hours or less, else one of two or three back-to-back shifts.
    pub fn shift(self, seed: u64) -> Hours {
        let span = self.span();
        if span <= 10 {
            return self;
        }
        let n = span.div_ceil(8);
        let len = span / n;
        let i = (seed % n as u64) as u8;
        let open = (self.open + i * len) % 24;
        let close = if i + 1 == n { self.close } else { (open + len) % 24 };
        Hours { open, close }
    }

    pub fn label(self) -> String {
        if self.always() {
            "24h".to_string()
        } else {
            format!("{:02}:00-{:02}:00", self.open, self.close)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_minutes_run_by_day_and_rest_by_night() {
        let at = |d, h, m| SimClock::starting_at(d, h, m).work_minutes();
        assert_eq!(at(0, 0, 0), 0);
        assert_eq!(at(0, 8, 0), 0);
        assert_eq!(at(0, 8, 30), 30);
        assert_eq!(at(0, 17, 0), 540);
        // Still all evening and night, and the morning carries on from there.
        assert_eq!(at(0, 23, 59), 540);
        assert_eq!(at(1, 7, 59), 540);
        assert_eq!(at(1, 8, 1), 541);
        // Never runs backwards.
        let mut c = SimClock::default();
        let mut last = 0;
        for _ in 0..3 * MINUTES_PER_DAY {
            c.advance();
            let w = c.work_minutes();
            assert!(w >= last && w - last <= 1);
            assert_eq!(w > last, (8 * 60 + 1..=17 * 60).contains(&c.minute_of_day()), "at {}", c.format_hhmm());
            last = w;
        }
    }
}
