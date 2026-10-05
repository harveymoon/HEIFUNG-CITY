//! Player tools: draw roads and zones into a project, edit control points,
//! bulldoze gestures, inspect things. Undo/redo/implement live here too.

use bevy::prelude::*;
use nd_sim::roads::project_on_segment;
use nd_sim::zoning::point_in_polygon;
use nd_sim::plan::sample_curve;
use nd_sim::roads::RoadKind;
use nd_sim::{BuildingId, CitizenId, Gesture, GestureId, LandUse, Plan, Vec2 as SVec2};

use crate::camera::{CameraMode, Cursor};
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
    /// Draw smooth roads through the clicked points.
    pub curved: bool,
    /// Road picked with the Edit tool for upgrading.
    pub selected_gesture: Option<GestureId>,
    /// Street or path.
    pub kind: RoadKind,
}

/// Road presets in the spirit of Cities Skylines' road menu: (label, forward, backward, kind).
pub const ROAD_PRESETS: [(&str, u8, u8, RoadKind); 7] = [
    ("Path", 0, 0, RoadKind::Path),
    ("Lane", 1, 1, RoadKind::Street),
    ("Street", 2, 2, RoadKind::Street),
    ("Avenue", 3, 3, RoadKind::Street),
    ("One-way", 2, 0, RoadKind::Street),
    ("Ramp", 1, 0, RoadKind::Street),
    ("Highway", 4, 0, RoadKind::Street),
];

impl ToolState {
    /// Where a road end dropped at `p` would snap to.
    pub fn road_snap(sim: &Sim, p: SVec2) -> (SVec2, bool) {
        sim.world.roads.snap_point(p, POINT_PICK_RADIUS)
    }
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
            curved: false,
            selected_gesture: None,
            kind: RoadKind::Street,
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
            Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind } => {
                let hw = if *kind == RoadKind::Path { 4.0 } else { (*lanes_fwd + *lanes_bwd) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 + 3.0 };
                let built = g.road_polyline();
                for (i, w) in built.windows(2).enumerate() {
                    let (_, q) = project_on_segment(p, w[0], w[1]);
                    let d = p.distance(q);
                    if d < hw && best.map_or(true, |b| d < b.0) {
                        // Map the built leg back to the control leg for insertion.
                        let ctrl = if *curved {
                            let mid = (w[0] + w[1]) * 0.5;
                            (0..points.len().saturating_sub(1)).min_by(|&a, &b| {
                                let da = project_on_segment(mid, points[a], points[a + 1]).1.distance(mid);
                                let db = project_on_segment(mid, points[b], points[b + 1]).1.distance(mid);
                                da.total_cmp(&db)
                            }).unwrap_or(0)
                        } else {
                            i
                        };
                        best = Some((d, *id, ctrl));
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
    mode: Res<CameraMode>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut sim: ResMut<Sim>,
) {
    if *mode == CameraMode::Walk {
        tools.drag = None;
        return;
    }
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
    if keys.just_pressed(KeyCode::KeyC) && tools.tool == Tool::Road && !ctrl {
        tools.curved = !tools.curved;
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
                    let (p, snapped) = ToolState::road_snap(&sim, p);
                    tools.points.push(p);
                    if snapped {
                        tools.status = "Snapped to the road: a junction forms there.".into();
                    }
                }
            }
            if finish {
                if tools.points.len() >= 2 {
                    let kind = tools.kind;
                    let (lf, lb) = if kind == RoadKind::Path { (0, 0) } else { (tools.lanes_fwd, tools.lanes_bwd) };
                    let g = Gesture::Road { points: tools.points.clone(), lanes_fwd: lf, lanes_bwd: lb, curved: tools.curved && tools.points.len() >= 3, kind };
                    sim.world.add_gesture(g);
                    tools.status = format!("{} {} added to project ({} points). Ctrl+Enter implements.", if tools.curved { "Curved" } else { "Straight" }, if kind == RoadKind::Path { "path" } else { "road" }, tools.points.len());
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
                    tools.selected_gesture = Some(hp.0);
                } else {
                    tools.selected_gesture = tools.hover_gesture;
                    if tools.selected_gesture.is_some() {
                        tools.status = "Road selected: change lanes, direction or curve in the panel.".into();
                    }
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
            if point_in_polygon(p, &lot.polygon) {
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
            let mut ctrl: Vec<SVec2> = tools.points.clone();
            if let Some(c) = cursor.ground {
                let (c, snapped) = if tools.tool == Tool::Road { ToolState::road_snap(&sim, c) } else { (c, false) };
                gizmos.circle(Isometry3d::new(lift(c), flat), if snapped { 4.5 } else { 3.0 }, if snapped { white } else { cyan });
                ctrl.push(c);
            }
            for &p in &tools.points {
                gizmos.circle(Isometry3d::new(lift(p), flat), 2.0, cyan);
            }
            let built: Vec<SVec2> = if tools.tool == Tool::Road && tools.curved { sample_curve(&ctrl) } else { ctrl.clone() };
            let mut pts: Vec<Vec3> = built.iter().map(|&p| lift(p)).collect();
            if pts.len() >= 2 {
                if tools.tool == Tool::Zone && pts.len() >= 3 {
                    pts.push(pts[0]);
                }
                gizmos.linestrip(pts, cyan);
                if tools.tool == Tool::Road && tools.curved && ctrl.len() >= 3 {
                    gizmos.linestrip(ctrl.iter().map(|&p| lift(p)).collect::<Vec<_>>(), Color::linear_rgb(0.3, 0.4, 0.5));
                }
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
            for (id, colour) in [(tools.hover_gesture, Color::linear_rgb(0.0, 1.2, 1.5)), (tools.selected_gesture, Color::linear_rgb(2.5, 1.6, 0.4))] {
                let Some(id) = id else { continue };
                if let Some(g) = plan.gestures.get(&id) {
                    let mut pts: Vec<Vec3> = g.road_polyline().iter().map(|&p| lift(p)).collect();
                    if !g.is_road() && pts.len() >= 3 {
                        pts.push(pts[0]);
                    }
                    gizmos.linestrip(pts, colour);
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
