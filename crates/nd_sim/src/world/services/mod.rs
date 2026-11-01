//! Services and emergencies: placing service buildings and attractions,
//! road-distance coverage, the hourly roll for fires, crimes and illness,
//! the 911 call, the dispatcher, the units driving out and back, fire
//! damage and spread, hospital stays, stadium nights, upkeep and the
//! schooling wage bonus. The data types live in `crate::services`.

use super::*;
use crate::services::*;
use std::cmp::Ordering;
use rand::seq::SliceRandom;
use std::collections::BinaryHeap;

mod place;
mod coverage;
mod incidents;
mod staffing;
mod dispatch;
mod units;
mod fire;
mod care;
mod events;
#[cfg(test)]
mod tests;

/// A node and the road budget left on reaching it, ordered by budget.
#[derive(PartialEq)]
struct Budget(f32, u32);

impl Eq for Budget {}

impl PartialOrd for Budget {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Budget {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0).then(self.1.cmp(&other.1))
    }
}

/// Floors a service building is drawn and powered with, by tier.
fn service_floors(kind: ServiceKind, tier: Tier) -> u8 {
    let f: [u8; 3] = match kind {
        ServiceKind::Police => [1, 2, 4],
        ServiceKind::Fire => [1, 2, 3],
        ServiceKind::Health => [2, 4, 8],
        ServiceKind::School => [1, 2, 4],
        ServiceKind::Dispatch => [1, 2, 5],
        ServiceKind::Monument => [1, 1, 1],
        ServiceKind::Stadium => [1, 2, 3],
        ServiceKind::Jail => [1, 2, 3],
        ServiceKind::TrashDepot => [1, 1, 2],
        ServiceKind::Vertiport => [1, 2, 4],
        ServiceKind::ParkingEnforcement => [1, 1, 2],
        ServiceKind::University => [2, 3, 5],
        // [attractions] Halls of two or three tall storeys; the grounds
        // people walk round are low.
        ServiceKind::ArtMuseum | ServiceKind::ScienceMuseum | ServiceKind::HistoryMuseum | ServiceKind::Aquarium | ServiceKind::ConcertHall => [3, 3, 3],
        ServiceKind::Observatory => [2, 2, 2],
        ServiceKind::Pier | ServiceKind::GrandPark | ServiceKind::Zoo | ServiceKind::BotanicalGarden | ServiceKind::AmusementPark => [1, 1, 1],
        // (D4) The halls; the nave is tall; the casino's hotel tower.
        ServiceKind::ConventionCentre => [3, 3, 3],
        ServiceKind::Cathedral => [4, 4, 4],
        ServiceKind::CasinoResort => [14, 14, 14],
        // [r6-architects] The house and halls are low; the housing slab
        // stands on its pillars twelve storeys up.
        ServiceKind::LedgeHouse | ServiceKind::HillChapel | ServiceKind::FanLibrary | ServiceKind::CultureHouse => [3, 3, 3],
        ServiceKind::PrairieHall => [2, 2, 2],
        ServiceKind::SkySlab => [12, 12, 12],
        // [r5-trash] Low sheds; the incinerator hall is tall.
        ServiceKind::Landfill => [1, 1, 1],
        ServiceKind::Incinerator => [3, 4, 5],
        ServiceKind::RecyclingPlant | ServiceKind::TransferStation => [1, 2, 2],
    };
    f[tier.index()]
}

impl World {
    /// The services' own random stream, so fires, crimes and illness never
    /// shift what the other systems draw. Not saved: a loaded world reseeds
    /// it from the seed and the clock.
    pub(super) fn srng(&mut self) -> &mut crate::SimRng {
        let (seed, tick) = (self.stream_seed(Stream::Services), self.clock.tick);
        self.services.rng.get_or_insert_with(|| crate::rng_for(seed, tick))
    }

    // ── the hourly roll ────────────────────────────────────────────────────

    pub(super) fn hourly_services(&mut self) {
        crate::prof_scope!("hourly_services");
        self.recruit_staff();
        self.balance_rosters();
        if !self.config.emergencies {
            let tick = self.clock.tick;
            let citizens = &self.citizens;
            self.services.admissions.retain(|a| a.until > tick && citizens.get(a.citizen).is_some_and(|c| c.inside == Some(a.hospital)));
            self.refresh_coverage();
            return;
        }
        let hour = self.clock.hour();
        let night = !(5..20).contains(&hour);
        let pop = self.citizens.len().max(1);
        let unemployment = self.citizens.values().filter(|c| c.work.is_none()).count() as f32 / pop as f32;
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for bid in ids {
            let b = &self.buildings[bid];
            if !b.operating() || b.land_use == LandUse::Park || b.service.is_some_and(|s| s.kind == ServiceKind::Monument) {
                continue;
            }
            let lot = b.lot;
            let lv = self.lots.get(lot).map_or(b.land_value, |l| self.land_value.at(l.center));
            let lu_fire = match b.land_use {
                LandUse::Industrial => 2.0,
                LandUse::Entertainment => 1.4,
                _ => 1.0,
            } * if b.is_derelict() { 2.0 } else { 1.0 };
            let p_fire = FIRE_RISK[b.tier.index()] * (1.0 + 2.0 * (1.0 - b.condition)) * lu_fire * (1.0 - FIRE_COVER_CUT * self.services.coverage_of(lot, ServiceKind::Fire));
            let occupied = !b.residents.is_empty() || !b.workers.is_empty() || b.business.is_some();
            let lu_crime = match b.land_use {
                LandUse::Entertainment => 1.5,
                LandUse::Commercial => 1.2,
                _ => 1.0,
            };
            let p_crime = CRIME_RISK * (1.0 + 3.0 * unemployment) * (1.4 - lv).max(0.3) * if night { 1.6 } else { 1.0 } * lu_crime * (1.0 - CRIME_COVER_CUT * self.services.coverage_of(lot, ServiceKind::Police));
            if self.srng().random::<f32>() < p_fire {
                self.start_fire(bid);
            }
            if occupied && self.srng().random::<f32>() < p_crime {
                self.start_crime(bid);
            }
        }
        // Illness: anyone indoors, more for the old and the hungry, less
        // where a hospital is near home.
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            let c = &self.citizens[cid];
            let Some(inside) = c.inside else { continue };
            // Small children and the old fall ill most.
            let age = c.age as f32;
            let frailty = if age < 6.0 { 1.8 } else if age >= 67.0 { 1.5 + (age - 67.0) / 15.0 } else { 0.6 + age / 80.0 };
            let hunger = ((0.35 - c.satiety) / 0.35).clamp(0.0, 1.0);
            let home_lot = c.home.and_then(|h| self.buildings.get(h)).or_else(|| self.buildings.get(inside)).map(|b| b.lot);
            let cov = home_lot.map_or(0.0, |l| self.services.coverage_of(l, ServiceKind::Health));
            let p = SICK_RISK * frailty * (1.0 + 4.0 * hunger) * (1.0 - SICK_COVER_CUT * cov);
            if self.srng().random::<f32>() < p && !self.is_patient(cid) {
                self.fall_ill(cid);
            }
        }
        // Discharged or wandered off: free the bed.
        let tick = self.clock.tick;
        let citizens = &self.citizens;
        self.services.admissions.retain(|a| a.until > tick && citizens.get(a.citizen).is_some_and(|c| c.inside == Some(a.hospital)));
        self.services.rubble.retain(|r| r.2 > tick);
        self.refresh_coverage();
    }

    // ── every tick ─────────────────────────────────────────────────────────

    pub(super) fn step_services(&mut self) {
        let tick = self.clock.tick;
        if tick % 10 == 3 {
            // Stations come and go; roads change under them.
            let lost: Vec<UnitId> = self.services.units.iter().filter(|(_, u)| !self.station_alive(u.station)).map(|(id, _)| id).collect();
            for id in lost {
                self.station_lost(id);
            }
            let stations: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind.unit_kind().is_some())).map(|(id, _)| id).collect();
            for s in stations {
                self.ensure_units(s);
            }
            if self.services.coverage_key != Some(self.coverage_signature()) {
                self.refresh_coverage();
            }
        }
        // [emergency] Car chases before lost units are looked for.
        self.step_chases();
        self.recover_lost_units();
        // Lights and sirens pull up short of the kerb and the crew is out
        // while the car is still rolling to a stop: on scene within
        // `PULL_UP` metres of the stop (the last stretch of braking took a
        // minute or two of clock).
        let pulling_up: Vec<CarId> = self
            .services
            .units
            .values()
            .filter(|u| u.state == UnitState::Responding)
            .filter_map(|u| u.car)
            .filter(|c| self.traffic.cars.get(*c).is_some_and(|car| car.idx + 1 == car.route.len() && car.s_end - car.s < PULL_UP))
            .collect();
        for car in pulling_up {
            self.unit_car_arrived(car);
        }
        let active: Vec<u32> = self.services.active().map(|i| i.id).collect();
        for id in active {
            self.step_incident(id);
        }
        self.step_packing();
        self.dispatch_queue();
        if tick % 5 == 2 {
            self.step_patrols();
        }
        if self.clock.minute_of_day() == EVENT_MINUTE {
            self.start_stadium_events();
        }
        if self.clock.minute_of_day() == SCHOOL_MINUTE {
            self.send_children_to_school();
        }
        if tick % 10 == 1 {
            self.step_custody();
        }
        // Pupils stay until the school closes.
        if tick % 10 == 5 {
            let hour = self.clock.hour() as u8;
            let buildings = &self.buildings;
            for c in self.citizens.values_mut() {
                if let Activity::Visiting(b) = c.activity {
                    if c.goes_to_school() && buildings.get(b).and_then(|x| x.service).is_some_and(|s| s.kind == ServiceKind::School && s.kind.hours().contains(hour)) {
                        c.next_decision_tick = c.next_decision_tick.max(tick + 15);
                    }
                }
            }
        }
        // Keep the record short.
        let resolved = self.services.incidents.iter().filter(|i| !i.active()).count();
        if resolved > KEEP_RESOLVED {
            let mut drop = resolved - KEEP_RESOLVED;
            self.services.incidents.retain(|i| {
                if !i.active() && drop > 0 {
                    drop -= 1;
                    false
                } else {
                    true
                }
            });
        }
        let s = &mut self.services;
        s.by_car = s.units.iter().filter_map(|(id, u)| u.car.map(|c| (c, id))).collect();
        s.stats.units = s.units.len() as u32;
        s.stats.units_busy = s.units.values().filter(|u| u.busy()).count() as u32;
        s.stats.on_patrol = s.units.values().filter(|u| u.state == UnitState::Patrol).count() as u32;
        s.stats.fires_burning = s.active().filter(|i| i.kind == IncidentKind::Fire).count() as u32;
        s.stats.waiting_for_ambulance = s.active().filter(|i| i.kind == IncidentKind::Medical && matches!(i.stage, Stage::Unreported | Stage::Reported | Stage::Dispatched)).count() as u32;
        s.stats.in_hospital = s.admissions.len() as u32;
    }

    // ── repairs ────────────────────────────────────────────────────────────

    fn step_incident(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        if !inc.on_road && !self.buildings.contains_key(inc.building) {
            self.resolve(id, Outcome::Gone);
            return;
        }
        // [services] The unit or aircraft on the job is gone (its station
        // pulled down, a drone scrapped, a car lost): the call goes back in
        // the queue, and a patient on the way to hospital is delivered.
        // Such incidents used to stay active for ever, pinning the patient,
        // the crime scene or the wrecks.
        if !inc.outside && matches!(inc.stage, Stage::Dispatched | Stage::OnScene | Stage::Transporting) && !self.incident_served(&inc) {
            // [fire-scene] The second engine is still on it: it takes the
            // fire over.
            if self.promote_second(id) {
                return;
            }
            self.orphaned(id);
            return;
        }
        if inc.kind == IncidentKind::Fire && self.step_fire(id) {
            return;
        }
        // A patient who is no longer where they fell ill (left town, the
        // building came down) is not waiting any more.
        if inc.kind == IncidentKind::Medical && matches!(inc.stage, Stage::Unreported | Stage::Reported | Stage::Dispatched | Stage::OnScene) {
            // [driving] Hurt on the road: still out there, not taken anywhere.
            let here = inc.citizen.and_then(|c| self.citizens.get(c)).is_some_and(|c| if inc.on_road { c.inside.is_none() && !c.activity.is_traveling() } else { c.inside == Some(inc.building) });
            if !here {
                self.resolve(id, Outcome::Gone);
                return;
            }
            let c = &mut self.citizens[inc.citizen.unwrap()];
            c.next_decision_tick = c.next_decision_tick.max(tick + 30);
        }
        match inc.stage {
            Stage::Unreported => {
                if tick >= inc.report_at {
                    self.report(id);
                }
            }
            Stage::Reported => {
                let since = tick.saturating_sub(inc.reported.unwrap_or(tick));
                // (Dispatch itself runs over the whole queue: `dispatch_queue`.)
                let give_up = match inc.kind {
                    IncidentKind::Crime | IncidentKind::Crash if since > CRIME_GIVE_UP => Some(Outcome::Unsolved),
                    IncidentKind::Medical if since > MEDICAL_GIVE_UP => Some(Outcome::Recovered),
                    _ => None,
                };
                if let Some(o) = give_up {
                    let k = inc.kind.slot();
                    let city_has = self.services.units.values().any(|u| u.kind == inc.kind.unit() && self.crewed_units(u.station) > 0);
                    let free = self.free_units()[k];
                    let why = &mut self.services.stats.unanswered_why_today[k];
                    match (city_has, free) {
                        (false, _) => why.no_station += 1,
                        (true, 0) => why.all_units_busy += 1,
                        _ => why.no_route += 1,
                    }
                    self.resolve(id, o);
                }
            }
            Stage::Dispatched if inc.outside && inc.arrive_at.is_some_and(|t| tick >= t) => self.on_scene(id),
            Stage::OnScene if inc.kind != IncidentKind::Fire => {
                let done = {
                    let i = self.services.incident_mut(id).unwrap();
                    i.work_left -= 1.0;
                    i.work_left <= 0.0
                };
                if done {
                    self.finish_scene(id);
                }
            }
            _ => {}
        }
    }

    // ── stadium nights and the daily books ─────────────────────────────────

    /// Midnight: upkeep, the schooling wage bonus, tonight's games, and the
    /// day's counters roll over. Runs before `daily_update` closes the books.
    pub(super) fn daily_services(&mut self) {
        self.repair_civic();
        let sites: Vec<(BuildingId, f32)> = self.buildings.iter().filter_map(|(id, b)| b.service.map(|s| (id, s.kind.upkeep_per_day(s.tier)))).collect();
        let upkeep: f32 = sites.iter().map(|(_, x)| x).sum();
        for (id, x) in sites {
            self.economy.debit_item(Flow::ServiceUpkeep, x, Item::Building(id));
        }
        // Workers living near a school earn more: night classes, a trade.
        let mut bonus = 0.0;
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            let c = &self.citizens[cid];
            let (Some(work), Some(home)) = (c.work, c.home) else { continue };
            let (Some(wb), Some(hb)) = (self.buildings.get(work), self.buildings.get(home)) else { continue };
            let cov = self.services.coverage_of(hb.lot, ServiceKind::School);
            let base = Self::base_income(wb, c.seed);
            let income = base * (1.0 + EDUCATION_BONUS * cov);
            bonus += income - base;
            self.citizens[cid].income = income;
        }
        let stadiums: Vec<(BuildingId, Tier, bool)> = self.buildings.iter().filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::Stadium).map(|s| (id, s.tier, b.operating()))).collect();
        let mut tonight = Vec::new();
        for (id, tier, open) in stadiums {
            // [under-construction] The draw is made for a stadium still
            // being built too (the dice stay in step); it hosts nothing.
            if self.srng().random_bool(EVENT_CHANCE[tier.index()]) && open {
                tonight.push(id);
            }
        }
        let s = &mut self.services;
        s.events_today = tonight;
        let st = &mut s.stats;
        st.avg_response_yesterday = if st.responses_today > 0 { st.response_ticks_today as f32 / st.responses_today as f32 } else { st.avg_response_yesterday };
        st.incidents_today = [0; 3];
        st.resolved_today = [0; 3];
        st.unanswered_today = 0;
        st.arrests_today = 0;
        st.response_ticks_today = 0;
        st.responses_today = 0;
        st.tickets_today = 0.0;
        st.pupils_today = 0;
        st.out_of_town_calls_today = 0;
        st.response_min_yesterday = std::mem::take(&mut st.response_min_today);
        st.saturated_min_yesterday = st.saturated_min_today;
        st.saturated_min_today = [0; 3];
        st.queue_max_today = st.queued;
        st.unanswered_why_today = Default::default();
        st.mutual_aid_today = 0;
        st.diverted_today = 0;
        st.chases_today = 0;
        st.second_alarms_today = 0;
        st.chases_tagged_today = 0;
        st.chases_lost_today = 0;
        st.unreachable_today = 0;
        st.out_of_town_fees_today = 0.0;
        st.bills_paid_today = 0.0;
        st.bills_covered_today = 0.0;
        st.stadium_crowd_today = 0;
        st.upkeep_today = upkeep;
        st.education_bonus_today = bonus;
    }
}

