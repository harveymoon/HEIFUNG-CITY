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

    /// The square parcels are cut in, the water and the shore reach.
    fn cut_inputs(&self) -> (crate::zoning::WaterMask, Option<(Vec2, Vec2)>, Option<f32>) {
        let water = self.water_mask();
        // [island-lots] The square grows to take in streets drawn past the
        // configured one (an island off the coast): parcels used to be cut
        // off at `half_extent`, so such a road had no land to zone at all.
        let h = self.parcel_half();
        // [waterfront] Down to the water along roads near it (cities since).
        let shore = self.shore.reach_water.then_some(crate::zoning::SHORE_REACH);
        (water, Some((Vec2::splat(-h), Vec2::splat(h))), shore)
    }

    /// [island-lots] Hillsides are graded (terraced lot by lot); only
    /// cliffs and water are rough.
    fn mark_rough(&self, p: &mut Parcel) {
        if !self.landscape.is_flat() {
            let ground = self.landscape.lot_ground(&p.polygon, p.center);
            p.rough_why = match ground {
                crate::landscape::LotGround::Problem(why) => Some(why),
                _ => None,
            };
            p.rough = p.rough_why.is_some();
            p.hillside = ground == crate::landscape::LotGround::Hillside;
        }
    }

    /// The parcels the city has, less those under a station, yard or dock
    /// footprint.
    fn published_parcels(&self, raw: Vec<Parcel>) -> Vec<Parcel> {
        let footprints = self.structure_footprints();
        if footprints.is_empty() {
            return raw;
        }
        raw.into_iter().filter(|p| !footprints.iter().any(|fp| point_in_polygon(p.center, fp) || polygons_overlap(&shrink_about_centroid(fp, 0.995), &p.polygon))).collect()
    }

    /// [stable-parcels] Cut the parcels for today's roads where they
    /// changed since the last cut (`zoning::recut_parcels`), keeping every
    /// other parcel exactly as it was and leaving the `locked` ground (lots
    /// under the player's buildings) whole: rough ground marked, nothing
    /// under a station, yard or dock footprint.
    pub(super) fn cut_parcels(&mut self, locked: &[Vec<Vec2>]) -> Vec<Parcel> {
        let (water, clip, shore) = self.cut_inputs();
        // Where something stands (the lots as they were): a whole chunk is
        // not recut under a building far from the works.
        let mut built: Vec<Vec2> = self.lots.values().filter(|l| l.building.is_some()).map(|l| l.center).collect();
        built.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        let mut cut = crate::prof!("cut.recut", crate::zoning::recut_parcels(&self.parcel_cut.raw, &self.parcel_cut.basis, &self.roads, &water, clip, shore, locked, &built));
        for (p, kept) in cut.parcels.iter_mut().zip(&cut.kept) {
            if !*kept {
                self.mark_rough(p);
            }
        }
        let recut_some = cut.kept.iter().any(|k| !k);
        self.parcel_cut = crate::zoning::ParcelCut { raw: cut.parcels.clone(), basis: cut.basis, saved: Vec::new(), recut_some };
        self.published_parcels(cut.parcels)
    }

    /// The parcels after a load: the saved outlines made into parcels
    /// again, or (a save from before they were saved) a full cut.
    fn restore_parcels(&mut self) -> Vec<Parcel> {
        let (water, clip, shore) = self.cut_inputs();
        let saved = std::mem::take(&mut self.parcel_cut.saved);
        let mut raw = if saved.is_empty() {
            let cut = crate::prof!("cut.generate", crate::zoning::generate_parcels_with(&self.roads, &water, clip, shore));
            // [parcel-size] Slivers, scraps and huge pieces nothing stands
            // on are tidied now; under the lots nothing changes.
            let grid = LotGrid::new(&self.lots);
            let free: Vec<bool> = cut.iter().map(|p| !grid.near(p.center, 0.0).into_iter().any(|id| self.lots[id].contains(p.center))).collect();
            crate::zoning::tidy_free_parcels(cut, &free, &self.roads, &water)
        } else {
            crate::zoning::parcels_from_outlines(&saved, &self.roads, &water)
        };
        for p in &mut raw {
            self.mark_rough(p);
        }
        let basis = crate::zoning::CutBasis::of(&self.roads, &water, clip, shore);
        self.parcel_cut = crate::zoning::ParcelCut { raw: raw.clone(), basis, saved: Vec::new(), recut_some: false };
        self.published_parcels(raw)
    }

    /// Parcels for a load (saved, else cut afresh) tied to the saved lot
    /// holding each centre: lots are saved, parcels were derived.
    pub(in crate::world) fn recut_parcels(&mut self) {
        let parcels = self.restore_parcels();
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
    ///
    /// [stable-parcels] `self.lots` holds the old lots when this is called.
    /// With `keep_lots`, a lot whose parcels all came through the recut
    /// unchanged and are still planned for its use stays exactly as it is,
    /// under the same id with its building on it; only the rest are cut
    /// and joined again. `locked` are the lots under city buildings: their
    /// ground is never recut or reshaped (the parcels round them stop at
    /// their outline); each stays the one Civic lot it is, with a zone of
    /// its own, out of the zoning until the building is bulldozed (returned,
    /// unless a road now runs over it or it fronts no way at all). `parks`
    /// are lots with a park: their parcels stay park only where the plan
    /// leaves them unzoned (a park rezoned for building is left for
    /// development to clear).
    pub(super) fn build_lots_from_parcels(&mut self, locked: &[LotId], parks: &[Vec<Vec2>], keep_lots: bool) -> Vec<LotId> {
        let old_parcels = std::mem::take(&mut self.parcels);
        let pins: Vec<Vec<Vec2>> = locked.iter().filter_map(|id| self.lots.get(*id)).map(|l| l.polygon.clone()).collect();
        let parcels = self.cut_parcels(&pins);
        self.parcels = ParcelMap::new(parcels, old_parcels.version + 1);
        if self.parcel_cut.recut_some {
            self.rebase_painted_parcels();
        }
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
        // [stable-parcels] Which old lot each unchanged parcel was part of,
        // and which old lots kept every parcel they had.
        let mut keep: Vec<LotId> = Vec::new();
        let mut members_of: HashMap<LotId, Vec<usize>> = HashMap::new();
        if keep_lots {
            let old_by_key: HashMap<ParcelKey, usize> = old_parcels.parcels.iter().enumerate().map(|(i, p)| (p.key, i)).collect();
            let mut old_count: HashMap<LotId, usize> = HashMap::new();
            for p in &old_parcels.parcels {
                if let Some(l) = p.lot {
                    *old_count.entry(l).or_default() += 1;
                }
            }
            for (i, p) in self.parcels.parcels.iter().enumerate() {
                let Some(&j) = old_by_key.get(&p.key) else { continue };
                let o = &old_parcels.parcels[j];
                if let Some(l) = o.lot.filter(|_| o.polygon == p.polygon) {
                    members_of.entry(l).or_default().push(i);
                }
            }
            let mut ids: Vec<LotId> = members_of.keys().copied().collect();
            ids.sort();
            // [fix/roads-tools] Backland zoned for a use that takes it in
            // (flag G5), not part of a usable lot yet: the empty lots of
            // that use beside it are cut again so it can be folded into
            // one (a kept lot is out of the joining).
            let in_old_lot = |i: usize| members_of.values().any(|m| m.contains(&i));
            let wanting: Vec<(usize, LandUse)> = (0..zoning.len())
                .filter_map(|i| {
                    let (lu, _) = zoning[i]?;
                    let p = &self.parcels.parcels[i];
                    (lu != LandUse::Park && lu.takes_backland() && !p.path_frontage && !p.rough && pinned_to[i].is_none() && !in_old_lot(i)).then_some((i, lu))
                })
                .collect();
            let refold = |l: &Lot| {
                if l.building.is_some() || l.land_use == LandUse::Park || !l.land_use.takes_backland() {
                    return false;
                }
                // [land-assembly] Backland that is a lot of its own stays one
                // (a company in front may join it to its site).
                if l.landlocked {
                    return false;
                }
                if wanting.is_empty() {
                    return false;
                }
                let bl = crate::geom::bounds(&l.polygon);
                wanting.iter().any(|&(i, lu)| lu == l.land_use && crate::geom::boxes_overlap(bl, crate::geom::bounds(&self.parcels.parcels[i].polygon), 0.5))
            };
            for lid in ids {
                let m = &members_of[&lid];
                let Some(l) = self.lots.get(lid) else { continue };
                if locked.contains(&lid) || old_count.get(&lid) != Some(&m.len()) {
                    continue;
                }
                if refold(l) {
                    continue;
                }
                if m.iter().all(|&i| pinned_to[i].is_none() && zoning[i].is_some_and(|(lu, _)| lu == l.land_use)) {
                    keep.push(lid);
                }
            }
        }
        // Everything else goes; the kept lots and the locked ones stay.
        self.lots.retain(|id, _| keep.binary_search(&id).is_ok() || locked.contains(&id));
        let mut done = vec![false; zoning.len()];
        for &lid in &keep {
            let m = members_of[&lid].clone();
            let first = *m.iter().min().unwrap();
            let Some((lu, gid)) = zoning[first] else { continue };
            let zid = match zone_of.get(&gid) {
                Some(z) => *z,
                None => self.zones.insert(Zone { polygon: self.lots[lid].polygon.clone(), land_use: lu }),
            };
            let rough = m.iter().any(|&k| self.parcels.parcels[k].rough);
            let roads = &self.roads;
            let lot = &mut self.lots[lid];
            lot.zone = zid;
            refresh_lot_roads(lot, roads, false);
            lot.landlocked |= rough;
            for &k in &m {
                let p = &mut self.parcels.parcels[k];
                p.lot = Some(lid);
                p.land_use = Some(lu);
                done[k] = true;
            }
        }
        // [waterfront] Unzoned backland between a lot and the water: a lot
        // in front of it takes it in, so waterfront lots (and beaches) run
        // the whole way down to the shore.
        let shore_back: Vec<usize> = (0..zoning.len())
            .filter(|&i| {
                let p = &self.parcels.parcels[i];
                zoning[i].is_none() && pinned_to[i].is_none() && !p.path_frontage && !p.rough && p.water_frontage >= crate::zoning::LotShore::MIN_FRONTAGE
            })
            .collect();
        let mut shore_taken = vec![false; shore_back.len()];
        // [land-assembly] Backland painted with no lot in front to take it
        // in is a landlocked lot of its own (flag G5 left it unzoned).
        for lu in LandUse::ALL {
            let members: Vec<usize> = (0..zoning.len()).filter(|&i| !done[i] && zoning[i].is_some_and(|(l, _)| l == lu)).collect();
            if members.is_empty() {
                continue;
            }
            let polys: Vec<Vec<Vec2>> = members.iter().map(|&i| self.parcels.parcels[i].polygon.clone()).collect();
            for (idx, poly) in join_parcels(&polys, lu, &self.roads) {
                let first = members[idx[0]];
                let Some((_, gid)) = zoning[first] else { continue };
                let (poly, shore) = self.reach_the_water(poly, &shore_back, &mut shore_taken);
                let center = if idx.len() == 1 && shore.is_empty() { self.parcels.parcels[first].center } else { inner_center(&poly) };
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
                for &k in &shore {
                    let p = &mut self.parcels.parcels[k];
                    p.lot = Some(id);
                    p.land_use = Some(lu);
                }
            }
        }
        // [free-placement] [stable-parcels] The city's lots stay as they
        // are, outline and all; only what hangs off the roads is refreshed.
        let mut pinned = Vec::new();
        for (k, &lid) in locked.iter().enumerate() {
            let Some(lot) = self.lots.get(lid) else { continue };
            // A street or track run over it (widened, or an older save's
            // lot cut before the road was): what is left, if enough is.
            if crate::zoning::corridor_over(&lot.polygon, &self.roads) {
                if let Some(rest) = crate::zoning::trim_corridors(&lot.polygon, &self.roads) {
                    let l = &mut self.lots[lid];
                    l.center = inner_center(&rest);
                    l.polygon = rest;
                }
            }
            let lot = &self.lots[lid];
            if crate::zoning::corridor_over(&lot.polygon, &self.roads) || !crate::zoning::fronts_a_way(&lot.polygon, &self.roads) {
                // Taken by the works (a project refuses this for the city's
                // structures) or cut off from every way: the building
                // finds another lot below, or goes.
                self.lots.remove(lid);
                continue;
            }
            let members: Vec<usize> = (0..pinned_to.len()).filter(|&i| pinned_to[i] == Some(k)).collect();
            let zid = self.zones.insert(Zone { polygon: lot.polygon.clone(), land_use: LandUse::Civic });
            let rough = members.iter().any(|&i| self.parcels.parcels[i].rough);
            let roads = &self.roads;
            let lot = &mut self.lots[lid];
            lot.zone = zid;
            refresh_lot_roads(lot, roads, true);
            lot.landlocked |= rough;
            for &i in &members {
                let p = &mut self.parcels.parcels[i];
                p.lot = Some(lid);
                p.land_use = Some(LandUse::Civic);
            }
            pinned.push(lid);
        }
        self.refresh_planned_zoning();
        pinned
    }

    /// [stable-parcels] After a recut, every paint stroke names the parcels
    /// it now paints: each key is found again (by itself, or by the old
    /// centre, `ParcelMap::find`) and replaced by the key of the parcel it
    /// found. Strokes made on different cuts then always name the same
    /// parcels, so a newer stroke (an unzoning) still wins over an older
    /// one wherever the works re-cut the ground under both; left on their
    /// old keys, each found its own pieces of the new parcels, and an old
    /// park's paint came back on a corner the unzoning's keys missed.
    fn rebase_painted_parcels(&mut self) {
        let parcels = &self.parcels;
        let rebase = |keys: &mut Vec<ParcelKey>| {
            let mut v: Vec<ParcelKey> = keys.iter().filter_map(|k| parcels.find(k)).map(|i| parcels.parcels[i].key).collect();
            v.sort();
            v.dedup();
            *keys = v;
        };
        for g in self.master.gestures.values_mut() {
            if let Gesture::ParcelZone { parcels: keys, .. } = g {
                rebase(keys);
            }
        }
        self.plan_changed();
    }

    /// [waterfront] A lot's outline with the unzoned backland parcels
    /// (`cands`, indices into the parcels; `taken` marks those used) that
    /// lie between it and the water joined on, while it stays one piece
    /// under the shore size cap; and the parcels it took.
    fn reach_the_water(&self, mut poly: Vec<Vec2>, cands: &[usize], taken: &mut [bool]) -> (Vec<Vec2>, Vec<usize>) {
        let mut took = Vec::new();
        if cands.is_empty() {
            return (poly, took);
        }
        for _ in 0..3 {
            let bounds = crate::geom::bounds(&poly);
            let mut grew = false;
            for (k, &i) in cands.iter().enumerate() {
                if taken[k] {
                    continue;
                }
                let p = &self.parcels.parcels[i];
                if !crate::geom::boxes_overlap(bounds, crate::geom::bounds(&p.polygon), 0.5) {
                    continue;
                }
                let Some(u) = crate::zoning::joined_outline(&[poly.clone(), p.polygon.clone()]) else { continue };
                let area = crate::geom::polygon_area(&u).abs();
                let shared = (crate::geom::perimeter(&poly) + crate::geom::perimeter(&p.polygon) - crate::geom::perimeter(&u)) * 0.5;
                if shared < 3.0 || area < (crate::geom::polygon_area(&poly).abs() + p.area) * 0.98 || area > crate::zoning::MAX_SHORE_PARCEL_AREA {
                    continue;
                }
                poly = u;
                taken[k] = true;
                took.push(i);
                grew = true;
            }
            if !grew {
                break;
            }
        }
        (poly, took)
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
        let lu = lu?;
        if let Some(why) = p.refusal(lu) {
            return Some(why);
        }
        // [land-assembly] Backland nothing in front takes in is zoned all the
        // same (Harvey 2026-10-15, replacing flag G5's refusal): a landlocked
        // lot, built on once a company on a road plot beside it joins it to
        // its site.
        None
    }

    /// [fix/roads-tools] Could backland parcel `i` be folded into a lot of
    /// `lu` in front of it (a neighbouring parcel with a frontage for it)?
    pub fn backland_has_a_front(&self, i: usize, lu: LandUse) -> bool {
        let p = &self.parcels.parcels[i];
        let (lo, hi) = crate::geom::bounds(&p.polygon);
        let reach = (hi - lo).length() * 0.5 + 4.0;
        let near: Vec<&[Vec2]> = self.parcels.in_circle(p.center, reach).into_iter().filter(|&j| j != i && self.parcels.parcels[j].path_frontage).map(|j| self.parcels.parcels[j].polygon.as_slice()).collect();
        crate::zoning::backland_joins_any(&p.polygon, near, lu, &self.roads)
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

/// [stable-parcels] A lot kept through a rebuild: its outline, centre and
/// everything on it stay; its frontage is found again on the new roads
/// (their ids change) the way it was made (`placed`: a city building's
/// lot, `make_placed_lot`).
pub(in crate::world) fn refresh_lot_roads(lot: &mut Lot, roads: &RoadNetwork, placed: bool) {
    let fresh = if placed {
        // [attractions] The city's own ground keeps its street front
        // whatever notches its plots left (a landmark over joined plots).
        crate::zoning::make_placed_lot(lot.zone, lot.polygon.clone(), lot.center, roads).filter(|f| !f.landlocked).or_else(|| crate::zoning::make_placed_lot_concave(lot.zone, lot.polygon.clone(), lot.center, roads))
    } else {
        make_lot(lot.zone, lot.land_use, lot.polygon.clone(), lot.center, roads)
    };
    let Some(f) = fresh else { return };
    lot.corners = f.corners;
    lot.front_edge = f.front_edge;
    lot.front_dir = f.front_dir;
    lot.front_segment = f.front_segment;
    lot.front_t = f.front_t;
    lot.access_node = f.access_node;
    lot.car_segment = f.car_segment;
    lot.car_point = f.car_point;
    lot.landlocked = f.landlocked;
}
