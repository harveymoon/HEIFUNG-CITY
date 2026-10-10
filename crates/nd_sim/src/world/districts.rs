//! [districts] The world side of districts (`crate::districts`): painting,
//! policies, what they change about growth, the daily facelift of what
//! already stands, and per-district statistics.
//!
//! What a policy does:
//! - **New buildings** in the district take its [`Look`] (saved on the
//!   building), which sets their architecture (in `nd_procgen`), tier and
//!   height (`Look::tier`, `Look::floors`) and the trades favoured among
//!   the ones the market can carry (`Look::favours`).
//! - **Standing buildings** are refitted to the district's look a few at a
//!   time (a quarter a day): a district changes its face over days, not in
//!   one frame, the way Cities: Skylines replaces buildings after a
//!   specialisation is set. Historic districts keep their look (an
//!   industrial sub-zone still retools its businesses there: the check for
//!   Historic comes after the industry branch in `daily_districts`).
//! - **Historic** freezes the tier: no upgrades, no decay of tier, no
//!   redevelopment (`district_frozen`, read by the daily update).
//! - **Industry** policy is an industry bias that follows the district's
//!   cells (`World::industry_bias_at`); standing industry retools a quarter
//!   a day.
//! - Eco, historic and tech campus buildings lift the land value around
//!   them (`Look::land_value`).

use super::*;
use crate::districts::{CommercialSpec, District, Look, ResidentialSpec, MAX_DISTRICTS};
use crate::industry::IndustryKind;
use crate::land_value::LandValueBuilder;

/// Share of the buildings out of step with their district refitted a day.
const FACELIFT_SHARE: f64 = 0.25;

/// What changes about one district's policy; `None` fields stay as they are.
#[derive(Clone, Debug, Default)]
pub struct PolicyChange {
    pub residential: Option<Option<ResidentialSpec>>,
    pub commercial: Option<Option<CommercialSpec>>,
    pub industry: Option<Option<IndustryKind>>,
    pub historic: Option<bool>,
    pub name: Option<String>,
    /// [supertall] The district's height limit (m), `Some(None)` to lift it.
    pub height_limit: Option<Option<f32>>,
}

/// One district's figures, for the window and the API.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DistrictStats {
    pub id: u16,
    pub name: String,
    pub policy: String,
    pub area_m2: f32,
    pub centroid: [f32; 2],
    pub lots: u32,
    pub vacant_lots: u32,
    /// Buildings standing (not sites), by land use index.
    pub buildings: u32,
    pub by_land_use: [u32; 6],
    pub by_tier: [u32; 3],
    pub under_construction: u32,
    /// Buildings already in the district's look.
    pub in_look: u32,
    pub homes: u32,
    pub residents: u32,
    pub jobs: u32,
    pub workers: u32,
    pub businesses_open: u32,
    pub hotels: u32,
    pub beds: u32,
    pub tourists_staying: u32,
    pub land_value_avg: f32,
    pub rent_avg: f32,
    pub attraction: f32,
}

impl World {
    // ── editing ─────────────────────────────────────────────────────────────

    /// A new, empty district; the name is generated when not given.
    pub fn create_district(&mut self, name: Option<String>) -> Result<u16, String> {
        let (seed, tick) = (self.config.seed, self.clock.tick);
        self.districts.create(name, seed, tick).ok_or_else(|| format!("at most {MAX_DISTRICTS} districts"))
    }

    /// Paint district `id` (0 erases) with a brush. Returns cells changed.
    pub fn paint_district_circle(&mut self, center: Vec2, radius: f32, id: u16) -> Result<usize, String> {
        if id != 0 && self.districts.get(id).is_none() {
            return Err(format!("no district {id}"));
        }
        Ok(self.districts.paint_circle(center, radius.clamp(4.0, 400.0), id))
    }

    /// Paint district `id` (0 erases) over a polygon.
    pub fn paint_district_polygon(&mut self, polygon: &[Vec2], id: u16) -> Result<usize, String> {
        if id != 0 && self.districts.get(id).is_none() {
            return Err(format!("no district {id}"));
        }
        if polygon.len() < 3 {
            return Err("a district polygon needs at least three corners".into());
        }
        Ok(self.districts.paint_polygon(polygon, id))
    }

    pub fn remove_district(&mut self, id: u16) -> bool {
        self.districts.remove(id)
    }

    /// Change a district's name or policies. The standing buildings follow
    /// over the next days (`daily_districts`).
    pub fn set_district_policy(&mut self, id: u16, change: PolicyChange) -> Result<(), String> {
        let d = self.districts.get_mut(id).ok_or_else(|| format!("no district {id}"))?;
        if let Some(r) = change.residential {
            d.residential = r;
        }
        if let Some(c) = change.commercial {
            d.commercial = c;
        }
        if let Some(i) = change.industry {
            d.industry = i;
        }
        if let Some(h) = change.historic {
            d.historic = h;
        }
        if let Some(l) = change.height_limit {
            d.height_limit = l.filter(|m| m.is_finite()).map(|m| m.clamp(10.0, crate::towers::TOWER_MAX_M));
        }
        if let Some(n) = change.name.filter(|n| !n.trim().is_empty()) {
            d.name = n.trim().chars().take(40).collect();
        }
        self.districts.version += 1;
        Ok(())
    }

    // ── reading ─────────────────────────────────────────────────────────────

    /// The district a lot is in (by its centre).
    pub fn district_of_lot(&self, lot: LotId) -> Option<&District> {
        self.lots.get(lot).and_then(|l| self.districts.at(l.center))
    }

    /// The look a new building of `lu` on `lot` takes.
    pub fn district_look_for(&self, lot: LotId, lu: LandUse) -> Option<Look> {
        self.district_of_lot(lot).and_then(|d| d.look_for(lu))
    }

    /// Standing buildings on this lot keep their tier (Historic).
    pub fn district_frozen(&self, lot: LotId) -> bool {
        self.district_of_lot(lot).is_some_and(|d| d.historic)
    }

    // ── growth hooks ────────────────────────────────────────────────────────

    /// The business a new building on `lot` should open as because of
    /// tourism or the district's policy, or `None` to choose as usual:
    /// - a hotel on commercial land while visitors are being turned away
    ///   for want of a bed (more readily in a tourist quarter);
    /// - otherwise a weighted pick of the viable kinds, the district's
    ///   favoured trades weighing more.
    pub(super) fn district_business(&mut self, lot: LotId, lu: LandUse, jobs: u16) -> Option<BusinessKind> {
        if lu != LandUse::Commercial {
            return None;
        }
        let look = self.district_look_for(lot, lu);
        let tourist_quarter = look == Some(Look::Tourist);
        let demand = self.tourism.hotel_demand;
        let hotels_building = self.buildings.values().filter(|b| b.construction.is_some() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Hotel)).count();
        // One hotel at a time is enough to answer a queue; a tourist quarter
        // builds them twice as eagerly.
        let need = crate::tourism::HOTEL_DEMAND_TO_BUILD * if tourist_quarter { 0.5 } else { 1.0 };
        let hotel_cover_ok = self.economy.cover_of(BusinessKind::Hotel).is_none_or(|c| c >= 0.85);
        if demand >= need && hotels_building < 1 + (demand / 25.0) as usize && hotel_cover_ok {
            return Some(BusinessKind::Hotel);
        }
        let look = look?;
        let favours = look.favours();
        if favours.is_empty() {
            return None;
        }
        let viable = self.business_kinds_viable(lu, Some(jobs))?;
        if viable.is_empty() {
            return None;
        }
        let weights: Vec<f32> = viable.iter().map(|k| favours.iter().find(|(f, _)| f == k).map_or(1.0, |(_, w)| *w)).collect();
        let total: f32 = weights.iter().sum();
        let mut r = crate::rng_for(self.stream_seed(Stream::DistrictBusiness), self.clock.tick ^ lot_key(lot)).random_range(0.0..total);
        for (k, w) in viable.iter().zip(&weights) {
            if r < *w {
                return Some(*k);
            }
            r -= w;
        }
        viable.last().copied()
    }

    /// Eco, historic and campus buildings lift the land around them.
    pub(super) fn splat_districts(&self, b: &mut LandValueBuilder) {
        for bld in self.buildings.values().filter(|b| b.operating()) {
            let Some((r, w)) = bld.look.and_then(|l| l.land_value()) else { continue };
            if let Some(lot) = self.lots.get(bld.lot) {
                b.splat(lot.center, r, w);
            }
        }
    }

    // ── the daily facelift ─────────────────────────────────────────────────

    /// A quarter of the buildings out of step with their district's policy
    /// are refitted each day: homes and shops to its look, industry to its
    /// sub-zone. Deterministic: each building rolls on its own seed and the
    /// day.
    pub(super) fn daily_districts(&mut self) {
        if self.districts.list.is_empty() && self.buildings.values().all(|b| b.look.is_none()) {
            return;
        }
        let day = self.clock.day() as u64;
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for id in ids {
            let b = &self.buildings[id];
            // [supertall] a league tower keeps its own architecture
            if !b.operating() || !b.is_grown() || b.plot.tower.is_some() {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let district = self.districts.at(lot.center).cloned();
            let mut rng = crate::rng_for(b.seed ^ 0xFACE, day);
            if !rng.random_bool(FACELIFT_SHARE) {
                continue;
            }
            // Industry: retool to the district's sub-zone.
            if b.land_use == LandUse::Industrial {
                let Some(kind) = district.as_ref().and_then(|d| d.industry) else { continue };
                if b.business.as_ref().is_none_or(|z| z.kind.industry() == Some(kind) || z.kind == BusinessKind::Parking) {
                    continue;
                }
                let lot_id = b.lot;
                let Some(site) = self.industry_site(lot_id) else { continue };
                let Some(bk) = crate::industry::pick_business(&crate::industry::business_weights(kind, &site), rng.random::<f32>()) else { continue };
                let b = &mut self.buildings[id];
                let jobs = b.jobs;
                let old = b.business.take();
                let mut bz = Business::new(bk, jobs);
                if let Some(old) = old {
                    bz.money = old.money;
                    bz.closed = old.closed;
                    bz.broke_days = old.broke_days;
                }
                b.business = Some(bz);
                b.floors = b.floors.min(kind.max_floors()).max(1);
                b.seed = b.seed.wrapping_add(1);
                b.recompute_capacity();
                self.fit_business_name(id);
                self.events.push(SimEvent::BuildingChanged(id));
                continue;
            }
            // Historic districts keep what stands, whatever it looks like.
            if district.as_ref().is_some_and(|d| d.historic) {
                continue;
            }
            let want = district.as_ref().and_then(|d| d.look_for(b.land_use));
            if b.look == want {
                continue;
            }
            let value = self.lot_value(b.lot);
            let b = &mut self.buildings[id];
            b.look = want;
            // A refit keeps the tenants and the size; the tier follows the
            // look (the stacks come down a step, a campus goes up).
            if let Some(l) = want {
                let t = l.tier(b.tier);
                if t != b.tier {
                    b.tier = t;
                    b.recompute_capacity();
                    b.update_rent(value);
                }
            }
            self.events.push(SimEvent::BuildingChanged(id));
        }
    }

    // ── statistics ─────────────────────────────────────────────────────────

    /// Every district's figures, in id order.
    pub fn district_stats(&self) -> Vec<DistrictStats> {
        let ext = self.districts.grid.extents();
        let mut out: Vec<DistrictStats> = self
            .districts
            .list
            .iter()
            .map(|d| {
                let e = ext.get(&d.id);
                DistrictStats {
                    id: d.id,
                    name: d.name.clone(),
                    policy: d.policy_label(),
                    area_m2: e.map_or(0.0, |e| e.area_m2()),
                    centroid: e.map_or([0.0, 0.0], |e| e.centroid().to_array()),
                    ..Default::default()
                }
            })
            .collect();
        let index: HashMap<u16, usize> = out.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
        let mut lv_sum = vec![0.0f32; out.len()];
        let mut rent_n = vec![0u32; out.len()];
        let mut rent_sum = vec![0.0f32; out.len()];
        for (lot_id, lot) in self.lots.iter() {
            let id = self.districts.grid.at(lot.center);
            let Some(&i) = index.get(&id) else { continue };
            let s = &mut out[i];
            s.lots += 1;
            lv_sum[i] += self.land_value.at(lot.center);
            let Some(b) = lot.building.and_then(|b| self.buildings.get(b)) else {
                if lot.usable() {
                    s.vacant_lots += 1;
                }
                continue;
            };
            // [review] A merged building spans several lots: counted once,
            // at its own (it was counted once per plot, two to four times).
            if b.lot != lot_id {
                continue;
            }
            if b.construction.is_some() {
                s.under_construction += 1;
                continue;
            }
            s.buildings += 1;
            s.by_land_use[b.land_use.index()] += 1;
            s.by_tier[b.tier.index()] += 1;
            let want = self.districts.get(id).and_then(|d| d.look_for(b.land_use));
            if want.is_some() && b.look == want {
                s.in_look += 1;
            }
            s.homes += b.homes as u32;
            s.residents += b.residents.len() as u32;
            s.jobs += b.jobs as u32;
            s.workers += b.workers.len() as u32;
            if let Some(z) = b.business.as_ref().filter(|z| !z.closed) {
                s.businesses_open += 1;
                if z.kind == BusinessKind::Hotel {
                    s.hotels += 1;
                    s.beds += self.hotel_beds(lot.building.unwrap()) as u32;
                }
            }
            if b.homes > 0 {
                rent_n[i] += 1;
                rent_sum[i] += b.rent;
            }
        }
        for t in self.tourism.tourists.values() {
            if let Some(&i) = self.building_lot(t.hotel).and_then(|l| index.get(&self.districts.grid.at(l.center))) {
                out[i].tourists_staying += 1;
            }
        }
        for a in &self.tourism.attractions {
            if let Some(&i) = index.get(&self.districts.grid.at(a.pos)) {
                out[i].attraction += a.score;
            }
        }
        for (i, s) in out.iter_mut().enumerate() {
            s.land_value_avg = if s.lots > 0 { lv_sum[i] / s.lots as f32 } else { 0.0 };
            s.rent_avg = if rent_n[i] > 0 { rent_sum[i] / rent_n[i] as f32 } else { 0.0 };
        }
        out
    }
}

/// A lot's key as a number, to salt a roll by lot.
fn lot_key(lot: LotId) -> u64 {
    use slotmap::Key;
    lot.data().as_ffi()
}
