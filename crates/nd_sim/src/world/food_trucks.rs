//! [r14-foodtrucks] Food trucks at work (`crate::food_trucks`): the town's
//! crowds read, the day planned, the ingredients bought, the drive out, the
//! kerb, the queue at the hatch, the drive home, the books at midnight and
//! new trucks where crowds went unfed.

use super::*;
use crate::food_trucks::*;
use crate::vehicle::Errand;

/// Grid cells (m) the nightlife streets and office districts are read in.
const NIGHT_CELL: f32 = 300.0;
const OFFICE_CELL: f32 = 400.0;
/// Least venues for a nightlife street, least office jobs for a district.
const MIN_VENUES: usize = 2;
const MIN_OFFICE_JOBS: u32 = 40;
/// Crowd at a stadium game, by tier (people outside before and after).
const STADIUM_CROWD: [f32; 3] = [120.0, 320.0, 700.0];
/// A rave's crowd outside.
const RAVE_CROWD: f32 = 80.0;
/// Minutes a customer stays after being served (eating at the kerb).
const EAT_MINUTES: u64 = 8;
/// Driving speed assumed when timing the drive out (m/s).
const PLAN_SPEED: f32 = 6.0;

fn key(b: BuildingId) -> u64 {
    slotmap::Key::data(&b).as_ffi()
}

fn tkey(t: TruckId) -> u64 {
    slotmap::Key::data(&t).as_ffi()
}

impl World {
    // ── the crowds ─────────────────────────────────────────────────────────

    /// Where the town's crowds are today, from its own numbers: park
    /// visitors and beach-goers (yesterday's visitor-hours, today's so
    /// far), the stadiums playing today, gigs and tonight's rave, the
    /// streets of bars and clubs, the office districts.
    pub fn crowd_spots(&self) -> Vec<CrowdSpot> {
        let mut out: Vec<CrowdSpot> = Vec::new();
        let hours_so_far = (self.clock.hour() as f32 - 9.0).clamp(1.0, 10.0);
        // Parks.
        for (anchor, st) in &self.parks.parks {
            if st.preserve {
                continue;
            }
            let Some(lot) = self.building_lot(*anchor) else { continue };
            let crowd = (st.visits_yesterday as f32 / 9.0).max(st.visits_today as f32 / hours_so_far);
            let at = if st.lots.len() > 1 {
                let (sum, area) = st.lots.iter().filter_map(|l| self.lots.get(*l)).fold((Vec2::ZERO, 0.0), |(s, a), l| (s + l.center * l.area(), a + l.area()));
                if area > 0.0 { sum / area } else { lot.center }
            } else {
                lot.center
            };
            out.push(CrowdSpot { kind: SpotKind::Park, at, name: st.name.clone(), crowd, from: 10, to: 20, building: Some(*anchor) });
        }
        // Beaches.
        for (b, st) in &self.shore.beaches {
            if !st.access || !st.open {
                continue;
            }
            let Some(lot) = self.building_lot(*b) else { continue };
            let crowd = (st.visits_yesterday as f32 / 8.0).max(st.visitors as f32 * 0.6);
            out.push(CrowdSpot { kind: SpotKind::Beach, at: lot.center, name: st.name.clone(), crowd, from: 10, to: 19, building: Some(*b) });
        }
        // Stadiums with a game today.
        for &b in &self.services.events_today {
            let Some(bld) = self.buildings.get(b).filter(|x| x.operating() && !x.is_derelict()) else { continue };
            let Some(lot) = self.building_lot(b) else { continue };
            let tier = bld.service.map_or(Tier::Mid, |s| s.tier);
            out.push(CrowdSpot { kind: SpotKind::Stadium, at: lot.center, name: bld.name.clone(), crowd: STADIUM_CROWD[tier.index()], from: 16, to: 23, building: Some(b) });
        }
        // Gigs today, and tonight's rave.
        let day0 = self.clock.day() * crate::clock::MINUTES_PER_DAY;
        let hour_of = |t: u64| -> u8 { (t.saturating_sub(day0) / 60).min(30) as u8 };
        for g in &self.social.gigs {
            if g.until <= day0 || g.from >= day0 + crate::clock::MINUTES_PER_DAY {
                continue;
            }
            let Some(lot) = self.building_lot(g.venue) else { continue };
            let name = self.buildings.get(g.venue).map(|b| b.name.clone()).unwrap_or_default();
            let (a, z) = (hour_of(g.from).saturating_sub(1), hour_of(g.until) + 1);
            if z > a {
                out.push(CrowdSpot { kind: SpotKind::Festival, at: lot.center, name: format!("a gig at {name}"), crowd: g.crowd as f32, from: a, to: z, building: Some(g.venue) });
            }
        }
        let rave = self.street_events.rave.as_ref().map(|r| (r.building, r.started, r.ends_at)).or_else(|| self.street_events.rave_planned.map(|(b, t)| (b, t, t + 6 * 60)));
        if let Some((b, from, until)) = rave {
            if until > day0 {
                if let Some(lot) = self.building_lot(b) {
                    let (a, z) = (hour_of(from), hour_of(until).max(hour_of(from) + 2));
                    out.push(CrowdSpot { kind: SpotKind::Festival, at: lot.center, name: "a warehouse rave".into(), crowd: RAVE_CROWD, from: a, to: z, building: Some(b) });
                }
            }
        }
        // Nightlife streets and office districts, cell by cell.
        let mut night: BTreeMap<(i32, i32), (f32, usize, BuildingId, f32)> = BTreeMap::new();
        let mut office: BTreeMap<(i32, i32), (u32, BuildingId, u16)> = BTreeMap::new();
        for (id, b) in &self.buildings {
            if !b.operating() || b.is_derelict() || b.jobs == 0 {
                continue;
            }
            let Some(z) = b.business.as_ref().filter(|z| !z.closed) else { continue };
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let p = lot.center;
            let late = {
                let h = z.kind.hours();
                h.contains(23) || h.contains(0)
            };
            if b.land_use == LandUse::Entertainment && late {
                let w = b.jobs as f32 * z.kind.row().customers_per_job.unwrap_or(4) as f32;
                let c = ((p.x / NIGHT_CELL).floor() as i32, (p.y / NIGHT_CELL).floor() as i32);
                let e = night.entry(c).or_insert((0.0, 0, id, 0.0));
                e.0 += w;
                e.1 += 1;
                if w > e.3 || (w == e.3 && key(id) < key(e.2)) {
                    e.2 = id;
                    e.3 = w;
                }
            }
            if z.kind == BusinessKind::Office || b.land_use == LandUse::Civic {
                let c = ((p.x / OFFICE_CELL).floor() as i32, (p.y / OFFICE_CELL).floor() as i32);
                let e = office.entry(c).or_insert((0, id, 0));
                e.0 += b.jobs as u32;
                if b.jobs > e.2 || (b.jobs == e.2 && key(id) < key(e.1)) {
                    e.1 = id;
                    e.2 = b.jobs;
                }
            }
        }
        let street_name = |b: BuildingId| -> String {
            self.lots.get(self.buildings[b].lot).and_then(|l| self.streets.of(l.front_segment)).map(|s| s.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| self.buildings[b].name.clone())
        };
        for (_, (w, n, best, _)) in night {
            if n < MIN_VENUES {
                continue;
            }
            let at = self.building_lot(best).map(|l| l.center).unwrap_or_default();
            out.push(CrowdSpot { kind: SpotKind::Nightlife, at, name: street_name(best), crowd: w / 3.0, from: 20, to: 27, building: Some(best) });
        }
        for (_, (jobs, best, _)) in office {
            if jobs < MIN_OFFICE_JOBS {
                continue;
            }
            let at = self.building_lot(best).map(|l| l.center).unwrap_or_default();
            out.push(CrowdSpot { kind: SpotKind::Offices, at, name: format!("the offices on {}", street_name(best)), crowd: jobs as f32 / 5.0, from: 7, to: 15, building: Some(best) });
        }
        out.retain(|s| s.crowd >= MIN_CROWD);
        out
    }

    // ── the day's plan ─────────────────────────────────────────────────────

    /// Plan the trucks' day from `from` o'clock on: each truck at home
    /// keeps the stints it has begun or finished and picks the best for its
    /// menu among the crowds, spread so no spot gets them all. In truck
    /// order, so the same city plans the same way.
    pub fn plan_food_trucks(&mut self, from: u8) {
        let spots = self.crowd_spots();
        let day = self.clock.day();
        let wet = crate::yards::wet(&self.weather);
        // The climate's curve for the day, moved by how far today already
        // is from it (a warm spell, or a sky set by hand, carries on).
        let weather = self.weather.clone();
        let now = self.clock.minute_of_day() as f32 / 60.0;
        let offset = weather.temp_c - weather.temp_at(now);
        let temp_at = move |h: u8| weather.temp_at(h as f32) + offset;
        let mut taken: BTreeMap<usize, u32> = BTreeMap::new();
        let ids: Vec<TruckId> = self.food_trucks.trucks.keys().collect();
        for id in ids {
            let t = &self.food_trucks.trucks[id];
            if t.job == TruckJob::Leaving {
                continue;
            }
            let base = self.building_lot(t.base).map(|l| l.center).unwrap_or(t.last_pos);
            // Keep today's stints under way or done; drop the rest (and
            // any left from yesterday).
            let keep: Vec<Stint> = t.plan.iter().filter(|s| s.day == day && (s.from < from || !matches!(s.state, StintState::Planned))).cloned().collect();
            let mut busy: Vec<(u8, u8)> = keep.iter().map(|s| (s.from, s.to)).collect();
            busy.extend(self.crew_busy_hours(t.household));
            let room = MAX_STINTS.saturating_sub(keep.len());
            let picks = if room == 0 { Vec::new() } else { pick_stints(t.kind, base, from, &spots, &taken, &temp_at, wet, tkey(id) ^ day.wrapping_mul(0x9E37), &busy, room) };
            let mut plan = keep;
            for (i, a, b, _) in picks {
                *taken.entry(i).or_default() += 1;
                let s = &spots[i];
                plan.push(Stint { kind: s.kind, at: s.at, name: s.name.clone(), from: a, to: b, crowd: s.crowd, state: StintState::Planned, day });
            }
            plan.sort_by_key(|s| (s.day, s.from));
            self.food_trucks.trucks[id].plan = plan;
            self.hold_operator(id);
        }
        self.food_trucks.spots = spots;
    }

    /// The hours (0..28, past midnight as 24+) when every grown-up of the
    /// household is at a job's shift: nobody could take the truck out, so
    /// the plan leaves them free (an hour's slack on each side comes from
    /// `pick_stints`' gap rule). Runs of such hours, as (first, last).
    pub(crate) fn crew_busy_hours(&self, hh: HouseholdId) -> Vec<(u8, u8)> {
        let Some(h) = self.households.get(hh) else { return Vec::new() };
        let shifts: Vec<Option<crate::clock::Hours>> = h
            .members
            .iter()
            .filter_map(|m| self.citizens.get(*m))
            .filter(|c| c.age >= crate::citizens::ADULT_AGE && c.age < 75)
            .map(|c| c.work.and_then(|w| self.buildings.get(w)).map(|b| b.shift_for(c.seed)))
            .collect();
        if shifts.is_empty() || shifts.iter().any(|s| s.is_none()) {
            return Vec::new();
        }
        let mut out: Vec<(u8, u8)> = Vec::new();
        for hour in 0u8..28 {
            if shifts.iter().all(|s| s.is_some_and(|s| s.contains(hour % 24))) {
                match out.last_mut() {
                    Some(r) if r.1 + 1 == hour => r.1 = hour,
                    _ => out.push((hour, hour)),
                }
            }
        }
        out
    }

    /// When a truck at home must set off for stint `s`.
    fn departure_tick(&self, t: &Truck, s: &Stint) -> u64 {
        let base = self.building_lot(t.base).map(|l| l.center).unwrap_or(t.last_pos);
        let drive = (base.distance(s.at) / PLAN_SPEED / 60.0) as u64 + 15;
        Self::stint_ticks(s).0.saturating_sub(drive)
    }

    /// The one who runs the truck, at home, stays in (resting, or asleep)
    /// till it is time to take the truck out: they do not wander off
    /// across town an hour before their stint.
    fn hold_operator(&mut self, id: TruckId) {
        let Some(t) = self.food_trucks.trucks.get(id) else { return };
        let Some(op) = t.operator.or(t.crew) else { return };
        let Some((_, s)) = t.current_stint() else { return };
        if !matches!(s.state, StintState::Planned) {
            return;
        }
        let dep = self.departure_tick(t, s);
        let home = t.base;
        if let Some(c) = self.citizens.get_mut(op) {
            if matches!(c.activity, Activity::AtHome) && c.inside == Some(home) {
                c.next_decision_tick = c.next_decision_tick.max(dep);
            }
        }
    }

    // ── the tick ───────────────────────────────────────────────────────────

    pub(super) fn step_food_trucks(&mut self) {
        if self.clock.minute_of_day() == 0 {
            self.daily_food_trucks();
        }
        if self.food_trucks.trucks.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day();
        if minute == PLAN_HOUR as u64 * 60 + 2 {
            self.plan_food_trucks(PLAN_HOUR + 1);
        } else if minute == REPLAN_HOUR as u64 * 60 + 2 {
            self.plan_food_trucks(REPLAN_HOUR + 2);
        }
        self.track_food_trucks();
        if tick % 5 == 1 {
            self.run_food_trucks();
            self.tidy_truck_queues();
        }
        if minute % 60 == 30 {
            let h = self.clock.hour() as usize;
            let open = self.food_trucks.trucks.values().filter(|t| t.place.open()).count() as u16;
            let s = &mut self.food_trucks.stats;
            if s.open_by_hour.len() < 24 {
                s.open_by_hour.resize(24, 0);
            }
            s.open_by_hour[h] = open;
        }
    }

    /// Trucks on the road: the crew rides along; a truck whose car went
    /// (road works rebuilt the lanes) is put back at home.
    fn track_food_trucks(&mut self) {
        let ids: Vec<TruckId> = self.food_trucks.trucks.keys().collect();
        for id in ids {
            let t = &self.food_trucks.trucks[id];
            let TruckPlace::Road(car) = t.place else { continue };
            let crew = t.crew;
            let alive = self.traffic.cars.get(car).is_some_and(|c| c.errand == Errand::FoodTruck(id));
            if !alive {
                if t.job == TruckJob::Leaving {
                    self.food_trucks.trucks.remove(id);
                    continue;
                }
                self.truck_home_now(id);
                continue;
            }
            let (pos, heading) = {
                let c = &self.traffic.cars[car];
                (c.pos, c.heading)
            };
            self.food_trucks.trucks[id].last_pos = pos;
            if let Some(c) = crew.and_then(|c| self.citizens.get_mut(c)).filter(|c| matches!(c.activity, Activity::Riding { bus } if bus == car)) {
                c.pos = pos;
                c.prev_pos = pos;
                c.heading = heading;
            }
        }
    }

    /// The stint's start and end, as ticks.
    fn stint_ticks(s: &Stint) -> (u64, u64) {
        let d = s.day * crate::clock::MINUTES_PER_DAY;
        (d + s.from as u64 * 60, d + s.to as u64 * 60)
    }

    /// Every few minutes: trucks at home set off for their next stint in
    /// time; open trucks close at the end of the stint (or sold out, or
    /// with nobody left serving) and move on or go home; parked trucks
    /// open when the stint starts.
    fn run_food_trucks(&mut self) {
        let tick = self.clock.tick;
        let ids: Vec<TruckId> = self.food_trucks.trucks.keys().collect();
        for id in ids {
            let Some(t) = self.food_trucks.trucks.get(id) else { continue };
            match (t.place, t.job) {
                (TruckPlace::Base, TruckJob::Idle) => {
                    let Some((i, s)) = t.current_stint() else { continue };
                    let (_, end) = Self::stint_ticks(s);
                    if tick < self.departure_tick(t, s) {
                        continue;
                    }
                    if tick + 60 > end {
                        self.skip_stint(id, i, "too late to be worth it");
                        continue;
                    }
                    self.set_off(id, i, None);
                }
                (TruckPlace::Pitch { pitch, open }, _) => {
                    let Some((i, s)) = t.current_stint() else {
                        self.truck_go_home(id, Some(pitch));
                        continue;
                    };
                    let (start, end) = Self::stint_ticks(s);
                    let stock = t.stock;
                    let crew_ok = t.crew.is_some_and(|c| self.citizens.get(c).is_some_and(|x| matches!(x.activity, Activity::Idle) && x.pos.distance(pitch.vendor_spot(t.kind.cart())) < 3.0));
                    if !open {
                        if tick + 10 >= start {
                            self.open_truck(id, i);
                        }
                        continue;
                    }
                    let why = if tick >= end {
                        None
                    } else if stock < 1.0 {
                        Some("sold out")
                    } else if !crew_ok {
                        Some("nobody left serving")
                    } else {
                        continue;
                    };
                    self.close_truck(id, i, why);
                    // On to the next stint if it starts soon, else home.
                    let next = self.food_trucks.trucks[id].current_stint().map(|(j, s)| (j, Self::stint_ticks(s).0));
                    match next {
                        Some((j, st)) if st < tick + 120 && self.food_trucks.trucks[id].stock >= 5.0 => {
                            self.set_off(id, j, Some(pitch));
                            // Could not move on (no pitch, nobody): home.
                            if matches!(self.food_trucks.trucks[id].place, TruckPlace::Pitch { .. }) {
                                self.truck_go_home(id, Some(pitch));
                            }
                        }
                        _ => self.truck_go_home(id, Some(pitch)),
                    }
                }
                _ => {}
            }
        }
    }

    fn skip_stint(&mut self, id: TruckId, i: usize, why: &str) {
        if let Some(s) = self.food_trucks.trucks.get_mut(id).and_then(|t| t.plan.get_mut(i)) {
            s.state = StintState::Skipped(why.to_string());
        }
    }

    /// Someone of the household free to drive it out and serve: a grown-up
    /// at home (or standing about), awake or early enough to be got up,
    /// not due at a job now.
    fn truck_crew(&self, id: TruckId) -> Option<CitizenId> {
        let t = self.food_trucks.trucks.get(id)?;
        if let Some(c) = t.crew.filter(|c| self.citizens.contains_key(*c)) {
            return Some(c);
        }
        let hh = self.households.get(t.household)?;
        let hour = self.clock.hour() as u8;
        let operator = t.operator;
        let mut best: Option<(u8, CitizenId)> = None;
        for &m in &hh.members {
            let Some(c) = self.citizens.get(m) else { continue };
            if c.age < crate::citizens::ADULT_AGE || c.age >= 75 || self.food_trucks.crewed_by(m).is_some() {
                continue;
            }
            // Not on the move (in a car, on a bus or a train) and not at a
            // job's shift: someone at home, out in the park or at a shop
            // nips home to take the truck out.
            let free = matches!(c.activity, Activity::AtHome | Activity::Idle | Activity::Visiting(_) | Activity::Sitting { .. } | Activity::Working);
            let at_home = c.inside.is_some() && c.inside == hh.home;
            if !free {
                continue;
            }
            let on_shift = c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| b.shift_for(c.seed).contains(hour));
            if on_shift {
                continue;
            }
            // The one with no job first, then whoever is at home.
            let rank = if Some(m) == operator { 0 } else if c.work.is_none() { 1 } else { 2 } + if c.asleep { 3 } else { 0 } + if at_home { 0 } else { 6 };
            if best.is_none_or(|(r, _)| rank < r) {
                best = Some((rank, m));
            }
        }
        best.map(|(_, m)| m)
    }

    /// A free pitch near `at`: a legal kerb slot, else a trading pitch on
    /// a street whose kerb is for loading (shops, bars), never a clearway.
    pub(super) fn find_pitch(&self, at: Vec2) -> Option<Pitch> {
        for r in [120.0, PARK_REACH, PARK_REACH * 1.6] {
            if let Some(k) = self.free_kerbs_near(at, r, 1).into_iter().next() {
                return Some(Pitch { segment: k.segment, side: k.side, slot: k.slot, pos: k.pos, heading: k.heading, legal: true });
            }
        }
        for r in [120.0, PARK_REACH, PARK_REACH * 1.6] {
            let k = self.free_illegal_slots_near(at, r, 6).into_iter().find(|k| self.roads.try_segment(k.segment).is_some_and(|s| s.lanes_fwd < 2 && s.lanes_bwd < 2));
            if let Some(k) = k {
                return Some(Pitch { segment: k.segment, side: k.side, slot: k.slot, pos: k.pos, heading: k.heading, legal: false });
            }
        }
        // A corner place at the end of a street, clear of the junction.
        for r in [120.0, PARK_REACH, PARK_REACH * 1.6] {
            if let Some(k) = self.free_squeeze_near(at, r).filter(|k| !self.food_trucks.holds(k.segment, k.side, k.slot)) {
                return Some(Pitch { segment: k.segment, side: k.side, slot: k.slot, pos: k.pos, heading: k.heading, legal: false });
            }
        }
        None
    }

    /// Off to stint `i`: from home (buying the day's ingredients first)
    /// or from the pitch it stands at.
    fn set_off(&mut self, id: TruckId, i: usize, from_pitch: Option<Pitch>) {
        let tick = self.clock.tick;
        let (kind, base, at) = {
            let t = &self.food_trucks.trucks[id];
            (t.kind, t.base, t.plan[i].at)
        };
        if kind.row().warm_only && !ice_cream_weather(self.weather.temp_c, crate::yards::wet(&self.weather)) {
            let why = if crate::yards::wet(&self.weather) { "wet weather: nobody wants ice cream" } else { "too cold for ice cream" };
            self.skip_stint(id, i, why);
            self.food_trucks.stats.too_cold_today += 1;
            return;
        }
        // Nobody free, or nowhere to park: try again in a few minutes, till
        // half the stint is gone.
        let (start, end) = Self::stint_ticks(&self.food_trucks.trucks[id].plan[i]);
        let give_up = tick > start + (end - start) / 2;
        let Some(crew) = self.truck_crew(id) else {
            if give_up {
                self.skip_stint(id, i, "nobody free to drive it");
            }
            return;
        };
        let Some(pitch) = self.find_pitch(at) else {
            if give_up {
                self.skip_stint(id, i, "nowhere to park near the crowd");
            }
            return;
        };
        if from_pitch.is_none() && self.food_trucks.trucks[id].stock < kind.row().stock * 0.5 {
            self.restock_truck(id);
        }
        if self.food_trucks.trucks[id].stock < 5.0 {
            self.skip_stint(id, i, "no money for ingredients");
            return;
        }
        let from = match from_pitch {
            Some(p) => Some((p.segment, p.pos)),
            None => self.truck_stop(base),
        };
        let Some((fseg, fpt)) = from else {
            self.skip_stint(id, i, "no street from home");
            return;
        };
        let Some((route, s0, s1)) = self.route_any_lane(fseg, fpt, pitch.segment, pitch.pos) else {
            self.skip_stint(id, i, "no road there");
            return;
        };
        let model = self.food_trucks.trucks[id].model;
        let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, model.paint);
        {
            let c = &mut self.traffic.cars[car];
            c.model = model;
            c.errand = Errand::FoodTruck(id);
            c.lateral = c.lateral.max(2.0);
        }
        // The crew gets in (from home, or back from wherever they were).
        if self.citizens[crew].inside.is_some() || !matches!(self.citizens[crew].activity, Activity::Riding { .. } | Activity::Idle) {
            self.leave_building(crew);
        }
        if let Some(e) = self.building_entrance(base) {
            let c = &mut self.citizens[crew];
            c.pos = e;
            c.prev_pos = e;
        }
        let name = self.food_trucks.trucks[id].name.clone();
        let spot = self.food_trucks.trucks[id].plan[i].name.clone();
        {
            let c = &mut self.citizens[crew];
            c.asleep = false;
            c.inside = None;
            c.activity = Activity::Riding { bus: car };
            c.transit = None;
            c.thought = format!("Taking {name} out to {spot}.");
            // A meal off the truck before the day starts.
            c.satiety = c.satiety.max(0.7);
        }
        let t = &mut self.food_trucks.trucks[id];
        t.crew = Some(crew);
        t.place = TruckPlace::Road(car);
        t.job = TruckJob::ToPitch { stint: i as u8, pitch };
        t.plan[i].state = StintState::Driving;
        t.since_job = tick;
    }

    /// The morning's shopping: the day's servings of each ingredient, from
    /// the businesses in town holding it (the nearest first, at most half
    /// of what each has), the rest wholesale from out of town, as far as
    /// the household can pay.
    fn restock_truck(&mut self, id: TruckId) {
        let (kind, hh, base, have) = {
            let t = &self.food_trucks.trucks[id];
            (t.kind, t.household, t.base, t.stock)
        };
        let row = kind.row();
        let Some(money) = self.households.get(hh).map(|h| h.money) else { return };
        let per_meal_wholesale: f32 = row.ingredients.iter().map(|(r, n)| r.base_price() * 1.6 * n).sum();
        let mut want = (row.stock - have).max(0.0);
        // As far as the purse goes (keeping something for the household).
        let afford = ((money - 50.0).max(0.0) / per_meal_wholesale.max(0.01)).floor();
        want = want.min(afford);
        if want < 5.0 {
            return;
        }
        let at = self.building_lot(base).map(|l| l.center).unwrap_or_default();
        let mut local = 0.0f32;
        let mut whole = 0.0f32;
        let mut cost = 0.0f32;
        for &(r, per) in row.ingredients {
            let mut need = want * per;
            // Sellers in town holding it, nearest first.
            let mut sellers: Vec<(f32, BuildingId, f32)> = self
                .buildings
                .iter()
                .filter(|(_, b)| b.operating() && !b.is_derelict())
                .filter_map(|(bid, b)| {
                    let z = b.business.as_ref().filter(|z| !z.closed)?;
                    let s = z.stock_of(r);
                    (s >= 0.5).then(|| (self.lots.get(b.lot).map_or(f32::MAX, |l| l.center.distance(at)), bid, s))
                })
                .collect();
            sellers.sort_by(|a, b| a.0.total_cmp(&b.0).then(key(a.1).cmp(&key(b.1))));
            for (_, bid, s) in sellers.into_iter().take(6) {
                if need <= 0.01 {
                    break;
                }
                let take = need.min(s * 0.5);
                let got = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()).map_or(0.0, |z| z.take(r, take));
                if got <= 0.0 {
                    continue;
                }
                let price = got * r.base_price() * 1.15;
                let paid = self.transfer(Account::Household(hh), Account::Business(bid), price, Kind::Supplies);
                if let Some(z) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()) {
                    z.revenue_today += paid;
                    z.deals += 1;
                }
                cost += paid;
                need -= got;
                local += got / per;
            }
            if need > 0.01 {
                let paid = self.transfer(Account::Household(hh), Account::Outside, need * r.base_price() * 1.6, Kind::Imports);
                cost += paid;
                whole += need / per;
            }
        }
        let n_ing = row.ingredients.len().max(1) as f32;
        let t = &mut self.food_trucks.trucks[id];
        t.stock += want;
        t.costs_today += cost;
        t.bought_local_today += local / n_ing;
        t.bought_wholesale_today += whole / n_ing;
    }

    /// The truck at its pitch: in it pulls, the crew to the hatch.
    pub(super) fn food_truck_arrived(&mut self, car: CarId, id: TruckId) {
        let tick = self.clock.tick;
        let Some(t) = self.food_trucks.trucks.get(id) else {
            self.traffic.remove_car(car);
            return;
        };
        let (job, crew) = (t.job, t.crew);
        let pos = self.traffic.cars.get(car).map_or(t.last_pos, |c| c.pos);
        match job {
            TruckJob::ToPitch { stint, pitch } => {
                self.traffic.remove_car(car);
                // [r14-foodtrucks] A route ends on the lane beside the slot;
                // one that ended far off (re-planned roads) parks where it
                // stopped, at the kerb.
                let pitch = if pos.distance(pitch.pos) < 60.0 {
                    pitch
                } else {
                    match self.find_pitch(pos) {
                        Some(p) => p,
                        None => {
                            self.truck_home_now(id);
                            return;
                        }
                    }
                };
                let cart = self.food_trucks.trucks[id].kind.cart();
                let t = &mut self.food_trucks.trucks[id];
                t.place = TruckPlace::Pitch { pitch, open: false };
                t.job = TruckJob::Idle;
                t.last_pos = pitch.pos;
                t.since_job = tick;
                let (start, end) = t.plan.get(stint as usize).map(Self::stint_ticks).unwrap_or((tick, tick));
                if let Some(c) = crew.and_then(|c| self.citizens.get_mut(c)) {
                    let v = pitch.vendor_spot(cart);
                    c.activity = Activity::Idle;
                    c.inside = None;
                    c.pos = v;
                    c.prev_pos = v;
                    c.heading = pitch.out().y.atan2(pitch.out().x);
                    c.prev_heading = c.heading;
                    c.next_decision_tick = end + 10;
                    c.thought = "Hatch up, awning out. Here they come.".to_string();
                }
                if tick + 10 >= start {
                    self.open_truck(id, stint as usize);
                }
            }
            TruckJob::Home => {
                self.traffic.remove_car(car);
                self.truck_home_now(id);
            }
            TruckJob::Leaving => {
                self.traffic.remove_car(car);
                self.food_trucks.trucks.remove(id);
            }
            TruckJob::Idle => {
                self.traffic.remove_car(car);
                self.truck_home_now(id);
            }
        }
    }

    /// Hatch up: open for business at stint `i`.
    fn open_truck(&mut self, id: TruckId, i: usize) {
        let tick = self.clock.tick;
        let t = &mut self.food_trucks.trucks[id];
        let TruckPlace::Pitch { pitch, .. } = t.place else { return };
        t.place = TruckPlace::Pitch { pitch, open: true };
        let Some(s) = t.plan.get_mut(i) else { return };
        s.state = StintState::Trading;
        t.visits.push(Visit { kind: s.kind, name: s.name.clone(), at: pitch.pos, opened: tick, closed: None, meals: 0, takings: 0.0 });
    }

    /// Hatch down: the stint is over (`why` when it ends early).
    fn close_truck(&mut self, id: TruckId, i: usize, why: Option<&str>) {
        let tick = self.clock.tick;
        let t = &mut self.food_trucks.trucks[id];
        if let TruckPlace::Pitch { pitch, .. } = t.place {
            t.place = TruckPlace::Pitch { pitch, open: false };
        }
        if let Some(s) = t.plan.get_mut(i) {
            s.state = match why {
                Some(w) if w != "sold out" => StintState::Skipped(w.to_string()),
                _ => StintState::Done,
            };
        }
        if let Some(v) = t.visits.last_mut().filter(|v| v.closed.is_none()) {
            v.closed = Some(tick);
        }
        // The queue goes.
        let queued: Vec<CitizenId> = self.food_trucks.trips.iter().filter(|(_, t)| **t == id).map(|(c, _)| *c).collect();
        for c in queued {
            self.food_trucks.trips.remove(&c);
        }
    }

    /// Home from the pitch (or wherever it stands).
    fn truck_go_home(&mut self, id: TruckId, from: Option<Pitch>) {
        let tick = self.clock.tick;
        let (base, model, crew) = {
            let t = &self.food_trucks.trucks[id];
            (t.base, t.model, t.crew)
        };
        let Some(p) = from else {
            self.truck_home_now(id);
            return;
        };
        let to = self.truck_stop(base);
        let route = to.and_then(|(seg, pt)| self.route_any_lane(p.segment, p.pos, seg, pt));
        let Some((route, s0, s1)) = route else {
            self.truck_home_now(id);
            return;
        };
        let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, model.paint);
        {
            let c = &mut self.traffic.cars[car];
            c.model = model;
            c.errand = Errand::FoodTruck(id);
            c.lateral = c.lateral.max(p.pos.distance(c.pos).min(2.6));
        }
        if let Some(c) = crew.and_then(|c| self.citizens.get_mut(c)).filter(|c| matches!(c.activity, Activity::Idle)) {
            c.activity = Activity::Riding { bus: car };
            c.inside = None;
            c.thought = "Packed up. Home.".to_string();
        }
        let t = &mut self.food_trucks.trucks[id];
        t.place = TruckPlace::Road(car);
        t.job = TruckJob::Home;
        t.since_job = tick;
    }

    /// Back home now: the truck in the yard, the crew indoors.
    fn truck_home_now(&mut self, id: TruckId) {
        let tick = self.clock.tick;
        let Some(t) = self.food_trucks.trucks.get_mut(id) else { return };
        // A stint driving or trading when it was cut short is over.
        for s in t.plan.iter_mut() {
            if matches!(s.state, StintState::Driving | StintState::Trading) {
                s.state = StintState::Done;
            }
        }
        if let Some(v) = t.visits.last_mut().filter(|v| v.closed.is_none()) {
            v.closed = Some(tick);
        }
        let (crew, base) = (t.crew.take(), t.base);
        t.place = TruckPlace::Base;
        t.job = TruckJob::Idle;
        t.since_job = tick;
        if let Some(lot) = self.building_lot(base) {
            let p = lot.center;
            self.food_trucks.trucks[id].last_pos = p;
        }
        let queued: Vec<CitizenId> = self.food_trucks.trips.iter().filter(|(_, t)| **t == id).map(|(c, _)| *c).collect();
        for c in queued {
            self.food_trucks.trips.remove(&c);
        }
        if let Some(cid) = crew {
            self.crew_home(cid, base);
            self.hold_operator(id);
        }
    }

    /// The crew back indoors at home (if still with the truck).
    fn crew_home(&mut self, cid: CitizenId, base: BuildingId) {
        let tick = self.clock.tick;
        let entrance = self.building_entrance(base);
        let Some(c) = self.citizens.get_mut(cid) else { return };
        if !matches!(c.activity, Activity::Riding { .. } | Activity::Idle) || c.home != Some(base) {
            return;
        }
        if let Some(e) = entrance {
            c.pos = e;
            c.prev_pos = e;
        }
        c.inside = Some(base);
        c.activity = Activity::AtHome;
        c.next_decision_tick = tick + 10;
    }

    // ── customers ──────────────────────────────────────────────────────────

    /// The open truck nearest `here` within `reach` with room in its
    /// queue and something left to sell.
    fn truck_near(&self, here: Vec2, reach: f32) -> Option<(TruckId, Pitch, f32)> {
        self.food_trucks
            .open_trucks()
            .filter(|(id, t, _)| t.stock >= 1.0 && self.food_trucks.queue_len(*id) < QUEUE_MAX)
            .map(|(id, _, p)| (id, p, p.pos.distance(here)))
            .filter(|x| x.2 <= reach)
            .min_by(|a, b| a.2.total_cmp(&b.2).then(tkey(a.0).cmp(&tkey(b.0))))
    }

    /// Would `id` stop for a bite from a truck near them now? People out
    /// in a park, at the game, on a night out, on their lunch break or
    /// strolling past, the hungrier the likelier (ice cream tempts the
    /// young whatever), once every few hours, if they can pay. True when
    /// they set off for the hatch.
    pub(super) fn truck_bite(&mut self, id: CitizenId, reach: f32, base_chance: f32) -> bool {
        if self.food_trucks.trucks.is_empty() || self.food_trucks.disabled {
            return false;
        }
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        let Some(c) = self.citizens.get(id) else { return false };
        if c.home.is_none() || c.age < 8 || c.asleep || self.food_trucks.trips.contains_key(&id) {
            return false;
        }
        if self.food_trucks.last_meal.get(&id).is_some_and(|&t| tick < t + 180) {
            return false;
        }
        // Not mid-shift, but a lunch break is fine.
        let at_work = c.inside.is_some() && c.inside == c.work;
        if at_work && !(11..14).contains(&hour) {
            return false;
        }
        // Not from home: they eat in.
        if c.inside.is_some() && c.inside == c.home {
            return false;
        }
        if self.food_trucks.crewed_by(id).is_some() {
            return false;
        }
        let here = match c.inside.and_then(|b| self.building_entrance(b)) {
            Some(e) => e,
            None => c.pos,
        };
        let Some((tid, pitch, d)) = self.truck_near(here, reach) else { return false };
        let kind = self.food_trucks.trucks[tid].kind;
        let row = kind.row();
        let mut want = base_chance;
        if c.satiety < 0.6 {
            want += 0.3;
        }
        if c.satiety < 0.35 {
            want += 0.3;
        }
        if kind == TruckKind::IceCream && c.age < 30 {
            want += 0.15;
        }
        if row.satiety < 0.3 && c.satiety > 0.85 && kind != TruckKind::IceCream {
            want *= 0.5;
        }
        want *= 1.0 - 0.5 * (d / reach.max(1.0));
        if crate::hash::hash01(c.seed ^ tkey(tid), tick, 0xB17E) >= want {
            return false;
        }
        let purse = self.purse_of(id);
        let can_pay = match purse {
            Account::Household(h) => self.households.get(h).is_some_and(|x| x.money >= row.price),
            _ => c.savings >= row.price,
        };
        if !can_pay {
            return false;
        }
        let k = self.food_trucks.queue_len(tid);
        let spot = pitch.queue_spot(k, kind.cart());
        let from = here;
        let Some(path) = self.ground_route(from, spot) else { return false };
        let exit = self.leave_building(id);
        let name = self.food_trucks.trucks[tid].name.clone();
        let c = &mut self.citizens[id];
        if let Some(p) = exit {
            c.pos = p;
            c.prev_pos = p;
        }
        c.mode = crate::citizens::TravelMode::Walk;
        c.transit = None;
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
        c.next_decision_tick = tick + 90;
        c.thought = match kind {
            TruckKind::IceCream => format!("Is that {name}? Ice cream!"),
            TruckKind::Coffee => format!("Coffee from {name}."),
            _ => format!("Something from {name}, the truck by the kerb."),
        };
        self.food_trucks.trips.insert(id, tid);
        true
    }

    /// Someone set on a meal out goes to a truck near by instead of a shop,
    /// half the time.
    pub(super) fn truck_instead_of_shop(&mut self, id: CitizenId) -> bool {
        if self.food_trucks.trucks.is_empty() {
            return false;
        }
        let Some(c) = self.citizens.get(id) else { return false };
        if crate::hash::hash01(c.seed, self.clock.tick, 0x5B0F) > 0.5 {
            return false;
        }
        self.truck_bite(id, STROLL_REACH, 1.0)
    }

    /// At the hatch: served (pay, eat), then a few minutes at the kerb.
    /// False when this was no truck visit, for the usual arrival.
    pub(super) fn truck_arrive(&mut self, id: CitizenId) -> bool {
        let Some(&tid) = self.food_trucks.trips.get(&id) else { return false };
        let tick = self.clock.tick;
        let open = self.food_trucks.trucks.get(tid).and_then(|t| if t.place.open() { t.place.pitch().copied() } else { None });
        let pos = self.citizens[id].pos;
        let Some(pitch) = open.filter(|p| p.pos.distance(pos) < 14.0) else {
            self.food_trucks.trips.remove(&id);
            if open.is_none() {
                let c = &mut self.citizens[id];
                c.activity = Activity::Idle;
                c.inside = None;
                c.thought = "The truck's packed up and gone.".to_string();
                c.next_decision_tick = tick + 5;
                return true;
            }
            return false;
        };
        let (kind, household, name) = {
            let t = &self.food_trucks.trucks[tid];
            (t.kind, t.household, t.name.clone())
        };
        if self.food_trucks.trucks[tid].stock < 1.0 {
            self.food_trucks.trips.remove(&id);
            let c = &mut self.citizens[id];
            c.activity = Activity::Idle;
            c.thought = format!("{name} sold out just as I got there.");
            c.next_decision_tick = tick + 5;
            return true;
        }
        let row = kind.row();
        let (seed, class, hungry_before, ahead) = {
            let c = &self.citizens[id];
            let ahead = self.food_trucks.trips.iter().filter(|(o, t)| **t == tid && **o != id && self.citizens.get(**o).is_some_and(|x| matches!(x.activity, Activity::Idle))).count();
            (c.seed, c.class(), c.satiety < 0.25, ahead)
        };
        let price = row.price * super::places::tab_factor(class) * (0.9 + 0.2 * crate::hash::hash01(seed, tick, 0x9A1));
        let purse = self.purse_of(id);
        let paid = self.transfer(purse, Account::Household(household), price, Kind::Outings);
        let stay = SERVE_MINUTES + ahead as u64 * 2 + EAT_MINUTES + crate::hash::mix(seed, tick) % 6;
        let memory = format!("Bought {} from {name} at the kerb.", match kind {
            TruckKind::IceCream => "an ice cream",
            TruckKind::Taco => "tacos",
            TruckKind::Noodle => "a box of noodles",
            TruckKind::Kebab => "a kebab",
            TruckKind::Coffee => "a coffee",
            TruckKind::HotDog => "a hot dog",
            TruckKind::Fruit => "a cup of fruit",
        });
        {
            let c = &mut self.citizens[id];
            c.activity = Activity::Idle;
            c.inside = None;
            c.next_decision_tick = tick + stay;
            c.satiety = (c.satiety + row.satiety).min(1.0);
            c.mood = (c.mood + 0.02).min(1.0);
            c.thought = match kind {
                TruckKind::IceCream => "Dripping down the cone already.".to_string(),
                TruckKind::Coffee => "Hot coffee, cold fingers.".to_string(),
                TruckKind::Kebab => "Best thing at this hour.".to_string(),
                _ => "Eating it standing at the kerb. Good, though.".to_string(),
            };
            let d = pitch.pos - c.pos;
            if d.length_squared() > 0.01 {
                c.heading = d.y.atan2(d.x);
                c.prev_heading = c.heading;
            }
            if crate::hash::hash01(seed, tkey(tid), 0x4E) < 0.3 {
                c.remember(tick, memory);
            }
        }
        self.food_trucks.last_meal.insert(id, tick);
        let spot_kind = self.food_trucks.trucks[tid].visits.last().map(|v| v.kind).unwrap_or_default();
        let t = &mut self.food_trucks.trucks[tid];
        t.stock -= 1.0;
        t.meals_today += 1;
        t.meals_total += 1;
        t.takings_today += paid;
        t.takings_total += paid as f64;
        if hungry_before {
            t.fed_hungry_today += 1;
        }
        if let Some(v) = t.visits.last_mut() {
            v.meals += 1;
            v.takings += paid;
        }
        let s = &mut self.food_trucks.stats;
        s.meals_today += 1;
        s.meals_total += 1;
        s.takings_today += paid;
        if hungry_before {
            s.fed_hungry_today += 1;
        }
        if s.meals_by_spot.len() < SpotKind::COUNT {
            s.meals_by_spot.resize(SpotKind::COUNT, 0);
        }
        s.meals_by_spot[spot_kind.index()] += 1;
        true
    }

    /// Customers no longer on their way to (or at) their truck leave its
    /// queue.
    fn tidy_truck_queues(&mut self) {
        let trucks = &self.food_trucks.trucks;
        let citizens = &self.citizens;
        self.food_trucks.trips.retain(|cid, tid| {
            let Some(p) = trucks.get(*tid).and_then(|t| if t.place.open() { t.place.pitch().copied() } else { None }) else { return false };
            match citizens.get(*cid).map(|c| (&c.activity, c.pos)) {
                Some((Activity::Idle, pos)) => pos.distance(p.pos) < 14.0,
                Some((Activity::Traveling { to: None, purpose: Purpose::Wander, .. }, _)) => true,
                _ => false,
            }
        });
    }

    // ── midnight ───────────────────────────────────────────────────────────

    /// The books: the lease and the permit, the day's tallies rolled, a
    /// truck losing money for a week (or whose household left) sold; new
    /// trucks where crowds went unfed.
    fn daily_food_trucks(&mut self) {
        let day = self.clock.day();
        self.food_trucks.last_meal.retain(|_, t| *t + crate::clock::MINUTES_PER_DAY > day * crate::clock::MINUTES_PER_DAY);
        let ids: Vec<TruckId> = self.food_trucks.trucks.keys().collect();
        for id in ids {
            let (hh, traded, leaving) = {
                let t = &self.food_trucks.trucks[id];
                (t.household, !t.visits.is_empty(), t.job == TruckJob::Leaving)
            };
            if leaving {
                continue;
            }
            let mut cost = self.transfer(Account::Household(hh), Account::Outside, LEASE_PER_DAY, Kind::Cars);
            if traded {
                cost += self.transfer(Account::Household(hh), Account::Treasury, PERMIT_PER_DAY, Kind::Fees);
            }
            let gone = self.households.get(hh).is_none_or(|h| h.home.is_none() || h.members.is_empty());
            let t = &mut self.food_trucks.trucks[id];
            t.costs_today += cost;
            if t.takings_today + 0.01 < t.costs_today {
                t.losing_days = t.losing_days.saturating_add(1);
            } else if t.takings_today > 0.0 {
                t.losing_days = 0;
            }
            t.visits_yesterday = std::mem::take(&mut t.visits);
            t.meals_yesterday = t.meals_today;
            t.takings_yesterday = t.takings_today;
            t.costs_yesterday = t.costs_today;
            t.meals_today = 0;
            t.takings_today = 0.0;
            t.costs_today = 0.0;
            t.bought_local_today = 0.0;
            t.bought_wholesale_today = 0.0;
            t.fed_hungry_today = 0;
            // A truck still out keeps its open visit for tonight.
            if t.place.open() {
                if let Some(v) = t.visits_yesterday.last().cloned().filter(|v| v.closed.is_none()) {
                    t.visits.push(Visit { meals: 0, takings: 0.0, ..v });
                }
            }
            let sell = gone || t.losing_days >= LOSING_DAYS;
            if sell {
                self.sell_truck(id, if gone { "the family left town" } else { "a week of losing money" });
            }
        }
        let s = &mut self.food_trucks.stats;
        s.meals_yesterday = s.meals_today;
        s.takings_yesterday = s.takings_today;
        s.fed_hungry_yesterday = s.fed_hungry_today;
        s.open_by_hour_yesterday = std::mem::take(&mut s.open_by_hour);
        s.meals_by_spot_yesterday = std::mem::take(&mut s.meals_by_spot);
        s.too_cold_yesterday = s.too_cold_today;
        s.meals_today = 0;
        s.takings_today = 0.0;
        s.fed_hungry_today = 0;
        s.too_cold_today = 0;
        if !self.food_trucks.disabled {
            self.found_food_trucks();
        }
    }

    /// Sold: a truck at home goes at once; one out drives home first (the
    /// day's trade done), then is sold from there.
    fn sell_truck(&mut self, id: TruckId, why: &str) {
        let Some(t) = self.food_trucks.trucks.get(id) else { return };
        let (name, at, place) = (t.name.clone(), t.pos(), t.place);
        if let TruckPlace::Road(car) = place {
            if let Some(cid) = self.food_trucks.trucks[id].crew.take() {
                let base = self.food_trucks.trucks[id].base;
                self.crew_home(cid, base);
            }
            let t = &mut self.food_trucks.trucks[id];
            t.job = TruckJob::Leaving;
            if !self.redirect_out(car) {
                self.food_trucks.trucks.remove(id);
                self.traffic.remove_car(car);
            }
        } else {
            if let Some(cid) = self.food_trucks.trucks[id].crew {
                let base = self.food_trucks.trucks[id].base;
                self.crew_home(cid, base);
            }
            self.food_trucks.trucks.remove(id);
        }
        self.food_trucks.trips.retain(|_, t| *t != id);
        self.food_trucks.stats.sold_total += 1;
        let tick = self.clock.tick;
        self.social.push_news(tick, format!("🚚 {name} is up for sale: {why}."), at, None);
    }

    /// New trucks where the crowds outnumber them: up to `NEW_PER_NIGHT` a
    /// night and one per `PEOPLE_PER_TRUCK` residents, each of the type the
    /// town's crowds want most for the trucks of it already out, started by
    /// a household with savings (one with someone out of work first).
    pub fn found_food_trucks(&mut self) -> usize {
        let cap = (self.citizens.len() / PEOPLE_PER_TRUCK).max(1);
        let mut made = 0;
        while made < NEW_PER_NIGHT && self.food_trucks.trucks.len() < cap {
            let spots = self.crowd_spots();
            let crowd: f32 = spots.iter().map(|s| s.crowd).sum();
            if crowd < MIN_CROWD * 4.0 * (self.food_trucks.trucks.len() + 1) as f32 {
                break;
            }
            let kind = self.wanted_truck_kind(&spots);
            if self.found_food_truck(kind, None).is_err() {
                break;
            }
            made += 1;
        }
        made
    }

    /// The type of truck the town's crowds want most, against the trucks of
    /// each type already out (the weather an ordinary afternoon's).
    fn wanted_truck_kind(&self, spots: &[CrowdSpot]) -> TruckKind {
        let temp = self.weather.temp_at(14.0);
        let mut counts = [0u32; TruckKind::COUNT];
        for t in self.food_trucks.trucks.values() {
            counts[t.kind.index()] += 1;
        }
        let salt = self.clock.day() ^ self.food_trucks.numbered as u64;
        TruckKind::ALL
            .into_iter()
            .map(|k| {
                let v: f32 = spots.iter().map(|s| (s.from..s.to).map(|h| if k.trades_at(h % 24) { appeal(k, s, h, temp, false) } else { 0.0 }).fold(0.0, f32::max)).sum();
                let v = v / (1.0 + 1.5 * counts[k.index()] as f32) * (1.0 + 0.1 * crate::hash::hash01(salt, k as u64, 0x7A));
                (v, k)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|(_, k)| k)
            .unwrap_or_default()
    }

    /// [r14-foodtrucks] Start a truck of type `kind` (the remote API's dev
    /// lever, and the nightly start-ups): run by `household`, or the
    /// household best placed for it (savings put by, someone out of work,
    /// a home with a street). Its deposit is paid; it plans its day.
    pub fn found_food_truck(&mut self, kind: TruckKind, household: Option<HouseholdId>) -> Result<TruckId, String> {
        let owners: std::collections::BTreeSet<HouseholdId> = self.food_trucks.trucks.values().map(|t| t.household).collect();
        let hh = match household {
            Some(h) => {
                if !self.households.contains_key(h) {
                    return Err("no such household".into());
                }
                h
            }
            None => {
                let salt = self.clock.day() ^ (self.food_trucks.numbered as u64).wrapping_mul(0x51);
                let mut best: Option<(u8, u64, HouseholdId)> = None;
                for (hid, h) in self.households.iter() {
                    if owners.contains(&hid) || h.money < START_SAVINGS || h.members.is_empty() {
                        continue;
                    }
                    let Some(home) = h.home else { continue };
                    if self.truck_stop(home).is_none() {
                        continue;
                    }
                    let adults: Vec<&crate::citizens::Citizen> = h.members.iter().filter_map(|m| self.citizens.get(*m)).filter(|c| c.age >= crate::citizens::ADULT_AGE && c.age < 65).collect();
                    if adults.is_empty() {
                        continue;
                    }
                    let rank = if adults.iter().any(|c| c.work.is_none()) { 0 } else { 1 };
                    let h = crate::hash::mix(slotmap::Key::data(&hid).as_ffi(), salt);
                    if best.is_none_or(|(r, bh, _)| (rank, h) < (r, bh)) {
                        best = Some((rank, h, hid));
                    }
                }
                best.ok_or("no household with the savings to start one")?.2
            }
        };
        let home = self.households[hh].home.ok_or("the household has no home")?;
        let operator = self.households[hh].members.iter().copied().filter(|m| self.citizens.get(*m).is_some_and(|c| c.age >= crate::citizens::ADULT_AGE && c.age < 65)).min_by_key(|m| (self.citizens[*m].work.is_some(), slotmap::Key::data(m).as_ffi()));
        let family = self.households[hh].members.iter().filter_map(|m| self.citizens.get(*m)).next().map(|c| c.name.rsplit(' ').next().unwrap_or(&c.name).to_string()).unwrap_or_else(|| "Corner".into());
        self.transfer(Account::Household(hh), Account::Outside, TRUCK_DEPOSIT, Kind::Cars);
        self.food_trucks.numbered += 1;
        let number = self.food_trucks.numbered;
        let name = truck_name(kind, &family, crate::hash::mix(slotmap::Key::data(&hh).as_ffi(), 0xF7));
        let at = self.building_lot(home).map(|l| l.center).unwrap_or_default();
        let tick = self.clock.tick;
        let id = self.food_trucks.trucks.insert(Truck {
            kind,
            name: name.clone(),
            household: hh,
            base: home,
            number,
            model: kind.model(number),
            place: TruckPlace::Base,
            job: TruckJob::Idle,
            crew: None,
            operator,
            plan: Vec::new(),
            stock: 0.0,
            visits: Vec::new(),
            visits_yesterday: Vec::new(),
            meals_today: 0,
            meals_yesterday: 0,
            meals_total: 0,
            takings_today: 0.0,
            takings_yesterday: 0.0,
            takings_total: 0.0,
            costs_today: 0.0,
            costs_yesterday: 0.0,
            bought_local_today: 0.0,
            bought_wholesale_today: 0.0,
            losing_days: 0,
            fed_hungry_today: 0,
            since: tick,
            last_pos: at,
            since_job: tick,
        });
        self.food_trucks.stats.founded_total += 1;
        self.social.push_news(tick, format!("🚚 {name}, a new {}, starts trading round town.", kind.label()), at, None);
        // Today's plan from the next hour (at night: the morning's plan).
        let hour = self.clock.hour() as u8;
        if (PLAN_HOUR..22).contains(&hour) {
            self.plan_food_trucks(hour + 1);
        }
        Ok(id)
    }

    /// After a load: the crews of trucks on the road ride with them again
    /// (the save's riders were set down), a truck whose car was not kept is
    /// home, and the crews at open trucks stand at the hatch.
    pub(super) fn food_trucks_after_load(&mut self) {
        let ids: Vec<TruckId> = self.food_trucks.trucks.keys().collect();
        for id in ids {
            let (place, crew) = {
                let t = &self.food_trucks.trucks[id];
                (t.place, t.crew)
            };
            match place {
                TruckPlace::Road(car) => {
                    if !self.traffic.cars.get(car).is_some_and(|c| c.errand == Errand::FoodTruck(id)) {
                        self.truck_home_now(id);
                    } else if let Some(c) = crew.and_then(|c| self.citizens.get_mut(c)) {
                        c.activity = Activity::Riding { bus: car };
                        c.inside = None;
                    }
                }
                TruckPlace::Pitch { .. } | TruckPlace::Base => {
                    if crew.is_some_and(|c| !self.citizens.contains_key(c)) {
                        self.food_trucks.trucks[id].crew = None;
                    }
                }
            }
        }
    }

    /// Parked trucks to draw: id, pose, model, open, type.
    pub fn parked_food_trucks(&self) -> impl Iterator<Item = (TruckId, Vec2, f32, crate::vehicle::VehicleModel, bool, TruckKind)> + '_ {
        self.food_trucks.trucks.iter().filter_map(|(id, t)| t.place.pitch().map(|p| (id, p.pos, p.heading, t.model, t.place.open(), t.kind)))
    }

    /// The truck a car on the road is.
    pub fn food_truck_of_car(&self, car: CarId) -> Option<TruckId> {
        match self.traffic.cars.get(car)?.errand {
            Errand::FoodTruck(id) => Some(id),
            _ => None,
        }
    }
}
