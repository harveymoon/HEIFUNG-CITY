//! [horizon] The land past the map: the same ground, coast and sea the map
//! is made of, carried on out to the horizon.
//!
//! [r4-tiles] It lives in the simulation now (`nd_sim::landscape::far`):
//! land bought past the map is generated from the same function, so a
//! bought tile is exactly the ground the horizon showed there.

pub use nd_sim::landscape::far::*;
