//! [social] What ties do over days: fading, friends helping out, loans,
//! moving in together.

use super::*;

impl World {
    /// Ties fade without contact; a couple living together keeps in touch.
    pub(in crate::world) fn fade_ties(&mut self) {
        let tick = self.clock.tick;
        let hh: BTreeMap<CitizenId, Option<HouseholdId>> = self.social.circles.keys().map(|id| (*id, self.citizens.get(*id).and_then(|c| c.household))).collect();
        let mut broke_up: Vec<(CitizenId, CitizenId)> = Vec::new();
        for (id, c) in self.social.circles.iter_mut() {
            let mine = hh.get(id).copied().flatten();
            for t in c.ties.iter_mut() {
                let together = mine.is_some() && self.citizens.get(t.who).is_some_and(|x| x.household == mine);
                if together {
                    t.last = tick;
                    continue;
                }
                let days = tick.saturating_sub(t.last) / 1440;
                if days >= 2 {
                    t.familiarity *= 0.97;
                }
                if days >= 5 {
                    t.affinity *= if t.affinity < 0.0 { 0.985 } else { 0.97 };
                    t.spark *= 0.8;
                }
                if t.partner && days >= 6 && t.affinity < 0.35 {
                    t.partner = false;
                    broke_up.push((*id, t.who));
                }
            }
            // A word exchanged once is forgotten in a few days unless it
            // left a mark; known faces fade slowly.
            c.ties.retain(|t| t.partner || t.affinity.abs() >= 0.15 || if t.talks <= 1 { tick < t.last + 3 * 1440 } else { t.familiarity >= 0.05 });
        }
        for (a, b) in broke_up {
            if let Some(t) = self.social.circles.get_mut(&b).and_then(|c| c.tie_mut(a)) {
                t.partner = false;
            }
            let other = self.citizens.get(b).map(|c| c.name.clone()).unwrap_or_default();
            if let Some(c) = self.citizens.get_mut(a) {
                c.remember(tick, format!("{other} and I drifted apart."));
            }
        }
    }

    /// Friends put in a word: a job where they work, a flat where they live.
    pub(in crate::world) fn friends_help(&mut self) {
        let tick = self.clock.tick;
        // Jobs.
        let jobless: Vec<CitizenId> = self.social.circles.keys().copied().filter(|id| self.citizens.get(*id).is_some_and(|c| c.work.is_none() && c.can_work())).collect();
        for id in jobless {
            let ties: Vec<(f32, CitizenId)> = self.social.circles[&id].ties.iter().filter(|t| t.bond().is_friendly() && t.trust >= 0.3).map(|t| (t.affinity, t.who)).collect();
            let mut best: Option<(f32, BuildingId, CitizenId)> = None;
            for (aff, f) in ties {
                let Some(w) = self.citizens.get(f).and_then(|c| c.work) else { continue };
                let Some(b) = self.buildings.get(w) else { continue };
                if b.free_jobs() == 0 || b.business.as_ref().is_some_and(|z| z.closed) || !b.operating() {
                    continue;
                }
                if best.is_none_or(|x| aff > x.0) {
                    best = Some((aff, w, f));
                }
            }
            if let Some((_, w, f)) = best {
                self.assign_job(id, w);
                let (fname, bname) = (self.citizens[f].name.clone(), self.buildings[w].name.clone());
                self.citizens[id].remember(tick, format!("{fname} put in a word for me at {bname}."));
                if let Some(t) = self.social.circles.get_mut(&id).and_then(|c| c.tie_mut(f)) {
                    t.trust = (t.trust + 0.1).min(1.0);
                    t.affinity = (t.affinity + 0.08).min(1.0);
                }
                self.social.today.jobs_found += 1;
            }
        }
        // Flats: a household squeezed by the rent hears of somewhere
        // cheaper from a friend who lives there.
        let squeezed: Vec<HouseholdId> = self.households.iter().filter(|(_, h)| h.rent_stress_days >= 2 && h.home.is_some() && h.moved_day < self.clock.day()).map(|(id, _)| id).collect();
        for hid in squeezed {
            let Some(hh) = self.households.get(hid) else { continue };
            let Some(cur) = hh.home.and_then(|h| self.buildings.get(h)) else { continue };
            let cur_rent = cur.rent;
            let income = self.household_income(hid);
            let members = hh.members.clone();
            let mut found: Option<(BuildingId, CitizenId)> = None;
            'm: for m in &members {
                let Some(c) = self.social.circles.get(m) else { continue };
                for t in c.ties.iter().filter(|t| t.bond().is_friendly()) {
                    let Some(h) = self.citizens.get(t.who).and_then(|x| x.home) else { continue };
                    let Some(b) = self.buildings.get(h) else { continue };
                    if Some(h) != hh.home && b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0 && b.rent < cur_rent * 0.9 && b.rent <= income * trips::RENT_STRESS_SHARE {
                        found = Some((h, t.who));
                        break 'm;
                    }
                }
            }
            if let Some((h, f)) = found {
                let fname = self.citizens[f].name.clone();
                self.move_household(hid, h, &format!("Moved to {{}}: {fname} told us about a cheaper flat."));
                self.social.today.homes_found += 1;
            }
        }
    }

    /// The money a citizen can lend or must borrow against: their
    /// household's purse (pockets are pooled into it every midnight, before
    /// this runs), or their own savings when they have no household.
    pub(super) fn loan_purse(&self, id: CitizenId) -> Option<(Option<HouseholdId>, f32)> {
        let c = self.citizens.get(id)?;
        Some(match c.household.and_then(|h| self.households.get(h).map(|x| (h, x.money))) {
            Some((h, m)) => (Some(h), m),
            None => (None, c.savings),
        })
    }

    pub(super) fn add_to_loan_purse(&mut self, id: CitizenId, amount: f32) {
        let Some(c) = self.citizens.get_mut(id) else { return };
        match c.household.and_then(|h| self.households.get_mut(h)) {
            Some(h) => h.money += amount,
            None => c.savings += amount,
        }
    }

    /// The broke borrow from friends who trust them; debts are paid back
    /// when they can be, and a debt left too long sours the friendship.
    /// [review] Money is read from the household purse: `daily_household`
    /// pools every pocket into it first, so pockets alone never qualified.
    pub(in crate::world) fn loans(&mut self) {
        let tick = self.clock.tick;
        let broke: Vec<CitizenId> = self.social.circles.keys().copied().filter(|id| self.citizens.get(*id).is_some_and(|c| c.age >= 18) && self.loan_purse(*id).is_some_and(|p| p.1 < -10.0)).collect();
        for id in broke {
            // A household member may already have borrowed for it.
            let Some((own_hh, purse)) = self.loan_purse(id) else { continue };
            if purse >= -10.0 {
                continue;
            }
            let need = (-purse + 20.0).min(120.0);
            let lender = self.social.circles[&id]
                .ties
                .iter()
                .filter(|t| t.bond().is_friendly())
                .filter_map(|t| {
                    let trust = self.social.circles.get(&t.who).and_then(|c| c.tie(id)).map_or(0.0, |x| if x.lent > 0.0 { 0.0 } else { x.trust });
                    let (their_hh, money) = self.loan_purse(t.who)?;
                    let same = own_hh.is_some() && their_hh == own_hh;
                    (!same && trust >= 0.4 && money > need + 100.0).then_some((t.who, money))
                })
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|x| x.0);
            let Some(l) = lender else { continue };
            self.add_to_loan_purse(l, -need);
            self.add_to_loan_purse(id, need);
            if let Some(t) = self.social.circles.get_mut(&l).and_then(|c| c.tie_mut(id)) {
                t.lent += need;
            }
            let (ln, bn) = (self.citizens[l].name.clone(), self.citizens[id].name.clone());
            self.citizens[id].remember(tick, format!("{ln} lent me ${need:.0}. I owe them."));
            self.citizens[l].remember(tick, format!("Lent {bn} ${need:.0}."));
            self.social.today.loans += 1;
        }
        // Paying back.
        let debts: Vec<(CitizenId, CitizenId, f32, u64)> = self.social.circles.iter().flat_map(|(l, c)| c.ties.iter().filter(|t| t.lent > 0.0).map(move |t| (*l, t.who, t.lent, t.last))).collect();
        for (l, b, owed, _) in debts {
            let Some((_, money)) = self.loan_purse(b) else { continue };
            if money > owed + 60.0 {
                self.add_to_loan_purse(b, -owed);
                self.add_to_loan_purse(l, owed);
                if let Some(t) = self.social.circles.get_mut(&l).and_then(|c| c.tie_mut(b)) {
                    t.lent = 0.0;
                    t.trust = (t.trust + 0.1).min(1.0);
                }
                let ln = self.citizens.get(l).map(|c| c.name.clone()).unwrap_or_default();
                self.citizens[b].remember(tick, format!("Paid {ln} back."));
            } else if hash01(key64(l) ^ key64(b), self.clock.day(), 0xDE87) < 0.1 {
                if let Some(t) = self.social.circles.get_mut(&l).and_then(|c| c.tie_mut(b)) {
                    t.trust = (t.trust - 0.1).max(0.0);
                    t.affinity = (t.affinity - 0.08).max(-1.0);
                }
            }
        }
    }

    /// A partner living alone moves in with the other, if the home has room
    /// in the household (five at most).
    pub(in crate::world) fn move_in_together(&mut self) {
        let tick = self.clock.tick;
        let couples: Vec<(CitizenId, CitizenId)> = self.social.circles.iter().filter_map(|(a, c)| c.partner().map(|b| (*a, b))).filter(|(a, b)| a < b).collect();
        for (a, b) in couples {
            let (Some(ca), Some(cb)) = (self.citizens.get(a), self.citizens.get(b)) else { continue };
            let (Some(ha), Some(hb)) = (ca.household, cb.household) else { continue };
            if ha == hb {
                continue;
            }
            let together = self.social.circles.get(&a).and_then(|c| c.tie(b)).is_some_and(|t| t.affinity >= 0.6 && tick.saturating_sub(t.since) >= 2 * 1440);
            if !together {
                continue;
            }
            let size = |h: HouseholdId| self.households.get(h).map_or(9, |x| x.size());
            // The one living alone moves.
            let (mover, into) = if size(ha) == 1 && size(hb) < 5 {
                (a, hb)
            } else if size(hb) == 1 && size(ha) < 5 {
                (b, ha)
            } else {
                continue;
            };
            if self.move_into_household(mover, into) {
                self.social.today.moved_in += 1;
                let home = self.households.get(into).and_then(|h| h.home).and_then(|h| self.buildings.get(h)).map(|b| b.name.clone()).unwrap_or_default();
                let other = if mover == a { b } else { a };
                let on = self.citizens[other].name.clone();
                self.citizens[mover].remember(tick, format!("Moved in with {on} at {home}."));
                let mn = self.citizens[mover].name.clone();
                self.citizens[other].remember(tick, format!("{mn} moved in with us."));
            }
        }
    }

    /// `cid` (alone in their household) joins household `into`: their home
    /// is let go, their money comes along.
    pub(super) fn move_into_household(&mut self, cid: CitizenId, into: HouseholdId) -> bool {
        let Some(old) = self.citizens.get(cid).and_then(|c| c.household) else { return false };
        if self.households.get(old).is_none_or(|h| h.size() != 1) || !self.properties_of(cid).is_empty() {
            return false;
        }
        let Some(new_home) = self.households.get(into).and_then(|h| h.home) else { return false };
        if !self.buildings.contains_key(new_home) {
            return false;
        }
        let (money, groceries, old_home) = {
            let h = &self.households[old];
            (h.money, h.groceries, h.home)
        };
        let at_home = self.citizens[cid].inside.is_some() && self.citizens[cid].inside == old_home;
        if at_home {
            self.leave_building(cid);
            self.citizens[cid].activity = Activity::Idle;
        }
        if let Some(b) = old_home.and_then(|h| self.buildings.get_mut(h)) {
            b.residents.retain(|x| *x != cid);
        }
        self.remove_household(old);
        let h = &mut self.households[into];
        h.members.push(cid);
        h.money += money;
        h.groceries += groceries;
        self.buildings[new_home].residents.push(cid);
        let c = &mut self.citizens[cid];
        c.household = Some(into);
        c.home = Some(new_home);
        c.rent_stress_days = 0;
        if at_home {
            self.travel_to(cid, Some(new_home), Purpose::Home);
        }
        true
    }
}
