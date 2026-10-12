//! Procedural generators. Everything here produces plain mesh data so it can
//! be unit-tested and rendered by any engine.

pub mod buildings;
/// [climate] Each climate's ground, plants, water, snow and weather looks.
pub mod climate;
/// Colour helpers (`rgba`, `mul`, `mix`, `at`, `pick`).
pub mod color;
/// [rail-crossing] Level crossing posts, lamps and barrier arms.
pub mod crossing;
/// [plants] The hydro plant's arch dam.
pub mod dam;
/// Named heights and decal depths (what stands proud of what).
pub mod depth;
pub mod footbridge;
/// Stateless hashes and their salts.
pub mod hash;
/// [mapgen] Whole example cities traced through a tensor field.
pub mod citygen;
pub mod lights;
/// [subway] Metro tunnels, platforms and cars for the Underground view.
pub mod metro;
pub mod mesh;
pub mod overlay;
pub mod palette;
pub mod preview;
/// [looks] Low-poly people from a citizen's look.
pub mod people;
/// [society] Camps, RVs and overflowing trash.
pub mod society;
/// [social] Murals and community gardens.
pub mod social;
pub mod street;
pub mod surface;
pub mod terrain;
pub mod toll;
pub mod textures;
pub mod vehicles;
pub mod vacant;
pub mod walk;

#[cfg(test)]
mod golden_tests;

pub use buildings::transit::bus_stops;
pub use buildings::dirty;
pub use toll::{toll_booths, TollDraw};
pub use buildings::{battery_lights, solar_head, solar_trackers, SolarTracker, SOLAR_HUB, SOLAR_MAX_TILT, building_style, style_extras, coal_vents, CoalVent, generate_building, wind_rotor, wind_turbines, BuildingRules, PlantField, CraneRig, crane_slew, GameRig, Extras, LandUseRules, Neighbour, Style, StyleBook, Turbine, NEON_MARK, NIGHT_MARK, WINDOW_LIGHT_MARK};
pub use mesh::MeshData;
pub use palette::Palette;
pub use surface::{Geometry, Surface};
pub use terrain::{build_far_ground, build_terrain, flat_height, scatter_nature, terrain_height, TerrainConfig};
pub use textures::{generate as generate_texture, mip_chain, roughness_map, Texture};
pub use vacant::generate_vacant;
pub use vehicles::{vehicle_mesh, VehicleMesh};
pub use walk::{walk_detail, WALK_RADIUS, WALK_REDRAW};
