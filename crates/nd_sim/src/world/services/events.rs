//! Daily civic events: children to school, stadium nights.

use super::*;

impl World {
    /// School-age children at home or about go to the nearest school whose
    /// district covers their home and that has places left.
    pub(super) fn send_children_to_school(&mut self) {
        let tick = self.clock.tick;
        let mut schools: Vec<(BuildingId, Vec2, u32, LandUse)> = self
            .buildings
            .iter()
            .filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::School).map(|s| (id, b, s)))
            .filter(|(_, b, _)| !b.is_derelict() && b.operating())
            .filter_map(|(id, b, s)| self.lots.get(b.lot).map(|l| (id, l.center, s.kind.capacity(s.tier), b.land_use)))
            .collect();
        if schools.is_empty() {
            return;
        }
        let kids: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.goes_to_school() && matches!(c.activity, Activity::AtHome | Activity::Idle) && c.home.is_some()).map(|(id, _)| id).collect();
        for cid in kids {
            if self.is_patient(cid) {
                continue;
            }
            let Some(home_lot) = self.citizens[cid].home.and_then(|h| self.buildings.get(h)).map(|b| b.lot) else { continue };
            if self.services.coverage_of(home_lot, ServiceKind::School) <= 0.0 {
                continue;
            }
            let at = self.lots.get(home_lot).map_or(Vec2::ZERO, |l| l.center);
            let Some(k) = (0..schools.len()).filter(|&k| schools[k].2 > 0).min_by(|&a, &b| schools[a].1.distance(at).total_cmp(&schools[b].1.distance(at))) else { break };
            schools[k].2 -= 1;
            let (sid, lu) = (schools[k].0, schools[k].3);
            let name = self.buildings[sid].name.clone();
            let c = &mut self.citizens[cid];
            c.asleep = false;
            c.remember(tick, format!("Off to {name} for the day."));
            self.services.stats.pupils_today += 1;
            // [passengers] Driven there by a parent now and then.
            if !self.school_run(cid, sid, Purpose::Visit(lu)) {
                self.travel_to(cid, Some(sid), Purpose::Visit(lu));
            }
        }
    }

    pub(super) fn start_stadium_events(&mut self) {
        let tick = self.clock.tick;
        let stadiums: Vec<BuildingId> = self.services.events_today.iter().copied().filter(|b| self.buildings.get(*b).is_some_and(|b| !b.is_derelict() && b.operating())).collect();
        for sid in stadiums {
            let (seats, lu, name, at) = {
                let b = &self.buildings[sid];
                let s = b.service.unwrap();
                (s.kind.capacity(s.tier), b.land_use, b.name.clone(), self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center))
            };
            let mut fans: Vec<CitizenId> = self
                .citizens
                .iter()
                .filter(|(_, c)| matches!(c.activity, Activity::AtHome | Activity::Idle) && c.savings - TICKET_PRICE > OVERDRAFT && c.pos.distance(at) <= EVENT_REACH)
                .map(|(id, _)| id)
                .filter(|id| !self.is_patient(*id))
                .collect();
            fans.shuffle(self.srng());
            let take = ((seats as f32 * self.srng().random_range(0.5..0.95)) as usize).min(fans.len());
            for &cid in &fans[..take] {
                self.pay_city(Account::Person(cid), Flow::Tickets, TICKET_PRICE, Some(Item::Building(sid)));
                let c = &mut self.citizens[cid];
                c.remember(tick, format!("Got a ticket for tonight's game at {name}."));
                c.thought = "Game night!".to_string();
                self.services.stats.tickets_today += TICKET_PRICE;
                self.travel_to(cid, Some(sid), Purpose::Visit(lu));
            }
            self.services.stats.stadium_crowd_today += take as u32;
            self.events.push(SimEvent::StadiumEvent(sid));
        }
    }
}
