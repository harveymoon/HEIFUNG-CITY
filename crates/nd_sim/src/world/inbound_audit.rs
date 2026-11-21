//! [r12-hunger] Inbound bookings no load answers.
//!
//! A buyer counts what it ordered as inbound (`World::note_inbound`) so it
//! does not order twice, and a delivery takes it off (`World::deliver`). A
//! load lost on the way (a lorry taken off the road, a depot's job dropped,
//! a quay or hub gone) left its booking standing for good, and the buyer
//! never ordered that input again. On Harvey's day-80 city 129 of 203
//! grocers had 5,100 units of produce, flour, meat and milk booked that no
//! lorry, train, ship, hub or depot held: they stood with bare shelves,
//! making a sixth of the groceries their staff could, and half the town
//! went hungry.
//!
//! Every [`AUDIT_HOURS`] hours the loads on their way (queued lorries,
//! lorries on the road, rail orders, trains and yard aprons, ships ordered
//! and at sea, island ferries, cargo hubs' stages, airports, private quays,
//! container lorries' jobs) are summed by buyer and resource. What a buyer
//! has booked beyond them is unanswered; what was unanswered at the last
//! audit too is written off (the smaller of the two), so a load caught
//! between two places at the moment of one audit is never lost.
//!
//! A cargo hub's rail and sea loads had no such end either: the load the
//! hub expected stood in its books for good when the goods never came (the
//! same day-80 city's two hubs expected 470 loads, 7,800 units, with 40 on
//! their way; the rest aged past five days in the run). A load expected
//! [`HUB_GIVE_UP_HOURS`] hours comes by road from the edge of the map to its
//! buyer instead, as a rail order or an airport's load does after its wait.

use super::*;

/// Hours between audits.
pub const AUDIT_HOURS: u64 = 6;
/// Hours a cargo hub waits for a rail or sea load before it comes by road.
pub const HUB_GIVE_UP_HOURS: u64 = 48;

impl World {
    /// Every load on its way to a buyer, by buyer and resource.
    pub(super) fn loads_on_the_way(&self) -> HashMap<(BuildingId, Resource), f32> {
        let mut out: HashMap<(BuildingId, Resource), f32> = HashMap::new();
        let mut add = |to: BuildingId, r: Resource, amount: f32| {
            if amount > 0.0 {
                *out.entry((to, r)).or_default() += amount;
            }
        };
        for q in &self.freight_queue {
            add(q.cargo.to, q.cargo.resource, q.cargo.amount);
        }
        for c in self.traffic.cars.values() {
            if let Some(c) = &c.cargo {
                add(c.to, c.resource, c.amount);
            }
        }
        for o in &self.rail.orders {
            add(o.cargo.to, o.cargo.resource, o.cargo.amount);
        }
        for t in self.rail.trains.values() {
            for c in &t.cargo {
                add(c.to, c.resource, c.amount);
            }
        }
        for b in self.buildings.values() {
            for c in &b.rail_inbound {
                add(c.to, c.resource, c.amount);
            }
        }
        for (_, c) in &self.ports.inbound {
            add(c.to, c.resource, c.amount);
        }
        for s in self.ports.ships.values() {
            for c in &s.cargo {
                add(c.to, c.resource, c.amount);
            }
        }
        for l in &self.ports.ferry_freight {
            add(l.cargo.to, l.cargo.resource, l.cargo.amount);
        }
        for h in self.cargo_hubs.hubs.values() {
            for l in h.waiting.iter().chain(&h.trunk).chain(&h.inbound).chain(&h.ready) {
                add(l.buyer, l.resource, l.amount);
            }
        }
        for a in self.airports.airports.values() {
            for l in a.cargo_waiting.iter().chain(&a.cargo_landed) {
                add(l.cargo.to, l.cargo.resource, l.cargo.amount);
            }
        }
        for (_, c, _) in &self.shore.inbound {
            add(c.to, c.resource, c.amount);
        }
        for j in self.containers.lorries.values() {
            if let Some(c) = &j.cargo {
                add(c.to, c.resource, c.amount);
            }
        }
        out
    }

    /// Write off the bookings unanswered at this audit and the last (the
    /// smaller amount); remember this audit's for the next. Returns the
    /// units written off.
    pub(super) fn audit_inbound(&mut self) -> f32 {
        let found = self.loads_on_the_way();
        let last: HashMap<(BuildingId, Resource), f32> = std::mem::take(&mut self.inbound_unanswered).into_iter().map(|(b, r, a)| ((b, r), a)).collect();
        let mut now: Vec<(BuildingId, Resource, f32)> = Vec::new();
        let mut written = 0.0;
        for (bid, b) in self.buildings.iter_mut() {
            let Some(bz) = b.business.as_mut() else { continue };
            for (r, booked) in bz.inbound.iter_mut() {
                let missing = *booked - found.get(&(bid, *r)).copied().unwrap_or(0.0);
                if missing < 0.5 {
                    continue;
                }
                let off = missing.min(last.get(&(bid, *r)).copied().unwrap_or(0.0));
                if off > 0.0 {
                    *booked = (*booked - off).max(0.0);
                    written += off;
                }
                let left = missing - off;
                if left >= 0.5 {
                    now.push((bid, *r, left));
                }
            }
        }
        self.inbound_unanswered = now;
        self.economy.market.food.today.written_off += written;
        written
    }

    /// Rail and sea loads a hub has waited on too long come by road from
    /// the edge of the map to their buyers. Returns how many.
    pub(super) fn give_up_hub_loads(&mut self) -> usize {
        let tick = self.clock.tick;
        let mut late: Vec<crate::cargo_hub::HubLoad> = Vec::new();
        for st in self.cargo_hubs.hubs.values_mut() {
            let (old, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut st.inbound).into_iter().partition(|l| tick.saturating_sub(l.since) >= HUB_GIVE_UP_HOURS * 60);
            st.inbound = keep;
            late.extend(old);
        }
        let n = late.len();
        for l in late {
            if self.buildings.contains_key(l.buyer) {
                self.dispatch_truck(super::market::TruckOrigin::CityEdge, Cargo { from: BuildingId::default(), to: l.buyer, resource: l.resource, amount: l.amount, price: l.price }, 6);
            }
        }
        n
    }
}
