//! Procedural generators. Everything here produces plain mesh data so it can
//! be unit-tested and rendered by any engine.

pub mod buildings;
pub mod mesh;
pub mod palette;
pub mod street;
pub mod surface;
pub mod terrain;
pub mod textures;

pub use buildings::transit::bus_stops;
pub use buildings::Style as BuildingStyle;
pub use buildings::{generate_building, BuildingRules, Extras, LandUseRules, Style};
pub use mesh::MeshData;
pub use palette::Palette;
pub use surface::{Geometry, Surface};
pub use terrain::{build_terrain, terrain_height, TerrainConfig};
pub use textures::{generate as generate_texture, mip_chain, Texture};
