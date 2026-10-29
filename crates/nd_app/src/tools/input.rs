//! The tools' input: hotkeys and project commands (undo, redo,
//! implement) here, then the active tool's own input (`super::<tool>`),
//! given this frame's buttons and pointer as a [`Frame`].

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use nd_sim::{Plan, Vec2 as SVec2};

use super::{bulldoze, buildings, edit, elevation_label, rail, refused_text, road, transit, zone, EffectivePlan, Tool, ToolState, TransitMode};
use crate::camera::{CameraMode, Cursor, OrbitCamera};
use crate::selection::{apply_pick, Selection};
use crate::sim::Sim;
use crate::ui::UiFlags;

/// One frame of the player's input, as the tools read it.
#[derive(Clone, Copy)]
pub(super) struct Frame<'a> {
    pub keys: &'a ButtonInput<KeyCode>,
    pub buttons: &'a ButtonInput<MouseButton>,
    pub flags: &'a UiFlags,
    pub scroll: &'a AccumulatedMouseScroll,
    /// The effective plan (the master plan with the project applied).
    pub plan: &'a Plan,
    /// The camera's yaw (a marquee is square to it).
    pub yaw: f32,
    pub ctrl: bool,
    /// A right click (released within a few pixels of where it went down).
    pub right_click: bool,
    /// Enter (not Ctrl+Enter) on the map, or a right click.
    pub finish: bool,
    pub right_down: bool,
    pub click: bool,
    pub shift_click: bool,
    pub middle: bool,
    /// Where the pointer meets the ground.
    pub ground: Option<SVec2>,
}

impl Frame<'_> {
    /// A key just pressed, unless an egui text field has the keyboard
    /// (every key is typing then, not a hotkey).
    pub fn key(&self, k: KeyCode) -> bool {
        !self.flags.keyboard_captured && self.keys.just_pressed(k)
    }

    /// A key that stands in for a click on the map: it counts only when
    /// the pointer is on the map (over the panel it belongs to the widget
    /// under it).
    pub fn map_key(&self, k: KeyCode) -> bool {
        self.key(k) && !self.flags.pointer_over_ui
    }
}

pub(crate) fn tool_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    cursor: Res<Cursor>,
    flags: Res<UiFlags>,
    mode: Res<CameraMode>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut sim: ResMut<Sim>,
    mut eplan: ResMut<EffectivePlan>,
    cams: Query<&OrbitCamera>,
    scroll: Res<AccumulatedMouseScroll>,
) {
    if *mode == CameraMode::Walk {
        tools.drag = None;
        tools.stroke = None;
        return;
    }
    let yaw = cams.iter().next().map_or(0.0, |c| c.yaw);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // While an egui text field has focus every key is typing, not a hotkey.
    let key = |k: KeyCode| !flags.keyboard_captured && keys.just_pressed(k);
    // Keys that stand in for a click on the map count only when the pointer
    // is on the map; over the panel they belong to the widget under it.
    let map_key = |k: KeyCode| key(k) && !flags.pointer_over_ui;
    let plan = eplan.sync(&sim.world);

    // Tool hotkeys.
    // Digits always; letters as aliases where they do not clash with the
    // camera (Q/E yaw, WASD pan) — so Edit has no letter.
    let picks = [
        (KeyCode::Digit1, Some(KeyCode::KeyI), Tool::Inspect),
        (KeyCode::Digit2, Some(KeyCode::KeyR), Tool::Road),
        (KeyCode::Digit3, Some(KeyCode::KeyZ), Tool::Zone),
        (KeyCode::Digit4, None, Tool::Edit),
        (KeyCode::Digit5, Some(KeyCode::KeyB), Tool::Bulldoze),
        (KeyCode::Digit6, Some(KeyCode::KeyN), Tool::Buildings),
        (KeyCode::Digit7, Some(KeyCode::KeyT), Tool::Transit),
        (KeyCode::Digit8, Some(KeyCode::KeyY), Tool::Rail),
        (KeyCode::Digit9, None, Tool::District),
    ];
    for (a, b, tool) in picks {
        let alias = b.is_some_and(|b| key(b) && !ctrl);
        if (key(a) || alias) && !(tool == Tool::Zone && ctrl) {
            tools.tool = tool;
            tools.ring_pin = None;
            tools.points.clear();
            tools.drag = None;
            tools.stroke = None;
        }
    }
    // [lines] Editing a line ends with the line, or when another tool or
    // mode is picked; Esc lets a picked slot go, then finishes.
    if let Some(e) = tools.line_edit {
        if !super::line_edit::still_editing(sim.world.transit.routes.contains_key(e.route), tools.tool, tools.transit_mode) {
            tools.line_edit = None;
            tools.stop_drag = None;
            if sim.world.transit.routes.get(e.route).is_some_and(|r| r.stops.is_empty()) {
                sim.world.remove_bus_route(e.route);
            }
        } else if key(KeyCode::Escape) && !flags.data_view {
            if e.slot.is_some() {
                tools.line_edit = Some(super::line_edit::LineEdit { slot: None, ..e });
                tools.status = "Stops go in where they belong again.".into();
            } else {
                super::line_edit::finish(&mut sim.world, &mut tools);
            }
            return;
        }
    }
    // [w1] Editing a metro line ends with the line, or with the tool or mode.
    if let Some(e) = tools.metro_edit {
        if !sim.world.subway.lines.contains_key(e.line) || tools.tool != Tool::Transit || tools.transit_mode != TransitMode::Metro {
            tools.metro_edit = None;
        }
    }
    if key(KeyCode::Escape) && !flags.data_view {
        tools.points.clear();
        tools.drag = None;
        // A stroke already painted stays (Ctrl+Z takes it back); a marquee
        // not yet released is dropped.
        tools.stroke = None;
        *selection = Selection::None;
        if tools.tool != Tool::Transit {
            tools.route_sel = None;
        }
        // [w1] Esc ends editing a metro line (every change is already made).
        if tools.metro_edit.is_some() {
            super::metro_edit::finish(&sim.world, &mut tools);
        }
        // [dams] Esc leaves the dam wall for later (the inspector's "Draw
        // its dam wall" picks it up again).
        if let Some(step) = tools.dam.take() {
            tools.dam_note = None;
            let name = sim.world.buildings.get(step.plant).map_or_else(|| "The plant".to_string(), |b| b.name.clone());
            tools.status = format!("{name} has no dam wall yet: it makes no power until it has one (select it and press \"Draw its dam wall\").");
        }
        // [metro-ux] Esc cancels the line being drawn.
        if !tools.metro_draft.is_empty() {
            tools.status = "Line cancelled: nothing was built.".into();
        }
        tools.metro_draft.clear();
    }
    if map_key(KeyCode::Backspace) && matches!(tools.tool, Tool::Road | Tool::Zone) {
        tools.points.pop();
        let n = tools.points.len();
        tools.point_levels.truncate(n);
    }
    // Depth view: cycles Underground -> Ground -> Sky.
    if key(KeyCode::KeyU) && !ctrl {
        tools.depth = tools.depth.next();
        tools.status = format!("{} view.", tools.depth.label());
    }
    // Road level, as in Cities: Skylines: Page Up raises, Page Down lowers.
    if tools.tool == Tool::Road && !tools.roundabout && (key(KeyCode::PageUp) || key(KeyCode::PageDown)) {
        tools.step_elevation(key(KeyCode::PageUp));
        tools.status = format!("Drawing at {}.", elevation_label(tools.elevation));
    }
    // [tools] ] raises and [ lowers the next point while drawing (the
    // camera's bracket zoom gives way to the Road tool); with Shift, the
    // whole road drawn so far goes up or down with it.
    if tools.captures_brackets() && (key(KeyCode::BracketRight) || key(KeyCode::BracketLeft)) {
        let up = key(KeyCode::BracketRight);
        if tools.roundabout {
            tools.step_elevation(up);
            tools.status = format!("Roundabout at {} ([ lowers, ] raises): roads drawn to it at that level join it, others pass under or over.", elevation_label(tools.elevation));
        } else if shift && !tools.points.is_empty() {
            tools.step_whole_road(up);
            tools.status = format!("The whole road at {} (Shift+[ ] move every point; [ ] the next one).", elevation_label(tools.elevation));
        } else {
            tools.step_elevation(up);
            tools.status = format!("Next point at {} ([ lowers, ] raises; Shift for the whole road).", elevation_label(tools.elevation));
        }
    }
    if map_key(KeyCode::Backspace) && tools.tool == Tool::Transit && tools.transit_mode == TransitMode::Metro {
        // [metro-ux] Backspace takes back the last stop (or opens a loop).
        tools.status = crate::metro_ux::undo_stop(&sim.world, &mut tools.metro_draft);
    } else if map_key(KeyCode::Backspace) && tools.tool == Tool::Transit {
        if let Some(r) = tools.route_sel {
            let n = sim.world.transit.routes.get(r).map_or(0, |x| x.stops.len());
            if n > 0 {
                sim.world.remove_bus_stop(r, n - 1);
                if !sim.world.transit.routes.contains_key(r) {
                    tools.route_sel = None;
                }
            }
        }
    }
    if key(KeyCode::KeyC) && tools.tool == Tool::Road && !ctrl {
        let m = if tools.curved && !tools.circle { crate::tools::DrawMethod::Straight } else { crate::tools::DrawMethod::Curved };
        tools.set_draw(m);
    }

    // Project commands.
    if ctrl && key(KeyCode::KeyZ) {
        tools.points.clear();
        tools.drag = None;
        // [fix/review-app] A brush stroke's counts end with it.
        tools.stroke = None;
        tools.status = if sim.world.undo() { "Undid last step".into() } else { "Nothing to undo".into() };
    }
    if ctrl && key(KeyCode::KeyY) {
        tools.stroke = None;
        tools.status = if sim.world.redo() { "Redid step".into() } else { "Nothing to redo".into() };
    }
    if ctrl && key(KeyCode::Enter) {
        tools.points.clear();
        tools.drag = None;
        let name = sim.world.project.as_ref().map_or_else(|| "Project".to_string(), |p| p.name.clone());
        let roads = sim.world.preview().planned_roads.len();
        tools.status = match sim.world.try_implement() {
            // The roads would take a depot, station or other city structure,
            // or a raised road has no room for its columns.
            Err(why) => refused_text(&name, roads, &why),
            Ok(()) if sim.world.plan_warnings.is_empty() => "Project implemented".into(),
            Ok(()) => format!("Project implemented. {}", sim.world.plan_warnings.join(" ")),
        };
    }

    let pointer = window.single().ok().and_then(|w| w.cursor_position());
    if buttons.just_pressed(MouseButton::Right) {
        tools.right_press = if flags.pointer_over_ui { None } else { pointer };
    }
    let right_click = buttons.just_released(MouseButton::Right)
        && match (tools.right_press.take(), pointer) {
            (Some(down), Some(up)) => down.distance(up) < 6.0,
            _ => false,
        };
    // [r4-hillside] X (or the Road pane's button) lays the zig-zag offered
    // for a leg too steep for its kind.
    let zigzag = tools.tool == Tool::Road && !ctrl && (key(KeyCode::KeyX) || std::mem::take(&mut tools.switchback_asked)) && tools.take_switchback();
    let finish = (map_key(KeyCode::Enter) && !ctrl) || right_click || zigzag;
    let right_down = buttons.just_pressed(MouseButton::Right) && !flags.pointer_over_ui;
    let click = buttons.just_pressed(MouseButton::Left) && !shift && !flags.pointer_over_ui;
    let shift_click = buttons.just_pressed(MouseButton::Left) && shift && !flags.pointer_over_ui;
    // [fix/review-app] A middle click on a panel is not a map click: it
    // used to clear the remote API's pins.
    let middle = buttons.just_pressed(MouseButton::Middle) && !flags.pointer_over_ui;
    let ground = cursor.ground;
    tools.hover_ground = ground;

    // [r1-roads] (K20) The Intersection editor works on the map: while it
    // is open the tools leave the map's clicks to it.
    if tools.junction_editor.is_some() {
        return;
    }
    let f = Frame { keys: &keys, buttons: &buttons, flags: &flags, scroll: &scroll, plan, yaw, ctrl, right_click, finish, right_down, click, shift_click, middle, ground };
    let (sim, tools, selection) = (&mut *sim, &mut *tools, &mut *selection);
    match tools.tool {
        Tool::Road if tools.roundabout => road::roundabout_input(&f, sim, tools, selection),
        Tool::Road => road::input(&f, sim, tools, selection),
        Tool::Zone => zone::input(&f, sim, tools, selection),
        Tool::Edit => edit::input(&f, sim, tools, selection),
        // [districts] input in `district_tool::district_input`
        Tool::District => {}
        Tool::Bulldoze if tools.cleanup => bulldoze::cleanup_input(&f, sim, tools, selection),
        Tool::Bulldoze => bulldoze::input(&f, sim, tools, selection),
        Tool::Inspect => {
            // [feat/pt-ux] By the pointer's ray: a tower by its walls, an
            // aircraft in the air (even against the sky), and from far out
            // not the people.
            if click && (ground.is_some() || cursor.ray.is_some()) {
                {
                    let picked = super::pick_click(sim, &super::Click { ground, depth: tools.depth, ray: cursor.ray, view_dist: if ground.is_some() { cursor.view_dist } else { f32::INFINITY } });
                    apply_pick(sim, tools, selection, picked);
                }
            }
        }
        Tool::Transit => transit::input(&f, sim, tools, selection),
        Tool::Rail => rail::input(&f, sim, tools, selection),
        Tool::Buildings => buildings::input(&f, sim, tools, selection),
    }
}
