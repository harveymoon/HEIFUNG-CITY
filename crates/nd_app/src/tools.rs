//! Player tools: draw roads and zones into a project, edit control points,
//! bulldoze gestures, inspect things. Undo/redo/implement live here too.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use nd_sim::roads::project_on_segment;
use nd_sim::zoning::point_in_polygon;
use nd_sim::plan::sample_curve;
use nd_sim::roads::RoadKind;
use nd_sim::{Activity, BuildingId, CarId, CitizenId, Gesture, GestureId, LandUse, Plan, Vec2 as SVec2, World};

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
    Power,
    Transit,
    Rail,
}

impl Tool {
    /// Toolbar order, which is also the digit hotkey order (1 to 8).
    pub const ALL: [Tool; 8] = [Tool::Inspect, Tool::Road, Tool::Zone, Tool::Edit, Tool::Bulldoze, Tool::Power, Tool::Transit, Tool::Rail];

    /// The name on the toolbar button.
    pub fn label(self) -> &'static str {
        match self {
            Tool::Inspect => "Inspect",
            Tool::Road => "Road",
            Tool::Zone => "Zone",
            Tool::Edit => "Edit",
            Tool::Bulldoze => "Bulldoze",
            Tool::Power => "Power",
            Tool::Transit => "Buses",
            Tool::Rail => "Rail",
        }
    }

    /// The keys that pick the tool (see `tool_hotkeys`), shown under the
    /// button. Edit has no letter: Q and E turn the camera.
    pub fn hotkey(self) -> &'static str {
        match self {
            Tool::Inspect => "1 / I",
            Tool::Road => "2 / R",
            Tool::Zone => "3 / Z",
            Tool::Edit => "4",
            Tool::Bulldoze => "5 / B",
            Tool::Power => "6 / N",
            Tool::Transit => "7 / T",
            Tool::Rail => "8 / Y",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TransitMode {
    #[default]
    Route,
    Depot,
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
    /// Plant to place with the Power tool.
    pub power_kind: nd_sim::PowerPlantKind,
    /// Transit tool: drawing lines or placing a depot.
    pub transit_mode: TransitMode,
    /// The line being drawn or edited.
    pub route_sel: Option<nd_sim::RouteId>,
    pub hover_stop: Option<(nd_sim::RouteId, usize)>,
    /// Where the right button went down (window pixels); a right *click*
    /// (released within a few pixels) finishes a gesture, a right drag only
    /// orbits the camera. None when it went down over the UI.
    pub right_press: Option<Vec2>,
    pub stop_drag: Option<(nd_sim::RouteId, usize)>,
    /// Draw the lines whatever tool is active.
    pub show_routes: bool,
    /// Rail tool: station or freight yard.
    pub rail_kind: nd_sim::RailFacility,
    /// People starred in the person pane; listed in the People window.
    pub favorites: Vec<CitizenId>,
}

/// Road presets in the spirit of Cities Skylines' road menu: (label, forward, backward, kind).
pub const ROAD_PRESETS: [(&str, u8, u8, RoadKind); 8] = [
    ("Path", 0, 0, RoadKind::Path),
    ("Rail", 0, 0, RoadKind::Rail),
    ("Lane", 1, 1, RoadKind::Street),
    ("Street", 2, 2, RoadKind::Street),
    ("Avenue", 3, 3, RoadKind::Street),
    ("One-way", 2, 0, RoadKind::Street),
    ("Ramp", 1, 0, RoadKind::Street),
    ("Highway", 4, 0, RoadKind::Street),
];

impl ToolState {
    /// Where a road end dropped at `p` would snap to.
    pub fn road_snap(sim: &Sim, p: SVec2, kind: RoadKind) -> (SVec2, bool) {
        sim.world.roads.snap_point_kind(p, POINT_PICK_RADIUS, kind)
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
            right_press: None,
            hover_point: None,
            hover_gesture: None,
            curved: false,
            selected_gesture: None,
            kind: RoadKind::Street,
            power_kind: nd_sim::PowerPlantKind::Solar,
            transit_mode: TransitMode::Route,
            route_sel: None,
            hover_stop: None,
            stop_drag: None,
            show_routes: false,
            rail_kind: nd_sim::RailFacility::Station,
            favorites: Vec::new(),
        }
    }
}

/// The master plan with the project applied, cached between changes.
/// `World::effective_plan()` clones every gesture, and the tools, the gizmos
/// and the Edit panel all want it each frame; `plan_version` bumps on every
/// change to either, so comparing it is enough to know when to clone again.
#[derive(Resource, Default)]
pub struct EffectivePlan {
    plan: Plan,
    key: Option<u64>,
}

impl EffectivePlan {
    /// Bring the cache up to date with `world` (a clone only when the plan changed).
    pub fn sync(&mut self, world: &World) -> &Plan {
        if self.key != Some(world.plan_version) {
            self.plan = world.effective_plan().clone();
            self.key = Some(world.plan_version);
        }
        &self.plan
    }

    /// Forget the cache: a loaded world starts its own `plan_version` count.
    pub fn invalidate(&mut self) {
        self.key = None;
    }
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Citizen(CitizenId),
    Building(BuildingId),
    /// A vehicle with nobody to inspect at the wheel: a truck or a bus.
    /// Clicking a driven car selects its driver instead.
    Car(CarId),
}

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolState>()
            .init_resource::<Selection>()
            .init_resource::<EffectivePlan>()
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
                let hw = if *kind != RoadKind::Street { 4.0 } else { (*lanes_fwd + *lanes_bwd) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 + 3.0 };
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
    window: Query<&Window, With<PrimaryWindow>>,
    cursor: Res<Cursor>,
    flags: Res<UiFlags>,
    mode: Res<CameraMode>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut sim: ResMut<Sim>,
    mut eplan: ResMut<EffectivePlan>,
) {
    if *mode == CameraMode::Walk {
        tools.drag = None;
        return;
    }
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
        (KeyCode::Digit6, Some(KeyCode::KeyN), Tool::Power),
        (KeyCode::Digit7, Some(KeyCode::KeyT), Tool::Transit),
        (KeyCode::Digit8, Some(KeyCode::KeyY), Tool::Rail),
    ];
    for (a, b, tool) in picks {
        let alias = b.is_some_and(|b| key(b) && !ctrl);
        if (key(a) || alias) && !(tool == Tool::Zone && ctrl) {
            tools.tool = tool;
            tools.points.clear();
            tools.drag = None;
        }
    }
    if key(KeyCode::Escape) {
        tools.points.clear();
        tools.drag = None;
        *selection = Selection::None;
    }
    if map_key(KeyCode::Backspace) && matches!(tools.tool, Tool::Road | Tool::Zone) {
        tools.points.pop();
    }
    if map_key(KeyCode::Backspace) && tools.tool == Tool::Transit {
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
        tools.curved = !tools.curved;
    }

    // Project commands.
    if ctrl && key(KeyCode::KeyZ) {
        tools.points.clear();
        tools.drag = None;
        tools.status = if sim.world.undo() { "Undid last step".into() } else { "Nothing to undo".into() };
    }
    if ctrl && key(KeyCode::KeyY) {
        tools.status = if sim.world.redo() { "Redid step".into() } else { "Nothing to redo".into() };
    }
    if ctrl && key(KeyCode::Enter) {
        tools.points.clear();
        tools.drag = None;
        sim.world.implement();
        tools.status = "Project implemented".into();
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
    let finish = (map_key(KeyCode::Enter) && !ctrl) || right_click;
    let click = buttons.just_pressed(MouseButton::Left) && !shift && !flags.pointer_over_ui;
    let shift_click = buttons.just_pressed(MouseButton::Left) && shift && !flags.pointer_over_ui;
    let ground = cursor.ground;

    match tools.tool {
        Tool::Road => {
            if click {
                if let Some(p) = ground {
                    let (p, snapped) = ToolState::road_snap(&sim, p, tools.kind);
                    tools.points.push(p);
                    if snapped {
                        tools.status = "Snapped to the road: a junction forms there.".into();
                    }
                }
            }
            if finish {
                if tools.points.len() >= 2 {
                    let kind = tools.kind;
                    let (lf, lb) = if kind != RoadKind::Street { (0, 0) } else { (tools.lanes_fwd, tools.lanes_bwd) };
                    let g = Gesture::Road { points: tools.points.clone(), lanes_fwd: lf, lanes_bwd: lb, curved: tools.curved && tools.points.len() >= 3, kind };
                    sim.world.add_gesture(g);
                    tools.status = format!("{} {} added to project ({} points). Ctrl+Enter implements.", if tools.curved { "Curved" } else { "Straight" }, match kind { RoadKind::Path => "path", RoadKind::Rail => "track", RoadKind::Street => "road" }, tools.points.len());
                }
                tools.points.clear();
            }
        }
        Tool::Zone => {
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).and_then(|(id, _)| match plan.gestures.get(&id) {
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
            tools.hover_point = ground.and_then(|p| nearest_control_point(plan, p));
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);

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
                if let (Some(p), Some((id, seg))) = (ground, ground.and_then(|p| nearest_gesture(plan, p))) {
                    if let Some(mut g) = plan.gestures.get(&id).cloned() {
                        let at = (seg + 1).min(g.points().len());
                        g.points_mut().insert(at, p);
                        sim.world.set_gesture(id, g, true);
                        tools.status = "Control point inserted".into();
                    }
                }
            }
            if map_key(KeyCode::Delete) {
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
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);
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
        Tool::Transit => match tools.transit_mode {
            TransitMode::Route => {
                tools.hover_stop = ground.and_then(|p| sim.world.transit.stop_near(p, 5.0));
                if click {
                    if let Some((r, i)) = tools.hover_stop {
                        tools.route_sel = Some(r);
                        tools.stop_drag = Some((r, i));
                        tools.status = format!("{} selected. Drag the stop to move it, Delete removes it, click a street to add stops.", sim.world.transit.routes[r].name);
                    } else if let Some(p) = ground {
                        let r = match tools.route_sel {
                            Some(r) if sim.world.transit.routes.contains_key(r) => r,
                            _ => {
                                let r = sim.world.add_bus_route();
                                tools.route_sel = Some(r);
                                r
                            }
                        };
                        tools.status = match sim.world.add_bus_stop(r, p) {
                            Ok(i) => format!("Stop {} added to {}. Keep clicking streets round a loop; Enter or right-click when done.", i + 1, sim.world.transit.routes[r].name),
                            Err(e) => format!("No stop: {e}."),
                        };
                    }
                }
                if buttons.just_released(MouseButton::Left) {
                    if let Some((r, i)) = tools.stop_drag.take() {
                        if let Some(p) = ground {
                            let moved = sim.world.transit.routes.get(r).and_then(|x| x.stops.get(i)).is_some_and(|s| s.pos.distance(p) > 2.5);
                            if moved && sim.world.move_bus_stop(r, i, p) {
                                tools.status = "Stop moved.".into();
                            }
                        }
                    }
                }
                if map_key(KeyCode::Delete) {
                    if let Some((r, i)) = tools.hover_stop.take() {
                        sim.world.remove_bus_stop(r, i);
                        if !sim.world.transit.routes.contains_key(r) {
                            tools.route_sel = None;
                            tools.status = "Line removed.".into();
                        } else {
                            tools.status = "Stop removed.".into();
                        }
                    }
                }
                if finish {
                    if let Some(r) = tools.route_sel.take() {
                        let n = sim.world.transit.routes.get(r).map_or(0, |x| x.stops.len());
                        tools.status = if n >= 2 { "Line closed into a loop. Buses leave the depot in the morning.".into() } else { "A line needs at least two stops.".into() };
                    }
                }
            }
            TransitMode::Depot => {
                if click {
                    if let Some(p) = ground {
                        let lot = sim.world.lots.iter().find(|(_, l)| point_in_polygon(p, &l.polygon)).map(|(id, _)| id);
                        match lot {
                            Some(lot) => {
                                tools.status = match sim.world.build_depot(lot) {
                                    Ok(id) => {
                                        *selection = Selection::Building(id);
                                        "Bus depot built. Buses leave it in the morning.".into()
                                    }
                                    Err(e) => format!("Cannot build: {e}."),
                                };
                            }
                            None => tools.status = "Click a vacant lot by a street.".into(),
                        }
                    }
                }
            }
        },
        Tool::Rail => {
            if click {
                if let Some(p) = ground {
                    let kind = tools.rail_kind;
                    tools.status = match sim.world.place_rail_facility(kind, p) {
                        Ok(id) => {
                            *selection = Selection::Building(id);
                            // `place_rail_facility` sets the link; should a sim change
                            // ever leave it off, say so rather than panic mid-click.
                            let link = sim.world.buildings.get(id).and_then(|b| b.rail.as_ref()).map(|l| (l.rail_connected, l.road_connected));
                            match link {
                                Some((true, true)) => format!("{} placed and connected. Trains call from the next headway.", kind.label()),
                                Some((true, false)) => format!("{} placed on the track. Join a road to its street stub and it opens.", kind.label()),
                                Some((false, true)) => format!("{} placed by the street. Lay track onto its rail stub and it opens.", kind.label()),
                                Some((false, false)) => format!("{} placed. Lay track to its rail stub and a road to its street stub.", kind.label()),
                                None => format!("{} placed, but it reports no rail link; select it to check.", kind.label()),
                            }
                        }
                        Err(e) => format!("Cannot build: {e}."),
                    };
                }
            }
        }
        Tool::Power => {
            if click {
                if let Some(p) = ground {
                    let lot = sim.world.lots.iter().find(|(_, l)| point_in_polygon(p, &l.polygon)).map(|(id, _)| id);
                    match lot {
                        Some(lot) => {
                            let kind = tools.power_kind;
                            tools.status = match sim.world.build_power(lot, kind) {
                                Ok(id) => {
                                    *selection = Selection::Building(id);
                                    format!("{} built for {:.0}.", kind.label(), kind.build_cost())
                                }
                                Err(e) => format!("Cannot build: {e}."),
                            };
                        }
                        None => tools.status = "Click a vacant lot.".into(),
                    }
                }
            }
        }
    }
}

fn pick(sim: &Sim, p: SVec2) -> Selection {
    let w = &sim.world;
    // People first: anyone in the open, or at the wheel of a car.
    let mut best: Option<(f32, CitizenId)> = None;
    for (id, c) in w.citizens.iter() {
        if !c.is_visible() && !matches!(c.activity, Activity::Driving { .. }) {
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
    // Then vehicles: a car with a driver selects the driver (the car is
    // highlighted with them); a truck or a bus is selected on its own.
    let mut best_car: Option<(f32, CarId)> = None;
    for (id, car) in w.traffic.cars.iter() {
        let d = car.pos.distance(p);
        let reach = if car.bus.is_some() { 8.0 } else { 5.0 };
        if d < reach && best_car.map_or(true, |(bd, _)| d < bd) {
            best_car = Some((d, id));
        }
    }
    if let Some((_, id)) = best_car {
        let car = &w.traffic.cars[id];
        return if car.bus.is_none() && w.citizens.contains_key(car.citizen) { Selection::Citizen(car.citizen) } else { Selection::Car(id) };
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

fn draw_gizmos(mut gizmos: Gizmos, tools: Res<ToolState>, cursor: Res<Cursor>, sim: Res<Sim>, selection: Res<Selection>, mut eplan: ResMut<EffectivePlan>) {
    // Re-sync so a point dragged by `tool_input` this frame draws where it is.
    let plan = eplan.sync(&sim.world);
    draw_selection(&mut gizmos, &sim, *selection);
    let cyan = Color::linear_rgb(1.4, 1.6, 1.8);
    let red = Color::linear_rgb(3.0, 0.2, 0.4);
    let white = Color::linear_rgb(3.0, 3.0, 3.0);
    let lift = |p: SVec2| Vec3::new(p.x, 0.8, p.y);
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);

    // Bus lines: the lane path of every leg in the line's colour, stops as
    // rings, the selected line bright, broken legs red.
    if tools.tool == Tool::Transit || tools.show_routes {
        let palette = [[0.2, 1.4, 2.2], [2.4, 0.5, 0.3], [0.5, 2.2, 0.5], [2.2, 1.8, 0.3], [1.8, 0.4, 2.0], [0.3, 1.0, 2.4], [2.2, 1.0, 0.6], [1.2, 1.2, 1.2]];
        for (id, route) in &sim.world.transit.routes {
            let c = palette[route.color as usize % palette.len()];
            let sel = tools.route_sel == Some(id);
            let k = if sel { 1.6 } else { 0.7 };
            let col = Color::linear_rgb(c[0] * k, c[1] * k, c[2] * k);
            for (i, leg) in route.legs.iter().enumerate() {
                let pts: Vec<Vec3> = leg.iter().map(|&p| Vec3::new(p.x, 1.2, p.y)).collect();
                if pts.len() >= 2 {
                    gizmos.linestrip(pts, if route.broken.contains(&i) { red } else { col });
                }
            }
            for (i, s) in route.stops.iter().enumerate() {
                let hovered = tools.hover_stop == Some((id, i));
                gizmos.circle(Isometry3d::new(Vec3::new(s.pos.x, 1.2, s.pos.y), flat), if hovered { 4.0 } else { 2.4 }, if hovered { white } else { col });
            }
        }
        if tools.tool == Tool::Transit {
            if let Some(c) = cursor.ground {
                let on_street = sim.world.roads.nearest_street(c).is_some_and(|(_, _, q)| q.distance(c) <= nd_sim::transit::STOP_SNAP);
                if tools.transit_mode == TransitMode::Route && on_street && tools.hover_stop.is_none() {
                    gizmos.circle(Isometry3d::new(lift(c), flat), 2.0, cyan);
                }
            }
        }
    }

    match tools.tool {
        Tool::Road | Tool::Zone => {
            let mut ctrl: Vec<SVec2> = tools.points.clone();
            if let Some(c) = cursor.ground {
                let (c, snapped) = if tools.tool == Tool::Road { ToolState::road_snap(&sim, c, tools.kind) } else { (c, false) };
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
                if let Some(g) = plan.gestures.get(&id) {
                    let mut pts: Vec<Vec3> = g.points().iter().map(|&p| lift(p)).collect();
                    if !g.is_road() && pts.len() >= 3 {
                        pts.push(pts[0]);
                    }
                    gizmos.linestrip(pts, red);
                }
            }
        }
        Tool::Power => {
            if let Some(c) = cursor.ground {
                if let Some((_, lot)) = sim.world.lots.iter().find(|(_, l)| point_in_polygon(c, &l.polygon)) {
                    let k = tools.power_kind;
                    let ok = lot.building.is_none() && !lot.landlocked && k.allowed_on(lot.land_use) && lot.area() >= k.min_area();
                    let mut pts: Vec<Vec3> = lot.polygon.iter().map(|&p| lift(p)).collect();
                    if pts.len() >= 3 {
                        pts.push(pts[0]);
                        gizmos.linestrip(pts, if ok { Color::linear_rgb(0.3, 2.5, 0.6) } else { red });
                    }
                }
            }
        }
        Tool::Rail => {
            if let Some(c) = cursor.ground {
                // The piece as it would land: footprint, track side, street side.
                let (f, rail_fused, road_fused) = nd_sim::Facility::plan(tools.rail_kind, c, &sim.world.roads);
                let fp = f.footprint();
                let blocked = sim.world.buildings.values().any(|b| sim.world.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &fp)));
                let col = if blocked { red } else { Color::linear_rgb(0.3, 2.5, 0.6) };
                let mut pts: Vec<Vec3> = fp.iter().map(|&p| lift(p)).collect();
                pts.push(pts[0]);
                gizmos.linestrip(pts, col);
                let [ra, rb] = f.rail_line();
                gizmos.line(lift(ra), lift(rb), if rail_fused { white } else { Color::linear_rgb(1.6, 1.2, 0.3) });
                let [sa, sb] = f.road_line();
                gizmos.line(lift(sa), lift(sb), if road_fused { white } else { cyan });
            }
            // Connectors of placed pieces that still want joining.
            for (id, b) in sim.world.buildings.iter() {
                let Some(link) = &b.rail else { continue };
                if link.rail_connected && link.road_connected {
                    continue;
                }
                let Some(fac) = sim.world.facilities.values().find(|x| x.building == Some(id)) else { continue };
                if !link.rail_connected {
                    let [a, bb] = fac.rail_line();
                    gizmos.line(lift(a) + Vec3::Y, lift(bb) + Vec3::Y, red);
                }
                if !link.road_connected {
                    let [a, bb] = fac.road_line();
                    gizmos.line(lift(a) + Vec3::Y, lift(bb) + Vec3::Y, red);
                }
            }
            // The railway itself, so branches are easy to see while placing.
            for (sid, seg) in sim.world.roads.alive_segments() {
                if seg.is_rail() {
                    let (a, b) = sim.world.roads.segment_points(sid);
                    gizmos.line(lift(a), lift(b), Color::linear_rgb(1.6, 1.2, 0.3));
                }
            }
        }
        Tool::Transit => {}
        Tool::Inspect => {}
    }
}

/// The selected thing, bright: a building's lot raised off the ground, a
/// ring round a person or vehicle. For a person also the road ahead (the
/// rest of their route, the lane path of their car or bus, the line to
/// their station), the destination's lot, and the route so far, dimmer.
fn draw_selection(gizmos: &mut Gizmos, sim: &Sim, selection: Selection) {
    let w = &sim.world;
    let f = sim.frac();
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let gold = Color::linear_rgb(3.2, 2.4, 0.7);
    let route = Color::linear_rgb(0.5, 2.6, 3.2);
    let trail = Color::linear_rgb(0.18, 0.55, 0.75);
    let dest = Color::linear_rgb(2.8, 1.0, 3.0);
    let at = |p: SVec2, y: f32| Vec3::new(p.x, y, p.y);
    // The same blend as the renderer, so the ring sits under the figure.
    let blend = |a: SVec2, b: SVec2| if a.distance(b) < 30.0 { a.lerp(b, f) } else { b };

    match selection {
        Selection::None => {}
        Selection::Building(b) => lot_outline(gizmos, w, b, 1.6, gold),
        Selection::Car(id) => {
            let Some(car) = w.traffic.cars.get(id) else { return };
            let p = blend(car.prev_pos, car.pos);
            gizmos.circle(Isometry3d::new(at(p, 0.5), flat), if car.bus.is_some() { 7.5 } else { 4.0 }, gold);
            car_route(gizmos, w, car, route);
            if let Some(cargo) = &car.cargo {
                lot_outline(gizmos, w, cargo.to, 1.2, dest);
            }
        }
        Selection::Citizen(id) => {
            let Some(c) = w.citizens.get(id) else { return };
            let here = blend(c.prev_pos, c.pos);
            if !c.trail.is_empty() {
                let mut pts: Vec<Vec3> = c.trail.iter().map(|&q| at(q, 0.7)).collect();
                pts.push(at(here, 0.7));
                if pts.len() >= 2 {
                    gizmos.linestrip(pts, trail);
                }
            }
            let mut destination = c.transit.map(|t| t.target);
            match &c.activity {
                Activity::Traveling { to, path, idx, .. } => {
                    destination = to.or(destination);
                    let mut pts = vec![at(here, 1.0)];
                    pts.extend(path.iter().skip(*idx).map(|&q| at(q, 1.0)));
                    if pts.len() >= 2 {
                        gizmos.linestrip(pts, route);
                    }
                    gizmos.circle(Isometry3d::new(at(here, 0.4), flat), 1.6, gold);
                }
                Activity::Driving { car, to, .. } => {
                    destination = to.or(destination);
                    if let Some(car) = w.traffic.cars.get(*car) {
                        let p = blend(car.prev_pos, car.pos);
                        gizmos.circle(Isometry3d::new(at(p, 0.5), flat), 4.0, gold);
                        car_route(gizmos, w, car, route);
                    }
                }
                Activity::Riding { bus } => {
                    if let Some(bus) = w.traffic.cars.get(*bus) {
                        let p = blend(bus.prev_pos, bus.pos);
                        gizmos.circle(Isometry3d::new(at(p, 0.5), flat), 7.5, gold);
                        car_route(gizmos, w, bus, route);
                    }
                }
                Activity::OnTrain { train } => {
                    if let Some(t) = w.rail.trains.get(*train) {
                        let s = t.prev_s + (t.s - t.prev_s) * f;
                        let head = nd_sim::rail::point_along(&t.poly, s).0;
                        gizmos.circle(Isometry3d::new(at(head, 0.5), flat), 8.0, gold);
                        // The line ahead as far as the rider's station.
                        let alight = match c.transit.map(|t| t.leg) {
                            Some(nd_sim::transit::TransitLeg::Rail { alight, .. }) => Some(alight),
                            _ => None,
                        };
                        let s_end = alight.and_then(|b| t.stops.iter().find(|(stop, _)| *stop == b)).map_or_else(|| nd_sim::rail::path_length(&t.poly), |&(_, s1)| s1);
                        let pts: Vec<Vec3> = sub_polyline(&t.poly, s, s_end).iter().map(|&q| at(q, 1.0)).collect();
                        if pts.len() >= 2 {
                            gizmos.linestrip(pts, route);
                        }
                    }
                }
                Activity::Waiting { at: q } => {
                    gizmos.circle(Isometry3d::new(at(*q, 0.4), flat), 1.6, gold);
                }
                Activity::Idle | Activity::Sitting { .. } => {
                    gizmos.circle(Isometry3d::new(at(here, 0.4), flat), 1.6, gold);
                }
                Activity::AtHome | Activity::Working | Activity::Visiting(_) | Activity::Arriving => {
                    // Indoors: show the building they are in.
                    if let Some(b) = c.inside {
                        lot_outline(gizmos, w, b, 1.6, gold);
                    }
                }
            }
            if let Some(b) = destination {
                lot_outline(gizmos, w, b, 1.2, dest);
            }
        }
    }
}

fn lot_outline(gizmos: &mut Gizmos, w: &nd_sim::World, b: BuildingId, y: f32, col: Color) {
    let Some(lot) = w.buildings.get(b).and_then(|b| w.lots.get(b.lot)) else { return };
    let mut pts: Vec<Vec3> = lot.polygon.iter().map(|&p| Vec3::new(p.x, y, p.y)).collect();
    if pts.len() >= 3 {
        pts.push(pts[0]);
        gizmos.linestrip(pts, col);
    }
}

/// The rest of a car's lane route, from where it is to where it stops.
fn car_route(gizmos: &mut Gizmos, w: &nd_sim::World, car: &nd_sim::Car, col: Color) {
    let net = &w.traffic.net;
    // Routes outlive lane rebuilds for a tick; never index past the lanes.
    if car.idx >= car.route.len() || car.route.iter().any(|&l| l as usize >= net.lanes.len()) {
        return;
    }
    let pts: Vec<Vec3> = net.polyline(&car.route[car.idx..], car.s, car.s_end).iter().map(|&q| Vec3::new(q.x, 1.0, q.y)).collect();
    if pts.len() >= 2 {
        gizmos.linestrip(pts, col);
    }
}

/// The stretch of a polyline between two arc lengths.
fn sub_polyline(pts: &[SVec2], s0: f32, s1: f32) -> Vec<SVec2> {
    let (s0, s1) = if s0 <= s1 { (s0, s1) } else { (s1, s0) };
    let mut out = vec![nd_sim::rail::point_along(pts, s0).0];
    let mut acc = 0.0;
    for w in pts.windows(2) {
        acc += w[0].distance(w[1]);
        if acc > s0 && acc < s1 {
            out.push(w[1]);
        }
    }
    out.push(nd_sim::rail::point_along(pts, s1).0);
    out
}
