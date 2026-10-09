//! [overlay] Everything drawn over the city for the player rather than as
//! part of it: the selection, the tools' previews, highlights and marks.
//!
//! - [`OverlaySet`]: `Validate` runs right after the sim step (the
//!   selection is checked there, `crate::selection::validate_selection`);
//!   `Build` (meshes) then `Draw` (gizmos) run inside `SimSet::Sync`, so
//!   every overlay sees the post-tick state and this frame's `frac`.
//! - [`selection`]: the one drawing of the selection, an exhaustive match.
//! - [`shapes`]: the gold, the flat-circle rotation, boxes, outlines and
//!   mesh pieces the overlays share.

use bevy::prelude::*;

use crate::sim::SimSet;

pub mod selection;
pub mod shapes;

/// When the overlays run: see the module docs.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum OverlaySet {
    /// After `SimSet::Step`, before `SimSet::Sync`: drop what the step
    /// made stale (a selection whose thing is gone).
    Validate,
    /// In `SimSet::Sync`: mesh overlays rebuilt or restyled.
    Build,
    /// In `SimSet::Sync`, after `Build`: gizmos.
    Draw,
}

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::selection::Selection>()
            .configure_sets(Update, OverlaySet::Validate.after(SimSet::Step).before(SimSet::Sync))
            .configure_sets(Update, (OverlaySet::Build, OverlaySet::Draw).chain().in_set(SimSet::Sync))
            // [fix/review-app] The selection checked once, right after the
            // sim moved, for every reader after it (overlays, the remote
            // API's `selected`, a flag's snapshot), not only by the
            // inspector's egui pass.
            .add_systems(Update, crate::selection::validate_selection.in_set(OverlaySet::Validate))
            .add_systems(Update, draw_selected.in_set(OverlaySet::Draw));
    }
}

/// The selection's gizmos (see [`selection::draw_selection`]).
#[allow(clippy::too_many_arguments)]
fn draw_selected(
    mut gizmos: Gizmos,
    sim: Res<crate::sim::Sim>,
    sel: Res<crate::selection::Selection>,
    tools: Res<crate::tools::ToolState>,
    time: Res<Time>,
    (mode, follow): (Res<crate::camera::CameraMode>, Res<crate::camera::CameraFollow>),
    assets: Res<crate::assets::GameAssets>,
    water: Res<crate::ferry_select::WaterFrame>,
) {
    let cx = selection::SelectionCx {
        sim: &sim,
        sel: *sel,
        t: time.elapsed_secs(),
        palette: &assets.palette.car_colors,
        under: tools.depth == crate::tools::Depth::Underground,
        pov_vehicle: follow.pov_in_vehicle(&sim, *mode),
        pov_walk: *mode == crate::camera::CameraMode::Walk && follow.target.is_some(),
        water: water.fresh(&sim),
    };
    selection::draw_selection(&mut gizmos, &cx);
}
