//! Player tools: draw roads and zones into a project, paint parcels with the
//! zone brush or marquee, edit control points, bulldoze gestures, inspect
//! things. Undo/redo/implement live here too.
//!
//! One file per tool (`road`, `zone`, `edit`, `bulldoze`, `buildings`,
//! `transit`, `rail`; Inspect is `pick`), each with its input and its
//! preview; the input frame in `input`; the state in `state` and `kinds`.

mod buildings;
mod bulldoze;
pub(crate) use bulldoze::piece_at as road_piece_at;
mod edit;
mod input;
mod kinds;
pub mod line_edit;
mod pick;
mod plan_cache;
mod rail;
mod road;
mod state;
mod transit;
mod zone;

use bevy::prelude::*;
use nd_sim::Vec2 as SVec2;

use crate::camera::{Cursor, OrbitCamera};
use crate::overlay::OverlaySet;
use crate::sim::{Sim, SimSet};
use crate::ui::UiFlags;

pub(crate) use input::tool_input;
pub use kinds::*;
pub(crate) use pick::{building_prism, pick, pick_click, Click};
pub use plan_cache::EffectivePlan;
pub use state::*;
pub use zone::marquee_rect;
pub(crate) use crate::selection::apply_pick;
pub use crate::selection::Selection;

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolState>()
            .init_resource::<EffectivePlan>()
            .add_plugins(crate::overlay::OverlayPlugin)
            .add_systems(Update, (tool_input, crate::road_select::keep_on_rebuild, zone::follow_zone_tool).chain().in_set(SimSet::Input))
            // [fix/review-app] After the sim step, like the meshes: drawn in
            // Input, outlines and rings used the pre-tick state and last
            // frame's `frac`, so at speed the gold box trailed its car.
            .add_systems(Update, draw_gizmos.in_set(OverlaySet::Draw))
            .add_systems(Update, (path_trim_toasts, connectivity_toasts).in_set(SimSet::Sync));
    }
}

pub(crate) const POINT_PICK_RADIUS: f32 = 8.0;

/// [park-paths] A path cut back because the park (or other grounds) it was
/// joined to went: say so.
fn path_trim_toasts(sim: Res<Sim>, mut flags: ResMut<UiFlags>) {
    for e in &sim.events {
        if let nd_sim::SimEvent::PathTrimmed { removed, grounds, .. } = e {
            flags.message = if *removed {
                format!("The {} a path was joined to is gone: the path is removed.", grounds.label())
            } else {
                format!("The {} a path was joined to is gone: the path is cut back to its last point.", grounds.label())
            };
        }
    }
}

/// [pt-walk] A new connectivity alert (repeated failed trips, a newly
/// stranded area with people in it): toast it. Alerts already there when
/// the city loads are not repeated.
fn connectivity_toasts(sim: Res<Sim>, mut flags: ResMut<UiFlags>, mut seen: Local<Option<(String, u32)>>) {
    let k = &sim.world.connectivity;
    let game = sim.world.identity.game_id.clone();
    let newest = k.next_seq;
    match seen.as_ref() {
        Some((g, s)) if *g == game => {
            if let Some(a) = k.alerts.iter().filter(|a| a.seq >= *s).last() {
                flags.message = a.text.clone();
            }
        }
        _ => {}
    }
    *seen = Some((game, newest));
}

/// The previews' colours.
const CYAN: Color = Color::linear_rgb(1.4, 1.6, 1.8);
const RED: Color = Color::linear_rgb(3.0, 0.2, 0.4);
const WHITE: Color = Color::linear_rgb(3.0, 3.0, 3.0);

/// A point of a preview, a little over the built surface.
fn lift(sim: &Sim, p: SVec2) -> Vec3 {
    Vec3::new(p.x, 0.8 + sim.world.surface_height(p), p.y)
}

/// What a tool's preview reads.
#[derive(Clone, Copy)]
struct Preview<'a> {
    tools: &'a ToolState,
    cursor: &'a Cursor,
    sim: &'a Sim,
    /// The effective plan, re-synced so a point dragged this frame draws
    /// where it is.
    plan: &'a nd_sim::Plan,
    /// The camera's yaw.
    yaw: f32,
    /// Seconds since start, for pulses.
    t: f32,
}

/// The tools' gizmos: the bus lines and rage marks (`overlay::routes`),
/// then the active tool's preview.
#[allow(clippy::too_many_arguments)]
fn draw_gizmos(mut gizmos: Gizmos, tools: Res<ToolState>, cursor: Res<Cursor>, sim: Res<Sim>, mut eplan: ResMut<EffectivePlan>, cams: Query<&OrbitCamera>, time: Res<Time>, cam_mode: Res<crate::camera::CameraMode>) {
    // Re-sync so a point dragged by `tool_input` this frame draws where it is.
    let plan = eplan.sync(&sim.world);
    let t = time.elapsed_secs();
    crate::overlay::routes::rage_marks(&mut gizmos, &sim, t, cams.iter().next(), *cam_mode == crate::camera::CameraMode::Walk);
    crate::overlay::routes::bus_lines(&mut gizmos, &tools, &cursor, &sim, t);
    let p = Preview { tools: &tools, cursor: &cursor, sim: &sim, plan, yaw: cams.iter().next().map_or(0.0, |c| c.yaw), t };
    let g = &mut gizmos;
    match tools.tool {
        Tool::Zone if tools.zone_mode != ZoneMode::Polygon => zone::paint_preview(g, &p),
        Tool::Road if tools.roundabout => road::roundabout_preview(g, &p),
        Tool::Road => road::preview(g, &p),
        Tool::Zone => zone::polygon_preview(g, &p),
        Tool::Edit => edit::preview(g, &p),
        Tool::Bulldoze => bulldoze::preview(g, &p),
        Tool::Rail if tools.dock.is_some() => rail::dock_preview(g, &p),
        Tool::Buildings => buildings::draw_build_preview(g, &tools, &cursor, &sim),
        Tool::Rail => rail::preview(g, &p),
        Tool::Transit => {}
        Tool::Inspect => {}
        // [districts] drawn by `district_tool::district_gizmos`
        Tool::District => {}
    }
}
