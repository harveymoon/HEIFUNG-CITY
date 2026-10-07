//! [free-placement] Player-placed buildings on any usable parcel, zoned or
//! not, and the ground under them taken out of the zoning.
//!
//! Everything the Build window puts on a lot (plants, batteries, services
//! and attractions, depots, metro stations, the cargo hub, city parking)
//! goes on a lot of any zone, or on unzoned parcels with road access: a lot
//! is made for them when the building goes down. The ground under a city
//! building is the city's own: its lot's use is Civic with a zone of its
//! own outline, so zoned growth never claims it, zone tallies do not count
//! it and the zone tools leave it alone while the building stands
//! (`parcel_refusal`). Rebuilds keep it so (`build_lots_from_parcels` pins
//! it), and an older save's city buildings are switched over on load. When
//! the building is bulldozed the lot goes back to whatever the master plan
//! has there, or is unzoned ground again.

use super::*;
use crate::plan::parcel_zoning;
use crate::services::Placeable;
use crate::zoning::{inner_center, join_parcels, make_placed_lot};

/// Unzoned parcels this close to the one clicked may join it into a site,
/// metres from its centre.
const SITE_REACH: f32 = 90.0;

/// Where a building put down at a point would stand.
#[derive(Clone, Debug)]
pub struct PlacementSite {
    /// The lot it takes when the ground is a lot already (zoned, or the
    /// city's own).
    pub lot: Option<LotId>,
    /// Otherwise the unzoned parcels (indices into `parcels`) a lot is made
    /// of for it.
    pub parcels: Vec<usize>,
    /// Outline of the lot.
    pub outline: Vec<Vec2>,
    /// The zone the ground leaves (None: unzoned, or the city's already).
    pub leaves_zone: Option<LandUse>,
}

impl PlacementSite {
    /// What placing there does to the zoning, for the preview: "unzoned
    /// parcel: fine", "takes this parcel out of the residential zone".
    pub fn note(&self) -> String {
        match (self.lot, self.leaves_zone) {
            (None, _) if self.parcels.len() > 1 => format!("unzoned ground ({} parcels): fine, a lot is made for it", self.parcels.len()),
            (None, _) => "unzoned parcel: fine, a lot is made for it".into(),
            (Some(_), Some(lu)) => format!("takes this lot out of the {} zone", lu.label().to_lowercase()),
            (Some(_), None) => "city ground".into(),
        }
    }
}

impl World {
    /// One of the buildings the player puts down on a lot (a plant,
    /// battery, depot, service, attraction, station, cargo hub or a parking
    /// lot the city runs): its ground is the city's, out of the zoning.
    pub fn is_city_building(&self, id: BuildingId) -> bool {
        self.buildings.get(id).is_some_and(|b| Self::is_civic_structure(b) || self.vehicles.city_lots.contains_key(&id))
    }

    /// A lot under a city building ([`World::is_city_building`]).
    pub fn is_city_ground(&self, lot: &Lot) -> bool {
        lot.building.is_some_and(|b| self.is_city_building(b))
    }

    /// Where `what` would go for a click at `p`: the lot under the point,
    /// else a site made of the unzoned parcel there (joined with unzoned
    /// neighbours when one parcel is too small for it). Err says why the
    /// ground there cannot take a building at all.
    pub fn placement_site(&self, what: Placeable, p: Vec2) -> Result<PlacementSite, String> {
        if let Some((id, l)) = self.lots.iter().find(|(_, l)| l.contains(p)) {
            let leaves_zone = (!self.is_city_ground(l) && self.zones.contains_key(l.zone)).then_some(l.land_use);
            return Ok(PlacementSite { lot: Some(id), parcels: Vec::new(), outline: l.polygon.clone(), leaves_zone });
        }
        let i = self.parcels.at(p).ok_or("no parcel here: click ground along a road")?;
        let parcel = &self.parcels.parcels[i];
        if self.under_facility(parcel.center) {
            return Err("that ground belongs to a station or yard".into());
        }
        if parcel.rough {
            return Err("too steep or wet to build on".into());
        }
        if !self.free_parcel(i) {
            return Err("that parcel is not free".into());
        }
        let need = what.min_area();
        if parcel.path_frontage && parcel.area >= need {
            return Ok(PlacementSite { lot: None, parcels: vec![i], outline: parcel.polygon.clone(), leaves_zone: None });
        }
        // Too small on its own (or backland): it joins its unzoned
        // neighbours as civic ground would be joined, a works yard's size.
        let near: Vec<usize> = self.parcels.in_circle(parcel.center, SITE_REACH).into_iter().filter(|&k| k == i || self.free_parcel(k)).collect();
        let polys: Vec<Vec<Vec2>> = near.iter().map(|&k| self.parcels.parcels[k].polygon.clone()).collect();
        let me = near.iter().position(|&k| k == i).unwrap_or(0);
        let group = join_parcels(&polys, LandUse::Civic, &self.roads).into_iter().find(|(idx, _)| idx.contains(&me));
        match group {
            Some((idx, outline)) => Ok(PlacementSite { lot: None, parcels: idx.iter().map(|&k| near[k]).collect(), outline, leaves_zone: None }),
            None => Ok(PlacementSite { lot: None, parcels: vec![i], outline: parcel.polygon.clone(), leaves_zone: None }),
        }
    }

    /// An unzoned parcel with nothing on it, on open ground.
    fn free_parcel(&self, i: usize) -> bool {
        let p = &self.parcels.parcels[i];
        p.lot.is_none() && p.land_use.is_none() && !p.rough && !self.under_facility(p.center)
    }

    /// The lot a site would become (not inserted).
    fn site_lot(&self, site: &PlacementSite) -> Option<Lot> {
        if let Some(id) = site.lot {
            return self.lots.get(id).cloned();
        }
        let center = match site.parcels.as_slice() {
            [one] => self.parcels.parcels[*one].center,
            _ => inner_center(&site.outline),
        };
        make_placed_lot(ZoneId::default(), site.outline.clone(), center, &self.roads)
    }

    /// Why `what` cannot go down at `p` (the placement preview and the
    /// click), or the site it would take.
    pub fn placement_problem_at(&self, what: Placeable, p: Vec2) -> Result<PlacementSite, String> {
        let site = self.placement_site(what, p)?;
        match self.site_problem(what, &site) {
            Some(why) => Err(why),
            None => Ok(site),
        }
    }

    /// Why `what` cannot go on a site found by [`World::placement_site`]
    /// (the preview keeps the site while the pointer stays on its parcel).
    pub fn site_problem(&self, what: Placeable, site: &PlacementSite) -> Option<String> {
        match site.lot {
            Some(id) => self.placement_problem(what, id),
            None => match self.site_lot(site) {
                Some(lot) => self.lot_problem(what, &lot, None),
                None => Some("no road access to front".into()),
            },
        }
    }

    /// Put `what` down at `p` on the lot there, or on a lot made for the
    /// unzoned parcels there (taken back if the building cannot go up).
    pub(super) fn place_on_site(&mut self, what: Placeable, p: Vec2) -> Result<BuildingId, String> {
        let site = self.placement_problem_at(what, p)?;
        if let Some(id) = site.lot {
            return self.place_on_lot(what, id);
        }
        let lot = self.make_site_lot(&site).ok_or("no road access to front")?;
        let out = self.place_on_lot(what, lot);
        if out.is_err() {
            self.drop_city_lot(lot, true);
        }
        out
    }

    /// Make a city lot for a site of unzoned parcels: a zone of its own,
    /// its parcels tied to it.
    fn make_site_lot(&mut self, site: &PlacementSite) -> Option<LotId> {
        let mut lot = self.site_lot(site)?;
        let zid = self.zones.insert(Zone { polygon: lot.polygon.clone(), land_use: LandUse::Civic });
        lot.zone = zid;
        let id = self.lots.insert(lot);
        for &i in &site.parcels {
            let p = &mut self.parcels.parcels[i];
            p.lot = Some(id);
            p.land_use = Some(LandUse::Civic);
        }
        self.rebuild_walk();
        self.events.push(SimEvent::LotsChanged);
        Some(id)
    }

    /// Take a lot out of the zoning for a city building: Civic, with a zone
    /// of its own outline (the lot keeps its shape, so nothing moves). A
    /// station's, yard's or dock's own lot (in no zone) is left alone.
    pub(super) fn take_for_city(&mut self, lot_id: LotId) {
        let Some(l) = self.lots.get(lot_id) else { return };
        let old = l.zone;
        let Some(z) = self.zones.get(old) else { return };
        if l.land_use == LandUse::Civic && z.land_use == LandUse::Civic && z.polygon == l.polygon {
            return;
        }
        let own_outline = z.polygon == l.polygon;
        let polygon = l.polygon.clone();
        let zid = self.zones.insert(Zone { polygon, land_use: LandUse::Civic });
        let l = &mut self.lots[lot_id];
        l.zone = zid;
        l.land_use = LandUse::Civic;
        l.park = None;
        for p in self.parcels.parcels.iter_mut().filter(|p| p.lot == Some(lot_id)) {
            p.land_use = Some(LandUse::Civic);
        }
        // A painted lot's zone was its own outline: nothing else is in it.
        if own_outline && !self.lots.values().any(|l| l.zone == old) {
            self.zones.remove(old);
        }
        // The building keeps its own use (`Building::land_use`: what it
        // draws, pays and is for); only the ground is the city's.
        self.events.push(SimEvent::LotsChanged);
    }

    /// The use a city building has whatever ground it stands on: what it
    /// draws from the grid, what its jobs pay and what kind of place it is
    /// (a plant or depot works like industry, a stadium is a night out, a
    /// city car park is a commercial lot). None for anything else (grown
    /// buildings, stations and docks keep the use they were given).
    pub fn city_use(b: &Building) -> Option<LandUse> {
        use crate::services::ServiceKind;
        if b.is_power() || b.depot || b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::CargoHub) {
            return Some(LandUse::Industrial);
        }
        if b.metro {
            return Some(LandUse::Civic);
        }
        match b.service.map(|s| s.kind) {
            Some(ServiceKind::Stadium) => Some(LandUse::Entertainment),
            Some(ServiceKind::TrashDepot) => Some(LandUse::Industrial),
            Some(_) => Some(LandUse::Civic),
            None => None,
        }
    }

    /// The use to show for a building (inspector, hover, API): Civic for a
    /// city building (its ground is out of the zoning), else its own.
    pub fn shown_use(&self, id: BuildingId) -> LandUse {
        match self.buildings.get(id) {
            Some(_) if self.is_city_building(id) => LandUse::Civic,
            Some(b) => b.land_use,
            None => LandUse::Civic,
        }
    }

    /// A city building is gone from `lot_id`: the ground goes back to what
    /// the master plan has there (most of its parcels decide), or is unzoned
    /// ground again and the lot goes. `walk`: rebuild the walk graph (not
    /// in a rebuild, which does it itself).
    pub(super) fn release_city_lot(&mut self, lot_id: LotId, walk: bool) {
        let Some(l) = self.lots.get(lot_id) else { return };
        if l.building.is_some() || !self.zones.contains_key(l.zone) {
            return;
        }
        let mut members: Vec<usize> = (0..self.parcels.len()).filter(|&i| self.parcels.parcels[i].lot == Some(lot_id)).collect();
        if members.is_empty() {
            members = self.parcels.in_polygon(&l.polygon);
        }
        let zoning = parcel_zoning(&self.parcels, &self.master);
        let mut votes: Vec<(LandUse, usize)> = Vec::new();
        for &i in &members {
            if let Some((lu, _)) = zoning.get(i).copied().flatten() {
                match votes.iter_mut().find(|(v, _)| *v == lu) {
                    Some(v) => v.1 += 1,
                    None => votes.push((lu, 1)),
                }
            }
        }
        // Any zoned parcel keeps the lot zoned (the next rebuild cuts it to
        // the plan's own lots); ties go to the earlier use.
        let planned = votes.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.index().cmp(&a.0.index()))).map(|(lu, _)| lu);
        let Some(lu) = planned else {
            self.drop_city_lot(lot_id, walk);
            return;
        };
        let l = &self.lots[lot_id];
        let (polygon, center, old_zone) = (l.polygon.clone(), l.center, l.zone);
        // The zone polygon the plan has there, else a zone of its own
        // outline as a painted lot has.
        let zid = match self.zones.iter().find(|(id, z)| *id != old_zone && z.land_use == lu && z.polygon.len() >= 3 && z.contains(center)).map(|(id, _)| id) {
            Some(z) => z,
            None => self.zones.insert(Zone { polygon: polygon.clone(), land_use: lu }),
        };
        let landlocked = l.landlocked;
        let footfall = (l.footfall_today, l.footfall_hour);
        match crate::zoning::make_lot(zid, lu, polygon, center, &self.roads) {
            Some(mut lot) => {
                lot.landlocked |= landlocked;
                (lot.footfall_today, lot.footfall_hour) = footfall;
                self.lots[lot_id] = lot;
            }
            None => {
                let l = &mut self.lots[lot_id];
                l.zone = zid;
                l.land_use = lu;
            }
        }
        if !self.lots.values().any(|l| l.zone == old_zone) {
            self.zones.remove(old_zone);
        }
        for &i in &members {
            self.parcels.parcels[i].land_use = Some(lu);
        }
        if walk {
            self.rebuild_walk();
        }
        self.events.push(SimEvent::LotsChanged);
    }

    /// Unzoned ground again: the lot and its zone go, its parcels are free.
    fn drop_city_lot(&mut self, lot_id: LotId, walk: bool) {
        let Some(l) = self.lots.remove(lot_id) else { return };
        if !self.lots.values().any(|o| o.zone == l.zone) {
            self.zones.remove(l.zone);
        }
        for p in self.parcels.parcels.iter_mut().filter(|p| p.lot == Some(lot_id)) {
            p.lot = None;
            p.land_use = None;
        }
        self.services.rubble.retain(|r| r.0 != lot_id);
        for camp in &mut self.society.homeless.camps {
            if camp.lot == Some(lot_id) {
                camp.lot = None;
            }
        }
        for c in self.citizens.values_mut() {
            c.pending_footfall.retain(|l| *l != lot_id);
        }
        if walk {
            self.rebuild_walk();
        }
        self.events.push(SimEvent::LotsChanged);
    }

    /// After a rebuild (and on load): every city building's lot is the
    /// city's (an older save's first rebuild switches them over without
    /// moving anything), and city ground made for a building that did not
    /// survive the works goes back to the plan.
    pub(super) fn settle_city_ground(&mut self, pinned: &[LotId], walk: bool) {
        let city: Vec<(BuildingId, LotId)> = self.buildings.iter().filter(|(id, _)| self.is_city_building(*id)).map(|(id, b)| (id, b.lot)).collect();
        for (bid, lot) in city {
            if self.lots.get(lot).is_some_and(|l| l.building == Some(bid)) {
                self.take_for_city(lot);
            }
        }
        for &id in pinned {
            if self.lots.get(id).is_some_and(|l| l.building.is_none()) {
                self.release_city_lot(id, walk);
            }
        }
    }

    /// Bulldoze a city building (the Bulldoze tool, the inspector, the
    /// remote API): it goes at once and its ground goes back to the plan.
    /// Grown buildings are not the city's to pull down; stations, yards and
    /// docks go with their track or pier.
    pub fn bulldoze_building(&mut self, bid: BuildingId) -> Result<String, String> {
        let b = self.buildings.get(bid).ok_or("no such building")?;
        if b.rail.is_some() || b.port.is_some() {
            return Err(format!("{} is a station, yard or dock: it is not bulldozed on its own", b.name));
        }
        if !self.is_city_building(bid) {
            return Err(format!("{} is not the city's: place a city building over it to buy it out", b.name));
        }
        let (name, power, service, metro) = (b.name.clone(), b.is_power(), b.service.is_some(), b.metro);
        self.demolish(bid);
        if power {
            self.refresh_power();
        }
        if service {
            self.refresh_coverage();
        }
        if metro {
            self.subway.version += 1;
        }
        Ok(format!("{name} bulldozed"))
    }
}
