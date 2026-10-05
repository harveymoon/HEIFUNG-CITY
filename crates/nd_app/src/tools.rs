//! Player tools: draw roads and zones into a project, edit control points,
//! bulldoze gestures, inspect things. Undo/redo/implement live here too.

use bevy::prelude::*;
use nd_sim::roads::project_on_segment;
use nd_sim::zoning::point_in_polygon;
use nd_sim::{BuildingId, CitizenId, Gesture, GestureId, LandUse, Plan, Vec2 as SVec2};

use crate::camera::Cursor;
use crate::sim::{Sim, SimSet};
use crate::ui::UiFlags;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Inspect,
    Road,
    Zone,
    Edit,
    Bulldoze,
}

#[derive(Resource)]
pub struct ToolState {
    pub tool: Tool,
    pub lanes_fwd: u8,
    pub lanes_bwd: u8,
    pub land_use: LandUse,
    pub points: Vec<SVec2>,
    pub status: String,
    pub drag: Option<(GestureId, usize)>,
    pub hover_point: Option<(GestureId, usize)>,
    pub hover_gesture: Option<GestureId>,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            tool: Tool::Road,
            lanes_fwd: 1,
            lanes_bwd: 1,
            land_use: LandUse::Residential,
            points: Vec::new(),
            status: String::new(),
            drag: None,
            hover_point: None,
            hover_gesture: None,
        }
    }
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Citizen(CitizenId),
    Building(BuildingId),
}

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolState>()
            .init_resource::<Selection>()
            .configure_sets(Update, (SimSet::Input, SimSet::Step, SimSet::Sync).chain())
            .add_systems(Update, (tool_input, draw_gizmos).chain().in_set(SimSet::Input));
    }
}

const POINT_PICK_RADIUS: f32 = 8.0;

fn nearest_control_point(plan: &Plan, p: SVec2) -> Option<(GestureId, usize)> {
    let mut best: Option<(f32, GestureId, usize)> = None;
    for (id, g) in &plan.gestures {
        for (i, &q) in g.points().iter().enumerate() {
            let d = p.distance(q);
            if d < POINT_PICK_RADIUS && best.map_or(true, |b| d < b.0) {
                best = Some((d, *id, i));
            }
        }
    }
    best.map(|(_, id, i)| (id, i))
}

/// Gesture under the cursor: a road whose polyline is close, or a zone that
/// contains the point. Returns the segment index for roads so points can be
/// inserted there.
fn nearest_gesture(plan: &Plan, p: SVec2) -> Option<(GestureId, usize)> {
    let mut best: Option<(f32, GestureId, usize)> = None;
    for (id, g) in &plan.gestures {
        match g {
            Gesture::Road { points, lanes_fwd, lanes_bwd } => {
                let hw = (*lanes_fwd + *lanes_bwd) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 + 3.0;
                for (i, w) in points.windows(2).enumerate() {
                    let (_, q) = project_on_segment(p, w[0], w[1]);
                    let d = p.distance(q);
                    if d < hw && best.map_or(true, |b| d < b.0) {
                        best = Some((d, *id, i));
                    }
                }
            }
            Gesture::Zone { polygon, .. } => {
                if point_in_polygon(p, polygon) {
                    let d = 1000.0;
                    if best.map_or(true, |b| d < b.0) {
                        best = Some((d, *id, 0));
                    }
                }
            }
        }
    }
    best.map(|(_, id, i)| (id, i))
}

fn tool_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Res<Cursor>,
    flags: Res<UiFlags>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut sim: ResMut<Sim>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // Tool hotkeys.
    let picks = [
        (KeyCode::Digit1, KeyCode::KeyI, Tool::Inspect),
        (KeyCode::Digit2, KeyCode::KeyR, Tool::Road),
        (KeyCode::Digit3, KeyCode::KeyZ, Tool::Zone),
        (KeyCode::Digit4, KeyCode::KeyE, Tool::Edit),
        (KeyCode::Digit5, KeyCode::KeyB, Tool::Bulldoze),
    ];
    for (a, b, tool) in picks {
        if (keys.just_pressed(a) || (keys.just_pressed(b) && !ctrl)) && !(tool == Tool::Zone && ctrl) {
            tools.tool = tool;
            tools.points.clear();
            tools.drag = None;
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        tools.points.clear();
        tools.drag = None;
        *selection = Selection::None;
    }
    if keys.just_pressed(KeyCode::Backspace) && matches!(tools.tool, Tool::Road | Tool::Zone) {
        tools.points.pop();
    }

    // Project commands.
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        tools.points.clear();
        tools.drag = None;
        tools.status = if sim.world.undo() { "Undid last step".into() } else { "Nothing to undo".into() };
    }
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        tools.status = if sim.world.redo() { "Redid step".into() } else { "Nothing to redo".into() };
    }
    if ctrl && keys.just_pressed(KeyCode::Enter) {
        tools.points.clear();
        tools.drag = None;
        sim.world.implement();
        tools.status = "Project implemented".into();
    }

    let finish = (keys.just_pressed(KeyCode::Enter) && !ctrl) || buttons.just_pressed(MouseButton::Right);
    let click = buttons.just_pressed(MouseButton::Left) && !shift && !flags.pointer_over_ui;
    let shift_click = buttons.just_pressed(MouseButton::Left) && shift && !flags.pointer_over_ui;
    let ground = cursor.ground;

    match tools.tool {
        Tool::Road => {
            if click {
                if let Some(p) = ground {
                    let p = sim.world.roads.node_near(p, POINT_PICK_RADIUS).map(|n| sim.world.roads.node(n).pos).unwrap_or(p);
                    tools.points.push(p);
                }
            }
            if finish {
                if tools.points.len() >= 2 {
                    let g = Gesture::Road { points: tools.points.clone(), lanes_fwd: tools.lanes_fwd, lanes_bwd: tools.lanes_bwd };
                    sim.world.add_gesture(g);
                    tools.status = format!("Road added to project ({} nodes). Ctrl+Enter implements.", tools.points.len());
                }
                tools.points.clear();
            }
        }
        Tool::Zone => {
            let plan = sim.world.effective_plan();
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(&plan, p)).and_then(|(id, _)| match plan.gestures.get(&id) {
                Some(Gesture::Zone { .. }) => Some(id),
                _ => None,
            });
            if click && ctrl {
                if let Some(id) = tools.hover_gesture {
                    if sim.world.repaint_zone(id, tools.land_use) {
                        tools.status = format!("Zone repainted as {}. Ctrl+Enter implements.", tools.land_use.label());
                    }
                }
            } else if click {
                if let Some(p) = ground {
                    tools.points.push(p);
                }
            }
            if finish {
                if tools.points.len() >= 3 {
                    let g = Gesture::Zone { polygon: tools.points.clone(), land_use: tools.land_use };
                    sim.world.add_gesture(g);
                    tools.status = format!("{} zone added to project. Ctrl+Enter implements.", tools.land_use.label());
                }
                tools.points.clear();
            }
        }
        Tool::Edit => {
            let plan = sim.world.effective_plan();
            tools.hover_point = ground.and_then(|p| nearest_control_point(&plan, p));
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(&plan, p)).map(|(id, _)| id);

            if let Some((id, idx)) = tools.drag {
                if let (Some(p), Some(mut g)) = (ground, plan.gestures.get(&id).cloned()) {
                    if idx < g.points().len() {
                        g.points_mut()[idx] = p;
                        let release = !buttons.pressed(MouseButton::Left);
                        sim.world.set_gesture(id, g, release);
                        if release {
                            tools.drag = None;
                            tools.status = "Control point moved".into();
                        }
                    } else {
                        tools.drag = None;
                    }
                } else {
                    tools.drag = None;
                }
            } else if click {
                if let Some(hp) = tools.hover_point {
                    tools.drag = Some(hp);
                }
            } else if shift_click {
                if let (Some(p), Some((id, seg))) = (ground, ground.and_then(|p| nearest_gesture(&plan, p))) {
                    if let Some(mut g) = plan.gestures.get(&id).cloned() {
                        let at = (seg + 1).min(g.points().len());
                        g.points_mut().insert(at, p);
                        sim.world.set_gesture(id, g, true);
                        tools.status = "Control point inserted".into();
                    }
                }
            }
            if keys.just_pressed(KeyCode::Delete) && !flags.pointer_over_ui {
                if let Some((id, idx)) = tools.hover_point {
                    if let Some(mut g) = plan.gestures.get(&id).cloned() {
                        if g.points().len() > g.min_points() {
                            g.points_mut().remove(idx);
                            sim.world.set_gesture(id, g, true);
                            tools.status = "Control point removed".into();
                        } else {
                            sim.world.remove_gesture(id);
                            tools.status = "Gesture removed".into();
                        }
                    }
                }
            }
        }
        Tool::Bulldoze => {
            let plan = sim.world.effective_plan();
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(&plan, p)).map(|(id, _)| id);
            if click {
                if let Some(id) = tools.hover_gesture {
                    sim.world.remove_gesture(id);
                    tools.status = "Marked for removal. Ctrl+Enter implements.".into();
                }
            }
        }
        Tool::Inspect => {
            if click {
                if let Some(p) = ground {
                    *selection = pick(&sim, p);
                }
            }
        }
    }
}

fn pick(sim: &Sim, p: SVec2) -> Selection {
    let w = &sim.world;
    let mut best: Option<(f32, CitizenId)> = None;
    for (id, c) in w.citizens.iter() {
        if !c.is_visible() && !matches!(c.activity, nd_sim::Activity::Driving { .. }) {
            continue;
        }
        let d = c.pos.distance(p);
        if d < 7.0 && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, id));
        }
    }
    if let Some((_, id)) = best {
        return Selection::Citizen(id);
    }
    for (_, lot) in w.lots.iter() {
        if let Some(b) = lot.building {
            if point_in_polygon(p, &lot.corners) {
                return Selection::Building(b);
            }
        }
    }
    Selection::None
}

fn draw_gizmos(mut gizmos: Gizmos, tools: Res<ToolState>, cursor: Res<Cursor>, sim: Res<Sim>) {
    let cyan = Color::linear_rgb(1.4, 1.6, 1.8);
    let red = Color::linear_rgb(3.0, 0.2, 0.4);
    let white = Color::linear_rgb(3.0, 3.0, 3.0);
    let lift = |p: SVec2| Vec3::new(p.x, 0.8, p.y);
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);

    match tools.tool {
        Tool::Road | Tool::Zone => {
            let mut pts: Vec<Vec3> = tools.points.iter().map(|&p| lift(p)).collect();
            if let Some(c) = cursor.ground {
                let c = if tools.tool == Tool::Road {
                    sim.world.roads.node_near(c, POINT_PICK_RADIUS).map(|n| sim.world.roads.node(n).pos).unwrap_or(c)
                } else {
                    c
                };
                gizmos.circle(Isometry3d::new(lift(c), flat), 3.0, cyan);
                pts.push(lift(c));
            }
            if pts.len() >= 2 {
                if tools.tool == Tool::Zone && pts.len() >= 3 {
                    pts.push(pts[0]);
                }
                gizmos.linestrip(pts, cyan);
            }
        }
        Tool::Edit => {
            let plan = sim.world.effective_plan();
            for (id, g) in &plan.gestures {
                for (i, &p) in g.points().iter().enumerate() {
                    let hovered = tools.hover_point == Some((*id, i)) || tools.drag == Some((*id, i));
                    gizmos.circle(Isometry3d::new(lift(p), flat), if hovered { 5.0 } else { 2.5 }, if hovered { white } else { cyan });
                }
            }
            if let Some(id) = tools.hover_gesture {
                if let Some(g) = plan.gestures.get(&id) {
                    let mut pts: Vec<Vec3> = g.points().iter().map(|&p| lift(p)).collect();
                    if !g.is_road() && pts.len() >= 3 {
                        pts.push(pts[0]);
                    }
                    gizmos.linestrip(pts, Color::linear_rgb(0.0, 1.2, 1.5));
                }
            }
        }
        Tool::Bulldoze => {
            if let Some(id) = tools.hover_gesture {
                if let Some(g) = sim.world.gesture(id) {
                    let mut pts: Vec<Vec3> = g.points().iter().map(|&p| lift(p)).collect();
                    if !g.is_road() && pts.len() >= 3 {
                        pts.push(pts[0]);
                    }
                    gizmos.linestrip(pts, red);
                }
            }
        }
        Tool::Inspect => {}
    }
}
