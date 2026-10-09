//! Crews: hiring for the stations and balancing the shifts.

use super::*;

impl World {
    /// Services hire: each with open jobs takes up to two people an hour,
    /// the jobless first, else someone on a lower wage elsewhere (a public
    /// job pays well), so a fully employed town still staffs its stations.
    pub(super) fn recruit_staff(&mut self) {
        let posts: Vec<(BuildingId, f32)> = self.buildings.iter().filter(|(_, b)| b.service.is_some() && b.operating() && !b.is_derelict() && b.free_jobs() > 0).map(|(id, b)| (id, b.wage())).collect();
        for (bid, wage) in posts {
            for _ in 0..2 {
                if self.buildings[bid].free_jobs() == 0 {
                    break;
                }
                // Round-the-clock posts hire for the thinnest shift first.
                let want = self.thinnest_shift(bid).map(|(s, _)| s);
                let b = &self.buildings[bid];
                let jobless = self
                    .citizens
                    .iter()
                    .filter(|(_, c)| c.work.is_none() && c.can_work())
                    .find(|(_, c)| want.is_none_or(|w| b.shift_for(c.seed).open == w))
                    .or_else(|| self.citizens.iter().find(|(_, c)| c.work.is_none() && c.can_work()))
                    .map(|(id, _)| id);
                let poach = || {
                    self.citizens
                        .iter()
                        .filter(|(_, c)| c.can_work())
                        .filter_map(|(id, c)| c.work.and_then(|w| self.buildings.get(w)).filter(|w| !w.is_special() && w.operating() && w.wage() < wage).map(|_| id))
                        .next()
                };
                let Some(cid) = jobless.or_else(poach) else { break };
                if let Some(old) = self.citizens[cid].work.and_then(|w| self.buildings.get_mut(w)) {
                    old.workers.retain(|x| *x != cid);
                }
                self.assign_job(cid, bid);
            }
        }
    }

    /// For a round-the-clock emergency post: the shift (by its opening hour)
    /// with the fewest workers, and how many it has. None for other buildings.
    pub(super) fn thinnest_shift(&self, bid: BuildingId) -> Option<(u8, usize)> {
        let b = self.buildings.get(bid)?;
        let s = b.service?;
        if !matches!(s.kind, ServiceKind::Police | ServiceKind::Fire | ServiceKind::Health | ServiceKind::Dispatch | ServiceKind::Jail) {
            return None;
        }
        let shifts: Vec<u8> = (0..3u64).map(|k| s.kind.hours().shift(k).open).collect();
        shifts.iter().map(|&open| (open, b.workers.iter().filter(|w| self.citizens.get(**w).is_some_and(|c| b.shift_for(c.seed).open == open)).count())).min_by_key(|x| x.1)
    }

    /// Emergency posts keep every shift manned: once an hour, a post with an
    /// empty shift and a doubled-up one swaps a worker with someone whose
    /// hours fit the empty shift, from an ordinary job paying less (they take
    /// the public job, ours takes theirs). Hired at random, a 911 centre of
    /// three could leave the evening with nobody at the desk.
    pub(super) fn balance_rosters(&mut self) {
        let posts: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.service.is_some() && b.operating() && !b.is_derelict() && b.workers.len() >= 3).map(|(id, _)| id).collect();
        for bid in posts {
            let Some((want, 0)) = self.thinnest_shift(bid) else { continue };
            let b = &self.buildings[bid];
            let wage = b.wage();
            // Ordered, so ties between equal shifts break the same way every run.
            let mut by_shift: std::collections::BTreeMap<u8, Vec<CitizenId>> = std::collections::BTreeMap::new();
            for &w in &b.workers {
                if let Some(c) = self.citizens.get(w) {
                    by_shift.entry(b.shift_for(c.seed).open).or_default().push(w);
                }
            }
            let Some(spare) = by_shift.values().filter(|v| v.len() >= 2).max_by_key(|v| v.len()).and_then(|v| v.last().copied()) else { continue };
            let cand = self
                .citizens
                .iter()
                .filter(|(_, c)| c.can_work() && b.shift_for(c.seed).open == want)
                .filter_map(|(id, c)| c.work.filter(|w| self.buildings.get(*w).is_some_and(|x| !x.is_special() && x.operating() && x.wage() < wage)).map(|w| (id, w)))
                .next();
            let Some((cid, old)) = cand else { continue };
            self.buildings[bid].workers.retain(|x| *x != spare);
            self.buildings[old].workers.retain(|x| *x != cid);
            self.assign_job(cid, bid);
            self.assign_job(spare, old);
        }
    }
}
