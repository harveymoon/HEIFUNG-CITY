//! Fires: burning, gutting and burning down, and the city repairing its own
//! structures.

use super::*;

impl World {
    /// [services] What one of the city's own structures cost to put up
    /// (the base for its repair bill): what it was placed as, a plant at its
    /// site's scale. [review] Batteries and city car parks fell through to
    /// a cargo hub's price, plants to their unscaled one.
    pub(super) fn civic_value(&self, id: BuildingId) -> f32 {
        let Some(b) = self.buildings.get(id) else { return 0.0 };
        if let Some(k) = b.plant {
            return k.cost_at(self.plant_scale(b));
        }
        self.placed_as(id).map_or(0.0, |p| p.cost())
    }

    /// [services] Midnight: the city mends its own structures (stations,
    /// plants, depots, docks...), `REPAIR_PER_DAY` of condition a day at
    /// `REPAIR_SHARE` of the build price, while the treasury can pay. They
    /// do not decay with age (`daily_update`: their upkeep keeps them), so
    /// this is fire damage: a gutted station is out of service (derelict)
    /// until the repairs bring it past that line.
    pub(super) fn repair_civic(&mut self) {
        let jobs: Vec<(BuildingId, f32, f32, String)> = self
            .buildings
            .iter()
            .filter(|(id, b)| Self::is_civic_structure(b) && b.operating() && b.condition < 1.0 && !self.services.is_burning(*id))
            .map(|(id, b)| {
                let amount = REPAIR_PER_DAY.min(1.0 - b.condition);
                (id, amount, amount * REPAIR_SHARE * self.civic_value(id), b.name.clone())
            })
            .collect();
        for (id, amount, cost, name) in jobs {
            if self.economy.treasury < cost {
                continue;
            }
            self.economy.debit_item(Flow::ServiceUpkeep, cost, Item::Named(format!("repairs to {name}")));
            let b = &mut self.buildings[id];
            let was = b.is_derelict();
            b.condition = (b.condition + amount).min(1.0);
            if was != b.is_derelict() {
                self.events.push(SimEvent::BuildingChanged(id));
            }
        }
    }

    /// One tick of a fire: growth (or knock-down with an engine there),
    /// damage, spread and burning down. True when the incident ended.
    pub(super) fn step_fire(&mut self, id: u32) -> bool {
        let tick = self.clock.tick;
        // [fire-scene] Each engine at work knocks it down (a fire from out
        // of town has its one engine).
        let engines = self.engines_on_scene(id).max(1) as f32;
        self.maybe_second_alarm(id);
        let (bid, fighting, intensity) = {
            let i = self.services.incident_mut(id).unwrap();
            let fighting = i.stage == Stage::OnScene;
            if fighting {
                i.intensity -= ENGINE_RATE * engines;
            } else {
                i.intensity = (i.intensity + FIRE_GROWTH).min(1.0);
            }
            (i.building, fighting, i.intensity)
        };
        if intensity <= 0.0 {
            self.services.incident_mut(id).unwrap().intensity = 0.0;
            self.resolve(id, Outcome::Extinguished);
            return true;
        }
        // [services] The city's own structures are gutted, never razed:
        // their condition stops at the burned-down line.
        let civic = Self::is_civic_structure(&self.buildings[bid]);
        let (before, after) = {
            let b = &mut self.buildings[bid];
            let before = b.condition;
            b.condition = (b.condition - FIRE_DAMAGE * intensity * if fighting { 0.3 } else { 1.0 }).max(if civic { BURNED_DOWN } else { 0.0 });
            (before, b.condition)
        };
        if (before * 10.0).floor() != (after * 10.0).floor() {
            self.events.push(SimEvent::BuildingChanged(bid));
        }
        let bname = self.buildings[bid].name.clone();
        if let Some(i) = self.services.incident_mut(id) {
            match i.damage.iter_mut().find(|d| d.building == bid) {
                Some(d) => d.lost += before - after,
                None => i.damage.push(Damage { building: bid, name: bname, lost: before - after, destroyed: false }),
            }
        }
        if after <= BURNED_DOWN {
            if civic {
                self.gutted(id, bid);
            } else {
                self.burn_down(id, bid);
            }
            return true;
        }
        if intensity > SPREAD_INTENSITY && tick % SPREAD_EVERY == 0 {
            let here = self.building_lot(bid).map(|l| (l.center, l.polygon.clone()));
            if let Some((here, poly)) = here {
                // Neighbours: lots that touch this one (a shared corner within
                // SPREAD_RADIUS of reach) or stand that close centre to centre.
                let touches = |l: &Lot| l.center.distance(here) <= SPREAD_RADIUS || (l.center.distance(here) < 150.0 && l.polygon.iter().any(|p| poly.iter().any(|q| q.distance(*p) < SPREAD_GAP)));
                let near: Vec<BuildingId> = self.buildings.iter().filter(|(nid, nb)| *nid != bid && self.lots.get(nb.lot).is_some_and(touches)).map(|(nid, _)| nid).collect();
                for nb in near {
                    if self.srng().random::<f32>() < SPREAD_CHANCE * intensity {
                        if let Some(new) = self.start_fire(nb) {
                            // The neighbours see it coming: a quick call.
                            if let Some(i) = self.services.incident_mut(new) {
                                i.report_at = tick + 1;
                            }
                            self.events.push(SimEvent::FireSpread(nb));
                        }
                    }
                }
            }
        }
        false
    }

    /// [services] A civic structure (station, plant, depot, dock...) that
    /// burned out: the fire is over, the building stands gutted and out of
    /// service (derelict: no crews, no coverage) until the city's repairs
    /// (`repair_civic`, paid daily) bring it back. Burning them down removed
    /// the player's building outright and orphaned its units' calls.
    pub(super) fn gutted(&mut self, id: u32, bid: BuildingId) {
        let tick = self.clock.tick;
        let Some(b) = self.buildings.get(bid) else { return };
        let name = b.name.clone();
        let people: Vec<CitizenId> = b.workers.clone();
        for p in people {
            if let Some(c) = self.citizens.get_mut(p) {
                c.remember(tick, format!("{name} burned out. Shut until the city fixes it."));
            }
        }
        self.events.push(SimEvent::BuildingChanged(bid));
        self.resolve(id, Outcome::Gutted);
    }

    pub(super) fn burn_down(&mut self, id: u32, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        let (lot, name) = (b.lot, b.name.clone());
        let center = self.lots.get(lot).map_or(Vec2::ZERO, |l| l.center);
        self.events.push(SimEvent::BurnedDown(bid, lot));
        self.demolish_with(bid, None, Some(&format!("{name} burned down. Lost everything.")));
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Fire);
        if let Some(l) = self.lots.get_mut(lot) {
            if l.building == Some(bid) {
                l.building = None;
            }
        }
        let tick = self.clock.tick;
        self.services.rubble.push((lot, center, tick + RUBBLE_TICKS));
        self.services.stats.burned_down_total += 1;
        if let Some(i) = self.services.incident_mut(id) {
            for d in i.damage.iter_mut().filter(|d| d.building == bid) {
                d.destroyed = true;
            }
        }
        self.resolve(id, Outcome::BurnedDown);
        // Anything else going on in there is over too.
        let others: Vec<u32> = self.services.active().filter(|i| i.building == bid).map(|i| i.id).collect();
        for o in others {
            self.resolve(o, Outcome::Gone);
        }
    }
}
