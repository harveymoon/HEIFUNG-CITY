//! Homelessness and skid row. A household evicted for rent it cannot pay,
//! with no cheaper home in town and no money left, mostly stays on the
//! street instead of leaving (`World::become_homeless`, hooked into the
//! daily household round); so does anyone whose home was pulled down with
//! nowhere to go. They camp where land is cheapest: tents, tarps and
//! shopping carts on the pavement of run-down streets and on derelict
//! vacant lots, a few households to a camp; households with a car live in
//! it, an old van or RV parked at the kerb. Camps grow as people arrive and
//! shrink as they leave; they pull the land value around them down.
//!
//! Nothing harsh happens to them. They still earn gig money (or keep a
//! job), get a charity meal when hungry, and walk back to their camp for
//! the night. They are housed again by the ordinary daily search as soon as
//! something they can afford is free (so zoning cheap housing empties the
//! camps), and outreach from a nearby clinic or police post finds some of
//! them a place a little above what they could manage alone. After long
//! enough on the street some give up on the town.

use crate::citizens::{Activity, CitizenId, HouseholdId, Purpose};
use crate::land_value::LandValueBuilder;
use crate::services::ServiceKind;
use crate::zoning::{LandUse, LotId};
use crate::World;
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};

// ── balance ────────────────────────────────────────────────────────────────

/// Land value below which people camp on a street or a vacant lot (and the
/// renderer shows the camp).
pub const SKID_ROW_LV: f32 = 0.36;
/// Share of evicted households with nowhere to go who stay on the street
/// rather than leave town.
pub const STAY_SHARE: f32 = 0.7;
/// Households a camp holds; a newcomer joins one within `CAMP_REACH` of
/// where they lived, else starts one at least `CAMP_GAP` from the others.
pub const CAMP_CAP: usize = 6;
pub const CAMP_REACH: f32 = 260.0;
pub const CAMP_GAP: f32 = 24.0;
/// Chance a household with a car lives in it (an RV or van at the kerb).
pub const RV_SHARE: f32 = 0.75;
/// RVs park at least this far apart.
pub const RV_GAP: f32 = 10.0;
/// A charity meal brings a hungry homeless person back to this satiety.
pub const MEAL_SATIETY: f32 = 0.75;
pub const MEAL_COST: f32 = 3.0;
/// Daily chance outreach (a clinic or police post covering the camp) finds
/// a household a home with rent up to `OUTREACH_SHARE` of its income.
pub const OUTREACH_CHANCE: f32 = 0.25;
pub const OUTREACH_SHARE: f32 = 0.7;
/// Days on the street before some leave town, and the daily chance then.
pub const GIVE_UP_DAYS: u64 = 10;
pub const GIVE_UP_CHANCE: f32 = 0.12;
/// Land value a camp takes off its surroundings per tent (radius `CAMP_LV_RADIUS`).
pub const CAMP_LV: f32 = 0.012;
pub const CAMP_LV_RADIUS: f32 = 90.0;
/// Night: homeless people head back to their camp from this hour and stay
/// until `WAKE_HOUR`.
pub const NIGHT_HOUR: u64 = 21;
pub const WAKE_HOUR: u64 = 7;

/// A camp: tents on a pavement or a vacant lot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Camp {
    pub id: u32,
    pub pos: Vec2,
    /// Along the kerb (or the lot front), for laying the tents out.
    pub dir: Vec2,
    /// The vacant lot it is on, or the lot whose pavement it lines (None:
    /// an unzoned parcel).
    pub lot: Option<LotId>,
    /// Towards the street.
    #[serde(default)]
    pub out: Vec2,
    /// On a pavement (else on a vacant lot).
    pub sidewalk: bool,
    pub households: Vec<HouseholdId>,
    /// People living here, refreshed hourly.
    pub people: u16,
    pub founded: u64,
}

impl Camp {
    /// Tents for the people here (two to a tent, at most ten).
    pub fn tents(&self) -> u8 {
        (self.people as u32).div_ceil(2).clamp(1, 10) as u8
    }
}

/// A household living in its vehicle at the kerb.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rv {
    pub household: HouseholdId,
    pub pos: Vec2,
    pub heading: f32,
    pub seed: u64,
    /// A camper (true) or an old van.
    pub camper: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HomelessStats {
    pub evicted_today: u32,
    pub evicted_total: u32,
    pub rehoused_today: u32,
    pub rehoused_total: u32,
    pub outreach_total: u32,
    pub meals_today: u32,
    pub left_town_total: u32,
    /// Households with nowhere to camp (no cheap land in town).
    pub sleeping_rough: u32,
    /// Camps at the last three midnights, newest last.
    pub camps_history: Vec<u32>,
    pub people_history: Vec<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Homeless {
    pub camps: Vec<Camp>,
    pub rvs: Vec<Rv>,
    pub next_camp: u32,
    /// Households on the street and since when.
    pub since: Vec<(HouseholdId, u64)>,
    /// Where an evicted household lived, so it camps nearby.
    pub origin: Vec<(HouseholdId, Vec2)>,
    pub stats: HomelessStats,
}

impl Homeless {
    pub fn camp_of(&self, h: HouseholdId) -> Option<&Camp> {
        self.camps.iter().find(|c| c.households.contains(&h))
    }

    pub fn rv_of(&self, h: HouseholdId) -> Option<&Rv> {
        self.rvs.iter().find(|r| r.household == h)
    }

    /// Where a homeless household sleeps.
    pub fn bed_of(&self, h: HouseholdId) -> Option<Vec2> {
        self.rv_of(h).map(|r| r.pos).or_else(|| self.camp_of(h).map(|c| c.pos))
    }

    pub fn households(&self) -> usize {
        self.since.len()
    }

    pub fn camp_people(&self) -> u32 {
        self.camps.iter().map(|c| c.people as u32).sum()
    }

    /// Camps growing: more now than at the midnight two days ago.
    pub fn growing(&self) -> bool {
        let h = &self.stats.camps_history;
        h.len() >= 2 && (self.camps.len() as u32) > h[0] && self.camps.len() >= 2
    }
}

/// A place to camp.
struct Spot {
    pos: Vec2,
    dir: Vec2,
    out: Vec2,
    lot: Option<LotId>,
    sidewalk: bool,
}

impl World {
    /// Hook from the daily household round: an evicted household with no
    /// cheaper home and no money stays on the street (true) rather than
    /// leaving town (false: the caller emigrates it).
    pub fn become_homeless(&mut self, hid: HouseholdId) -> bool {
        if !self.config.society {
            return false;
        }
        let Some(hh) = self.households.get(hid) else { return false };
        let home = hh.home;
        let at = home.and_then(|h| self.building_lot(h)).map(|l| l.center);
        let Some(at) = at else { return false };
        // Nowhere cheap enough to camp: they leave as before.
        if self.camp_spots().is_empty() {
            return false;
        }
        if self.society_rng().random::<f32>() >= STAY_SHARE {
            return false;
        }
        self.evict(hid, "Evicted from {}. Nothing in town we can afford.");
        self.society.homeless.origin.retain(|o| o.0 != hid);
        self.society.homeless.origin.push((hid, at));
        self.society.homeless.stats.evicted_today += 1;
        self.society.homeless.stats.evicted_total += 1;
        self.settle_homeless(hid);
        true
    }

    /// Put a household on the street now, whatever its rent (the remote
    /// API's `society {action: "evict"}`, for trying skid row out). False if
    /// it had no home.
    pub fn force_homeless(&mut self, hid: HouseholdId) -> bool {
        let Some(at) = self.households.get(hid).and_then(|h| h.home).and_then(|b| self.building_lot(b)).map(|l| l.center) else { return false };
        self.evict(hid, "Evicted from {}. The landlord changed the locks.");
        self.society.homeless.origin.retain(|o| o.0 != hid);
        self.society.homeless.origin.push((hid, at));
        self.society.homeless.stats.evicted_today += 1;
        self.society.homeless.stats.evicted_total += 1;
        self.settle_homeless(hid);
        true
    }

    /// Put a household out of its home onto the street.
    pub fn evict(&mut self, hid: HouseholdId, template: &str) {
        let tick = self.clock.tick;
        let Some(hh) = self.households.get_mut(hid) else { return };
        let old = hh.home.take();
        hh.money = hh.money.max(0.0);
        hh.rent_stress_days = 0;
        let members = hh.members.clone();
        let name = old.and_then(|o| self.buildings.get(o)).map(|b| b.name.clone()).unwrap_or_default();
        if let Some(b) = old.and_then(|o| self.buildings.get_mut(o)) {
            b.households.retain(|&h| h != hid);
            b.residents.retain(|c| !members.contains(c));
        }
        for m in members {
            let at_home = self.citizens.get(m).is_some_and(|c| c.inside.is_some() && c.inside == old);
            if at_home {
                self.society_leave_building(m);
            }
            let Some(c) = self.citizens.get_mut(m) else { continue };
            c.home = None;
            c.rent_stress_days = 0;
            c.mood = (c.mood - 0.15).max(0.0);
            c.thought = "Everything we own fits in a cart now.".to_string();
            c.remember(tick, template.replace("{}", &name));
            self.concerns.heard(m, c, crate::concerns::Concern::Displacement, 0.8);
            if matches!(c.activity, Activity::AtHome) {
                c.activity = Activity::Idle;
            }
            c.next_decision_tick = c.next_decision_tick.min(tick + 5);
        }
    }

    /// Cheap pavements, vacant lots and unzoned rough parcels by a road,
    /// in lot then parcel order.
    fn camp_spots(&self) -> Vec<Spot> {
        let mut out = Vec::new();
        for (id, l) in self.lots.iter() {
            if !l.usable() || l.land_use == LandUse::Park || self.under_facility(l.center) {
                continue;
            }
            if self.land_value.at(l.center) >= SKID_ROW_LV {
                continue;
            }
            let along = (l.front_edge[1] - l.front_edge[0]).normalize_or_zero();
            match l.building {
                None if !self.services.is_rubble(id) => out.push(Spot { pos: l.center, dir: along, out: l.front_dir, lot: Some(id), sidewalk: false }),
                // Built lots lend their pavement, if it is on a street.
                Some(_) if l.car_access() => out.push(Spot { pos: l.entrance() + l.front_dir.normalize_or_zero() * 1.5, dir: along, out: l.front_dir, lot: Some(id), sidewalk: true }),
                _ => {}
            }
        }
        // Unzoned land along a road nobody has built on.
        for p in &self.parcels.parcels {
            if p.land_use.is_some() || p.lot.is_some() || p.rough || !p.path_frontage || p.area < 60.0 {
                continue;
            }
            if self.land_value.at(p.center) >= SKID_ROW_LV || self.under_facility(p.center) {
                continue;
            }
            let along = (p.front_edge[1] - p.front_edge[0]).normalize_or_zero();
            let mid = (p.front_edge[0] + p.front_edge[1]) * 0.5;
            out.push(Spot { pos: p.center, dir: along, out: (mid - p.center).normalize_or_zero(), lot: None, sidewalk: false });
        }
        out
    }

    /// A kerb spot on a cheap street for an RV, near `near`.
    fn rv_spot(&self, near: Vec2) -> Option<(Vec2, f32)> {
        let mut best: Option<(f32, Vec2, f32)> = None;
        for l in self.lots.values() {
            if !l.usable() || !l.car_access() || self.land_value.at(l.center) >= SKID_ROW_LV {
                continue;
            }
            let Some(seg) = self.roads.try_segment(l.front_segment) else { continue };
            if !seg.is_street() || seg.fronts_nothing() {
                continue;
            }
            let (a, b) = self.roads.segment_points(l.front_segment);
            let ab = b - a;
            let t = ((l.entrance() - a).dot(ab) / ab.length_squared().max(1e-3)).clamp(0.15, 0.85);
            let q = a + ab * t;
            let side = (l.entrance() - q).normalize_or_zero();
            let p = q + side * (seg.half_width() - crate::roads::SHOULDER * 0.6);
            if self.society.homeless.rvs.iter().any(|r| r.pos.distance(p) < RV_GAP) {
                continue;
            }
            // Parked with the traffic on that side (right-hand traffic).
            let dir = ab.normalize_or_zero();
            let right = Vec2::new(dir.y, -dir.x);
            let heading_dir = if right.dot(side) > 0.0 { dir } else { -dir };
            let d = p.distance(near);
            if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
                best = Some((d, p, heading_dir.y.atan2(heading_dir.x)));
            }
        }
        best.map(|(_, p, h)| (p, h))
    }

    /// Find a homeless household a place: its car, a camp nearby, or a new
    /// camp on the nearest cheap spot.
    fn settle_homeless(&mut self, hid: HouseholdId) {
        let tick = self.clock.tick;
        if self.society.homeless.bed_of(hid).is_some() {
            return;
        }
        let Some(hh) = self.households.get(hid) else { return };
        let members = hh.members.clone();
        let origin = self.society.homeless.origin.iter().find(|o| o.0 == hid).map(|o| o.1).or_else(|| members.first().and_then(|m| self.citizens.get(*m)).map(|c| c.pos)).unwrap_or(Vec2::ZERO);
        if !self.society.homeless.since.iter().any(|s| s.0 == hid) {
            self.society.homeless.since.push((hid, tick));
        }
        let has_car = members.iter().filter_map(|m| self.citizens.get(*m)).any(|c| c.owns_car);
        if has_car && self.society_rng().random::<f32>() < RV_SHARE {
            if let Some((pos, heading)) = self.rv_spot(origin) {
                let seed = self.society_rng().random::<u64>();
                let camper = self.society_rng().random_bool(0.55);
                self.society.homeless.rvs.push(Rv { household: hid, pos, heading, seed, camper });
                for m in &members {
                    if let Some(c) = self.citizens.get_mut(*m) {
                        c.remember(tick, if camper { "Living in the camper now. Parked where nobody moves us on." } else { "Sleeping in the van. Could be worse." });
                    }
                }
                return;
            }
        }
        let join = self.society.homeless.camps.iter().filter(|c| c.households.len() < CAMP_CAP && c.pos.distance(origin) < CAMP_REACH).min_by(|a, b| a.pos.distance(origin).total_cmp(&b.pos.distance(origin))).map(|c| c.id);
        let id = match join {
            Some(id) => Some(id),
            None => {
                let camps = &self.society.homeless.camps;
                let spot = self.camp_spots().into_iter().filter(|s| camps.iter().all(|c| c.pos.distance(s.pos) >= CAMP_GAP)).min_by(|a, b| a.pos.distance(origin).total_cmp(&b.pos.distance(origin)));
                spot.map(|s| {
                    let h = &mut self.society.homeless;
                    let id = h.next_camp;
                    h.next_camp += 1;
                    h.camps.push(Camp { id, pos: s.pos, dir: s.dir, out: s.out, lot: s.lot, sidewalk: s.sidewalk, households: Vec::new(), people: 0, founded: tick });
                    id
                })
            }
        };
        match id.and_then(|id| self.society.homeless.camps.iter_mut().find(|c| c.id == id)) {
            Some(camp) => {
                camp.households.push(hid);
                camp.people += members.len() as u16;
                let pos = camp.pos;
                for m in &members {
                    if let Some(c) = self.citizens.get_mut(*m) {
                        c.remember(tick, "Pitched a tent with the others. Safer in a group.");
                    }
                }
                let _ = pos;
            }
            None => self.society.homeless.stats.sleeping_rough += 1,
        }
    }

    // ── the hourly round ───────────────────────────────────────────────────

    pub(crate) fn hourly_homeless(&mut self) {
        crate::prof_scope!("hourly_homeless");
        let tick = self.clock.tick;
        let hour = self.clock.hour();
        // Who is on the street: every household without a home that has
        // arrived (immigrants always have one).
        let homeless: Vec<HouseholdId> = self.households.iter().filter(|(_, h)| h.home.is_none() && !h.members.is_empty()).map(|(id, _)| id).collect();
        // Housed again (or gone): off the books, out of the camp.
        let h = &mut self.society.homeless;
        let before: Vec<HouseholdId> = h.since.iter().map(|s| s.0).collect();
        let mut rehoused = 0;
        // [perf] A set for the lookups below: `Vec::contains` made this
        // homeless x homeless every hour.
        let on_street: std::collections::HashSet<HouseholdId> = homeless.iter().copied().collect();
        for hid in before {
            if on_street.contains(&hid) {
                continue;
            }
            h.since.retain(|s| s.0 != hid);
            h.origin.retain(|o| o.0 != hid);
            h.rvs.retain(|r| r.household != hid);
            for c in h.camps.iter_mut() {
                c.households.retain(|x| *x != hid);
            }
            if self.households.get(hid).is_some_and(|x| x.home.is_some()) {
                rehoused += 1;
                for m in self.households[hid].members.clone() {
                    if let Some(c) = self.citizens.get_mut(m) {
                        c.remember(tick, "A roof again. Slept for twelve hours straight.");
                        c.mood = (c.mood + 0.15).min(1.0);
                    }
                }
            }
        }
        let h = &mut self.society.homeless;
        h.stats.rehoused_today += rehoused;
        h.stats.rehoused_total += rehoused;
        h.stats.sleeping_rough = 0;
        for hid in &homeless {
            self.settle_homeless(*hid);
        }
        // Camps on land that got dear or was built on move on, quietly.
        let lv = &self.land_value;
        let lots = &self.lots;
        let h = &mut self.society.homeless;
        let mut displaced: Vec<HouseholdId> = Vec::new();
        h.camps.retain(|c| {
            let ground = match c.lot {
                Some(l) => lots.get(l).is_some_and(|l| c.sidewalk || l.building.is_none()),
                // An unzoned parcel: until a lot is laid over it.
                None => !lots.values().any(|l| l.contains(c.pos)),
            };
            let ok = lv.at(c.pos) < SKID_ROW_LV + 0.08 && ground;
            if !ok {
                displaced.extend(c.households.iter().copied());
            }
            ok && !c.households.is_empty()
        });
        for hid in displaced {
            self.settle_homeless(hid);
        }
        // People per camp.
        let counts: Vec<u16> = self.society.homeless.camps.iter().map(|c| c.households.iter().filter_map(|h| self.households.get(*h)).map(|h| h.members.len() as u16).sum()).collect();
        for (c, n) in self.society.homeless.camps.iter_mut().zip(counts) {
            c.people = n;
        }
        // Fed, and home to the camp for the night.
        for hid in homeless {
            let Some(bed) = self.society.homeless.bed_of(hid) else { continue };
            let members = self.households.get(hid).map(|h| h.members.clone()).unwrap_or_default();
            for m in members {
                let Some(c) = self.citizens.get(m) else { continue };
                if c.satiety < 0.3 {
                    let paid = self.households.get_mut(hid).map(|h| {
                        let p = h.money.clamp(0.0, MEAL_COST);
                        h.money -= p;
                        p
                    });
                    let _ = paid;
                    let c = &mut self.citizens[m];
                    c.satiety = c.satiety.max(MEAL_SATIETY);
                    c.hungry_days = 0;
                    if self.society.homeless.stats.meals_today % 7 == 0 {
                        c.remember(tick, "Hot soup at the mission van. Good people.");
                    }
                    self.society.homeless.stats.meals_today += 1;
                }
                let c = &self.citizens[m];
                let night = hour >= NIGHT_HOUR || hour < WAKE_HOUR;
                if !night || self.services.in_custody(m) || self.is_patient(m) {
                    continue;
                }
                let free = matches!(c.activity, Activity::Idle | Activity::Traveling { to: None, purpose: Purpose::Wander, .. }) && c.inside.is_none();
                if !free {
                    continue;
                }
                if c.pos.distance(bed) < 12.0 {
                    // Bedded down until morning.
                    let wake = tick + ((WAKE_HOUR + 24 - hour) % 24) * 60;
                    let c = &mut self.citizens[m];
                    c.activity = Activity::Idle;
                    c.next_decision_tick = c.next_decision_tick.max(wake);
                    continue;
                }
                // Walk back to the camp along the pavements.
                let here = c.pos;
                let k = slotmap::Key::data(&m).as_ffi();
                let jitter = Vec2::new(((k % 7) as f32 - 3.0) * 0.9, ((k / 7 % 5) as f32 - 2.0) * 0.9);
                if let Some((mut path, _)) = self.walk.route(here, bed + jitter) {
                    path.push(bed + jitter);
                    let c = &mut self.citizens[m];
                    c.mode = crate::citizens::TravelMode::Walk;
                    c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
                    c.next_decision_tick = c.next_decision_tick.max(tick + 90);
                } else {
                    let c = &mut self.citizens[m];
                    c.pos = bed + jitter;
                    c.prev_pos = c.pos;
                    c.activity = Activity::Idle;
                }
            }
        }
    }

    // ── midnight ───────────────────────────────────────────────────────────

    pub(crate) fn daily_homeless(&mut self) {
        crate::prof_scope!("daily_homeless");
        let tick = self.clock.tick;
        let day = tick / 1440;
        let list: Vec<(HouseholdId, u64)> = self.society.homeless.since.clone();
        for (hid, since) in list {
            let Some(hh) = self.households.get(hid) else { continue };
            if hh.home.is_some() {
                continue;
            }
            let members = hh.members.clone();
            let income: f32 = members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum();
            // Outreach from a clinic or a police post that covers the camp.
            let bed = self.society.homeless.bed_of(hid);
            let reach = bed.map_or(0.0, |p| self.coverage_at(p, ServiceKind::Health).max(self.coverage_at(p, ServiceKind::Police)));
            if reach >= 0.5 && self.society_rng().random::<f32>() < OUTREACH_CHANCE {
                let home = self.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Residential && b.construction.is_none() && b.free_homes() > 0 && b.rent <= income * OUTREACH_SHARE).min_by(|a, b| a.1.rent.total_cmp(&b.1.rent)).map(|(id, _)| id);
                if let Some(home) = home {
                    self.society_move_household(hid, home, "An outreach worker found us a place at {}.");
                    self.society.homeless.stats.outreach_total += 1;
                    continue;
                }
            }
            // Long enough on the street and some give up on the town.
            if day.saturating_sub(since / 1440) >= GIVE_UP_DAYS && self.society_rng().random::<f32>() < GIVE_UP_CHANCE {
                for m in &members {
                    if let Some(c) = self.citizens.get_mut(*m) {
                        c.remember(tick, "Nothing for us here. Hitching out.");
                        self.concerns.heard(*m, c, crate::concerns::Concern::Housing, 0.8);
                    }
                }
                self.society.homeless.stats.left_town_total += members.len() as u32;
                self.society_emigrate_household(hid);
            }
        }
        let camps = self.society.homeless.camps.len() as u32;
        let people = self.homeless_people() as u32;
        let s = &mut self.society.homeless.stats;
        s.camps_history.push(camps);
        if s.camps_history.len() > 3 {
            s.camps_history.remove(0);
        }
        s.people_history.push(people);
        if s.people_history.len() > 3 {
            s.people_history.remove(0);
        }
        s.evicted_today = 0;
        s.rehoused_today = 0;
        s.meals_today = 0;
    }

    /// People living on the street (in camps, in vehicles or rough).
    pub fn homeless_people(&self) -> usize {
        self.society.homeless.since.iter().filter_map(|(h, _)| self.households.get(*h)).map(|h| h.members.len()).sum()
    }

    /// Is this citizen living on the street?
    pub fn is_homeless(&self, c: CitizenId) -> bool {
        self.citizens.get(c).and_then(|c| c.household).is_some_and(|h| self.society.homeless.since.iter().any(|s| s.0 == h))
    }

    pub(crate) fn splat_homeless(&self, b: &mut LandValueBuilder) {
        for c in &self.society.homeless.camps {
            b.splat(c.pos, CAMP_LV_RADIUS, -CAMP_LV * c.tents() as f32);
        }
        for r in &self.society.homeless.rvs {
            b.splat(r.pos, 50.0, -CAMP_LV);
        }
    }
}
