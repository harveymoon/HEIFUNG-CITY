//! [sound] The UI cues, found by watching what changed rather than by
//! wiring a call into every tool: the state after the sim's step is noted
//! (`snapshot`), and what the player's input and the HUD changed before
//! the next step is heard (`actions`, `ui_events`).

use bevy::prelude::*;
use bevy_egui::egui;
use nd_audio::Cue;

use super::Sound;
use crate::selection::Selection;
use crate::sim::Sim;
use crate::tools::{Tool, ToolState};
use crate::ui::{Info, UiFlags};

/// The state the last frame's step left.
#[derive(Resource, Default)]
pub(super) struct Seen {
    generation: Option<u64>,
    tool: Option<Tool>,
    points: usize,
    /// The active project: id and steps.
    project: Option<(u64, usize)>,
    gestures: usize,
    buildings: usize,
    refused: bool,
    speed: usize,
    paused: bool,
    /// The HUD's last frame: windows open, City Data, the selection, the toast.
    windows: Vec<Info>,
    data_view: bool,
    selected: bool,
    message: String,
    ui_seen: bool,
}

pub(super) fn snapshot(sim: Res<Sim>, tools: Res<ToolState>, mut seen: ResMut<Seen>) {
    let w = &sim.world;
    seen.generation = Some(sim.generation);
    seen.tool = Some(tools.tool);
    seen.points = tools.points.len();
    seen.project = w.project.as_ref().map(|p| (p.id, p.steps.len()));
    seen.gestures = w.master.gestures.len();
    seen.buildings = w.buildings.len();
    seen.refused = w.implement_refused.is_some();
    seen.speed = sim.speed_index;
    seen.paused = sim.paused;
}

/// What the tools, the HUD's buttons and the keys changed since the step.
pub(super) fn actions(
    sim: Res<Sim>,
    tools: Res<ToolState>,
    flags: Res<UiFlags>,
    mouse: Res<ButtonInput<MouseButton>>,
    seen: Res<Seen>,
    mut sound: ResMut<Sound>,
) {
    let w = &sim.world;
    if seen.generation != Some(sim.generation) {
        // A city loaded or started: nothing the player did.
        return;
    }
    if seen.tool.is_some_and(|t| t != tools.tool) {
        sound.push(Cue::ToolChange);
    }
    if tools.points.len() > seen.points {
        sound.push(Cue::RoadPoint);
    }
    let project = w.project.as_ref().map(|p| (p.id, p.steps.len()));
    let gestures = w.master.gestures.len();
    let refused = w.implement_refused.is_some();
    match (seen.project, project) {
        // A new step in the same project: what it did.
        (Some((a, n)), Some((b, m))) if a == b && m > n => {
            if let Some(step) = w.project.as_ref().and_then(|p| p.steps.last()) {
                sound.push(step_cue(step, tools.tool));
            }
        }
        // A first step in a fresh project.
        (None, Some((_, m))) if m > 0 => {
            if let Some(step) = w.project.as_ref().and_then(|p| p.steps.last()) {
                sound.push(step_cue(step, tools.tool));
            }
        }
        _ => {}
    }
    // Implemented: the project went into the plan.
    if seen.project.is_some_and(|(_, n)| n > 0) && project.map(|p| p.0) != seen.project.map(|p| p.0) && gestures > seen.gestures && !refused {
        sound.push(Cue::Implement);
    }
    if refused && !seen.refused {
        sound.push(Cue::Error);
    }
    let buildings = w.buildings.len();
    if buildings > seen.buildings && tools.tool == Tool::Buildings {
        sound.push(Cue::BuildingPlaced);
    }
    if buildings < seen.buildings && tools.tool == Tool::Bulldoze {
        sound.push(Cue::Bulldoze);
    }
    // A click the Buildings tool says no to.
    if tools.tool == Tool::Buildings && mouse.just_pressed(MouseButton::Left) && !flags.pointer_over_ui && tools.build_here.as_ref().is_some_and(|b| !b.0) {
        sound.push(Cue::Error);
    }
    if sim.paused != seen.paused {
        sound.push(if sim.paused { Cue::Pause } else { Cue::Resume });
    } else if sim.speed_index != seen.speed {
        sound.push(if sim.speed_index > seen.speed { Cue::SpeedUp } else { Cue::SpeedDown });
    }
}

/// What a project step did, by its changes.
fn step_cue(step: &nd_sim::plan::PlanDelta, tool: Tool) -> Cue {
    if step.values().any(|g| g.is_none()) {
        return Cue::Bulldoze;
    }
    if step.values().flatten().any(|g| matches!(g, nd_sim::Gesture::Zone { .. } | nd_sim::Gesture::ParcelZone { .. })) {
        return Cue::ZonePaint;
    }
    // Dragging a point with the Edit tool is a point placed, not a road made.
    if tool == Tool::Edit {
        Cue::RoadPoint
    } else {
        Cue::RoadDone
    }
}

/// The HUD's frame: buttons clicked, windows opened and closed, City Data,
/// the inspector, toasts. Read between egui's end of pass and its output.
pub(super) fn ui_events(
    outputs: Query<&bevy_egui::EguiFullOutput, With<bevy_egui::PrimaryEguiContext>>,
    mut contexts: Query<&mut bevy_egui::EguiContext, With<bevy_egui::PrimaryEguiContext>>,
    flags: Res<UiFlags>,
    selection: Res<Selection>,
    mut seen: ResMut<Seen>,
    mut sound: ResMut<Sound>,
) {
    if let Ok(out) = outputs.single() {
        if let Some(full) = out.0.as_ref() {
            let clicked = full.platform_output.events.iter().any(|e| match e {
                egui::output::OutputEvent::Clicked(info) => !matches!(info.typ, egui::WidgetType::TextEdit | egui::WidgetType::Slider | egui::WidgetType::DragValue),
                _ => false,
            });
            if clicked {
                sound.push(Cue::Click);
            }
        }
    }
    let windows: Vec<Info> = match contexts.single_mut() {
        Ok(mut ctx) => {
            let ctx = ctx.get_mut();
            Info::ALL.into_iter().filter(|i| i.is_open(ctx)).collect()
        }
        Err(_) => return,
    };
    let selected = *selection != Selection::None;
    if seen.ui_seen {
        let opened = windows.iter().any(|i| !seen.windows.contains(i)) || (flags.data_view && !seen.data_view) || (selected && !seen.selected);
        let closed = seen.windows.iter().any(|i| !windows.contains(i)) || (!flags.data_view && seen.data_view) || (!selected && seen.selected);
        if opened {
            sound.push(Cue::WindowOpen);
        } else if closed {
            sound.push(Cue::WindowClose);
        }
        if !flags.message.is_empty() && flags.message != seen.message {
            sound.push(Cue::Alert);
        }
    }
    seen.ui_seen = true;
    seen.windows = windows;
    seen.data_view = flags.data_view;
    seen.selected = selected;
    if seen.message != flags.message {
        seen.message = flags.message.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::{Gesture, GestureId, LandUse, Vec2 as SVec2};

    /// [sound] A project step's cue: a deletion is a bulldoze, zones are
    /// paint, a road is a road finished (or a point moved with Edit).
    #[test]
    fn a_project_step_sounds_like_what_it_did() {
        let road = Gesture::Roundabout { center: SVec2::ZERO, radius: 20.0, lanes: 1, level: 0.0 };
        let zone = Gesture::Zone { polygon: vec![SVec2::ZERO, SVec2::X, SVec2::Y], land_use: LandUse::Residential };
        let step = |g: Option<Gesture>| -> nd_sim::plan::PlanDelta { [(GestureId(1), g)].into_iter().collect() };
        assert_eq!(step_cue(&step(Some(road.clone())), Tool::Road), Cue::RoadDone);
        assert_eq!(step_cue(&step(Some(road)), Tool::Edit), Cue::RoadPoint);
        assert_eq!(step_cue(&step(Some(zone)), Tool::Zone), Cue::ZonePaint);
        assert_eq!(step_cue(&step(None), Tool::Bulldoze), Cue::Bulldoze);
    }
}
