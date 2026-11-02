//! [parks] (D5) The parks in the world: one state (and one name) for park
//! lots side by side, the litter, campers and dealers that run a park
//! down, the paid clean-up, nature preserves, and what a run-down park does
//! to the land round it and to who goes there. Data in `crate::parks`.

use super::*;
use crate::parks::{self as pk, ParkState};
use crate::services::ServiceKind;
use crate::buildings::Construction;
use std::collections::BTreeMap;

impl World {
    /// Park lots the city keeps as parks (not beaches, not a landmark's
    /// grounds), with their open park building.
    fn park_buildings(&self) -> Vec<(BuildingId, LotId)> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Park && b.service.is_none() && b.operating())
            .filter(|(id, b)| self.lots.get(b.lot).is_some_and(|l| !l.shore.beach && l.land_use == LandUse::Park && l.building == Some(*id) && l.polygon.len() >= 3))
            .map(|(id, b)| (id, b.lot))
            .collect()
    }

    /// The lots of a park (its state's, or the holder's own lot).
    pub fn park_lots(&self, anchor: BuildingId) -> Vec<LotId> {
        match self.parks.parks.get(&anchor) {
            Some(s) if !s.lots.is_empty() => s.lots.iter().copied().filter(|l| self.lots.contains_key(*l)).collect(),
            _ => self.buildings.get(anchor).map(|b| b.lot).into_iter().collect(),
        }
    }

    /// The parks as they stand: [r8-parks] the park lots of one layout
    /// cluster (`ParkLayout::cluster`, the ground `park::plan_parks` lays
    /// out as one) are one park, held by the building of the biggest park
    /// it took in (else its oldest), so a park keeps its name as lots join
    /// it; every lot of it is named alike, sites going up on it too. Gone
    /// parks are dropped.
    pub(super) fn refresh_parks(&mut self) {
        let now = self.park_buildings();
        let mut by_key: BTreeMap<LotId, Vec<(BuildingId, LotId)>> = BTreeMap::new();
        for (b, l) in &now {
            by_key.entry(self.park_key(*l)).or_default().push((*b, *l));
        }
        let mut anchors: Vec<(BuildingId, Vec<BuildingId>, Vec<LotId>, LotId)> = Vec::new();
        for (key, group) in by_key {
            let mut members: Vec<BuildingId> = group.iter().map(|x| x.0).collect();
            members.sort();
            let mut lots: Vec<LotId> = group.iter().map(|x| x.1).collect();
            lots.sort();
            let held = |b: &BuildingId| self.parks.parks.get(b).map_or(0.0, |s| s.area);
            let anchor = members
                .iter()
                .copied()
                .filter(|b| self.parks.parks.contains_key(b))
                .max_by(|a, b| held(a).total_cmp(&held(b)).then((self.buildings[*b].built_tick, *b).cmp(&(self.buildings[*a].built_tick, *a))))
                .or_else(|| members.iter().copied().min_by_key(|b| (self.buildings[*b].built_tick, *b)))
                .unwrap_or(members[0]);
            let others: Vec<BuildingId> = members.into_iter().filter(|b| *b != anchor).collect();
            anchors.push((anchor, others, lots, key));
        }
        let keep: std::collections::HashSet<BuildingId> = anchors.iter().map(|x| x.0).collect();
        // A park that merged into another hands its troubles on.
        let gone: Vec<(BuildingId, ParkState)> = self.parks.parks.iter().filter(|(b, _)| !keep.contains(b)).map(|(b, s)| (*b, s.clone())).collect();
        self.parks.parks.retain(|b, _| keep.contains(b));
        let mut names: HashMap<LotId, String> = HashMap::new();
        for (b, others, lots, key) in anchors {
            let preserve = lots.iter().any(|l| self.lots[*l].park.as_ref().is_some_and(|p| p.preserve));
            if !self.parks.parks.contains_key(&b) {
                let name = self.buildings[b].name.clone();
                let mut s = ParkState { name, ..Default::default() };
                // Merged with an old park: take on its litter and dealers.
                if let Some((_, old)) = gone.iter().find(|(g, _)| others.contains(g)) {
                    s.trash = old.trash;
                    s.dealers = old.dealers;
                    s.kept = old.kept;
                    s.cleaned = old.cleaned;
                    s.cleanups = old.cleanups;
                }
                self.parks.parks.insert(b, s);
            }
            let area: f32 = lots.iter().map(|l| self.lots[*l].area()).sum();
            let s = self.parks.parks.get_mut(&b).unwrap();
            s.preserve = preserve;
            if s.members != others || s.lots != lots {
                s.members = others.clone();
                s.lots = lots;
                s.area = area;
            }
            names.insert(key, s.name.clone());
            if self.buildings[b].name != s.name {
                let n = s.name.clone();
                self.buildings[b].name = n;
                self.events.push(SimEvent::BuildingChanged(b));
            }
        }
        // Every park building on a park's ground carries its name, the
        // sites of its lots still going up too.
        let renames: Vec<(BuildingId, String)> = self
            .lots
            .iter()
            .filter(|(_, l)| l.land_use == LandUse::Park && !l.shore.beach)
            .filter_map(|(id, l)| {
                let name = names.get(&self.park_key(id))?;
                let b = l.building?;
                let bld = self.buildings.get(b)?;
                (bld.land_use == LandUse::Park && bld.service.is_none() && bld.lot == id && bld.name != *name).then(|| (b, name.clone()))
            })
            .collect();
        for (b, name) in renames {
            self.buildings[b].name = name;
            self.events.push(SimEvent::BuildingChanged(b));
        }
        self.parks.reindex();
    }

    /// [r8-parks] The park a park lot is part of: the key every lot of its
    /// layout cluster carries (the lot itself when it has no plan).
    pub fn park_key(&self, lot: LotId) -> LotId {
        self.lots.get(lot).and_then(|l| l.park.as_ref()).and_then(|p| p.cluster).unwrap_or(lot)
    }

    /// [r8-parks] What the park `lot` is part of holds over all its lots:
    /// its features (counted, "2 playgrounds") and its car park's bays.
    pub fn park_contents(&self, lot: LotId) -> (Vec<String>, usize) {
        let mut counts: Vec<(crate::park::ParkFeature, usize)> = Vec::new();
        let mut bays = 0;
        for l in self.park_cluster_lots(lot) {
            let Some(p) = self.lots[l].park.as_ref() else { continue };
            bays += p.car_park.as_ref().filter(|c| c.own).map_or(0, |c| c.spaces.len());
            for f in &p.features {
                match counts.iter_mut().find(|(k, _)| *k == f.kind) {
                    Some(c) => c.1 += 1,
                    None => counts.push((f.kind, 1)),
                }
            }
        }
        let words = counts.into_iter().map(|(k, n)| if n == 1 { k.label().to_string() } else { format!("{n} {}s", k.label()) }).collect();
        (words, bays)
    }

    /// [r8-parks] Every lot of the park `lot` is part of, built or not.
    pub fn park_cluster_lots(&self, lot: LotId) -> Vec<LotId> {
        let key = self.park_key(lot);
        let mut v: Vec<LotId> = self.lots.iter().filter(|(id, l)| l.land_use == LandUse::Park && !l.shore.beach && (*id == lot || l.park.as_ref().and_then(|p| p.cluster) == Some(key))).map(|(id, _)| id).collect();
        v.sort();
        v
    }

    /// [r8-parks] The park's building sites on the park `lot` is part of:
    /// its lots' shares (`followers`) or its lead sites.
    pub(super) fn park_sites(&self, lot: LotId, followers: bool) -> Vec<BuildingId> {
        self.park_cluster_lots(lot)
            .into_iter()
            .filter_map(|l| self.lots[l].building)
            .filter(|b| self.buildings.get(*b).is_some_and(|x| x.land_use == LandUse::Park && x.construction.as_ref().is_some_and(|c| c.follows_park == followers)))
            .collect()
    }

    /// [r8-parks] How long a park takes to lay out: by its whole ground
    /// (a day for a small one, up to four for a big one).
    pub(super) fn park_build_days(&self, lot: LotId, tier: Tier) -> f32 {
        // The ground still to lay out: the whole park when it is new, the
        // lots it takes in when it grows.
        let area: f32 = self.park_cluster_lots(lot).into_iter().filter(|l| self.lots[*l].building.is_none() || *l == lot).map(|l| self.lots[l].area()).sum();
        let f = match tier {
            Tier::Low => 0.8,
            Tier::Mid => 1.0,
            Tier::High => 1.3,
        };
        ((0.8 + area / 3000.0) * f).clamp(1.0, 4.0)
    }

    /// [r8-parks] Build the park `lot` is part of as one project: every
    /// empty lot of it (backland too: the park is its way in) becomes a
    /// site at once under the park's name. One site on a street leads (it
    /// takes the deliveries, `step_construction`); the others keep pace
    /// with it and open with it. A park already open, or going up, takes
    /// its new lots in the same way. Returns the lead site, if one was
    /// started. Every park is built so, by the city's develop round; only
    /// the testing `build_now` opens one at once.
    pub fn build_park(&mut self, lot: LotId) -> Option<BuildingId> {
        let cluster = self.park_cluster_lots(lot);
        let built: Vec<BuildingId> = cluster
            .iter()
            .filter_map(|l| self.lots[*l].building)
            .filter(|b| self.buildings.get(*b).is_some_and(|x| x.land_use == LandUse::Park && x.service.is_none()))
            .collect();
        let vacant: Vec<LotId> = cluster.iter().copied().filter(|l| self.lot_free_for(*l, SiteRule::ANY)).collect();
        if vacant.is_empty() {
            return None;
        }
        let tier = self.lots[lot].park.as_ref().map_or(Tier::Mid, |p| p.tier);
        let days = self.park_build_days(lot, tier);
        let mut name = built.iter().find_map(|b| self.parks.of(*b).map(|s| s.name.clone())).or_else(|| built.first().map(|b| self.buildings[*b].name.clone()));
        let leading = built.iter().copied().find(|b| self.buildings[*b].construction.as_ref().is_some_and(|c| !c.follows_park));
        let mut started = None;
        if leading.is_none() && built.is_empty() {
            // A new park: the asked lot leads if on a street, else the
            // biggest of its lots on one.
            let mut street: Vec<LotId> = vacant.iter().copied().filter(|l| self.site_clear(*l, SiteRule::STREET)).collect();
            street.sort_by(|a, b| (*b == lot).cmp(&(*a == lot)).then(self.lots[*b].area().total_cmp(&self.lots[*a].area())).then(a.cmp(b)));
            for l in street {
                if let Some(b) = self.build_on_named(l, None, name.clone()) {
                    name = Some(self.buildings[b].name.clone());
                    started = Some(b);
                    break;
                }
            }
        }
        let name = name.unwrap_or_else(|| names::building_name(&mut self.rng, LandUse::Park));
        let tick = self.clock.tick;
        for l in vacant {
            if !self.lot_free_for(l, SiteRule::ANY) {
                continue;
            }
            let value = self.lot_value(l);
            let seed = self.fresh_seed();
            let mut b = Building::new(l, LandUse::Park, 0, tier, seed, name.clone(), tick, value);
            let mut c = Construction::new(days);
            c.follows_park = true;
            c.deliveries = Construction::DELIVERIES;
            b.construction = Some(c);
            b.recompute_capacity();
            let id = self.buildings.insert(b);
            self.lots[l].building = Some(id);
            self.events.push(SimEvent::BuildingBuilt(id));
        }
        started
    }

    /// [r8-parks] Empty lots of a park that is open or going up (a zone
    /// grown onto it, backland behind it) join it now, as the park's own
    /// works, rather than one by one at random. Returns how many parks
    /// grew.
    pub(super) fn extend_parks(&mut self) -> usize {
        let mut keys: Vec<LotId> = self
            .lots
            .iter()
            .filter(|(id, l)| l.land_use == LandUse::Park && !l.shore.beach && l.building.is_none() && l.park.as_ref().is_some_and(|p| p.cluster.is_some()) && self.site_clear(*id, SiteRule::ANY))
            .map(|(id, _)| self.park_key(id))
            .collect();
        keys.sort();
        keys.dedup();
        let mut grew = 0;
        for key in keys {
            let lots = self.park_cluster_lots(key);
            let has_park = lots.iter().filter_map(|l| self.lots[*l].building).any(|b| self.buildings.get(b).is_some_and(|x| x.land_use == LandUse::Park && x.service.is_none()));
            if has_park {
                if let Some(&l) = lots.iter().find(|l| self.lot_free_for(**l, SiteRule::ANY)) {
                    self.build_park(l);
                    grew += 1;
                }
            }
        }
        grew
    }

    /// [r8-parks] Once an hour (in the first ten-minute slot): the shares
    /// of each park being built keep pace with its lead site; a park's
    /// new lots with no lead site (an open park growing) are laid out by
    /// its own crew, together, and open together.
    pub(super) fn step_park_followers(&mut self, slot: u64, working: bool) {
        if slot != 0 {
            return;
        }
        let mut groups: BTreeMap<LotId, Vec<BuildingId>> = BTreeMap::new();
        let mut leads: HashMap<LotId, f32> = HashMap::new();
        for (id, b) in self.buildings.iter() {
            let Some(c) = &b.construction else { continue };
            if b.land_use != LandUse::Park || !self.lots.contains_key(b.lot) {
                continue;
            }
            let key = self.park_key(b.lot);
            if c.follows_park {
                groups.entry(key).or_default().push(id);
            } else {
                let p = leads.entry(key).or_insert(0.0);
                *p = p.max(c.progress);
            }
        }
        for (key, sites) in groups {
            let lead = leads.get(&key).copied();
            let now = sites.iter().filter_map(|b| self.buildings[*b].construction.as_ref()).map(|c| c.progress).fold(0.0, f32::max);
            let step = self.buildings[sites[0]].construction.as_ref().map_or(0.0, |c| c.work_hour_step());
            let progress = match lead {
                Some(p) => p.min(0.999),
                None if working => (now + step).min(1.0),
                None => now,
            };
            for b in &sites {
                if let Some(c) = self.buildings[*b].construction.as_mut() {
                    if (progress * 10.0).floor() > (c.progress * 10.0).floor() {
                        self.events.push(SimEvent::BuildingChanged(*b));
                    }
                    c.progress = progress;
                }
            }
            if lead.is_none() && progress >= 1.0 {
                for b in sites {
                    self.finish_construction(b);
                }
            }
        }
    }

    /// [r8-parks] Each park building's spaces are the bays of the car park
    /// on its lot (none hidden); cars parked in them stand on their bays.
    pub(crate) fn sync_park_bays(&mut self) {
        let mut bays: HashMap<BuildingId, Vec<(Vec2, f32)>> = HashMap::new();
        for (id, b) in self.buildings.iter_mut() {
            if b.land_use != LandUse::Park || b.service.is_some() {
                continue;
            }
            let spaces = self.lots.get(b.lot).and_then(|l| l.park.as_ref()).and_then(|p| p.car_park.as_ref()).filter(|c| c.own).map(|c| c.spaces.clone()).unwrap_or_default();
            b.parking_capacity = if b.operating() { spaces.len() as u16 } else { 0 };
            if !spaces.is_empty() {
                bays.insert(id, spaces);
            }
        }
        if bays.is_empty() {
            return;
        }
        for car in self.vehicles.cars.values_mut() {
            let crate::CarPlace::Parked(s) = &mut car.place else { continue };
            if !s.kind.uses_building() {
                continue;
            }
            if let Some(v) = s.building.and_then(|b| bays.get(&b)) {
                if let Some(&(p, h)) = v.get(s.slot as usize) {
                    s.pos = p;
                    s.heading = h;
                }
            }
        }
    }

    /// [r8-parks] A free bay of park `b`'s car park, as a spot, if it has
    /// one.
    pub(in crate::world) fn park_bay_spot(&self, kind: crate::vehicle::ParkKind, b: BuildingId) -> Option<crate::vehicle::ParkSpot> {
        let bld = self.buildings.get(b).filter(|x| x.land_use == LandUse::Park)?;
        let lot = self.lots.get(bld.lot)?;
        let bays = &lot.park.as_ref()?.car_park.as_ref().filter(|c| c.own)?.spaces;
        let taken: std::collections::HashSet<u16> = self.vehicles.cars.values().filter_map(|c| c.spot()).filter(|s| s.building == Some(b) && s.kind.uses_building()).map(|s| s.slot).collect();
        let (i, &(pos, heading)) = bays.iter().enumerate().find(|(i, _)| !taken.contains(&(*i as u16)))?;
        Some(crate::vehicle::ParkSpot { kind, building: Some(b), segment: lot.car_segment, side: 0, slot: i as u16, pos, heading, since: self.clock.tick, ..Default::default() })
    }

    /// [r8-parks] The building of park `b`'s car park (on whichever lot of
    /// the park holds it), when it has a bay free.
    pub(in crate::world) fn park_car_park_for(&self, b: BuildingId) -> Option<BuildingId> {
        let bld = self.buildings.get(b).filter(|x| x.land_use == LandUse::Park && x.service.is_none())?;
        if self.lots.get(bld.lot)?.park.as_ref()?.cluster_lots <= 1 {
            return None;
        }
        self.park_cluster_lots(bld.lot)
            .into_iter()
            .filter(|l| self.lots[*l].park.as_ref().is_some_and(|p| p.car_park.as_ref().is_some_and(|c| c.own)))
            .filter_map(|l| self.lots[l].building)
            .find(|x| self.buildings.get(*x).is_some_and(|y| y.land_use == LandUse::Park && y.parking_free() > 0))
    }

    /// [r8-parks] Is a parked car standing in the open in a park's car
    /// park (drawn there and picked there, as at the kerb)?
    pub fn in_park_car_park(&self, s: &crate::vehicle::ParkSpot) -> bool {
        s.kind.uses_building()
            && s.building.and_then(|b| self.buildings.get(b)).filter(|b| b.land_use == LandUse::Park).and_then(|b| self.lots.get(b.lot)).and_then(|l| l.park.as_ref()).and_then(|p| p.car_park.as_ref()).is_some_and(|c| c.distance(s.pos) < 0.5)
    }

    /// [parks] (D5) Park hub gates: the point deep in each park lot where
    /// its walkways meet. The Path tool snaps a path onto one; the path is
    /// laid to the park's edge gate on that side (`path_into_parks`), so
    /// the park lot is not cut in two, and the park's own walkway runs on
    /// from the gate to the hub.
    pub fn park_hubs(&self) -> Vec<(LotId, Vec2)> {
        self.lots
            .iter()
            .filter(|(_, l)| l.land_use == LandUse::Park && !l.shore.beach && l.polygon.len() >= 3)
            .filter_map(|(id, l)| l.park.as_ref().and_then(|p| p.paths.iter().find(|p| p.len() == 2).or(p.paths.first())).and_then(|p| p.last().copied()).filter(|h| point_in_polygon(*h, &l.polygon)).map(|h| (id, h)))
            .collect()
    }

    /// The park hub nearest `p` within `r`.
    pub fn park_hub_near(&self, p: Vec2, r: f32) -> Option<(LotId, Vec2)> {
        self.park_hubs().into_iter().filter(|(_, h)| h.distance(p) <= r).min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)))
    }

    /// [parks] (D5) A path whose end is inside a park lot (on its hub, or
    /// anywhere in it) and comes from outside it ends instead at the
    /// park's edge gate nearest the way from there to that point: a path
    /// into a park's interior would cut the park lot in two. The park
    /// lays its walkway on from the gate (`note_path_links`).
    pub fn path_into_parks(&self, pts: &[Vec2]) -> Vec<Vec2> {
        let mut out = pts.to_vec();
        let n = out.len();
        if n < 2 || self.walk.junctions.is_empty() {
            return out;
        }
        for (end, nb) in [(0, 1), (n - 1, n - 2)] {
            let p = out[end];
            let from = out[nb];
            let Some((lid, lot)) = self.lots.iter().find(|(_, l)| l.land_use == LandUse::Park && !l.shore.beach && l.park.is_some() && point_in_polygon(p, &l.polygon)) else { continue };
            if point_in_polygon(from, &lot.polygon) {
                continue;
            }
            let gate = self.walk.junctions.iter().filter(|j| j.lot == Some(lid)).map(|j| j.pos).min_by(|a, b| (from.distance(*a) + a.distance(p)).total_cmp(&(from.distance(*b) + b.distance(p))));
            if let Some(g) = gate {
                out[end] = g;
            }
        }
        out
    }

    /// The park list now (the API's look at a park just made).
    pub fn refresh_parks_now(&mut self) {
        self.refresh_parks();
    }

    /// On the hour: the park list, and who is in each park now (visitors
    /// for the litter, homeless people whiling the day away).
    pub(super) fn hourly_parks(&mut self) {
        self.refresh_parks();
        if self.parks.parks.is_empty() {
            return;
        }
        let mut here: HashMap<BuildingId, (u32, u16)> = HashMap::new();
        for c in self.citizens.values() {
            let at = match &c.activity {
                Activity::Sitting { park, .. } => *park,
                Activity::Visiting(b) => *b,
                _ => continue,
            };
            let Some(a) = self.parks.anchor(at) else { continue };
            let e = here.entry(a).or_default();
            e.0 += 1;
            if c.household.and_then(|h| self.households.get(h)).is_some_and(|h| h.home.is_none()) {
                e.1 += 1;
            }
        }
        for (a, s) in self.parks.parks.iter_mut() {
            let (v, h) = here.get(a).copied().unwrap_or_default();
            s.visits_today += v;
            s.homeless_today = s.homeless_today.max(h);
        }
    }

    /// Midnight: litter dropped and cleared, the campers by each park,
    /// dealers drawn to the neglected ones on cheap land the police seldom
    /// reach, and how well kept each park is now.
    pub(super) fn daily_parks(&mut self) {
        if self.parks.parks.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let depots = self.trash_depots();
        let camps: Vec<(Vec2, u16)> = self.society.homeless.camps.iter().map(|c| (c.pos, c.people)).collect();
        let ids: Vec<BuildingId> = self.parks.parks.keys().copied().collect();
        let mut changed: Vec<BuildingId> = Vec::new();
        for a in ids {
            let lot_ids = self.park_lots(a);
            let lots: Vec<&Lot> = lot_ids.iter().filter_map(|l| self.lots.get(*l)).collect();
            let Some(first) = lots.first() else { continue };
            let area: f32 = lots.iter().map(|l| l.area()).sum();
            let centre = if lots.len() == 1 { first.center } else { lots.iter().map(|l| l.center * l.area()).sum::<Vec2>() / area.max(1.0) };
            let served = depots.iter().any(|(_, p, r, trucks)| *trucks > 0 && p.distance(centre) <= *r);
            let campers: u32 = camps
                .iter()
                .filter(|(p, _)| lots.iter().any(|l| point_in_polygon(*p, &l.polygon) || crate::geom::distance_to_outline(*p, &l.polygon) <= pk::CAMP_REACH))
                .map(|(_, n)| *n as u32)
                .sum();
            let lv = self.land_value.at(centre);
            let police = self.services.coverage_of(lot_ids[0], ServiceKind::Police);
            let s = &self.parks.parks[&a];
            let grace = s.cleaned.is_some_and(|t| tick < t + pk::CLEAN_GRACE_DAYS * 1440);
            let load = s.visits_today as f32 / (area / 50.0).max(1.0);
            let wild = if s.preserve { 0.4 } else { 1.0 };
            let trash = (s.trash + pk::LITTER_PER_VISIT * load * wild - if served { pk::LITTER_CLEARED } else { pk::LITTER_CLEARED_UNSERVED }).clamp(0.0, 1.0);
            let homeless = s.homeless_today;
            let campers = campers.min(u16::MAX as u32) as u16;
            let dealers = if s.preserve || grace {
                0
            } else {
                // Only cheap land the police seldom reach draws them; litter
                // and people sleeping rough make it worse.
                let cheap = ((pk::DEALER_LV - lv).max(0.0) / pk::DEALER_LV) * (1.0 - police);
                let risk = cheap * (1.0 + trash + if campers + homeless > 0 { 0.5 } else { 0.0 });
                let target = ((risk * pk::MAX_DEALERS as f32).floor() as u8).min(pk::MAX_DEALERS);
                // One comes or goes a day.
                match target.cmp(&s.dealers) {
                    std::cmp::Ordering::Greater => s.dealers + 1,
                    std::cmp::Ordering::Less => s.dealers - 1,
                    std::cmp::Ordering::Equal => s.dealers,
                }
            };
            let kept = ParkState::kept_for(trash, dealers, campers, homeless);
            let look = |t: f32, d: u8| ((t * 4.0) as u32, d);
            let s = self.parks.parks.get_mut(&a).unwrap();
            if look(s.trash, s.dealers) != look(trash, dealers) {
                changed.push(a);
                changed.extend(s.members.iter().copied());
            }
            s.trash = trash;
            s.dealers = dealers;
            s.campers = campers;
            s.homeless = homeless;
            s.homeless_today = 0;
            s.kept = kept;
            s.area = area;
            s.visits_yesterday = s.visits_today;
            s.visits_today = 0;
        }
        if !changed.is_empty() {
            self.parks.version += 1;
            for b in changed {
                self.events.push(SimEvent::BuildingChanged(b));
            }
        }
    }

    /// A run-down park sours the land round it (on top of the lift every
    /// park gives): the worse kept, the more.
    pub(super) fn splat_parks(&self, b: &mut LandValueBuilder) {
        for (a, s) in &self.parks.parks {
            if s.kept >= 0.6 {
                continue;
            }
            let w = -(0.6 - s.kept) * 0.3;
            for l in self.park_lots(*a).into_iter().filter_map(|l| self.lots.get(l)) {
                b.splat(l.center, 150.0, w);
            }
        }
    }

    /// How much a park's state draws people (negative: puts them off): a
    /// run-down park is somewhere people would rather not sit.
    pub fn park_pull(&self, b: BuildingId) -> f32 {
        match self.parks.of(b) {
            Some(s) if s.kept < 0.7 => -(0.7 - s.kept) * 1.2,
            _ => 0.0,
        }
    }

    /// Pay a crew to clean up a park: the litter cleared, the dealers moved
    /// on (they stay away for a while), and the people camping in or by it
    /// found somewhere else. Returns what it cost.
    pub fn clean_up_park(&mut self, b: BuildingId) -> Result<(f32, String), String> {
        let a = self.parks.anchor(b).ok_or("not one of the city's parks")?;
        let s = &self.parks.parks[&a];
        let cost = s.cleanup_cost();
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; a clean-up of {} costs {cost:.0}", self.economy.treasury, s.name));
        }
        let name = s.name.clone();
        self.economy.debit_item(Flow::Construction, cost, Item::Named("park clean-up".to_string()));
        // The campers in reach are found somewhere else.
        let lots: Vec<Vec<Vec2>> = self.park_lots(a).into_iter().filter_map(|l| self.lots.get(l)).map(|l| l.polygon.clone()).collect();
        let mut moved: Vec<HouseholdId> = Vec::new();
        self.society.homeless.camps.retain(|c| {
            let near = lots.iter().any(|p| point_in_polygon(c.pos, p) || crate::geom::distance_to_outline(c.pos, p) <= pk::CAMP_REACH);
            if near {
                moved.extend(c.households.iter().copied());
            }
            !near
        });
        let people = moved.len();
        let tick = self.clock.tick;
        {
            let s = self.parks.parks.get_mut(&a).unwrap();
            s.trash = 0.0;
            s.dealers = 0;
            s.campers = 0;
            s.homeless = 0;
            s.cleaned = Some(tick);
            s.cleanups += 1;
            s.kept = ParkState::kept_for(0.0, 0, 0, 0);
        }
        for hid in moved {
            self.settle_homeless(hid);
        }
        let members = self.parks.parks[&a].members.clone();
        self.parks.version += 1;
        self.events.push(SimEvent::BuildingChanged(a));
        for m in members {
            self.events.push(SimEvent::BuildingChanged(m));
        }
        let camp_note = if people > 0 { format!(", {people} households camping by it found somewhere else") } else { String::new() };
        Ok((cost, format!("{name} cleaned up for {cost:.0}: litter cleared, dealers moved on{camp_note}. They keep away for {} days.", pk::CLEAN_GRACE_DAYS)))
    }

    /// Leave a park wild (a nature preserve: meadow, woodland, a trail) or
    /// make it a kept park again. Its lots are planned again at once.
    pub fn set_park_preserve(&mut self, b: BuildingId, on: bool) -> Result<String, String> {
        let a = self.parks.anchor(b).ok_or("not one of the city's parks")?;
        let lots: Vec<Vec<Vec2>> = self.park_lots(a).into_iter().filter_map(|l| self.lots.get(l)).map(|l| l.polygon.clone()).collect();
        let centre = self.buildings.get(a).and_then(|x| self.lots.get(x.lot)).map(|l| crate::zoning::inner_center(&l.polygon)).ok_or("no lot")?;
        self.parks.preserves.retain(|p| !lots.iter().any(|poly| point_in_polygon(*p, poly)));
        if on {
            self.parks.preserves.push(centre);
        }
        if let Some(s) = self.parks.parks.get_mut(&a) {
            s.preserve = on;
            if on {
                s.dealers = 0;
            }
        }
        self.rebuild_walk();
        let members = self.parks.parks.get(&a).map(|s| s.members.clone()).unwrap_or_default();
        self.parks.version += 1;
        self.events.push(SimEvent::BuildingChanged(a));
        for m in members {
            self.events.push(SimEvent::BuildingChanged(m));
        }
        let name = self.buildings.get(a).map_or_else(String::new, |x| x.name.clone());
        Ok(if on { format!("{name} is left wild: a nature preserve of meadow and woodland with a trail.") } else { format!("{name} is a kept park again: lawns, paths and benches.") })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A street with two park lots side by side and a home across it.
    fn park_town() -> (World, Vec<BuildingId>) {
        let mut cfg = WorldConfig::test_town();
        cfg.emergencies = false;
        cfg.social = false;
        let mut w = World::new(cfg);
        w.economy.treasury = 1.0e6;
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-60.0, 6.0), Vec2::new(60.0, 6.0), Vec2::new(60.0, 50.0), Vec2::new(-60.0, 50.0)], LandUse::Park);
        let mut minds = crate::mind::IdleMind;
        for k in 0..6000 {
            w.tick(&mut minds);
            let parks = w.buildings.values().filter(|b| b.land_use == LandUse::Park);
            if k > 200 && parks.clone().count() >= 2 && parks.clone().all(|b| b.operating()) {
                break;
            }
        }
        let parks: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Park && b.operating()).map(|(id, _)| id).collect();
        (w, parks)
    }

    #[test]
    fn park_lots_side_by_side_are_one_park_with_one_name() {
        let (mut w, parks) = park_town();
        if parks.len() < 2 {
            return;
        }
        w.refresh_parks();
        assert_eq!(w.parks.parks.len(), 1, "one park: {:?}", w.parks.parks.values().map(|s| &s.name).collect::<Vec<_>>());
        let (a, s) = w.parks.parks.iter().next().unwrap();
        assert_eq!(s.members.len() + 1, parks.len());
        for b in &parks {
            assert_eq!(w.buildings[*b].name, s.name, "every lot carries the park's name");
            assert_eq!(w.parks.anchor(*b), Some(*a));
        }
    }

    #[test]
    fn a_neglected_park_runs_down_and_a_clean_up_restores_it() {
        let (mut w, parks) = park_town();
        let Some(&b) = parks.first() else { return };
        w.refresh_parks();
        let a = w.parks.anchor(b).unwrap();
        {
            let s = w.parks.parks.get_mut(&a).unwrap();
            s.trash = 0.9;
            s.dealers = 3;
            s.homeless_today = 6;
            s.visits_today = 4000;
        }
        w.daily_parks();
        let s = &w.parks.parks[&a];
        assert!(s.run_down(), "litter, dealers and homeless: run down ({})", s.kept);
        assert!(w.park_pull(b) < 0.0, "people would rather sit elsewhere");
        let before = w.economy.treasury;
        let (cost, msg) = w.clean_up_park(b).expect("paid for");
        assert!(cost > 0.0 && (before - w.economy.treasury - cost).abs() < 1.0, "{msg}");
        let s = &w.parks.parks[&a];
        assert!(!s.run_down() && s.dealers == 0 && s.trash == 0.0);
        // Dealers keep away for the grace days, whatever the land.
        w.daily_parks();
        assert_eq!(w.parks.parks[&a].dealers, 0);
    }

    #[test]
    fn a_path_to_a_park_hub_ends_at_its_edge_gate() {
        let (w, parks) = park_town();
        let Some(&b) = parks.first() else { return };
        let lot = w.buildings[b].lot;
        let Some((_, hub)) = w.park_hubs().into_iter().find(|(l, _)| *l == lot) else { panic!("a park lot has a hub") };
        let outside = Vec2::new(hub.x, 140.0);
        let pts = w.path_into_parks(&[outside, hub]);
        assert!(!point_in_polygon(pts[1], &w.lots[lot].polygon), "the path stops at the park's edge");
        assert!(w.walk.junctions.iter().any(|j| j.lot == Some(lot) && j.pos.distance(pts[1]) < 0.01), "on one of its gates");
        assert_eq!(pts[0], outside);
    }

    #[test]
    fn a_park_can_be_left_wild() {
        let (mut w, parks) = park_town();
        let Some(&b) = parks.first() else { return };
        w.refresh_parks();
        w.set_park_preserve(b, true).expect("made a preserve");
        let lot = w.buildings[b].lot;
        assert!(w.lots[lot].park.as_ref().is_some_and(|p| p.preserve && p.features.is_empty()), "planned wild");
        w.refresh_parks();
        assert!(w.parks.of(b).is_some_and(|s| s.preserve));
        w.set_park_preserve(b, false).expect("a park again");
        assert!(w.lots[lot].park.as_ref().is_some_and(|p| !p.preserve));
    }

    /// [r8-parks] A deep park zone: lots on the street and backland behind.
    fn deep_park() -> World {
        let mut cfg = WorldConfig::test_town();
        cfg.emergencies = false;
        cfg.social = false;
        let mut w = World::new(cfg);
        w.economy.treasury = 1.0e6;
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-70.0, 6.0), Vec2::new(70.0, 6.0), Vec2::new(70.0, 96.0), Vec2::new(-70.0, 96.0)], LandUse::Park);
        w
    }

    #[test]
    fn a_park_builds_as_one_project_with_its_backland_and_one_name() {
        let mut w = deep_park();
        let park_lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Park).map(|(id, _)| id).collect();
        assert!(park_lots.len() >= 3);
        let key = w.park_key(park_lots[0]);
        assert!(park_lots.iter().all(|l| w.park_key(*l) == key), "the zone is one park");
        assert!(park_lots.iter().any(|l| w.lots[*l].landlocked), "with backland behind");
        let first = *park_lots.iter().find(|l| !w.lots[**l].landlocked).unwrap();
        let lead = w.build_park(first).expect("a lead site on the street");
        // Every lot of it is a site now, under one name; one leads.
        let sites: Vec<BuildingId> = park_lots.iter().map(|l| w.lots[*l].building.expect("every lot of the park is built on, backland too")).collect();
        let name = w.buildings[lead].name.clone();
        assert!(sites.iter().all(|b| w.buildings[*b].name == name && w.buildings[*b].construction.is_some()));
        assert_eq!(sites.iter().filter(|b| w.buildings[**b].construction.as_ref().is_some_and(|c| !c.follows_park)).count(), 1, "one lead site");
        // The others keep pace with it, and it opens them all.
        w.buildings[lead].construction.as_mut().unwrap().progress = 0.5;
        w.step_park_followers(0, true);
        assert!(sites.iter().all(|b| w.buildings[*b].construction.as_ref().unwrap().progress == 0.5));
        w.finish_construction(lead);
        assert!(sites.iter().all(|b| w.buildings[*b].operating()), "the park opens as one");
        w.refresh_parks();
        assert_eq!(w.parks.parks.len(), 1);
        let s = w.parks.parks.values().next().unwrap();
        assert_eq!(s.lots.len(), park_lots.len(), "the city counts the whole park");
        assert_eq!(s.name, name);
        assert!((s.area - park_lots.iter().map(|l| w.lots[*l].area()).sum::<f32>()).abs() < 1.0);
    }

    #[test]
    fn new_lots_of_an_open_park_join_it_under_its_name() {
        let mut w = deep_park();
        let park_lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Park).map(|(id, _)| id).collect();
        let first = *park_lots.iter().find(|l| !w.lots[**l].landlocked).unwrap();
        let lead = w.build_park(first).unwrap();
        w.finish_construction(lead);
        w.refresh_parks();
        let name = w.buildings[lead].name.clone();
        // A lot of it cleared (as a rezoned or re-cut lot would be): the
        // park takes it back in, as its own works, under its name.
        let back = *park_lots.iter().find(|l| w.lots[**l].landlocked).unwrap();
        let old = w.lots[back].building.take().unwrap();
        w.buildings.remove(old);
        assert_eq!(w.extend_parks(), 1);
        let b = w.lots[back].building.expect("rebuilt");
        assert_eq!(w.buildings[b].name, name);
        assert!(w.buildings[b].construction.as_ref().is_some_and(|c| c.follows_park));
        // With no lead site, its crew lays it out over the working day.
        for _ in 0..60 {
            w.step_park_followers(0, true);
        }
        assert!(w.buildings[b].operating());
    }

    #[test]
    fn a_big_parks_car_park_holds_visible_cars() {
        let mut w = deep_park();
        let park_lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Park).map(|(id, _)| id).collect();
        let first = *park_lots.iter().find(|l| !w.lots[**l].landlocked).unwrap();
        let lead = w.build_park(first).unwrap();
        w.finish_construction(lead);
        let (lot, bays) = park_lots.iter().find_map(|l| w.lots[*l].park.as_ref().and_then(|p| p.car_park.as_ref()).filter(|c| c.own).map(|c| (*l, c.spaces.clone()))).expect("a 12,600 m2 park has a car park");
        let b = w.lots[lot].building.unwrap();
        assert_eq!(w.buildings[b].parking_capacity as usize, bays.len(), "its spaces are its bays, none hidden");
        let s1 = w.park_bay_spot(crate::vehicle::ParkKind::Building, b).unwrap();
        assert!(bays.iter().any(|(p, _)| p.distance(s1.pos) < 0.01));
        assert!(w.in_park_car_park(&s1), "a car there is out in the open");
        // A park lot without the car park has no spaces of its own.
        let other = park_lots.iter().filter(|l| **l != lot).find_map(|l| w.lots[*l].building).unwrap();
        assert_eq!(w.buildings[other].parking_capacity, 0);
        assert!(w.park_bay_spot(crate::vehicle::ParkKind::Building, other).is_none());
    }

    #[test]
    fn an_old_save_regroups_its_park_lots_and_renames_them() {
        let mut w = deep_park();
        let park_lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Park).map(|(id, _)| id).collect();
        let first = *park_lots.iter().find(|l| !w.lots[**l].landlocked).unwrap();
        let lead = w.build_park(first).unwrap();
        w.finish_construction(lead);
        // As a save from before: no park key on the lots, every lot its own
        // name, the backland left bare.
        let back = *park_lots.iter().find(|l| w.lots[**l].landlocked).unwrap();
        let gone = w.lots[back].building.take().unwrap();
        w.buildings.remove(gone);
        for (k, l) in park_lots.iter().enumerate() {
            if let Some(p) = w.lots[*l].park.as_mut() {
                p.cluster = None;
            }
            if let Some(b) = w.lots[*l].building {
                w.buildings[b].name = format!("Old Park {k}");
            }
        }
        w.parks = Default::default();
        let text = w.to_ron().unwrap();
        let mut back_w = World::from_ron(&text).unwrap();
        let key = back_w.park_key(park_lots[0]);
        assert!(back_w.lots[park_lots[0]].park.as_ref().is_some_and(|p| p.cluster.is_some()), "planned again on load");
        assert!(park_lots.iter().all(|l| back_w.park_key(*l) == key));
        assert_eq!(back_w.parks.parks.len(), 1, "one park");
        let name = back_w.parks.parks.values().next().unwrap().name.clone();
        assert!(park_lots.iter().filter_map(|l| back_w.lots[*l].building).all(|b| back_w.buildings[b].name == name), "every lot renamed after the park");
        // The bare backland is built with its park.
        back_w.extend_parks();
        assert!(back_w.lots[back].building.is_some_and(|b| back_w.buildings[b].name == name));
    }
}
