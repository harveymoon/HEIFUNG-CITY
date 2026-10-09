//! [perfect-city] The "Perfect AI City" preset: a planned city laid out on
//! a temperate coast and furnished on day one with everything a city
//! needs, so the player starts at population zero and watches it grow.
//!
//! The street plan comes from the same tensor-field tracer as the other
//! presets, with a strong square grid (avenues about 300 m apart), one
//! downtown by the sea and a midtown inland. The plan is deliberate about
//! where things go: the prevailing wind (`nd_sim::weather::wind_dir`)
//! blows to the east and north-east, so the works sit at the east end by
//! the freight yard and the highway, farmland on the upwind west edge,
//! homes between, parks and a public beach along the shore.
//!
//! [`furnish`] then puts down, through the ordinary placement calls (every
//! result checked): diamond interchanges, power (solar, wind, hydro or
//! geothermal where the site allows, batteries for the evening peak and a
//! gas-free coal plant for base load), police, fire, health, schools, a
//! university, trash depots, a jail and dispatch, bus depots and lines, a
//! metro line through the core, rail stations and a freight yard, a ferry
//! terminal, districts with their policies, and a deep treasury.

use super::field::Basis;
use super::{dry, levels, Spec};
use glam::Vec2;
use nd_sim::landscape::Landscape;

/// The site: a temperate coast with the river to the east of downtown (the
/// works across it), two lakes by the western farms, hot spots beyond the
/// highway and an island offshore.
pub const SEED: u64 = 5;

/// Downtown, by the sea in the middle of the city.
pub const DOWNTOWN: Vec2 = Vec2::new(0.0, -330.0);
/// The downtown square (a park block) this far north of the centre.
pub const SQUARE_NORTH: f32 = 110.0;
/// Midtown, inland.
pub const MIDTOWN: Vec2 = Vec2::new(-380.0, 60.0);
/// The works, downwind (east).
pub const WORKS: Vec2 = Vec2::new(640.0, -200.0);
/// Logistics by the highway, downwind.
pub const DEPOTS: Vec2 = Vec2::new(620.0, 230.0);
/// Farmland, upwind (west).
pub const FARMS: Vec2 = Vec2::new(-760.0, -120.0);

pub(super) fn spec(land: &Landscape) -> Spec {
    // (`water_distance` saturates at 161 m: ask for less, or `dry` walks
    // the point all the way to the middle of the map.)
    let downtown = dry(land, DOWNTOWN, 150.0);
    let midtown = dry(land, MIDTOWN, 150.0);
    let mut lv = levels();
    // Avenues a little closer than the other presets' arterials: more
    // capacity across the grid.
    lv[0].dsep = [300.0, 300.0];
    lv[1].dsep = [150.0, 150.0];
    let centre = Vec2::new(0.0, -160.0);
    Spec {
        centre,
        radii: Vec2::new(960.0, 760.0),
        cores: vec![(downtown, 330.0), (midtown, 220.0)],
        // No radial plaza: its star of streets jams once downtown fills. The
        // square is a park block instead (`zone`).
        plazas: Vec::new(),
        bases: vec![Basis::Grid { centre, size: 6000.0, theta: 0.0, weight: 1.6 }],
        twists: Vec::new(),
        industry: vec![(WORKS, 330.0), (DEPOTS, 240.0)],
        farms: vec![(FARMS, 240.0)],
        line_pull: 0.25,
        levels: lv,
    }
}
