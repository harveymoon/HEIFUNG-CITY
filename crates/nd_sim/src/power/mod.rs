//! Electricity: plants the player places, a weather state that solar and
//! wind depend on, and an hourly allocation of supply to demand. The grid
//! itself is abstract (one city-wide pool); who goes dark first is not: the
//! rich are served first, so brownouts land on the cheap districts.

use crate::buildings::{Building, Tier};
use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

pub use crate::weather::{SiteResource, Spell, Weather};

mod battery;
mod dispatch;
mod load;
mod plants;

pub use battery::*;
pub use dispatch::*;
pub use load::*;
pub use plants::*;

#[cfg(test)]
mod tests;
