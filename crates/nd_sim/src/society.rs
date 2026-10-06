//! The street-level society systems, in one saved bundle: crime and the
//! people who commit it (`crime.rs`), homelessness and skid row
//! (`homeless.rs`) and the city's trash (`trash.rs`). Each keeps its own
//! state here; the systems that drive them are `impl World` blocks in those
//! files, hooked into the tick from `world/society.rs`.
//!
//! They draw from their own random stream (like the services), so turning
//! them on or off never shifts what the rest of the simulation rolls.

use crate::SimRng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Society {
    pub crime: crate::crime::Crime,
    pub homeless: crate::homeless::Homeless,
    pub trash: crate::trash::Trash,
    /// Not saved: a loaded world reseeds it from the seed and the clock.
    #[serde(skip)]
    pub rng: Option<SimRng>,
}
