//! [social] What groups make: gigs, events, murals, gardens, businesses.

use super::*;

impl World {
    /// Groups with momentum to spare make something.
    pub(in crate::world) fn produce(&mut self) {
        let ready: Vec<(u32, GroupKind)> = self.social.active_groups().filter(|g| g.momentum >= g.kind.threshold() && g.members.len() >= g.kind.min_members()).map(|g| (g.id, g.kind)).collect();
        for (gid, kind) in ready {
            let made = match kind {
                GroupKind::Band => self.make_gig(gid),
                GroupKind::SportsClub | GroupKind::BookClub => self.make_event(gid, kind),
                GroupKind::StreetArt => self.make_mural(gid),
                GroupKind::Startup => self.make_business(gid),
                GroupKind::Garden => self.make_garden(gid),
                GroupKind::Neighbourhood => self.make_petition(gid),
            };
            if let Some(g) = self.social.group_mut(gid) {
                if made {
                    g.momentum -= g.kind.threshold() * 0.8;
                    g.reputation = (g.reputation + 0.08).min(1.0);
                } else {
                    // Nothing to be done for now: it simmers.
                    g.momentum = g.momentum.min(g.kind.threshold() * 1.3);
                }
            }
            if made {
                self.social.today.works += 1;
            }
        }
    }

    pub(super) fn group_pos(&self, gid: u32) -> Vec2 {
        let g = self.social.group(gid);
        g.and_then(|g| g.base).and_then(|b| self.building_lot(b)).map(|l| l.center).or_else(|| g.and_then(|g| self.home_point(g.founder))).unwrap_or(Vec2::ZERO)
    }

    /// A band books a night at its venue (or the one its members like best).
    pub(super) fn make_gig(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let Some(g) = self.social.group(gid) else { return false };
        let venue = g.base.filter(|b| self.buildings.get(*b).is_some_and(|x| x.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue && !z.closed)));
        let venue = venue.or_else(|| self.group_base(GroupKind::Band, &g.members));
        let Some(venue) = venue else { return false };
        if self.social.gigs.iter().any(|x| x.venue == venue && x.from / 1440 == day) {
            return false;
        }
        let from = day * 1440 + 20 * 60;
        let name = g.name.clone();
        self.social.gigs.push(Gig { group: gid, venue, from, until: from + 210, crowd: 0 });
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Gig { venue, day });
        }
        let vname = self.buildings.get(venue).map(|b| b.name.clone()).unwrap_or_default();
        let pos = self.building_lot(venue).map_or(Vec2::ZERO, |l| l.center);
        let tick = self.clock.tick;
        self.social.push_news(tick, format!("♪ {name} play {vname} tonight at 20:00."), pos, Some(gid));
        // A good night for the venue's name.
        let rep = self.social.group(gid).map_or(0.0, |g| g.reputation);
        let r = self.places.reputation.entry(venue).or_default();
        r.add((0.4 + rep).min(1.0));
        r.great_nights += 1;
        true
    }

    /// A match in the park, a reading at the café: the place's name rises
    /// and the members are pleased with themselves.
    pub(super) fn make_event(&mut self, gid: u32, kind: GroupKind) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        let Some(place) = g.base.filter(|b| self.buildings.contains_key(*b)) else { return false };
        let (name, members, rep) = (g.name.clone(), g.members.clone(), g.reputation);
        let pname = self.buildings[place].name.clone();
        let r = self.places.reputation.entry(place).or_default();
        r.add((0.3 + rep).min(1.0));
        let (work, text, memory) = match kind {
            GroupKind::SportsClub => (Work::Match { park: place, day }, format!("⚽ {name} played a match in {pname}; half the street came to watch."), format!("{name} played in {pname}. What a game.")),
            _ => (Work::Reading { place, day }, format!("📖 {name} held a reading at {pname}."), format!("Read at the {name} evening at {pname}.")),
        };
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(work);
        }
        for m in members {
            if let Some(c) = self.citizens.get_mut(m) {
                c.mood = (c.mood + 0.05).min(1.0);
                c.remember(tick, memory.clone());
            }
        }
        let pos = self.building_lot(place).map_or(Vec2::ZERO, |l| l.center);
        self.social.push_news(tick, text, pos, Some(gid));
        true
    }

    /// A street-art crew paints a wall near its members: art where it is
    /// cheap and lively, unwelcome where the land is dear.
    pub(super) fn make_mural(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        let at = self.group_pos(gid);
        let painted: std::collections::BTreeSet<LotId> = self.social.murals.iter().map(|m| m.lot).collect();
        let mut cands: Vec<(f32, LotId, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.service.is_none() && b.rail.is_none() && b.port.is_none() && !b.is_power() && b.land_use != LandUse::Park)
            .filter(|(_, b)| matches!(b.land_use, LandUse::Commercial | LandUse::Industrial | LandUse::Entertainment) || (b.land_use == LandUse::Residential && b.tier != Tier::High))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, b.lot, l)))
            .filter(|(_, lid, l)| !painted.contains(lid) && l.front_edge[0].distance(l.front_edge[1]) >= 10.0 && l.center.distance(at) <= 600.0)
            .map(|(id, lid, l)| (l.center.distance(at) + 300.0 * hash01(key64(lid), day, 0x3A1), lid, id))
            .collect();
        cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let Some(&(_, lot_id, bid)) = cands.first() else { return false };
        let l = &self.lots[lot_id];
        let along = (l.front_edge[1] - l.front_edge[0]).normalize_or_zero();
        let len = l.front_edge[0].distance(l.front_edge[1]);
        let inward = (l.center - (l.front_edge[0] + l.front_edge[1]) * 0.5).normalize_or_zero();
        let width = (len * 0.4).clamp(4.0, 8.0);
        let side = if hash01(key64(lot_id), 0, 0x51DE) < 0.5 { -1.0 } else { 1.0 };
        let pos = (l.front_edge[0] + l.front_edge[1]) * 0.5 + along * side * (len * 0.5 - width * 0.5 - 1.0).max(0.0) + inward * 0.9;
        let quality = (g.reputation + 0.2 * self.interests_of(g.founder)[GroupKind::StreetArt.index()]).clamp(-1.0, 1.0);
        let name = g.name.clone();
        let members = g.members.clone();
        self.social.murals.push(Mural { group: gid, lot: lot_id, building: Some(bid), seed: mix(key64(lot_id), gid as u64), day, quality, pos, along, facing: -inward, width, height: 3.4 });
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Mural { lot: lot_id, day });
        }
        let bname = self.buildings[bid].name.clone();
        self.social.push_news(tick, format!("🎨 {name} painted a mural on {bname}."), pos, Some(gid));
        for m in members {
            if let Some(c) = self.citizens.get_mut(m) {
                c.remember(tick, format!("We painted the wall at {bname}. Best thing we've done."));
                c.mood = (c.mood + 0.05).min(1.0);
            }
        }
        // The people who live or work there have a view on it.
        let dear = self.land_value.at(pos) > 0.6;
        let people: Vec<CitizenId> = self.buildings[bid].residents.iter().chain(self.buildings[bid].workers.iter()).copied().take(12).collect();
        for p in people {
            if let Some(c) = self.citizens.get_mut(p) {
                let likes = interests(c.seed, c.age, c.class())[GroupKind::StreetArt.index()] > 0.35 && !dear;
                c.remember(tick, if likes { format!("Somebody painted a mural on {bname}. Love it.") } else { format!("Somebody sprayed all over the wall at {bname}.") });
            }
        }
        true
    }

    /// A garden on a vacant lot near the members.
    pub(super) fn make_garden(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        if self.social.gardens.iter().any(|x| x.group == gid) {
            // One garden each; tending it keeps the name up.
            return false;
        }
        let at = self.group_pos(gid);
        let used: std::collections::BTreeSet<LotId> = self.social.gardens.iter().map(|x| x.lot).collect();
        let lot = self
            .lots
            .iter()
            .filter(|(id, l)| !used.contains(id) && l.area() >= 150.0 && l.land_use != LandUse::Park && l.center.distance(at) <= 700.0)
            .filter(|(id, _)| self.lot_free_for(*id, SiteRule::STREET))
            .min_by(|a, b| a.1.center.distance(at).total_cmp(&b.1.center.distance(at)).then(a.0.cmp(&b.0)))
            .map(|(id, l)| (id, l.center));
        let Some((lot, pos)) = lot else { return false };
        let name = g.name.clone();
        let members = g.members.clone();
        self.social.gardens.push(Garden { group: gid, lot, seed: mix(key64(lot), gid as u64), day, pos });
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Garden { lot, day });
        }
        let street = self.streets.of(self.lots[lot].front_segment).map(|s| s.name.clone()).unwrap_or_else(|| "the street".into());
        self.social.push_news(tick, format!("🌱 {name} turned the empty lot on {street} into a community garden."), pos, Some(gid));
        for m in members {
            if let Some(c) = self.citizens.get_mut(m) {
                c.remember(tick, format!("Planted our garden on {street}."));
                c.mood = (c.mood + 0.06).min(1.0);
            }
        }
        self.events.push(SimEvent::LotsChanged);
        true
    }

    /// A startup with enough money between its members opens a real
    /// business on a vacant lot near them.
    pub(super) fn make_business(&mut self, gid: u32) -> bool {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let Some(g) = self.social.group(gid) else { return false };
        if g.works.iter().any(|w| matches!(w, Work::Business { .. })) {
            return false;
        }
        let members = g.members.clone();
        let founder = g.founder;
        let name = g.name.clone();
        let wealth = |c: &Citizen| c.savings.max(0.0) + c.household.and_then(|h| self.households.get(h)).map_or(0.0, |h| h.money.max(0.0) * 0.5);
        let pot: f32 = members.iter().filter_map(|m| self.citizens.get(*m)).map(wealth).sum();
        if pot < 700.0 {
            return false;
        }
        // What to open: from the founders' tastes.
        let i = self.interests_of(founder);
        let hed = self.citizens.get(founder).map_or(0.5, |c| crate::places::traits(c.seed)[2]);
        let kind = if i[GroupKind::Band.index()] > 0.5 || hed > 0.7 {
            BusinessKind::Venue
        } else if mix(gid as u64, 0x5A7) % 3 == 0 {
            BusinessKind::Bakery
        } else if mix(gid as u64, 0x5A8) % 2 == 0 {
            BusinessKind::GadgetShop
        } else {
            BusinessKind::Office
        };
        let lu = if kind == BusinessKind::Venue { LandUse::Entertainment } else { LandUse::Commercial };
        let at = self.home_point(founder).unwrap_or(Vec2::ZERO);
        let lot = self
            .lots
            .iter()
            .filter(|(id, l)| l.land_use == lu && l.car_access() && self.lot_free_for(*id, SiteRule::STREET) && !self.social.gardens.iter().any(|g| g.lot == *id))
            .map(|(id, l)| (id, l.center.distance(at)))
            .filter(|x| x.1 <= 1500.0)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|x| x.0);
        let Some(lot) = lot else { return false };
        // The stake: a share of everyone's savings.
        let stake = (pot * 0.4).min(2500.0);
        let shares: Vec<(CitizenId, f32)> = members.iter().filter_map(|m| self.citizens.get(*m).map(|c| (*m, stake * wealth(c) / pot.max(1.0)))).collect();
        let Some(bid) = self.build_on_as(lot, Some(kind)) else { return false };
        for (m, share) in shares {
            let m = &m;
            let from_pocket = share.min(self.citizens[*m].savings.max(0.0));
            self.citizens[*m].savings -= from_pocket;
            let rest = share - from_pocket;
            if let Some(h) = self.citizens[*m].household.and_then(|h| self.households.get_mut(h)) {
                h.money -= rest;
            }
        }
        let bname = format!("{name}");
        if let Some(b) = self.buildings.get_mut(bid) {
            b.name = bname.clone();
            if let Some(z) = b.business.as_mut() {
                z.money += stake;
                let fname = self.citizens.get(founder).map(|c| c.name.clone()).unwrap_or_default();
                z.origin = Some(format!("Founded by {fname} and friends ({} people pooling ${stake:.0}).", members.len()));
            }
        }
        self.set_business_owner(bid, founder);
        if let Some(g) = self.social.group_mut(gid) {
            g.works.push(Work::Business { building: bid, day });
        }
        for m in &members {
            if let Some(c) = self.citizens.get_mut(*m) {
                c.remember(tick, format!("We put our savings together and are opening {bname}, a {}.", kind.label()));
            }
        }
        let pos = self.lots[lot].center;
        self.social.push_news(tick, format!("💡 {bname}: friends pooled ${stake:.0} to open a {} near their homes.", kind.label()), pos, Some(gid));
        true
    }

    /// 19:25: tonight's gigs: the band goes to play.
    pub(super) fn start_gigs(&mut self) {
        let tick = self.clock.tick;
        let tonight: Vec<(u32, BuildingId)> = self.social.gigs.iter().filter(|g| g.from > tick && g.from <= tick + 60).map(|g| (g.group, g.venue)).collect();
        for (gid, venue) in tonight {
            let members = self.social.group(gid).map(|g| g.members.clone()).unwrap_or_default();
            for m in members {
                if self.free_for_outing(m) {
                    self.travel_to(m, Some(venue), Purpose::Visit(LandUse::Entertainment));
                    if let Some(c) = self.citizens.get_mut(m) {
                        c.thought = "Gig tonight. Hope somebody turns up.".to_string();
                    }
                }
            }
        }
    }

    /// Tonight's gig crowd (the venue's visitors while the band plays).
    pub(super) fn count_gig_crowds(&mut self) {
        let tick = self.clock.tick;
        let on: Vec<(usize, BuildingId)> = self.social.gigs.iter().enumerate().filter(|(_, g)| g.on_at(tick)).map(|(i, g)| (i, g.venue)).collect();
        for (i, v) in on {
            let n = self.buildings.get(v).map_or(0, |b| b.visitors as u32);
            let g = &mut self.social.gigs[i];
            g.crowd = g.crowd.max(n);
        }
    }

    /// [land] Murals and gardens on the land value: a good mural livens a
    /// cheap street and grates on a dear one; gardens are liked.
    pub(in crate::world) fn splat_social(&self, b: &mut LandValueBuilder) {
        if !self.config.social {
            return;
        }
        for m in &self.social.murals {
            let dear = self.land_value.at(m.pos) > 0.6;
            let w = 0.02 * m.quality - if dear { 0.015 } else { 0.0 };
            b.splat(m.pos, 70.0, w);
        }
        for g in &self.social.gardens {
            b.splat(g.pos, 80.0, 0.025);
        }
    }
}
