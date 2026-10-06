//! Procedural generators. Everything here produces plain mesh data so it can
//! be unit-tested and rendered by any engine.

pub mod buildings;
pub mod mesh;
pub mod palette;
pub mod street;
pub mod surface;
pub mod terrain;
pub mod textures;
pub mod vehicles;
pub mod vacant;
pub mod walk;

pub use buildings::transit::bus_stops;
pub use buildings::{generate_building, BuildingRules, Extras, LandUseRules, Neighbour, Style, StyleBook};
pub use mesh::MeshData;
pub use palette::Palette;
pub use surface::{Geometry, Surface};
pub use terrain::{build_terrain, scatter_nature, terrain_height, TerrainConfig};
pub use textures::{generate as generate_texture, mip_chain, Texture};
pub use vacant::generate_vacant;
pub use vehicles::{vehicle_mesh, VehicleMesh};
pub use walk::{walk_detail, WALK_RADIUS, WALK_REDRAW};
