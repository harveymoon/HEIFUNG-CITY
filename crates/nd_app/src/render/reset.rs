//! The end-of-frame reset of the full-rebuild flag.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// Clear the full-rebuild flag once every reader has seen it. It runs
/// after all of `SimSet::Sync` (the renderers ordered after
/// `sync_cars` and `sync_trains` included); clearing it inside
/// `sync_buildings` hid it from the emergency, stunt, turbine, air, parked
/// car and layer renderers, so a Load kept their stale meshes.
pub(super) fn clear_full_rebuild(mut sim: ResMut<Sim>) {
    if sim.full_rebuild {
        sim.full_rebuild = false;
    }
}
