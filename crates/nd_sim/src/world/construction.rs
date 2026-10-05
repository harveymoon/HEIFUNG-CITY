//! Building sites: crews, materials and opening day.

use super::*;

impl World {
    /// Every sim hour: building sites advance, wait for materials at each
    /// quarter, and open when the work is done.
    pub(super) fn step_construction(&mut self) {
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for bid in ids {
            let (order, waiting, before, after) = {
                let c = self.buildings[bid].construction.as_mut().unwrap();
                let before = c.progress;
                // One load is kept on the road ahead of the work, so a
                // quarter only stalls when its truck is late.
                let order = !c.ordered && c.deliveries < Construction::DELIVERIES && c.deliveries <= c.stage() + 1;
                if c.waiting_for_materials() {
                    (order, true, before, before)
                } else {
                    let ceiling = (c.stage() as f32 + 1.0) * 0.25;
                    c.progress = (c.progress + c.hourly_step()).min(ceiling);
                    (order, false, before, c.progress)
                }
            };
            if order {
                self.order_materials(bid);
            }
            if waiting {
                continue;
            }
            if after >= 1.0 {
                self.finish_construction(bid);
            } else if (after * 10.0).floor() > (before * 10.0).floor() {
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
    }

    /// A truckload of parts for a building site: from a local fab when one
    /// has stock, else imported. The developer pays, so the money comes from
    /// outside the city like export income does.
    pub(super) fn order_materials(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        let Some(lot) = self.lots.get(b.lot) else { return };
        let at = lot.center;
        let amount = 4.0 + b.floors as f32 * 2.0;
        let resource = Resource::Parts;
        if let Some(c) = self.buildings[bid].construction.as_mut() {
            c.ordered = true;
        }
        let seller = self.best_seller(resource, at, amount, bid);
        if let Some((seller_id, price)) = seller {
            let got = self.buildings[seller_id].business.as_mut().unwrap().take(resource, amount);
            if got > 0.0 {
                let cost = got * price;
                let sb = self.buildings[seller_id].business.as_mut().unwrap();
                sb.money += cost;
                sb.revenue_today += cost;
                sb.deals += 1;
                self.economy.market.deals_today += 1;
                self.ship(seller_id, bid, resource, got, cost);
                return;
            }
        }
        let cost = amount * resource.base_price() * IMPORT_MARKUP;
        self.economy.materials_today += cost;
        self.import(bid, resource, amount, 0.0);
    }

    /// Open a finished building: the real homes and jobs appear, the site
    /// crew is let go (and steps outside if it was on site). Public so tests
    /// and scripts can skip the wait.
    pub fn finish_construction(&mut self, bid: BuildingId) {
        let tick = self.clock.tick;
        let (car_access, entrance) = match self.buildings.get(bid).and_then(|b| self.lots.get(b.lot)) {
            Some(l) => (l.car_access(), l.entrance()),
            None => (true, Vec2::ZERO),
        };
        let Some(b) = self.buildings.get_mut(bid) else { return };
        if b.construction.take().is_none() {
            return;
        }
        let crew = std::mem::take(&mut b.workers);
        b.recompute_capacity();
        if !car_access {
            b.parking_capacity = 0;
        }
        b.built_tick = tick;
        let name = b.name.clone();
        for cid in crew {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(bid)) {
                self.lay_off(cid, Some(format!("Topped out {name}. Crew's laid off; looking for the next site.")));
            }
            let Some(c) = self.citizens.get_mut(cid) else { continue };
            if c.inside == Some(bid) {
                c.inside = None;
                c.pos = entrance;
                c.prev_pos = entrance;
                c.activity = Activity::Idle;
                c.next_decision_tick = tick + 1;
            }
        }
        self.events.push(SimEvent::BuildingChanged(bid));
    }
}
