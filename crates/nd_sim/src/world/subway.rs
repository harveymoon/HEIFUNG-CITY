//! [subway] Metro stations on lots, lines of tunnels between them, the
//! trains that run them and the people who ride (`crate::subway` has the
//! types and the pure geometry). Shared with buses and the railway:
//! `walk_to_transit` and `board_waiting` (`world/trains.rs`).

use super::*;
use crate::subway::{self as metro, MetroLine, MetroLineId, MetroPlan, MetroTrain, MetroTrainId};

/// At most this many metro trains run at once, city-wide.
const MAX_METRO_TRAINS: usize = 80;
/// [r4-timetable] Minutes from the entrance down to the platform, for
/// whether a line still runs when a rider gets there.
const METRO_STAIRS: u64 = 2;

impl World {
    /// Put a metro entrance on a lot (whatever grew there is bought out):
    /// a kiosk on a small plaza at the street, the platform below. It
    /// does nothing until a line calls at it.
    pub fn build_metro_station(&mut self, lot_id: LotId) -> Result<BuildingId, String> {
        let what = crate::services::Placeable::MetroStation;
        if let Some(problem) = self.placement_problem(what, lot_id) {
            return Err(problem);
        }
        let at = self.lots[lot_id].center;
        if let Some(near) = self.metro_stations().into_iter().filter_map(|s| self.metro_platform(s).map(|p| (s, p))).find(|(_, p)| p.distance(at) < metro::MIN_SPACING) {
            return Err(format!("too close to {} (stations need {:.0} m between them)", self.buildings[near.0].name, metro::MIN_SPACING));
        }
        self.take_over_lot(lot_id, "metro station")?;
        self.economy.debit(Flow::Construction, what.cost());
        let name = self.metro_station_name(at);
        let id = self.place_special(lot_id, name, metro::STATION_JOBS, 0, |b| b.metro = true);
        self.subway.version += 1;
        Ok(id)
    }

    /// A name for a new station: a place name picked by where it stands
    /// (not from the world's dice, so placing one perturbs nothing else),
    /// the first not already taken.
    fn metro_station_name(&self, at: Vec2) -> String {
        let n = metro::STATION_NAMES.len();
        let start = ((at.x.abs() * 7.0 + at.y.abs() * 13.0) as usize) % n;
        let taken = |name: &str| self.buildings.values().any(|b| b.metro && b.name == name);
        (0..n).map(|k| metro::STATION_NAMES[(start + k) % n]).find(|name| !taken(name)).map_or_else(|| format!("Metro {}", self.metro_stations().len() + 1), str::to_string)
    }

    /// Every metro station standing: the entrances and the Central Station.
    pub fn metro_stations(&self) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.metro).map(|(id, _)| id).collect()
    }

    /// Where a station's platform is: under the middle of its lot.
    pub fn metro_platform(&self, station: BuildingId) -> Option<Vec2> {
        let b = self.buildings.get(station).filter(|b| b.metro)?;
        self.lots.get(b.lot).map(|l| l.center)
    }

    /// A station's street entrance and how far people walk to it.
    pub(super) fn metro_entrance(&self, station: BuildingId) -> Option<(Vec2, f32)> {
        let b = self.buildings.get(station).filter(|b| b.metro && !b.is_derelict() && b.operating())?;
        let reach = if b.rail.is_some() { metro::CENTRAL_REACH } else { metro::REACH };
        self.building_entrance(station).map(|p| (p, reach))
    }

    /// The stations of a line as given (in order; the first again at the
    /// end closes a loop of three or more), checked: every one a metro
    /// station, none twice. Returns the distinct stations and whether the
    /// line is a loop.
    pub(super) fn metro_calls(&self, stations: &[BuildingId]) -> Result<(Vec<BuildingId>, bool), String> {
        let mut seen: Vec<BuildingId> = Vec::new();
        let mut looped = false;
        for (k, &s) in stations.iter().enumerate() {
            if self.metro_platform(s).is_none() {
                return Err("every stop must be a metro station (or the Central Station)".into());
            }
            if seen.last() == Some(&s) {
                continue;
            }
            // [metro-ux] Back to the first station as the last stop: a loop.
            if k + 1 == stations.len() && seen.first() == Some(&s) && seen.len() >= 3 {
                looped = true;
                continue;
            }
            if seen.contains(&s) {
                return Err(format!("{} is on the line twice", self.buildings[s].name));
            }
            seen.push(s);
        }
        if seen.len() < 2 {
            return Err("a line needs at least two stations".into());
        }
        Ok((seen, looped))
    }

    /// [metro-ux] What a line through `stations` would be (as
    /// `add_metro_line` takes them), without laying it: its tunnel, length,
    /// times, trains and cost, at the default headway.
    pub fn metro_line_preview(&self, stations: &[BuildingId]) -> Result<metro::LinePreview, String> {
        let (seen, looped) = self.metro_calls(stations)?;
        let pts: Vec<Vec2> = seen.iter().filter_map(|&s| self.metro_platform(s)).collect();
        Ok(metro::preview(&pts, looped, metro::DEFAULT_HEADWAY, self.config.traffic_seconds_per_tick))
    }

    /// [metro-ux] The line tool's preview of a draft, however short (one
    /// station: no tunnel yet).
    pub fn metro_draft_preview(&self, draft: &metro::MetroDraft) -> metro::LinePreview {
        let pts: Vec<Vec2> = draft.stations.iter().filter_map(|&s| self.metro_platform(s)).collect();
        metro::preview(&pts, draft.looped, metro::DEFAULT_HEADWAY, self.config.traffic_seconds_per_tick)
    }

    /// [metro-ux] Click a station into a line being drawn. Tunnels are
    /// bored station to station under anything (streets, buildings, water,
    /// road tunnels: the metro runs deeper), so any two stations connect;
    /// what can refuse a click is the order (the last stop again, a station
    /// already on the line, a loop of two) and the money: a stop whose
    /// tunnel the treasury could not pay for is refused with the sums.
    pub fn metro_draft_add(&self, draft: &mut metro::MetroDraft, station: BuildingId) -> metro::DraftStep {
        let Some(b) = self.buildings.get(station).filter(|_| self.metro_platform(station).is_some()) else {
            return metro::DraftStep::Refused("that is not a metro station".into());
        };
        if b.is_derelict() {
            return metro::DraftStep::Refused(format!("{} is derelict", b.name));
        }
        let mut next = draft.clone();
        let step = next.push(station, |s| self.buildings.get(s).map_or_else(|| "that station".to_string(), |b| b.name.clone()));
        if matches!(step, metro::DraftStep::Refused(_)) {
            return step;
        }
        let p = self.metro_draft_preview(&next);
        if p.tunnel_cost > self.economy.treasury {
            return metro::DraftStep::Refused(format!(
                "the treasury cannot pay the tunnel to {}: {:.0} m of line costs {:.0}, the treasury has {:.0}",
                b.name, p.length_m, p.tunnel_cost, self.economy.treasury
            ));
        }
        *draft = next;
        step
    }

    /// [metro-ux] The metro station a click at `p` means: the one whose lot
    /// it is on, else the nearest whose platform or entrance is within
    /// `radius` (stations stand 160 m apart, so this is never ambiguous
    /// below 80 m).
    pub fn metro_station_at(&self, p: Vec2, radius: f32) -> Option<BuildingId> {
        let stations = self.metro_stations();
        if let Some(s) = stations.iter().copied().find(|&s| self.buildings.get(s).and_then(|b| self.lots.get(b.lot)).is_some_and(|l| crate::zoning::point_in_polygon(p, &l.polygon))) {
            return Some(s);
        }
        stations
            .into_iter()
            .filter_map(|s| {
                let d = [self.metro_platform(s), self.building_entrance(s)].into_iter().flatten().map(|q| q.distance(p)).fold(f32::INFINITY, f32::min);
                (d <= radius).then_some((s, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(s, _)| s)
    }

    /// Lay a new line through `stations` in order and pay for its tunnels.
    /// [metro-ux] The first station again at the end (three stations or
    /// more) makes a loop.
    pub fn add_metro_line(&mut self, stations: &[BuildingId]) -> Result<MetroLineId, String> {
        let (seen, looped) = self.metro_calls(stations)?;
        let pts: Vec<Vec2> = seen.iter().filter_map(|&s| self.metro_platform(s)).collect();
        let (path, stops) = metro::lay_line(&pts, looped);
        let length = crate::geom::path_length(&path);
        let cost = length * metro::TUNNEL_COST_PER_M;
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; {:.0} m of tunnel costs {:.0}", self.economy.treasury, length, cost));
        }
        self.economy.debit(Flow::Construction, cost);
        self.subway.next_line += 1;
        let n = self.subway.next_line;
        let line = MetroLine { name: format!("Line {n}"), color: ((n - 1) % metro::LINE_COLOURS as u32) as u8, stations: seen, path, stops, headway: metro::DEFAULT_HEADWAY, last_dispatch: 0, riders_today: 0, riders_yesterday: 0, runs_today: 0, looped, timetable: self.transit.default_preset.map(|p| p.timetable(crate::timetable::Kind::Metro, metro::DEFAULT_HEADWAY)), timeline: Default::default() };
        let id = self.subway.lines.insert(line);
        self.subway.version += 1;
        Ok(id)
    }

    /// Close a line: its trains are taken off (riders set down at the next
    /// platform's entrance) and the people waiting for it walk instead.
    pub fn remove_metro_line(&mut self, id: MetroLineId) -> bool {
        if self.subway.lines.remove(id).is_none() {
            return false;
        }
        let trains: Vec<MetroTrainId> = self.subway.trains.iter().filter(|(_, t)| t.line == id).map(|(tid, _)| tid).collect();
        for t in trains {
            self.end_metro_run(t);
        }
        self.strand_metro_waiting(|leg_line, _, _| leg_line == id);
        self.subway.version += 1;
        true
    }

    /// Minutes between trains each way on a line.
    pub fn set_metro_headway(&mut self, id: MetroLineId, minutes: u32) -> Result<u32, String> {
        let l = self.subway.lines.get_mut(id).ok_or("no such line")?;
        l.headway = minutes.clamp(metro::MIN_HEADWAY, metro::MAX_HEADWAY);
        // [r4-timetable] With a timetable: the day's headway (the rushes,
        // the evening and the night keep theirs).
        if let Some(t) = l.timetable.as_mut() {
            t.values[crate::timetable::Part::Day.index()] = l.headway;
        }
        Ok(l.headway)
    }

    /// The best metro ride for a trip, if any line serves both ends.
    pub(super) fn plan_subway_trip(&self, from: Vec2, to: Vec2) -> Option<MetroPlan> {
        if self.subway.lines.is_empty() {
            return None;
        }
        self.subway.plan_trip(from, to, self.config.traffic_seconds_per_tick, self.config.walk_speed, |s| self.metro_entrance(s), |_, _, _, _| true)
    }

    /// [r4-timetable] The best metro ride for a trip setting off now: only
    /// lines whose trains still run (or have started) by the time the
    /// rider has walked to the platform. (Some 300 a night chose the
    /// metro at 23:xx and reached the platform after the last train.)
    pub(super) fn plan_subway_trip_now(&self, from: Vec2, to: Vec2) -> Option<MetroPlan> {
        if self.subway.lines.is_empty() {
            return None;
        }
        let now = self.clock.tick;
        let (walk, secs) = (self.config.walk_speed.max(0.1), self.config.traffic_seconds_per_tick);
        self.subway.plan_trip(from, to, secs, self.config.walk_speed, |s| self.metro_entrance(s), |l, i, j, d| l.still_calls(now, now + (d * 1.5 / walk).ceil() as u64 + METRO_STAIRS, i, j, secs))
    }

    /// Lines lose stations that were pulled down (the tunnel is re-laid
    /// through the rest); a line left with fewer than two closes.
    fn prune_subway(&mut self) {
        let gone: Vec<BuildingId> = self.subway.lines.values().flat_map(|l| l.stations.iter().copied()).filter(|s| self.metro_platform(*s).is_none()).collect();
        if gone.is_empty() {
            return;
        }
        let ids: Vec<MetroLineId> = self.subway.lines.keys().collect();
        for id in ids {
            let stations: Vec<BuildingId> = self.subway.lines[id].stations.iter().copied().filter(|s| !gone.contains(s)).collect();
            if stations.len() == self.subway.lines[id].stations.len() {
                continue;
            }
            if stations.len() < 2 {
                self.remove_metro_line(id);
                continue;
            }
            let pts: Vec<Vec2> = stations.iter().filter_map(|&s| self.metro_platform(s)).collect();
            // A loop left with two stations runs end to end.
            let looped = self.subway.lines[id].looped && pts.len() >= 3;
            let (path, stops) = metro::lay_line(&pts, looped);
            let l = &mut self.subway.lines[id];
            l.looped = looped;
            l.stations = stations;
            l.path = path;
            l.stops = stops;
            // The runs out there follow the old tunnel: they finish it.
        }
        self.strand_metro_waiting(|_, board, alight| gone.contains(&board) || gone.contains(&alight));
        self.subway.boardings.retain(|(b, _, _)| !gone.contains(b));
        self.subway.version += 1;
    }

    /// People waiting for a metro ride `pred` rules out give up and go on
    /// by other means.
    pub(super) fn strand_metro_waiting(&mut self, pred: impl Fn(MetroLineId, BuildingId, BuildingId) -> bool) {
        let tick = self.clock.tick;
        let stranded: Vec<CitizenId> = self
            .citizens
            .iter()
            .filter(|(_, c)| !matches!(c.activity, Activity::OnMetro { .. }))
            .filter(|(_, c)| matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Subway { line, board, alight }) if pred(line, board, alight)))
            .map(|(id, _)| id)
            .collect();
        for id in stranded {
            // On the platform or still walking to the entrance: either way
            // they set off afresh for where they were going, by other means
            // (someone left walking to a closed entrance would stand there
            // waiting for nothing).
            // [pt-transit] Up from the platform to the street first.
            let up = match self.citizens[id].transit.map(|t| t.leg) {
                Some(TransitLeg::Subway { board, .. }) if self.citizens[id].on_platform() => self.building_entrance(board),
                _ => None,
            };
            let c = &mut self.citizens[id];
            if let Some(p) = up {
                c.pos = p;
                c.prev_pos = p;
            }
            let trip = c.transit.take();
            c.activity = Activity::Idle;
            c.thought = "The metro's closed. Walking it.".into();
            c.next_decision_tick = tick + 1;
            if let Some(t) = trip.filter(|t| self.buildings.contains_key(t.target)) {
                self.travel_to_impl(id, Some(t.target), t.purpose, false, false);
            }
        }
    }

    /// Dispatch on each line's headway, run every train, call at stations.
    pub(super) fn step_subway(&mut self) {
        self.prune_subway();
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day() as u32;
        let seconds = self.config.traffic_seconds_per_tick;
        // [r4-timetable] Each line on its own timetable: its hours, and the
        // headway of the part of the day it is in.
        {
            let due: Vec<MetroLineId> = self.subway.lines.iter().filter(|(_, l)| l.stations.len() >= 2 && l.headway_at(minute).is_some_and(|h| l.last_dispatch == 0 || l.last_dispatch > tick || tick - l.last_dispatch >= h as u64)).map(|(id, _)| id).collect();
            for id in due {
                if self.subway.trains.len() + 2 > MAX_METRO_TRAINS {
                    break;
                }
                let l = &mut self.subway.lines[id];
                l.last_dispatch = tick.max(1);
                l.runs_today += 2;
                l.timeline.run(minute / 60, 2);
                let color = l.color;
                for forward in [true, false] {
                    let run = self.subway.lines[id].run(forward, color);
                    self.subway.trains.insert(MetroTrain { line: id, forward, run });
                }
                self.subway.stats.runs_today += 2;
            }
        }
        let ids: Vec<MetroTrainId> = self.subway.trains.keys().collect();
        for id in ids {
            let arrived = {
                let t = &mut self.subway.trains[id].run;
                t.prev_s = t.s;
                if t.dwell > 0.0 {
                    t.dwell -= seconds;
                    if t.dwell <= 0.0 {
                        t.next_stop += 1;
                    }
                    None
                } else if t.next_stop >= t.stops.len() {
                    Some(false)
                } else {
                    let target = t.stops[t.next_stop].1;
                    t.s = (t.s + t.speed * seconds).min(target);
                    (t.s >= target - 0.01).then_some(true)
                }
            };
            match arrived {
                Some(true) => self.metro_train_arrived(id),
                Some(false) => self.end_metro_run(id),
                None => {}
            }
        }
        // Riders ride along, out of sight.
        for t in self.subway.trains.values() {
            let (pos, heading) = (t.head(), t.car_pose_at(0, t.run.s).1);
            for &p in &t.run.passengers {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.pos = pos;
                    c.prev_pos = pos;
                    c.heading = heading;
                }
            }
        }
        self.subway.stats.trains = self.subway.trains.len() as u32;
    }

    /// A run is over (or its line closed): anyone still aboard gets off at
    /// the last platform it reached and goes on from that entrance.
    fn end_metro_run(&mut self, id: MetroTrainId) {
        let Some(t) = self.subway.trains.remove(id) else { return };
        let last = t.run.stops.get(t.run.next_stop.saturating_sub(1).min(t.run.stops.len().saturating_sub(1))).map(|s| s.0);
        let pos = last.and_then(|b| self.building_entrance(b)).unwrap_or(t.head());
        for p in t.run.passengers {
            self.metro_rider_off(p, pos);
        }
    }

    /// Off the train at an entrance and on to wherever they were going.
    fn metro_rider_off(&mut self, p: CitizenId, at: Vec2) {
        let Some(trip) = self.citizens.get_mut(p).and_then(|c| {
            c.pos = at;
            c.prev_pos = at;
            c.activity = Activity::Idle;
            c.transit.take()
        }) else {
            self.strand_driver(p, at);
            return;
        };
        if self.buildings.contains_key(trip.target) {
            self.travel_to_impl(p, Some(trip.target), trip.purpose, true, false);
        } else {
            self.strand_driver(p, at);
        }
    }

    /// A train stands at a platform: riders for here get off, people
    /// waiting for this line in this direction get on and pay.
    fn metro_train_arrived(&mut self, id: MetroTrainId) {
        let (at, line, forward, last) = {
            let t = &mut self.subway.trains[id];
            t.run.dwell = metro::DWELL_SECONDS;
            let last = t.run.next_stop + 1 >= t.run.stops.len();
            (t.run.stops[t.run.next_stop].0, t.line, t.forward, last)
        };
        let exit = self.building_entrance(at).unwrap_or(self.subway.trains[id].head());
        let on_line = self.subway.lines.get(line).cloned();
        let passengers = std::mem::take(&mut self.subway.trains[id].run.passengers);
        let mut keep: Vec<CitizenId> = Vec::new();
        for p in passengers {
            let stays = !last
                && self.citizens.get(p).and_then(|c| c.transit).is_some_and(|t| match t.leg {
                    // Riding on, unless their stop has left the line.
                    TransitLeg::Subway { alight, .. } => alight != at && on_line.as_ref().is_some_and(|l| l.index_of(alight).is_some()),
                    _ => false,
                });
            if stays {
                keep.push(p);
            } else {
                self.metro_rider_off(p, exit);
            }
        }
        if let (false, Some(l)) = (last, on_line) {
            let name = self.buildings.get(at).map(|b| b.name.clone()).unwrap_or_default();
            let boarding = self.board_waiting(|t| matches!(&t.leg, TransitLeg::Subway { line: ll, board, alight } if *ll == line && *board == at && l.serves(forward, *board, *alight)), metro::CAPACITY.saturating_sub(keep.len()), metro::FARE, Flow::SubwayFares, Item::Named(name.to_string()), &format!("Took the {} from {name}.", l.name));
            let n = boarding.len() as u32;
            let tick = self.clock.tick;
            let waited: u64 = boarding.iter().map(|&p| self.citizens[p].transit.map_or(0, |t| if t.since > 0 { tick.saturating_sub(t.since) } else { 0 })).sum();
            if let Some(l) = self.subway.lines.get_mut(line) {
                l.timeline.board(self.clock.minute_of_day() as u32 / 60, n, waited as u32);
            }
            for p in boarding {
                self.citizens[p].activity = Activity::OnMetro { train: id };
                keep.push(p);
            }
            if n > 0 {
                self.subway.stats.riders_today += n;
                self.subway.stats.fares_today += n as f32 * metro::FARE;
                if let Some(l) = self.subway.lines.get_mut(line) {
                    l.riders_today += n;
                }
                self.subway.note_boarding(at, n);
            }
        }
        self.subway.trains[id].run.passengers = keep;
    }

    /// Midnight: stations, tunnels and the day's runs are paid for (before
    /// the books close), and the day's counts roll over.
    pub(super) fn daily_subway(&mut self) {
        let stations = self.buildings.values().filter(|b| b.metro && b.rail.is_none()).count() as f32;
        let km = self.subway.lines.values().map(|l| l.length()).sum::<f32>() / 1000.0;
        let upkeep = stations * metro::STATION_UPKEEP_PER_DAY + km * metro::TUNNEL_UPKEEP_PER_KM + self.subway.stats.runs_today as f32 * metro::RUN_COST;
        if upkeep > 0.0 {
            self.economy.debit(Flow::SubwayUpkeep, upkeep);
        }
        self.subway.stats.upkeep_today = upkeep;
        self.subway.roll_day();
    }

    /// Counts read every tick, tallied with the rest of the economy.
    pub(super) fn tally_subway(&mut self, waiting: u32, riding: u32) {
        let s = &mut self.subway.stats;
        s.waiting = waiting;
        s.riding = riding;
        s.lines = self.subway.lines.len() as u32;
        s.trains = self.subway.trains.len() as u32;
        s.tunnel_m = self.subway.lines.values().map(|l| l.length()).sum();
        s.stations = self.buildings.values().filter(|b| b.metro).count() as u32;
    }

    /// After a load: trains are not saved, so riders aboard are set down
    /// at the nearest entrance of their line and go on from there.
    pub(super) fn subway_after_load(&mut self) {
        let riders: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::OnMetro { .. })).map(|(id, _)| id).collect();
        for id in riders {
            let pos = self.citizens[id].pos;
            let near = self.metro_stations().into_iter().filter_map(|s| self.building_entrance(s)).min_by(|a, b| a.distance(pos).total_cmp(&b.distance(pos))).unwrap_or(pos);
            self.metro_rider_off(id, near);
        }
        self.subway.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;

    /// Someone standing at `pos` with no home or job, for riding tests.
    fn walker(w: &mut World, pos: Vec2) -> CitizenId {
        let n = w.citizens.len();
        w.citizens.insert(Citizen {
            name: format!("Rider {n}"),
            seed: 500 + n as u64,
            home: None,
            work: None,
            pos,
            heading: 0.0,
            prev_pos: pos,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            car: None,
            commute: Default::default(),
            look: None,
            to_car: None,
            drunk: Default::default(),
            owns_car: false,
            owns_bike: false,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::Idle,
            memories: Vec::new(),
            places: Default::default(),
            thought: String::new(),
            spotlight: false,
            next_decision_tick: u64::MAX / 2,
            arrived_tick: 0,
            inside: None,
            mood: 0.6,
            income: 120.0,
            savings: 400.0,
            rent_stress_days: 0,
            satiety: 0.9,
            groceries: 0.0,
            hungry_days: 0,
            trail: Vec::new(),
            trail_stops: Default::default(),
            pending_footfall: Vec::new(),
            household: None,
            age: 35,
            wakefulness: 0.9,
            asleep: false,
            packed_meal: 1.0,
        })
    }

    /// A long street with homes along it and lots to put stations on.
    fn town() -> World {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-1200.0, 0.0), Vec2::new(1200.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-1150.0, 4.0), Vec2::new(1150.0, 4.0), Vec2::new(1150.0, 60.0), Vec2::new(-1150.0, 60.0)], LandUse::Residential);
        w.add_zone(vec![Vec2::new(-1150.0, -4.0), Vec2::new(1150.0, -4.0), Vec2::new(1150.0, -60.0), Vec2::new(-1150.0, -60.0)], LandUse::Commercial);
        w.implement();
        w.economy.treasury = 1_000_000.0;
        w
    }

    fn lot_near(w: &World, p: Vec2) -> LotId {
        w.lots.iter().filter(|(_, l)| l.usable()).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).unwrap()
    }

    #[test]
    fn stations_take_lots_and_lines_join_them_through_tunnels() {
        let mut w = town();
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).expect("station A");
        assert!(w.buildings[a].metro && w.buildings[a].is_special());
        let near = lot_near(&w, Vec2::new(-850.0, 30.0));
        assert!(w.build_metro_station(near).is_err(), "too close to A");
        let b = w.build_metro_station(lot_near(&w, Vec2::new(0.0, -30.0))).expect("station B");
        let c = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).expect("station C");
        assert!(w.add_metro_line(&[a]).is_err(), "one station is no line");
        assert!(w.add_metro_line(&[a, b, a]).is_err(), "no station twice");
        let before = w.economy.treasury;
        let line = w.add_metro_line(&[a, b, c]).expect("line");
        let l = &w.subway.lines[line];
        assert!(l.length() > 1700.0 && l.stops.len() == 3);
        assert!((before - w.economy.treasury - l.length() * metro::TUNNEL_COST_PER_M).abs() < 1.0, "tunnels paid for");
        // Tunnels leave the streets, lots and lanes alone.
        let segs = w.roads.alive_segments().count();
        w.add_metro_line(&[c, a]).expect("a second line");
        assert_eq!(w.roads.alive_segments().count(), segs);
        // Bulldozing a station re-lays the line through the others.
        w.demolish(b);
        w.step_subway();
        assert_eq!(w.subway.lines[line].stations, vec![a, c]);
        w.demolish(c);
        w.step_subway();
        assert!(w.subway.lines.is_empty(), "lines with one station left close");
    }

    /// [pt-transit] F23: a rider for the metro goes in at the entrance and
    /// waits on the platform below, out of sight of the street, and comes
    /// back up to the street if they give up.
    #[test]
    fn metro_riders_wait_on_the_platform_out_of_sight() {
        let mut w = town();
        let mut mind = IdleMind;
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b]).unwrap();
        let at = w.building_entrance(a).unwrap();
        let platform = w.metro_platform(a).unwrap();
        assert!(platform.distance(at) > 3.0, "the platform is not the kerb");
        let rider = walker(&mut w, at);
        w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Subway { line, board: a, alight: b }, target: b, purpose: Purpose::Wander, since: 0, bike: false });
        w.citizens[rider].activity = Activity::Traveling { to: Some(b), purpose: Purpose::ToStop, path: vec![at], idx: 0 };
        w.clock = SimClock::starting_at(0, 3, 0);
        for _ in 0..5 {
            w.tick(&mut mind);
            if matches!(w.citizens[rider].activity, Activity::Waiting { .. }) {
                break;
            }
        }
        let c = &w.citizens[rider];
        assert!(matches!(c.activity, Activity::Waiting { .. }), "{}", c.activity.label());
        assert!(c.pos.distance(platform) < 1.0, "down on the platform, not at the kerb: {:?} vs {:?}", c.pos, platform);
        assert!(c.on_platform() && !c.is_visible(), "out of sight at street level");
        assert!(w.activity_line(rider).contains("metro platform"), "{}", w.activity_line(rider));
    }

    #[test]
    fn trains_run_both_ways_on_the_headway_and_carry_riders() {
        let mut w = town();
        let mut mind = IdleMind;
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b]).unwrap();
        w.set_metro_headway(line, 6).unwrap();
        assert_eq!(w.set_metro_headway(line, 1), Ok(metro::MIN_HEADWAY));
        w.set_metro_headway(line, 6).unwrap();
        // A rider waiting at A for B.
        let home = w.buildings.iter().find(|(_, x)| x.homes > 0).map(|(id, _)| id);
        let at = w.building_entrance(a).unwrap();
        let rider = walker(&mut w, at);
        let target = home.unwrap_or(b);
        w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Subway { line, board: a, alight: b }, target, purpose: Purpose::Wander, since: 1, bike: false });
        w.citizens[rider].activity = Activity::Waiting { at: w.building_entrance(a).unwrap() };
        w.clock = SimClock::starting_at(0, 8, 0);
        let fares = w.economy.ledger.today[Flow::SubwayFares.index()];
        let mut rode = false;
        let mut both_ways = false;
        for _ in 0..90 {
            w.tick(&mut mind);
            rode |= matches!(w.citizens[rider].activity, Activity::OnMetro { .. });
            both_ways |= w.subway.trains.values().any(|t| t.forward) && w.subway.trains.values().any(|t| !t.forward);
        }
        assert!(both_ways, "runs leave both termini");
        assert!(rode, "the rider boarded: {}", w.citizens[rider].activity.label());
        assert!(!matches!(w.citizens[rider].activity, Activity::OnMetro { .. } | Activity::Waiting { .. }), "and got off again: {}", w.citizens[rider].activity.label());
        assert!(w.citizens[rider].pos.distance(w.building_entrance(b).unwrap()) < 400.0, "near the far station");
        assert!(w.economy.ledger.today[Flow::SubwayFares.index()] >= fares + metro::FARE - 0.01, "fare paid");
        assert_eq!(w.subway.lines[line].riders_today, 1);
        assert_eq!(w.subway.boardings_at(a).0, 1);
        assert!(w.subway.stats.runs_today >= 2 * (90 / 6) as u32 - 2, "a pair of runs every headway: {}", w.subway.stats.runs_today);
        // No train after midnight; upkeep is booked at the day's end.
        w.clock = SimClock::starting_at(0, 23, 59);
        w.tick(&mut mind);
        assert!(w.economy.ledger.yesterday[Flow::SubwayUpkeep.index()] < 0.0, "upkeep in the ledger");
        let runs = w.subway.stats.runs_today;
        for _ in 0..60 {
            w.tick(&mut mind);
        }
        assert_eq!(w.subway.stats.runs_today, runs, "no service at night");
    }

    #[test]
    fn people_choose_the_subway_for_long_trips_between_stations() {
        let mut w = town();
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-1000.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(1000.0, 30.0))).unwrap();
        let from = w.building_entrance(a).unwrap() + Vec2::new(40.0, 0.0);
        let to = w.building_entrance(b).unwrap() - Vec2::new(40.0, 0.0);
        let c = walker(&mut w, from);
        w.citizens[c].owns_bike = false;
        w.clock = SimClock::starting_at(0, 8, 0);
        let q = super::trips::TripQuery { from, to, target: b, purpose: Purpose::Wander, allow_transit: true, allow_car: false, allow_bike: false };
        assert!(!w.mode_options(c, &q).iter().any(|o| o.mode == crate::citizens::TripMode::Subway), "no line yet");
        let line = w.add_metro_line(&[a, b]).unwrap();
        let opts = w.mode_options(c, &q);
        let sub = opts.iter().find(|o| o.mode == crate::citizens::TripMode::Subway).expect("the subway is an option");
        let walk = opts.iter().find(|o| o.mode == crate::citizens::TripMode::Walk).unwrap();
        assert!(sub.minutes < walk.minutes * 0.6, "riding beats a 2 km walk: {} vs {}", sub.minutes, walk.minutes);
        assert!(matches!(sub.plan, super::trips::ModePlan::Subway { line: l, board, alight, .. } if l == line && board == a && alight == b));
        // At night there is no service.
        w.clock = SimClock::starting_at(1, 2, 0);
        assert!(!w.mode_options(c, &q).iter().any(|o| o.mode == crate::citizens::TripMode::Subway));
    }

    #[test]
    fn the_central_station_is_a_station_a_metro_stop_and_a_bus_terminal() {
        let mut w = town();
        let c = w.place_rail_facility(RailFacility::Central, Vec2::new(0.0, TRUNK_Y + 40.0)).expect("central station");
        let b = &w.buildings[c];
        assert!(b.metro && b.depot && b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::Central));
        assert_eq!(b.name, "Central Station");
        assert!(w.place_rail_facility(RailFacility::Central, Vec2::new(900.0, TRUNK_Y + 40.0)).is_err(), "one per city");
        // Its street stub joined to the town: it opens.
        let stub = w.facilities.values().find(|f| f.building == Some(c)).unwrap().road_line();
        w.add_road(&[Vec2::new(0.0, 0.0), (stub[0] + stub[1]) * 0.5], 1, 1);
        let link = w.buildings[c].rail.clone().unwrap();
        assert!(link.works(), "connected both ways: {link:?}");
        assert!(w.stations.contains(&c), "trains call at it");
        let platform = link.platform[0].distance(link.platform[1]);
        assert!(platform > rail::CENTRAL_PLATFORM * 0.9, "the long platform runs along the trunk: {platform:.0} m");
        // A metro line can start under it.
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[c, a]).expect("a line from the Central Station");
        assert_eq!(w.subway.lines[line].stations[0], c);
        // People walk further to it than to a plain station.
        assert!(w.metro_entrance(c).unwrap().1 > w.metro_entrance(a).unwrap().1);
        // And it lifts the land round it.
        let at = w.lots[w.buildings[c].lot].center;
        let before = w.land_value.at(at + Vec2::new(0.0, 150.0));
        w.update_land_value();
        assert!(w.land_value.at(at + Vec2::new(0.0, 150.0)) > before + 0.1, "land value draw");
    }

    #[test]
    fn closing_a_line_sends_its_riders_on_their_way() {
        let mut w = town();
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b]).unwrap();
        let at = w.building_entrance(a).unwrap();
        let rider = walker(&mut w, at);
        w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Subway { line, board: a, alight: b }, target: b, purpose: Purpose::Wander, since: 1, bike: false });
        w.citizens[rider].activity = Activity::Waiting { at: w.building_entrance(a).unwrap() };
        assert!(w.remove_metro_line(line));
        assert!(w.citizens[rider].transit.is_none_or(|t| !matches!(t.leg, TransitLeg::Subway { .. })));
        assert!(!matches!(w.citizens[rider].activity, Activity::Waiting { .. }));
        assert!(!w.remove_metro_line(line), "already gone");
    }

    #[test]
    fn a_save_keeps_lines_and_puts_riders_back_on_the_street() {
        let mut w = town();
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b]).unwrap();
        w.set_metro_headway(line, 7).unwrap();
        let rider = walker(&mut w, Vec2::ZERO);
        w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Subway { line, board: a, alight: b }, target: b, purpose: Purpose::Wander, since: 1, bike: false });
        w.citizens[rider].activity = Activity::OnMetro { train: MetroTrainId::default() };
        let text = w.to_ron().unwrap();
        let back = World::from_ron(&text).unwrap();
        assert_eq!(back.subway.lines.len(), 1);
        let l = back.subway.lines.values().next().unwrap();
        assert_eq!((l.headway, l.stations.len()), (7, 2));
        assert!(back.buildings.values().filter(|b| b.metro).count() == 2);
        assert!(!matches!(back.citizens[rider].activity, Activity::OnMetro { .. }), "nobody is left on a train that was not saved");
    }

    // [metro-ux] The line tool: clicking stations into a draft.

    #[test]
    fn clicks_pick_stations_and_a_draft_checks_them_against_the_city() {
        let mut w = town();
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(0.0, -30.0))).unwrap();
        let c = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        // A click on the lot, or near the platform, picks the station;
        // empty ground does not.
        let pa = w.metro_platform(a).unwrap();
        assert_eq!(w.metro_station_at(pa, 60.0), Some(a));
        assert_eq!(w.metro_station_at(pa + Vec2::new(0.0, 50.0), 60.0), Some(a));
        assert_eq!(w.metro_station_at(Vec2::new(-450.0, 400.0), 60.0), None);
        // Something that is not a station is refused.
        let home = BuildingId::default();
        let mut d = metro::MetroDraft::default();
        assert!(matches!(w.metro_draft_add(&mut d, home), metro::DraftStep::Refused(why) if why.contains("not a metro station")));
        assert_eq!(w.metro_draft_add(&mut d, a), metro::DraftStep::Started);
        assert_eq!(w.metro_draft_add(&mut d, b), metro::DraftStep::Added);
        let two = w.metro_draft_preview(&d);
        assert!(two.length_m > 850.0 && two.trains >= 2, "{two:?}");
        assert_eq!(w.metro_draft_add(&mut d, c), metro::DraftStep::Added);
        assert_eq!(w.metro_draft_add(&mut d, a), metro::DraftStep::Looped);
        let p = w.metro_draft_preview(&d);
        assert!(p.looped && p.length_m > 3400.0, "round and back: {:.0}", p.length_m);
        assert_eq!(w.metro_line_preview(&d.calls()).unwrap(), p, "the preview is what will be laid");
        // A treasury that cannot pay the next tunnel refuses the click and
        // leaves the draft as it was.
        let mut poor = metro::MetroDraft::default();
        w.metro_draft_add(&mut poor, a);
        w.economy.treasury = 1000.0;
        let before = poor.clone();
        assert!(matches!(w.metro_draft_add(&mut poor, c), metro::DraftStep::Refused(why) if why.contains("treasury")));
        assert_eq!(poor, before);
        w.economy.treasury = 1_000_000.0;
        // The draft lays as a loop and the tunnel is paid.
        let t0 = w.economy.treasury;
        let line = w.add_metro_line(&d.calls()).expect("a loop line");
        let l = &w.subway.lines[line];
        assert!(l.looped && l.stations == vec![a, b, c]);
        assert!(l.path[0].distance(*l.path.last().unwrap()) < 0.5, "the tunnel closes");
        assert!((t0 - w.economy.treasury - l.length() * metro::TUNNEL_COST_PER_M).abs() < 1.0);
        // A loop of two is still refused through the API path.
        assert!(w.add_metro_line(&[a, b, a]).is_err());
    }

    /// [r4-metroloop] A mind that always wants to be off somewhere: a
    /// stroll, whenever it is asked.
    struct RestlessMind;

    impl crate::mind::MindProvider for RestlessMind {
        fn decide(&mut self, _ctx: &crate::mind::MindContext) -> Option<crate::mind::Decision> {
            Some(crate::mind::Decision { intent: crate::mind::Intent::Wander, thought: String::new(), memory: None })
        }

        fn name(&self) -> &'static str {
            "restless"
        }
    }

    /// A rider walking in at A's entrance for a ride to B, their next
    /// decision already due when they get there.
    fn rider_due_at_a(w: &mut World) -> (CitizenId, BuildingId, BuildingId, MetroLineId) {
        let a = w.build_metro_station(lot_near(w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b]).unwrap();
        w.set_metro_headway(line, 6).unwrap();
        let at = w.building_entrance(a).unwrap();
        let rider = walker(w, at);
        w.clock = SimClock::starting_at(0, 8, 0);
        let c = &mut w.citizens[rider];
        c.transit = Some(TransitTrip { leg: TransitLeg::Subway { line, board: a, alight: b }, target: b, purpose: Purpose::Wander, since: 0, bike: false });
        c.activity = Activity::Traveling { to: Some(b), purpose: Purpose::ToStop, path: vec![at], idx: 0 };
        c.next_decision_tick = 0;
        (rider, a, b, line)
    }

    /// [r4-metroloop] Harvey's day 46: Ezra Varga walked to Steam Alley,
    /// went down to the platform and a minute later set off again, on a
    /// 42-minute walk round a viaduct back to the same platform, five
    /// times. A rider whose decision falls due on the platform is not
    /// asked again: they wait (the wait's start stays put, so giving up
    /// can still come) and board the next train.
    #[test]
    fn a_waiting_metro_rider_whose_decision_is_due_waits_for_the_train() {
        let mut w = town();
        let mut mind = RestlessMind;
        let (rider, ..) = rider_due_at_a(&mut w);
        let mut since: Option<u64> = None;
        let mut boarded = false;
        for _ in 0..60 {
            w.tick(&mut mind);
            let c = &w.citizens[rider];
            match &c.activity {
                Activity::Waiting { .. } => {
                    assert!(c.on_platform(), "waiting down on the platform");
                    let s = c.transit.unwrap().since;
                    assert!(since.is_none_or(|x| x == s), "the wait began once: {since:?} then {s}");
                    since = Some(s);
                }
                Activity::OnMetro { .. } => {
                    boarded = true;
                    break;
                }
                Activity::Traveling { purpose: Purpose::ToStop, .. } if since.is_none() => {}
                other => panic!("left the platform without the train: {}", other.label()),
            }
        }
        assert!(since.is_some(), "they waited");
        assert!(boarded, "and boarded the next train: {}", w.citizens[rider].activity.label());
    }

    /// [r4-metroloop] A late answer from the mind for someone on the
    /// platform waits too (as for someone on the train), and any trip
    /// planned from the platform after all sets off from the street
    /// entrance, not from under the middle of the lot.
    #[test]
    fn a_trip_planned_again_from_the_platform_starts_at_the_entrance() {
        let mut w = town();
        let mut mind = IdleMind;
        let (rider, a, b, _) = rider_due_at_a(&mut w);
        w.citizens[rider].next_decision_tick = u64::MAX / 2;
        for _ in 0..5 {
            w.tick(&mut mind);
            if w.citizens[rider].on_platform() {
                break;
            }
        }
        let platform = w.metro_platform(a).unwrap();
        let entrance = w.building_entrance(a).unwrap();
        assert!(w.citizens[rider].on_platform() && w.citizens[rider].pos.distance(platform) < 1.0);
        w.apply_late_decision(rider, crate::mind::Decision { intent: crate::mind::Intent::Wander, thought: "Fancy a stroll.".into(), memory: None });
        assert!(w.citizens[rider].on_platform(), "still waiting: {}", w.citizens[rider].activity.label());
        assert!(w.deferred_intents.contains_key(&rider), "the stroll keeps for later");
        // Something else sends them off (a friend calling, say).
        w.travel_to_impl(rider, Some(b), Purpose::Wander, false, false);
        let c = &w.citizens[rider];
        let Activity::Traveling { path, .. } = &c.activity else { panic!("on their way: {}", c.activity.label()) };
        assert!(c.pos.distance(entrance) < 1.0, "up at the entrance: {:?} vs {:?}", c.pos, entrance);
        assert!(path[0].distance(entrance) < path[0].distance(platform) || path[0].distance(entrance) < 15.0, "the walk starts at the street: {:?}", path[0]);
    }

    /// [r4-metroloop] After the last train (and a grace for it to come),
    /// whoever is still on the platform comes up and goes another way:
    /// waiting riders no longer decide again, so nothing else would send
    /// them on until `WAIT_GIVE_UP`.
    #[test]
    fn riders_still_waiting_after_the_last_train_go_another_way() {
        let mut w = town();
        let mut mind = IdleMind;
        let (rider, a, ..) = rider_due_at_a(&mut w);
        w.clock = SimClock::starting_at(0, 0, 20);
        for _ in 0..5 {
            w.tick(&mut mind);
            if w.citizens[rider].on_platform() {
                break;
            }
        }
        assert!(w.citizens[rider].on_platform(), "down to wait at 00:2x");
        for _ in 0..30 {
            w.tick(&mut mind);
        }
        let c = &w.citizens[rider];
        assert!(!matches!(c.activity, Activity::Waiting { .. }) && c.transit.is_none(), "gave up: {}", c.activity.label());
        assert!(c.pos.distance(w.metro_platform(a).unwrap()) > 3.0, "and came up from the platform");
        assert!(c.thought.contains("last train"), "{}", c.thought);
        assert!(crate::transit::service_over(35, metro::SERVICE_START, metro::SERVICE_END, crate::transit::LAST_RUN_GRACE));
        assert!(!crate::transit::service_over(10, metro::SERVICE_START, metro::SERVICE_END, crate::transit::LAST_RUN_GRACE));
        assert!(!crate::transit::service_over(12 * 60, crate::transit::SERVICE_START, crate::transit::SERVICE_END, crate::transit::LAST_RUN_GRACE));
        assert!(crate::transit::service_over(2, crate::transit::SERVICE_START, crate::transit::SERVICE_END, crate::transit::LAST_RUN_GRACE), "the buses end at 23:30");
    }

    #[test]
    fn a_loop_line_runs_round_and_shrinks_to_a_plain_line() {
        let mut w = town();
        let mut mind = IdleMind;
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(0.0, -30.0))).unwrap();
        let c = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b, c, a]).unwrap();
        w.set_metro_headway(line, 6).unwrap();
        // A rider from C home to A takes the loop forward (C, then A).
        let at = w.building_entrance(c).unwrap();
        let rider = walker(&mut w, at);
        w.clock = SimClock::starting_at(0, 8, 0);
        w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Subway { line, board: c, alight: a }, target: a, purpose: Purpose::Wander, since: w.clock.tick, bike: false });
        w.citizens[rider].activity = Activity::Waiting { at };
        let mut rode = false;
        for _ in 0..240 {
            w.tick(&mut mind);
            rode |= matches!(w.citizens[rider].activity, Activity::OnMetro { .. });
            if rode && !matches!(w.citizens[rider].activity, Activity::OnMetro { .. }) {
                break;
            }
        }
        assert!(rode, "boarded the loop: {}", w.citizens[rider].activity.label());
        assert!(!matches!(w.citizens[rider].activity, Activity::OnMetro { .. }), "and got off");
        assert!(w.citizens[rider].pos.distance(w.building_entrance(a).unwrap()) < 400.0, "at A");
        // Pulling down a station leaves two: the loop runs end to end.
        w.demolish(b);
        w.step_subway();
        let l = &w.subway.lines[line];
        assert!(!l.looped && l.stations == vec![a, c]);
        assert!(l.path[0].distance(*l.path.last().unwrap()) > 1000.0);
        // Loops survive a save.
        let line2 = {
            let d = w.build_metro_station(lot_near(&w, Vec2::new(-400.0, -30.0))).unwrap();
            let e = w.build_metro_station(lot_near(&w, Vec2::new(400.0, -30.0))).unwrap();
            w.add_metro_line(&[a, d, e, a]).unwrap()
        };
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert!(back.subway.lines[line2].looped);
    }

    /// [r4-timetable] A line's trains follow its timetable: twice as often
    /// in the morning rush as at midday, and with a 24-hour timetable on
    /// through the night, each run on the line's timeline and paid for.
    #[test]
    fn metro_trains_run_to_the_lines_timetable() {
        use crate::timetable::{LineRef, Part, Preset};
        let mut w = town();
        let mut mind = IdleMind;
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b]).unwrap();
        w.set_metro_headway(line, 20).unwrap();
        let t = w.set_line_preset(LineRef::Metro(line), Preset::Commuter).unwrap();
        assert_eq!((t.value(Part::MorningRush), t.value(Part::Day), t.value(Part::Evening)), (10, 20, 30));
        w.clock = SimClock::starting_at(0, 5, 0);
        while w.clock.minute_of_day() < 13 * 60 {
            w.tick(&mut mind);
        }
        let tl = &w.subway.lines[line].timeline.today;
        assert_eq!(tl[7].runs, 2 * 6, "every 10 min in the rush, both ways");
        assert_eq!(tl[11].runs, 2 * 3, "every 20 min at midday");
        // Out of hours, nothing; 24 hours, a train at 02:00.
        w.clock = SimClock::starting_at(1, 1, 0);
        for _ in 0..120 {
            w.tick(&mut mind);
        }
        assert!(w.subway.trains.values().all(|t| t.line != line), "no trains in the night");
        w.set_line_preset(LineRef::Metro(line), Preset::Hours24).unwrap();
        let before = w.subway.lines[line].runs_today;
        for _ in 0..30 {
            w.tick(&mut mind);
        }
        assert!(w.subway.lines[line].runs_today > before, "the night train ran");
        assert!(w.subway.trains.values().any(|t| t.line == line));
        assert_eq!(w.subway.lines[line].headway, 20, "the line's day headway kept");
    }

    /// [r4-timetable] Mode choice will not send a rider to a metro that
    /// has stopped by the time they reach the platform (Harvey's day 46:
    /// some 300 a night chose it at 23:xx and found the last train gone),
    /// and a 24-hour line is offered all night. Riders who board are on the
    /// timeline with their wait.
    #[test]
    fn riders_do_not_choose_a_metro_that_will_have_stopped() {
        use crate::timetable::{LineRef, Preset};
        let mut w = town();
        let mut mind = IdleMind;
        let a = w.build_metro_station(lot_near(&w, Vec2::new(-900.0, 30.0))).unwrap();
        let b = w.build_metro_station(lot_near(&w, Vec2::new(900.0, 30.0))).unwrap();
        let line = w.add_metro_line(&[a, b]).unwrap();
        // A few minutes' walk from the entrance.
        let ea = w.building_entrance(a).unwrap();
        let from = ea + Vec2::new(-250.0, 0.0);
        let to = w.building_entrance(b).unwrap() + Vec2::new(60.0, 0.0);
        let walk_min = from.distance(ea) * 1.25 / w.config.walk_speed;
        assert!(walk_min > 4.0, "a walk to the entrance: {walk_min}");
        w.clock = SimClock::starting_at(0, 12, 0);
        assert!(w.plan_subway_trip_now(from, to).is_some(), "midday: the metro");
        w.clock = SimClock::starting_at(0, 23, 59);
        assert!(w.plan_subway_trip(from, to).is_some(), "the line serves the trip");
        assert!(w.plan_subway_trip_now(from, to).is_none(), "but it stops at midnight, before they reach it");
        w.set_line_preset(LineRef::Metro(line), Preset::Hours24).unwrap();
        assert!(w.plan_subway_trip_now(from, to).is_some(), "a 24-hour line still takes them");
        // A rider on the platform at 00:40 on the 24-hour line waits for
        // the night train and boards it; the wait is on the timeline.
        let rider = walker(&mut w, ea);
        w.clock = SimClock::starting_at(1, 0, 40);
        w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Subway { line, board: a, alight: b }, target: b, purpose: Purpose::Wander, since: w.clock.tick, bike: false });
        w.citizens[rider].activity = Activity::Waiting { at: ea };
        let mut rode = false;
        for _ in 0..40 {
            w.tick(&mut mind);
            rode |= matches!(w.citizens[rider].activity, Activity::OnMetro { .. });
        }
        assert!(rode, "boarded the night train: {} ({})", w.citizens[rider].activity.label(), w.citizens[rider].thought);
        let slot = w.subway.lines[line].timeline.today[0];
        assert_eq!((slot.boarded, slot.timed), (1, 1));
        assert!(slot.wait_min <= 12 && slot.samples > 0, "{slot:?}");
    }
}
