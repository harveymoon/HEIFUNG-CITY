//! Power plants: a coal station with stacks and a coal yard, a field of
//! hexagonal solar panels on sun-tracking pedestals, a farm of helical wind turbines, and a nuclear station with its
//! containment dome and cooling towers.

use super::parts::*;
use super::Ctx;
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use rand::Rng;

mod battery;
mod coal;
mod field;
mod nuclear;
mod packer;
mod solar;
mod wind;
pub use battery::*;
pub use coal::*;
pub use field::*;
pub use nuclear::*;
use packer::*;
pub use solar::*;
pub use wind::*;

#[cfg(test)]
mod tests;
