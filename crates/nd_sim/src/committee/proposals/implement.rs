//! Carrying a passed proposal out: projects filed, sites picked, the work
//! done, and what members remember of it.

use super::*;

impl World {
    /// File a passed proposal as a committee project: its options, the
    /// secretary's pick and reason. Zoning goes into a world project of the
    /// same name too, parked, so the map previews it dimmed like the
    /// mayor's own plans. With no options the project waits for the mayor.
    pub fn committee_file_project(&mut self, meeting: u32, proposal: &Proposal, options: Vec<SiteOption>, pick: Option<usize>, reason: String) -> u32 {
        let id = self.committee.fresh_id();
        let name = format!("Committee: {}", proposal.title);
        let pick = pick.filter(|&i| i < options.len());
        let status = if pick.is_some() { ProjectStatus::Pending } else { ProjectStatus::NeedsMayor };
        let mut p = CommitteeProject { id, name, meeting, proposal: proposal.id, by: proposal.by, action: proposal.action.clone(), options, pick, reason, status, world_project: None, log: Vec::new() };
        self.stage_zoning(&mut p);
        self.committee.projects.push(p);
        id
    }

    /// For a zoning project with a site: the parked world project holding
    /// the painted parcels (replacing an older one).
    pub(super) fn stage_zoning(&mut self, p: &mut CommitteeProject) {
        let Action::Rezone { to, .. } = &p.action else { return };
        let Some(lu) = parse_land_use(to) else { return };
        if let Some(old) = p.world_project.take() {
            self.delete_project(old);
        }
        let Some(site) = p.site().cloned() else { return };
        let ids: Vec<usize> = site.parcels.iter().filter_map(|k| self.parcels.parcels.iter().position(|q| q.key == *k)).collect();
        if ids.is_empty() {
            return;
        }
        let before = self.project.as_ref().map(|q| q.id);
        let wid = self.new_project(Some(&p.name));
        let (_, painted) = self.paint_parcels(&ids, Some(lu), None);
        match before {
            Some(b) => {
                self.switch_project(b);
            }
            None => {
                self.park_project();
            }
        }
        if painted == 0 {
            self.delete_project(wid);
            p.log.push("none of the parcels could be zoned any more".into());
            return;
        }
        p.world_project = Some(wid);
    }

    /// The mayor picks another of the options.
    pub fn committee_pick_site(&mut self, id: u32, pick: usize) -> Result<(), String> {
        let mut p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        if pick >= p.options.len() {
            return Err(format!("{} has {} options", p.name, p.options.len()));
        }
        p.pick = Some(pick);
        p.status = ProjectStatus::Pending;
        p.log.push(format!("day {}: the mayor picked option {}", self.committee_today() + 1, pick + 1));
        self.stage_zoning(&mut p);
        *self.committee.project_mut(id).unwrap() = p;
        Ok(())
    }

    /// Look for sites again (the city changed, or there were none).
    pub fn committee_refresh_sites(&mut self, id: u32) -> Result<usize, String> {
        let mut p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        p.options = site_options(self, &p.action);
        p.pick = if p.options.is_empty() { None } else { Some(0) };
        p.status = if p.pick.is_some() { ProjectStatus::Pending } else { ProjectStatus::NeedsMayor };
        p.log.push(format!("day {}: sites looked for again, {} found", self.committee_today() + 1, p.options.len()));
        self.stage_zoning(&mut p);
        let n = p.options.len();
        *self.committee.project_mut(id).unwrap() = p;
        Ok(n)
    }

    /// The mayor turns a committee project down.
    pub fn committee_dismiss_project(&mut self, id: u32) -> Result<(), String> {
        let today = self.committee_today();
        let p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        if let Some(w) = p.world_project {
            self.delete_project(w);
        }
        let q = self.committee.project_mut(id).unwrap();
        q.world_project = None;
        q.status = ProjectStatus::Dismissed { day: today };
        q.log.push(format!("day {}: dismissed by the mayor", today + 1));
        self.committee_note_outcome(&p.name, &format!("the mayor dismissed it on day {}", today + 1), -1);
        Ok(())
    }

    /// The mayor implements a committee project: the one way the
    /// committee's work reaches the city. Returns what was done.
    pub fn committee_implement(&mut self, id: u32) -> Result<String, String> {
        let today = self.committee_today();
        let p = self.committee.project(id).cloned().ok_or("no such committee project")?;
        if !p.status.is_open() {
            return Err(format!("{} is {}", p.name, p.status.key()));
        }
        let result = self.carry_out(&p);
        let q = self.committee.project_mut(id).unwrap();
        match &result {
            Ok(done) => {
                q.status = ProjectStatus::Implemented { day: today };
                q.world_project = None;
                q.log.push(format!("day {}: implemented by the mayor: {done}", today + 1));
            }
            Err(why) => {
                q.status = ProjectStatus::Failed { why: why.clone() };
                q.log.push(format!("day {}: could not implement: {why}", today + 1));
            }
        }
        if result.is_ok() {
            self.committee_note_outcome(&p.name, &format!("implemented on day {}", today + 1), 0);
        }
        result
    }

    pub(super) fn carry_out(&mut self, p: &CommitteeProject) -> Result<String, String> {
        let a = p.action.validate(self)?;
        let site = p.site().cloned();
        let need_site = || -> Result<SiteOption, String> { site.clone().ok_or_else(|| "no site picked: look for sites again or pick one".to_string()) };
        match &a {
            Action::BuildService { .. } | Action::BuildPower { .. } | Action::BuildDepot { .. } => {
                let s = need_site()?;
                let what = a.placeable().ok_or("unknown building")?;
                let pt = s.point.ok_or("the site has no point")?;
                let b = self.place_building(what, pt)?;
                let name = self.buildings.get(b).map_or_else(String::new, |b| b.name.clone());
                Ok(format!("{} \"{name}\" built on {}", a.title(), s.label))
            }
            Action::Rezone { .. } => {
                let wid = p.world_project.ok_or("the zoning plan is gone: look for sites again")?;
                let warnings = self.implement_project(wid).ok_or("the zoning plan is gone: look for sites again")?;
                if self.all_projects().any(|q| q.id == wid) {
                    return Err(warnings.first().cloned().unwrap_or_else(|| "the zoning was refused".into()));
                }
                Ok(format!("zoned {} parcels{}", site.map_or(0, |s| s.parcels.len()), if warnings.is_empty() { String::new() } else { format!(" ({})", warnings.join("; ")) }))
            }
            Action::AddBusLine { .. } => {
                let s = need_site()?;
                let route = self.add_bus_route();
                let mut added = 0;
                for q in &s.stops {
                    if self.add_bus_stop(route, *q).is_ok() {
                        added += 1;
                    }
                }
                if added < 2 {
                    self.remove_bus_route(route);
                    return Err("the stops no longer sit by a street".into());
                }
                let short = p.name.trim_start_matches("Committee: ").to_string();
                let _ = self.rename_bus_route(route, &short);
                self.set_route_buses(route, 2);
                Ok(format!("bus line \"{short}\" with {added} stops and 2 buses"))
            }
            Action::ChangeTax { category, delta_pct } => {
                let mut done = Vec::new();
                for c in tax_categories(category) {
                    let now = self.economy.taxes.rate(c);
                    let set = self.set_tax_rate(c, now + delta_pct / 100.0);
                    done.push(format!("{} {:.1}% -> {:.1}%", c.label(), now * 100.0, set * 100.0));
                }
                Ok(done.join(", "))
            }
            Action::SetToll { toll, price } => {
                let ids: Vec<u32> = match toll {
                    Some(id) => vec![*id],
                    None => self.tolls.booths.iter().map(|b| b.id).collect(),
                };
                for id in &ids {
                    self.set_toll_price(*id, *price)?;
                }
                Ok(format!("{} toll booth(s) at {price:.0} credits", ids.len()))
            }
            Action::RoadRepair { funding } => {
                let set = self.set_road_funding(*funding);
                Ok(format!("road maintenance funding at {:.0}%", set * 100.0))
            }
            Action::Remediate { .. } => {
                let s = need_site()?;
                let (cost, amount) = self.remediate(s.point.ok_or("the site has no point")?, s.radius.max(40.0))?;
                Ok(format!("{amount:.1} cells of ground cleaned for {} credits", thousands(cost)))
            }
        }
    }

    /// Tell the members who voted on `name` what came of it.
    pub(super) fn committee_note_outcome(&mut self, project_name: &str, outcome: &str, verdict: i8) {
        let title = project_name.trim_start_matches("Committee: ");
        for p in self.committee.pool.iter_mut() {
            if let Some(m) = p.memory.iter_mut().rev().find(|m| m.proposal == title) {
                m.outcome = outcome.to_string();
                if verdict != 0 {
                    m.verdict = verdict;
                }
            }
        }
    }
}

impl Committee {
    /// After the votes: every seated member remembers how they voted and
    /// whether it passed, with their scope's headline figure to judge it by
    /// later; the record counts go up.
    pub fn remember_votes(&mut self, meeting: u32, day: u32, proposals: &[Proposal], report: &CityReport) {
        for prop in proposals {
            let passed = prop.outcome == Some(crate::committee::Outcome::Passed);
            if let Some(m) = self.member_mut(prop.by) {
                m.record.proposed += 1;
                if passed {
                    m.record.proposals_passed += 1;
                }
            }
            for b in &prop.ballots {
                let Some(m) = self.member_mut(b.member) else { continue };
                match b.vote {
                    Vote::Yes => m.record.votes_yes += 1,
                    Vote::No => m.record.votes_no += 1,
                    Vote::Abstain => m.record.abstained += 1,
                }
                let key = CityReport::scope_metric(m.scope);
                let entry = MemoryEntry { day, meeting, proposal: prop.title.clone(), kind: prop.action.kind_key().into(), vote: Some(b.vote), passed, proposed: prop.by == b.member, metric: report.metric(key).map(|v| (key.to_string(), v)), outcome: String::new(), verdict: 0 };
                m.remember(entry);
            }
        }
    }

    /// At a meeting's start: judge what members remember by how their
    /// figures moved since ("crime down 18% since").
    pub fn judge_memories(&mut self, report: &CityReport) {
        for p in self.pool.iter_mut() {
            for m in p.memory.iter_mut() {
                let Some((key, then)) = m.metric.clone() else { continue };
                let Some(now) = report.metric(&key) else { continue };
                if m.day >= report.day {
                    continue;
                }
                let d = now - then;
                let rel = if then.abs() > 1e-3 { d / then.abs() } else { d };
                let label = key.replace('_', " ");
                let moved = if rel.abs() < 0.03 { format!("{label} flat since") } else { format!("{label} {} {:.0}% since", if d > 0.0 { "up" } else { "down" }, rel.abs() * 100.0) };
                let good = (d > 0.0) == CityReport::higher_is_better(&key);
                let base = m.outcome.split(';').next().unwrap_or("").trim().to_string();
                let base = if base.contains(" since") { String::new() } else { base };
                m.outcome = if base.is_empty() { moved } else { format!("{base}; {moved}") };
                if m.passed && m.verdict == 0 && rel.abs() >= 0.03 {
                    m.verdict = if good { 1 } else { -1 };
                }
            }
        }
    }
}
