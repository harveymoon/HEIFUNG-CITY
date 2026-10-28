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
// [tenants] a sign over each shop of a building let in parts
mod shopfronts;
// [malls] Strip malls, malls and mega-malls.
mod malls;
mod civic;
mod commercial;
mod construction;
mod dense;
/// [r4-style] The Hong Kong detail pass (docs/STYLE_REFERENCE.md).
pub mod hk;
/// [capsule] The Capsule district's white domes, saucers and pill towers.
mod capsule;
/// [r4-brick] Foundry Row's Brick & Steel: rows, lofts, pub, hall, estate.
mod brick;
/// [r4-brick] Brick & Steel's works: saw-tooth sheds, foundry, gas works.
mod brick_works;
/// [lantern] Lantern Alley: shophouses, alleys, stalls, halls, workshops.
mod lantern;
/// [garden] The Garden Suburb's houses, shops and garden manor.
mod garden;
/// [r4-harbour] Harbour Pastel: terraces, boathouses, cafés, sheds, villa.
mod harbour;
/// [r4-oldstone] Old Town Charter's Old Stone: houses, rows, arcades, halls, manor.
mod oldstone;
/// [r4-oldstone] Old Stone's works: the mill, the tannery, cobbled yards.
mod oldstone_works;
/// [r4-solarpunk] Solarpunk Green: terraces, towers, halls, farms, villa.
mod solarpunk;
/// [r4-patchwork] Patchwork: shacks, stilt houses, stalls, scrap, compound.
mod patchwork;
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
pub mod rail;
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
/// [attractions] The landmark attractions' looks.
mod attractions;
/// [pt-visual] Player-placed monuments: obelisks, statues, arches, memorials, halos.
mod monuments;
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
#[cfg(test)]
mod pt_visual_tests;
pub use parts::{NEON_MARK, NIGHT_MARK, WINDOW_LIGHT_MARK};
pub use construction::{crane_paint, crane_slew, CraneRig, CRANE_LIFT_MINUTES, CRANE_SLEW_RANGE};
pub use services::GameRig;
pub use power::{battery_lights, coal_vents, CoalVent, solar_head, solar_trackers, wind_rotor, wind_turbines, PlantField, SolarTracker, Turbine, SOLAR_HUB, SOLAR_MAX_TILT, TURBINE_MAST, TURBINE_RADIUS, TURBINE_ROTOR_H};
pub use parking::surface_lot_bays_world;
pub use rural::strip_mine_hole;

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

/// A specific style with its extras; [viewer] the building viewer's
/// catalogue draws every style this way.
#[allow(clippy::too_many_arguments)]
pub fn generate_styled_extras(
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
    let mut rng = rng_for(seed, 13);
    let r = rules.for_land_use(land_use);
    // [districts] A look re-tints the walls, windows and signs.
    let tinted = extras.look.filter(|_| extras.service.is_none() && extras.plant.is_none()).map(|l| district::apply(l, rules.style(tier), r, palette));
    let finish = match &tinted {
        Some((ts, lr, p)) => Finish { rules: lr, style: ts, palette: p },
        None => Finish { rules: r, style: rules.style(tier), palette },
    };
    let ctx = Ctx::new(lot, style, land_use, floors, tier, condition, seed, finish, extras);
    if let Some(progress) = extras.demolition {
        // What stood there, cut down to what is left, in its site.
        let mut g = generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, &Extras { demolition: None, construction: None, ..*extras });
        // [r4-style] Cut down as one: no AC unit left hanging in the air.
        g.flatten_detail();
        // [fix/buildings-ui] Stripped out first: the power is off.
        construction::lights_out(&mut g);
        construction::demolition_site(&mut g, &ctx, &mut rng, progress);
        return g;
    }
    if let Some(progress) = extras.construction {
        return building_site(&ctx, style, &mut rng, extras, progress);
    }
    finished(&ctx, style, &mut rng, extras)
}

/// A building still going up: its site at `progress`.
fn building_site(ctx: &Ctx, style: Style, rng: &mut nd_sim::SimRng, extras: &Extras, progress: f32) -> Geometry {
    let mut g = Geometry::default();
    // [supertall] A league tower's site: the core leading, a jump crane.
    if let Some(t) = extras.tower.filter(|_| style == Style::Supertall) {
        supertall::site(&mut g, ctx, &t, progress);
        return g;
    }
    if Style::CAPSULE.contains(&style) {
        // [K39] The domes, pills and saucers go up in their own shape: the
        // finished building (from the same stream, so the same building)
        // cut at the height the work has reached.
        let shell = finished(ctx, style, &mut rng.clone(), extras);
        construction::shaped_site(&mut g, ctx, rng, progress, shell);
        return g;
    }
    construction::construction_site(&mut g, ctx, rng, progress);
    if weathered(ctx.tier, ctx.wear) {
        g.weather();
    }
    g
}

/// A finished building: its style, a rooftop deck, the owner's brand, and
/// the weathered materials if it is cheap or decaying.
fn finished(ctx: &Ctx, style: Style, rng: &mut nd_sim::SimRng, extras: &Extras) -> Geometry {
    let mut g = Geometry::default();
    (style.def().draw)(&mut g, ctx, rng, extras);
    // [w1-render] A landmark's neon follows the day like the signs.
    if style.is_attraction() {
        parts::mark_glow(&mut g, 0, NEON_MARK);
    }
    if extras.rooftop_pad && style.pad_capable() && g.pads.is_empty() {
        air::rooftop_deck(&mut g, ctx);
    }
    // [megacorp] The owner's brand: rooftop signs, banners, the HQ crown.
    // [K19] Not on a public beach: a campus that annexed one as its
    // "plaza" laid a lawn quad over the sand, the towels under it.
    if let Some(m) = extras.brand.as_ref().filter(|_| style != Style::Beach) {
        brand::decorate(&mut g, ctx, m);
    }
    // [tenants] A sign over each shop on the ground floor ([malls] a mall
    // style hangs its own).
    if !extras.units.is_empty() && !style.is_mall() {
        shopfronts::draw(&mut g, ctx, extras.units, ctx.seed);
    }
    // [capsule] The dome district is spotless whatever its tier or age.
    if weathered(ctx.tier, ctx.wear) && !Style::CAPSULE.contains(&style) {
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
mod capsule_tests;
#[cfg(test)]
mod garden_tests;

#[cfg(test)]
mod brick_tests;

#[cfg(test)]
mod lantern_tests;

#[cfg(test)]
mod harbour_tests;

#[cfg(test)]
mod oldstone_tests;

#[cfg(test)]
mod solarpunk_tests;

#[cfg(test)]
mod patchwork_tests;

#[cfg(test)]
mod hk_tests;

#[cfg(test)]
mod coplanar_tests;

#[cfg(test)]
pub(crate) mod sweep_tests;

#[cfg(test)]
mod tests;
