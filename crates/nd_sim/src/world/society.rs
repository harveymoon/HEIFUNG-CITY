//! [society] Where crime, homelessness and trash (`crate::crime`,
//! `crate::homeless`, `crate::trash`) plug into the tick, plus the few
//! private world systems they need, opened to the crate here so the
//! society files never reach into `world`'s internals directly.

use super::*;

impl World {
    /// The society systems' own random stream (see `Society::rng`).
    pub(crate) fn society_rng(&mut self) -> &mut SimRng {
        let (seed, tick) = (self.stream_seed(Stream::Society), self.clock.tick);
        self.society.rng.get_or_insert_with(|| crate::rng_for(seed, tick))
    }

    /// Hourly at :40, clear of the services' :50 roll.
    pub(super) fn hourly_society(&mut self) {
        if !self.config.society {
            return;
        }
        self.hourly_trash();
        self.hourly_homeless();
        self.hourly_crime();
    }

    /// Midnight, after the services' books and before the day's update.
    pub(super) fn daily_society(&mut self) {
        if !self.config.society {
            return;
        }
        self.daily_trash();
        self.daily_homeless();
        self.daily_crime();
    }

    /// Every tick: the trash trucks.
    pub(super) fn step_society(&mut self) {
        if !self.config.society {
            return;
        }
        self.step_trash();
    }

    /// A car reached the end of its route: true if it was a trash truck
    /// (handled here), false for anything else.
    pub(super) fn society_car_arrived(&mut self, car: CarId) -> bool {
        if self.society.trash.truck_of_car(car).is_none() {
            return false;
        }
        self.trash_car_arrived(car);
        true
    }

    /// Crime hot spots, camps and overflowing bins drag the land value down
    /// (called from the land value strip that handles lots).
    pub(super) fn splat_society(&self, b: &mut LandValueBuilder) {
        if !self.config.society {
            return;
        }
        self.splat_crime(b);
        self.splat_homeless(b);
        self.splat_trash(b);
    }

    // ── doors into private systems ─────────────────────────────────────────

    /// A citizen gone for good without leaving (killed): not an emigrant.
    pub(crate) fn society_remove_citizen(&mut self, cid: CitizenId) {
        self.remove_citizen(cid);
    }

    pub(crate) fn society_emigrate_household(&mut self, hid: HouseholdId, why: crate::economy::LeaveReason) {
        self.emigrate_household(hid, why);
    }

    pub(crate) fn society_move_household(&mut self, hid: HouseholdId, to: BuildingId, template: &str) {
        self.move_household(hid, to, template);
    }

    pub(crate) fn society_leave_building(&mut self, cid: CitizenId) -> Option<Vec2> {
        self.leave_building(cid)
    }

    pub(crate) fn society_city_leaves(&self) -> Vec<NodeId> {
        self.city_leaves()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crime::{recruit_chance, Offence, Pressure};
    use crate::homeless::SKID_ROW_LV;
    use crate::mind::IdleMind;
    use crate::services::{Outcome, ServiceKind};
    use crate::trash::{Hauler, TRUCK_CAPACITY};

    fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
    }

    /// The services tests' town: an avenue with civic land to the north and
    /// housing to the south, on flat ground.
    fn town(seed: u64) -> World {
        let mut w = World::new(WorldConfig { seed, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.economy.treasury = 1_000_000.0;
        w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-700.0, 120.0), Vec2::new(700.0, 120.0)], 1, 1);
        for x in [-600.0, -300.0, 0.0, 300.0, 600.0] {
            w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
        }
        w.add_zone(square(-690.0, 8.0, 1380.0, 50.0), LandUse::Civic);
        w.add_zone(square(-690.0, -60.0, 1380.0, 52.0), LandUse::Residential);
        w
    }

    /// Land value by position, for scenarios (holds until the next recompute).
    fn set_land_value(w: &mut World, f: impl Fn(Vec2) -> f32) {
        let n = w.land_value.n;
        for y in 0..n {
            for x in 0..n {
                let p = w.land_value.cell_center(x, y);
                w.land_value.values[y * n + x] = f(p);
            }
        }
        w.land_value.version += 1;
    }

    fn civic_lot(w: &World, x: f32) -> LotId {
        w.lots
            .iter()
            .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 500.0 && l.car_point.y.abs() < 12.0)
            .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
            .map(|(id, _)| id)
            .expect("a civic lot")
    }

    fn building_at(w: &mut World, x: f32) -> BuildingId {
        let lot = civic_lot(w, x);
        w.build_on(lot);
        let id = w.lots[lot].building.unwrap();
        w.finish_construction(id);
        id
    }

    /// Every residential lot built and filled to at least `n` people.
    fn populate(w: &mut World, n: usize) {
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
        for l in lots {
            w.build_on(l);
            if let Some(b) = w.lots[l].building {
                w.finish_construction(b);
            }
        }
        for _ in 0..200 {
            if w.citizens.len() >= n {
                break;
            }
            w.immigrate();
        }
        assert!(w.citizens.len() >= n, "only {} citizens", w.citizens.len());
    }

    fn staff(w: &mut World, b: BuildingId, who: &[CitizenId]) {
        for &c in who {
            if let Some(old) = w.citizens[c].work {
                w.buildings[old].workers.retain(|x| *x != c);
            }
            w.citizens[c].work = Some(b);
            w.buildings[b].workers.push(c);
        }
    }

    fn adults(w: &World) -> Vec<CitizenId> {
        w.citizens.iter().filter(|(_, c)| c.can_work()).map(|(id, _)| id).collect()
    }

    /// West of `x0` poor, jobless and unhappy; east of `x1` rich and employed.
    fn split_town(w: &mut World, x0: f32, x1: f32, job: BuildingId) {
        let ids: Vec<CitizenId> = w.citizens.keys().collect();
        for id in ids {
            let x = w.home_point(id).unwrap().x;
            if x < x0 {
                if let Some(j) = w.citizens[id].work.take() {
                    w.buildings[j].workers.retain(|k| *k != id);
                }
                let c = &mut w.citizens[id];
                c.income = 30.0;
                c.mood = 0.3;
            } else if x > x1 {
                let c = &mut w.citizens[id];
                c.income = 220.0;
                c.mood = 0.8;
                if c.can_work() && c.work.is_none() {
                    c.work = Some(job);
                }
            }
        }
    }

    #[test]
    fn the_pressure_on_a_poor_jobless_neighbourhood_far_outweighs_a_rich_covered_one() {
        let poor = Pressure { class: Tier::Low, jobless: true, homeless: false, hungry_days: 1, mood: 0.3, land_value: 0.2, police: 0.0, peers: 0, boldness: 1.0, age: 25 };
        let rich = Pressure { class: Tier::High, jobless: false, hungry_days: 0, mood: 0.8, land_value: 0.85, police: 1.0, ..poor };
        assert!(recruit_chance(&poor) > 0.02, "{}", recruit_chance(&poor));
        assert_eq!(recruit_chance(&rich), 0.0, "rich land adds nothing");
        // Contagion: offenders next door make it likelier, police coverage less.
        let peers = Pressure { peers: 3, ..poor };
        assert!(recruit_chance(&peers) > 2.0 * recruit_chance(&poor));
        let policed = Pressure { police: 1.0, ..peers };
        assert!(recruit_chance(&policed) < 0.3 * recruit_chance(&peers));
    }

    #[test]
    fn criminals_emerge_in_poor_uncovered_areas_and_not_in_rich_covered_ones() {
        let mut w = town(11);
        populate(&mut w, 260);
        let station = w.build_service(civic_lot(&w, 500.0), ServiceKind::Police, Tier::Mid).unwrap();
        set_land_value(&mut w, |p| if p.x < 0.0 { 0.18 } else { 0.85 });
        split_town(&mut w, -300.0, 300.0, station);
        let rich_home = w.citizens.keys().find(|c| w.home_point(*c).unwrap().x > 350.0).unwrap();
        assert!(w.crime_pressure(rich_home).unwrap().police > 0.9, "the east is policed");
        let poor_home = w.citizens.keys().find(|c| w.home_point(*c).unwrap().x < -350.0).unwrap();
        assert_eq!(w.crime_pressure(poor_home).unwrap().police, 0.0, "the west is not");
        for _ in 0..24 * 12 {
            w.recruit_offenders();
        }
        let (mut west, mut east) = (0, 0);
        for o in &w.society.crime.offenders {
            let x = w.home_point(o.citizen).unwrap().x;
            if x < -300.0 {
                west += 1;
            } else if x > 300.0 {
                east += 1;
            }
        }
        assert!(west >= 3, "crime grows out of the poor west: {west}");
        assert_eq!(east, 0, "and not out of the rich, policed east");
    }

    #[test]
    fn crime_spreads_through_an_unpoliced_poor_district_and_police_hold_it_back() {
        let run = |police: bool| {
            let mut w = town(5);
            populate(&mut w, 260);
            if police {
                for x in [-500.0, 0.0, 500.0] {
                    w.build_service(civic_lot(&w, x), ServiceKind::Police, Tier::High).unwrap();
                }
            }
            set_land_value(&mut w, |_| 0.2);
            let ids: Vec<CitizenId> = w.citizens.keys().collect();
            for id in ids {
                if let Some(j) = w.citizens[id].work.take() {
                    w.buildings[j].workers.retain(|k| *k != id);
                }
                let c = &mut w.citizens[id];
                c.income = 30.0;
                c.mood = 0.3;
            }
            // A handful of offenders in the far west.
            let seeds: Vec<CitizenId> = adults(&w).into_iter().filter(|c| w.home_point(*c).unwrap().x < -550.0).take(4).collect();
            for s in &seeds {
                w.turn_to_crime(*s);
            }
            for _ in 0..24 * 3 {
                w.recruit_offenders();
            }
            let near = w.society.crime.offenders.iter().filter(|o| !seeds.contains(&o.citizen) && w.home_point(o.citizen).unwrap().x < -450.0).count();
            (w.society.crime.offenders.len() - seeds.len(), near)
        };
        let (open, near) = run(false);
        let (held, _) = run(true);
        assert!(open > held, "without police more turn ({open} vs {held})");
        assert!(near >= 1, "some next to the first offenders");
    }

    /// An offence with the police (and maybe a jail) in town, run until the
    /// incident is over; the first seed with an arrest.
    fn arrest(with_jail: bool) -> (World, CitizenId) {
        for seed in 1..40 {
            let mut w = town(seed);
            populate(&mut w, 40);
            let folk = adults(&w);
            let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Police, Tier::Mid).unwrap();
            staff(&mut w, station, &folk[..4]);
            if with_jail {
                w.build_service(civic_lot(&w, -150.0), ServiceKind::Jail, Tier::Low).unwrap();
            }
            let shop = building_at(&mut w, 100.0);
            let thief = folk[8];
            w.turn_to_crime(thief);
            let id = w.offend(thief, shop, Offence::Vandalism).unwrap();
            let mut minds = IdleMind;
            for _ in 0..600 {
                w.tick(&mut minds);
                if let Some(i) = w.services.incident_mut(id) {
                    if i.stage == crate::services::Stage::OnScene {
                        i.arrived = Some(i.started);
                    }
                }
                if !w.services.incident(id).unwrap().active() {
                    break;
                }
            }
            if w.services.incident(id).unwrap().outcome == Some(Outcome::Arrest) {
                return (w, thief);
            }
        }
        panic!("no arrest in 40 seeds");
    }

    fn run_hours(w: &mut World, hours: u64) {
        let mut minds = IdleMind;
        for _ in 0..hours * 60 {
            w.tick(&mut minds);
        }
    }

    #[test]
    fn an_arrest_goes_to_jail_and_comes_out_or_is_let_go_early_without_one() {
        let (mut w, thief) = arrest(true);
        run_hours(&mut w, 2);
        let o = w.society.crime.offender(thief).expect("still on the books");
        assert!(o.in_custody && o.jailed && o.arrests == 1, "{o:?}");
        assert_eq!(w.society.crime.stats.jailed_total, 1);
        run_hours(&mut w, 24 * 5 + 2);
        // The sentence ran out: released once (out, they may well offend
        // again and be jailed afresh, which is a new sentence).
        let s = &w.society.crime.stats;
        assert!(s.released_total >= 1, "the sentence ran out: tick {} released {} arrests {}", w.clock.tick, s.released_total, s.arrests_total);
        assert_eq!(s.released_early_total, 0);
        // Out: gone straight, back on the street with a record, or back
        // inside for something new.
        assert!(s.reformed_total >= 1 || w.society.crime.offender(thief).is_some_and(|o| o.released_at.is_some()));

        let (mut w, thief) = arrest(false);
        run_hours(&mut w, 2);
        let o = w.society.crime.offender(thief).expect("held");
        assert!(o.in_custody && !o.jailed);
        // Let go within the hold (watched tick by tick: out, they may well
        // offend and be picked up again later, which is a new custody).
        let mut minds = IdleMind;
        let mut out = false;
        for _ in 0..(crate::services::HELD_HOURS + 2) * 60 {
            w.tick(&mut minds);
            if w.society.crime.stats.released_early_total > 0 {
                out = true;
                break;
            }
        }
        assert!(out, "let go after a few hours");
        assert_eq!(w.society.crime.stats.released_early_total, 1);
    }

    #[test]
    fn offences_cost_their_victims_and_scar_the_block() {
        let mut w = town(3);
        populate(&mut w, 60);
        let folk = adults(&w);
        let thief = folk[0];
        w.turn_to_crime(thief);
        let home = w.buildings.iter().find(|(id, b)| !b.households.is_empty() && Some(*id) != w.citizens[thief].home).map(|(id, _)| id).unwrap();
        for h in w.buildings[home].households.clone() {
            w.households[h].money = 400.0;
        }
        let before: f32 = w.buildings[home].households.iter().map(|h| w.households[*h].money).sum();
        w.offend(thief, home, Offence::Burglary).unwrap();
        let after: f32 = w.buildings[home].households.iter().map(|h| w.households[*h].money).sum();
        assert!(after < before, "money gone");
        let victims = w.buildings[home].residents.iter().filter(|r| w.citizens[**r].memories.iter().any(|m| m.text.contains("burgled"))).count();
        assert!(victims > 0, "the victims remember");
        let cond = w.buildings[home].condition;
        w.offend(thief, home, Offence::Vandalism).unwrap();
        assert!(w.buildings[home].condition < cond);
        assert_eq!(w.society.crime.offender(thief).unwrap().crimes, 2);
        let at = w.building_lot(home).unwrap().center;
        assert!(w.society.crime.heat_at(at, 100.0) > 1.0, "a hot spot");
        assert_eq!(w.services.active().filter(|i| i.kind == crate::IncidentKind::Crime).count(), 2, "both reported");
        // The hot spot takes land value off the block.
        let mut b = LandValueBuilder::new(&w.land_value);
        w.splat_society(&mut b);
        let mut lv = w.land_value.clone();
        // [pt-growth] A base under the compression knee, so it reads as is.
        b.finish(0.4, &mut lv);
        assert!(lv.at(at) < 0.38 && (lv.at(at + Vec2::new(400.0, 0.0)) - 0.4).abs() < 0.01, "{}", lv.at(at));
    }

    /// [pt-growth] Households put out on the same night drive round and pull
    /// up one by one, and a fresh cheap street fills a few at a time.
    #[test]
    fn rvs_arrive_one_by_one_and_do_not_pile_up_on_one_street() {
        let mut w = town(5);
        populate(&mut w, 80);
        set_land_value(&mut w, |_| 0.2);
        for c in w.citizens.values_mut() {
            c.owns_car = true;
        }
        let now = w.clock.tick;
        let hids: Vec<HouseholdId> = w.households.keys().collect();
        for hid in hids.into_iter().take(30) {
            w.force_homeless(hid);
        }
        let rvs = w.society.homeless.rvs.clone();
        assert!(rvs.len() >= 4, "some live in their vehicles: {}", rvs.len());
        let mut ticks: Vec<u64> = rvs.iter().map(|r| r.parked_tick).collect();
        assert!(ticks.iter().all(|t| *t > now), "none parked in the same second they lost their home");
        ticks.sort();
        ticks.dedup();
        assert!(ticks.len() * 2 >= rvs.len(), "they arrive at different times: {ticks:?}");
        assert!(rvs.iter().all(|r| !r.parked(now)), "not drawn yet");
        for (i, r) in rvs.iter().enumerate() {
            let near = rvs[..i].iter().filter(|o| o.pos.distance(r.pos) < crate::homeless::RV_CLUSTER_RADIUS).count();
            assert!(near < crate::homeless::RV_CLUSTER, "rv {i} pulled up beside {near} others");
        }
    }

    /// Make one household broke and stressed with nothing cheaper in town.
    fn ruin(w: &mut World, hid: HouseholdId) {
        for m in w.households[hid].members.clone() {
            w.citizens[m].income = if w.citizens[m].can_work() { 20.0 } else { 0.0 };
        }
        let income = w.household_income(hid);
        let ids: Vec<BuildingId> = w.buildings.keys().collect();
        for b in ids {
            w.buildings[b].rent = income.max(10.0) * 0.9;
        }
        let hh = &mut w.households[hid];
        hh.rent_stress_days = 5;
        hh.money = -5_000.0;
        hh.moved_day = u64::MAX;
    }


    /// [feat/pt-ux] A van left behind by a household that left town stands
    /// abandoned until the tow contractor's turn; a lived-in one the player
    /// orders towed goes within hours and its household is settled again.
    #[test]
    fn abandoned_vans_are_towed_and_ordered_tows_move_people_on() {
        use crate::homeless::{Rv, RV_ABANDONED_DAYS, RV_TOW_TICKS};
        let mut w = town(3);
        populate(&mut w, 30);
        let hids: Vec<HouseholdId> = w.households.keys().take(2).collect();
        let (gone, living) = (hids[0], hids[1]);
        // `gone` was on the street in its van and has left town.
        let rv = |household, seed, x: f32| Rv { household, pos: Vec2::new(x, 0.0), heading: 0.0, seed, camper: true, abandoned: None, tow_at: None, parked_tick: 0 };
        w.society.homeless.rvs.push(rv(gone, 3, -50.0));
        w.society.homeless.since.push((gone, 0));
        w.households.remove(gone);
        w.hourly_homeless();
        let r = w.society.homeless.rv(3).copied().expect("left at the kerb");
        assert!(r.abandoned.is_some() && !r.lived_in());
        assert!(w.society.homeless.rv_of(gone).is_none(), "nobody sleeps in it");
        // `living` is on the street, sleeping in theirs.
        w.households[living].home = None;
        w.society.homeless.rvs.push(rv(living, 4, 50.0));
        w.society.homeless.since.push((living, w.clock.tick));
        assert_eq!(w.society.homeless.bed_of(living), Some(Vec2::new(50.0, 0.0)));
        w.order_tow_rv(4).unwrap();
        assert!(w.order_tow_rv(4).is_err(), "already coming");
        w.clock.tick += RV_TOW_TICKS;
        w.hourly_homeless();
        assert!(w.society.homeless.rv(4).is_none(), "towed");
        assert!(w.society.homeless.rv(3).is_some(), "the abandoned one waits its turn");
        assert_ne!(w.society.homeless.bed_of(living), Some(Vec2::new(50.0, 0.0)));
        w.clock.tick += RV_ABANDONED_DAYS * 1440;
        w.hourly_homeless();
        assert!(w.society.homeless.rv(3).is_none(), "the contractor hauled it off");
    }
    /// [fix/buildings-ui] G16: a road drawn across a camp on a vacant lot
    /// moves the camp on at once; its people find a new spot, and no camp
    /// is left on a carriageway.
    #[test]
    fn a_road_over_a_camp_moves_it_on() {
        use crate::homeless::{camp_on_way, Camp};
        let mut w = town(5);
        populate(&mut w, 30);
        set_land_value(&mut w, |_| 0.2);
        let lot = civic_lot(&w, -450.0);
        let l = &w.lots[lot];
        assert!(l.building.is_none());
        let (pos, dir, out) = (l.center, (l.front_edge[1] - l.front_edge[0]).normalize_or_zero(), l.front_dir);
        let hid = w.households.keys().next().unwrap();
        w.households[hid].home = None;
        w.society.homeless.since.push((hid, w.clock.tick));
        w.society.homeless.camps.push(Camp { id: 99, pos, dir, out, lot: Some(lot), sidewalk: false, households: vec![hid], people: 1, founded: 0 });
        assert!(!camp_on_way(&w.roads, pos, dir, out));
        w.hourly_homeless();
        assert!(w.society.homeless.camps.iter().any(|c| c.id == 99), "the camp stands while its lot is vacant");
        // A street right through the camp.
        w.add_road(&[Vec2::new(pos.x, 0.0), Vec2::new(pos.x, 120.0)], 1, 1);
        w.implement();
        assert!(w.society.homeless.camps.iter().all(|c| c.id != 99), "the camp moved off the new street");
        assert!(w.society.homeless.camps.iter().all(|c| c.sidewalk || !camp_on_way(&w.roads, c.pos, c.dir, c.out)), "no tents on a carriageway");
        let bed = w.society.homeless.bed_of(hid);
        assert!(bed.is_none_or(|b| b.distance(pos) > 6.0), "its people pitched somewhere else: {bed:?}");
        assert!(bed.is_some() || w.society.homeless.stats.sleeping_rough > 0);
    }

    #[test]
    fn evicted_households_camp_on_cheap_ground_and_are_housed_again() {
        let mut w = town(4);
        populate(&mut w, 60);
        set_land_value(&mut w, |p| if p.x < 0.0 { 0.2 } else { 0.7 });
        // Evict households from the cheap west until one stays.
        let west: Vec<HouseholdId> = w.households.iter().filter(|(_, h)| h.home.is_some_and(|b| w.building_lot(b).unwrap().center.x < -100.0)).map(|(id, _)| id).collect();
        let mut stayed = None;
        for hid in west {
            ruin(&mut w, hid);
            w.daily_household(hid);
            if w.households.get(hid).is_some_and(|h| h.home.is_none()) {
                stayed = Some(hid);
                break;
            }
        }
        let hid = stayed.expect("someone stays on the street");
        w.hourly_homeless();
        assert!(w.is_homeless(w.households[hid].members[0]));
        let bed = w.society.homeless.bed_of(hid).expect("a tent or a van");
        assert!(w.land_value.at(bed) < SKID_ROW_LV, "skid row is on cheap land");
        assert!(w.society.homeless.camps.len() + w.society.homeless.rvs.len() >= 1 && w.homeless_people() >= 1);
        // Nobody camps in the dear east.
        assert!(w.society.homeless.camps.iter().all(|c| c.pos.x < 0.0));
        // Cheap homes come free: the daily search houses them and the camp clears.
        let income = w.household_income(hid);
        let cheap = w.buildings.iter().find(|(_, b)| b.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
        w.buildings[cheap].rent = (income * 0.2).max(0.5);
        w.buildings[cheap].homes = w.buildings[cheap].households.len() as u16 + 1;
        w.households[hid].moved_day = 0;
        w.daily_household(hid);
        assert_eq!(w.households[hid].home, Some(cheap));
        w.hourly_homeless();
        assert!(w.society.homeless.bed_of(hid).is_none(), "left the camp");
        assert_eq!(w.society.homeless.stats.rehoused_total, 1);
        assert!(!w.is_homeless(w.households[hid].members[0]));
    }

    #[test]
    fn with_no_cheap_ground_the_evicted_leave_town_as_before() {
        let mut w = town(4);
        populate(&mut w, 40);
        set_land_value(&mut w, |_| 0.8);
        let hid = w.households.keys().next().unwrap();
        ruin(&mut w, hid);
        w.daily_household(hid);
        assert!(!w.households.contains_key(hid), "out of town");
        assert_eq!(w.society.homeless.stats.evicted_total, 0);
    }

    #[test]
    fn trash_piles_up_without_service_and_the_neighbours_complain() {
        let mut w = town(8);
        populate(&mut w, 80);
        for _ in 0..24 * 6 {
            w.hourly_trash();
        }
        let used = w.society.trash.bins.values().filter(|b| b.daily > 0.0).count();
        let over = w.society.trash.bins.values().filter(|b| b.overflow() > 0.0).count();
        assert!(over * 2 > used, "most bins in use overflow after six days: {over} of {used}");
        assert_eq!(w.society.trash.stats.collected_city_today, 0.0);
        let moods: f32 = w.citizens.values().map(|c| c.mood).sum();
        w.daily_trash();
        assert!(w.society.trash.stats.complaints_yesterday > 0);
        assert!(w.citizens.values().map(|c| c.mood).sum::<f32>() < moods);
        assert!(w.citizens.values().any(|c| c.memories.iter().any(|m| m.text.contains("overflowing"))));
    }

    #[test]
    fn city_trucks_collect_within_their_capacity() {
        let mut w = town(9);
        populate(&mut w, 120);
        let crew = adults(&w);
        let depot = w.build_service(civic_lot(&w, 0.0), ServiceKind::TrashDepot, Tier::Mid).unwrap();
        staff(&mut w, depot, &crew[..8]);
        // A week's worth sitting in every bin.
        w.hourly_trash();
        for b in w.society.trash.bins.values_mut() {
            b.stored = b.daily.max(10.0) * 7.0;
        }
        let start: f32 = w.society.trash.stored();
        let over0 = w.society.trash.bins.values().filter(|b| b.overflow() > 0.0).count();
        let mut minds = IdleMind;
        let mut saw_truck = false;
        let mut max_trucks = 0;
        // From 07:00 to the end of the shift.
        for _ in 0..11 * 60 {
            w.tick(&mut minds);
            for t in w.society.trash.trucks.values() {
                assert!(t.load <= TRUCK_CAPACITY + 1.0, "over capacity");
            }
            max_trucks = max_trucks.max(w.society.trash.trucks.values().filter(|t| t.hauler == Hauler::City(depot)).count());
            saw_truck |= w.traffic.cars.values().any(|c| c.model.body == crate::vehicle::BodyType::GarbageTruck);
        }
        let s = &w.society.trash.stats;
        assert!(saw_truck, "garbage trucks drive the streets");
        assert_eq!(max_trucks, 4, "a middle-tier depot runs four trucks with eight crew");
        assert!(s.collected_city_today > 0.0 && s.stops_today >= 10, "{} kg at {} stops", s.collected_city_today, s.stops_today);
        assert!(w.society.trash.stored() < start);
        let over = w.society.trash.bins.values().filter(|b| b.overflow() > 0.0).count();
        assert!(over < over0, "{over} vs {over0}");
        // A few minutes a stop: no truck makes more than ~150 stops in a shift.
        for t in w.society.trash.trucks.values() {
            assert!(t.stops_today <= 150);
        }
    }

    #[test]
    fn the_private_hauler_serves_only_the_rich_where_the_city_does_not() {
        let mut w = town(12);
        populate(&mut w, 120);
        let rich: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.homes > 0).map(|(id, _)| id).take(4).collect();
        for &b in &rich {
            w.buildings[b].tier = Tier::High;
        }
        for h in w.households.values_mut() {
            h.money = 500.0;
        }
        w.hourly_trash();
        for (id, bin) in &w.society.trash.bins {
            assert_eq!(bin.private, w.rich_client(*id), "{id:?}");
        }
        assert!(rich.iter().all(|b| w.society.trash.bins[b].private), "the rich subscribe");
        let clients = w.society.trash.stats.private_clients as usize;
        assert!(clients >= 4 && clients < w.society.trash.bins.len() / 2, "{clients}");
        for b in w.society.trash.bins.values_mut() {
            b.stored = b.capacity * 1.5;
        }
        let poor_full: f32 = w.society.trash.bins.iter().filter(|(_, b)| !b.private).map(|(_, b)| b.stored).sum();
        let mut minds = IdleMind;
        for _ in 0..9 * 60 {
            w.tick(&mut minds);
        }
        assert!(w.society.trash.stats.collected_private_today > 0.0, "the hauler came");
        assert_eq!(w.society.trash.stats.collected_city_today, 0.0);
        let poor_now: f32 = w.society.trash.bins.iter().filter(|(_, b)| !b.private).map(|(_, b)| b.stored).sum();
        assert!(poor_now >= poor_full, "nobody empties the bins of the rest");
        // The fee comes out of the rich households at midnight.
        for _ in 0..16 * 60 {
            w.tick(&mut minds);
        }
        assert!(w.society.trash.stats.private_revenue_yesterday > 0.0);
        // A staffed depot covering them: the clients cancel.
        let crew = adults(&w);
        let what = crate::services::Placeable::Service(ServiceKind::TrashDepot, Tier::Mid);
        let lot = w.lots.iter().filter(|(id, l)| l.land_use == LandUse::Civic && w.placement_problem(what, *id).is_none()).min_by(|a, b| a.1.center.x.abs().total_cmp(&b.1.center.x.abs())).map(|(id, _)| id).expect("a lot for the depot");
        let depot = w.build_service(lot, ServiceKind::TrashDepot, Tier::Mid).unwrap();
        // [under-construction] It covers once what stood there is cleared.
        w.finish_demolition(depot);
        staff(&mut w, depot, &crew[..8]);
        w.hourly_trash();
        assert_eq!(w.society.trash.stats.private_clients, 0, "demand falls as the city covers");
    }

    fn fingerprint(w: &World) -> (usize, u32, usize, u32, u32, i64, usize) {
        let s = &w.society;
        (s.crime.offenders.len(), s.crime.offences_today() + s.crime.stats.offences_yesterday.iter().sum::<u32>(), s.homeless.camps.len(), s.homeless.stats.evicted_total, s.trash.stats.stops_today, s.trash.stored() as i64, w.citizens.len())
    }

    #[test]
    fn society_is_deterministic_and_survives_a_save() {
        let make = || {
            let mut w = town(21);
            populate(&mut w, 120);
            let crew = adults(&w);
            let depot = w.build_service(civic_lot(&w, 300.0), ServiceKind::TrashDepot, Tier::Low).unwrap();
            staff(&mut w, depot, &crew[..4]);
            set_land_value(&mut w, |p| if p.x < 0.0 { 0.2 } else { 0.6 });
            let ids: Vec<CitizenId> = adults(&w).into_iter().take(6).collect();
            for c in ids {
                w.turn_to_crime(c);
            }
            run_hours(&mut w, 36);
            w
        };
        let (a, b) = (make(), make());
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert!(a.society.trash.stats.collected_city_total > 0.0);
        let text = a.to_ron().unwrap();
        let back = World::from_ron(&text).unwrap();
        assert_eq!(back.society.trash.bins.len(), a.society.trash.bins.len());
        assert_eq!(back.society.crime.offenders.len(), a.society.crime.offenders.len());
        assert_eq!(back.society.trash.trucks.len(), a.society.trash.trucks.len());
    }
}
