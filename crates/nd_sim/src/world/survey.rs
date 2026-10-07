//! The World side of the land survey (`crate::survey`): what an industrial
//! lot's surroundings are ([`SiteContext`], the one place the industry
//! choice and the suitability layers both read), and the survey layers'
//! values over a [`LandSurvey`] grid or at a single point.
//!
//! Read-only: nothing here touches the RNG or the world, so turning a map
//! layer on cannot change the city.

use super::industry::{near, CRUDE_REACH};
use super::*;
use crate::ground::{self, GroundSample};
use crate::industry::{self, IndustryKind, IndustrySite};
use crate::ports::PortKind;
use crate::survey::{self, LandLayer, LandSurvey};

/// Industry-relevant surroundings, gathered once from the world so that
/// many sites can be scored without walking every building each time:
/// shipping ports, freight yards, bus depots, highway pieces, and the
/// buildings that count as industrial neighbours, oil wells or truck lots.
pub struct SiteContext {
    ports: Vec<Vec2>,
    yards: Vec<Vec2>,
    depots: Vec<Vec2>,
    highway: Vec<(Vec2, Vec2)>,
    /// Lot, centre, the industry kind it reads as, a working oil well, a fleet lot.
    others: Vec<(LotId, Vec2, Option<IndustryKind>, bool, bool)>,
    /// [dirty-industry] Working strip mines (ore for a smelter).
    mines: Vec<(LotId, Vec2)>,
    /// [economy] Working cattle and chicken farms, and fishing ports.
    livestock: Vec<(LotId, Vec2)>,
    slaughterhouses: Vec<(LotId, Vec2)>,
    fishing_ports: Vec<Vec2>,
}

impl SiteContext {
    /// What the industry choice weighs at `c`, given the ground there and its
    /// contamination; buildings on `exclude` (the lot being decided) do not
    /// count as their own neighbours. The same rules as ever for a lot
    /// (`World::industry_site`); the survey asks it about grid points.
    pub fn site(&self, w: &World, c: Vec2, ground: GroundSample, contamination: f32, exclude: Option<LotId>) -> IndustrySite {
        let nearest = |pts: &[Vec2]| pts.iter().map(|p| p.distance(c)).fold(f32::INFINITY, f32::min);
        let port = near(nearest(&self.ports), 80.0, 420.0);
        let yard = near(nearest(&self.yards), 80.0, 360.0);
        let depot = near(nearest(&self.depots), 60.0, 350.0);
        let hw = self.highway.iter().map(|(a, b)| crate::geom::project_on_segment(c, *a, *b).1.distance(c)).fold(f32::INFINITY, f32::min);
        let highway = near(hw, 30.0, 200.0);
        let mut neighbours = Vec::new();
        let mut dirty = Vec::new();
        let mut fleet_lot_near = false;
        let mut crude_wells = 0u32;
        for &(lot, at, kind, well, fleet) in &self.others {
            if Some(lot) == exclude {
                continue;
            }
            let d = at.distance(c);
            if well && d < CRUDE_REACH {
                crude_wells += 1;
            }
            if fleet && d < 300.0 {
                fleet_lot_near = true;
            }
            if let Some(k) = kind {
                if d < industry::NEIGHBOUR_RADIUS {
                    neighbours.push((k, d));
                }
                // [dirty-industry] Dirty plants count as far as they carry.
                if k.is_dirty() && d < k.dirt_reach() {
                    dirty.push((k, d));
                }
            }
        }
        neighbours.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        dirty.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let ore_mines = self.mines.iter().filter(|(lot, at)| Some(*lot) != exclude && at.distance(c) < industry::ORE_REACH).count() as u32;
        let livestock_farms = self.livestock.iter().filter(|(lot, at)| Some(*lot) != exclude && at.distance(c) < industry::LIVESTOCK_REACH).count() as u32;
        let slaughterhouses = self.slaughterhouses.iter().filter(|(lot, at)| Some(*lot) != exclude && at.distance(c) < industry::LIVESTOCK_REACH).count() as u32;
        let fishing_port = near(nearest(&self.fishing_ports), 120.0, 700.0);
        IndustrySite { ground, contamination, land_value: w.land_value.at(c), port, yard, highway, depot, neighbours, fleet_lot_near, crude_wells, livestock_farms, slaughterhouses, fishing_port, ore_mines, dirty }
    }
}

impl World {
    /// Gather the surroundings industry weighs (see [`SiteContext`]).
    pub fn site_context(&self) -> SiteContext {
        let centre = |b: BuildingId| self.building_lot(b).map(|l| l.center);
        let ports = self.ports.working(PortKind::ShippingPort).into_iter().filter_map(centre).collect();
        let yards = self.yards.iter().copied().filter_map(centre).collect();
        let depots = self.buildings.iter().filter(|(_, b)| b.depot && !b.is_derelict()).filter_map(|(id, _)| centre(id)).collect();
        let highway = self.roads.alive_segments().filter(|(_, s)| s.is_highway()).map(|(sid, _)| self.roads.segment_points(sid)).collect();
        let mut others = Vec::new();
        let mut livestock = Vec::new();
        let mut slaughterhouses = Vec::new();
        let mut mines = Vec::new();
        let fishing_ports = self.ports.ports.values().filter(|p| p.kind.is_fishing() && p.works()).filter_map(|p| p.building).filter_map(centre).collect();
        for b in self.buildings.values() {
            let Some(l) = self.lots.get(b.lot) else { continue };
            let kind = match (&b.business, b.plant) {
                (Some(z), _) => z.kind.industry(),
                // Coal smoke and reactor fears read as heavy industry.
                (None, Some(PowerPlantKind::Coal)) | (None, Some(PowerPlantKind::Nuclear)) => Some(IndustryKind::Refinery),
                _ => None,
            };
            let well = b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::OilWell && !z.closed);
            let fleet = b.business.as_ref().is_some_and(|z| z.kind.is_truck_lot());
            if kind.is_some() || well || fleet {
                others.push((b.lot, l.center, kind, well, fleet));
            }
            if b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::StripMine && !z.closed) {
                mines.push((b.lot, l.center));
            }
            if b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::CowFarm | BusinessKind::ChickenFarm | BusinessKind::Feedlot) && !z.closed) {
                livestock.push((b.lot, l.center));
            }
            if b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Slaughterhouse && !z.closed) {
                slaughterhouses.push((b.lot, l.center));
            }
        }
        SiteContext { ports, yards, depots, highway, others, mines, livestock, slaughterhouses, fishing_ports }
    }

    /// The ground's industry weights at every corner of the survey, in
    /// `IndustryKind::ALL` order: what a new industrial lot there would draw
    /// from, by the sim's own [`industry::weights`]. The player's area bias
    /// is left out on purpose: the layer shows what the ground favours, which
    /// is what the player wants to know before setting one. Water corners
    /// are all zero.
    pub fn survey_industry_weights(&self, s: &LandSurvey) -> Vec<[f32; crate::industry::IndustryKind::COUNT]> {
        let ctx = self.site_context();
        s.corners()
            .enumerate()
            .map(|(k, p)| {
                if s.wet[k] {
                    return [0.0; crate::industry::IndustryKind::COUNT];
                }
                industry::weights(&ctx.site(self, p, s.ground[k], self.contamination.at(p), None))
            })
            .collect()
    }

    /// A survey layer's value at every corner, `0..1`: the resource for the
    /// resource layers, the chance of the kind for an industry layer (pass
    /// the weights from [`World::survey_industry_weights`] to reuse them),
    /// and for "best industry" the favourite's chance.
    pub fn survey_values(&self, s: &LandSurvey, layer: LandLayer, weights: Option<&[[f32; crate::industry::IndustryKind::COUNT]]>) -> Vec<f32> {
        if layer.is_industry() {
            let own;
            let w = match weights {
                Some(w) => w,
                None => {
                    own = self.survey_industry_weights(s);
                    &own
                }
            };
            return w
                .iter()
                .map(|w| {
                    let sh = survey::shares(w);
                    match layer {
                        LandLayer::Industry(k) => sh[k.index()],
                        _ => sh.iter().copied().fold(0.0, f32::max),
                    }
                })
                .collect();
        }
        let need_contamination = matches!(layer, LandLayer::Farmland | LandLayer::Contamination) && !self.contamination.cells.is_empty();
        s.corners()
            .enumerate()
            .map(|(k, p)| {
                let c = if need_contamination { self.contamination.at(p) } else { 0.0 };
                survey::resource_value(layer, &s.ground[k], survey::PointResources { heat: s.heat[k], shipping: s.shipping[k], wind: s.wind[k], solar: s.solar[k], contamination: c })
            })
            .collect()
    }

    /// Every survey layer at one point: the resources, and the chance of
    /// each industry kind there (as an industrial lot centred on the point).
    pub fn survey_at(&self, p: Vec2) -> Vec<(LandLayer, f32)> {
        let g = ground::sample(&self.landscape, p);
        let heat = survey::geothermal_at(&self.landscape, p);
        let ship = survey::shipping_at(&self.landscape, p);
        let c = self.contamination.at(p);
        let (wind, solar) = (survey::wind_value(&self.landscape, p), survey::solar_value(&self.landscape, p));
        let r = survey::PointResources { heat, shipping: ship, wind, solar, contamination: c };
        let mut out: Vec<(LandLayer, f32)> = LandLayer::RESOURCES.iter().map(|l| (*l, survey::resource_value(*l, &g, r))).collect();
        let sh = if self.landscape.is_water(p) { [0.0; crate::industry::IndustryKind::COUNT] } else { survey::shares(&industry::weights(&self.site_context().site(self, p, g, c, None))) };
        out.extend(IndustryKind::ALL.iter().map(|k| (LandLayer::Industry(*k), sh[k.index()])));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::LandscapePreset;
    use crate::mind::IdleMind;

    fn valley() -> World {
        World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, 11), ..WorldConfig::test_town() })
    }

    /// A small grid with industry and homes, run until some of it stands.
    fn built(w: &mut World) {
        for i in -1..=1 {
            let c = i as f32 * 120.0;
            w.add_road(&[Vec2::new(-180.0, c), Vec2::new(180.0, c)], 1, 1);
            w.add_road(&[Vec2::new(c, -180.0), Vec2::new(c, 180.0)], 1, 1);
        }
        w.add_zone(vec![Vec2::new(-170.0, -110.0), Vec2::new(-10.0, -110.0), Vec2::new(-10.0, -10.0), Vec2::new(-170.0, -10.0)], LandUse::Industrial);
        w.add_zone(vec![Vec2::new(10.0, 10.0), Vec2::new(170.0, 10.0), Vec2::new(170.0, 110.0), Vec2::new(10.0, 110.0)], LandUse::Residential);
        let mut minds = IdleMind;
        for _ in 0..1500 {
            w.tick(&mut minds);
        }
    }

    #[test]
    fn lot_site_matches_the_context_site() {
        // The refactor kept one path: a lot's site is the context's site at
        // its centre with the lot's own averaged ground.
        let mut w = valley();
        built(&mut w);
        assert!(w.buildings.values().any(|b| b.land_use == LandUse::Industrial), "some industry to be a neighbour");
        let ctx = w.site_context();
        let mut checked = 0;
        for (id, lot) in w.lots.iter() {
            let a = w.industry_site(id).unwrap();
            let b = ctx.site(&w, lot.center, ground::sample_area(&w.landscape, lot.center, &lot.polygon), w.contamination.over(lot.center, &lot.polygon), Some(id));
            assert_eq!(a, b);
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[test]
    fn survey_layers_show_the_ground_and_the_industry_choice() {
        let w = valley();
        let s = LandSurvey::new(&w.landscape, Vec2::splat(-900.0), Vec2::splat(900.0), 40.0, 4000);
        let weights = w.survey_industry_weights(&s);
        assert_eq!(weights.len(), s.len());
        // Oil wells only ever stand over oil; the layer agrees with the ground.
        let oil = w.survey_values(&s, LandLayer::Oil, None);
        let best_oil = w.survey_values(&s, LandLayer::Industry(IndustryKind::OilExtraction), Some(&weights));
        for k in 0..s.len() {
            if s.ground[k].oil < 0.25 {
                assert_eq!(best_oil[k], 0.0, "no wells off the oil");
            }
            assert!((0.0..=1.0).contains(&oil[k]) && (0.0..=1.0).contains(&best_oil[k]));
        }
        if let Some(k) = (0..s.len()).find(|&k| s.ground[k].oil > 0.9 && !s.wet[k]) {
            assert!(best_oil[k] > 0.4, "over a rich deposit oil is the likely pick: {}", best_oil[k]);
        }
        // Shares sum to one on land.
        let sums: Vec<f32> = IndustryKind::ALL.iter().map(|kk| w.survey_values(&s, LandLayer::Industry(*kk), Some(&weights))).fold(vec![0.0; s.len()], |mut acc, v| {
            for (a, x) in acc.iter_mut().zip(v) {
                *a += x;
            }
            acc
        });
        for k in 0..s.len() {
            if !s.wet[k] {
                assert!((sums[k] - 1.0).abs() < 1e-3, "shares sum to 1: {}", sums[k]);
            }
        }
        // Farming suitability follows fertile ground: richer soil, more farms.
        let farm = w.survey_values(&s, LandLayer::Industry(IndustryKind::Farming), Some(&weights));
        let dry: Vec<usize> = (0..s.len()).filter(|&k| !s.wet[k] && s.ground[k].oil < 0.1 && s.ground[k].rock < 0.1 && s.ground[k].water < 0.05).collect();
        let rich = dry.iter().copied().max_by(|a, b| s.ground[*a].fertility.total_cmp(&s.ground[*b].fertility));
        let poor = dry.iter().copied().min_by(|a, b| s.ground[*a].fertility.total_cmp(&s.ground[*b].fertility));
        if let (Some(r), Some(p)) = (rich, poor) {
            if s.ground[r].fertility > s.ground[p].fertility + 0.3 {
                assert!(farm[r] > farm[p], "farming likelier on rich soil: {} vs {}", farm[r], farm[p]);
            }
        }
        // Fishing needs the shore.
        let fish = w.survey_values(&s, LandLayer::Industry(IndustryKind::Fishing), Some(&weights));
        for k in 0..s.len() {
            if s.ground[k].water == 0.0 {
                assert_eq!(fish[k], 0.0, "no fishing inland");
            }
        }
        // Best industry is the top share.
        let best = w.survey_values(&s, LandLayer::BestIndustry, Some(&weights));
        for k in 0..s.len() {
            assert!(best[k] + 1e-6 >= farm[k].max(fish[k]).max(best_oil[k]));
        }
        // A point query agrees with the grid at a corner.
        let k = dry.first().copied().unwrap_or(0);
        let p = s.corners().nth(k).unwrap();
        let at = w.survey_at(p);
        let f = at.iter().find(|(l, _)| *l == LandLayer::Industry(IndustryKind::Farming)).unwrap().1;
        assert!((f - farm[k]).abs() < 1e-4, "point and grid agree: {f} vs {}", farm[k]);
        let soil = at.iter().find(|(l, _)| *l == LandLayer::Farmland).unwrap().1;
        assert!((soil - s.ground[k].fertility).abs() < 1e-4);
    }

    #[test]
    fn surveying_changes_nothing() {
        let mut a = valley();
        let mut b = valley();
        let s = LandSurvey::new(&a.landscape, Vec2::splat(-400.0), Vec2::splat(400.0), 50.0, 1000);
        for l in LandLayer::ALL {
            let _ = a.survey_values(&s, l, None);
        }
        let _ = a.survey_at(Vec2::new(10.0, 20.0));
        built(&mut a);
        built(&mut b);
        assert_eq!(a.lots.len(), b.lots.len());
        assert_eq!(a.buildings.len(), b.buildings.len());
        assert_eq!(a.clock.tick, b.clock.tick);
    }
}
