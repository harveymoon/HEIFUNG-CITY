//! [w1] Building sites as the player reads them: what is going up and what
//! is coming down (the Construction map layer), what will stand on a lot
//! when the work is done (the inspector), and how much of the demand for
//! new buildings the sites already under way will meet (the Demand panel).
//! Read-only: nothing here changes the city.

use super::*;
use crate::towers::FLOOR_M;

/// Which way a site is going.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SiteKind {
    /// A building going up (a new one, or a redevelopment once the old one
    /// is down).
    GoingUp,
    /// A building coming down with a successor already planned for the lot
    /// (a redevelopment, or a plot bought for a building next door).
    Replacing,
    /// A building coming down with nothing planned: the lot is left vacant.
    Clearing,
}

impl SiteKind {
    pub const ALL: [SiteKind; 3] = [SiteKind::GoingUp, SiteKind::Replacing, SiteKind::Clearing];

    pub fn key(self) -> &'static str {
        match self {
            SiteKind::GoingUp => "going_up",
            SiteKind::Replacing => "coming_down_rebuild",
            SiteKind::Clearing => "coming_down",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SiteKind::GoingUp => "going up",
            SiteKind::Replacing => "coming down, rebuild planned",
            SiteKind::Clearing => "coming down, lot left vacant",
        }
    }
}

/// What will stand on a site's lot when the work is done.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedBuilding {
    /// The building that will stand there: the site itself (a building
    /// going up, a redevelopment's successor), or the building that bought
    /// the plot to spread onto it (a campus buying for an annex names the
    /// university).
    pub building: BuildingId,
    /// "Riverside Lofts", or "a campus dorm".
    pub name: String,
    pub land_use: LandUse,
    pub tier: Tier,
    pub floors: u8,
    /// Planned height in metres (floors at the land use's storey height, or
    /// a tower's own height).
    pub height_m: f32,
    /// The plot joins a bigger building next door rather than getting one
    /// of its own.
    pub joins: bool,
}

/// A building site or demolition, for the player.
#[derive(Clone, Debug, PartialEq)]
pub struct SitePlan {
    pub kind: SiteKind,
    /// Share of the current work done, `0..1` (the building, or the
    /// demolition while the old building comes down).
    pub progress: f32,
    /// Floors of what is coming down (demolitions only).
    pub coming_down_floors: Option<u8>,
    /// What stands there when the work is done; `None`: a vacant lot.
    pub next: Option<PlannedBuilding>,
}

impl World {
    /// [w1] The site at `bid`, if it is one: going up or coming down, how
    /// far, and what is planned to stand there afterwards.
    pub fn site_plan(&self, bid: BuildingId) -> Option<SitePlan> {
        let b = self.buildings.get(bid)?;
        if let Some(d) = &b.demolition {
            let next = if d.clear_lot { self.planned_for_lot(b.lot, bid) } else { Some(self.planned_self(bid)) };
            let kind = if next.is_some() { SiteKind::Replacing } else { SiteKind::Clearing };
            return Some(SitePlan { kind, progress: d.progress, coming_down_floors: Some(d.floors), next });
        }
        let c = b.construction.as_ref()?;
        Some(SitePlan { kind: SiteKind::GoingUp, progress: c.progress, coming_down_floors: None, next: Some(self.planned_self(bid)) })
    }

    /// Every site in the city, with its kind and progress (slot order).
    pub fn construction_sites(&self) -> Vec<(BuildingId, SiteKind, f32)> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.construction.is_some() || b.demolition.is_some())
            .filter_map(|(id, _)| self.site_plan(id).map(|p| (id, p.kind, p.progress)))
            .collect()
    }

    /// The building `bid` as it will stand when finished.
    fn planned_self(&self, bid: BuildingId) -> PlannedBuilding {
        let b = &self.buildings[bid];
        PlannedBuilding { building: bid, name: b.name.clone(), land_use: b.land_use, tier: b.tier, floors: b.floors, height_m: self.building_height(bid), joins: false }
    }

    /// What is planned for `lot` once `site` (coming down) has cleared it:
    /// a campus annex, or a plot bought for a bigger building next door.
    fn planned_for_lot(&self, lot: LotId, site: BuildingId) -> Option<PlannedBuilding> {
        for k in &self.corps.campuses {
            if let Some((_, annex)) = k.pending.iter().find(|(l, _)| *l == lot) {
                let (tier, floors) = self.annex_size(lot, *annex)?;
                let lu = self.lots.get(lot).map_or(LandUse::Civic, |l| l.land_use);
                let campus = self.buildings.get(k.building).map_or_else(|| "the campus".to_string(), |b| b.name.clone());
                return Some(PlannedBuilding { building: k.building, name: format!("{} for {campus}", annex.label().to_lowercase()), land_use: lu, tier, floors, height_m: floors as f32 * FLOOR_M[lu.index()], joins: false });
            }
        }
        let owner = *self.estate.reserved.get(&lot)?;
        if owner == site || !self.buildings.contains_key(owner) {
            return None;
        }
        Some(PlannedBuilding { joins: true, ..self.planned_self(owner) })
    }

    /// [w1] How much of each land use's demand the sites under way already
    /// meet: the demand there would be without them, less the demand there
    /// is (`Economy::demand_with`), one land use's sites at a time. Zero
    /// for venues (they follow the population and the market).
    /// [civic-demand] Civic offices and parks going up count against their
    /// need.
    pub fn demand_under_way(&self) -> [f32; 6] {
        let e = &self.economy;
        let mut by_use = [(0u32, 0u32); 6];
        for b in self.buildings.values() {
            if b.construction.is_some() {
                let (h, j) = b.planned_capacity();
                let x = &mut by_use[b.land_use.index()];
                x.0 += h as u32;
                x.1 += j as u32;
            }
        }
        let real = e.under_way();
        let now = e.demand_with(real);
        let mut out = [0.0f32; 6];
        for lu in LandUse::ALL {
            let i = lu.index();
            let (h, j) = by_use[i];
            let mut without = crate::economy::UnderWay { homes: real.homes.saturating_sub(h), jobs: real.jobs.saturating_sub(j), ..real };
            match lu {
                LandUse::Civic => without.civic_m2 = 0.0,
                LandUse::Park => without.park_m2 = 0.0,
                _ => {}
            }
            if without == real {
                continue;
            }
            out[i] = (e.demand_with(without)[i] - now[i]).max(0.0);
        }
        out
    }

    /// [w1] Sites going up per land use (demolitions not counted), and the
    /// homes and jobs they will bring.
    pub fn sites_by_use(&self) -> [(u32, u32, u32); 6] {
        let mut out = [(0u32, 0u32, 0u32); 6];
        for b in self.buildings.values() {
            if b.construction.is_some() && !b.being_demolished() {
                let (h, j) = b.planned_capacity();
                let x = &mut out[b.land_use.index()];
                x.0 += 1;
                x.1 += h as u32;
                x.2 += j as u32;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings::DemolitionReason;

    fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
    }

    fn town() -> World {
        let mut w = World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential).unwrap();
        w
    }

    fn two_lots(w: &World) -> (LotId, LotId) {
        let mut ids: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Residential).map(|(id, _)| id).collect();
        assert!(ids.len() >= 2, "two vacant lots");
        ids.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x));
        (ids[0], ids[1])
    }

    fn build(w: &mut World, lot: LotId) -> BuildingId {
        w.build_on(lot);
        w.lots[lot].building.expect("built")
    }

    #[test]
    fn a_site_going_up_plans_its_own_height() {
        let mut w = town();
        let (a, _) = two_lots(&w);
        let bid = build(&mut w, a);
        let b = &w.buildings[bid];
        assert!(b.construction.is_some());
        let plan = w.site_plan(bid).expect("a site");
        assert_eq!(plan.kind, SiteKind::GoingUp);
        let next = plan.next.expect("itself");
        assert_eq!(next.building, bid);
        assert_eq!(next.floors, b.floors);
        assert!((next.height_m - b.floors as f32 * FLOOR_M[LandUse::Residential.index()]).abs() < 1e-3);
        assert_eq!(w.construction_sites().len(), 1);
        // Once open it is no site.
        w.finish_construction(bid);
        assert!(w.site_plan(bid).is_none());
        assert!(w.construction_sites().is_empty());
    }

    #[test]
    fn a_redevelopment_comes_down_then_plans_its_successor() {
        let mut w = town();
        let (a, _) = two_lots(&w);
        let bid = build(&mut w, a);
        w.finish_construction(bid);
        let old = w.buildings[bid].floors;
        w.redevelop(bid, Tier::High, old + 5, 0.9);
        let plan = w.site_plan(bid).unwrap();
        assert_eq!(plan.kind, SiteKind::Replacing);
        assert_eq!(plan.coming_down_floors, Some(old));
        assert_eq!(plan.next.as_ref().unwrap().floors, old + 5);
        assert_eq!(w.construction_sites()[0].1, SiteKind::Replacing);
    }

    #[test]
    fn a_plain_demolition_leaves_nothing_planned_unless_the_plot_was_bought() {
        let mut w = town();
        let (a, b) = two_lots(&w);
        let site = build(&mut w, a);
        w.finish_construction(site);
        let big = build(&mut w, b);
        assert!(w.begin_demolition(site, None, None));
        w.tag_demolition(site, DemolitionReason::Bulldozed);
        let plan = w.site_plan(site).unwrap();
        assert_eq!(plan.kind, SiteKind::Clearing);
        assert!(plan.next.is_none());
        // A neighbour bought the plot: its building is what comes next.
        w.estate.reserved.insert(a, big);
        let plan = w.site_plan(site).unwrap();
        assert_eq!(plan.kind, SiteKind::Replacing);
        let next = plan.next.unwrap();
        assert_eq!(next.building, big);
        assert!(next.joins);
        assert_eq!(next.floors, w.buildings[big].floors);
    }

    #[test]
    fn homes_going_up_meet_some_of_the_housing_demand() {
        let mut w = town();
        let (a, b) = two_lots(&w);
        for l in [a, b] {
            build(&mut w, l);
        }
        w.update_economy();
        let e = &w.economy;
        assert!(e.homes_building > 0);
        // People waiting for a home, a few more than the sites will take:
        // the sites count against them, and the demand is not at its cap.
        w.economy.waiting_immigrants = w.economy.homes_building / 2 + 2;
        w.economy.recompute();
        let under = w.demand_under_way();
        let r = LandUse::Residential.index();
        assert!(under[r] > 0.0, "the homes going up meet some of the demand: {under:?}");
        assert!(w.economy.demand[r] + under[r] <= 1.0 + 1e-4 || w.economy.demand[r] >= 1.0 - 1e-4);
        assert_eq!(under[LandUse::Park.index()], 0.0);
        let sites = w.sites_by_use();
        assert_eq!(sites[r].0, 2);
        assert!(sites[r].1 > 0);
        // Recompute is what demand_with says for the real figures.
        let e = &w.economy;
        assert_eq!(e.demand, e.demand_with(e.under_way()));
    }

    /// [civic-demand] The civic demand for a town of `pop` as it stands.
    fn civic_demand(w: &mut World, pop: u32) -> f32 {
        w.update_economy();
        w.economy.population = pop;
        w.economy.recompute();
        w.economy.demand[LandUse::Civic.index()]
    }

    #[test]
    fn civic_offices_built_bring_the_civic_demand_down() {
        let mut w = World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        let c = LandUse::Civic.index();
        let pop = 1500;
        let bare = civic_demand(&mut w, pop);
        assert!(bare > 0.99, "a town of {pop} with no civic land: {bare}");
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Civic).unwrap();
        let zoned = civic_demand(&mut w, pop);
        assert!(w.economy.civic_m2_zoned > 0.0);
        assert!(zoned < bare && zoned >= 0.5, "zoning eases it a little, never all the way: {zoned}");
        let mut lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Civic).map(|(id, _)| id).collect();
        lots.sort_by(|a, b| w.lots[*a].center.x.total_cmp(&w.lots[*b].center.x));
        assert!(lots.len() >= 2, "civic lots");
        let site = build(&mut w, lots[0]);
        let going_up = civic_demand(&mut w, pop);
        assert!(w.economy.civic_m2_building > 0.0 && w.economy.civic_m2 == 0.0);
        assert!(going_up < zoned, "a civic site going up counts: {going_up} after {zoned}");
        assert!(w.demand_under_way()[c] > 0.0, "the light part of the bar");
        w.finish_construction(site);
        let open = civic_demand(&mut w, pop);
        assert!(w.economy.civic_m2 > 0.0 && w.economy.civic_m2_building == 0.0);
        assert!(open <= going_up + 1e-5, "{open} after {going_up}");
        // Every lot built: the town of 1500 has more than it needs.
        for &l in &lots[1..] {
            let b = build(&mut w, l);
            w.finish_construction(b);
        }
        let full = civic_demand(&mut w, pop);
        assert!(w.economy.civic_m2 >= w.economy.civic_need_m2(), "{} m2 for a need of {}", w.economy.civic_m2, w.economy.civic_need_m2());
        assert!(full <= 0.06, "ample civic: {full}");
        // The town grows: it wants more again.
        let grown = civic_demand(&mut w, 30_000);
        assert!(grown > 0.5, "a city of 30000: {grown}");
    }

    #[test]
    fn parks_laid_out_bring_the_park_demand_down() {
        let mut w = World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        let p = LandUse::Park.index();
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Park).unwrap();
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Park).map(|(id, _)| id).collect();
        let area: f32 = lots.iter().map(|l| w.lots[*l].area()).sum();
        // A town that needs about twice the parkland zoned.
        let pop = (area * 2.0 / crate::economy::PARK_M2_PER_HEAD) as u32;
        let park = |w: &mut World| {
            w.update_economy();
            w.economy.population = pop;
            w.economy.recompute();
            w.economy.demand[p]
        };
        let zoned = park(&mut w);
        for &l in &lots {
            let b = build(&mut w, l);
            w.finish_construction(b);
        }
        let open = park(&mut w);
        assert!((w.economy.park_m2 - area).abs() < 1.0, "{} of {area}", w.economy.park_m2);
        assert!(open < zoned - 0.1, "{open} after {zoned}");
        assert!((0.4..0.6).contains(&open), "half the parkland it needs: {open}");
    }
}
