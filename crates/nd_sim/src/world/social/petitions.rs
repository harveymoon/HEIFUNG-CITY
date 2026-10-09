//! [social] Petitions and protests: what a neighbourhood asks the city for.

use super::*;

impl World {
    /// What a neighbourhood association wants most, and why.
    pub(super) fn neighbourhood_ask(&self, gid: u32) -> Option<(Ask, String, Vec2)> {
        let g = self.social.group(gid)?;
        let at = self.home_point(g.founder)?;
        let n = g.members.len().max(1) as f32;
        let stressed = g.members.iter().filter(|m| self.citizens.get(**m).is_some_and(|c| c.rent_stress_days > 0 || c.household.and_then(|h| self.households.get(h)).is_some_and(|h| h.rent_stress_days > 0))).count() as f32;
        let jobless = g.members.iter().filter(|m| self.citizens.get(**m).is_some_and(|c| c.can_work() && c.work.is_none())).count() as f32;
        let heat = self.society.crime.heat_at(at, crate::crime::FEAR_RADIUS);
        let lot = self.citizens.get(g.founder)?.home.and_then(|h| self.buildings.get(h)).map(|b| b.lot);
        let police = lot.map_or(0.0, |l| self.services.coverage_of(l, ServiceKind::Police));
        let near_misses = self.peds.recent.iter().filter(|p| p.pos.distance(at) < 300.0).count() as f32;
        let mess: f32 = self.society.trash.bins.iter().filter(|(b, _)| self.building_lot(**b).is_some_and(|l| l.center.distance(at) < 250.0)).map(|(_, x)| x.overflow().min(2.0)).sum();
        let park = self.buildings.values().filter(|b| b.land_use == LandUse::Park && b.operating()).filter_map(|b| self.lots.get(b.lot)).any(|l| l.center.distance(at) < 400.0);
        let noise = self.land_value.nuisance_at(at);
        let street = self.street_of_home(g.founder).unwrap_or_else(|| "our street".into());
        let mut asks: Vec<(f32, Ask, String)> = vec![
            (heat * 0.6 + (1.0 - police) * 0.4 * (heat > 0.3) as u8 as f32, Ask::MorePolice, format!("more police on {street}: {} break-ins and fights this month", (heat * 3.0).round().max(1.0))),
            (stressed / n * 1.2, Ask::LowerRent, format!("something done about rents round {street}: {} of us can barely pay", stressed as u32)),
            (near_misses * 0.25, Ask::Crossing, format!("a safe crossing on {street}: people keep nearly getting hit")),
            (mess * 0.3, Ask::CleanStreets, format!("the bins on {street} emptied: the rubbish is piling up")),
            (if park { 0.0 } else { 0.55 }, Ask::APark, format!("a park near {street}: nowhere for the kids to play")),
            (jobless / n * 1.1, Ask::Jobs, format!("jobs near {street}: {} of us are out of work", jobless as u32)),
            (noise * 1.5, Ask::Quiet, format!("peace and quiet on {street}: the noise is unbearable")),
        ];
        asks.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.key().cmp(b.1.key())));
        let (w, ask, why) = asks.into_iter().next()?;
        (w >= 0.3).then_some((ask, why, at))
    }

    pub(super) fn ask_met(&self, p: &Petition) -> bool {
        let Some(g) = self.social.group(p.group) else { return false };
        match p.ask {
            Ask::MorePolice => self.lots.iter().filter(|(_, l)| l.center.distance(p.at) < 60.0).any(|(id, _)| self.services.coverage_of(id, ServiceKind::Police) >= 0.6),
            Ask::LowerRent => g.members.iter().all(|m| self.citizens.get(*m).is_none_or(|c| c.household.and_then(|h| self.households.get(h)).is_none_or(|h| h.rent_stress_days == 0))),
            Ask::Crossing => !self.peds.recent.iter().any(|x| x.pos.distance(p.at) < 300.0 && x.tick + 1440 * 2 > self.clock.tick),
            Ask::CleanStreets => !self.society.trash.bins.iter().any(|(b, x)| x.overflow() > 0.3 && self.building_lot(*b).is_some_and(|l| l.center.distance(p.at) < 250.0)),
            Ask::APark => self.buildings.values().filter(|b| b.land_use == LandUse::Park && b.operating()).filter_map(|b| self.lots.get(b.lot)).any(|l| l.center.distance(p.at) < 400.0),
            Ask::Jobs => g.members.iter().all(|m| self.citizens.get(*m).is_none_or(|c| !c.can_work() || c.work.is_some())),
            Ask::Quiet => self.land_value.nuisance_at(p.at) < 0.1,
        }
    }

    /// A neighbourhood association petitions the player.
    pub(super) fn make_petition(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        if self.social.petitions.iter().any(|p| p.group == gid && p.state == PetitionState::Open) {
            return false;
        }
        let Some((ask, why, at)) = self.neighbourhood_ask(gid) else { return false };
        // Not the same thing again within the week.
        if self.social.petitions.iter().any(|p| p.group == gid && p.ask == ask && p.day + 7 > day) {
            return false;
        }
        let Some(g) = self.social.group(gid) else { return false };
        let (name, members, founder) = (g.name.clone(), g.members.clone(), g.founder);
        let signatures = members.len() as u32 + members.iter().filter_map(|m| self.social.circles.get(m)).map(|c| c.friends().count() as u32).sum::<u32>();
        let text = format!("{name} asks the city for {why}. {signatures} signatures.");
        let rally = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.land_use != LandUse::Residential && !b.is_power())
            // City hall's steps first, then a park, then anywhere public.
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance(at) + match b.land_use { LandUse::Civic => 0.0, LandUse::Park => 400.0, _ => 900.0 })))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|x| x.0);
        let id = self.social.next_petition;
        self.social.next_petition += 1;
        self.social.petitions.push(Petition { id, group: gid, day, ask, text: text.clone(), at, signatures, state: PetitionState::Open, rally, settled: None });
        if self.social.petitions.len() > 40 {
            self.social.petitions.remove(0);
        }
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Petition { id, day });
        }
        if let Some(c) = self.citizens.get_mut(founder) {
            c.remember(tick, format!("We sent the city our petition: {why}."));
            let c = &self.citizens[founder];
            self.concerns.said(tick, founder, c, ask.concern(), 0.7, text.clone());
        }
        self.social.push_news(tick, format!("🏠 Petition: {text}"), at, Some(gid));
        true
    }

    /// Petitions answered, ignored into a protest, or given up.
    pub(in crate::world) fn check_petitions(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let open: Vec<Petition> = self.social.petitions.iter().filter(|p| matches!(p.state, PetitionState::Open | PetitionState::Protested)).cloned().collect();
        for p in open {
            let met = self.ask_met(&p);
            let gname = self.social.group(p.group).map(|g| g.name.clone()).unwrap_or_default();
            let members = self.social.group(p.group).map(|g| g.members.clone()).unwrap_or_default();
            let state = if met {
                PetitionState::Answered
            } else if p.state == PetitionState::Open && day >= p.day + 3 && p.rally.is_some() && self.social.group(p.group).is_some_and(|g| g.momentum >= 0.6) {
                PetitionState::Protested
            } else if day >= p.day + 8 {
                PetitionState::Lapsed
            } else {
                continue;
            };
            if let Some(x) = self.social.petitions.iter_mut().find(|x| x.id == p.id) {
                x.state = state;
                x.settled = Some(day);
            }
            match state {
                PetitionState::Answered => {
                    for m in &members {
                        if let Some(c) = self.citizens.get_mut(*m) {
                            c.mood = (c.mood + 0.1).min(1.0);
                            c.remember(tick, "The city listened to our petition.".to_string());
                        }
                    }
                    if let Some(g) = self.social.group_mut(p.group) {
                        g.reputation = (g.reputation + 0.15).min(1.0);
                    }
                    self.social.push_news(tick, format!("🏠 The city answered {gname}'s petition."), p.at, Some(p.group));
                }
                PetitionState::Protested => {
                    let rally = p.rally.and_then(|r| self.buildings.get(r)).map(|b| b.name.clone()).unwrap_or_default();
                    if let Some(g) = self.social.group_mut(p.group) {
                        g.works.push(Work::Protest { petition: p.id, day });
                    }
                    self.social.push_news(tick, format!("✊ {gname} will protest at {rally} today: nobody answered their petition."), p.at, Some(p.group));
                    for m in &members {
                        if let Some(c) = self.citizens.get_mut(*m) {
                            c.remember(tick, format!("Nobody answered. We march on {rally} tomorrow."));
                            self.concerns.heard(*m, c, p.ask.concern(), 0.5);
                        }
                    }
                }
                _ => {
                    for m in &members {
                        if let Some(c) = self.citizens.get_mut(*m) {
                            c.remember(tick, "Our petition went nowhere.".to_string());
                            c.mood = (c.mood - 0.04).max(0.0);
                        }
                    }
                }
            }
        }
        // A protest is held the morning after it is called.
        for p in self.social.petitions.iter_mut().filter(|p| p.state == PetitionState::Protested && p.settled == Some(day)) {
            p.settled = Some(day + 1);
        }
    }

    /// 11:25: associations whose petitions were ignored rally.
    pub(super) fn start_protests(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let rallies: Vec<(u32, u32, BuildingId)> = self.social.petitions.iter().filter(|p| p.state == PetitionState::Protested && p.settled == Some(day)).filter_map(|p| p.rally.map(|r| (p.id, p.group, r))).collect();
        for (_, gid, rally) in rallies {
            let Some(g) = self.social.group(gid) else { continue };
            let mut marchers = g.members.clone();
            // Members' friends come along.
            for m in g.members.clone() {
                if let Some(c) = self.social.circles.get(&m) {
                    marchers.extend(c.ties.iter().filter(|t| t.bond().is_friendly()).map(|t| t.who).take(2));
                }
            }
            marchers.sort();
            marchers.dedup();
            let lu = self.buildings.get(rally).map_or(LandUse::Civic, |b| b.land_use);
            let gname = g.name.clone();
            for m in marchers {
                if !self.free_for_outing(m) || hash01(key64(m), tick, 0x9A11) < 0.3 {
                    continue;
                }
                self.travel_to(m, Some(rally), Purpose::Visit(lu));
                if let Some(c) = self.citizens.get_mut(m) {
                    if c.activity.is_traveling() {
                        c.thought = format!("Marching with {gname}. They'll have to listen now.");
                        c.remember(tick, format!("Joined the {gname} protest."));
                    }
                }
            }
        }
    }
}
