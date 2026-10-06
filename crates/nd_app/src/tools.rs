//! Player tools: draw roads and zones into a project, paint parcels with the
//! zone brush or marquee, edit control points, bulldoze gestures, inspect
//! things. Undo/redo/implement live here too.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use nd_sim::roads::project_on_segment;
use nd_sim::zoning::point_in_polygon;
use nd_sim::plan::sample_curve;
use nd_sim::roads::RoadKind;
use nd_sim::{Activity, BuildingId, CarId, CitizenId, Gesture, GestureId, LandUse, Plan, Vec2 as SVec2, World};

use crate::camera::{CameraMode, Cursor, OrbitCamera};
use crate::sim::{Sim, SimSet};
use crate::ui::UiFlags;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Inspect,
    Road,
    Zone,
    Edit,
    Bulldoze,
    /// Placed specials by sub-category: power, services, transit, rail,
    /// attractions (`nd_sim::services::BuildCategory`).
    Buildings,
    Transit,
    Rail,
}

impl Tool {
    /// Toolbar order, which is also the digit hotkey order (1 to 8).
    pub const ALL: [Tool; 8] = [Tool::Inspect, Tool::Road, Tool::Zone, Tool::Edit, Tool::Bulldoze, Tool::Buildings, Tool::Transit, Tool::Rail];

    /// The name on the toolbar button.
    pub fn label(self) -> &'static str {
        match self {
            Tool::Inspect => "Inspect",
            Tool::Road => "Road",
            Tool::Zone => "Zone",
            Tool::Edit => "Edit",
            Tool::Bulldoze => "Bulldoze",
            Tool::Buildings => "Buildings",
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
            Tool::Buildings => "6 / N",
            Tool::Transit => "7 / T",
            Tool::Rail => "8 / Y",
        }
    }
}

/// How the Zone tool zones: by polygon (every parcel whose centre is inside
/// takes the use), by brush (parcels under a circle as it is dragged), or
/// by marquee (parcels whose centre is in a dragged rectangle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZoneMode {
    Polygon,
    #[default]
    Brush,
    Marquee,
}

impl ZoneMode {
    pub const ALL: [ZoneMode; 3] = [ZoneMode::Brush, ZoneMode::Marquee, ZoneMode::Polygon];

    pub fn label(self) -> &'static str {
        match self {
            ZoneMode::Polygon => "Polygon",
            ZoneMode::Brush => "Brush",
            ZoneMode::Marquee => "Marquee",
        }
    }

    pub fn parse(s: &str) -> Option<ZoneMode> {
        ZoneMode::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// A brush stroke or marquee drag in progress: which button started it,
/// whether it erases, and the paint gesture it is growing.
#[derive(Clone, Debug)]
pub struct PaintStroke {
    pub button: MouseButton,
    pub erase: bool,
    pub gesture: Option<GestureId>,
    pub painted: usize,
    /// Parcels the brush passed over that refused the use, with a reason.
    pub refused: std::collections::BTreeMap<usize, &'static str>,
    /// Where a marquee drag started.
    pub from: Option<SVec2>,
}

/// Brush radius limits, metres.
pub const BRUSH_MIN: f32 = 4.0;
pub const BRUSH_MAX: f32 = 80.0;

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
    /// Buildings tool: the open sub-category, the item picked in it and
    /// the tier for services and attractions.
    pub build_cat: nd_sim::BuildCategory,
    pub build: nd_sim::Placeable,
    pub build_tier: nd_sim::Tier,
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
    /// Rail tool: a dock instead of a station or yard.
    pub dock: Option<nd_sim::PortKind>,
    /// People starred in the person pane; listed in the People window.
    pub favorites: Vec<CitizenId>,
    /// Zone tool: polygon, brush or marquee.
    pub zone_mode: ZoneMode,
    /// Zone brush radius in metres.
    pub brush_radius: f32,
    /// Zone tool: left button unzones instead of painting `land_use`.
    pub unzone: bool,
    pub stroke: Option<PaintStroke>,
    /// Parcel under the pointer (index into `World::parcels`).
    pub hover_parcel: Option<usize>,
    /// A project being renamed in the toolbar: its id and the name so far.
    pub rename: Option<(u64, String)>,
    /// Zone tool, industrial land: the sub-zone painting sets (a key of
    /// `nd_sim::world::INDUSTRY_CHOICES`; "auto" lets the ground decide).
    pub industry: &'static str,
    /// Road tool: stamp roundabouts instead of drawing roads.
    pub roundabout: bool,
    /// Roundabout radius (centre to the middle of the ring road), metres.
    pub ring_radius: f32,
    /// Roundabout ring lanes, 1 or 2.
    pub ring_lanes: u8,
    /// Where the roundabout would go (snapped), while the pointer is on the map.
    pub ring_hover: Option<SVec2>,
    /// A centre set by the remote API (`tool {ring_at}`) that stands in
    /// for the pointer until the mouse is used.
    pub ring_pin: Option<SVec2>,
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
    /// What the Buildings tool puts down: the picked item at the picked tier.
    pub fn build_item(&self) -> nd_sim::Placeable {
        self.build.with_tier(self.build_tier)
    }

    /// Pick an item (and open its sub-category).
    pub fn pick_build(&mut self, p: nd_sim::Placeable) {
        self.build_cat = p.category();
        if let nd_sim::Placeable::Service(_, t) = p {
            self.build_tier = t;
        }
        self.build = p;
    }

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
            build_cat: nd_sim::BuildCategory::Power,
            build: nd_sim::Placeable::Plant(nd_sim::PowerPlantKind::Solar),
            build_tier: nd_sim::Tier::Mid,
            transit_mode: TransitMode::Route,
            route_sel: None,
            hover_stop: None,
            stop_drag: None,
            show_routes: false,
            rail_kind: nd_sim::RailFacility::Station,
            dock: None,
            favorites: Vec::new(),
            zone_mode: ZoneMode::Brush,
            brush_radius: 12.0,
            unzone: false,
            stroke: None,
            hover_parcel: None,
            rename: None,
            industry: "auto",
            roundabout: false,
            ring_radius: 20.0,
            ring_lanes: 1,
            ring_hover: None,
            ring_pin: None,
        }
    }
}

impl ToolState {
    /// The roundabout the Road tool would stamp where the pointer is.
    pub fn ring_preview(&self) -> Option<nd_sim::roads::Ring> {
        if self.tool != Tool::Road || !self.roundabout {
            return None;
        }
        self.ring_hover.map(|c| nd_sim::roads::Ring::new(c, self.ring_radius, self.ring_lanes))
    }

    /// Does the mouse wheel belong to the tool (Ctrl+wheel sizes the
    /// roundabout) rather than the camera?
    pub fn captures_wheel(&self, ctrl: bool) -> bool {
        ctrl && self.tool == Tool::Road && self.roundabout
    }

    /// Does the active tool use right-drags itself (the zone brush erases
    /// with them), so the camera must not orbit?
    pub fn captures_right_drag(&self) -> bool {
        self.tool == Tool::Zone && matches!(self.zone_mode, ZoneMode::Brush | ZoneMode::Marquee)
    }

    /// The land use a paint stroke gives: the selected one, or none when
    /// unzoning.
    pub fn paint_use(&self, erase: bool) -> Option<LandUse> {
        if erase || self.unzone { None } else { Some(self.land_use) }
    }

    /// The industry sub-zone painting industrial land records.
    pub fn paint_industry(&self) -> Option<&'static str> {
        (self.land_use == LandUse::Industrial && !self.unzone).then_some(self.industry)
    }
}

/// The marquee from `a` to `b` as a rectangle square to the camera (its
/// yaw), so it looks like the box the player dragged on screen.
pub fn marquee_rect(a: SVec2, b: SVec2, yaw: f32) -> Vec<SVec2> {
    let (s, c) = yaw.sin_cos();
    let right = SVec2::new(c, -s);
    let fwd = SVec2::new(s, c);
    let d = b - a;
    vec![a, a + right * d.dot(right), b, a + fwd * d.dot(fwd)]
}

/// One stroke step: paint (or erase) the parcels in `ids`, note the ones
/// that refused, and keep the stroke's gesture growing.
fn paint_step(sim: &mut Sim, tools: &mut ToolState, ids: &[usize]) {
    let Some(mut stroke) = tools.stroke.take() else { return };
    let lu = tools.paint_use(stroke.erase);
    if let Some(lu) = lu {
        for &i in ids {
            if let Some(why) = sim.world.parcel_refusal(i, Some(lu)) {
                stroke.refused.insert(i, why);
            }
        }
    }
    let industry = if stroke.erase { None } else { tools.paint_industry() };
    let (g, n) = sim.world.paint_parcels_with(ids, lu, industry, stroke.gesture);
    stroke.gesture = g;
    stroke.painted += n;
    tools.stroke = Some(stroke);
}

/// What a finished stroke did, for the status line.
fn stroke_status(stroke: &PaintStroke, lu: Option<LandUse>) -> String {
    let what = lu.map_or("unzoned".to_string(), |l| format!("zoned {}", l.label()));
    let mut out = format!("{} parcel{} {what}.", stroke.painted, if stroke.painted == 1 { "" } else { "s" });
    if !stroke.refused.is_empty() {
        let mut reasons: Vec<&str> = stroke.refused.values().copied().collect();
        reasons.sort();
        reasons.dedup();
        out.push_str(&format!(" {} refused ({}).", stroke.refused.len(), reasons.join("; ")));
    }
    if stroke.painted > 0 {
        out.push_str(" Ctrl+Z undoes the stroke, Ctrl+Enter implements.");
    }
    out
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
            // Paint strokes have no shape to pick: undo or the eraser takes them back.
            Gesture::ParcelZone { .. } => {}
            Gesture::Roundabout { center, radius, lanes } => {
                let ring = nd_sim::roads::Ring::new(*center, *radius, *lanes);
                let d = (p.distance(ring.center) - ring.radius).abs();
                if d < ring.half_width() + 2.0 && best.map_or(true, |b| d < b.0) {
                    best = Some((d, *id, 0));
                }
            }
        }
    }
    best.map(|(_, id, i)| (id, i))
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
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
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
    if key(KeyCode::Escape) {
        tools.points.clear();
        tools.drag = None;
        // A stroke already painted stays (Ctrl+Z takes it back); a marquee
        // not yet released is dropped.
        tools.stroke = None;
        *selection = Selection::None;
        if tools.tool != Tool::Transit {
            tools.route_sel = None;
        }
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
        tools.status = if sim.world.plan_warnings.is_empty() { "Project implemented".into() } else { format!("Project implemented. {}", sim.world.plan_warnings.join(" ")) };
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
    let right_down = buttons.just_pressed(MouseButton::Right) && !flags.pointer_over_ui;
    let click = buttons.just_pressed(MouseButton::Left) && !shift && !flags.pointer_over_ui;
    let shift_click = buttons.just_pressed(MouseButton::Left) && shift && !flags.pointer_over_ui;
    let ground = cursor.ground;

    match tools.tool {
        Tool::Road if tools.roundabout => {
            // The pointer takes over from a centre the remote API pinned.
            if click || right_down || buttons.just_pressed(MouseButton::Middle) {
                tools.ring_pin = None;
            }
            tools.ring_hover = tools.ring_pin.or(if flags.pointer_over_ui { None } else { ground }).map(|p| sim.world.roundabout_snap(p));
            // Ctrl+wheel sizes it (the camera leaves the wheel alone then).
            if ctrl && !flags.pointer_over_ui {
                let lines = match scroll.unit {
                    bevy::input::mouse::MouseScrollUnit::Line => scroll.delta.y,
                    bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta.y / 60.0,
                };
                if lines != 0.0 {
                    tools.ring_radius = (tools.ring_radius + lines * 2.0).clamp(nd_sim::roads::RING_MIN_RADIUS, nd_sim::roads::RING_MAX_RADIUS);
                    tools.status = format!("Roundabout {:.0} m across.", tools.ring_radius * 2.0);
                }
            }
            if click {
                if let Some(c) = tools.ring_hover {
                    let (r, lanes) = (tools.ring_radius, tools.ring_lanes);
                    tools.status = match sim.world.add_roundabout(c, r, lanes) {
                        Ok(_) => format!("Roundabout {:.0} m across stamped into the project: roads crossing it are cut and joined, buildings in red come down. Ctrl+Z undoes, Ctrl+Enter implements.", r * 2.0),
                        Err(e) => format!("No roundabout: {e}."),
                    };
                }
            }
        }
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
                    let g = Gesture::Road { points: tools.points.clone(), lanes_fwd: lf, lanes_bwd: lb, curved: tools.curved && tools.points.len() >= 3, kind, elevation: Vec::new() };
                    sim.world.add_gesture(g);
                    tools.status = format!("{} {} added to project ({} points). Ctrl+Enter implements.", if tools.curved { "Curved" } else { "Straight" }, match kind { RoadKind::Path => "path", RoadKind::Rail => "track", RoadKind::Street => "road" }, tools.points.len());
                }
                tools.points.clear();
            }
        }
        Tool::Zone => {
            tools.hover_parcel = ground.and_then(|p| sim.world.parcels.at(p));
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).and_then(|(id, _)| match plan.gestures.get(&id) {
                Some(Gesture::Zone { .. }) => Some(id),
                _ => None,
            });
            match tools.zone_mode {
                ZoneMode::Polygon => {
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
                            let industry = tools.paint_industry();
                            sim.world.add_zone_gesture(tools.points.clone(), tools.land_use, industry);
                            tools.status = format!("{} zone added to project: the parcels whose centre it holds take it. Ctrl+Enter implements.", tools.land_use.label());
                        }
                        tools.points.clear();
                    }
                }
                ZoneMode::Brush | ZoneMode::Marquee => {
                    // Left paints the selected use (or unzones with Unzone
                    // picked), right always erases back to unzoned.
                    if tools.stroke.is_none() {
                        let start = if click { Some((MouseButton::Left, tools.unzone)) } else if right_down { Some((MouseButton::Right, true)) } else { None };
                        if let (Some((button, erase)), Some(p)) = (start, ground) {
                            tools.stroke = Some(PaintStroke { button, erase, gesture: None, painted: 0, refused: Default::default(), from: Some(p) });
                        }
                    }
                    let Some(stroke) = tools.stroke.clone() else { return };
                    let released = !buttons.pressed(stroke.button);
                    if tools.zone_mode == ZoneMode::Brush {
                        if let Some(p) = ground {
                            let ids = sim.world.parcels.in_circle(p, tools.brush_radius);
                            paint_step(&mut sim, &mut tools, &ids);
                        }
                    } else if released {
                        if let (Some(a), Some(b)) = (stroke.from, ground) {
                            let rect = marquee_rect(a, b, yaw);
                            let ids = sim.world.parcels.in_polygon(&rect);
                            paint_step(&mut sim, &mut tools, &ids);
                        }
                    }
                    if released {
                        if let Some(done) = tools.stroke.take() {
                            tools.status = stroke_status(&done, tools.paint_use(done.erase));
                        }
                    }
                }
            }
        }
        Tool::Edit => {
            tools.hover_point = ground.and_then(|p| nearest_control_point(plan, p));
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);

            if let Some((id, idx)) = tools.drag {
                if let (Some(p), Some(mut g)) = (ground, plan.gestures.get(&id).cloned()) {
                    if idx < g.points().len() {
                        g.move_point(idx, p);
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
                        if let Some(pts) = g.points_mut() {
                            pts.insert(at, p);
                            sim.world.set_gesture(id, g, true);
                            tools.status = "Control point inserted".into();
                        }
                    }
                }
            }
            if map_key(KeyCode::Delete) {
                if let Some((id, idx)) = tools.hover_point {
                    if let Some(mut g) = plan.gestures.get(&id).cloned() {
                        if g.points().len() > g.min_points() {
                            if let Some(pts) = g.points_mut() {
                                pts.remove(idx);
                            }
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
                    // A bus picked shows its whole line: path, stops, its
                    // sister buses. Anything else picked lets the line go.
                    tools.route_sel = match *selection {
                        Selection::Car(id) => sim.world.traffic.cars.get(id).and_then(|c| c.bus.as_ref()).map(|b| b.route),
                        _ => None,
                    };
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
            if let (true, Some(p), Some(dock)) = (click, ground, tools.dock) {
                tools.status = match sim.world.place_port(dock, p) {
                    Ok(id) => {
                        *selection = Selection::Building(id);
                        match sim.world.ports.port_of(id) {
                            Some(port) if port.works() => format!("{} built with its pier. Ships call from the next sailing.", dock.label()),
                            Some(port) if !port.road_connected => format!("{} built with its pier. Join its street stub to the city's roads and it opens.", dock.label()),
                            _ => format!("{} built, but no ship can reach it from off the map.", dock.label()),
                        }
                    }
                    Err(e) => format!("Cannot build: {e}."),
                };
            } else if click {
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
        Tool::Buildings => {
            if click {
                if let Some(p) = ground {
                    let what = tools.build_item();
                    tools.status = match sim.world.place_building(what, p) {
                        Ok(id) => {
                            *selection = Selection::Building(id);
                            format!("{} built for {:.0}; {:.0}/day to run.", what.label(), what.cost(), what.upkeep())
                        }
                        Err(e) => format!("Cannot build: {e}."),
                    };
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

fn draw_gizmos(mut gizmos: Gizmos, tools: Res<ToolState>, cursor: Res<Cursor>, sim: Res<Sim>, selection: Res<Selection>, mut eplan: ResMut<EffectivePlan>, cams: Query<&OrbitCamera>, time: Res<Time>) {
    // Re-sync so a point dragged by `tool_input` this frame draws where it is.
    let plan = eplan.sync(&sim.world);
    draw_selection(&mut gizmos, &sim, *selection);
    let cyan = Color::linear_rgb(1.4, 1.6, 1.8);
    let red = Color::linear_rgb(3.0, 0.2, 0.4);
    let white = Color::linear_rgb(3.0, 3.0, 3.0);
    let lift = |p: SVec2| Vec3::new(p.x, 0.8 + sim.world.surface_height(p), p.y);
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);

    // Bus lines: the lane path of every leg in the line's colour, stops as
    // rings, broken legs red. With the Buses tool or the toggle every line
    // shows; otherwise only a selected one (picked in the Transit window,
    // the Buses list or by clicking one of its buses), drawn bright with
    // marker posts at its stops and a pulsing ring over each of its buses.
    let all_lines = tools.tool == Tool::Transit || tools.show_routes;
    if all_lines || tools.route_sel.is_some() {
        let palette = [[0.2, 1.4, 2.2], [2.4, 0.5, 0.3], [0.5, 2.2, 0.5], [2.2, 1.8, 0.3], [1.8, 0.4, 2.0], [0.3, 1.0, 2.4], [2.2, 1.0, 0.6], [1.2, 1.2, 1.2]];
        let road_lift = |p: SVec2| Vec3::new(p.x, 1.2 + sim.world.surface_height(p), p.y);
        let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 4.0).sin();
        for (id, route) in &sim.world.transit.routes {
            let sel = tools.route_sel == Some(id);
            if !all_lines && !sel {
                continue;
            }
            let c = palette[route.color as usize % palette.len()];
            let k = if sel { 1.6 } else { 0.7 };
            let col = Color::linear_rgb(c[0] * k, c[1] * k, c[2] * k);
            for (i, leg) in route.legs.iter().enumerate() {
                let pts: Vec<Vec3> = leg.iter().map(|&p| road_lift(p)).collect();
                if pts.len() >= 2 {
                    let leg_col = if route.broken.contains(&i) { red } else { col };
                    if sel {
                        // Gizmo lines are a pixel wide: a second pass just
                        // above makes the selected line read as a band.
                        gizmos.linestrip(pts.iter().map(|p| *p + Vec3::Y * 0.6), leg_col);
                    }
                    gizmos.linestrip(pts, leg_col);
                }
            }
            for (i, s) in route.stops.iter().enumerate() {
                let hovered = tools.hover_stop == Some((id, i));
                let at = road_lift(s.pos);
                gizmos.circle(Isometry3d::new(at, flat), if hovered { 4.0 } else if sel { 3.2 } else { 2.4 }, if hovered { white } else { col });
                if sel {
                    gizmos.line(at, at + Vec3::Y * 9.0, col);
                    gizmos.circle(Isometry3d::new(at + Vec3::Y * 9.0, flat), 1.0, col);
                }
            }
            if sel {
                let f = sim.frac();
                let bright = Color::linear_rgb(c[0] * (1.6 + pulse), c[1] * (1.6 + pulse), c[2] * (1.6 + pulse));
                for car in sim.world.traffic.cars.values().filter(|car| car.bus.as_ref().is_some_and(|b| b.route == id)) {
                    let p = car.prev_pos.lerp(car.pos, f);
                    let at = Vec3::new(p.x, sim.world.car_height(car) + 0.3, p.y);
                    gizmos.circle(Isometry3d::new(at, flat), 7.0 + 1.5 * pulse, bright);
                    gizmos.line(at + Vec3::Y * 4.0, at + Vec3::Y * 16.0, bright);
                }
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
        Tool::Zone if tools.zone_mode != ZoneMode::Polygon => {
            let ok = Color::linear_rgb(0.3, 2.5, 0.6);
            let erase_col = Color::linear_rgb(2.2, 1.2, 0.4);
            let erasing = tools.stroke.as_ref().map_or(tools.unzone, |s| s.erase);
            let lu = tools.paint_use(erasing);
            let outline = |gizmos: &mut Gizmos, poly: &[SVec2], col: Color| {
                let mut pts: Vec<Vec3> = poly.iter().map(|&p| lift(p)).collect();
                if pts.len() >= 3 {
                    pts.push(pts[0]);
                    gizmos.linestrip(pts, col);
                }
            };
            let parcels = &sim.world.parcels;
            let mark = |gizmos: &mut Gizmos, ids: &[usize]| {
                for &i in ids.iter().take(400) {
                    let Some(p) = parcels.parcels.get(i) else { continue };
                    let col = match lu {
                        Some(l) if sim.world.parcel_refusal(i, Some(l)).is_none() => ok,
                        Some(_) => red,
                        None => erase_col,
                    };
                    outline(gizmos, &p.polygon, col);
                }
            };
            if let Some(c) = cursor.ground {
                if tools.zone_mode == ZoneMode::Brush {
                    gizmos.circle(Isometry3d::new(lift(c), flat), tools.brush_radius, if erasing { erase_col } else { cyan });
                    mark(&mut gizmos, &parcels.in_circle(c, tools.brush_radius));
                } else if let Some(a) = tools.stroke.as_ref().and_then(|s| s.from) {
                    let yaw = cams.iter().next().map_or(0.0, |c| c.yaw);
                    let rect = marquee_rect(a, c, yaw);
                    outline(&mut gizmos, &rect, if erasing { erase_col } else { cyan });
                    mark(&mut gizmos, &parcels.in_polygon(&rect));
                } else if let Some(i) = tools.hover_parcel {
                    mark(&mut gizmos, &[i]);
                }
            }
        }
        // The roundabout itself is drawn by the renderer (`sync_ring_preview`);
        // here only a ring of the outer edge, lifted onto the ground.
        Tool::Road if tools.roundabout => {
            if let Some(r) = tools.ring_preview() {
                gizmos.circle(Isometry3d::new(lift(r.center), flat), r.radius + r.half_width(), cyan);
                gizmos.circle(Isometry3d::new(lift(r.center), flat), 1.2, white);
            }
        }
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
        Tool::Rail if tools.dock.is_some() => {
            // The dock as it would land: footprint, pier, and its street side.
            if let (Some(c), Some(kind)) = (cursor.ground, tools.dock) {
                match sim.world.plan_port(kind, c) {
                    Ok(port) => {
                        let ok = Color::linear_rgb(0.3, 2.5, 0.6);
                        let mut pts: Vec<Vec3> = port.footprint().iter().map(|&p| lift(p)).collect();
                        pts.push(pts[0]);
                        gizmos.linestrip(pts, ok);
                        let [a, b] = port.pier;
                        gizmos.line(lift(a), lift(b), white);
                        gizmos.line(lift(b), lift(port.berth), cyan);
                        let [sa, sb] = port.road_line();
                        gizmos.line(lift(sa), lift(sb), cyan);
                    }
                    Err(_) => {
                        gizmos.circle(Isometry3d::new(lift(c), flat), 12.0, red);
                    }
                }
            }
            for port in sim.world.ports.ports.values().filter(|p| !p.works()) {
                let [a, b] = port.road_line();
                gizmos.line(lift(a) + Vec3::Y, lift(b) + Vec3::Y, red);
            }
        }
        Tool::Buildings => draw_build_preview(&mut gizmos, &tools, &cursor, &sim),
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
            let p = car.pose_at(f).0;
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
                        let p = car.pose_at(f).0;
                        gizmos.circle(Isometry3d::new(at(p, 0.5), flat), 4.0, gold);
                        car_route(gizmos, w, car, route);
                    }
                }
                Activity::Flying { drone } => {
                    if let Some(d) = w.air.drones.get(*drone) {
                        let p = blend(d.prev_pos, d.pos);
                        let alt = d.prev_alt + (d.alt - d.prev_alt) * f;
                        gizmos.circle(Isometry3d::new(at(p, alt), flat), 6.0, gold);
                        gizmos.line(at(p, alt), at(d.goal, d.goal_alt + 1.0), route);
                    }
                }
                Activity::Riding { bus } => {
                    if let Some(bus) = w.traffic.cars.get(*bus) {
                        let p = bus.pose_at(f).0;
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

/// The Buildings tool's preview: the lot under the cursor green or red,
/// the coverage radius round it for services (road distance; the ring is
/// the straight-line bound), and for rail pieces the footprint as placed.
fn draw_build_preview(gizmos: &mut Gizmos, tools: &ToolState, cursor: &Cursor, sim: &Sim) {
    let Some(c) = cursor.ground else { return };
    let w = &sim.world;
    let what = tools.build_item();
    let lift = |p: SVec2| Vec3::new(p.x, 0.8, p.y);
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let green = Color::linear_rgb(0.3, 2.5, 0.6);
    let red = Color::linear_rgb(3.0, 0.2, 0.4);
    if let nd_sim::Placeable::Dock(kind) = what {
        match w.plan_port(kind, c) {
            Ok(port) => {
                let mut pts: Vec<Vec3> = port.footprint().iter().map(|&p| lift(p)).collect();
                pts.push(pts[0]);
                gizmos.linestrip(pts, green);
                let [a, b] = port.pier;
                gizmos.line(lift(a), lift(b), Color::linear_rgb(3.0, 3.0, 3.0));
            }
            Err(_) => {
                gizmos.circle(Isometry3d::new(lift(c), flat), 12.0, red);
            }
        }
        return;
    }
    if let nd_sim::Placeable::Rail(kind) = what {
        let (f, _, _) = nd_sim::Facility::plan(kind, c, &w.roads);
        let fp = f.footprint();
        let blocked = w.buildings.values().any(|b| b.is_special() && w.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &fp)));
        let mut pts: Vec<Vec3> = fp.iter().map(|&p| lift(p)).collect();
        pts.push(pts[0]);
        gizmos.linestrip(pts, if blocked { red } else { green });
        return;
    }
    let Some((id, lot)) = w.lots.iter().find(|(_, l)| point_in_polygon(c, &l.polygon)) else { return };
    let ok = w.placement_problem(what, id).is_none();
    let mut pts: Vec<Vec3> = lot.polygon.iter().map(|&p| lift(p)).collect();
    if pts.len() >= 3 {
        pts.push(pts[0]);
        gizmos.linestrip(pts, if ok { green } else { red });
    }
    let r = what.radius();
    if r > 0.0 {
        let col = Color::linear_rgb(0.4, 1.4, 2.4);
        gizmos.circle(Isometry3d::new(lift(lot.center), flat), r, col);
        gizmos.circle(Isometry3d::new(lift(lot.center), flat), (r - nd_sim::services::COVERAGE_FADE).max(1.0), col.with_alpha(0.5));
    }
}
