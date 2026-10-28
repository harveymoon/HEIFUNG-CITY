//! [viewer] The catalogue: every style the building generator can draw,
//! each in a few seeded variants at the tiers and heights it is built at.
//!
//! A style comes from the generator's one table (`nd_procgen::buildings::
//! STYLES`), so a new style turns up here on its own; [`group`] sorts it
//! into a section for the filters, and the test below fails while a style
//! has no section. A variant is drawn with `generate_styled_extras` (the
//! style forced, the same grammar the city uses) on a made-up rectangular
//! lot sized for the kind of building.

use nd_procgen::buildings::{adjacency::Family, generate_styled_extras, BuildingRules};
use nd_procgen::{Extras, Geometry, Palette, Style};
use nd_sim::roads::{NodeId, SegmentId};
use nd_sim::{LandUse, Lot, Tier, Vec2 as SVec2};

/// Seeds per tier and height: enough to see how a style varies.
pub const SEEDS: u64 = 3;

/// One style in the catalogue.
#[derive(Clone, Debug)]
pub struct Entry {
    pub style: Style,
    /// The style's key: its name in code and in notes (`Tenement`).
    pub key: String,
    pub label: &'static str,
    /// Section for the filters ("Residential", "Attractions"...).
    pub group: &'static str,
    pub family: Family,
    /// The land use it is drawn for: its zone menu's, else its section's.
    pub land_use: LandUse,
    /// The tiers whose menus offer it (all three for styles no menu has:
    /// those are forced by a business, a form or a placed service).
    pub tiers: Vec<Tier>,
    /// On a zone's menu (picked by weight) rather than forced.
    pub on_menu: bool,
    pub variants: Vec<Variant>,
}

/// One drawing of a style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variant {
    pub tier: Tier,
    pub floors: u8,
    pub seed: u64,
}

/// The style's key as written in notes and the API.
pub fn style_key(s: Style) -> String {
    format!("{s:?}")
}

/// A style by its key or its label (any case; spaces and underscores alike).
pub fn style_of(name: &str) -> Option<Style> {
    let n = name.trim().to_ascii_lowercase().replace(['_', '-'], " ");
    let squash = |s: &str| s.to_ascii_lowercase().replace(['_', '-', ' ', '\''], "");
    Style::ALL.into_iter().find(|s| squash(&style_key(*s)) == squash(&n) || s.label().eq_ignore_ascii_case(&n) || squash(s.label()) == squash(&n))
}

/// The catalogue's section for a style. Every style has one; `"Other"`
/// is what the test refuses.
pub fn group(s: Style) -> &'static str {
    use Style::*;
    match s {
        Stacks | Tenement | Brutalist | BalconySlab | Courtyard | Crystal | Fortress | GardenTower | Villas | WalledCity | PencilTower | TwinSlab | Megastructure | GlassTerraces | CapsuleHotel | HillsideSteps => "Residential",
        Shophouse | MarketHall | RetailBlock | PodiumOffice | SlabTower | TwistTower | CantileverMall => "Commercial",
        Junkyard | RustyWarehouse | Factory | TankFarm | Datacenter | Depot | Workshops | FlattedFactory | FabPlant => "Industrial",
        ShackBar | Club | Cinema | Casino | Arena | NightMarket | NeonTower => "Entertainment",
        Precinct | Ministry | Library | Courthouse | Monument | CivicBunker | CivicTower | Panopticon => "Civic",
        ScrubLot | Garden | Plaza | Beach => "Parks and beaches",
        ParkingLot | CarPark => "Parking",
        PowerCoal | PowerSolar | PowerWind | PowerNuclear | PowerHydro | PowerTidal | PowerGeothermal | PowerBattery => "Power",
        BusDepot | Station | FreightYard | MetroEntrance | CentralStation | Vertiport => "Transit and rail",
        ShippingPort | FerryTerminal | MarinaSmall | MarinaLarge | FishingPortSmall | FishingPortLarge => "Docks",
        PoliceStation | FireStation | Hospital | School | DispatchCenter | Prison | TrashDepot | University => "City services",
        Landmark | Stadium | ArtMuseum | ScienceMuseum | HistoryMuseum | PleasurePier | GrandPark | Zoo | Observatory | Aquarium | BotanicalGarden | ConcertHall | AmusementPark | ConventionCentre | Cathedral | CasinoResort => "Attractions",
        Farmstead | OilField | Refinery | ContainerYard | FishProcessing | TruckLot | Smelter | Feedlot | ChemicalWorks | CowFarm | ChickenFarm | Slaughterhouse | StripMine | FishGutting | FishStickFactory | TrashCompany => "Industry trades",
        Mechanic | CarDealer | BikeShop | FishMarket | SeafoodRestaurant | GolfRange | ArtStudio | Gallery => "Shops and trades",
        Shacks | TrailerPark | Huts | Suburb | McMansion | Mansion | MegaHotel | Museum => "Homes and estates",
        Supertall => "League towers",
        BeachResort | WaterfrontHotel | PierRestaurant | BeachBar | BoatRental => "Waterfront",
        DomeHouse | SaucerHouse | PopsicleDome | TwinDomes | DomePods | CapsuleTower | SaucerStack | StiltTower | DomeHall | DomeCampus => "Capsule district",
        StripMall | Mall | MegaMall => "Malls",
        // [r4-brick] Foundry Row's look.
        BrickRow | WarehouseLoft | MillLofts | BrickWorkshop | CornerPub | UnionHall | SawtoothFactory | FoundryWorks | GasWorks | LoftEstate => "Brick & Steel",
        LanternShophouse | AlleyWalkups | StallRows | FoodCourt | TeaHouse | LanternMarket | AlleyWorkshops | BackKitchens | LanternCompound => "Lantern Alley",
        GardenHouse | Bungalow | CulDeSac | ShoppingParade | GardenCentre | CornerCafe | GardenManor => "Garden suburb",
        #[allow(unreachable_patterns)]
        _ => "Other",
    }
}

/// Every section, in catalogue order.
pub const GROUPS: [&str; 23] = [
    "Residential",
    "Commercial",
    "Industrial",
    "Entertainment",
    "Civic",
    "Parks and beaches",
    "Parking",
    "Power",
    "Transit and rail",
    "Docks",
    "City services",
    "Attractions",
    "Industry trades",
    "Shops and trades",
    "Homes and estates",
    "League towers",
    "Waterfront",
    "Capsule district",
    "Malls",
    "Brick & Steel",
    "Lantern Alley",
    "Garden suburb",
    "Other",
];

/// The land use a section's forced styles are drawn for.
fn group_land_use(g: &str) -> LandUse {
    match g {
        "Residential" | "Homes and estates" | "Capsule district" | "Brick & Steel" | "Lantern Alley" | "Garden suburb" => LandUse::Residential,
        "Commercial" | "Shops and trades" | "Malls" | "Parking" | "League towers" => LandUse::Commercial,
        "Entertainment" | "Waterfront" => LandUse::Entertainment,
        "Civic" | "City services" | "Attractions" | "Transit and rail" => LandUse::Civic,
        "Parks and beaches" => LandUse::Park,
        _ => LandUse::Industrial,
    }
}

/// [lantern] The land use a Lantern Alley style is built for (the look's
/// menus are not the zone menus, so the section alone cannot say).
fn lantern_land_use(s: Style) -> Option<LandUse> {
    use Style::*;
    match s {
        LanternShophouse | AlleyWalkups | LanternCompound => Some(LandUse::Residential),
        StallRows | FoodCourt => Some(LandUse::Commercial),
        TeaHouse | LanternMarket => Some(LandUse::Entertainment),
        AlleyWorkshops | BackKitchens => Some(LandUse::Industrial),
        _ => None,
    }
}

/// [garden] The land use a style off every zone menu is drawn for when its
/// section's is not its own: the Garden Suburb's shops among its homes.
fn own_land_use(s: Style) -> Option<LandUse> {
    matches!(s, Style::ShoppingParade | Style::GardenCentre | Style::CornerCafe).then_some(LandUse::Commercial)
}

/// The heights a style is shown at: low and tall where its family grows.
fn heights(s: Style, family: Family) -> &'static [u8] {
    if s == Style::Supertall {
        return &[60, 120, 200];
    }
    match family {
        Family::Slum | Family::Walkup => &[3, 6],
        Family::Slab => &[6, 14],
        Family::Spire => &[12, 30],
        Family::Luxury => &[3, 10],
        Family::Office => &[5, 16],
        Family::Retail | Family::Night => &[2, 5],
        _ => &[2],
    }
}

/// The lot a style is drawn on, width by depth in metres: big enough for
/// the big kinds (a stadium, a port, a plant), street-sized for the rest.
pub fn lot_size(s: Style, tier: Tier) -> (f32, f32) {
    use Style::*;
    match s {
        // [lantern] A compound on a family's two plots, an estate's four.
        LanternCompound => match tier {
            Tier::High => (64.0, 52.0),
            _ => (44.0, 36.0),
        },
        LanternMarket | FoodCourt => (40.0, 44.0),
        Stadium | Arena | GrandPark | Zoo | AmusementPark | BotanicalGarden | ConventionCentre | MegaMall | ShippingPort | PowerCoal | PowerNuclear | PowerSolar | PowerWind | PowerHydro | PowerTidal | PowerGeothermal | FreightYard | CentralStation | StripMine | OilField | Refinery | Farmstead | CowFarm | Feedlot | Prison | University | CasinoResort | Megastructure => (130.0, 110.0),
        Hospital | School | MarketHall | Mall | MegaHotel | Mansion | ArtMuseum | ScienceMuseum | HistoryMuseum | ConcertHall | Cathedral | Aquarium | Observatory | PleasurePier | Station | ContainerYard | TruckLot | Smelter | ChemicalWorks | ChickenFarm | Slaughterhouse | TankFarm | Depot | Junkyard | Factory | FabPlant | BusDepot | TrashDepot | TrashCompany | MarinaLarge | FishingPortLarge | FerryTerminal | GolfRange | BeachResort | Beach | TrailerPark | Suburb | Supertall | CarDealer | StripMall | Casino | Cinema | DomeCampus | Courthouse | Ministry | CivicTower | FoundryWorks | GasWorks | LoftEstate | SawtoothFactory | CulDeSac | GardenManor => (75.0, 65.0),
        _ => match tier {
            Tier::Low => (30.0, 32.0),
            Tier::Mid => (36.0, 38.0),
            Tier::High => (44.0, 46.0),
        },
    }
}

/// A rectangular lot of `w` by `d` metres centred on the origin, fronting
/// a street along its south edge (as the Build window's thumbnails).
pub fn lot(w: f32, d: f32, land_use: LandUse) -> Lot {
    let (hw, hd) = (w * 0.5, d * 0.5);
    let c = [SVec2::new(-hw, -hd), SVec2::new(hw, -hd), SVec2::new(hw, hd), SVec2::new(-hw, hd)];
    Lot {
        zone: Default::default(),
        land_use,
        polygon: c.to_vec(),
        corners: c,
        front_edge: [c[0], c[1]],
        center: SVec2::ZERO,
        front_dir: SVec2::new(0.0, -1.0),
        front_segment: SegmentId(0),
        front_t: 0.5,
        access_node: NodeId(0),
        building: None,
        landlocked: false,
        footfall_today: 0,
        footfall_hour: 0,
        car_segment: Some(SegmentId(0)),
        car_point: SVec2::new(0.0, -hd - 3.0),
        park: None,
        shore: Default::default(),
    }
}

/// The whole catalogue, in style order, with the menus of `rules`.
pub fn entries(rules: &BuildingRules) -> Vec<Entry> {
    Style::ALL
        .into_iter()
        .map(|style| {
            let def = style.def();
            let g = group(style);
            let menus: Vec<_> = rules.styles.iter().filter(|w| w.style == style && w.weight > 0.0).collect();
            let land_use = menus.first().map_or_else(|| lantern_land_use(style).or(own_land_use(style)).unwrap_or(group_land_use(g)), |w| w.land_use);
            let mut tiers: Vec<Tier> = Tier::ALL.into_iter().filter(|t| menus.iter().any(|w| w.tier == *t)).collect();
            let on_menu = !tiers.is_empty();
            if !on_menu {
                tiers = vec![Tier::Low, Tier::Mid, Tier::High];
            }
            let mut variants = Vec::new();
            for &tier in &tiers {
                for &floors in heights(style, def.family) {
                    for k in 0..SEEDS {
                        // A seed of its own per style, tier, height and k.
                        let seed = 0x5EED_0000_0000 ^ ((style.index() as u64) << 20) ^ ((tier as u64) << 16) ^ ((floors as u64) << 8) ^ k;
                        variants.push(Variant { tier, floors, seed });
                    }
                }
            }
            Entry { style, key: style_key(style), label: def.label, group: g, family: def.family, land_use, tiers, on_menu, variants }
        })
        .collect()
}

/// Draw one variant of a style: the city's grammar with the style forced,
/// window lights marked for the viewer to switch.
pub fn geometry(e: &Entry, v: Variant, rules: &BuildingRules, palette: &Palette) -> Geometry {
    let (w, d) = lot_size(e.style, v.tier);
    let lot = lot(w, d, e.land_use);
    let tower = (e.style == Style::Supertall).then(|| nd_sim::towers::Tower {
        height: (v.floors as f32 * 4.0).clamp(120.0, 900.0),
        design: nd_sim::towers::TowerDesign::ALL[(v.seed % 4) as usize],
        seed: v.seed,
        started_day: 0,
        topped_out_day: Some(1),
        record_bid: false,
    });
    // [lantern] A look's own styles in its colours. [garden] The Garden
    // Suburb's too, its manor at the top tier as a billionaire's (the grand one).
    let look = if Style::LANTERN.contains(&e.style) {
        Some(nd_sim::districts::Look::Lantern)
    } else {
        Style::GARDEN.contains(&e.style).then_some(nd_sim::districts::Look::Garden)
    };
    let form = (e.style == Style::GardenManor && v.tier == Tier::High).then_some(nd_sim::estate::Form::Mansion);
    let extras = Extras { powered: 1.0, dynamic_lights: true, tower, look, form, ..Extras::default() };
    generate_styled_extras(&lot, e.land_use, v.floors, v.tier, 0.9, v.seed, e.style, rules, palette, &extras)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [viewer] The catalogue lists every style the generator has, each in
    /// a named section with variants, and finds it again by key or label.
    #[test]
    fn catalogue_lists_every_style() {
        let rules = BuildingRules::default();
        let all = entries(&rules);
        assert_eq!(all.len(), Style::ALL.len());
        for (e, s) in all.iter().zip(Style::ALL) {
            assert_eq!(e.style, s);
            assert_ne!(e.group, "Other", "{s:?} has no catalogue section: add it to viewer::catalogue::group");
            assert!(GROUPS.contains(&e.group));
            assert!(!e.variants.is_empty() && !e.tiers.is_empty(), "{s:?}");
            assert_eq!(style_of(&e.key), Some(s), "{s:?} by key");
            assert_eq!(style_of(e.label), Some(s), "{s:?} by label {:?}", e.label);
        }
        // The zone menus' styles say so; services and forms do not.
        assert!(all[Style::Tenement.index()].on_menu);
        assert!(!all[Style::Stadium.index()].on_menu);
        assert_eq!(style_of("walk-up tenement"), Some(Style::Tenement));
        assert_eq!(style_of("pencil_tower"), Some(Style::PencilTower));
    }

    /// Every style's first and last variant draws something.
    #[test]
    fn every_catalogue_style_draws() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        for e in entries(&rules) {
            for v in [e.variants[0], *e.variants.last().unwrap()] {
                let g = geometry(&e, v, &rules, &pal);
                assert!(g.triangle_count() > 0, "{} {v:?} drew nothing", e.key);
            }
        }
    }
}
