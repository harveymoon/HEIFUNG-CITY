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
use crate::zoning::{inner_center, make_placed_lot};

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
    /// [big-sites] Neighbouring lots joined to `lot` to make room for
    /// something bigger than it (a stadium over several plots): whatever
    /// grew on them is bought out, and the ground becomes one lot.
    pub joined: Vec<LotId>,
}

impl PlacementSite {
    /// What placing there does to the zoning, for the preview: "unzoned
    /// parcel: fine", "takes this parcel out of the residential zone".
    pub fn note(&self) -> String {
        if !self.joined.is_empty() {
            return format!("takes {} plots, joined into one; what stands on them is bought out", self.joined.len() + 1);
        }
        match (self.lot, self.leaves_zone) {
            (None, _) if self.parcels.len() > 1 => format!("unzoned ground ({} parcels): fine, a lot is made for it", self.parcels.len()),
            (None, _) => "unzoned parcel: fine, a lot is made for it".into(),
            (Some(_), Some(lu)) => format!("takes this lot out of the {} zone", lu.label().to_lowercase()),
            (Some(_), None) => "city ground".into(),
        }
    }
}

/// [review] What a new-building search tolerates on a lot besides the
/// fixed bars (promised to an owner, under a station or yard, rubble):
/// see [`World::site_clear`] and [`World::lot_free_for`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SiteRule {
    /// Backland with no road frontage (`landlocked`) may be taken (it is
    /// joined to a plot that has a street).
    pub backland: bool,
    /// A historic district's lot may be taken (`district_frozen`).
    pub historic: bool,
}

impl SiteRule {
    /// A lot on a street, in any district: ordinary growth and businesses.
    pub const STREET: SiteRule = SiteRule { backland: false, historic: true };
    /// A lot on a street, not in a historic district (corps, campuses).
    pub const STREET_NOT_HISTORIC: SiteRule = SiteRule { backland: false, historic: false };
    /// Any lot, backland too, in any district (plots joined to a site).
    pub const ANY: SiteRule = SiteRule { backland: true, historic: true };
    /// Backland too, but not in a historic district (merged sites: towers;
    /// [r4-oldstone] McMansions and mansions take `ANY`, empty plots only
    /// there).
    pub const ANY_NOT_HISTORIC: SiteRule = SiteRule { backland: true, historic: false };
}

impl World {
    /// [review] The one test of whether something new may be put on a lot,
    /// whatever stands there now (a buyout may clear it): it exists, is
    /// not promised to an owner (`estate.reserved`), not under a station or
    /// yard, not rubble, and passes `rule` on backland and historic
    /// districts. About twenty searches each had their own copy, some
    /// missing the reserved or rubble checks (parking and taxi yards went
    /// up on plots an owner had bought).
    pub(crate) fn site_clear(&self, lid: LotId, rule: SiteRule) -> bool {
        let Some(l) = self.lots.get(lid) else { return false };
        (rule.backland || !l.landlocked) && !self.estate.reserved.contains_key(&lid) && !self.services.is_rubble(lid) && (rule.historic || !self.district_frozen(lid)) && !self.under_facility(l.center)
    }

    /// [review] A vacant lot something new may go on: nothing stands there
    /// and [`World::site_clear`].
    pub(crate) fn lot_free_for(&self, lid: LotId, rule: SiteRule) -> bool {
        self.lots.get(lid).is_some_and(|l| l.building.is_none()) && self.site_clear(lid, rule)
    }

    /// One of the buildings the player puts down on a lot (a plant,
    /// battery, depot, service, attraction, station, cargo hub or a parking
    /// lot the city runs): its ground is the city's, out of the zoning.
    /// [review] [`Building::is_city_structure`]: a city car park carries
    /// `city_parking` (set on load for older saves), so `city_lots` is no
    /// second test.
    pub fn is_city_building(&self, id: BuildingId) -> bool {
        self.buildings.get(id).is_some_and(|b| b.is_city_structure())
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
            // [big-sites] A lot too small for it takes in its neighbours.
            if let Some((joined, outline)) = self.joined_site(what, id) {
                return Ok(PlacementSite { lot: Some(id), parcels: Vec::new(), outline, leaves_zone, joined });
            }
            return Ok(PlacementSite { lot: Some(id), parcels: Vec::new(), outline: l.polygon.clone(), leaves_zone, joined: Vec::new() });
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
        // A parcel big enough on its own is the site, however big: a plant
        // there is no bigger than its kind's largest (`size_scale`) and
        // stands on part of it.
        if parcel.path_frontage && parcel.area >= need && crate::zoning::site_thickness(&parcel.polygon) >= what.min_thickness() {
            return Ok(PlacementSite { lot: None, parcels: vec![i], outline: parcel.polygon.clone(), leaves_zone: None, joined: Vec::new() });
        }
        // [plant-scale] Too small on its own (or backland): it joins just
        // enough unzoned neighbours to reach what the building needs, and
        // never more ground than it can use (`max_site_area`).
        let mut near: Vec<usize> = self.parcels.in_circle(parcel.center, SITE_REACH).into_iter().filter(|&k| k == i || self.free_parcel(k)).collect();
        near.sort_unstable();
        near.dedup();
        let polys: Vec<Vec<Vec2>> = near.iter().map(|&k| self.parcels.parcels[k].polygon.clone()).collect();
        let Some(me) = near.iter().position(|&k| k == i) else {
            return Ok(PlacementSite { lot: None, parcels: vec![i], outline: parcel.polygon.clone(), leaves_zone: None, joined: Vec::new() });
        };
        let (idx, outline) = crate::zoning::grow_site_steps(&polys, me, need, what.min_thickness(), what.max_site_area().max(need), &self.roads, what.join_steps(), what.join_steps() > 12);
        Ok(PlacementSite { lot: None, parcels: idx.iter().map(|&k| near[k]).collect(), outline, leaves_zone: None, joined: Vec::new() })
    }

    /// [big-sites] Lots near `lot` that may be joined to it for a big
    /// city building: not the city's (nor a park, nor under a station or
    /// yard, nor promised to an owner), nothing civic on them.
    fn joinable_lot(&self, id: LotId) -> bool {
        let Some(l) = self.lots.get(id) else { return false };
        l.land_use != LandUse::Park
            && !self.is_city_ground(l)
            && self.zones.contains_key(l.zone)
            && self.site_clear(id, SiteRule::ANY)
            && !l.building.and_then(|b| self.buildings.get(b)).is_some_and(Self::is_civic_structure)
    }

    /// [big-sites] Something bigger than the lot it is put on (a stadium,
    /// a university, a coal plant) takes in as few neighbouring lots as it
    /// needs to reach its smallest site (`grow_site`, never more than it
    /// can use): the joined lots and the outline, or None when the lot is
    /// big enough on its own or has no neighbours to grow into.
    pub fn joined_site(&self, what: Placeable, lot: LotId) -> Option<(Vec<LotId>, Vec<Vec2>)> {
        let l = self.lots.get(lot)?;
        let need = what.min_area();
        if l.area() >= need || matches!(what, Placeable::Rail(_) | Placeable::Dock(_) | Placeable::Toll(_)) || !self.joinable_lot(lot) {
            return None;
        }
        // [plant-groups] A small solar or wind lot joins the farm next door instead.
        if let Placeable::Plant(k) = what {
            if self.small_plant_site_joins(k, &l.polygon, Some(lot)).is_some() {
                return None;
            }
        }
        let reach = SITE_REACH + need.sqrt();
        let mut near: Vec<LotId> = self.lots.iter().filter(|(id, o)| *id == lot || (o.center.distance(l.center) < reach && self.joinable_lot(*id))).map(|(id, _)| id).collect();
        near.sort();
        let polys: Vec<Vec<Vec2>> = near.iter().map(|id| self.lots[*id].polygon.clone()).collect();
        let me = near.iter().position(|&id| id == lot)?;
        let (idx, outline) = crate::zoning::grow_site_steps(&polys, me, need, what.min_thickness(), what.max_site_area().max(need), &self.roads, what.join_steps(), what.join_steps() > 12);
        if idx.len() < 2 || crate::zoning::polygon_area(&outline).abs() < need * 0.98 {
            return None;
        }
        let joined: Vec<LotId> = idx.iter().map(|&k| near[k]).filter(|&id| id != lot).collect();
        Some((joined, outline))
    }

    /// [big-sites] Join the site's lots into its first: what stands on the
    /// others is bought out and cleared at once, their ground (and parcels)
    /// becomes the first lot's, which takes the joined outline.
    pub(super) fn join_site_lots(&mut self, site: &PlacementSite, what: Placeable) -> Result<LotId, String> {
        let lot = site.lot.ok_or("no lot")?;
        for &id in &site.joined {
            if let Some(bid) = self.lots.get(id).and_then(|l| l.building) {
                self.take_over_lot(id, &what.label())?;
                if self.buildings.get(bid).is_some_and(|b| b.demolition.is_some()) {
                    self.finish_demolition(bid);
                }
                // Anything still standing there (a site that cannot be
                // cleared at once) keeps the plot out of the join.
                if self.lots.get(id).is_some_and(|l| l.building.is_some()) {
                    return Err("a plot it needs could not be cleared".into());
                }
            }
        }
        for &id in &site.joined {
            self.lots.remove(id);
            for p in self.parcels.parcels.iter_mut().filter(|p| p.lot == Some(id)) {
                p.lot = Some(lot);
            }
        }
        let roads = &self.roads;
        let l = &mut self.lots[lot];
        l.polygon = site.outline.clone();
        l.center = inner_center(&site.outline);
        // [attractions] A landmark's site may be notched: the city's ground
        // keeps its street front (`refresh_lot_roads` for placed lots).
        let landmark = matches!(what, Placeable::Service(k, _) if k.is_landmark());
        super::plan::refresh_lot_roads(l, roads, landmark);
        self.rebuild_walk();
        self.events.push(SimEvent::LotsChanged);
        Ok(lot)
    }

    /// An unzoned parcel with nothing on it, on open ground.
    fn free_parcel(&self, i: usize) -> bool {
        let p = &self.parcels.parcels[i];
        p.lot.is_none() && p.land_use.is_none() && !p.rough && !self.under_facility(p.center)
    }

    /// The lot a site would become (not inserted).
    pub(super) fn site_lot(&self, site: &PlacementSite) -> Option<Lot> {
        self.site_lot_for(site, false)
    }

    /// [`World::site_lot`]; `concave`: [attractions] a landmark's site of
    /// several parcels may be notched (`make_placed_lot_concave`).
    fn site_lot_for(&self, site: &PlacementSite, concave: bool) -> Option<Lot> {
        if let Some(id) = site.lot {
            return self.lots.get(id).cloned();
        }
        let center = match site.parcels.as_slice() {
            [one] => self.parcels.parcels[*one].center,
            _ => inner_center(&site.outline),
        };
        if concave && site.parcels.len() > 1 {
            return crate::zoning::make_placed_lot_concave(ZoneId::default(), site.outline.clone(), center, &self.roads);
        }
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
        if !site.joined.is_empty() {
            // [attractions] A landmark's joined ground is the lot clicked,
            // grown to the outline, as `join_site_lots` makes it (its
            // street front kept, whatever notches the plots leave).
            if matches!(what, Placeable::Service(k, _) if k.is_landmark()) {
                if let Some(mut l) = site.lot.and_then(|id| self.lots.get(id)).cloned() {
                    l.polygon = site.outline.clone();
                    l.center = inner_center(&site.outline);
                    return self.lot_problem(what, &l, None);
                }
            }
            // [big-sites] The joined ground as one lot.
            let lot = make_placed_lot(ZoneId::default(), site.outline.clone(), inner_center(&site.outline), &self.roads);
            // [attractions] A long run of plots along one street joins into a
            // strip too thin to be a lot at all: say so, not "no road".
            let area = crate::zoning::polygon_area(&site.outline).abs();
            let per: f32 = (0..site.outline.len()).map(|i| site.outline[i].distance(site.outline[(i + 1) % site.outline.len()])).sum();
            if lot.as_ref().is_some_and(|l| l.landlocked) && area / (per * per).max(1.0) < 0.031 {
                return Some(format!("the plots here only join into a long thin strip; {} wants a deeper block", crate::names::a(what.label())));
            }
            return match lot {
                Some(l) => self.lot_problem(what, &l, None),
                None => Some("no road access to front".into()),
            };
        }
        match site.lot {
            Some(id) => self.placement_problem(what, id),
            None => match self.site_lot_for(site, what.join_steps() > 12) {
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
            if !site.joined.is_empty() {
                let id = self.join_site_lots(&site, what)?;
                return self.place_on_lot(what, id);
            }
            return self.place_on_lot(what, id);
        }
        let lot = self.make_site_lot(&site, what.join_steps() > 12).ok_or("no road access to front")?;
        let out = self.place_on_lot(what, lot);
        if out.is_err() {
            self.drop_city_lot(lot, true);
        }
        out
    }

    /// Make a city lot for a site of unzoned parcels: a zone of its own,
    /// its parcels tied to it.
    fn make_site_lot(&mut self, site: &PlacementSite, concave: bool) -> Option<LotId> {
        let mut lot = self.site_lot_for(site, concave)?;
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
            // [r5-trash] The disposal sites are heavy industry.
            Some(k) if k.is_waste() => Some(LandUse::Industrial),
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
        // [dock-access] A dock goes with its pier: the ground under its
        // footprint goes back to the plan; its access road stays a road.
        if b.port.is_some() {
            return self.bulldoze_dock(bid);
        }
        if b.rail.is_some() {
            return Err(format!("{} is a station or yard: it is not bulldozed on its own", b.name));
        }
        if !self.is_city_building(bid) {
            return Err(format!("{} is not the city's: place a city building over it to buy it out", b.name));
        }
        let (name, power, service, metro) = (b.name.clone(), b.is_power(), b.service.is_some(), b.metro);
        let lot = b.lot;
        let grounds = self.walk.junctions.iter().any(|j| j.lot == Some(lot)) || self.lots.get(lot).is_some_and(|l| l.park.is_some());
        // [attractions] A pier's deck goes with it.
        self.remove_pier(bid);
        // [dams] Its dam wall goes with it (the crest's footpath stays).
        self.forget_dam(bid);
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
        // [park-paths] Its grounds' gates go, and a path joined to them is
        // cut back now. [attractions] A park, zoo, garden or pier's own
        // walkways go with it.
        if grounds {
            self.rebuild_walk();
        }
        self.settle_path_links();
        Ok(format!("{name} bulldozed"))
    }
}
