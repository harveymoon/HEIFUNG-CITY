//! [sound] HEIFUNG://CITY's sound, synthesised in code: no samples, no
//! downloaded assets. Render-side only: it is told what is around the
//! camera and what the player did, and never reads or writes the sim.
//!
//! - [`dsp`]: noise, oscillators, filters, smoothers.
//! - [`scene`]: what is around the listener and the bed settings it calls
//!   for (pure functions).
//! - [`beds`]: the ambience beds, high, mid and street, with weather,
//!   nature and water.
//! - [`cues`]: the UI sounds and the rate limiter.
//! - [`mix`]: the buses, slider gains and the master limiter.
//! - [`engine`]: everything mixed; voice pools ready for phase 2's spot
//!   sounds.
//! - [`output`]: the sound card (cpal) on its own thread, or silence.
//! - [`wav`]: offline renders and their measurements.

pub mod beds;
pub mod cues;
pub mod dsp;
pub mod engine;
pub mod mix;
pub mod output;
pub mod scene;
pub mod wav;

pub use cues::{Cue, RateLimiter};
pub use engine::{Command, Engine, Telemetry};
pub use mix::{Bus, MixLevels};
pub use output::{Handle, Status};
pub use scene::{bed_params, BedParams, Surroundings};
