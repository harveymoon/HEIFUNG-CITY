//! [social] Groups: forming, recruiting, splitting, folding, and their
//! meetings.

use super::*;

impl World {
    pub(in crate::world) fn daily_groups(&mut self) {
        self.form_groups();
        self.recruit();
        self.split_groups();
        self.group_momentum();
        self.produce();
        self.fold_groups();
    }

    pub(super) fn interests_of(&self, c: CitizenId) -> [f32; 7] {
        self.citizens.get(c).map_or([0.0; 7], |x| interests(x.seed, x.age, x.class()))
    }

    pub(super) fn groups_of(&self, c: CitizenId) -> Vec<u32> {
        self.social.circles.get(&c).map_or(Vec::new(), |k| k.groups.clone())
    }

    pub(super) fn in_group_of_kind(&self, c: CitizenId, kind: GroupKind) -> bool {
        self.groups_of(c).iter().any(|g| self.social.group(*g).is_some_and(|x| x.kind == kind))
    }

    /// Friends with a passion in common start something.
    pub(in crate::world) fn form_groups(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let active = self.social.active_groups().count();
        let cap = (self.citizens.len() / 45).max(2);
        if active >= cap {
            return;
        }
        let ids: Vec<CitizenId> = self.social.circles.iter().filter(|(_, c)| c.friends().count() >= 2 && c.groups.len() < MAX_GROUPS_EACH).map(|(id, _)| *id).collect();
        if ids.is_empty() {
            return;
        }
        let start = (mix(day, 0x6A0) % ids.len() as u64) as usize;
        let mut formed = 0;
        let mut per_kind = [0u32; 7];
        for g in self.social.active_groups() {
            per_kind[g.kind.index()] += 1;
        }
        for k in 0..ids.len().min(800) {
            if formed >= 2 || active + formed >= cap {
                break;
            }
            let f = ids[(start + k) % ids.len()];
            let Some(fc) = self.citizens.get(f) else { continue };
            if fc.age < 15 {
                continue;
            }
            let mine = self.interests_of(f);
            // Strongest passion that is not taken care of yet.
            // A city with five bands already has less room for a sixth.
            let mut kinds: Vec<(f32, GroupKind)> = GroupKind::ALL.iter().map(|k| (mine[k.index()], *k)).filter(|(v, k)| *v >= 0.45 && !self.in_group_of_kind(f, *k)).map(|(v, k)| (v - 0.06 * per_kind[k.index()] as f32, k)).collect();
            kinds.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let Some(&(_, kind)) = kinds.first() else { continue };
            if hash01(fc.seed, day, 0x6F0 + kind.index() as u64) > 0.35 {
                continue;
            }
            let home = fc.home.and_then(|h| self.building_lot(h)).map(|l| l.center);
            let mut crew: Vec<CitizenId> = vec![f];
            let friends: Vec<CitizenId> = self.social.circles[&f].friends().map(|t| t.who).collect();
            for m in friends {
                if crew.len() >= 5 {
                    break;
                }
                let Some(mc) = self.citizens.get(m) else { continue };
                if mc.age < 15 && kind != GroupKind::Band && kind != GroupKind::SportsClub {
                    continue;
                }
                if self.interests_of(m)[kind.index()] < 0.3 || self.in_group_of_kind(m, kind) || self.groups_of(m).len() >= MAX_GROUPS_EACH {
                    continue;
                }
                // A neighbourhood is people who live near each other.
                if kind == GroupKind::Neighbourhood {
                    let theirs = mc.home.and_then(|h| self.building_lot(h)).map(|l| l.center);
                    if !matches!((home, theirs), (Some(x), Some(y)) if x.distance(y) <= 450.0) {
                        continue;
                    }
                }
                crew.push(m);
            }
            if crew.len() < kind.min_members() {
                continue;
            }
            let id = self.social.next_group;
            self.social.next_group += 1;
            let name = self.group_name(kind, f, id);
            let base = self.group_base(kind, &crew);
            let rep = 0.1 * crew.iter().map(|m| self.interests_of(*m)[kind.index()]).sum::<f32>() / crew.len() as f32;
            self.social.groups.push(Group { id, kind, name: name.clone(), founded: day, founder: f, members: crew.clone(), momentum: 0.2, reputation: rep, base, works: Vec::new(), split_from: None, last_met: day, meetings: 0, idle_days: 0, folded: None });
            for &m in &crew {
                self.social.circles.entry(m).or_default().groups.push(id);
                if let Some(c) = self.citizens.get_mut(m) {
                    c.remember(tick, if m == f { format!("Started {name}, a {}, with friends.", kind.label()) } else { format!("Joined {name}, a {}.", kind.label()) });
                }
            }
            let pos = home.unwrap_or(Vec2::ZERO);
            let fname = self.citizens[f].name.clone();
            self.social.push_news(tick, format!("{} {fname} and friends started {name}, a {}.", kind.icon(), kind.label()), pos, Some(id));
            self.social.today.groups_formed += 1;
            formed += 1;
            per_kind[kind.index()] += 1;
        }
    }

    /// Where a group meets: a venue (or for gardeners, sportspeople and
    /// associations a park) most of its members like.
    pub(super) fn group_base(&self, kind: GroupKind, members: &[CitizenId]) -> Option<BuildingId> {
        let want = |b: &Building| match kind {
            GroupKind::SportsClub | GroupKind::Garden => b.land_use == LandUse::Park,
            GroupKind::Neighbourhood => b.land_use == LandUse::Park || b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::Bakery | BusinessKind::Cafe)),
            GroupKind::BookClub => b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::Bakery | BusinessKind::Venue | BusinessKind::Cafe | BusinessKind::Bookshop)),
            _ => b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue),
        };
        let mut score: BTreeMap<BuildingId, f32> = BTreeMap::new();
        for m in members {
            let Some(c) = self.citizens.get(*m) else { continue };
            for p in c.places.places.iter().filter(|p| !p.never && p.score > -0.1) {
                if self.buildings.get(p.place).is_some_and(|b| want(b)) {
                    *score.entry(p.place).or_insert(0.0) += 1.0 + p.score + if p.regular { 0.5 } else { 0.0 };
                }
            }
        }
        if let Some((b, _)) = score.into_iter().max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0))) {
            return Some(b);
        }
        // Nothing liked in common: the nearest such place to the first member.
        let at = members.first().and_then(|m| self.citizens.get(*m)).and_then(|c| c.home).and_then(|h| self.building_lot(h)).map(|l| l.center)?;
        self.buildings.iter().filter(|(_, b)| want(b) && b.operating()).filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance(at)))).min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0))).map(|x| x.0)
    }

    pub(super) fn street_of_home(&self, c: CitizenId) -> Option<String> {
        let lot = self.citizens.get(c)?.home.and_then(|h| self.building_lot(h))?;
        self.streets.of(lot.front_segment).map(|s| s.name.clone())
    }

    pub(super) fn group_name(&self, kind: GroupKind, founder: CitizenId, id: u32) -> String {
        let h = mix(self.config.seed ^ id as u64, 0x6A3E);
        let street = self.street_of_home(founder).unwrap_or_else(|| "Market Street".into());
        let word = street.split_whitespace().next().unwrap_or("Market").to_string();
        let fname = self.citizens.get(founder).map(|c| surname(&c.name).to_string()).unwrap_or_default();
        let name = match kind {
            GroupKind::Band => format!(
                "The {} {}",
                pick(&["Static", "Velvet", "Chrome", "Broken", "Midnight", "Electric", "Hollow", "Paper", "Feral", "Signal", "Neon", "Rust"], h),
                pick(&["Saints", "Moths", "Wires", "Echoes", "Ghosts", "Foxes", "Lanterns", "Sirens", "Circuits", "Dogs", "Comets", "Vandals"], h >> 8)
            ),
            GroupKind::SportsClub => format!("{word} {}", pick(&["FC", "Runners", "Ballers", "Strikers", "Athletic", "Rovers"], h)),
            GroupKind::BookClub => format!("The {word} {}", pick(&["Readers", "Book Circle", "Page Turners", "Night Readers"], h)),
            GroupKind::StreetArt => format!("{} {}", pick(&["Spray", "Rust", "Ultraviolet", "Concrete", "Gutter", "Prism", "Neon", "Drip"], h), pick(&["Crew", "Collective", "Kids", "Syndicate"], h >> 8)),
            GroupKind::Startup => format!("{fname} {}", pick(&["Labs", "& Co.", "Works", "Systems", "Collective"], h)),
            GroupKind::Garden => format!("{word} {}", pick(&["Growers", "Green Hands", "Garden Club", "Allotments"], h)),
            GroupKind::Neighbourhood => format!("{street} Residents' Association"),
        };
        if self.social.groups.iter().any(|g| g.name == name) {
            format!("{name} {}", id + 1)
        } else {
            name
        }
    }

    /// Members' friends who share the passion join (one a group a day).
    pub(super) fn recruit(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let groups: Vec<(u32, GroupKind, Vec<CitizenId>, String)> = self.social.active_groups().filter(|g| g.members.len() < MAX_MEMBERS).map(|g| (g.id, g.kind, g.members.clone(), g.name.clone())).collect();
        for (gid, kind, members, name) in groups {
            if hash01(gid as u64, day, 0x4EC) > 0.45 {
                continue;
            }
            let mut cands: Vec<(f32, CitizenId, CitizenId)> = Vec::new();
            for m in &members {
                let Some(c) = self.social.circles.get(m) else { continue };
                for t in c.friends() {
                    if members.contains(&t.who) || self.groups_of(t.who).len() >= MAX_GROUPS_EACH || self.in_group_of_kind(t.who, kind) {
                        continue;
                    }
                    let Some(x) = self.citizens.get(t.who) else { continue };
                    if x.age < 15 {
                        continue;
                    }
                    let i = self.interests_of(t.who)[kind.index()];
                    // Nobody joins a group with an enemy in it.
                    let foe = self.social.circles.get(&t.who).is_some_and(|k| members.iter().any(|y| k.bond(*y) == Bond::Enemy));
                    if i >= 0.3 && !foe {
                        cands.push((i + t.affinity, t.who, *m));
                    }
                }
            }
            cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let Some(&(_, new, via)) = cands.first() else { continue };
            if let Some(g) = self.social.group_mut(gid) {
                g.members.push(new);
                g.momentum += 0.1;
            }
            self.social.circles.entry(new).or_default().groups.push(gid);
            let vn = self.citizens.get(via).map(|c| c.name.clone()).unwrap_or_default();
            if let Some(c) = self.citizens.get_mut(new) {
                c.remember(tick, format!("{vn} got me into {name}."));
            }
        }
    }

    /// Rivals inside a group: one walks out, taking their friends; enough
    /// of them start their own.
    pub(super) fn split_groups(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let groups: Vec<(u32, GroupKind, Vec<CitizenId>, String)> = self.social.active_groups().map(|g| (g.id, g.kind, g.members.clone(), g.name.clone())).collect();
        for (gid, kind, members, name) in groups {
            // The worst pair.
            let mut worst: Option<(f32, CitizenId, CitizenId)> = None;
            for &x in &members {
                let Some(c) = self.social.circles.get(&x) else { continue };
                for &y in &members {
                    if x >= y {
                        continue;
                    }
                    let a = c.tie(y).map_or(0.0, |t| t.affinity);
                    if a <= RIVAL_AFFINITY && worst.is_none_or(|w| a < w.0) {
                        worst = Some((a, x, y));
                    }
                }
            }
            let Some((a, x, y)) = worst else { continue };
            if let Some(g) = self.social.group_mut(gid) {
                g.momentum = (g.momentum - 0.15).max(0.0);
            }
            if hash01(gid as u64, day, 0x5917) > 0.15 + (-a - 0.25).max(0.0) {
                continue;
            }
            // Who leaves: the one with fewer friends in the group.
            let friends_in = |p: CitizenId| self.social.circles.get(&p).map_or(0, |c| members.iter().filter(|m| c.bond(**m).is_friendly()).count());
            let (leaver, stayer) = if friends_in(x) < friends_in(y) || (friends_in(x) == friends_in(y) && x > y) { (x, y) } else { (y, x) };
            let feel = |p: CitizenId, q: CitizenId| self.social.circles.get(&p).and_then(|c| c.tie(q)).map_or(0.0, |t| t.affinity);
            let mut out: Vec<CitizenId> = vec![leaver];
            for &m in &members {
                if m != leaver && m != stayer && feel(m, leaver) > feel(m, stayer) + 0.1 {
                    out.push(m);
                }
            }
            if out.len() * 2 >= members.len() + 1 && out.len() > 1 {
                out.truncate(members.len() / 2);
            }
            if let Some(g) = self.social.group_mut(gid) {
                g.members.retain(|m| !out.contains(m));
            }
            for m in &out {
                if let Some(c) = self.social.circles.get_mut(m) {
                    c.groups.retain(|g| *g != gid);
                }
            }
            let ln = self.citizens.get(leaver).map(|c| c.name.clone()).unwrap_or_default();
            let sn = self.citizens.get(stayer).map(|c| c.name.clone()).unwrap_or_default();
            let pos = self.home_point(leaver).unwrap_or(Vec2::ZERO);
            if out.len() >= kind.min_members() {
                let id = self.social.next_group;
                self.social.next_group += 1;
                let new_name = self.group_name(kind, leaver, id);
                let base = self.group_base(kind, &out);
                self.social.groups.push(Group { id, kind, name: new_name.clone(), founded: day, founder: leaver, members: out.clone(), momentum: 0.3, reputation: 0.0, base, works: Vec::new(), split_from: Some(gid), last_met: day, meetings: 0, idle_days: 0, folded: None });
                for m in &out {
                    self.social.circles.entry(*m).or_default().groups.push(id);
                    if let Some(c) = self.citizens.get_mut(*m) {
                        c.remember(tick, format!("Left {name} over {sn}. Started {new_name}."));
                    }
                }
                self.social.push_news(tick, format!("{} {name} split: {ln} walked out after a row with {sn} and started {new_name}.", kind.icon()), pos, Some(id));
            } else {
                for m in &out {
                    if let Some(c) = self.citizens.get_mut(*m) {
                        c.remember(tick, format!("Quit {name}. Couldn't stand {sn} any more."));
                    }
                }
                self.social.push_news(tick, format!("{} {ln} quit {name} after a row with {sn}.", kind.icon()), pos, Some(gid));
            }
            self.social.today.splits += 1;
        }
    }

    /// Momentum: members who care keep it going; it cools by itself.
    pub(super) fn group_momentum(&mut self) {
        let day = self.clock.day();
        let updates: Vec<(u32, f32)> = self
            .social
            .active_groups()
            .map(|g| {
                let care: f32 = g.members.iter().map(|m| self.interests_of(*m)[g.kind.index()]).sum();
                let met_lately = if day.saturating_sub(g.last_met) <= 3 { 0.1 } else { 0.0 };
                (g.id, 0.08 * care + met_lately)
            })
            .collect();
        for (gid, gain) in updates {
            if let Some(g) = self.social.group_mut(gid) {
                g.momentum = g.momentum * 0.95 + gain;
                if g.momentum < 0.15 || g.members.len() < 2 {
                    g.idle_days += 1;
                } else {
                    g.idle_days = 0;
                }
            }
        }
    }

    pub(super) fn fold_groups(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let folding: Vec<(u32, String, GroupKind, Vec<CitizenId>)> = self.social.active_groups().filter(|g| g.members.len() < 2 || g.idle_days >= 10).map(|g| (g.id, g.name.clone(), g.kind, g.members.clone())).collect();
        for (gid, name, kind, members) in folding {
            if let Some(g) = self.social.group_mut(gid) {
                g.folded = Some(day);
            }
            for m in &members {
                if let Some(c) = self.social.circles.get_mut(m) {
                    c.groups.retain(|g| *g != gid);
                }
                if let Some(c) = self.citizens.get_mut(*m) {
                    c.remember(tick, format!("{name} is finished."));
                }
            }
            let pos = members.first().and_then(|m| self.home_point(*m)).unwrap_or(Vec2::ZERO);
            self.social.push_news(tick, format!("{} {name} folded.", kind.icon()), pos, Some(gid));
            self.social.today.groups_folded += 1;
        }
        // Gardens built over, murals whose wall came down.
        let lots = &self.lots;
        let gone_gardens: Vec<Garden> = self.social.gardens.iter().filter(|g| lots.get(g.lot).is_none_or(|l| l.building.is_some() || !l.usable())).cloned().collect();
        self.social.gardens.retain(|g| lots.get(g.lot).is_some_and(|l| l.building.is_none() && l.usable()));
        for g in gone_gardens {
            let members = self.social.group(g.group).map(|x| x.members.clone()).unwrap_or_default();
            for m in members {
                if let Some(c) = self.citizens.get_mut(m) {
                    c.remember(tick, "They built over our garden.".to_string());
                    c.mood = (c.mood - 0.05).max(0.0);
                }
            }
        }
        let buildings = &self.buildings;
        self.social.murals.retain(|m| m.building.is_none_or(|b| buildings.contains_key(b)) && lots.contains_key(m.lot));
    }

    /// Members free this evening go to the group's place, on its night.
    pub(super) fn group_meetings(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let meets: Vec<(u32, BuildingId, Vec<CitizenId>)> = self
            .social
            .groups
            .iter()
            .filter(|g| g.active() && mix(g.id as u64, day) % 3 == 0)
            .filter_map(|g| g.base.map(|b| (g.id, b, g.members.clone())))
            .collect();
        for (gid, base, members) in meets {
            let Some(bld) = self.buildings.get(base) else { continue };
            if !bld.serves_at(18) {
                continue;
            }
            let lu = if bld.land_use == LandUse::Park { LandUse::Park } else { LandUse::Entertainment };
            let gname = self.social.group(gid).map(|g| g.name.clone()).unwrap_or_default();
            for m in members {
                if !self.free_for_outing(m) || hash01(key64(m), tick, 0x3EE7) < 0.25 {
                    continue;
                }
                self.travel_to(m, Some(base), Purpose::Visit(lu));
                if let Some(c) = self.citizens.get_mut(m) {
                    if c.activity.is_traveling() || c.to_car.is_some() {
                        c.thought = format!("{gname} meets tonight.");
                    }
                }
            }
        }
    }

    /// Late evening: who turned up at their group's place.
    pub(super) fn tally_meetings(&mut self) {
        let day = self.clock.day();
        let mut updates: Vec<(u32, Vec<CitizenId>)> = Vec::new();
        for g in self.social.groups.iter().filter(|g| g.active()) {
            let Some(base) = g.base else { continue };
            let here: Vec<CitizenId> = g.members.iter().copied().filter(|m| self.citizens.get(*m).is_some_and(|c| matches!(c.activity, Activity::Visiting(b) | Activity::Sitting { park: b, .. } if b == base))).collect();
            if here.len() >= 2 {
                updates.push((g.id, here));
            }
        }
        let tick = self.clock.tick;
        for (gid, here) in updates {
            if let Some(g) = self.social.group_mut(gid) {
                g.momentum += 0.12 * here.len() as f32;
                g.meetings += 1;
                g.last_met = day;
            }
            // Members who meet grow closer.
            for (i, &x) in here.iter().enumerate() {
                for &y in here.iter().skip(i + 1) {
                    for (me, other) in [(x, y), (y, x)] {
                        let t = self.social.circles.entry(me).or_default().tie_or_new(other, tick, Met::Group, None);
                        t.familiarity = (t.familiarity + 0.04).min(1.0);
                        t.affinity = (t.affinity + 0.03).min(1.0);
                        t.last = tick;
                    }
                }
            }
        }
    }
}
