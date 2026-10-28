//! Which style a building is drawn in.

use super::*;

/// The style `generate_building` draws for these inputs: forced by the
/// extras for plants, car parks, depots and rail, otherwise picked by weight,
/// site and neighbours. The [`StyleBook`] calls this too, so the style it
/// records for a building is the one that is drawn.
#[allow(clippy::too_many_arguments)]
pub fn choose_style(lot: &Lot, land_use: LandUse, floors: u8, tier: Tier, seed: u64, rules: &BuildingRules, extras: &Extras) -> Style {
    let mut rng = rng_for(seed, 11);
    match (extras.plant, extras.parking) {
        _ if extras.service.is_some() => Style::for_service(extras.service.unwrap()),
        // [waterfront] A public beach is sand, not a lawn. [K19] Before the
        // annexes: a campus that took a beach over as a "plaza" drew a lawn
        // over the sand, its towel spots sunk under the grass.
        _ if land_use == LandUse::Park && lot.shore.beach => Style::Beach,
        // [megacorp] A campus annex has its kind's look; an HQ is a tower.
        _ if extras.annex.is_some_and(|a| Style::for_annex(a).is_some()) => Style::for_annex(extras.annex.unwrap()).unwrap(),
        _ if extras.brand.is_some_and(|m| m.role == nd_sim::corp::BrandRole::Headquarters) && land_use == LandUse::Commercial && extras.tower.is_none() => {
            if seed % 2 == 0 { Style::TwistTower } else { Style::SlabTower }
        }
        // [r4-brick] A family's big house or a billionaire's estate in
        // Foundry Row is a converted warehouse: the loft estate.
        _ if extras.form.is_some_and(|f| f.owner_occupied()) && extras.look == Some(nd_sim::districts::Look::BrickSteel) => Style::LoftEstate,
        // [lantern] In Lantern Lanes it is a courtyard shophouse compound.
        _ if extras.form.is_some_and(|f| f.owner_occupied()) && extras.look == Some(nd_sim::districts::Look::Lantern) => Style::LanternCompound,
        // [garden] A family's or a billionaire's house in a district look
        // that has its own (the story's mansion).
        _ if extras.form.is_some_and(|f| f.owner_occupied()) && extras.look.and_then(district::look_mansion).is_some() => district::look_mansion(extras.look.unwrap()).unwrap(),
        // [land] McMansions, mansions, mega hotels and museums.
        _ if Style::for_form(extras.form).is_some() => Style::for_form(extras.form).unwrap(),
        // [subway] Before the depot: the Central Station is a bus terminal too.
        _ if extras.rail.is_some_and(|r| r.kind == RailFacility::Central) => Style::CentralStation,
        _ if extras.metro => Style::MetroEntrance,
        _ if extras.depot => Style::BusDepot,
        _ if extras.port == Some(nd_sim::ports::PortKind::ShippingPort) => Style::ShippingPort,
        _ if extras.port.is_some_and(|k| k.is_ferry()) => Style::FerryTerminal,
        // [economy] Waterfront docks: each kind has its own land side.
        _ if extras.port == Some(nd_sim::ports::PortKind::MarinaSmall) => Style::MarinaSmall,
        _ if extras.port == Some(nd_sim::ports::PortKind::MarinaLarge) => Style::MarinaLarge,
        _ if extras.port == Some(nd_sim::ports::PortKind::FishingPortSmall) => Style::FishingPortSmall,
        _ if extras.port == Some(nd_sim::ports::PortKind::FishingPortLarge) => Style::FishingPortLarge,
        _ if extras.rail.is_some_and(|r| r.kind == RailFacility::Station) => Style::Station,
        _ if extras.rail.is_some_and(|r| r.kind == RailFacility::FreightYard) => Style::FreightYard,
        _ if extras.battery.is_some() => Style::PowerBattery,
        (Some(PowerPlantKind::Coal), _) => Style::PowerCoal,
        (Some(PowerPlantKind::Solar), _) => Style::PowerSolar,
        (Some(PowerPlantKind::Wind), _) => Style::PowerWind,
        (Some(PowerPlantKind::Nuclear), _) => Style::PowerNuclear,
        (Some(PowerPlantKind::Hydro), _) => Style::PowerHydro,
        (Some(PowerPlantKind::Tidal), _) => Style::PowerTidal,
        (Some(PowerPlantKind::Geothermal), _) => Style::PowerGeothermal,
        (None, Some(_)) if tier == Tier::Low || floors < 2 => Style::ParkingLot,
        (None, Some(_)) => Style::CarPark,
        // [economy] The new trades wear their own look whatever the zone or
        // the district: a car dealer is a car dealer in any architecture.
        // [r4-harbour] The front's own hotel, seafood shack and fish market
        // hall in a Harbour Pastel district (other trades keep theirs).
        (None, None) if !extras.rooftop_pad && extras.look == Some(nd_sim::districts::Look::HarbourPastel) && harbour::trade_style(extras.business).is_some() => harbour::trade_style(extras.business).unwrap(),
        (None, None) if extras.business.is_some_and(business_look_wins) => {
            let options = industry_styles(extras.business.unwrap(), tier).unwrap();
            pick_industry_style(options, extras.neighbours, &mut rng)
        }
        // [waterfront] A hotel on the water: balconies facing it, a roof pool.
        (None, None) if extras.business == Some(nd_sim::BusinessKind::Hotel) && lot.shore.is_waterfront() => Style::WaterfrontHotel,
        // [districts] A district's look, then hotels, before the land use's menu.
        // [r4-brick] (Works with a look of their own, a refinery or a fish
        // plant, keep it in a Brick & Steel or [lantern] Lantern
        // Alley district.)
        (None, None) if !extras.rooftop_pad && extras.look.is_some_and(|l| district::look_styles(l, land_use, tier).is_some()) && !(land_use == LandUse::Industrial && extras.business.is_some_and(|k| industry_styles(k, tier).is_some())) => {
            let menu = district::look_styles(extras.look.unwrap(), land_use, tier).unwrap();
            district::pick(menu, &adjacency::Site { floors, frontage: lot.width() }, extras.neighbours, &mut rng)
        }
        // [capsule] A tower with a pad in the Capsule district keeps the
        // look: its towers carry their own round deck.
        (None, None) if extras.rooftop_pad && extras.look == Some(nd_sim::districts::Look::Capsule) && district::look_styles(nd_sim::districts::Look::Capsule, land_use, tier).is_some_and(|m| m.iter().any(|(s, _)| s.pad_capable())) => {
            let menu: Vec<(Style, f32)> = district::look_styles(nd_sim::districts::Look::Capsule, land_use, tier).unwrap().iter().copied().filter(|(s, _)| s.pad_capable()).collect();
            let weights: Vec<f32> = menu.iter().map(|(s, w)| w * adjacency::neighbour_factor(*s, extras.neighbours)).collect();
            weighted_index(&weights, &mut rng).map_or(menu[0].0, |i| menu[i].0)
        }
        (None, None) if !extras.rooftop_pad && extras.business == Some(nd_sim::BusinessKind::Hotel) => {
            district::pick(district::hotel_styles(tier), &adjacency::Site { floors, frontage: lot.width() }, extras.neighbours, &mut rng)
        }
        (None, None) if extras.business.is_some_and(|k| industry_styles(k, tier).is_some()) => {
            let options = industry_styles(extras.business.unwrap(), tier).unwrap();
            pick_industry_style(options, extras.neighbours, &mut rng)
        }
        (None, None) => {
            let site = adjacency::Site { floors, frontage: lot.width() };
            let mut style = rules.pick_style_near(land_use, tier, &site, extras.neighbours, &mut rng);
            // A tower with a pad is one that can carry a helideck.
            let mut tries = 0;
            while extras.rooftop_pad && !style.pad_capable() && tries < 12 {
                style = rules.pick_style_near(land_use, tier, &site, extras.neighbours, &mut rng);
                tries += 1;
            }
            if extras.rooftop_pad && !style.pad_capable() {
                style = if land_use == LandUse::Commercial { Style::SlabTower } else { Style::Crystal };
            }
            style
        }
    }
}

/// The looks of an industry business, weighted, or `None` to draw from the
/// land use's own menu (parts fabs keep the generic industrial styles).
pub fn industry_styles(kind: nd_sim::BusinessKind, tier: Tier) -> Option<&'static [(Style, f32)]> {
    use nd_sim::BusinessKind as B;
    Some(match kind {
        B::GrainFarm | B::Dairy | B::Mill => &[(Style::Farmstead, 1.0)],
        // Every farm looks like one: a vertical farm is greenhouses and
        // barns on the ground, not a factory the inspector calls a farm.
        B::VertFarm => &[(Style::Farmstead, 1.0)],
        B::ProteinVat => &[(Style::TankFarm, 0.6), (Style::Farmstead, 0.4)],
        B::OilWell => &[(Style::OilField, 1.0)],
        B::Refinery => &[(Style::Refinery, 0.8), (Style::TankFarm, 0.2)],
        B::ContainerYard => &[(Style::ContainerYard, 0.7), (Style::Depot, 0.3)],
        B::FleetLot => &[(Style::TruckLot, 1.0)],
        // [parking] A tow company: its trucks and the pound.
        B::TowYard => &[(Style::TruckLot, 1.0)],
        // A cargo hub: container stacks, a gantry crane, lorries in bays.
        B::CargoHub => &[(Style::ContainerYard, 1.0)],
        B::Fishery | B::FishPlant => &[(Style::FishProcessing, 1.0)],
        // [economy] One look per new trade, whatever the land use.
        B::Mechanic => &[(Style::Mechanic, 1.0)],
        B::CarDealer => &[(Style::CarDealer, 1.0)],
        B::TrashCompany => &[(Style::TrashCompany, 1.0)],
        B::BikeShop => &[(Style::BikeShop, 1.0)],
        B::CowFarm => &[(Style::CowFarm, 1.0)],
        B::ChickenFarm => &[(Style::ChickenFarm, 1.0)],
        B::Slaughterhouse => &[(Style::Slaughterhouse, 1.0)],
        B::StripMine => &[(Style::StripMine, 1.0)],
        // [dirty-industry]
        B::Smelter => &[(Style::Smelter, 1.0)],
        B::Feedlot => &[(Style::Feedlot, 1.0)],
        B::ChemicalWorks => &[(Style::ChemicalWorks, 1.0)],
        // [waterfront]
        B::BeachResort => &[(Style::BeachResort, 1.0)],
        B::PierRestaurant => &[(Style::PierRestaurant, 1.0)],
        B::BeachBar => &[(Style::BeachBar, 1.0)],
        B::BoatRental => &[(Style::BoatRental, 1.0)],
        B::FishGutting => &[(Style::FishGutting, 1.0)],
        B::FishStickFactory => &[(Style::FishStickFactory, 1.0)],
        B::FishMarket => &[(Style::FishMarket, 1.0)],
        B::SeafoodRestaurant => &[(Style::SeafoodRestaurant, 1.0)],
        B::GolfRange => &[(Style::GolfRange, 1.0)],
        B::ArtStudio => &[(Style::ArtStudio, 1.0)],
        B::Gallery => &[(Style::Gallery, 1.0)],
        // A dock's operator: normally drawn by its port kind (`extras.port`);
        // this is the fallback when the port is not passed.
        B::Marina => match tier {
            Tier::High => &[(Style::MarinaLarge, 1.0)],
            _ => &[(Style::MarinaSmall, 1.0)],
        },
        B::FishingPort => match tier {
            Tier::High => &[(Style::FishingPortLarge, 1.0)],
            _ => &[(Style::FishingPortSmall, 1.0)],
        },
        B::Assembly => match tier {
            Tier::Low => &[(Style::FlattedFactory, 0.6), (Style::Workshops, 0.4)],
            _ => &[(Style::FabPlant, 0.6), (Style::Datacenter, 0.4)],
        },
        _ => return None,
    })
}

/// [economy] Businesses whose own look wins over a district's look and
/// the land use's menu: the trades the economy opens by pressure.
pub fn business_look_wins(kind: nd_sim::BusinessKind) -> bool {
    use nd_sim::BusinessKind as B;
    matches!(
        kind,
        B::Mechanic | B::CarDealer | B::TrashCompany | B::BikeShop | B::CowFarm | B::ChickenFarm | B::Slaughterhouse | B::StripMine | B::Marina | B::FishingPort | B::FishGutting | B::FishStickFactory | B::FishMarket | B::SeafoodRestaurant | B::GolfRange | B::ArtStudio | B::Gallery | B::Smelter | B::Feedlot | B::ChemicalWorks | B::BeachResort | B::PierRestaurant | B::BeachBar | B::BoatRental
    )
}

/// Weighted pick among an industry's looks, clustering by neighbours (not
/// weighed by the site: a business's look is what it is).
pub(crate) fn pick_industry_style(options: &[(Style, f32)], near: &[Neighbour], rng: &mut impl Rng) -> Style {
    let weights: Vec<f32> = options.iter().map(|(s, w)| w * adjacency::neighbour_factor(*s, near)).collect();
    weighted_index(&weights, rng).map_or(options[0].0, |i| options[i].0)
}

/// One draw from `rng` picks an index by weight: `None` (and no draw) when
/// the weights do not add up to a positive finite total. Every weighted
/// style pick (the land use menus, district menus, industry looks) goes
/// through here.
pub(crate) fn weighted_index(weights: &[f32], rng: &mut impl Rng) -> Option<usize> {
    let total: f32 = weights.iter().sum();
    if !(total > 0.0 && total.is_finite()) {
        return None;
    }
    let mut r = rng.random_range(0.0..total);
    for (i, w) in weights.iter().enumerate() {
        if r < *w {
            return Some(i);
        }
        r -= w;
    }
    Some(weights.len() - 1)
}
