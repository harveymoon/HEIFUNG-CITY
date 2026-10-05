//! Procedural generators. Everything here produces plain mesh data so it can
//! be unit-tested and rendered by any engine.

pub mod buildings;
pub mod mesh;
pub mod palette;
pub mod terrain;

pub use buildings::{generate_building, BuildingGeometry, BuildingRules, LandUseRules};
pub use mesh::MeshData;
pub use palette::Palette;
pub use terrain::{build_terrain, terrain_height, TerrainConfig};
