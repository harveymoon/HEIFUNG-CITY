//! [parks] (D5) The parks in the world: one state (and one name) for park
//! lots side by side, the litter, campers and dealers that run a park
//! down, the paid clean-up, nature preserves, and what a run-down park does
//! to the land round it and to who goes there. Data in `crate::parks`.

use super::*;
use crate::parks::{self as pk, ParkState};
use crate::services::ServiceKind;

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

    /// The parks as they stand: park lots side by side are one park, held
    /// by its oldest building (so a park keeps its name as lots join it),
    /// every lot of it named alike. Gone parks are dropped.
    pub(super) fn refresh_parks(&mut self) {
        let now = self.park_buildings();
        let by_lot: HashMap<LotId, BuildingId> = now.iter().map(|(b, l)| (*l, *b)).collect();
        let lot_ids: Vec<LotId> = now.iter().map(|x| x.1).collect();
        let groups = crate::shore::groups_of(&lot_ids, |a, b| crate::shore::touching(&self.lots[a].polygon, &self.lots[b].polygon));
        let mut anchors: Vec<(BuildingId, Vec<BuildingId>, Vec<LotId>)> = Vec::new();
        for lots in groups {
            let mut members: Vec<BuildingId> = lots.iter().map(|l| by_lot[l]).collect();
            members.sort();
            let anchor = members.iter().copied().filter(|b| self.parks.parks.contains_key(b)).min_by_key(|b| (self.buildings[*b].built_tick, *b)).or_else(|| members.iter().copied().min_by_key(|b| (self.buildings[*b].built_tick, *b))).unwrap_or(members[0]);
            let others: Vec<BuildingId> = members.into_iter().filter(|b| *b != anchor).collect();
            anchors.push((anchor, others, lots));
        }
        let keep: std::collections::HashSet<BuildingId> = anchors.iter().map(|x| x.0).collect();
        // A park that merged into another hands its troubles on.
        let gone: Vec<(BuildingId, ParkState)> = self.parks.parks.iter().filter(|(b, _)| !keep.contains(b)).map(|(b, s)| (*b, s.clone())).collect();
        self.parks.parks.retain(|b, _| keep.contains(b));
        for (b, others, lots) in anchors {
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
                let name = s.name.clone();
                for m in &others {
                    if self.buildings[*m].name != name {
                        self.buildings[*m].name = name.clone();
                        self.events.push(SimEvent::BuildingChanged(*m));
                    }
                }
            }
        }
        self.parks.reindex();
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
}
