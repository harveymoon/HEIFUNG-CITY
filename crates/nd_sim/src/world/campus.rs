//! [megacorp] Universities round their campus, their students, and schools
//! that families want to live near.
//!
//! *A campus* (`found_campus` when a University is placed) is an owner
//! record (`OwnerKind::University`) with a brand (its colour, a crest and its
//! initial). Daily (`daily_campuses`) it enrols the town's 18-24 year olds up
//! to its places, takes tuition, lets students arrive into free dorm rooms,
//! claims parks within [`CAMPUS_REACH`] as campus plaza, and grows by one
//! annex at a time (a [`DORM_SHARE`] of its places housed first, then
//! offices, a sports centre, a theatre and frat houses, up to
//! [`ANNEX_CAP`] by tier): it buys a block of homes nearby for a dorm, or
//! builds on the nearest vacant plot (bought and cleared if need be).
//!
//! *Students* go to class at 09:00 and stay till 15:00, and on a share of
//! evenings go out to a bar, a pizza place or the theatre near campus.
//! Pizza places, cafés (bakeries) and bookshops (gadget shops) near campus
//! see student walk-ins every open hour, and new shops and bars near a busy
//! campus open as those trades.
//!
//! *Schools and families*: a household with children picks a home a school
//! covers when one of its tier is free, and a family living out of reach of
//! any school moves into one now and then; land value near a school follows
//! how well it is staffed.
//!
//! Draws come from hashes and the estate's generators, never the world's.

use super::*;
use crate::corp::*;
use crate::estate::OwnerKind;
use crate::services::ServiceKind;
use slotmap::Key;

/// Share of a campus's places it houses in dorms before it builds anything
/// else.
pub const DORM_SHARE: f32 = 0.35;
/// School coverage from which a family counts a home as "near a school".
pub const FAMILY_SCHOOL_COVER: f32 = 0.3;
/// Chance a day a family out of reach of any school moves to one.
pub const FAMILY_MOVE_CHANCE: f32 = 0.12;

impl World {
    // ── founding ───────────────────────────────────────────────────────────

    /// A University was placed: its owner record and brand.
    pub(super) fn found_campus(&mut self, bid: BuildingId) {
        if self.corps.campuses.iter().any(|c| c.building == bid) {
            return;
        }
        let name = self.buildings[bid].name.clone();
        let owner = self.new_owner(name.clone(), OwnerKind::University { campus: bid });
        self.give_to(bid, owner, 0.0);
        let h = mix(self.config.seed ^ 0x0C0_11E6E, bid.data().as_ffi());
        let brand = {
            let others = self.corps.brands();
            new_brand(h, name, &[LogoShape::Shield], &others)
        };
        let day = self.clock.day() as u32;
        self.corps.campuses.push(Campus { building: bid, owner, brand, founded_day: day, annexes: Vec::new(), pending: Vec::new(), students: Vec::new(), funds: 0.0, expansions_total: 0 });
    }

    /// Whether `cid` is enrolled at a university.
    pub fn is_student(&self, cid: CitizenId) -> bool {
        self.corps.is_student(cid)
    }

    pub(super) fn campus_centre(&self, ki: usize) -> Option<Vec2> {
        self.building_lot(self.corps.campuses.get(ki)?.building).map(|l| l.center)
    }

    pub(super) fn campus_tier(&self, ki: usize) -> Tier {
        self.buildings.get(self.corps.campuses[ki].building).and_then(|b| b.service).map_or(Tier::Mid, |s| s.tier)
    }

    /// Places for students at the campus.
    pub fn campus_places(&self, ki: usize) -> usize {
        ServiceKind::University.capacity(self.campus_tier(ki)) as usize
    }

    /// Dorm rooms (homes in its dorms and frat houses).
    pub fn campus_dorm_homes(&self, ki: usize) -> u32 {
        self.corps.campuses[ki].annexes.iter().filter(|(_, a)| matches!(a, Annex::Dorm | Annex::Frat)).filter_map(|(b, _)| self.buildings.get(*b)).map(|b| b.planned_capacity().0 as u32).sum()
    }

    // ── the day ────────────────────────────────────────────────────────────

    /// Once a day: every campus enrols, takes tuition, houses arrivals,
    /// claims parks and maybe grows an annex.
    pub(super) fn daily_campuses(&mut self) {
        self.prune_campuses();
        for ki in 0..self.corps.campuses.len() {
            self.claim_pending(ki);
            self.enrol(ki);
            self.tuition(ki);
            self.dorm_arrivals(ki);
            self.claim_plazas(ki);
            let mut rng = self.erng(0xCA_4B05 + ki as u64);
            let day = self.clock.day() as u32;
            if day > self.corps.campuses[ki].founded_day && rng.random_bool(0.5) {
                self.grow_campus(ki);
            }
        }
    }

    /// Campuses whose main building is gone close; annexes and students
    /// that are gone leave the books.
    pub(super) fn prune_campuses(&mut self) {
        let buildings = &self.buildings;
        let citizens = &self.citizens;
        let lots = &self.lots;
        self.corps.campuses.retain(|c| buildings.get(c.building).is_some_and(|b| b.service.is_some_and(|s| s.kind == ServiceKind::University)));
        for c in &mut self.corps.campuses {
            c.annexes.retain(|(b, _)| buildings.get(*b).is_some_and(|x| x.plot.owner == Some(c.owner)));
            c.pending.retain(|(l, _)| lots.contains_key(*l));
            c.students.retain(|s| citizens.get(*s).is_some_and(|x| STUDENT_AGES.contains(&x.age)));
        }
    }

    /// A plot bought for an annex is clear: build it.
    pub(super) fn claim_pending(&mut self, ki: usize) {
        let pending = self.corps.campuses[ki].pending.clone();
        for (lot, annex) in pending {
            let Some(l) = self.lots.get(lot) else { continue };
            if l.building.is_some() {
                continue;
            }
            self.estate.reserved.remove(&lot);
            self.corps.campuses[ki].pending.retain(|(x, _)| *x != lot);
            self.build_annex(ki, lot, annex);
        }
    }

    /// Young adults in town fill the places, nearest the campus first; a
    /// hash of their seed decides who studies at all (about two in three).
    pub(super) fn enrol(&mut self, ki: usize) {
        let places = self.campus_places(ki);
        if self.corps.campuses[ki].students.len() >= places {
            return;
        }
        let Some(at) = self.campus_centre(ki) else { return };
        let mut cands: Vec<(f32, CitizenId)> = self
            .citizens
            .iter()
            .filter(|(id, c)| STUDENT_AGES.contains(&c.age) && c.home.is_some() && unit(mix(c.seed, 0x57_0D)) < 0.65 && !self.corps.is_student(*id))
            .map(|(id, c)| (c.home.and_then(|h| self.building_lot(h)).map_or(f32::MAX, |l| l.center.distance(at)), id))
            .collect();
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let room = places - self.corps.campuses[ki].students.len();
        let tick = self.clock.tick;
        let name = self.buildings[self.corps.campuses[ki].building].name.clone();
        for (_, cid) in cands.into_iter().take(room) {
            self.corps.campuses[ki].students.push(cid);
            if let Some(c) = self.citizens.get_mut(cid) {
                c.remember(tick, format!("Enrolled at {name}."));
            }
        }
    }

    /// Students pay their tuition from their pocket, when there is some.
    pub(super) fn tuition(&mut self, ki: usize) {
        let mut got = 0.0f64;
        for s in self.corps.campuses[ki].students.clone() {
            if let Some(c) = self.citizens.get_mut(s) {
                if c.savings >= TUITION {
                    c.savings -= TUITION;
                    got += TUITION as f64;
                }
            }
        }
        self.corps.campuses[ki].funds += got;
    }

    /// Free dorm rooms and room in the intake: students arrive from out of
    /// town to live in them, a few a day.
    pub(super) fn dorm_arrivals(&mut self, ki: usize) {
        let places = self.campus_places(ki);
        let k = &self.corps.campuses[ki];
        let mut free: Vec<BuildingId> = k.annexes.iter().filter(|(_, a)| matches!(a, Annex::Dorm | Annex::Frat)).map(|(b, _)| *b).filter(|b| self.buildings.get(*b).is_some_and(|x| x.operating() && x.free_homes() > 0)).collect();
        free.sort();
        let mut n = 0;
        while n < DORM_ARRIVALS_PER_DAY && self.corps.campuses[ki].students.len() < places {
            let Some(&dorm) = free.first() else { break };
            let Some(cid) = self.student_arrives(ki, dorm) else { break };
            self.corps.campuses[ki].students.push(cid);
            n += 1;
            if self.buildings[dorm].free_homes() == 0 {
                free.remove(0);
            }
        }
    }

    /// A student from out of town moves into a dorm room: a household of
    /// one, a little money, no car. Names and looks from hashes.
    pub(super) fn student_arrives(&mut self, ki: usize, dorm: BuildingId) -> Option<CitizenId> {
        let template = self.citizens.values().find(|c| c.stage() == crate::citizens::LifeStage::Adult)?.clone();
        let seed = self.fresh_seed();
        let mut rng = crate::rng_for(seed ^ 0x57_0D_E7, self.clock.tick);
        let family = names::family_name(&mut rng);
        let name = names::member_name(&mut rng, &family);
        let age = 18 + (mix(seed, 3) % 4) as u8;
        let pos = self.building_entrance(dorm).unwrap_or_default();
        let tick = self.clock.tick;
        let hid = self.households.insert(Household { members: Vec::new(), home: Some(dorm), money: 120.0, groceries: 2.0, rent_stress_days: 0, moved_day: self.clock.day() });
        let look = crate::looks::family_looks(&[(seed, age)], Tier::Low).into_iter().next();
        let campus = self.buildings[self.corps.campuses[ki].building].name.clone();
        let mut c = Citizen {
            name,
            seed,
            home: Some(dorm),
            work: None,
            pos,
            prev_pos: pos,
            heading: 0.0,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            owns_car: false,
            owns_bike: unit(mix(seed, 4)) < 0.5,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::Idle,
            memories: Vec::new(),
            places: Default::default(),
            thought: "First week on campus.".to_string(),
            spotlight: false,
            next_decision_tick: tick + 5,
            arrived_tick: tick,
            inside: None,
            mood: 0.75,
            income: GIG_INCOME * 0.6,
            savings: 60.0,
            rent_stress_days: 0,
            satiety: 0.8,
            groceries: 1.0,
            hungry_days: 0,
            trail: Vec::new(),
            pending_footfall: Vec::new(),
            household: Some(hid),
            age,
            car: None,
            to_car: None,
            look,
            ..template
        };
        c.memories.clear();
        c.remember(tick, format!("Came to Neon District to study at {campus}. Moved into the dorm."));
        let id = self.citizens.insert(c);
        self.households[hid].members.push(id);
        self.buildings[dorm].households.push(hid);
        self.buildings[dorm].residents.push(id);
        self.events.push(SimEvent::CitizenArrived(id));
        Some(id)
    }

    /// Parks within reach of the campus become campus plaza.
    pub(super) fn claim_plazas(&mut self, ki: usize) {
        let Some(at) = self.campus_centre(ki) else { return };
        let owner = self.corps.campuses[ki].owner;
        let parks: Vec<BuildingId> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Park && b.plot.owner.is_none() && b.operating())
            .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| l.center.distance(at) <= CAMPUS_REACH * 0.8))
            .map(|(id, _)| id)
            .collect();
        for p in parks.into_iter().take(1) {
            self.give_to(p, owner, 0.0);
            self.buildings[p].annex = Some(Annex::Plaza);
            self.corps.campuses[ki].annexes.push((p, Annex::Plaza));
            self.events.push(SimEvent::BuildingChanged(p));
            let (name, pos) = (self.buildings[self.corps.campuses[ki].building].name.clone(), self.lots[self.buildings[p].lot].center);
            let tick = self.clock.tick;
            self.social.push_news(tick, format!("🎓 {name} took over {} as a campus plaza.", self.buildings[p].name), pos, None);
        }
    }

    /// What the campus builds next, if anything: dorms until a share of its
    /// places is housed, then one of each kind of annex, then more dorms
    /// and frats.
    pub(super) fn next_annex(&self, ki: usize) -> Option<Annex> {
        let k = &self.corps.campuses[ki];
        let built = k.annexes.iter().filter(|(_, a)| *a != Annex::Plaza).count() + k.pending.len();
        if built >= ANNEX_CAP[self.campus_tier(ki).index()] {
            return None;
        }
        let places = self.campus_places(ki) as f32;
        // A first dorm, then one of each kind of annex, then dorms until a
        // share of the places is housed.
        if k.count(Annex::Dorm) == 0 {
            return Some(Annex::Dorm);
        }
        for a in [Annex::Admin, Annex::Sports, Annex::Theatre, Annex::Frat] {
            if k.count(a) == 0 {
                return Some(a);
            }
        }
        if (self.campus_dorm_homes(ki) as f32) < places * DORM_SHARE && k.count(Annex::Dorm) < 6 {
            return Some(Annex::Dorm);
        }
        if k.count(Annex::Frat) < 3 && k.count(Annex::Frat) * 2 < k.count(Annex::Dorm) {
            return Some(Annex::Frat);
        }
        ((self.campus_dorm_homes(ki) as f32) < places * 0.7).then_some(Annex::Dorm)
    }

    /// Grow by one annex: a dorm bought from the blocks of homes nearby, or
    /// a building on the nearest plot that takes it.
    pub(super) fn grow_campus(&mut self, ki: usize) {
        let Some(annex) = self.next_annex(ki) else { return };
        let Some(at) = self.campus_centre(ki) else { return };
        let owner = self.corps.campuses[ki].owner;
        let tick = self.clock.tick;
        let name = self.buildings[self.corps.campuses[ki].building].name.clone();
        // A block of homes nearby becomes a dorm as it stands.
        if annex == Annex::Dorm {
            let block = self
                .buildings
                .iter()
                .filter(|(id, b)| b.land_use == LandUse::Residential && b.plot.owner.is_none() && b.annex.is_none() && b.business.is_none() && b.homes >= 4 && b.homes <= 60 && b.plot.form.is_none() && self.buyable(*id))
                .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.center.distance(at), id)))
                .filter(|(d, _)| *d <= CAMPUS_REACH)
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            if let Some((_, bid)) = block {
                let price = self.sale_price(bid);
                self.give_to(bid, owner, price);
                let n = self.corps.campuses[ki].count(Annex::Dorm);
                let b = &mut self.buildings[bid];
                b.annex = Some(Annex::Dorm);
                b.name = Annex::Dorm.name(&name, n);
                self.corps.campuses[ki].annexes.push((bid, Annex::Dorm));
                self.corps.campuses[ki].funds -= (price * 0.01).min(self.corps.campuses[ki].funds.max(0.0));
                self.corps.campuses[ki].expansions_total += 1;
                self.corps.stats.campus_expansions_total += 1;
                self.events.push(SimEvent::BuildingChanged(bid));
                let pos = self.lots[self.buildings[bid].lot].center;
                let dorm = self.buildings[bid].name.clone();
                self.social.push_news(tick, format!("🎓 {name} bought a block of flats nearby for student housing: {dorm}."), pos, None);
                return;
            }
        }
        // Else the nearest plot that takes it: vacant first, then a low
        // building the market may sell (pulled down, then built on).
        let Some((lot, standing)) = self.annex_plot(ki, annex) else { return };
        match standing {
            None => {
                self.build_annex(ki, lot, annex);
            }
            Some(b) => {
                let memory = format!("{name} bought our building for its campus. We had to go.");
                self.pack_business(b, &format!("{name}'s campus"));
                self.demolish_with(b, None, Some(&memory));
                if self.lots.get(lot).is_some_and(|l| l.building.is_none()) {
                    self.build_annex(ki, lot, annex);
                } else {
                    self.estate.reserved.insert(lot, self.corps.campuses[ki].building);
                    self.corps.campuses[ki].pending.push((lot, annex));
                }
            }
        }
        self.corps.campuses[ki].expansions_total += 1;
        self.corps.stats.campus_expansions_total += 1;
    }

    /// The nearest plot within reach of the campus an annex can go on, and
    /// what stands there (`None`: vacant). Homes for dorms and frats; shops,
    /// nightlife or civic ground for the rest.
    pub(super) fn annex_plot(&self, ki: usize, annex: Annex) -> Option<(LotId, Option<BuildingId>)> {
        let at = self.campus_centre(ki)?;
        let min_area = match annex {
            Annex::Sports => 600.0,
            Annex::Theatre => 350.0,
            _ => 220.0,
        };
        let uses: &[LandUse] = match annex {
            Annex::Dorm | Annex::Frat => &[LandUse::Residential],
            _ => &[LandUse::Commercial, LandUse::Entertainment, LandUse::Civic],
        };
        let mut best: Option<(f32, LotId, Option<BuildingId>)> = None;
        for (lid, l) in self.lots.iter() {
            if !uses.contains(&l.land_use) || !l.usable() || l.landlocked || l.area() < min_area || self.estate.reserved.contains_key(&lid) || self.under_facility(l.center) || self.district_frozen(lid) || self.services.is_rubble(lid) {
                continue;
            }
            let d = l.center.distance(at);
            if d > CAMPUS_REACH {
                continue;
            }
            let (cost, standing) = match l.building {
                None => (d, None),
                Some(b) => {
                    let Some(x) = self.buildings.get(b) else { continue };
                    if !self.buyable(b) || x.plot.owner.is_some() || x.plot.merged() || x.floors > 4 || x.annex.is_some() {
                        continue;
                    }
                    // A standing building is worth it only twice as near.
                    (d * 2.0 + 40.0, Some(b))
                }
            };
            if best.is_none_or(|(c, bl, _)| cost < c || (cost == c && lid < bl)) {
                best = Some((cost, lid, standing));
            }
        }
        best.map(|(_, l, s)| (l, s))
    }

    /// Put up an annex on a clear plot: the campus's building of the kind,
    /// in the lot's land use, as a building site.
    pub(super) fn build_annex(&mut self, ki: usize, lot: LotId, annex: Annex) -> Option<BuildingId> {
        let l = self.lots.get(lot)?;
        if l.building.is_some() {
            return None;
        }
        let lu = l.land_use;
        let value = self.lot_value(lot);
        let cap = self.height_cap(lot).max(2);
        let (tier, floors) = match annex {
            Annex::Dorm => (Tier::Low, 6u8.min(cap.max(3))),
            Annex::Frat => (Tier::Mid, 2),
            Annex::Admin => (Tier::Mid, 4u8.min(cap.max(3))),
            Annex::Theatre => (Tier::Mid, 2),
            Annex::Sports => (Tier::Mid, 1),
            Annex::Plaza => return None,
        };
        let campus = self.buildings[self.corps.campuses[ki].building].name.clone();
        let n = self.corps.campuses[ki].count(annex);
        let seed = self.fresh_seed();
        let mut b = Building::new(lot, lu, floors, tier, seed, annex.name(&campus, n), self.clock.tick, value);
        b.annex = Some(annex);
        if !self.lots[lot].car_access() {
            b.parking_capacity = 0;
        }
        b.construction = Some(Construction::new(Construction::days_for(floors, tier)));
        b.recompute_capacity();
        b.update_rent(value);
        let id = self.buildings.insert(b);
        self.lots[lot].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        self.order_materials(id);
        let owner = self.corps.campuses[ki].owner;
        self.give_to(id, owner, 0.0);
        self.corps.campuses[ki].annexes.push((id, annex));
        let tick = self.clock.tick;
        let pos = self.lots[lot].center;
        self.social.push_news(tick, format!("🎓 {campus} is building {} ({}).", self.buildings[id].name, annex.label()), pos, None);
        Some(id)
    }

    /// [megacorp] Dev: the campus of `building` runs `days` of its daily
    /// round (enrolment, arrivals, plazas, one annex a day for sure) with
    /// its building sites finished. Returns the annexes it has.
    pub fn campus_grow_now(&mut self, building: BuildingId, days: u32) -> Result<usize, String> {
        let ki = self.corps.campuses.iter().position(|k| k.building == building).ok_or("not a university")?;
        for _ in 0..days.clamp(1, 60) {
            self.claim_pending(ki);
            self.enrol(ki);
            self.dorm_arrivals(ki);
            self.claim_plazas(ki);
            self.grow_campus(ki);
            let sites: Vec<BuildingId> = self.corps.campuses[ki].annexes.iter().map(|(b, _)| *b).filter(|b| self.buildings.get(*b).is_some_and(|x| x.construction.is_some())).collect();
            for s in sites {
                self.finish_construction(s);
            }
            // What stood on a bought plot comes down at once.
            let clearing: Vec<BuildingId> = self.corps.campuses[ki].pending.iter().filter_map(|(l, _)| self.lots.get(*l).and_then(|x| x.building)).collect();
            for b in clearing {
                self.finish_construction(b);
            }
            self.claim_pending(ki);
            let sites: Vec<BuildingId> = self.corps.campuses[ki].annexes.iter().map(|(b, _)| *b).filter(|b| self.buildings.get(*b).is_some_and(|x| x.construction.is_some())).collect();
            for s in sites {
                self.finish_construction(s);
            }
        }
        Ok(self.corps.campuses[ki].annexes.len())
    }

    // ── students through the day ───────────────────────────────────────────

    /// Every tick: students off to class at 09:00 and held there till
    /// 15:00; a share of them out for the evening at 20:30.
    pub(super) fn step_campus(&mut self) {
        if self.corps.campuses.is_empty() {
            return;
        }
        let minute = self.clock.minute_of_day();
        if minute == CLASS_MINUTE {
            self.send_students_to_class();
        }
        if minute == NIGHT_OUT_MINUTE {
            self.students_go_out();
        }
        let tick = self.clock.tick;
        if tick % 10 == 6 {
            let hour = self.clock.hour() as u8;
            if (9..CLASS_END_HOUR).contains(&hour) {
                for k in &self.corps.campuses {
                    for s in &k.students {
                        if let Some(c) = self.citizens.get_mut(*s) {
                            if matches!(c.activity, Activity::Visiting(b) if b == k.building) {
                                c.next_decision_tick = c.next_decision_tick.max(tick + 15);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn send_students_to_class(&mut self) {
        let tick = self.clock.tick;
        let list: Vec<(BuildingId, Vec<CitizenId>)> = self.corps.campuses.iter().filter(|k| self.buildings.get(k.building).is_some_and(|b| b.operating() && !b.is_derelict())).map(|k| (k.building, k.students.clone())).collect();
        for (campus, students) in list {
            let name = self.buildings[campus].name.clone();
            for cid in students {
                let Some(c) = self.citizens.get(cid) else { continue };
                if !matches!(c.activity, Activity::AtHome | Activity::Idle) || self.is_patient(cid) {
                    continue;
                }
                let c = &mut self.citizens[cid];
                c.asleep = false;
                c.thought = format!("Lectures at {name}.");
                self.corps.stats.students_to_class_today += 1;
                self.travel_to(cid, Some(campus), Purpose::Visit(LandUse::Civic));
            }
            let _ = tick;
        }
    }

    /// Places near a campus students go out to at night: bars and clubs,
    /// pizza places, the campus theatre, open now.
    pub(super) fn student_haunts(&self, at: Vec2, hour: u8) -> Vec<(f32, BuildingId, LandUse)> {
        let mut v: Vec<(f32, BuildingId, LandUse)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && !b.is_derelict() && b.opening_hours().contains(hour))
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| !z.closed && (z.kind.is_outing() || z.kind == BusinessKind::Bakery)) || b.annex == Some(Annex::Theatre))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.center.distance(at), id, b.land_use)))
            .filter(|(d, _, _)| *d <= STUDENT_TRADE_REACH)
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        v.truncate(6);
        v
    }

    pub(super) fn students_go_out(&mut self) {
        let day = self.clock.day();
        let hour = self.clock.hour() as u8;
        let tick = self.clock.tick;
        for ki in 0..self.corps.campuses.len() {
            let Some(at) = self.campus_centre(ki) else { continue };
            let haunts = self.student_haunts(at, hour);
            if haunts.is_empty() {
                continue;
            }
            for cid in self.corps.campuses[ki].students.clone() {
                let Some(c) = self.citizens.get(cid) else { continue };
                let h = mix(c.seed ^ 0x0_17E, day);
                if unit(h) >= NIGHT_OUT_SHARE || !matches!(c.activity, Activity::AtHome | Activity::Idle) || c.savings < 4.0 || self.is_patient(cid) {
                    continue;
                }
                let (_, venue, lu) = haunts[(mix(h, 1) % haunts.len() as u64) as usize];
                let name = self.buildings[venue].name.clone();
                let c = &mut self.citizens[cid];
                c.asleep = false;
                c.thought = format!("Night out at {name}.");
                c.remember(tick, format!("Went out with people from class to {name}."));
                self.corps.stats.students_out_today += 1;
                self.travel_to(cid, Some(venue), Purpose::Visit(lu));
            }
        }
    }

    /// Campuses and their students (count) for the hourly student trade:
    /// the gate and the head count.
    pub(super) fn campus_pull(&self) -> Vec<(Vec2, f32)> {
        self.corps.campuses.iter().filter_map(|k| self.building_lot(k.building).map(|l| (l.center, k.students.len() as f32))).filter(|(_, n)| *n > 0.0).collect()
    }

    /// Extra walk-in shoppers an hour at a shop at `p` from the campuses
    /// in `pull` (pizza places, cafés and bookshops).
    pub(super) fn student_feet(pull: &[(Vec2, f32)], p: Vec2) -> f32 {
        pull.iter().map(|(c, n)| n * STUDENT_FEET_PER_HOUR * (1.0 - p.distance(*c) / STUDENT_TRADE_REACH).max(0.0)).sum()
    }

    /// [megacorp] A new shop or bar near a busy campus opens as the trade
    /// students want: a pizza place on shop land, a bar on nightlife land
    /// (half the time, by a hash of the lot and the day).
    pub(super) fn campus_business(&self, lot_id: LotId, lu: LandUse) -> Option<BusinessKind> {
        if self.corps.campuses.is_empty() || !matches!(lu, LandUse::Commercial | LandUse::Entertainment) {
            return None;
        }
        let p = self.lots.get(lot_id)?.center;
        let near = self.corps.campuses.iter().filter(|k| k.students.len() >= 20).filter_map(|k| self.building_lot(k.building)).any(|l| l.center.distance(p) <= CAMPUS_REACH);
        if !near || unit(mix(lot_id.data().as_ffi(), self.clock.day())) >= 0.5 {
            return None;
        }
        Some(if lu == LandUse::Commercial { BusinessKind::Bakery } else { BusinessKind::Venue })
    }

    // ── schools and families ───────────────────────────────────────────────

    /// A household with children (anyone under 18).
    pub fn has_children(&self, hid: HouseholdId) -> bool {
        self.households.get(hid).is_some_and(|h| h.members.iter().any(|m| self.citizens.get(*m).is_some_and(|c| c.stage() == crate::citizens::LifeStage::Child)))
    }

    /// How well a school covers the home `bid` (`0..1`).
    pub fn school_cover(&self, bid: BuildingId) -> f32 {
        self.buildings.get(bid).map_or(0.0, |b| self.services.coverage_of(b.lot, ServiceKind::School))
    }

    /// Of `choices` (homes a household can afford, best tier first), the
    /// ones a family would take: those a school covers, if any are.
    pub(super) fn family_homes(&self, choices: Vec<BuildingId>) -> Vec<BuildingId> {
        let near: Vec<BuildingId> = choices.iter().copied().filter(|b| self.school_cover(*b) >= FAMILY_SCHOOL_COVER).collect();
        if near.is_empty() {
            choices
        } else {
            near
        }
    }

    /// A family living out of reach of any school moves to a home of the
    /// same tier or better that one covers, now and then (a hash of the
    /// household and the day). True if it moved.
    pub(super) fn family_school_move(&mut self, hid: HouseholdId, income: f32) -> bool {
        let Some(home) = self.households.get(hid).and_then(|h| h.home) else { return false };
        if !self.has_children(hid) || self.school_cover(home) >= FAMILY_SCHOOL_COVER || self.services.stats.coverage[3] <= 0.0 {
            return false;
        }
        if unit(mix(hid.data().as_ffi() ^ 0x5C_4001, self.clock.day())) >= FAMILY_MOVE_CHANCE {
            return false;
        }
        let tier = self.buildings.get(home).map_or(Tier::Low, |b| b.tier);
        let budget = income * trips::AFFORDABLE_SHARE;
        let best = self
            .buildings
            .iter()
            .filter(|(id, b)| *id != home && b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0 && b.annex.is_none() && b.tier >= tier && b.rent <= budget)
            .filter(|(id, _)| self.school_cover(*id) >= FAMILY_SCHOOL_COVER)
            .max_by(|a, b| self.school_cover(a.0).total_cmp(&self.school_cover(b.0)).then(b.0.cmp(&a.0)))
            .map(|(id, _)| id);
        let Some(to) = best else { return false };
        self.move_household(hid, to, "Moved to {} to be near a good school for the kids.");
        true
    }
}
