//! Building grammar. Takes a parcel, land use, floors, tier, condition and a
//! seed and returns geometry bucketed by surface.
//!
//! Every (land use, tier) pair has its own family of styles, picked by weight
//! from the rules file, so an industrial slum reads as junkyards and rusted
//! sheds while a high-value industrial block gets datacentres and container
//! depots. Footprints follow the parcel polygon: a triangular corner lot gets
//! a triangular building.

pub mod adjacency;
pub mod air;
/// [megacorp] Brands on buildings and the university's hall.
pub mod brand;
mod civic;
mod commercial;
mod construction;
mod dense;
/// [districts] District looks: style menus and palettes, hotels.
pub mod district;
mod entertainment;
/// [land] Shacks, trailer parks, huts, suburbs, McMansions, mansions, mega
/// hotels and museums.
mod estates;
mod industrial;
pub mod kit;
mod park;
mod parking;
pub mod parts;
mod power;
mod rail;
pub mod waterworks;
pub mod transit;
mod residential;
/// [supertall] Skyscrapers, supertalls and megatalls.
mod supertall;
mod towers;
mod works;
/// [subway] Metro entrances and the Central Station.
pub mod subway;

pub use adjacency::{building_style, style_extras, Neighbour, StyleBook};
mod services;
/// [society] The trash depot.
mod sanitation;
/// [economy] Shops, garages and places to go out opened by the economy.
mod trades;
/// [economy] Farms, slaughterhouse, strip mine, trash company, fish trades.
mod rural;
/// [economy] Land side of marinas and fishing ports.
mod docks;
/// [waterfront] Beach resorts, waterfront hotels, pier restaurants, beach bars,
/// boat rentals, public beaches and private jetties.
pub mod shore;
pub use shore::{BeachLook, ShoreLook};
// [dirty-industry] smelters, factory farms, chemical works, stained ground
pub mod dirty;
#[cfg(test)]
mod dirty_tests;
#[cfg(test)]
mod economy_tests;
pub use parts::WINDOW_LIGHT_MARK;
pub use construction::{crane_paint, crane_slew, CraneRig, CRANE_LIFT_MINUTES, CRANE_SLEW_RANGE};
pub use power::{battery_lights, coal_vents, CoalVent, solar_head, solar_trackers, wind_rotor, wind_turbines, PlantField, SolarTracker, Turbine, SOLAR_HUB, SOLAR_MAX_TILT, TURBINE_MAST, TURBINE_RADIUS, TURBINE_ROTOR_H};
pub use parking::surface_lot_bays_world;

use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use nd_sim::zoning::{point_in_polygon, polygon_area};
use nd_sim::park::ParkLayout;
use nd_sim::rail::{RailFacility, RailLink};
use nd_sim::power::PowerPlantKind;
use nd_sim::{LandUse, Lot, Tier};
use rand::Rng;
use serde::{Deserialize, Serialize};

mod choose;
mod context;
mod frame;
mod rules;
mod style;
pub use choose::*;
pub use context::*;
pub use frame::*;
pub use rules::*;
pub use style::*;

#[allow(clippy::too_many_arguments)]
pub fn generate_building(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    tier: Tier,
    condition: f32,
    seed: u64,
    rules: &BuildingRules,
    palette: &Palette,
    extras: &Extras,
) -> Geometry {
    let style = choose_style(lot, land_use, floors, tier, seed, rules, extras);
    generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, extras)
}

/// Generate a specific style; used by the tests and the style browser.
#[allow(clippy::too_many_arguments)]
pub fn generate_styled(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    tier: Tier,
    condition: f32,
    seed: u64,
    style: Style,
    rules: &BuildingRules,
    palette: &Palette,
) -> Geometry {
    generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, &Extras::default())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_styled_extras(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    tier: Tier,
    condition: f32,
    seed: u64,
    style: Style,
    rules: &BuildingRules,
    palette: &Palette,
    extras: &Extras,
) -> Geometry {
    // A truck lot shows its fleet the way a car park shows its cars.
    let parking = extras.parking.or((style == Style::TruckLot).then_some(extras.fleet));
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 13);
    let frame = Frame::from_lot(lot);
    let r = rules.for_land_use(land_use);
    let mut lot_poly: Vec<Vec2> = parts::clean_poly(&lot.polygon.iter().map(|p| frame.to_local(*p)).collect::<Vec<_>>(), 0.3);
    if lot_poly.len() < 3 {
        let (hw, hd) = (frame.width * 0.5, frame.depth * 0.5);
        lot_poly = vec![Vec2::new(-hw, -hd), Vec2::new(hw, -hd), Vec2::new(hw, hd), Vec2::new(-hw, hd)];
    }
    if polygon_area(&lot_poly) < 0.0 {
        lot_poly.reverse();
    }
    let poly = parts::offset_poly(&lot_poly, r.inset);
    // [districts] A look re-tints the walls, windows and signs.
    let tinted = extras.look.filter(|_| extras.service.is_none() && extras.plant.is_none()).map(|l| district::apply(l, rules.style(tier), r, palette));
    let (r, tier_style, palette) = match &tinted {
        Some((ts, lr, p)) => (lr, ts, p),
        None => (r, rules.style(tier), palette),
    };
    let ctx = Ctx {
        fw: (frame.width - 2.0 * r.inset).max(4.0),
        fd: (frame.depth - 2.0 * r.inset).max(4.0),
        frame,
        rules: r,
        style: tier_style,
        palette,
        land_use,
        tier,
        floors: floors.max(1),
        wear: (1.0 - condition).clamp(0.0, 1.0),
        lot_poly,
        poly,
        seed,
        parking,
        powered: extras.powered,
        rail: extras.rail.cloned().or_else(|| style.rail_kind().map(|k| back_edge_link(lot, k))),
        park: if land_use == LandUse::Park || matches!(style, Style::ScrubLot | Style::Garden | Style::Plaza) { Some(extras.park.cloned().unwrap_or_else(|| nd_sim::park::plan_single(lot, tier))) } else { None },
        animated_rotors: extras.animated_rotors,
        field: extras.field,
        animated_cranes: extras.animated_cranes,
        dynamic_lights: extras.dynamic_lights,
        battery: extras.battery,
        plant_scale: extras.plant_scale,
        shore: extras.shore,
    };
    if let Some(progress) = extras.demolition {
        let mut g = generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, &Extras { demolition: None, construction: None, ..*extras });
        construction::demolition_site(&mut g, &ctx, &mut rng, progress);
        return g;
    }
    if let Some(progress) = extras.construction {
        // [supertall] A league tower's site: the core leading, a jump crane.
        if let Some(t) = extras.tower.filter(|_| style == Style::Supertall) {
            supertall::site(&mut g, &ctx, &t, progress);
            return g;
        }
        construction::construction_site(&mut g, &ctx, &mut rng, progress);
        if weathered(tier, ctx.wear) {
            g.weather();
        }
        return g;
    }
    match style {
        Style::Stacks => residential::stacks(&mut g, &ctx, &mut rng),
        Style::Tenement => residential::tenement(&mut g, &ctx, &mut rng),
        Style::Brutalist => residential::brutalist(&mut g, &ctx, &mut rng),
        Style::BalconySlab => residential::balcony_slab(&mut g, &ctx, &mut rng),
        Style::Courtyard => residential::courtyard(&mut g, &ctx, &mut rng),
        Style::Crystal => residential::crystal(&mut g, &ctx, &mut rng),
        Style::Fortress => towers::glass_castle(&mut g, &ctx, &mut rng),
        Style::GardenTower => residential::garden_tower(&mut g, &ctx, &mut rng),
        Style::Villas => residential::villas(&mut g, &ctx, &mut rng),
        Style::Shophouse => commercial::shophouse(&mut g, &ctx, &mut rng),
        Style::MarketHall => commercial::market_hall(&mut g, &ctx, &mut rng),
        Style::RetailBlock => commercial::retail_block(&mut g, &ctx, &mut rng),
        Style::PodiumOffice => commercial::podium_office(&mut g, &ctx, &mut rng),
        Style::SlabTower => commercial::slab_tower(&mut g, &ctx, &mut rng),
        Style::TwistTower => commercial::twist_tower(&mut g, &ctx, &mut rng),
        Style::CantileverMall => commercial::cantilever_mall(&mut g, &ctx, &mut rng),
        Style::Junkyard => industrial::junkyard(&mut g, &ctx, &mut rng),
        Style::RustyWarehouse => industrial::rusty_warehouse(&mut g, &ctx, &mut rng),
        Style::Factory => industrial::factory(&mut g, &ctx, &mut rng),
        Style::TankFarm => industrial::tank_farm(&mut g, &ctx, &mut rng),
        Style::Datacenter => industrial::datacenter(&mut g, &ctx, &mut rng),
        Style::Depot => industrial::depot(&mut g, &ctx, &mut rng),
        Style::ShackBar => entertainment::shack_bar(&mut g, &ctx, &mut rng),
        Style::Club => entertainment::club(&mut g, &ctx, &mut rng),
        Style::Cinema => entertainment::cinema(&mut g, &ctx, &mut rng),
        Style::Casino => entertainment::casino(&mut g, &ctx, &mut rng),
        Style::Arena => entertainment::arena(&mut g, &ctx, &mut rng),
        Style::Precinct => civic::precinct(&mut g, &ctx, &mut rng),
        Style::Ministry => civic::ministry(&mut g, &ctx, &mut rng),
        Style::Library => civic::library(&mut g, &ctx, &mut rng),
        Style::Courthouse => civic::courthouse(&mut g, &ctx, &mut rng),
        Style::Monument => civic::monument(&mut g, &ctx, &mut rng),
        Style::ScrubLot => park::scrub_lot(&mut g, &ctx, &mut rng),
        Style::Garden => park::garden(&mut g, &ctx, &mut rng),
        Style::Plaza => park::plaza(&mut g, &ctx, &mut rng),
        Style::ParkingLot => parking::parking_lot(&mut g, &ctx, &mut rng),
        Style::CarPark => parking::car_park(&mut g, &ctx, &mut rng),
        Style::PowerCoal => power::coal(&mut g, &ctx, &mut rng),
        Style::PowerSolar => power::solar(&mut g, &ctx, &mut rng),
        Style::PowerWind => power::wind(&mut g, &ctx, &mut rng),
        Style::PowerNuclear => power::nuclear(&mut g, &ctx, &mut rng),
        Style::PowerBattery => power::battery(&mut g, &ctx, &mut rng),
        Style::BusDepot => transit::bus_depot(&mut g, &ctx, &mut rng),
        Style::Station => rail::station(&mut g, &ctx, &mut rng),
        Style::FreightYard => rail::freight_yard(&mut g, &ctx, &mut rng),
        Style::MetroEntrance => subway::metro_entrance(&mut g, &ctx, &mut rng),
        Style::CentralStation => subway::central_station(&mut g, &ctx, &mut rng),
        Style::WalledCity => dense::walled_city(&mut g, &ctx, &mut rng),
        Style::PencilTower => towers::pencil_tower(&mut g, &ctx, &mut rng),
        Style::TwinSlab => towers::twin_slab(&mut g, &ctx, &mut rng),
        Style::Megastructure => towers::megastructure(&mut g, &ctx, &mut rng),
        Style::GlassTerraces => towers::glass_terraces(&mut g, &ctx, &mut rng),
        Style::CapsuleHotel => dense::capsule_hotel(&mut g, &ctx, &mut rng),
        Style::HillsideSteps => dense::hillside_steps(&mut g, &ctx, &mut rng),
        Style::NightMarket => dense::night_market(&mut g, &ctx, &mut rng),
        Style::NeonTower => dense::neon_tower(&mut g, &ctx, &mut rng),
        Style::Workshops => works::workshops(&mut g, &ctx, &mut rng),
        Style::FlattedFactory => works::flatted_factory(&mut g, &ctx, &mut rng),
        Style::FabPlant => works::fab_plant(&mut g, &ctx, &mut rng),
        Style::PowerHydro => waterworks::hydro(&mut g, &ctx, &mut rng),
        Style::PowerTidal => waterworks::tidal(&mut g, &ctx, &mut rng),
        Style::PowerGeothermal => waterworks::geothermal(&mut g, &ctx, &mut rng),
        Style::ShippingPort => waterworks::shipping_port(&mut g, &ctx, &mut rng),
        Style::FerryTerminal => waterworks::ferry_terminal(&mut g, &ctx, &mut rng),
        Style::PoliceStation => services::police_station(&mut g, &ctx, &mut rng),
        Style::FireStation => services::fire_station(&mut g, &ctx, &mut rng),
        Style::Hospital => services::hospital(&mut g, &ctx, &mut rng),
        Style::School => services::school(&mut g, &ctx, &mut rng),
        Style::DispatchCenter => services::dispatch_center(&mut g, &ctx, &mut rng),
        Style::Landmark => services::landmark(&mut g, &ctx, &mut rng),
        Style::Stadium => services::stadium(&mut g, &ctx, &mut rng),
        Style::Prison => services::prison(&mut g, &ctx, &mut rng),
        Style::University => brand::university(&mut g, &ctx, &mut rng),
        Style::Farmstead => industrial::farmstead(&mut g, &ctx, &mut rng),
        Style::OilField => industrial::oil_field(&mut g, &ctx, &mut rng),
        Style::Refinery => industrial::refinery(&mut g, &ctx, &mut rng),
        Style::ContainerYard => industrial::container_yard(&mut g, &ctx, &mut rng),
        Style::FishProcessing => industrial::fish_processing(&mut g, &ctx, &mut rng),
        Style::TruckLot => industrial::truck_lot(&mut g, &ctx, &mut rng),
        Style::TrashDepot => sanitation::trash_depot(&mut g, &ctx, &mut rng),
        Style::Vertiport => air::vertiport(&mut g, &ctx, &mut rng),
        Style::Mechanic => trades::mechanic(&mut g, &ctx, &mut rng),
        Style::CarDealer => trades::car_dealer(&mut g, &ctx, &mut rng),
        Style::TrashCompany => rural::trash_company(&mut g, &ctx, &mut rng),
        Style::BikeShop => trades::bike_shop(&mut g, &ctx, &mut rng),
        Style::CowFarm => rural::cow_farm(&mut g, &ctx, &mut rng),
        Style::ChickenFarm => rural::chicken_farm(&mut g, &ctx, &mut rng),
        Style::Slaughterhouse => rural::slaughterhouse(&mut g, &ctx, &mut rng),
        Style::StripMine => rural::strip_mine(&mut g, &ctx, &mut rng),
        Style::FishGutting => rural::fish_gutting(&mut g, &ctx, &mut rng),
        Style::FishStickFactory => rural::fish_stick_factory(&mut g, &ctx, &mut rng),
        Style::FishMarket => trades::fish_market(&mut g, &ctx, &mut rng),
        Style::SeafoodRestaurant => trades::seafood_restaurant(&mut g, &ctx, &mut rng),
        Style::GolfRange => trades::golf_range(&mut g, &ctx, &mut rng),
        Style::ArtStudio => trades::art_studio(&mut g, &ctx, &mut rng),
        Style::Gallery => trades::gallery(&mut g, &ctx, &mut rng),
        Style::MarinaSmall => docks::marina_small(&mut g, &ctx, &mut rng),
        Style::MarinaLarge => docks::marina_large(&mut g, &ctx, &mut rng),
        Style::FishingPortSmall => docks::fishing_port_small(&mut g, &ctx, &mut rng),
        Style::FishingPortLarge => docks::fishing_port_large(&mut g, &ctx, &mut rng),
        Style::Shacks => estates::shacks(&mut g, &ctx, &mut rng),
        Style::TrailerPark => estates::trailer_park(&mut g, &ctx, &mut rng),
        Style::Huts => estates::huts(&mut g, &ctx, &mut rng),
        Style::Suburb => estates::suburb(&mut g, &ctx, &mut rng),
        Style::McMansion => estates::mcmansion(&mut g, &ctx, &mut rng),
        Style::Mansion => estates::mansion(&mut g, &ctx, &mut rng),
        Style::MegaHotel => estates::mega_hotel(&mut g, &ctx, &mut rng),
        Style::Museum => estates::museum(&mut g, &ctx, &mut rng),
        Style::Smelter => dirty::smelter(&mut g, &ctx, &mut rng),
        Style::Feedlot => dirty::feedlot(&mut g, &ctx, &mut rng),
        Style::ChemicalWorks => dirty::chemical_works(&mut g, &ctx, &mut rng),
        Style::Supertall => match &extras.tower {
            Some(t) => supertall::tower(&mut g, &ctx, t),
            // A tower without its record (a preview): a plain glass slab.
            None => commercial::slab_tower(&mut g, &ctx, &mut rng),
        },
        // [waterfront]
        Style::Beach => shore::beach(&mut g, &ctx, &mut rng, ctx.shore),
        Style::BeachResort => shore::beach_resort(&mut g, &ctx, &mut rng, ctx.shore),
        Style::WaterfrontHotel => shore::waterfront_hotel(&mut g, &ctx, &mut rng, ctx.shore),
        Style::PierRestaurant => shore::pier_restaurant(&mut g, &ctx, &mut rng, ctx.shore),
        Style::BeachBar => shore::beach_bar(&mut g, &ctx, &mut rng, ctx.shore),
        Style::BoatRental => shore::boat_rental(&mut g, &ctx, &mut rng, ctx.shore),
    }
    if extras.rooftop_pad && style.pad_capable() && g.pads.is_empty() {
        air::rooftop_deck(&mut g, &ctx);
    }
    // [megacorp] The owner's brand: rooftop signs, banners, the HQ crown.
    if let Some(m) = &extras.brand {
        brand::decorate(&mut g, &ctx, m);
    }
    if weathered(tier, ctx.wear) {
        g.weather();
    }
    g
}

/// Whether a building is drawn with the weathered materials: everything
/// cheap (low land value builds low tier), and anything left to decay.
pub fn weathered(tier: Tier, wear: f32) -> bool {
    match tier {
        Tier::Low => true,
        Tier::Mid => wear > 0.45,
        Tier::High => wear > 0.7,
    }
}

/// A stand-in track along the back of a lot, for previews and tests of the
/// station and yard styles when no railway is near.
fn back_edge_link(lot: &Lot, kind: RailFacility) -> RailLink {
    let a = lot.corners[3];
    let b = lot.corners[2];
    let d = (b - a).normalize_or_zero();
    let back = (a + b) * 0.5 + lot.front_dir * -4.0;
    let half = (a.distance(b) * 0.45).min(40.0);
    // `perp(along) * side` is the direction from the track to the building:
    // here, from behind the lot towards the road.
    let side = if Vec2::new(-d.y, d.x).dot(lot.front_dir) >= 0.0 { 1.0 } else { -1.0 };
    RailLink { kind, segment: nd_sim::SegmentId(0), platform: [back - d * half, back + d * half], side, rail_connected: true, road_connected: true, walk_connected: true }
}

#[cfg(test)]
mod coplanar_tests;

#[cfg(test)]
pub(crate) mod sweep_tests;

#[cfg(test)]
mod tests;
