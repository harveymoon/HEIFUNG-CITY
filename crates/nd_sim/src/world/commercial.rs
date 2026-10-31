//! [r4-leisure] The world's side of the commercial sub-zones
//! (`crate::commercial`): the player's sub-zone over the ground, the city's
//! pick for lots on auto, and the demand per sub-zone for the panels.

use super::*;
use crate::commercial::{AutoSite, CommercialBias, CommercialSub, HOMES_REACH, MAX_COMMERCIAL_BIASES};

impl World {
    /// The commercial sub-zone set over `p`: `Some(Some(sub))` a sub-zone,
    /// `Some(None)` "auto" over an older one, `None` nothing set (auto).
    pub fn commercial_bias_at(&self, p: Vec2) -> Option<Option<CommercialSub>> {
        self.commercial_bias.iter().rev().find(|b| b.polygon.len() >= 3 && point_in_polygon(p, &b.polygon)).map(|b| b.sub)
    }

    /// Set (or with `None` hand back to the city) the commercial sub-zone
    /// of an area. It belongs to the ground, so it survives the lots being
    /// recut; standing businesses stay as they are. Leisure is a land use
    /// of its own and is zoned, not set here.
    pub fn set_commercial_bias(&mut self, polygon: Vec<Vec2>, sub: Option<CommercialSub>) -> Result<(), String> {
        if polygon.len() < 3 {
            return Err("a commercial area needs at least three corners".into());
        }
        if sub == Some(CommercialSub::Leisure) {
            return Err("leisure is zoned (land use leisure), not set over the ground".into());
        }
        self.commercial_bias.push(CommercialBias { polygon, sub });
        if self.commercial_bias.len() > MAX_COMMERCIAL_BIASES {
            let extra = self.commercial_bias.len() - MAX_COMMERCIAL_BIASES;
            self.commercial_bias.drain(..extra);
        }
        Ok(())
    }

    /// A choice key from a paint stroke ("auto", "offices", ...).
    pub(super) fn apply_commercial_choice(&mut self, polygon: Vec<Vec2>, choice: &str) {
        let sub = CommercialSub::from_key(choice).filter(|s| *s != CommercialSub::Leisure);
        let _ = self.set_commercial_bias(polygon, sub);
    }

    /// The sub-zone the player set on a lot's land: leisure for leisure
    /// land, else the sub-zone over its centre; `None` on auto or off
    /// commercial land.
    pub fn commercial_sub_set(&self, lot: LotId) -> Option<CommercialSub> {
        let l = self.lots.get(lot)?;
        match l.land_use {
            LandUse::Entertainment => Some(CommercialSub::Leisure),
            LandUse::Commercial => self.commercial_bias_at(l.center).flatten(),
            _ => None,
        }
    }

    /// The sub-zone choice over a point, as a key ("auto" when none), for
    /// the Zone tool's read-out.
    pub fn commercial_choice_at(&self, p: Vec2) -> Option<String> {
        self.commercial_bias_at(p).map(|s| s.map_or("auto", |s| s.key()).to_string())
    }

    /// What a standing commercial building is, by its land and business.
    pub fn commercial_sub_of_building(&self, bid: BuildingId) -> Option<CommercialSub> {
        let b = self.buildings.get(bid)?;
        if b.land_use == LandUse::Entertainment {
            return Some(CommercialSub::Leisure);
        }
        if b.land_use != LandUse::Commercial {
            return None;
        }
        if let Some(s) = self.commercial_sub_set(b.lot) {
            return Some(s);
        }
        // [r4-leisure-places] by its kind when only one sub-zone grows it
        Some(match b.business.as_ref().map(|z| z.kind).and_then(CommercialSub::of_kind) {
            Some(s) => s,
            _ if b.plot.form.is_some_and(|f| f.is_mall()) || b.floors >= 4 => CommercialSub::Retail,
            _ => CommercialSub::Neighbourhood,
        })
    }

    /// What the city weighs for a lot on auto.
    pub fn commercial_auto_site(&self, lot: LotId) -> Option<AutoSite> {
        let l = self.lots.get(lot)?;
        let lanes = self.roads.try_segment(l.front_segment).map_or(2, |s| s.lanes());
        let homes_near = self
            .buildings
            .values()
            .filter(|b| b.homes > 0 && b.operating())
            .filter_map(|b| self.lots.get(b.lot).map(|x| (b.homes, x.center)))
            .filter(|(_, c)| c.distance_squared(l.center) <= HOMES_REACH * HOMES_REACH)
            .map(|(h, _)| h as f32)
            .sum();
        let nightlife = self.districts.at(l.center).is_some_and(|d| d.policies.late_hours || d.story.is_some_and(crate::stories::Story::is_nightlife));
        Some(AutoSite {
            lanes,
            homes_near,
            land_value: self.land_value.for_use(l.center, LandUse::Commercial),
            shop_demand: self.economy.demand_for(LandUse::Commercial),
            leisure_demand: self.economy.demand_for(LandUse::Entertainment),
            nightlife,
            population: self.citizens.len() as u32,
            unemployment: self.economy.unemployment_share(),
            seed: crate::hash::mix(self.config.seed, (l.center.x.to_bits() as u64) << 32 | l.center.y.to_bits() as u64),
        })
    }

    /// Which sub-zones the market would open a business in now, for a
    /// building of `jobs` (`None`: any size).
    pub fn commercial_subs_viable(&self, jobs: Option<u16>) -> [bool; 4] {
        CommercialSub::ALL.map(|s| !self.kinds_viable(s.menu(), jobs).is_empty())
    }

    /// Are the grocers' jobs open or going up short of what the town eats?
    fn grocers_short(&self) -> bool {
        let jobs: u32 = self.buildings.values().filter(|b| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Grocery && !z.closed)).map(|b| if b.construction.is_some() { b.planned_capacity().1 as u32 } else { b.jobs as u32 }).sum();
        (jobs as f32) < self.grocery_jobs_needed()
    }

    /// The city's pick for a commercial lot on auto (`None`: the market
    /// wants none of them; the land use's full menu decides as before).
    pub fn commercial_auto_pick(&self, lot: LotId, jobs: Option<u16>) -> Option<CommercialSub> {
        let site = self.commercial_auto_site(lot)?;
        let mut ok = self.commercial_subs_viable(jobs);
        // While the town is short of grocers, a lot on auto is a shop, not
        // a club: food first.
        if ok[CommercialSub::Leisure.index()] && self.grocers_short() {
            ok[CommercialSub::Leisure.index()] = false;
        }
        site.pick(ok)
    }

    /// The sub-zone a new building on a commercial lot goes up as: the
    /// player's, else the city's pick. `None` off commercial land or when
    /// the market wants nothing on auto.
    pub fn commercial_sub_for_new_building(&self, lot: LotId, jobs: Option<u16>) -> Option<CommercialSub> {
        match self.lots.get(lot)?.land_use {
            LandUse::Entertainment => Some(CommercialSub::Leisure),
            LandUse::Commercial => self.commercial_sub_set(lot).or_else(|| self.commercial_auto_pick(lot, jobs)),
            _ => None,
        }
    }

    /// [r4-leisure] How readily a commercial lot on auto in a nightlife
    /// district develops: as quickly as the demand for nights out when that
    /// is the higher (a club may go up there), else as shops'. Cheap: the
    /// develop loop asks it of every vacant commercial lot.
    pub(super) fn commercial_demand_at(&self, lot: &Lot) -> f32 {
        let shops = self.economy.demand_for(LandUse::Commercial);
        if self.commercial_bias.is_empty() || self.commercial_bias_at(lot.center).flatten().is_none() {
            let nightlife = self.districts.at(lot.center).is_some_and(|d| d.policies.late_hours || d.story.is_some_and(crate::stories::Story::is_nightlife));
            if nightlife {
                return shops.max(self.economy.demand_for(LandUse::Entertainment));
            }
        }
        shops
    }

    /// The demand per commercial sub-zone (`CommercialSub::ALL` order):
    /// neighbourhood, retail and offices share the shops' and offices'
    /// demand while the market would open one of their kinds (else 0: a
    /// lot zoned for them waits); leisure is the demand for nights out.
    pub fn commercial_demand(&self) -> [f32; 4] {
        let shops = self.economy.demand_for(LandUse::Commercial);
        let ok = self.commercial_subs_viable(None);
        CommercialSub::ALL.map(|s| match s {
            CommercialSub::Leisure => self.economy.demand_for(LandUse::Entertainment),
            _ if ok[s.index()] => shops,
            _ => 0.0,
        })
    }

    /// The panels' commercial bar: the highest of the sub-zones' demand.
    pub fn commercial_demand_overall(&self) -> f32 {
        self.commercial_demand().into_iter().fold(0.0, f32::max)
    }
}
