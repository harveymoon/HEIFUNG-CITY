//! [r4-timetable] Every line's timetable and its day's timeline, as one
//! surface for the Lines window and the remote API: which timetable a line
//! runs (`line_schedule`), setting one or a preset (`set_line_timetable`,
//! `set_line_preset`), the city's default for new lines, the quarter-hour
//! sample of riders waiting and on board (`sample_timelines`), whether a
//! rider's service has stopped for the night (`leg_service_over`) and the
//! midnight roll, with the cost of runs beyond the standard timetable for
//! the trains and the ferries (the metro pays per run already, the buses
//! per bus-minute).

use super::*;
use crate::subway::MetroLineId;
use crate::timetable::{Kind, LineRef, Preset, Timeline, Timetable};

/// [r4-timetable] What a passenger train run beyond the standard timetable
/// costs (crew, power, track access); the standard timetable's trains are
/// in the stations' upkeep.
pub const RAIL_RUN_COST: f32 = 60.0;
/// [r4-timetable] What a ferry sailing beyond the standard timetable costs
/// (crew and fuel); the standard sailings are in the terminal's upkeep.
pub const FERRY_RUN_COST: f32 = 40.0;

impl World {
    /// Every line there is to timetable, in the Lines window's order: bus
    /// lines, metro lines, the railway's passenger service (when there is
    /// a station), ferry terminals.
    pub fn timetabled_lines(&self) -> Vec<LineRef> {
        let mut out: Vec<LineRef> = self.transit.routes.keys().map(LineRef::Bus).collect();
        out.extend(self.subway.lines.keys().map(LineRef::Metro));
        if !self.stations.is_empty() || !self.halts.is_empty() {
            out.push(LineRef::Rail);
        }
        out.extend(self.ports.ports.values().filter(|p| p.kind.is_ferry()).filter_map(|p| p.building).map(LineRef::Ferry));
        out
    }

    /// A line's name for the window and the API.
    pub fn line_name(&self, line: LineRef) -> Option<String> {
        match line {
            LineRef::Bus(r) => self.transit.routes.get(r).map(|r| r.name.clone()),
            LineRef::Metro(l) => self.subway.lines.get(l).map(|l| l.name.clone()),
            LineRef::Rail => Some("Passenger trains".into()),
            LineRef::Ferry(b) => self.ports.port_of(b).and(self.buildings.get(b)).map(|b| b.name.clone()),
        }
    }

    /// The timetable a line runs, and whether it is its own (`false`: the
    /// standard hours and one frequency, as lines ran before timetables).
    pub fn line_schedule(&self, line: LineRef) -> Option<(Timetable, bool)> {
        match line {
            LineRef::Bus(r) => self.transit.routes.get(r).map(|r| (r.schedule(), r.timetable.is_some())),
            LineRef::Metro(l) => self.subway.lines.get(l).map(|l| (l.schedule(), l.timetable.is_some())),
            LineRef::Rail => Some((self.rail.schedule(), self.rail.timetable.is_some())),
            LineRef::Ferry(b) => self.ports.port_of(b).filter(|p| p.kind.is_ferry()).map(|p| (p.schedule(), p.timetable.is_some())),
        }
    }

    pub fn line_timeline(&self, line: LineRef) -> Option<&Timeline> {
        match line {
            LineRef::Bus(r) => self.transit.routes.get(r).map(|r| &r.timeline),
            LineRef::Metro(l) => self.subway.lines.get(l).map(|l| &l.timeline),
            LineRef::Rail => Some(&self.rail.timeline),
            LineRef::Ferry(b) => self.ports.port_of(b).filter(|p| p.kind.is_ferry()).map(|p| &p.timeline),
        }
    }

    /// Give a line a timetable (clamped to its kind's limits), or `None`
    /// back to the standard. A bus line's `buses` and a metro line's
    /// `headway` follow its day part, so the old controls and readers
    /// still say what runs most of the day.
    pub fn set_line_timetable(&mut self, line: LineRef, t: Option<Timetable>) -> Result<Timetable, String> {
        let t = t.map(|t| t.clamped(line.kind()));
        let day = t.map(|t| t.value(crate::timetable::Part::Day));
        match line {
            LineRef::Bus(r) => {
                let r = self.transit.routes.get_mut(r).ok_or("no such bus line")?;
                r.timetable = t;
                if let Some(d) = day {
                    r.buses = d as u8;
                }
                self.transit.version += 1;
            }
            LineRef::Metro(l) => {
                let l = self.subway.lines.get_mut(l).ok_or("no such metro line")?;
                l.timetable = t;
                if let Some(d) = day {
                    l.headway = d;
                }
                self.subway.version += 1;
            }
            LineRef::Rail => self.rail.timetable = t,
            LineRef::Ferry(b) => {
                let p = self.ports.ports.values_mut().find(|p| p.building == Some(b) && p.kind.is_ferry()).ok_or("no such ferry terminal")?;
                p.timetable = t;
            }
        }
        Ok(self.line_schedule(line).map(|x| x.0).unwrap_or(t.unwrap_or(Timetable::flat(line.kind(), line.kind().standard()))))
    }

    /// A preset for a line, scaled from what it runs in the day now.
    pub fn set_line_preset(&mut self, line: LineRef, preset: Preset) -> Result<Timetable, String> {
        let (now, _) = self.line_schedule(line).ok_or("no such line")?;
        let t = preset.timetable(line.kind(), now.value(crate::timetable::Part::Day));
        self.set_line_timetable(line, Some(t))
    }

    /// The city's default timetable for new lines; with `apply`, every
    /// line there is takes it too (scaled from its own day).
    pub fn set_default_preset(&mut self, preset: Option<Preset>, apply: bool) -> Result<(), String> {
        self.transit.default_preset = preset;
        if apply {
            for line in self.timetabled_lines() {
                match preset {
                    Some(p) => {
                        self.set_line_preset(line, p)?;
                    }
                    None => {
                        self.set_line_timetable(line, None)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// [r4-metroloop] Has the service a rider waits for stopped for the
    /// night (its last run had its grace)? By the line's own timetable.
    pub(super) fn leg_service_over(&self, leg: &TransitLeg, minute: u32) -> bool {
        let (t, kind) = match leg {
            TransitLeg::Bus { route, .. } => (self.transit.routes.get(*route).map(|r| r.schedule()), Kind::Bus),
            TransitLeg::Subway { line, .. } => (self.subway.lines.get(*line).map(|l| l.schedule()), Kind::Metro),
            TransitLeg::Rail { .. } => (Some(self.rail.schedule()), Kind::Rail),
            // [cable] By the line's own hours.
            TransitLeg::Cable { line, .. } => return self.cable.lines.get(*line).is_none_or(|l| l.closed_for(minute, crate::cable::CLOSING_GRACE)),
            _ => return false,
        };
        t.unwrap_or_else(|| Timetable::flat(kind, kind.standard())).service_over(minute, kind.grace())
    }

    /// The timeline of the line a rider's leg is on.
    pub(super) fn leg_timeline(&mut self, leg: &TransitLeg) -> Option<&mut Timeline> {
        match leg {
            TransitLeg::Bus { route, .. } => self.transit.routes.get_mut(*route).map(|r| &mut r.timeline),
            TransitLeg::Subway { line, .. } => self.subway.lines.get_mut(*line).map(|l| &mut l.timeline),
            TransitLeg::Rail { .. } => Some(&mut self.rail.timeline),
            // [ferry-islands] The terminal the rider boards at.
            TransitLeg::Ferry { board, .. } => self.ports.ports.values_mut().find(|p| p.building == Some(*board)).map(|p| &mut p.timeline),
            TransitLeg::Cable { line, .. } => self.cable.lines.get_mut(*line).map(|l| &mut l.timeline),
            _ => None,
        }
    }

    /// The quarter-hour sample: riders waiting for each line and on board
    /// its vehicles now; for a bus line, the buses out too.
    pub(super) fn sample_timelines(&mut self) {
        let hour = self.clock.minute_of_day() as u32 / 60;
        let mut bus_wait: HashMap<RouteId, u32> = HashMap::new();
        let mut metro_wait: HashMap<MetroLineId, u32> = HashMap::new();
        let mut rail_wait = 0;
        let mut pier_wait: HashMap<BuildingId, u32> = HashMap::new();
        let mut cable_wait: HashMap<crate::cable::CableLineId, u32> = HashMap::new();
        for c in self.citizens.values() {
            if !matches!(c.activity, Activity::Waiting { .. }) {
                continue;
            }
            match c.transit.map(|t| t.leg) {
                Some(TransitLeg::Bus { route, .. }) => *bus_wait.entry(route).or_default() += 1,
                Some(TransitLeg::Subway { line, .. }) => *metro_wait.entry(line).or_default() += 1,
                Some(TransitLeg::Rail { .. }) => rail_wait += 1,
                Some(TransitLeg::Ferry { board, .. }) => *pier_wait.entry(board).or_default() += 1,
                Some(TransitLeg::Cable { line, .. }) => *cable_wait.entry(line).or_default() += 1,
                _ => {}
            }
        }
        let mut bus_ride: HashMap<RouteId, (u32, u32)> = HashMap::new();
        for car in self.traffic.cars.values() {
            if let Some(b) = &car.bus {
                let e = bus_ride.entry(b.route).or_default();
                e.0 += b.passengers.len() as u32;
                if !b.to_depot {
                    e.1 += 1;
                }
            }
        }
        for (id, r) in self.transit.routes.iter_mut() {
            let (riding, out) = bus_ride.get(&id).copied().unwrap_or_default();
            r.timeline.sample(hour, bus_wait.get(&id).copied().unwrap_or(0), riding);
            r.timeline.run(hour, out);
        }
        let mut metro_ride: HashMap<MetroLineId, u32> = HashMap::new();
        for t in self.subway.trains.values() {
            *metro_ride.entry(t.line).or_default() += t.run.passengers.len() as u32;
        }
        for (id, l) in self.subway.lines.iter_mut() {
            l.timeline.sample(hour, metro_wait.get(&id).copied().unwrap_or(0), metro_ride.get(&id).copied().unwrap_or(0));
        }
        // [cable] Riders in the stations and in the cabins.
        for (id, l) in self.cable.lines.iter_mut() {
            let riding = l.aboard.len() as u32;
            l.timeline.sample(hour, cable_wait.get(&id).copied().unwrap_or(0), riding);
        }
        if !self.stations.is_empty() || !self.halts.is_empty() {
            let riding = self.rail.trains.values().filter(|t| t.kind == crate::rail::TrainKind::Passenger).map(|t| t.passengers.len() as u32).sum();
            self.rail.timeline.sample(hour, rail_wait, riding);
        }
        let mut ferry_ride: HashMap<BuildingId, u32> = HashMap::new();
        for s in self.ports.ships.values() {
            if s.kind.is_ferry() && !s.home {
                *ferry_ride.entry(s.port).or_default() += s.passengers;
            }
        }
        let boarding = &self.ports.boarding;
        for p in self.ports.ports.values_mut().filter(|p| p.kind.is_ferry()) {
            let Some(b) = p.building else { continue };
            p.timeline.sample(hour, boarding.get(&b).copied().unwrap_or(0) + pier_wait.get(&b).copied().unwrap_or(0), ferry_ride.get(&b).copied().unwrap_or(0));
        }
    }

    /// Midnight, before the books close: trains and ferries pay for the
    /// runs their timetable made beyond the standard one, then every
    /// timeline's today becomes yesterday.
    pub(super) fn daily_timetables(&mut self) {
        if self.rail.timetable.is_some() {
            let standard = Timetable::flat(Kind::Rail, Kind::Rail.standard()).daily_work(Kind::Rail);
            let made = self.rail.timeline.totals(false).3 as f32;
            let extra = (made - standard.round()).max(0.0) * RAIL_RUN_COST;
            if extra > 0.0 {
                self.economy.debit(Flow::RailUpkeep, extra);
            }
        }
        let standard = Timetable::flat(Kind::Ferry, Kind::Ferry.standard()).daily_work(Kind::Ferry).ceil();
        let charges: Vec<(BuildingId, f32)> = self.ports.ports.values().filter(|p| p.kind.is_ferry() && p.timetable.is_some()).filter_map(|p| p.building.map(|b| (b, (p.timeline.totals(false).3 as f32 - standard).max(0.0) * FERRY_RUN_COST))).filter(|(_, x)| *x > 0.0).collect();
        for (b, x) in charges {
            self.economy.debit_item(Flow::TransitUpkeep, x, Item::Building(b));
        }
        for r in self.transit.routes.values_mut() {
            r.timeline.roll();
        }
        for l in self.subway.lines.values_mut() {
            l.timeline.roll();
        }
        self.rail.timeline.roll();
        for p in self.ports.ports.values_mut() {
            if p.kind.is_ferry() {
                p.timeline.roll();
            }
        }
    }
}
