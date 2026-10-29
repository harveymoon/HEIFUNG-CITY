//! [r4-timetable] Timetables and the day's timeline for every line: bus
//! lines, metro lines, the railway's passenger service and each ferry
//! terminal's boats.
//!
//! A [`Timetable`] splits the day into parts: the hours the line runs
//! (`start..end`), with a morning rush and an evening rush inside them, the
//! day around the rushes and the evening after the evening rush, and an
//! optional night service through the hours outside. Each part has its
//! own value: minutes between departures for the metro, the trains and the
//! ferries, buses out for a bus line (extra buses leave the depot as a
//! busier part begins and go home as it ends). A line without one runs
//! as every line ran before timetables: its mode's fixed hours and one
//! headway or fleet all day (`Kind::hours`), so old saves keep their
//! service.
//!
//! A [`Timeline`] is what a line did each hour today and yesterday:
//! riders boarding and their waits, riders waiting and on board (sampled
//! every quarter hour), runs made and riders left at the stop when the
//! service had stopped. Counted, never drawn from the random generator.

use serde::{Deserialize, Serialize};

/// Minutes in a day.
pub const DAY: u32 = 1440;
/// Morning and evening rush by default.
pub const AM_RUSH: (u32, u32) = (6 * 60, 9 * 60);
pub const PM_RUSH: (u32, u32) = (16 * 60, 19 * 60);
/// Part edges snap to this many minutes in the editor.
pub const SNAP: u32 = 15;
/// The timeline samples riders waiting and on board this often (minutes).
pub const SAMPLE_EVERY: u64 = 15;

/// The kinds of line a timetable is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Bus,
    Metro,
    Rail,
    Ferry,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Bus, Kind::Metro, Kind::Rail, Kind::Ferry];

    /// The hours every line of this kind ran before timetables (and a
    /// line without one still runs): buses 05:30–23:30, the metro
    /// 05:00–24:00, trains 05:00–23:00, ferries 06:00–22:00.
    pub fn hours(self) -> (u32, u32) {
        match self {
            Kind::Bus => (crate::transit::SERVICE_START, crate::transit::SERVICE_END),
            Kind::Metro => (crate::subway::SERVICE_START, crate::subway::SERVICE_END),
            Kind::Rail => (crate::rail::SERVICE_START, crate::rail::SERVICE_END),
            Kind::Ferry => (crate::ports::FERRY_SERVICE_START, crate::ports::FERRY_SERVICE_END),
        }
    }

    /// A part's value: buses out for a bus line, else minutes between
    /// departures.
    pub fn per_bus(self) -> bool {
        self == Kind::Bus
    }

    /// The values a part may take.
    pub fn limits(self) -> (u32, u32) {
        match self {
            Kind::Bus => (0, crate::transit::DEPOT_CAPACITY),
            Kind::Metro => (crate::subway::MIN_HEADWAY, crate::subway::MAX_HEADWAY),
            Kind::Rail => (30, 240),
            Kind::Ferry => (30, 240),
        }
    }

    /// A step of the editor's dials.
    pub fn step(self) -> u32 {
        match self {
            Kind::Bus => 1,
            Kind::Metro => 2,
            Kind::Rail | Kind::Ferry => 15,
        }
    }

    /// The default value: the fleet or headway lines of this kind ran.
    pub fn standard(self) -> u32 {
        match self {
            Kind::Bus => 1,
            Kind::Metro => crate::subway::DEFAULT_HEADWAY,
            Kind::Rail => crate::rail::PASSENGER_HEADWAY as u32,
            Kind::Ferry => crate::ports::FERRY_HEADWAY as u32,
        }
    }

    /// Minutes the last run may still call after the service ends before
    /// whoever is still waiting gives up (`transit::LAST_RUN_GRACE`, twice
    /// that for the trains, which cross the map).
    pub fn grace(self) -> u32 {
        match self {
            Kind::Rail => crate::transit::LAST_RUN_GRACE * 2,
            _ => crate::transit::LAST_RUN_GRACE,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Bus => "bus",
            Kind::Metro => "metro",
            Kind::Rail => "rail",
            Kind::Ferry => "ferry",
        }
    }

    /// A part's value in words: "3 buses", "every 10 min".
    pub fn value_text(self, v: u32) -> String {
        match self {
            Kind::Bus => format!("{v} bus{}", if v == 1 { "" } else { "es" }),
            _ => format!("every {v} min"),
        }
    }
}

/// A line that runs to a timetable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineRef {
    Bus(crate::transit::RouteId),
    Metro(crate::subway::MetroLineId),
    /// The railway's passenger service (one for the city).
    Rail,
    /// A ferry terminal's boats, by its building.
    Ferry(crate::buildings::BuildingId),
}

impl LineRef {
    pub fn kind(self) -> Kind {
        match self {
            LineRef::Bus(_) => Kind::Bus,
            LineRef::Metro(_) => Kind::Metro,
            LineRef::Rail => Kind::Rail,
            LineRef::Ferry(_) => Kind::Ferry,
        }
    }
}

/// The parts of a day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Part {
    MorningRush,
    Day,
    EveningRush,
    Evening,
    Night,
}

impl Part {
    pub const ALL: [Part; 5] = [Part::MorningRush, Part::Day, Part::EveningRush, Part::Evening, Part::Night];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Part::MorningRush => "morning rush",
            Part::Day => "day",
            Part::EveningRush => "evening rush",
            Part::Evening => "evening",
            Part::Night => "night",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Part::MorningRush => "morning_rush",
            Part::Day => "day",
            Part::EveningRush => "evening_rush",
            Part::Evening => "evening",
            Part::Night => "night",
        }
    }

    pub fn from_key(s: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.key() == s || p.label() == s)
    }
}

/// When a line runs and how often, part by part (see the module notes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timetable {
    /// Service hours, minutes of the day: `start..end`, `end` up to 1440.
    pub start: u32,
    pub end: u32,
    /// The morning and evening rush, inside the service hours.
    pub am: (u32, u32),
    pub pm: (u32, u32),
    /// A night service runs through the hours outside `start..end`.
    pub night: bool,
    /// Each part's value, by `Part::index`: buses out, or minutes between
    /// departures (see [`Kind::per_bus`]).
    pub values: [u32; 5],
}

/// Ready-made timetables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Preset {
    /// More often in the rushes, less in the evening, nothing at night.
    Commuter,
    /// One headway (or fleet) all through the standard hours, as lines
    /// ran before timetables.
    AllDay,
    /// Round the clock at the same frequency.
    Hours24,
    /// The day as it is, the evening as busy as the day, and a sparse
    /// service through the night.
    NightOwl,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Commuter, Preset::AllDay, Preset::Hours24, Preset::NightOwl];

    pub fn key(self) -> &'static str {
        match self {
            Preset::Commuter => "commuter",
            Preset::AllDay => "all_day",
            Preset::Hours24 => "24h",
            Preset::NightOwl => "night_owl",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Preset::Commuter => "Commuter",
            Preset::AllDay => "All day",
            Preset::Hours24 => "24 hours",
            Preset::NightOwl => "Night owl",
        }
    }

    pub fn from_key(s: &str) -> Option<Preset> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        Preset::ALL.into_iter().find(|p| p.key() == s || p.label().to_ascii_lowercase().replace(' ', "_") == s).or(match s.as_str() {
            "24_hours" | "24hours" | "all_night" => Some(Preset::Hours24),
            "allday" => Some(Preset::AllDay),
            "nightowl" => Some(Preset::NightOwl),
            _ => None,
        })
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Preset::Commuter => "twice as often in the rushes, a little less in the evening, nothing at night",
            Preset::AllDay => "the same all through the standard hours, nothing at night",
            Preset::Hours24 => "the same round the clock",
            Preset::NightOwl => "the evening as busy as the day, and a sparse service all night",
        }
    }

    /// The preset for a line of `kind` whose day value is `base` (its
    /// headway or fleet now).
    pub fn timetable(self, kind: Kind, base: u32) -> Timetable {
        let (lo, hi) = kind.limits();
        let base = base.clamp(lo.max(if kind.per_bus() { 1 } else { lo }), hi);
        let mut t = Timetable::flat(kind, base);
        let (more, fewer, sparse) = if kind.per_bus() {
            ((base * 2).max(base + 1), (base * 2).div_ceil(3).max(1), base.div_ceil(2).max(1))
        } else {
            (base.div_ceil(2), base + base / 2, (base * 2).max(30))
        };
        let v = |x: u32| x.clamp(lo, hi);
        match self {
            Preset::AllDay => {}
            Preset::Commuter => {
                t.values[Part::MorningRush.index()] = v(more);
                t.values[Part::EveningRush.index()] = v(more);
                t.values[Part::Evening.index()] = v(fewer);
            }
            Preset::Hours24 => t.night = true,
            Preset::NightOwl => {
                t.night = true;
                t.values[Part::Night.index()] = v(sparse);
            }
        }
        t
    }
}

impl Timetable {
    /// The standard timetable of a line of `kind` running `value` all
    /// through its kind's hours: what every line did before timetables.
    pub fn flat(kind: Kind, value: u32) -> Timetable {
        let (start, end) = kind.hours();
        Timetable { start, end, am: AM_RUSH, pm: PM_RUSH, night: false, values: [value; 5] }.clamped(kind)
    }

    /// Edges in order and inside the day, every value within the kind's
    /// limits.
    pub fn clamped(mut self, kind: Kind) -> Timetable {
        self.start = self.start.min(DAY - SNAP);
        self.end = self.end.clamp(self.start + SNAP, DAY);
        self.am.0 = self.am.0.clamp(self.start, self.end);
        self.am.1 = self.am.1.clamp(self.am.0, self.end);
        self.pm.0 = self.pm.0.clamp(self.am.1, self.end);
        self.pm.1 = self.pm.1.clamp(self.pm.0, self.end);
        let (lo, hi) = kind.limits();
        for v in &mut self.values {
            *v = (*v).clamp(lo, hi);
        }
        self
    }

    /// The part of the day minute `m` falls in, `None` while the line does
    /// not run.
    pub fn part_at(&self, m: u32) -> Option<Part> {
        let m = m % DAY;
        if !(self.start..self.end).contains(&m) {
            return self.night.then_some(Part::Night);
        }
        Some(if (self.am.0..self.am.1).contains(&m) {
            Part::MorningRush
        } else if (self.pm.0..self.pm.1).contains(&m) {
            Part::EveningRush
        } else if m >= self.pm.1 {
            Part::Evening
        } else {
            Part::Day
        })
    }

    pub fn value(&self, p: Part) -> u32 {
        self.values[p.index()]
    }

    /// The value in force at minute `m` (buses out, or the headway), `None`
    /// while the line does not run.
    pub fn value_at(&self, m: u32) -> Option<u32> {
        self.part_at(m).map(|p| self.value(p))
    }

    /// Does the line run at minute `m`? (A bus line whose part runs no
    /// buses does not.)
    pub fn runs_at(&self, m: u32, kind: Kind) -> bool {
        self.value_at(m).is_some_and(|v| !kind.per_bus() || v > 0)
    }

    /// [r4-metroloop] Has the service stopped for the night at least
    /// `grace` minutes ago (`transit::service_over` on the line's own
    /// hours)? Never with a night service, nor once the first run of the
    /// morning is due within `grace` (a rider down for the first train
    /// waits for it).
    pub fn service_over(&self, m: u32, grace: u32) -> bool {
        let m = m % DAY;
        !self.night && crate::transit::service_over(m, self.start, self.end, grace) && (self.start + DAY - m) % DAY > grace
    }

    /// The day as runs of one part each, from midnight to midnight:
    /// `(from, to, part)`, `None` where nothing runs.
    pub fn segments(&self) -> Vec<(u32, u32, Option<Part>)> {
        let mut cuts = vec![0, self.start, self.am.0, self.am.1, self.pm.0, self.pm.1, self.end, DAY];
        cuts.sort_unstable();
        cuts.dedup();
        let mut out: Vec<(u32, u32, Option<Part>)> = Vec::new();
        for w in cuts.windows(2) {
            let p = self.part_at(w[0]);
            match out.last_mut() {
                Some(last) if last.2 == p && last.1 == w[0] => last.1 = w[1],
                _ => out.push((w[0], w[1], p)),
            }
        }
        out
    }

    /// Minutes a day each part runs.
    pub fn part_minutes(&self) -> [u32; 5] {
        let mut out = [0; 5];
        for (a, b, p) in self.segments() {
            if let Some(p) = p {
                out[p.index()] += b - a;
            }
        }
        out
    }

    /// The parts that run at all today, in order.
    pub fn parts(&self) -> Vec<Part> {
        let m = self.part_minutes();
        Part::ALL.into_iter().filter(|p| m[p.index()] > 0).collect()
    }

    /// Departures a day (one way) at the parts' headways, or for a bus line
    /// its bus-minutes a day.
    pub fn daily_work(&self, kind: Kind) -> f32 {
        let m = self.part_minutes();
        Part::ALL
            .into_iter()
            .map(|p| {
                let (mins, v) = (m[p.index()] as f32, self.value(p) as f32);
                if kind.per_bus() {
                    mins * v
                } else {
                    mins / v.max(1.0)
                }
            })
            .sum()
    }

    /// The most buses (or the shortest headway) any part runs.
    pub fn peak(&self, kind: Kind) -> u32 {
        let parts = self.parts();
        let vals = parts.iter().map(|p| self.value(*p));
        if kind.per_bus() {
            vals.max().unwrap_or(0)
        } else {
            vals.min().unwrap_or(kind.standard())
        }
    }

    /// The next tick at or after `t` (a sim tick, one a minute) when the
    /// line runs.
    pub fn next_running(&self, t: u64, kind: Kind) -> u64 {
        for k in 0..=(DAY as u64 / SNAP as u64 + 1) {
            let at = if k == 0 { t } else { (t / SNAP as u64 + k) * SNAP as u64 };
            if self.runs_at((at % DAY as u64) as u32, kind) {
                return at;
            }
        }
        t
    }

    /// The hours in words: "05:00–24:00", "24 hours", "05:00–24:00 and a
    /// night service".
    pub fn hours_text(&self) -> String {
        if self.night && self.part_minutes()[Part::Night.index()] > 0 && self.value(Part::Night) == self.value(Part::Day) {
            return "24 hours".into();
        }
        let span = format!("{}–{}", hm(self.start), hm(self.end));
        if self.night {
            format!("{span} and a night service")
        } else {
            span
        }
    }

    /// Is this a preset as it would be made for a line of `kind` with day
    /// value `base`?
    pub fn preset(&self, kind: Kind) -> Option<Preset> {
        let base = self.value(Part::Day);
        Preset::ALL.into_iter().find(|p| p.timetable(kind, base) == *self)
    }
}

/// "05:30"; 1440 is "24:00".
pub fn hm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// "05:30" or "5" or "5:30" as minutes of the day (24:00 is 1440).
pub fn parse_hm(s: &str) -> Option<u32> {
    let s = s.trim();
    let (h, m) = match s.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (s.parse::<u32>().ok()?, 0),
    };
    (h <= 24 && m < 60 && h * 60 + m <= DAY).then_some(h * 60 + m)
}

/// One hour of a line's day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    /// Riders who boarded; of them those whose wait was timed (a ferry's
    /// riders turn up at the pier untimed), and the minutes they waited.
    pub boarded: u32,
    pub timed: u32,
    pub wait_min: u32,
    /// Riders waiting and riders on board, summed over the samples.
    pub waiting: u32,
    pub riding: u32,
    pub samples: u32,
    /// Departures (a bus out for a quarter hour counts once per sample:
    /// for a bus line `runs` is buses out, summed over the samples).
    pub runs: u32,
    /// Riders still waiting when the service had stopped (gave up).
    pub missed: u32,
}

impl Slot {
    pub fn mean_wait(&self) -> Option<f32> {
        (self.timed > 0).then(|| self.wait_min as f32 / self.timed as f32)
    }

    pub fn mean_waiting(&self) -> f32 {
        self.waiting as f32 / self.samples.max(1) as f32
    }

    pub fn mean_riding(&self) -> f32 {
        self.riding as f32 / self.samples.max(1) as f32
    }
}

/// A line's day hour by hour, today and yesterday (24 slots each once
/// anything is recorded; empty in an older save).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timeline {
    pub today: Vec<Slot>,
    pub yesterday: Vec<Slot>,
}

impl Timeline {
    pub fn slot_mut(&mut self, hour: u32) -> &mut Slot {
        if self.today.len() != 24 {
            self.today.resize(24, Slot::default());
        }
        &mut self.today[(hour % 24) as usize]
    }

    /// `n` riders boarded in `hour`, having waited `wait_min` minutes all
    /// told.
    pub fn board(&mut self, hour: u32, n: u32, wait_min: u32) {
        if n == 0 {
            return;
        }
        let s = self.slot_mut(hour);
        s.boarded += n;
        s.timed += n;
        s.wait_min += wait_min;
    }

    /// `n` riders boarded in `hour`, their wait not known.
    pub fn board_untimed(&mut self, hour: u32, n: u32) {
        if n > 0 {
            self.slot_mut(hour).boarded += n;
        }
    }

    pub fn sample(&mut self, hour: u32, waiting: u32, riding: u32) {
        let s = self.slot_mut(hour);
        s.waiting += waiting;
        s.riding += riding;
        s.samples += 1;
    }

    pub fn run(&mut self, hour: u32, n: u32) {
        self.slot_mut(hour).runs += n;
    }

    pub fn miss(&mut self, hour: u32) {
        self.slot_mut(hour).missed += 1;
    }

    /// Midnight: today becomes yesterday.
    pub fn roll(&mut self) {
        self.yesterday = std::mem::take(&mut self.today);
    }

    pub fn day(&self, yesterday: bool) -> &[Slot] {
        if yesterday {
            &self.yesterday
        } else {
            &self.today
        }
    }

    /// Totals over a day: boarded, mean wait, missed, runs.
    pub fn totals(&self, yesterday: bool) -> (u32, Option<f32>, u32, u32) {
        let d = self.day(yesterday);
        let (b, t, w) = d.iter().fold((0, 0, 0), |(b, t, w), s| (b + s.boarded, t + s.timed, w + s.wait_min));
        (b, (t > 0).then(|| w as f32 / t as f32), d.iter().map(|s| s.missed).sum(), d.iter().map(|s| s.runs).sum())
    }

    /// Riders boarded and their mean wait in each part of the day under
    /// `t`, the part of each hour taken at its half past.
    pub fn by_part(&self, t: &Timetable, yesterday: bool) -> [(u32, Option<f32>); 5] {
        let mut sums = [(0u32, 0u32, 0u32); 5];
        for (h, s) in self.day(yesterday).iter().enumerate() {
            let p = t.part_at(h as u32 * 60 + 30).unwrap_or(Part::Night);
            let e = &mut sums[p.index()];
            e.0 += s.boarded;
            e.1 += s.timed;
            e.2 += s.wait_min;
        }
        sums.map(|(b, t, w)| (b, (t > 0).then(|| w as f32 / t as f32)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_follow_the_clock_and_the_night_is_optional() {
        // [r4-timetable] The metro's hours, rushes at 06–09 and 16–19.
        let t = Timetable::flat(Kind::Metro, 10);
        assert_eq!((t.start, t.end), (300, 1440), "old lines keep 05:00 to midnight");
        assert_eq!(t.part_at(4 * 60 + 59), None);
        assert_eq!(t.part_at(5 * 60), Some(Part::Day), "before the rush is the day's service");
        assert_eq!(t.part_at(6 * 60), Some(Part::MorningRush));
        assert_eq!(t.part_at(9 * 60), Some(Part::Day));
        assert_eq!(t.part_at(16 * 60 + 30), Some(Part::EveningRush));
        assert_eq!(t.part_at(19 * 60), Some(Part::Evening));
        assert_eq!(t.part_at(23 * 60 + 59), Some(Part::Evening));
        assert!(t.service_over(35, 30) && !t.service_over(20, 30), "the last run's half hour, then over");
        assert!(!t.service_over(4 * 60 + 40, 30), "the first train is due: wait for it");
        let mut n = t;
        n.night = true;
        assert_eq!(n.part_at(2 * 60), Some(Part::Night));
        assert!(!n.service_over(3 * 60, 30), "a night service never stops");
        // The segments cover the day once, in order.
        let segs = t.segments();
        assert_eq!(segs.first().map(|s| s.0), Some(0));
        assert_eq!(segs.last().map(|s| s.1), Some(DAY));
        assert!(segs.windows(2).all(|w| w[0].1 == w[1].0));
        assert_eq!(t.part_minutes(), [180, 480, 180, 300, 0]);
        assert_eq!(t.part_minutes().iter().sum::<u32>(), 1140);
    }

    #[test]
    fn presets_scale_the_line_and_old_hours_stay() {
        let c = Preset::Commuter.timetable(Kind::Metro, 30);
        assert_eq!((c.value(Part::MorningRush), c.value(Part::Day), c.value(Part::Evening)), (15, 30, 45));
        assert!(!c.night);
        let h = Preset::Hours24.timetable(Kind::Metro, 10);
        assert!(h.night && h.runs_at(3 * 60, Kind::Metro) && h.hours_text() == "24 hours");
        let o = Preset::NightOwl.timetable(Kind::Bus, 4);
        assert_eq!((o.value(Part::Night), o.value(Part::Evening)), (2, 4));
        let b = Preset::Commuter.timetable(Kind::Bus, 1);
        assert_eq!((b.value(Part::MorningRush), b.value(Part::Evening)), (2, 1), "one bus: a second in the rush");
        assert_eq!(Preset::AllDay.timetable(Kind::Bus, 3), Timetable::flat(Kind::Bus, 3));
        assert_eq!(c.preset(Kind::Metro), Some(Preset::Commuter));
        assert_eq!(Preset::from_key("24 hours"), Some(Preset::Hours24));
        assert_eq!(Preset::from_key("Night owl"), Some(Preset::NightOwl));
        // Each kind's standard hours.
        assert_eq!(Kind::Bus.hours(), (330, 1410));
        assert_eq!(Kind::Rail.hours(), (300, 1380));
        assert_eq!(Kind::Ferry.hours(), (360, 1320));
    }

    #[test]
    fn clamping_keeps_the_edges_in_order() {
        let t = Timetable { start: 600, end: 500, am: (100, 2000), pm: (0, 0), night: false, values: [0, 999, 3, 4, 5] }.clamped(Kind::Metro);
        assert!(t.start < t.end && t.start <= t.am.0 && t.am.0 <= t.am.1 && t.am.1 <= t.pm.0 && t.pm.0 <= t.pm.1 && t.pm.1 <= t.end);
        assert!(t.values.iter().all(|v| (4..=60).contains(v)));
        assert_eq!(parse_hm("5:30"), Some(330));
        assert_eq!(parse_hm("24:00"), Some(1440));
        assert_eq!(parse_hm("25"), None);
    }

    #[test]
    fn the_timeline_counts_by_hour_and_rolls_at_midnight() {
        let mut l = Timeline::default();
        l.board(7, 3, 30);
        l.sample(7, 6, 10);
        l.sample(7, 2, 20);
        l.miss(0);
        assert_eq!(l.today.len(), 24);
        assert_eq!(l.today[7].mean_wait(), Some(10.0));
        assert_eq!(l.today[7].mean_waiting(), 4.0);
        let t = Timetable::flat(Kind::Bus, 2);
        assert_eq!(l.by_part(&t, false)[Part::MorningRush.index()], (3, Some(10.0)));
        l.roll();
        assert!(l.today.is_empty() && l.yesterday[7].boarded == 3 && l.totals(true).2 == 1);
    }

    #[test]
    fn next_running_skips_the_night() {
        let t = Timetable::flat(Kind::Ferry, 90);
        assert_eq!(t.next_running(60, Kind::Ferry), 360);
        assert_eq!(t.next_running(12 * 60 + 7, Kind::Ferry), 12 * 60 + 7);
        assert_eq!(t.next_running(23 * 60, Kind::Ferry), DAY as u64 + 360);
    }
}
