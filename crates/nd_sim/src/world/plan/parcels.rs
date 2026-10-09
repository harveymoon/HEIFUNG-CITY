//! Parcels: the zoning painted onto them, and lots cut from them.

use super::*;

impl World {
    /// Land use of every parcel (by index into `parcels`) under the
    /// effective plan: the master with the active project applied. Kept
    /// current with every plan change.
    pub fn planned_zoning(&self) -> &[Option<LandUse>] {
        if self.planned.len() == self.parcels.len() {
            &self.planned
        } else {
            &[]
        }
    }

    pub(in crate::world) fn refresh_planned_zoning(&mut self) {
        let z: ParcelZoning = parcel_zoning(&self.parcels, self.effective_plan());
        self.planned = z.into_iter().map(|o| o.map(|(l, _)| l)).collect();
    }

    /// Paint parcels (indices into `parcels`) with a land use, or erase
    /// them back to unzoned with `None`. Only parcels that accept the use
    /// and are not already planned as it are painted (erasing, only zoned
    /// ones). With `stroke` naming the paint gesture of a stroke still
    /// being dragged, the parcels join it and its step is amended, so one
    /// stroke is one undo. Returns the stroke's gesture (None when nothing
    /// was painted and there was no stroke) and how many parcels changed.
    pub fn paint_parcels(&mut self, ids: &[usize], land_use: Option<LandUse>, stroke: Option<GestureId>) -> (Option<GestureId>, usize) {
        self.paint_parcels_with(ids, land_use, None, stroke)
    }

    /// [`World::paint_parcels`] that also sets the industry sub-zone of the
    /// painted ground (`industry`: an industry kind key, or "auto"), applied
    /// on implement. With a sub-zone, parcels already planned industrial are
    /// painted again so their sub-zone changes.
    pub fn paint_parcels_with(&mut self, ids: &[usize], land_use: Option<LandUse>, industry: Option<&str>, stroke: Option<GestureId>) -> (Option<GestureId>, usize) {
        // [waterfront] A park may be painted as a public beach (its
        // sub-zone), on parcels that run down to the water.
        let beach = land_use == Some(LandUse::Park) && industry == Some(crate::shore::BEACH_SUBZONE);
        let industry = industry.filter(|k| land_use == Some(LandUse::Industrial) || (beach && *k == crate::shore::BEACH_SUBZONE)).map(str::to_string);
        // Plain park over a beach turns it back into a park.
        let beaches_now = if land_use == Some(LandUse::Park) && !beach { self.planned_beaches() } else { Vec::new() };
        let planned = self.planned_zoning();
        let mut keys: Vec<ParcelKey> = Vec::new();
        for &i in ids {
            let Some(p) = self.parcels.parcels.get(i) else { continue };
            let now = planned.get(i).copied().flatten();
            let allowed = if beach { self.beach_refusal(i).is_none() } else { self.parcel_refusal(i, land_use).is_none() };
            let wanted = allowed
                && match land_use {
                    Some(lu) => now != Some(lu) || industry.is_some() || beaches_now.get(i).copied().unwrap_or(false),
                    None => now.is_some(),
                };
            if wanted {
                keys.push(p.key);
            }
        }
        keys.sort();
        keys.dedup();
        if keys.is_empty() {
            return (stroke, 0);
        }
        let n = keys.len();
        if let Some(id) = stroke {
            if let Some(Gesture::ParcelZone { parcels: mut have, land_use: l, industry: ind }) = self.gesture(id) {
                if l == land_use && ind == industry {
                    let before = have.len();
                    have.extend(keys);
                    have.sort();
                    have.dedup();
                    // Parcels the stroke already holds are not news.
                    let added = have.len() - before;
                    if added > 0 {
                        self.set_gesture(id, Gesture::ParcelZone { parcels: have, land_use, industry }, false);
                    }
                    return (Some(id), added);
                }
            }
        }
        let id = self.add_gesture(Gesture::ParcelZone { parcels: keys, land_use, industry });
        (Some(id), n)
    }

    /// A zone polygon, and with an industry sub-zone the same parcels
    /// painted with it, as one step (one undo).
    pub fn add_zone_gesture(&mut self, polygon: Vec<Vec2>, land_use: LandUse, industry: Option<&str>) -> GestureId {
        let zone = GestureId(self.next_gesture);
        self.next_gesture += 1;
        let mut changes = vec![(zone, Some(Gesture::Zone { polygon: polygon.clone(), land_use }))];
        if let (LandUse::Industrial, Some(ind)) = (land_use, industry) {
            let mut keys: Vec<ParcelKey> = self.parcels.in_polygon(&polygon).into_iter().filter(|&i| self.parcel_refusal(i, Some(land_use)).is_none()).map(|i| self.parcels.parcels[i].key).collect();
            keys.sort();
            if !keys.is_empty() {
                let id = GestureId(self.next_gesture);
                self.next_gesture += 1;
                changes.push((id, Some(Gesture::ParcelZone { parcels: keys, land_use: Some(land_use), industry: Some(ind.to_string()) })));
            }
        }
        // [waterfront] A beach drawn as a polygon: its parcels on the water.
        if land_use == LandUse::Park && industry == Some(crate::shore::BEACH_SUBZONE) {
            let mut keys: Vec<ParcelKey> = self.parcels.in_polygon(&polygon).into_iter().filter(|&i| self.beach_refusal(i).is_none()).map(|i| self.parcels.parcels[i].key).collect();
            keys.sort();
            if !keys.is_empty() {
                let id = GestureId(self.next_gesture);
                self.next_gesture += 1;
                changes.push((id, Some(Gesture::ParcelZone { parcels: keys, land_use: Some(land_use), industry: Some(crate::shore::BEACH_SUBZONE.to_string()) })));
            }
        }
        self.edit_project(|p| p.push_many(changes));
        zone
    }

    /// After an implement: the industry sub-zones the project's strokes
    /// painted, one bias per connected patch of their parcels, oldest
    /// stroke first so the newest wins.
    pub(super) fn apply_industry_strokes(&mut self, project: &Project) {
        let touched: std::collections::BTreeSet<GestureId> = project.touched().collect();
        let mut patches: Vec<(Vec<Vec2>, String)> = Vec::new();
        for id in touched {
            let Some(Gesture::ParcelZone { parcels, land_use: Some(LandUse::Industrial), industry: Some(kind) }) = self.master.gestures.get(&id) else { continue };
            let polys: Vec<Vec<Vec2>> = parcels.iter().filter_map(|k| self.parcels.find(k)).map(|i| self.parcels.parcels[i].polygon.clone()).collect();
            for outline in crate::zoning::union_outlines(&polys) {
                patches.push((outline, kind.clone()));
            }
        }
        for (outline, kind) in patches {
            self.apply_industry_choice(outline, &kind);
        }
    }

    /// Cut the parcels for today's roads: rough ground marked, nothing
    /// under a station, yard or dock footprint.
    pub(super) fn cut_parcels(&self) -> Vec<Parcel> {
        let water = self.water_mask();
        // [island-lots] The square grows to take in streets drawn past the
        // configured one (an island off the coast): parcels used to be cut
        // off at `half_extent`, so such a road had no land to zone at all.
        let h = self.parcel_half();
        // [waterfront] Down to the water along roads near it (cities since).
        let shore = self.shore.reach_water.then_some(crate::zoning::SHORE_REACH);
        let mut parcels = crate::zoning::generate_parcels_with(&self.roads, &water, Some((Vec2::splat(-h), Vec2::splat(h))), shore);
        if !self.landscape.is_flat() {
            let land = &self.landscape;
            for p in &mut parcels {
                // [island-lots] Hillsides are graded (terraced lot by lot);
                // only cliffs and water are rough.
                p.rough_why = land.lot_ground_problem(&p.polygon, p.center);
                p.rough = p.rough_why.is_some();
            }
        }
        let footprints = self.structure_footprints();
        parcels.retain(|p| !footprints.iter().any(|fp| point_in_polygon(p.center, fp) || polygons_overlap(&shrink_about_centroid(fp, 0.995), &p.polygon)));
        parcels
    }

    /// Cut the parcels again and tie each to the saved lot holding its
    /// centre (after a load: lots are saved, parcels are derived).
    pub(in crate::world) fn recut_parcels(&mut self) {
        let parcels = self.cut_parcels();
        self.parcels = ParcelMap::new(parcels, self.parcels.version + 1);
        let grid = LotGrid::new(&self.lots);
        for p in &mut self.parcels.parcels {
            let hit = grid.near(p.center, 0.0).into_iter().find(|&id| {
                let l = &self.lots[id];
                self.zones.contains_key(l.zone) && l.contains(p.center)
            });
            if let Some(id) = hit {
                p.lot = Some(id);
                p.land_use = Some(self.lots[id].land_use);
            }
        }
        self.refresh_planned_zoning();
    }

    /// Parcels from the roads, zoning from the master plan, a lot for every
    /// zoned parcel (or run of joined parcels). Zone polygons keep their
    /// zone; painted lots get a zone of their own outline.
    /// `pins` are lots under city buildings: their parcels are the city's
    /// ground whatever is planned, one Civic lot per pin with a zone of its
    /// own (returned), out of the zoning until the building is bulldozed.
    /// `parks` are lots with a park: their parcels
    /// stay park only where the plan leaves them unzoned (a park rezoned
    /// for building is left for development to clear).
    pub(super) fn build_lots_from_parcels(&mut self, pins: &[Vec<Vec2>], parks: &[Vec<Vec2>]) -> Vec<LotId> {
        let parcels = self.cut_parcels();
        self.parcels = ParcelMap::new(parcels, self.parcels.version + 1);
        self.zones.clear();
        let mut zoning = parcel_zoning(&self.parcels, &self.master);
        // [free-placement] Ground under the city's own buildings is not the
        // zone's, whatever zoning was painted or drawn over it since: those
        // parcels change only once the building is bulldozed.
        let mut pinned_to: Vec<Option<usize>> = vec![None; zoning.len()];
        for (i, z) in zoning.iter_mut().enumerate() {
            let c = self.parcels.parcels[i].center;
            if let Some(k) = pins.iter().position(|poly| point_in_polygon(c, poly)) {
                pinned_to[i] = Some(k);
                *z = None;
            } else if z.is_none() && parks.iter().any(|poly| point_in_polygon(c, poly)) {
                *z = Some((LandUse::Park, GestureId(u64::MAX)));
            }
        }
        // Zones newest first, as they always were listed.
        let zones: Vec<(GestureId, Vec<Vec2>, LandUse)> = self.master.zones().map(|(id, p, lu)| (id, p.to_vec(), lu)).collect();
        let mut zone_of: HashMap<GestureId, ZoneId> = HashMap::new();
        for (gid, polygon, land_use) in zones.into_iter().rev() {
            zone_of.insert(gid, self.zones.insert(Zone { polygon, land_use }));
        }
        for lu in LandUse::ALL {
            let members: Vec<usize> = (0..zoning.len()).filter(|&i| zoning[i].is_some_and(|(l, _)| l == lu)).collect();
            if members.is_empty() {
                continue;
            }
            let polys: Vec<Vec<Vec2>> = members.iter().map(|&i| self.parcels.parcels[i].polygon.clone()).collect();
            for (idx, poly) in join_parcels(&polys, lu, &self.roads) {
                let first = members[idx[0]];
                let Some((_, gid)) = zoning[first] else { continue };
                let center = if idx.len() == 1 { self.parcels.parcels[first].center } else { inner_center(&poly) };
                let zid = match zone_of.get(&gid) {
                    Some(z) => *z,
                    None => self.zones.insert(Zone { polygon: poly.clone(), land_use: lu }),
                };
                let Some(mut lot) = make_lot(zid, lu, poly, center, &self.roads) else { continue };
                if idx.iter().any(|&k| self.parcels.parcels[members[k]].rough) {
                    lot.landlocked = true;
                }
                let id = self.lots.insert(lot);
                for &k in &idx {
                    let p = &mut self.parcels.parcels[members[k]];
                    p.lot = Some(id);
                    p.land_use = Some(lu);
                }
            }
        }
        // [free-placement] One city lot per pin, from the parcels under it.
        let mut pinned = Vec::new();
        for k in 0..pins.len() {
            let members: Vec<usize> = (0..pinned_to.len()).filter(|&i| pinned_to[i] == Some(k)).collect();
            if members.is_empty() {
                continue;
            }
            let polys: Vec<Vec<Vec2>> = members.iter().map(|&i| self.parcels.parcels[i].polygon.clone()).collect();
            let Some(poly) = crate::zoning::joined_outline(&polys) else { continue };
            let center = if members.len() == 1 { self.parcels.parcels[members[0]].center } else { inner_center(&poly) };
            let zid = self.zones.insert(Zone { polygon: poly.clone(), land_use: LandUse::Civic });
            let Some(mut lot) = crate::zoning::make_placed_lot(zid, poly, center, &self.roads) else {
                self.zones.remove(zid);
                continue;
            };
            if members.iter().any(|&i| self.parcels.parcels[i].rough) {
                lot.landlocked = true;
            }
            let id = self.lots.insert(lot);
            for &i in &members {
                let p = &mut self.parcels.parcels[i];
                p.lot = Some(id);
                p.land_use = Some(LandUse::Civic);
            }
            pinned.push(id);
        }
        self.refresh_planned_zoning();
        pinned
    }

    /// A parcel under one of the city's own structures (or a station or
    /// yard footprint): its zoning cannot change until the structure goes.
    pub fn parcel_holds_civic(&self, i: usize) -> bool {
        let Some(p) = self.parcels.parcels.get(i) else { return false };
        if self.under_facility(p.center) {
            return true;
        }
        // [review] The cheap test first: a lot under a city structure (few),
        // then whether it holds the parcel's centre.
        self.lots.values().any(|l| self.is_city_ground(l) && l.contains(p.center))
    }

    /// Why parcel `i` cannot be zoned `lu` (or unzoned, with None): a city
    /// structure standing on it, then the parcel's own rules
    /// (`Parcel::refusal`).
    pub fn parcel_refusal(&self, i: usize, lu: Option<LandUse>) -> Option<&'static str> {
        let p = self.parcels.parcels.get(i)?;
        if self.parcel_holds_civic(i) {
            return Some("a city building stands here: bulldoze it first");
        }
        lu.and_then(|lu| p.refusal(lu))
    }

    /// [island-lots] Why there is no parcel at `p` for the Zone tool to
    /// paint, in plain words; None where there is one, or on a street or
    /// path itself (the pointer is only between parcels).
    pub fn no_parcel_why(&self, p: Vec2) -> Option<&'static str> {
        if self.parcels.at(p).is_some() {
            return None;
        }
        if !self.landscape.is_flat() && self.landscape.is_water(p) {
            return Some("open water: only the land along a road is zoned");
        }
        if self.under_facility(p) || self.ports.ports.values().any(|d| point_in_polygon(p, &d.corners())) {
            return Some("a station, yard or dock takes this ground");
        }
        let h = self.parcel_half();
        if p.x.abs() > h || p.y.abs() > h {
            return Some("past the edge of the buildable land");
        }
        let fronting = |s: &crate::roads::Segment| !s.is_rail() && !s.fronts_nothing();
        match self.roads.nearest_where(p, fronting) {
            Some((sid, _, q)) => {
                let d = q.distance(p) - self.roads.segment(sid).half_width();
                if d <= crate::zoning::SIDEWALK + 0.5 {
                    None
                } else if d <= crate::zoning::PARCEL_REACH {
                    Some("a scrap of land too small to be a parcel")
                } else {
                    Some("too far from a street or path: land is zoned along roads")
                }
            }
            None => Some("no street or path here: draw a road to zone along it"),
        }
    }
}
