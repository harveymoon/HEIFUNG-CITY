//! [r11-lot-memory] Recording what stood on each lot (`crate::lot_memory`):
//! a building gone, a standing building's trade changed, a shopfront let to
//! someone else. Called from the places that already do the work
//! (`finish_demolition`, `demolish_with`, `set_business`, `sync_tenants`);
//! a pure copy of what they know, no random numbers.

use super::*;
use crate::buildings::Demolition;
use crate::lot_memory::{Trace, TraceKind};

impl World {
    /// Building `bid` is gone (call before it is removed, or before its
    /// successor's construction takes over): what stood, from the
    /// demolition `d` when there is one (during a redevelopment the
    /// building's own fields are its successor's), else from the building.
    /// Nothing is kept of a site that never stood.
    pub(in crate::world) fn remember_demolished(&mut self, bid: BuildingId, d: Option<&Demolition>) {
        let Some(b) = self.buildings.get(bid) else { return };
        if (d.is_none() && b.construction.is_some()) || d.is_some_and(|d| d.was_site) {
            return;
        }
        // Only from its own lot: after a rebuild of the plan `b.lot` may be
        // an old key naming some other lot.
        let Some(lot) = self.lots.get(b.lot).filter(|l| l.building == Some(bid)) else { return };
        // A car park, a park: nothing stood that leaves a wall or a slab
        // (floors 0), but the lot remembers it.
        let flat = b.is_parking() || b.land_use == LandUse::Park;
        // The ground it stood on: its merged plots when it is being cleared
        // with them, its own lot when a successor takes the plots on.
        let outline = match d {
            Some(d) if !d.clear_lot => lot.polygon.clone(),
            _ => self.footprint(bid).map_or_else(|| lot.polygon.clone(), |f| f.polygon.clone()),
        };
        let t = match d {
            Some(d) => Trace {
                id: 0,
                kind: TraceKind::Demolished,
                tick: self.clock.tick,
                built: if d.built > 0 { d.built } else { b.built_tick.min(self.clock.tick) },
                at: lot.center,
                outline,
                floors: if flat { 0 } else { d.floors },
                land_use: d.land_use,
                business: d.business,
                name: if d.name.is_empty() && d.clear_lot { b.name.clone() } else { d.name.clone() },
                seed: d.seed,
                reason: d.reason,
                unit: None,
                now: None,
                building: bid,
            },
            None => Trace {
                id: 0,
                kind: TraceKind::Demolished,
                tick: self.clock.tick,
                built: b.built_tick,
                at: lot.center,
                outline,
                floors: if flat { 0 } else { b.floors.max(1) },
                land_use: b.land_use,
                business: b.business.as_ref().map(|z| z.kind),
                name: b.name.clone(),
                seed: b.seed,
                reason: crate::buildings::DemolitionReason::Unknown,
                unit: None,
                now: None,
                building: bid,
            },
        };
        let lot = lot.clone();
        self.lot_memory.record(t, Some(&lot));
    }

    /// The trade in standing building `bid` (or in its shopfront `unit`)
    /// changed from `was` to `now` (None: shut, to let). Not on sites,
    /// demolitions or tenants' own records (their shopfront is noted on
    /// the host, by `sync_tenants`).
    pub(in crate::world) fn remember_change(&mut self, bid: BuildingId, unit: Option<u8>, was: BusinessKind, name: String, now: Option<BusinessKind>) {
        let Some(b) = self.buildings.get(bid) else { return };
        if b.construction.is_some() || b.demolition.is_some() || b.is_tenant() || now == Some(was) {
            return;
        }
        let Some(lot) = self.lots.get(b.lot) else { return };
        let t = Trace {
            id: 0,
            kind: TraceKind::Changed,
            tick: self.clock.tick,
            built: b.built_tick,
            at: lot.center,
            outline: Vec::new(),
            floors: b.floors,
            land_use: b.land_use,
            business: Some(was),
            name,
            seed: crate::hash::mix(b.seed, self.lot_memory_salt(unit)),
            reason: crate::buildings::DemolitionReason::Unknown,
            unit,
            now,
            building: bid,
        };
        let lot = lot.clone();
        self.lot_memory.record(t, Some(&lot));
    }

    /// A different look for each change of a building: by how many it has
    /// had (deterministic, no draw).
    fn lot_memory_salt(&self, unit: Option<u8>) -> u64 {
        0x6057_5161 ^ (self.lot_memory.len() as u64).wrapping_mul(0x9E37) ^ unit.map_or(0xFF, |u| u as u64)
    }

    /// Dev and testing (the remote `lot_memory`): pull building `bid` down
    /// now, the lot left vacant, as a bulldozing would after its days of
    /// demolition. Not the city's own structures (bulldoze those).
    pub fn dev_demolish(&mut self, bid: BuildingId) -> Result<String, String> {
        let b = self.buildings.get(bid).ok_or("no such building")?;
        if b.is_tenant() || Self::is_civic_structure(b) || b.construction.is_some() {
            return Err(format!("{} is a tenant, a city structure or a site", b.name));
        }
        let name = b.name.clone();
        self.demolish(bid);
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Bulldozed);
        self.finish_demolition(bid);
        Ok(format!("{name} pulled down"))
    }

    /// Dev and testing: the trade in standing building `bid` becomes `kind`
    /// (or shuts, None), as when a shop changes hands.
    pub fn dev_change_trade(&mut self, bid: BuildingId, kind: Option<BusinessKind>) -> Result<String, String> {
        let b = self.buildings.get(bid).ok_or("no such building")?;
        if b.business.is_none() || !b.operating() {
            return Err(format!("{} has no trade to change", b.name));
        }
        let jobs = b.jobs;
        self.set_business(bid, kind.map(|k| crate::market::Business::new(k, jobs)));
        if kind.is_some() {
            self.fit_business_name(bid);
        }
        self.events.push(SimEvent::BuildingChanged(bid));
        Ok(self.buildings[bid].name.clone())
    }

    /// "What stood here" for the lot under building `bid` (or `lot`):
    /// newest first.
    pub fn lot_history(&self, lot: LotId) -> Vec<&crate::lot_memory::Trace> {
        self.lots.get(lot).map_or_else(Vec::new, |l| self.lot_memory.on_lot(l))
    }
}
