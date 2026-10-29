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
    /// Fields, barns and greenhouses: wants clean neighbours.
    Farm,
    /// Container yards, truck lots and fish docks: port-side trade.
    Logistics,
    /// Civic, parks, parking, plants and rail: neutral.
    Special,
}

/// Family and same-style cluster score of a style: its row of the style
/// table (`buildings/style.rs`), where every style declares both.
pub fn traits(style: Style) -> (Family, f32) {
    let d = style.def();
    (d.family, d.cluster)
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
        (Farm, Farm, 0.5),
        (Farm, Heavy, -1.2),
        (Farm, Tech, -0.2),
        (Farm, Walkup, 0.1),
        (Farm, Luxury, 0.2),
        (Farm, Slum, -0.2),
        (Farm, Logistics, -0.3),
        (Logistics, Logistics, 0.4),
        (Logistics, Heavy, 0.3),
        (Logistics, Tech, 0.1),
        (Logistics, Luxury, -0.6),
        (Logistics, Spire, -0.3),
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
    (score * 1.3).exp().clamp(0.03, 12.0)
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
        // [pt-growth] (F25) Villas are one or two storeys whatever the site
        // says: never the look of a site the sim counts more floors on.
        Style::Villas => if f <= 3.0 { if w > 24.0 { 1.3 } else { 1.0 } } else { 0.0 },
        Style::Fortress => low(8.0) * if w > 24.0 { 1.3 } else { 1.0 },
        // [pt-growth] (F8) The civic styles that draw a fixed height: a
        // monument plaza, the library and the precinct are low, a
        // courthouse three to five storeys.
        Style::Monument | Style::Library | Style::Precinct => if f <= 3.0 { 1.0 } else { 0.02 },
        Style::Courthouse => low(5.0),
        Style::Shophouse | Style::Tenement | Style::ShackBar | Style::NightMarket => low(5.0),
        Style::HillsideSteps => if w > 0.0 && w < 14.0 { 0.5 } else { 1.0 },
        Style::WalledCity | Style::Stacks => if f <= 1.0 { 0.12 } else { 1.0 },
        // [land] Low density: shacks, trailers and huts are one storey; the
        // walk-ups and capsule piles they stand in for are not.
        Style::Shacks | Style::TrailerPark | Style::Huts => if f <= 1.0 { 1.0 } else { 0.0 },
        Style::Suburb => if f <= 2.0 { 1.0 } else { 0.0 },
        Style::CapsuleHotel => if f <= 1.0 { 0.12 } else { 1.0 },
        // [capsule] Domes and saucers are low; the towers want their floors.
        // (A dome house of fifteen floors would be a silo: they give way.)
        Style::DomeHouse | Style::SaucerHouse | Style::TwinDomes | Style::DomePods => if f <= 4.0 { 1.0 } else { 0.02 },
        Style::DomeHall => if f <= 6.0 { 1.0 } else { 0.08 },
        Style::PopsicleDome => tall(4.0) * low(14.0),
        Style::CapsuleTower | Style::SaucerStack => tall(6.0),
        Style::StiltTower => tall(10.0),
        Style::DomeCampus => tall(3.0),
        // [r4-brick] Rows are three or four storeys, the mill wants height.
        Style::BrickRow | Style::CornerPub => low(4.0),
        Style::MillLofts => tall(5.0),
        // [garden] Houses and shops of the suburb are low; a close wants
        // a wide plot.
        Style::GardenHouse | Style::Bungalow => if f <= 3.0 { 1.0 } else { 0.02 },
        Style::CulDeSac => (if f <= 3.0 { 1.0 } else { 0.02 }) * if w > 0.0 && w < 34.0 { 0.15 } else { 1.0 },
        Style::ShoppingParade | Style::CornerCafe | Style::GardenCentre => if f <= 4.0 { 1.0 } else { 0.05 },
        // [r4-harbour] Terraces, boathouses, shacks and sheds are low; the
        // corner tower and the hotel want a few floors.
        Style::PastelTerrace | Style::AwningArcade => low(4.0),
        Style::Boathouses | Style::SeafoodShack | Style::BoardwalkCafe | Style::NetSheds => low(3.0),
        Style::LighthouseCorner => tall(3.0) * low(6.0),
        Style::BoutiqueHotel => tall(3.0),
        // [r4-oldstone] Old houses are two to four storeys; the arcade and
        // the inn want a street front of their own.
        Style::StoneHouse | Style::OldInn => low(3.0),
        Style::GabledRow => low(5.0),
        Style::MarketArcade => low(4.0) * if w > 0.0 && w < 14.0 { 0.5 } else { 1.0 },
        // [r4-solarpunk] Terraces and townhouses are low, the shops, the
        // farms and the depot too; the vine tower and the vertical farm
        // want floors.
        Style::TimberTerrace => low(4.0),
        Style::SolarTownhouses | Style::CoopStore | Style::RepairCafe | Style::GlassMarketHall | Style::GreenhouseRows | Style::BikeDepot => low(3.0),
        Style::VineTower => tall(4.0),
        Style::VerticalFarm => tall(3.0),
        // [r4-glass] Sheds and the quad are low, the flats mid-rise, the
        // tower and the atrium office want floors.
        Style::StartupSheds => low(2.0),
        Style::CampusQuad => low(6.0),
        Style::GlassLab | Style::ResearchLab => low(8.0),
        Style::StudentFlats => tall(3.0) * low(16.0),
        Style::CoLivingTower => tall(6.0),
        Style::AtriumOffice => tall(4.0),
        Style::ServerHall => low(4.0),
        // [r4-patchwork] Stalls, kiosks and the shebeen are low; the homes
        // count their rooms whatever they look (shacks and stilt houses
        // stand for as many households as a stack), so any height suits.
        Style::TarpMarket | Style::CornerKiosk | Style::Shebeen => low(3.0),
        // [r4-skyline] The towers want floors (the twins the most, and a
        // wide plot), the boutique row is low, the mall mid-rise.
        Style::LuxeTower | Style::PoolTower => tall(5.0),
        Style::TwinTowers => tall(9.0) * if w > 0.0 && w < 20.0 { 0.2 } else { 1.0 },
        Style::HelipadTower => tall(8.0),
        Style::BoutiqueRow => low(4.0),
        Style::MallPodium => low(8.0),
        _ => 1.0,
    }
}

/// Rank of a building for adjacency: buildings only look at neighbours
/// that rank lower, i.e. were built earlier.
fn rank(b: &nd_sim::Building) -> (u64, u64) {
    (b.built_tick, b.seed)
}

/// Everything about a building that can change the style `choose_style`
/// picks for it: while it is unchanged the book keeps the entry. Each
/// input that forces a style has a field of its own, so no two roles can
/// alias (the old packed `u8` could, and a brand HQ or a campus annex was
/// left out, so the book's label could disagree with the mesh).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    seed: u64,
    land_use: LandUse,
    tier: Tier,
    floors: u8,
    role: u16,
    rooftop: bool,
    look: Option<nd_sim::districts::Look>,
    /// [megacorp] The owner's headquarters (a tower style is forced).
    hq: bool,
    annex: Option<nd_sim::corp::Annex>,
    /// [supertall] A league tower (an HQ that is one keeps its own form).
    tower: bool,
    /// [waterfront] The lot is a beach, or on the water (a hotel there).
    beach: bool,
    waterfront: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    key: Key,
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

/// What a building is besides its zone: drives forced styles. Industry
/// businesses count by kind, so a retooled plant is restyled. Every input
/// [`style_extras`] passes that can force a style has its own value here,
/// so a building that gains or loses one is styled again.
fn role(b: &nd_sim::Building) -> u16 {
    // [land] A McMansion, mansion, mega hotel or museum is its own look.
    if let Some(f) = b.plot.form {
        return 90 + f as u16;
    }
    if let Some(s) = b.service {
        return 1000 + nd_sim::ServiceKind::ALL.iter().position(|k| *k == s.kind).unwrap_or(0) as u16;
    }
    if b.metro {
        return 70;
    }
    if let Some(p) = b.port {
        // [economy] Six dock kinds now: kept clear of the taxi and hotel values.
        return 100 + p as u16;
    }
    if b.business.as_ref().is_some_and(|z| matches!(z.kind, nd_sim::BusinessKind::TaxiCompany | nd_sim::BusinessKind::Hotel)) {
        return 75 + u16::from(b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::Hotel));
    }
    // [economy] Every business with a look of its own counts by kind (the
    // new trades are not all industries), clear of the service values.
    if let Some(z) = b.business.as_ref().filter(|z| z.kind.industry().is_some() || super::industry_styles(z.kind, Tier::Mid).is_some()) {
        if let Some(i) = nd_sim::BusinessKind::ALL.iter().position(|k| *k == z.kind) {
            return 2000 + i as u16;
        }
    }
    if b.depot {
        1
    } else if b.rail.is_some() {
        2
    } else if b.plant.is_some() {
        3
    } else if b.battery.is_some() {
        5
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
        // A rooftop pad picks a pad-capable style, so it is part of the key.
        let key_of = |id: BuildingId, b: &nd_sim::Building| Key {
            seed: b.seed,
            land_use: b.land_use,
            tier: b.tier,
            floors: b.floors,
            role: role(b),
            rooftop: world.air.has_rooftop(id),
            look: b.look,
            hq: world.brand_mark(id).is_some_and(|m| m.role == nd_sim::corp::BrandRole::Headquarters),
            annex: b.annex,
            tower: b.plot.tower.is_some(),
            beach: world.lots.get(b.lot).is_some_and(|l| l.shore.beach),
            waterfront: world.lots.get(b.lot).is_some_and(|l| l.shore.is_waterfront()),
        };
        let mut pending: Vec<BuildingId> = world
            .buildings
            .iter()
            // [tenants] A tenant is drawn as its building, with no style of its own.
            .filter(|(_, b)| !b.is_tenant())
            .filter(|(id, b)| self.entries.get(id).is_none_or(|e| e.key != key_of(*id, b)))
            .map(|(id, _)| id)
            .collect();
        if pending.is_empty() {
            return;
        }
        pending.sort_by_key(|id| rank(&world.buildings[*id]));
        // Grid of every building's lot centre for the radius queries.
        let cell = RADIUS;
        let mut grid: HashMap<(i32, i32), Vec<(BuildingId, Vec2)>> = HashMap::new();
        for (id, b) in world.buildings.iter().filter(|(_, b)| !b.is_tenant()) {
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
            let extras = style_extras(world, id, &near);
            let style = choose_style(lot, b.land_use, b.floors, b.tier, b.seed, rules, &extras);
            self.entries.insert(id, Entry { key: key_of(id, b), style, neighbours: near });
        }
    }
}

/// The inputs of a building that decide its style, and nothing else:
/// plants, car parks, depots, rail, docks, services, metro entrances, the
/// business (industry looks, hotels, a taxi yard drawn as a car park), a
/// rooftop pad and the district look. The renderer builds its [`Extras`]
/// on top of these (`..style_extras(..)`), and the [`StyleBook`] and
/// [`building_style`] pick from exactly these, so what is drawn and what
/// the inspector names are one decision.
pub fn style_extras<'a>(world: &'a World, id: BuildingId, neighbours: &'a [Neighbour]) -> Extras<'a> {
    let Some(b) = world.buildings.get(id) else { return Extras { neighbours, ..Extras::default() } };
    // [taxis] A taxi company's yard is drawn as a surface lot with empty bays.
    let taxi_yard = b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::TaxiCompany);
    Extras {
        parking: b.parking_visual().or(taxi_yard.then_some(0.0)),
        plant: b.plant,
        // [plant-scale] how big the plant is (modules, not stretching)
        plant_scale: world.plant_scale(b),
        // [power] grid battery: size, share full and flow
        battery: b.battery.as_ref().map(|x| (x.kind, x.level(), x.flow_mw)),
        depot: b.depot,
        rail: b.rail.as_ref(),
        port: b.port,
        neighbours,
        rooftop_pad: world.air.has_rooftop(id),
        // [K34] a home built of real boxes; a depot whose boxes are real
        boxes_of: world.containers.homes.get(&id).filter(|h| h.done && !h.boxes.is_empty()).map(|_| (&world.containers, id, world.clock.day() as i64)),
        real_boxes: world.containers.live && world.box_depot(id).is_some(),
        // Placed services (a vertiport among them) are their own style.
        service: b.service.map(|s| s.kind),
        business: b.business.as_ref().map(|z| z.kind),
        look: b.look,
        metro: b.metro,
        form: b.plot.form,
        // [waterfront] the water off a lot on it, a beach's crowd
        shore: super::shore::shore_look(world, id),
        // [megacorp] the owner's brand; a campus annex's own look
        brand: world.brand_mark(id),
        annex: b.annex,
        tower: b.plot.tower,
        // [attractions] a pier's deck over the water
        pier: world.attractions.piers.get(&id),
        // [parks] litter and dealers in a park let go
        park_look: world.parks.of(id).map(|s| super::park::ParkLook { trash: s.trash, dealers: s.dealers }),
        // [tenants] signs over its shops
        units: &b.units,
        ..Extras::default()
    }
}

/// The style a building is drawn in: the [`StyleBook`]'s entry when the
/// renderer has resolved it (picked against its neighbours), otherwise the
/// same pick against no neighbours, which agrees for every forced style
/// (plants, depots, services, farms, docks). The one function behind the
/// inspector's and the remote API's style label.
pub fn building_style(world: &World, id: BuildingId, rules: &BuildingRules, book: Option<&StyleBook>) -> Option<Style> {
    if let Some(s) = book.and_then(|b| b.style(id)) {
        return Some(s);
    }
    let b = world.buildings.get(id)?;
    let lot = world.lots.get(b.lot)?;
    Some(choose_style(lot, b.land_use, b.floors, b.tier, b.seed, rules, &style_extras(world, id, &[])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [pt-growth] (F8, F25) Styles that draw a fixed low height are never
    /// the look of a site the sim counts many floors on.
    #[test]
    fn low_styles_are_not_picked_for_tall_sites() {
        let site = |floors: u8| Site { floors, ..Site::default() };
        for s in [Style::Villas, Style::Monument, Style::Library, Style::Precinct] {
            assert!(site_fit(s, &site(2)) >= 1.0, "{s:?} on a low site");
            assert!(site_fit(s, &site(9)) <= 0.02, "{s:?} on a nine-floor site");
        }
        assert!(site_fit(Style::Courthouse, &site(10)) < site_fit(Style::Courthouse, &site(4)));
    }
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
            // [malls] A strip mall's form decides its look.
            let extras = Extras { neighbours: n, form: b.plot.form, ..Extras::default() };
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

    /// [inspector] One decision: the book, the renderer's extras and the
    /// inspector's label agree for every building, and a farm business is
    /// always drawn (and named) as a farm, whatever its zone's menu says.
    #[test]
    fn drawn_style_and_labelled_style_are_one_decision() {
        use nd_sim::BusinessKind as B;
        let mut w = town();
        let rules = BuildingRules::default();
        let ids: Vec<BuildingId> = w.buildings.keys().collect();
        let farms: Vec<BuildingId> = ids.iter().copied().step_by(3).collect();
        for (i, &id) in farms.iter().enumerate() {
            let kind = [B::GrainFarm, B::VertFarm, B::Dairy, B::Mill][i % 4];
            w.buildings[id].business = Some(nd_sim::Business::new(kind, 4));
            // [malls] A farm is no strip mall (a mall's form decides its look).
            w.buildings[id].plot.form = None;
        }
        let taxi = ids[1];
        w.buildings[taxi].business = Some(nd_sim::Business::new(B::TaxiCompany, 4));
        let mut book = StyleBook::default();
        book.refresh(&w, &rules);
        for &id in &ids {
            let b = &w.buildings[id];
            let drawn = choose_style(&w.lots[b.lot], b.land_use, b.floors, b.tier, b.seed, &rules, &style_extras(&w, id, book.neighbours(id)));
            assert_eq!(book.style(id), Some(drawn), "the book records what is drawn");
            assert_eq!(building_style(&w, id, &rules, Some(&book)), Some(drawn), "the label names what is drawn");
        }
        for &id in &farms {
            assert_eq!(building_style(&w, id, &rules, Some(&book)), Some(Style::Farmstead));
            assert_eq!(building_style(&w, id, &rules, None), Some(Style::Farmstead), "forced styles agree without the book too");
        }
        assert!(matches!(building_style(&w, taxi, &rules, Some(&book)), Some(Style::ParkingLot | Style::CarPark)), "a taxi yard is drawn and named as a car park");
        // A building that turns into a farm is restyled on the next refresh.
        let later = ids.iter().copied().find(|id| !farms.contains(id) && *id != taxi && book.style(*id) != Some(Style::Farmstead)).unwrap();
        w.buildings[later].business = Some(nd_sim::Business::new(B::GrainFarm, 4));
        w.buildings[later].plot.form = None;
        book.refresh(&w, &rules);
        assert_eq!(book.style(later), Some(Style::Farmstead));
    }

    /// Review B3: a building that becomes a campus annex (which forces its
    /// style) is restyled, so the book's label is what is drawn; and no two
    /// style-forcing roles share a key value.
    #[test]
    fn the_book_follows_annexes_and_roles_do_not_alias() {
        let mut w = town();
        let rules = BuildingRules::default();
        let mut book = StyleBook::default();
        book.refresh(&w, &rules);
        let ids: Vec<BuildingId> = w.buildings.keys().collect();
        for (k, annex) in [nd_sim::corp::Annex::Admin, nd_sim::corp::Annex::Theatre, nd_sim::corp::Annex::Sports, nd_sim::corp::Annex::Frat].into_iter().enumerate() {
            w.buildings[ids[k]].annex = Some(annex);
        }
        book.refresh(&w, &rules);
        for &id in &ids {
            let b = &w.buildings[id];
            let drawn = choose_style(&w.lots[b.lot], b.land_use, b.floors, b.tier, b.seed, &rules, &style_extras(&w, id, book.neighbours(id)));
            assert_eq!(book.style(id), Some(drawn), "the book records what is drawn");
        }
        assert_eq!(book.style(ids[0]), Some(Style::Ministry));
        // Every forcing input gets its own role value.
        let mut seen = std::collections::HashMap::new();
        let base = w.buildings[ids[5]].clone();
        let mut probe = |what: String, b: &nd_sim::Building| {
            let r = role(b);
            if let Some(prev) = seen.insert(r, what.clone()) {
                panic!("{what} and {prev} share role {r}");
            }
        };
        probe("plain".into(), &base);
        for f in [nd_sim::estate::Form::McMansion, nd_sim::estate::Form::Mansion, nd_sim::estate::Form::MegaHotel, nd_sim::estate::Form::Museum] {
            let mut b = base.clone();
            b.plot.form = Some(f);
            probe(format!("{f:?}"), &b);
        }
        for kind in nd_sim::BusinessKind::ALL {
            let mut b = base.clone();
            b.business = Some(nd_sim::Business::new(kind, 4));
            if role(&b) != role(&base) {
                probe(format!("{kind:?}"), &b);
            }
        }
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

    #[test]
    fn industry_businesses_wear_their_own_looks_and_farms_shun_oil() {
        use nd_sim::BusinessKind as B;
        let lots = {
            let mut w = World::new(nd_sim::world::WorldConfig::default());
            w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
            w.add_zone(vec![Vec2::new(-150.0, 8.0), Vec2::new(150.0, 8.0), Vec2::new(150.0, 70.0), Vec2::new(-150.0, 70.0)], LandUse::Industrial);
            w.lots.values().filter(|l| l.usable()).cloned().collect::<Vec<_>>()
        };
        let lot = lots.iter().max_by(|a, b| a.area().total_cmp(&b.area())).unwrap();
        let rules = BuildingRules::default();
        for (kind, want) in [(B::OilWell, Style::OilField), (B::FleetLot, Style::TruckLot), (B::Fishery, Style::FishProcessing), (B::GrainFarm, Style::Farmstead)] {
            for seed in 0..10 {
                let extras = Extras { business: Some(kind), ..Extras::default() };
                assert_eq!(choose_style(lot, LandUse::Industrial, 1, Tier::Low, seed, &rules, &extras), want, "{kind:?}");
            }
        }
        // Parts fabs keep the generic industrial menu.
        let fab = Extras { business: Some(B::Fab), ..Extras::default() };
        let s = choose_style(lot, LandUse::Industrial, 2, Tier::Mid, 3, &rules, &fab);
        assert!(!matches!(s, Style::Farmstead | Style::OilField | Style::TruckLot | Style::FishProcessing | Style::ContainerYard | Style::Refinery), "{s:?}");
        // Families: farms cluster and keep away from heavy industry.
        assert!(affinity(Style::Farmstead, Style::Farmstead) > 0.0);
        assert!(affinity(Style::Farmstead, Style::OilField) < -0.5);
        assert!(affinity(Style::ContainerYard, Style::TruckLot) > 0.0);
        let near_oil = [Neighbour { style: Style::OilField, distance: 20.0 }];
        assert!(neighbour_factor(Style::Farmstead, &near_oil) < 0.5);
    }
}
