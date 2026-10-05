//! Style adjacency: how a building's style relates to the styles already
//! standing around it.
//!
//! Every style belongs to a [`Family`] and has a *cluster* score for its own
//! kind. Families like each other or not (a slum likes slums and night life,
//! a glass castle does not want a container pile next door), and a style's
//! cluster score says whether it repeats happily (the Stacks, Shanghai twin
//! slabs, shophouse rows) or wants to stand apart (downtown spires, glass
//! castles, arenas). The pick multiplies each option's weight by
//! `exp(sum of affinities, nearer neighbours counting more)`.
//!
//! The renderer resolves styles with a [`StyleBook`]: a building only looks
//! at neighbours built before it (by build tick, then seed), so the result
//! does not depend on the order buildings are drawn in, and a building keeps
//! its style when a later neighbour arrives.

use super::{choose_style, BuildingRules, Extras, Style};
use glam::Vec2;
use nd_sim::buildings::BuildingId;
use nd_sim::world::World;
use nd_sim::{LandUse, Tier};
use std::collections::HashMap;

/// Neighbours further than this (lot centre to lot centre) do not count.
pub const RADIUS: f32 = 60.0;

/// A building standing near the lot being styled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbour {
    pub style: Style,
    /// Lot centre to lot centre, metres.
    pub distance: f32,
}

/// What the site offers a style: its height and street frontage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    pub floors: u8,
    pub frontage: f32,
}

impl Default for Site {
    /// A neutral site that every style suits equally.
    fn default() -> Self {
        Self { floors: 0, frontage: 0.0 }
    }
}

/// Broad kinds of building for the affinity table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Piles, walled blocks, capsules: informal and dense.
    Slum,
    /// Walk-ups, courtyards, terraces: street-scale housing.
    Walkup,
    /// Repeated mid-rise slabs.
    Slab,
    /// Signature towers that should not repeat.
    Spire,
    /// Gated luxury: villas and glass castles.
    Luxury,
    /// Shops and markets.
    Retail,
    /// Offices on podiums.
    Office,
    /// Bars, clubs, markets after dark.
    Night,
    /// Dirty industry.
    Heavy,
    /// Clean industry.
    Tech,
    /// Civic, parks, parking, plants and rail: neutral.
    Special,
}

/// Family and same-style cluster score of a style.
pub fn traits(style: Style) -> (Family, f32) {
    use Family::*;
    match style {
        Style::Stacks => (Slum, 1.4),
        Style::WalledCity => (Slum, 1.1),
        Style::CapsuleHotel => (Slum, 0.3),
        Style::Tenement => (Walkup, 0.6),
        Style::Courtyard => (Walkup, 0.2),
        Style::HillsideSteps => (Walkup, 0.7),
        Style::Brutalist => (Slab, 0.4),
        Style::BalconySlab => (Slab, 0.7),
        Style::TwinSlab => (Slab, 1.2),
        Style::PencilTower => (Slab, 0.6),
        Style::Crystal => (Spire, -1.6),
        Style::GardenTower => (Spire, -0.8),
        Style::SlabTower => (Spire, -0.8),
        Style::TwistTower => (Spire, -1.6),
        Style::CantileverMall => (Spire, -1.0),
        Style::Megastructure => (Spire, -1.8),
        Style::GlassTerraces => (Spire, -1.0),
        Style::Fortress => (Luxury, -1.4),
        Style::Villas => (Luxury, 0.5),
        Style::Shophouse => (Retail, 0.8),
        Style::MarketHall => (Retail, -0.4),
        Style::RetailBlock => (Retail, 0.2),
        Style::PodiumOffice => (Office, 0.1),
        Style::NightMarket => (Night, 0.7),
        Style::NeonTower => (Night, 0.2),
        Style::ShackBar => (Night, 0.6),
        Style::Club => (Night, 0.0),
        Style::Cinema => (Night, -0.6),
        Style::Casino => (Night, -1.0),
        Style::Arena => (Night, -2.0),
        Style::Junkyard => (Heavy, 0.8),
        Style::RustyWarehouse => (Heavy, 0.6),
        Style::Workshops => (Heavy, 0.9),
        Style::Factory => (Heavy, 0.4),
        Style::TankFarm => (Heavy, 0.5),
        Style::FlattedFactory => (Heavy, 0.5),
        Style::Datacenter => (Tech, 0.3),
        Style::Depot => (Tech, 0.3),
        Style::FabPlant => (Tech, -0.3),
        _ => (Special, 0.0),
    }
}

/// Affinity between two different families, symmetric.
fn family_affinity(a: Family, b: Family) -> f32 {
    use Family::*;
    let key = |x: Family, y: Family| (x as u8).min(y as u8) * 16 + (x as u8).max(y as u8);
    let k = key(a, b);
    let table: &[(Family, Family, f32)] = &[
        (Slum, Slum, 0.5),
        (Slum, Walkup, 0.3),
        (Slum, Night, 0.4),
        (Slum, Heavy, 0.3),
        (Slum, Luxury, -1.0),
        (Slum, Spire, -0.3),
        (Walkup, Walkup, 0.3),
        (Walkup, Slab, 0.2),
        (Walkup, Retail, 0.3),
        (Walkup, Luxury, -0.3),
        (Slab, Slab, 0.3),
        (Slab, Retail, 0.2),
        (Spire, Spire, -0.25),
        (Spire, Office, 0.3),
        (Spire, Luxury, 0.2),
        (Spire, Heavy, -0.5),
        (Luxury, Luxury, 0.2),
        (Luxury, Night, -0.4),
        (Luxury, Heavy, -0.8),
        (Retail, Retail, 0.2),
        (Retail, Night, 0.3),
        (Retail, Office, 0.2),
        (Night, Night, 0.4),
        (Heavy, Heavy, 0.4),
        (Heavy, Tech, -0.2),
        (Tech, Tech, 0.3),
    ];
    table.iter().find(|(x, y, _)| key(*x, *y) == k).map(|t| t.2).unwrap_or(0.0)
}

/// How much `candidate` wants to stand next to `other`: positive clusters,
/// negative keeps apart.
pub fn affinity(candidate: Style, other: Style) -> f32 {
    let (fa, cluster) = traits(candidate);
    if candidate == other {
        return cluster;
    }
    let (fb, _) = traits(other);
    if fa == Family::Special || fb == Family::Special {
        return 0.0;
    }
    family_affinity(fa, fb)
}

/// Weight multiplier for a style from its neighbours: `exp` of the summed
/// affinities, each scaled by how close the neighbour is, clamped so a
/// crowd can make a style rare or common but never impossible or certain.
pub fn neighbour_factor(style: Style, near: &[Neighbour]) -> f32 {
    if near.is_empty() {
        return 1.0;
    }
    // Sum in a fixed order so the result does not depend on how the
    // caller collected the list.
    let mut terms: Vec<f32> = near
        .iter()
        .filter(|n| n.distance.is_finite() && n.distance < RADIUS)
        .map(|n| affinity(style, n.style) * (1.0 - n.distance / RADIUS).clamp(0.0, 1.0).powf(0.7))
        .collect();
    terms.sort_by(f32::total_cmp);
    let score: f32 = terms.iter().sum();
    (score * 0.9).exp().clamp(0.04, 12.0)
}

/// How well a style suits the site's height and frontage. Towers need
/// floors to be towers, walk-up forms suit low sites, pencil towers want a
/// narrow frontage, sprawling compounds a wide one. A default site is
/// neutral.
pub fn site_fit(style: Style, site: &Site) -> f32 {
    if site.floors == 0 {
        return 1.0;
    }
    let f = site.floors as f32;
    let tall = |min: f32| if f >= min { 1.0 } else { (0.25 + 0.75 * f / min).min(1.0) };
    let low = |max: f32| if f <= max { 1.0 } else { (max / f).max(0.3) };
    let w = site.frontage;
    match style {
        Style::Crystal | Style::TwistTower | Style::SlabTower | Style::Megastructure => tall(10.0),
        Style::GardenTower | Style::GlassTerraces | Style::TwinSlab => tall(7.0),
        Style::PencilTower => tall(5.0) * if w > 0.0 && w < 18.0 { 1.6 } else if w > 30.0 { 0.5 } else { 1.0 },
        Style::Villas | Style::Fortress => low(8.0) * if w > 24.0 { 1.3 } else { 1.0 },
        Style::Shophouse | Style::Tenement | Style::ShackBar | Style::NightMarket => low(5.0),
        Style::HillsideSteps => if w > 0.0 && w < 14.0 { 0.5 } else { 1.0 },
        Style::WalledCity | Style::Stacks => 1.0,
        _ => 1.0,
    }
}

/// Rank of a building for adjacency: buildings only look at neighbours
/// that rank lower, i.e. were built earlier.
fn rank(b: &nd_sim::Building) -> (u64, u64) {
    (b.built_tick, b.seed)
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    key: (u64, LandUse, Tier, u8, u8),
    style: Style,
    neighbours: Vec<Neighbour>,
}

/// The styles of a city's buildings, resolved in build order so each one
/// sees the styles of the neighbours that stood when it went up. Keep one
/// per world, call [`StyleBook::refresh`] before generating, then pass
/// [`StyleBook::neighbours`] in [`Extras`]. A building's entry is kept while
/// its seed, land use, tier, floors and special role are unchanged, so it
/// does not change look when neighbours come and go.
#[derive(Default, Clone, Debug)]
pub struct StyleBook {
    entries: HashMap<BuildingId, Entry>,
}

/// What a building is besides its zone: drives forced styles.
fn role(b: &nd_sim::Building) -> u8 {
    if b.depot {
        1
    } else if b.rail.is_some() {
        2
    } else if b.plant.is_some() {
        3
    } else if b.parking_visual().is_some() {
        4
    } else {
        0
    }
}

impl StyleBook {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The resolved style of a building, if it has been resolved.
    pub fn style(&self, id: BuildingId) -> Option<Style> {
        self.entries.get(&id).map(|e| e.style)
    }

    /// The neighbours a building's style was picked against.
    pub fn neighbours(&self, id: BuildingId) -> &[Neighbour] {
        self.entries.get(&id).map(|e| e.neighbours.as_slice()).unwrap_or(&[])
    }

    /// Resolve every building that is new or changed, oldest first, and
    /// forget removed ones. Cheap when nothing changed.
    pub fn refresh(&mut self, world: &World, rules: &BuildingRules) {
        self.entries.retain(|id, _| world.buildings.contains_key(*id));
        let key_of = |b: &nd_sim::Building| (b.seed, b.land_use, b.tier, b.floors, role(b));
        let mut pending: Vec<BuildingId> = world
            .buildings
            .iter()
            .filter(|(id, b)| self.entries.get(id).is_none_or(|e| e.key != key_of(b)))
            .map(|(id, _)| id)
            .collect();
        if pending.is_empty() {
            return;
        }
        pending.sort_by_key(|id| rank(&world.buildings[*id]));
        // Grid of every building's lot centre for the radius queries.
        let cell = RADIUS;
        let mut grid: HashMap<(i32, i32), Vec<(BuildingId, Vec2)>> = HashMap::new();
        for (id, b) in world.buildings.iter() {
            if let Some(lot) = world.lots.get(b.lot) {
                let c = lot.center;
                grid.entry(((c.x / cell).floor() as i32, (c.y / cell).floor() as i32)).or_default().push((id, c));
            }
        }
        for id in pending {
            let b = &world.buildings[id];
            let Some(lot) = world.lots.get(b.lot) else { continue };
            let c = lot.center;
            let my_rank = rank(b);
            let (cx, cy) = ((c.x / cell).floor() as i32, (c.y / cell).floor() as i32);
            let mut near: Vec<Neighbour> = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for (other, oc) in grid.get(&(cx + dx, cy + dy)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if *other == id {
                            continue;
                        }
                        let d = oc.distance(c);
                        if d >= RADIUS || rank(&world.buildings[*other]) >= my_rank {
                            continue;
                        }
                        if let Some(e) = self.entries.get(other) {
                            near.push(Neighbour { style: e.style, distance: d });
                        }
                    }
                }
            }
            near.sort_by(|a, b| a.distance.total_cmp(&b.distance).then((a.style as usize).cmp(&(b.style as usize))));
            let extras = Extras {
                parking: b.parking_visual(),
                plant: b.plant,
                depot: b.depot,
                rail: b.rail.as_ref(),
                neighbours: &near,
                ..Extras::default()
            };
            let style = choose_style(lot, b.land_use, b.floors, b.tier, b.seed, rules, &extras);
            self.entries.insert(id, Entry { key: key_of(b), style, neighbours: near });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::rng_for;

    fn share(rules: &BuildingRules, lu: LandUse, tier: Tier, site: Site, near: &[Neighbour], style: Style) -> f32 {
        let mut rng = rng_for(5, 9);
        let n = 4000;
        (0..n).filter(|_| rules.pick_style_near(lu, tier, &site, near, &mut rng) == style).count() as f32 / n as f32
    }

    #[test]
    fn stacks_cluster_and_spires_keep_apart() {
        let rules = BuildingRules::default();
        let site = Site { floors: 5, frontage: 20.0 };
        let alone = share(&rules, LandUse::Residential, Tier::Low, site, &[], Style::Stacks);
        let stacks: Vec<Neighbour> = (0..4).map(|i| Neighbour { style: Style::Stacks, distance: 15.0 + 8.0 * i as f32 }).collect();
        let among = share(&rules, LandUse::Residential, Tier::Low, site, &stacks, Style::Stacks);
        assert!(among > alone * 1.4 && among > 0.5, "stacks should cluster: alone {alone}, among stacks {among}");

        let tall = Site { floors: 14, frontage: 24.0 };
        let alone = share(&rules, LandUse::Commercial, Tier::High, tall, &[], Style::TwistTower);
        let next_door = [Neighbour { style: Style::TwistTower, distance: 20.0 }];
        let beside = share(&rules, LandUse::Commercial, Tier::High, tall, &next_door, Style::TwistTower);
        assert!(beside < alone * 0.5, "a spire should not repeat its neighbour: alone {alone}, beside one {beside}");

        // Shanghai slabs line up.
        let mid = Site { floors: 9, frontage: 24.0 };
        let alone = share(&rules, LandUse::Residential, Tier::Mid, mid, &[], Style::TwinSlab);
        let row = [Neighbour { style: Style::TwinSlab, distance: 25.0 }, Neighbour { style: Style::TwinSlab, distance: 40.0 }];
        assert!(share(&rules, LandUse::Residential, Tier::Mid, mid, &row, Style::TwinSlab) > alone * 1.4);
        // Far neighbours do not count.
        let far = [Neighbour { style: Style::TwistTower, distance: RADIUS + 5.0 }];
        assert_eq!(neighbour_factor(Style::TwistTower, &far), 1.0);
    }

    #[test]
    fn height_changes_the_menu() {
        let rules = BuildingRules::default();
        let low = Site { floors: 3, frontage: 26.0 };
        let high = Site { floors: 16, frontage: 26.0 };
        let (cl, ch) = (share(&rules, LandUse::Residential, Tier::High, low, &[], Style::Crystal), share(&rules, LandUse::Residential, Tier::High, high, &[], Style::Crystal));
        assert!(ch > cl * 1.5, "towers belong on tall sites: {cl} vs {ch}");
        let (vl, vh) = (share(&rules, LandUse::Residential, Tier::High, low, &[], Style::Villas), share(&rules, LandUse::Residential, Tier::High, high, &[], Style::Villas));
        assert!(vl > vh * 1.5, "villas belong on low sites: {vl} vs {vh}");
        // A narrow frontage favours the pencil tower.
        let narrow = share(&rules, LandUse::Residential, Tier::Mid, Site { floors: 8, frontage: 12.0 }, &[], Style::PencilTower);
        let wide = share(&rules, LandUse::Residential, Tier::Mid, Site { floors: 8, frontage: 40.0 }, &[], Style::PencilTower);
        assert!(narrow > wide * 1.5, "pencil towers want narrow lots: {narrow} vs {wide}");
    }

    #[test]
    fn neighbour_order_does_not_matter() {
        let rules = BuildingRules::default();
        let mut near = vec![
            Neighbour { style: Style::Stacks, distance: 12.0 },
            Neighbour { style: Style::Tenement, distance: 33.0 },
            Neighbour { style: Style::WalledCity, distance: 21.0 },
        ];
        let site = Site { floors: 4, frontage: 15.0 };
        let a: Vec<Style> = (0..50).map(|s| rules.pick_style_near(LandUse::Residential, Tier::Low, &site, &near, &mut rng_for(s, 1))).collect();
        near.reverse();
        let b: Vec<Style> = (0..50).map(|s| rules.pick_style_near(LandUse::Residential, Tier::Low, &site, &near, &mut rng_for(s, 1))).collect();
        assert_eq!(a, b);
    }

    fn town() -> World {
        use nd_sim::world::WorldConfig;
        let mut w = World::new(WorldConfig::default());
        for i in -1..=1 {
            let c = i as f32 * 120.0;
            w.add_road(&[Vec2::new(-200.0, c), Vec2::new(200.0, c)], 1, 1);
            w.add_road(&[Vec2::new(c, -200.0), Vec2::new(c, 200.0)], 1, 1);
        }
        let sq = |x: f32, y: f32| vec![Vec2::new(x, y), Vec2::new(x + 110.0, y), Vec2::new(x + 110.0, y + 110.0), Vec2::new(x, y + 110.0)];
        w.add_zone(sq(5.0, 5.0), LandUse::Residential);
        w.add_zone(sq(-115.0, 5.0), LandUse::Residential);
        w.add_zone(sq(5.0, -115.0), LandUse::Commercial);
        let lots: Vec<_> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
        for id in lots {
            w.build_on(id);
        }
        w
    }

    /// The book resolves every building, only against older neighbours,
    /// the same way however it is filled, and keeps a building's style when
    /// a newer neighbour goes up.
    #[test]
    fn style_book_resolves_in_build_order() {
        let mut w = town();
        let rules = BuildingRules::default();
        let mut book = StyleBook::default();
        book.refresh(&w, &rules);
        assert_eq!(book.len(), w.buildings.len());
        assert!(w.buildings.len() > 10);
        for (id, b) in w.buildings.iter() {
            let n = book.neighbours(id);
            assert!(n.iter().all(|x| x.distance < RADIUS));
            // What the book recorded is what the generator draws.
            let lot = &w.lots[b.lot];
            let extras = Extras { neighbours: n, ..Extras::default() };
            assert_eq!(book.style(id), Some(choose_style(lot, b.land_use, b.floors, b.tier, b.seed, &rules, &extras)));
        }
        // The oldest building has no older neighbours.
        let oldest = w.buildings.iter().min_by_key(|(_, b)| rank(b)).map(|(id, _)| id).unwrap();
        assert!(book.neighbours(oldest).is_empty());
        // A fresh book gives the same answer.
        let mut again = StyleBook::default();
        again.refresh(&w, &rules);
        for id in w.buildings.keys() {
            assert_eq!(book.style(id), again.style(id));
        }
        // A newer neighbour does not restyle the old ones.
        let before: Vec<(BuildingId, Option<Style>)> = w.buildings.keys().map(|id| (id, book.style(id))).collect();
        w.add_zone(vec![Vec2::new(-115.0, -115.0), Vec2::new(-5.0, -115.0), Vec2::new(-5.0, -5.0), Vec2::new(-115.0, -5.0)], LandUse::Entertainment);
        let fresh: Vec<_> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
        for id in fresh {
            w.build_on(id);
        }
        book.refresh(&w, &rules);
        for (id, style) in before {
            if w.buildings.contains_key(id) {
                assert_eq!(book.style(id), style, "an older building changed style");
            }
        }
        assert_eq!(book.len(), w.buildings.len());
    }

    #[test]
    fn rules_files_without_new_styles_still_offer_them() {
        let mut rules = BuildingRules::default();
        rules.styles.retain(|s| s.style != Style::Workshops);
        let opts = rules.options(LandUse::Industrial, Tier::Low);
        assert!(opts.iter().any(|o| o.style == Style::Workshops));
        // An explicit zero switches a style off.
        let mut rules = BuildingRules::default();
        for s in &mut rules.styles {
            if s.style == Style::Workshops {
                s.weight = 0.0;
            }
        }
        assert!(!rules.options(LandUse::Industrial, Tier::Low).iter().any(|o| o.style == Style::Workshops));
    }
}
