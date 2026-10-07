//! Player tools: draw roads and zones into a project, paint parcels with the
//! zone brush or marquee, edit control points, bulldoze gestures, inspect
//! things. Undo/redo/implement live here too.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use nd_sim::roads::project_on_segment;
use nd_sim::zoning::point_in_polygon;
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
    /// [districts] Paint named districts and set their policies
    /// (`crate::district_tool`).
    District,
}

impl Tool {
    /// Toolbar order, which is also the digit hotkey order (1 to 9).
    pub const ALL: [Tool; 9] = [Tool::Inspect, Tool::Road, Tool::Zone, Tool::Edit, Tool::Bulldoze, Tool::Buildings, Tool::Transit, Tool::Rail, Tool::District];

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
            Tool::District => "Districts",
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
            Tool::District => "9",
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
    /// [subway] Place metro stations and join them into lines.
    Metro,
}

/// Which layer of the city the view and the tools work on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Depth {
    /// Tunnels: the surface hidden, ground roads as faint outlines.
    Underground,
    #[default]
    Ground,
    /// Routes in the air (a placeholder for now).
    Sky,
}

impl Depth {
    pub const ALL: [Depth; 3] = [Depth::Underground, Depth::Ground, Depth::Sky];

    pub fn label(self) -> &'static str {
        match self {
            Depth::Underground => "Underground",
            Depth::Ground => "Ground",
            Depth::Sky => "Sky",
        }
    }

    pub fn parse(s: &str) -> Option<Depth> {
        Depth::ALL.iter().copied().find(|d| d.label().eq_ignore_ascii_case(s.trim()))
    }

    /// The next layer for the cycle key.
    pub fn next(self) -> Depth {
        match self {
            Depth::Underground => Depth::Ground,
            Depth::Ground => Depth::Sky,
            Depth::Sky => Depth::Underground,
        }
    }
}

/// Elevation steps of the Road tool (Page Up / Page Down, or ] and [ while
/// drawing; Shift+[ ] for the whole road), metres.
pub const ELEVATION_STEP: f32 = 2.0;
pub const ELEVATION_MIN: f32 = -20.0;
pub const ELEVATION_MAX: f32 = 24.0;

/// How the sub-pane names a level.
pub fn elevation_label(level: f32) -> String {
    if level == 0.0 {
        "ground".into()
    } else if level >= nd_sim::roads::VIADUCT_LEVEL {
        format!("+{level:.0} m viaduct")
    } else if level > 0.0 {
        format!("+{level:.0} m embankment")
    } else if level <= nd_sim::roads::TUNNEL_LEVEL {
        format!("\u{2212}{:.0} m tunnel", -level)
    } else {
        format!("\u{2212}{:.0} m cutting", -level)
    }
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
    /// [lines] The stop picked in the Lines window (or with the remote
    /// API's `bus select_stop`): marked on the map with a tall post.
    pub stop_sel: Option<(nd_sim::RouteId, usize)>,
    /// Where the right button went down (window pixels); a right *click*
    /// (released within a few pixels) finishes a gesture, a right drag only
    /// orbits the camera. None when it went down over the UI.
    pub right_press: Option<Vec2>,
    pub stop_drag: Option<(nd_sim::RouteId, usize)>,
    /// Draw the lines whatever tool is active.
    pub show_routes: bool,
    /// [subway] Metro mode: the stations of the line being laid, in order.
    /// [metro-ux] Clicked station by station; a loop when closed on the first.
    pub metro_draft: nd_sim::subway::MetroDraft,
    /// [metro-ux] The station under the pointer in Metro mode.
    pub metro_hover: Option<BuildingId>,
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
    /// [grid-weather] Where the pointer meets the ground, for the Buildings
    /// tool's reading of the wind and sun under it.
    pub hover_ground: Option<SVec2>,
    /// A project being renamed in the toolbar: its id and the name so far.
    pub rename: Option<(u64, String)>,
    /// Zone tool, industrial land: the sub-zone painting sets (a key of
    /// `nd_sim::world::INDUSTRY_CHOICES`; "auto" lets the ground decide).
    pub industry: &'static str,
    /// Road tool: stamp roundabouts instead of drawing roads.
    pub roundabout: bool,
    /// Roundabout radius (centre to the middle of the ring road), metres.
    pub ring_radius: f32,
    /// Roundabout ring lanes, 1 to 3 (`roads::RING_MAX_LANES`).
    pub ring_lanes: u8,
    /// Where the roundabout would go (snapped), while the pointer is on the map.
    pub ring_hover: Option<SVec2>,
    /// A centre set by the remote API (`tool {ring_at}`) that stands in
    /// for the pointer until the mouse is used.
    pub ring_pin: Option<SVec2>,
    /// Road tool: the level the next point is placed at, metres above the
    /// ground (Page Up / Page Down step it).
    pub elevation: f32,
    /// The level of each of `points` (kept in step by `push_point`).
    pub point_levels: Vec<f32>,
    /// Road tool: drawing tunnels (deep, joined only to tunnels; portals
    /// where they climb to the ground).
    pub tunnel: bool,
    /// The layer the view shows and the tools work on.
    pub depth: Depth,
    /// What is wrong with the road being drawn (too steep, crossing at an
    /// awkward height), refreshed with its preview.
    pub grade_warnings: Vec<String>,
    /// A point set by the remote API (`tool {cursor}`) that stands in for
    /// the pointer in the road preview until the mouse moves.
    pub cursor_pin: Option<SVec2>,
    /// [districts] The District tool: the district painted, brush or
    /// polygon, the polygon's points (`crate::district_tool`).
    pub district: crate::district_tool::DistrictTool,
    /// [tools] The columns of the raised road being drawn, refreshed with
    /// its preview.
    pub pillars: PillarNote,
    /// [tools] The Intersection editor, when open.
    pub junction_editor: Option<crate::ui::junction::EditorState>,
    /// [tools] What implementing the active project would refuse on,
    /// refreshed with the plan view (None: nothing).
    pub plan_check: Option<nd_sim::ProjectCheck>,
}

/// [tools] What the player is told when implementing a project is refused:
/// nothing of it was built (it is all or nothing), and why.
pub fn refused_text(name: &str, roads: usize, why: &str) -> String {
    let why = why.trim_start_matches("Not implemented: ");
    if roads > 0 {
        format!("{name}: 0 of {roads} road{} built, nothing changed. {why}", if roads == 1 { "" } else { "s" })
    } else {
        format!("{name}: not implemented, nothing changed. {why}")
    }
}

/// [tools] What the Road tool says about the columns of the raised road
/// being drawn.
#[derive(Clone, Debug, Default)]
pub struct PillarNote {
    pub count: usize,
    /// Buildings a column would buy out (names).
    pub buyouts: Vec<String>,
    /// Empty lots a column takes.
    pub lots: usize,
    /// Why it cannot be built (no room for a column), if so.
    pub refusal: Option<String>,
    pub gaps: Vec<nd_sim::pillars::Gap>,
    pub at: Vec<SVec2>,
}

impl PillarNote {
    pub fn of(w: &nd_sim::World, plan: &nd_sim::pillars::PillarPlan) -> Self {
        Self {
            count: plan.pillars.len(),
            buyouts: plan.buyouts.iter().filter_map(|(_, b)| w.buildings.get(*b).map(|b| b.name.clone())).collect(),
            lots: plan.lots.len(),
            refusal: plan.refusal(),
            gaps: plan.gaps.clone(),
            at: plan.pillars.iter().map(|p| p.at).collect(),
        }
    }
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

    /// Where a point placed at `p` at `level` lands, and its level: it snaps
    /// onto a road only at about the same level (a viaduct passes over the
    /// street under it), unless `any` (the first point: a ramp or a portal
    /// starts on whatever road it is put on, at that road's level).
    pub fn road_snap_level(sim: &Sim, p: SVec2, kind: RoadKind, level: f32, any: bool) -> (SVec2, bool, f32) {
        let roads = &sim.world.roads;
        let (q, snapped) = roads.snap_point_kind(p, POINT_PICK_RADIUS, kind);
        if !snapped {
            return (p, false, level);
        }
        let at = roads.snap_level(p, POINT_PICK_RADIUS, kind).unwrap_or(0.0);
        if any || (at - level).abs() < nd_sim::roads::GRADE_SEPARATION {
            (q, true, at)
        } else {
            (p, false, level)
        }
    }

    /// Add a point of the road being drawn with its level.
    pub fn push_point(&mut self, p: SVec2, level: f32) {
        self.point_levels.resize(self.points.len(), self.elevation);
        self.points.push(p);
        self.point_levels.push(level);
    }

    /// The levels of `points`, one each.
    pub fn levels(&self) -> Vec<f32> {
        let mut l = self.point_levels.clone();
        l.resize(self.points.len(), self.elevation);
        l
    }

    /// Pick a road preset by label (or "Tunnel"): its kind and lanes, and
    /// the depth it is built at; the view follows (tunnels underground,
    /// everything else on the ground).
    pub fn pick_preset(&mut self, label: &str) -> bool {
        if label.eq_ignore_ascii_case("tunnel") {
            self.roundabout = false;
            self.kind = RoadKind::Street;
            if !self.tunnel && (self.lanes_fwd + self.lanes_bwd == 0 || self.kind != RoadKind::Street) {
                self.lanes_fwd = 2;
                self.lanes_bwd = 2;
            }
            self.tunnel = true;
            if self.elevation > nd_sim::roads::TUNNEL_LEVEL {
                self.elevation = nd_sim::roads::TUNNEL_DEPTH;
            }
            self.depth = Depth::Underground;
            return true;
        }
        let Some((_, f, b, k)) = ROAD_PRESETS.iter().find(|(l, ..)| l.eq_ignore_ascii_case(label)) else { return false };
        self.roundabout = false;
        self.kind = *k;
        if *k == RoadKind::Street {
            self.lanes_fwd = *f;
            self.lanes_bwd = *b;
        }
        if self.tunnel {
            self.tunnel = false;
            self.elevation = 0.0;
        }
        if self.depth == Depth::Underground {
            self.depth = Depth::Ground;
        }
        true
    }

    /// [tools] Does the Road tool take the [ and ] keys (the camera zooms
    /// with them otherwise)?
    pub fn captures_brackets(&self) -> bool {
        self.tool == Tool::Road && !self.roundabout
    }

    /// [tools] Raise or lower the whole road being drawn a step: every
    /// point placed so far and the next one.
    pub fn step_whole_road(&mut self, up: bool) {
        let max = if self.tunnel { 0.0 } else { ELEVATION_MAX };
        let d = if up { ELEVATION_STEP } else { -ELEVATION_STEP };
        let mut levels = self.levels();
        for l in &mut levels {
            *l = (*l + d).clamp(ELEVATION_MIN, max);
        }
        self.point_levels = levels;
        self.step_elevation(up);
    }

    /// Step the drawing level (tunnels stay deep).
    pub fn step_elevation(&mut self, up: bool) {
        let max = if self.tunnel { 0.0 } else { ELEVATION_MAX };
        let e = self.elevation + if up { ELEVATION_STEP } else { -ELEVATION_STEP };
        self.elevation = e.clamp(ELEVATION_MIN, max);
    }
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            district: Default::default(),
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
            stop_sel: None,
            stop_drag: None,
            show_routes: false,
            rail_kind: nd_sim::RailFacility::Station,
            metro_draft: Default::default(),
            metro_hover: None,
            dock: None,
            favorites: Vec::new(),
            zone_mode: ZoneMode::Brush,
            brush_radius: 12.0,
            unzone: false,
            stroke: None,
            hover_parcel: None,
            hover_ground: None,
            rename: None,
            industry: "auto",
            roundabout: false,
            ring_radius: 20.0,
            ring_lanes: 1,
            ring_hover: None,
            ring_pin: None,
            elevation: 0.0,
            point_levels: Vec::new(),
            tunnel: false,
            depth: Depth::Ground,
            grade_warnings: Vec::new(),
            cursor_pin: None,
            pillars: Default::default(),
            junction_editor: None,
            plan_check: None,
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
    /// A vehicle on the road: a car, a truck or a bus (a car's driver is a
    /// click away in the inspector).
    Car(CarId),
    /// A private car standing parked (`world.vehicles`).
    Parked(nd_sim::VehicleId),
    /// [camera] A pavement delivery robot (`world.bots`).
    Bot(nd_sim::BotId),
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

/// Control points of different gestures closer than this are one joint.
const SAME_POINT: f32 = 0.5;

/// The control point under the cursor. Where several gestures share it (a
/// road attached to another's end), the one whose legs from it run nearest
/// the cursor wins, then the newest: clicking a ramp's end where it meets
/// a highway must pick the ramp, not the highway (picking the older road
/// there turned the whole highway into a ramp when the player then picked
/// the Ramp preset in the upgrade panel).
fn nearest_control_point(plan: &Plan, p: SVec2) -> Option<(GestureId, usize)> {
    let mut best: Option<(f32, f32, GestureId, usize)> = None;
    for (id, g) in &plan.gestures {
        let pts = g.points();
        for (i, &q) in pts.iter().enumerate() {
            let d = p.distance(q);
            if d >= POINT_PICK_RADIUS {
                continue;
            }
            let legs = [i.checked_sub(1), (i + 1 < pts.len()).then_some(i + 1)];
            let leg = legs.into_iter().flatten().map(|j| p.distance(project_on_segment(p, q, pts[j]).1)).fold(f32::INFINITY, f32::min);
            // Later gestures win ties: the map iterates them oldest first.
            let better = match best {
                None => true,
                Some((bd, bl, ..)) if (d - bd).abs() <= SAME_POINT => leg <= bl,
                Some((bd, ..)) => d < bd,
            };
            if better {
                best = Some((d, leg, *id, i));
            }
        }
    }
    best.map(|(_, _, id, i)| (id, i))
}

/// Gesture under the cursor: a road whose polyline is close, or a zone that
/// contains the point. Returns the segment index for roads so points can be
/// inserted there.
fn nearest_gesture(plan: &Plan, p: SVec2) -> Option<(GestureId, usize)> {
    let mut best: Option<(f32, GestureId, usize)> = None;
    for (id, g) in &plan.gestures {
        match g {
            Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind, .. } => {
                let hw = if *kind != RoadKind::Street { 4.0 } else { (*lanes_fwd + *lanes_bwd) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 + 3.0 };
                let built = g.road_polyline();
                for (i, w) in built.windows(2).enumerate() {
                    let (_, q) = project_on_segment(p, w[0], w[1]);
                    let d = p.distance(q);
                    // `<=`: at a joint the newer road (the one attached) wins.
                    if d < hw && best.map_or(true, |b| d <= b.0) {
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
        if shift && !tools.points.is_empty() {
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
    let finish = (map_key(KeyCode::Enter) && !ctrl) || right_click;
    let right_down = buttons.just_pressed(MouseButton::Right) && !flags.pointer_over_ui;
    let click = buttons.just_pressed(MouseButton::Left) && !shift && !flags.pointer_over_ui;
    let shift_click = buttons.just_pressed(MouseButton::Left) && shift && !flags.pointer_over_ui;
    let ground = cursor.ground;
    tools.hover_ground = ground;

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
                    tools.ring_radius = (tools.ring_radius + lines * (tools.ring_radius * 0.08).max(2.0)).clamp(nd_sim::roads::RING_MIN_RADIUS, nd_sim::roads::RING_MAX_RADIUS);
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
            if ground.is_some() && (click || buttons.just_pressed(MouseButton::Middle)) {
                tools.cursor_pin = None;
            }
            if click {
                if let Some(p) = ground {
                    let first = tools.points.is_empty();
                    let (p, snapped, level) = ToolState::road_snap_level(&sim, p, tools.kind, tools.elevation, first);
                    tools.push_point(p, level);
                    if snapped {
                        tools.status = format!("Snapped to the road at {}: a junction forms there.", elevation_label(level));
                    }
                }
            }
            if finish {
                if tools.points.len() >= 2 {
                    let kind = tools.kind;
                    let (lf, lb) = if kind != RoadKind::Street { (0, 0) } else { (tools.lanes_fwd, tools.lanes_bwd) };
                    let levels = tools.levels();
                    let elevation = if levels.iter().any(|l| *l != 0.0) { levels.clone() } else { Vec::new() };
                    let curved = tools.curved && tools.points.len() >= 3;
                    // A crossing with the railway that is neither at grade
                    // nor clear of it is refused, not built as a crossing
                    // the track would have to dip or climb to.
                    let built = if curved { nd_sim::plan::sample_curve(&tools.points) } else { tools.points.clone() };
                    let built_levels = if curved { nd_sim::plan::sample_curve_levels(&tools.points, &levels) } else { levels.clone() };
                    if let Some(why) = sim.world.road_refusal(&built, &built_levels, kind, lf, lb) {
                        tools.status = format!("Not added: {why}.");
                    } else {
                        let g = Gesture::Road { points: tools.points.clone(), lanes_fwd: lf, lanes_bwd: lb, curved, kind, elevation };
                        sim.world.add_gesture(g);
                        let warn = if tools.grade_warnings.is_empty() { String::new() } else { format!(" Warning: {}.", tools.grade_warnings.join("; ")) };
                        tools.status = format!("{} {} added to project ({} points). Ctrl+Enter implements.{warn}", if tools.curved { "Curved" } else { "Straight" }, match kind { RoadKind::Path => "path", RoadKind::Rail => "track", RoadKind::Street if tools.tunnel => "tunnel", RoadKind::Street => "road" }, tools.points.len());
                    }
                }
                tools.points.clear();
                tools.point_levels.clear();
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
                // [tools] A junction or a crosswalk opens the Intersection
                // editor (Ctrl+click still grabs a control point there).
                let target = ground.filter(|_| !ctrl).and_then(|p| crate::ui::junction::target_at(&sim.world, p));
                if let Some(at) = target {
                    tools.junction_editor = crate::ui::junction::EditorState::open(&sim.world, at);
                    tools.status = "Intersection editor: lanes, left turns, signals and crosswalks.".into();
                } else if let Some(hp) = tools.hover_point {
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
                        if g.points_mut().is_some() {
                            g.insert_point(at, p);
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
                            g.remove_point(idx);
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
        // [districts] input in `district_tool::district_input`
        Tool::District => {}
        Tool::Bulldoze => {
            tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);
            // [tolls] A click on a toll booth takes the booth away, not the road.
            if click {
                if let Some(booth) = ground.and_then(|p| sim.world.tolls.nearest(p, 8.0)).map(|b| (b.id, b.name.clone())) {
                    let _ = sim.world.remove_toll(booth.0);
                    tools.status = format!("{} removed.", booth.1);
                    return;
                }
            }
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
                    *selection = pick(&sim, p, tools.depth);
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
            TransitMode::Metro => {
                // [metro-ux] Enter finishes; a right-click takes back the last stop.
                let enter = map_key(KeyCode::Enter) && !ctrl;
                metro_click(&mut sim, &mut tools, &mut selection, ground, click, enter, right_click);
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
                    // [tolls] A toll booth goes across the road, not on a lot.
                    if let nd_sim::Placeable::Toll(size) = what {
                        tools.status = match sim.world.place_toll(p, size) {
                            Ok(_) => format!("{} built for {:.0}; {:.0}/day to run. Toll {:.1} a car (set it in the Taxes window).", what.label(), what.cost(), what.upkeep(), size.default_price()),
                            Err(e) => format!("Cannot build: {e}."),
                        };
                        return;
                    }
                    let before = sim.world.economy.treasury;
                    tools.status = match sim.world.place_building(what, p) {
                        Ok(id) => {
                            *selection = Selection::Building(id);
                            // [clarity] Solar and wind are sized to their lot;
                            // [plant-groups] the price is what was paid.
                            let paid = before - sim.world.economy.treasury;
                            match sim.world.buildings.get(id).filter(|b| b.plant.is_some()) {
                                Some(b) => {
                                    let mut s = format!("{} built: {:.1} MW for {:.0}; {:.0}/day to run.", what.label(), sim.world.plant_capacity_mw(b), paid, sim.world.plant_upkeep(b));
                                    if let Some(g) = sim.world.plant_group_of(id) {
                                        let mw: f32 = g.members.iter().filter_map(|m| sim.world.buildings.get(*m)).map(|m| sim.world.plant_capacity_mw(m)).sum();
                                        s += &format!(" Part of {}: {} lots, {:.0} MW.", g.name, g.members.len(), mw);
                                    }
                                    s
                                }
                                None => format!("{} built for {:.0}; {:.0}/day to run.", what.label(), paid, what.upkeep()),
                            }
                        }
                        Err(e) => format!("Cannot build: {e}."),
                    };
                }
            }
        }
    }
}

/// [subway] The Buses tool's Metro mode. [metro-ux] Lines are drawn station
/// to station: a click on a station (its lot, or near its entrance or
/// platform) adds it and the tunnel to it is previewed; the first station
/// again closes a loop; Enter (or the panel's Finish) lays the line; a
/// right-click or Backspace takes back the last stop; Esc drops the draft.
/// A click that misses every station does nothing but say so: stations are
/// placed with the Build window (Transit, Metro, Metro station).
#[allow(clippy::too_many_arguments)]
fn metro_click(sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, ground: Option<SVec2>, click: bool, finish: bool, undo: bool) {
    let w = &mut sim.world;
    tools.metro_draft.stations.retain(|s| w.metro_platform(*s).is_some());
    if tools.metro_draft.stations.len() < 3 {
        tools.metro_draft.looped = false;
    }
    tools.metro_hover = ground.and_then(|p| w.metro_station_at(p, crate::metro_ux::PICK_RADIUS));
    if click {
        if let Some(p) = ground {
            tools.status = match w.metro_station_at(p, crate::metro_ux::PICK_RADIUS) {
                Some(b) => {
                    let msg = crate::metro_ux::add_stop(w, &mut tools.metro_draft, b);
                    *selection = Selection::Building(b);
                    msg
                }
                None => crate::metro_ux::missed_hint(w),
            };
        }
    }
    if undo {
        tools.status = crate::metro_ux::undo_stop(w, &mut tools.metro_draft);
    }
    if finish {
        tools.status = crate::metro_ux::finish_line(w, &mut tools.metro_draft);
    }
}

/// What a click at `p` picks: the vehicle drawn under it (tested where it
/// is drawn this frame, not where the sim will have it at the end of the
/// tick), else a person in the open, else a parked car, else a building.
/// The underground view draws only what is below ground, so there a click
/// picks only a vehicle in a tunnel: nothing it could not see.
pub(crate) fn pick(sim: &Sim, p: SVec2, depth: Depth) -> Selection {
    let w = &sim.world;
    let f = sim.frac();
    let under = depth == Depth::Underground;
    // Vehicles on the road, by their drawn pose: a click on the body wins.
    let car = w
        .traffic
        .cars
        .iter()
        .map(|(id, car)| (car.pose_at(f).0, id, car))
        .filter(|(at, _, car)| !under || w.car_height_at(car, *at) < crate::render::UNDERGROUND_BELOW)
        .map(|(at, id, car)| (at.distance(p), id, car))
        .filter(|(d, _, car)| *d < if car.bus.is_some() { 8.0 } else { 4.5 })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if under {
        return car.map_or(Selection::None, |(_, id, _)| Selection::Car(id));
    }
    // People in the open (drivers are picked through their car).
    let person = w
        .citizens
        .iter()
        .filter(|(_, c)| c.is_visible() && !matches!(c.activity, Activity::Driving { .. }))
        .map(|(id, c)| (c.prev_pos.lerp(c.pos, f).distance(p), id))
        .filter(|(d, _)| *d < 6.0)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    // [camera] Pavement delivery robots: small, so a click must be close,
    // and nearer than any car or person it might be standing by.
    let bot = w
        .bots
        .bots
        .iter()
        .map(|(id, b)| ((if b.prev_pos.distance(b.pos) < 30.0 { b.prev_pos.lerp(b.pos, f) } else { b.pos }).distance(p), id))
        .filter(|(d, _)| *d < 2.0)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((db, id)) = bot {
        if car.is_none_or(|c| c.0 > db) && person.is_none_or(|q| q.0 > db) {
            return Selection::Bot(id);
        }
    }
    match (car, person) {
        (Some((dc, id, _)), Some((dp, _))) if dc <= dp + 1.0 => return Selection::Car(id),
        (Some((_, id, _)), None) => return Selection::Car(id),
        (_, Some((_, id))) => return Selection::Citizen(id),
        _ => {}
    }
    // Parked cars at the kerb, in lots and on driveways.
    let parked = w
        .vehicles
        .cars
        .iter()
        .filter_map(|(vid, c)| c.spot().map(|s| (s.pos.distance(p), vid)))
        .filter(|(d, _)| *d < 3.5)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, vid)) = parked {
        return Selection::Parked(vid);
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

#[allow(clippy::too_many_arguments)]
fn draw_gizmos(mut gizmos: Gizmos, tools: Res<ToolState>, cursor: Res<Cursor>, sim: Res<Sim>, selection: Res<Selection>, mut eplan: ResMut<EffectivePlan>, cams: Query<&OrbitCamera>, time: Res<Time>, (cam_mode, follow): (Res<crate::camera::CameraMode>, Res<crate::camera::CameraFollow>)) {
    // Re-sync so a point dragged by `tool_input` this frame draws where it is.
    let plan = eplan.sync(&sim.world);
    // [camera] Not from inside the followed vehicle: its outline box would
    // be drawn round the eye.
    if !follow.pov_in_vehicle(&sim, *cam_mode) {
        draw_selection(&mut gizmos, &sim, *selection);
    }
    let cyan = Color::linear_rgb(1.4, 1.6, 1.8);
    let red = Color::linear_rgb(3.0, 0.2, 0.4);
    let white = Color::linear_rgb(3.0, 3.0, 3.0);
    let lift = |p: SVec2| Vec3::new(p.x, 0.8 + sim.world.surface_height(p), p.y);
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);

    // [rage] Drivers who have lost their temper flash and lean on the horn:
    // a red ring blinking over the car, faster the angrier they are.
    {
        let f = sim.frac();
        let t = time.elapsed_secs();
        for (id, car) in sim.world.traffic.cars.iter().filter(|(_, c)| c.drive.anger >= 0.5) {
            let phase = (slotmap::Key::data(&id).as_ffi() % 97) as f32 * 0.13;
            if ((t * (3.0 + 5.0 * car.drive.anger) + phase) as i64) % 2 == 0 {
                let (p, _) = car.pose_at(f);
                gizmos.circle(Isometry3d::new(Vec3::new(p.x, sim.world.car_height(car) + 2.4, p.y), flat), 1.1 + car.drive.anger, Color::linear_rgb(4.0, 0.3, 0.15));
            }
        }
    }

    // Bus lines: the lane path of every leg in the line's colour, stops as
    // rings, broken legs red. With the Buses tool or the toggle every line
    // shows; otherwise only a selected one (picked in the Transit window,
    // the Buses list or by clicking one of its buses), drawn bright with
    // marker posts at its stops and a pulsing ring over each of its buses.
    // [metro-ux] Not while drawing a metro line: the Metro layer is about the metro.
    let metro_mode = tools.tool == Tool::Transit && tools.transit_mode == TransitMode::Metro;
    let all_lines = (tools.tool == Tool::Transit && !metro_mode) || tools.show_routes;
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
                // [lines] The stop picked in the Lines window: a pulsing
                // white ring and a post three times as tall.
                if tools.stop_sel == Some((id, i)) {
                    gizmos.circle(Isometry3d::new(at, flat), 5.0 + 2.0 * pulse, white);
                    gizmos.line(at, at + Vec3::Y * 28.0, white);
                    gizmos.circle(Isometry3d::new(at + Vec3::Y * 28.0, flat), 2.0, white);
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
        // [subway] Metro lines over the map in the Metro mode (or with
        // the toggle), and the line being laid up to the pointer.
        if tools.show_routes && !(tools.tool == Tool::Transit && tools.transit_mode == TransitMode::Metro) {
            for l in sim.world.subway.lines.values() {
                let c = nd_procgen::metro::line_colour(l.color);
                let col = Color::linear_rgb(c[0], c[1], c[2]);
                gizmos.linestrip(l.path.iter().map(|&p| lift(p) + Vec3::Y * 0.5), col);
                for &s in &l.stations {
                    if let Some(p) = sim.world.metro_platform(s) {
                        gizmos.circle(Isometry3d::new(lift(p) + Vec3::Y * 0.5, flat), 6.0, col);
                    }
                }
            }
        }
        // [metro-ux] The Metro mode's stations, lines and the line being
        // drawn are the Metro layer's (`metro_ux::metro_gizmos`).
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
        Tool::Road => {
            // The road itself is a hologram (render/holo.rs); here only the
            // control points, each at its level with a stalk to the ground.
            let at = |p: SVec2, level: f32| Vec3::new(p.x, sim.world.ground_height(p).max(if sim.world.landscape.is_flat() { 0.0 } else { nd_sim::landscape::WATER_LEVEL }) + level + 0.8, p.y);
            let levels = tools.levels();
            let mut ctrl: Vec<SVec2> = tools.points.clone();
            if let Some(c) = tools.cursor_pin.or(cursor.ground) {
                let (c, snapped, level) = ToolState::road_snap_level(&sim, c, tools.kind, tools.elevation, tools.points.is_empty());
                let top = at(c, level);
                gizmos.circle(Isometry3d::new(top, flat), if snapped { 4.5 } else { 3.0 }, if snapped { white } else { cyan });
                if level != 0.0 {
                    gizmos.line(at(c, 0.0), top, cyan);
                }
                ctrl.push(c);
            }
            for (i, &p) in tools.points.iter().enumerate() {
                let l = levels.get(i).copied().unwrap_or(0.0);
                gizmos.circle(Isometry3d::new(at(p, l), flat), 2.0, cyan);
                if l != 0.0 {
                    gizmos.line(at(p, 0.0), at(p, l), cyan);
                }
            }
            if tools.curved && ctrl.len() >= 3 {
                let mut l = levels.clone();
                l.resize(ctrl.len(), tools.elevation);
                gizmos.linestrip(ctrl.iter().zip(&l).map(|(&p, &l)| at(p, l)).collect::<Vec<_>>(), Color::linear_rgb(0.3, 0.4, 0.5));
            }
        }
        // The Road tool is drawn above (both of its arms cover every case).
        Tool::Zone => {
            let mut ctrl: Vec<SVec2> = tools.points.clone();
            if let Some(c) = cursor.ground {
                gizmos.circle(Isometry3d::new(lift(c), flat), 3.0, cyan);
                ctrl.push(c);
            }
            for &p in &tools.points {
                gizmos.circle(Isometry3d::new(lift(p), flat), 2.0, cyan);
            }
            let mut pts: Vec<Vec3> = ctrl.iter().map(|&p| lift(p)).collect();
            if pts.len() >= 2 {
                if pts.len() >= 3 {
                    pts.push(pts[0]);
                }
                gizmos.linestrip(pts, cyan);
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
            // [clarity] The selected road's lanes as big arrows, one per
            // lane in its direction of travel, so "Flip direction" is no
            // guesswork.
            if let Some(g) = tools.selected_gesture.and_then(|id| plan.gestures.get(&id)) {
                if let Gesture::Road { lanes_fwd, lanes_bwd, kind: RoadKind::Street, .. } = g {
                    lane_arrows(&mut gizmos, &g.road_polyline(), *lanes_fwd, *lanes_bwd, &lift);
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
        // [districts] drawn by `district_tool::district_gizmos`
        Tool::District => {}
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
    // Heights are above the ground there, so lines never sink into hills.
    let at = |p: SVec2, y: f32| Vec3::new(p.x, y + w.surface_height(p), p.y);
    // The same blend as the renderer, so the ring sits under the figure.
    let blend = |a: SVec2, b: SVec2| if a.distance(b) < 30.0 { a.lerp(b, f) } else { b };

    match selection {
        Selection::None => {}
        // [camera] drawn by `crate::marks`, with its route and its shop.
        Selection::Bot(_) => {}
        Selection::Building(b) => {
            lot_outline(gizmos, w, b, 1.6, gold);
            // [plant-groups] A Sunfield or Wind Park is selected whole.
            if let Some(g) = w.plant_group_of(b) {
                for m in g.members.into_iter().filter(|&m| m != b) {
                    lot_outline(gizmos, w, m, 1.6, gold);
                }
            }
            // [land] Everything its owner owns (a rich person's other homes
            // and businesses), outlined and tied to it by a line.
            let owned = w.owned_with(b);
            if owned.len() > 1 {
                let teal = Color::linear_rgb(0.4, 2.6, 2.2);
                let from = w.footprint(b).map(|l| l.center);
                for p in owned.into_iter().skip(1) {
                    lot_outline(gizmos, w, p, 1.3, teal);
                    if let (Some(a), Some(c)) = (from, w.footprint(p).map(|l| l.center)) {
                        gizmos.line(at(a, 6.0), at(c, 6.0), teal);
                    }
                }
            }
        }
        Selection::Car(id) => {
            let Some(car) = w.traffic.cars.get(id) else { return };
            let (p, heading) = car.pose_at(f);
            let width = if car.bus.is_some() || car.cargo.is_some() { 2.7 } else { 2.1 };
            vehicle_outline(gizmos, p, heading, car.length() + 0.6, width + 0.5, w.car_height(car) + 0.35, gold);
            car_route(gizmos, w, car, route);
            // [clarity] An "o" where it set off, an "x" where it is going.
            let origin = w.car_journey(id).and_then(|j| j.from.pos);
            route_ends(gizmos, w, origin, car_route_end(w, car));
            // [services] A wreck with its recovery truck on the way: the
            // truck's road to it, in amber, and a ring round the truck.
            if let Some(t) = w.driving.wrecks.iter().find(|x| x.car == id).and_then(|x| x.truck).and_then(|t| w.traffic.cars.get(t)) {
                let amber = Color::linear_rgb(3.0, 1.4, 0.1);
                car_route(gizmos, w, t, amber);
                let (tp, th) = t.pose_at(f);
                vehicle_outline(gizmos, tp, th, t.length() + 0.6, 2.6, w.car_height(t) + 0.35, amber);
            }
            // Where it is going: the cargo's buyer, or the driver's errand.
            let to = car.cargo.as_ref().map(|c| c.to).or_else(|| match w.citizens.get(car.citizen).map(|c| &c.activity) {
                Some(Activity::Driving { to, .. }) => *to,
                _ => None,
            });
            if let Some(b) = to {
                lot_outline(gizmos, w, b, 1.2, dest);
            }
            // Who it waits for, back to the head of the jam: a red line from
            // car to car and a box round the head (red for a car standing
            // for no reason the sim knows, orange otherwise).
            let chain = w.traffic.jam_chain(id);
            if chain.cars.len() >= 2 {
                let jam = Color::linear_rgb(3.0, 0.35, 0.25);
                let pts: Vec<Vec3> = chain.cars.iter().filter_map(|&c| w.traffic.cars.get(c)).map(|c| {
                    let p = c.pose_at(f).0;
                    Vec3::new(p.x, w.car_height(c) + 1.6, p.y)
                }).collect();
                gizmos.linestrip(pts, jam);
                for &c in &chain.cars[1..chain.cars.len() - 1] {
                    if let Some(x) = w.traffic.cars.get(c) {
                        let (p, h) = x.pose_at(f);
                        vehicle_outline(gizmos, p, h, x.length() + 0.3, 2.3, w.car_height(x) + 0.3, Color::linear_rgb(1.4, 0.25, 0.2));
                    }
                }
                if let Some(head) = chain.head().and_then(|h| w.traffic.cars.get(h).map(|x| (h, x))) {
                    let (p, h) = head.1.pose_at(f);
                    let bad = !w.traffic.stop_reason(head.0).is_good();
                    let col = if bad { Color::linear_rgb(4.0, 0.2, 0.2) } else { Color::linear_rgb(3.4, 1.4, 0.2) };
                    vehicle_outline(gizmos, p, h, head.1.length() + 1.0, 3.0, w.car_height(head.1) + 0.6, col);
                }
            }
        }
        Selection::Parked(vid) => {
            let Some(spot) = w.vehicles.cars.get(vid).and_then(|c| c.spot()) else { return };
            vehicle_outline(gizmos, spot.pos, spot.heading, 5.2, 2.6, w.surface_height(spot.pos) + 0.35, gold);
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
                        gizmos.linestrip(pts.iter().map(|p| *p + Vec3::Y * 0.4), route);
                        gizmos.linestrip(pts, route);
                    }
                    person_outline(gizmos, at(here, 0.0), gold);
                }
                Activity::Driving { car, to, .. } => {
                    destination = to.or(destination);
                    if let Some(car) = w.traffic.cars.get(*car) {
                        // At the wheel: the car's outline and its route, as a
                        // selected car shows.
                        let (p, heading) = car.pose_at(f);
                        let width = if car.bus.is_some() || car.cargo.is_some() { 2.7 } else { 2.1 };
                        vehicle_outline(gizmos, p, heading, car.length() + 0.6, width + 0.5, w.car_height(car) + 0.35, gold);
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
                // [passengers] In someone's car: the car and its route.
                Activity::Passenger { driver, to, seated, .. } => {
                    destination = to.or(destination);
                    let car = w.citizens.get(*driver).and_then(|d| match d.activity {
                        Activity::Driving { car, .. } => w.traffic.cars.get(car),
                        _ => None,
                    });
                    match car.filter(|_| *seated) {
                        Some(car) => {
                            let (p, heading) = car.pose_at(f);
                            vehicle_outline(gizmos, p, heading, car.length() + 0.6, 2.6, w.car_height(car) + 0.35, gold);
                            car_route(gizmos, w, car, route);
                        }
                        None => person_outline(gizmos, at(here, 0.0), gold),
                    }
                }
                // [ferries] Away off the map: nothing to draw.
                Activity::Abroad { .. } => {}
                Activity::Riding { bus } => {
                    if let Some(bus) = w.traffic.cars.get(*bus) {
                        let p = bus.pose_at(f).0;
                        gizmos.circle(Isometry3d::new(at(p, 0.5), flat), 7.5, gold);
                        car_route(gizmos, w, bus, route);
                    }
                }
                // [subway] Underground: a ring where the train is, its line ahead.
                Activity::OnMetro { train } => {
                    if let Some(t) = w.subway.trains.get(*train) {
                        let s = t.run.prev_s + (t.run.s - t.run.prev_s) * f;
                        let head = nd_sim::rail::point_along(&t.run.poly, s).0;
                        gizmos.circle(Isometry3d::new(at(head, 0.5), flat), 8.0, gold);
                        let alight = match c.transit.map(|t| t.leg) {
                            Some(nd_sim::transit::TransitLeg::Subway { alight, .. }) => Some(alight),
                            _ => None,
                        };
                        let s_end = alight.and_then(|b| t.run.stops.iter().find(|(stop, _)| *stop == b)).map_or_else(|| nd_sim::rail::path_length(&t.run.poly), |&(_, s1)| s1);
                        let pts: Vec<Vec3> = sub_polyline(&t.run.poly, s, s_end).iter().map(|&q| at(q, 1.0)).collect();
                        if pts.len() >= 2 {
                            gizmos.linestrip(pts, route);
                        }
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
                    person_outline(gizmos, at(*q, 0.0), gold);
                }
                Activity::Idle | Activity::Sitting { .. } => {
                    person_outline(gizmos, at(here, 0.0), gold);
                }
                Activity::AtHome | Activity::Working | Activity::Visiting(_) | Activity::Arriving => {
                    // Indoors: show the building they are in.
                    if let Some(b) = c.inside {
                        lot_outline(gizmos, w, b, 1.6, gold);
                    }
                }
            }
            // [clarity] An "o" where the trip began, an "x" at its end.
            let origin = c.trail.first().copied().or_else(|| w.citizen_origin(id).and_then(|p| p.pos));
            let end = match &c.activity {
                Activity::Traveling { path, .. } => path.last().copied(),
                Activity::Driving { car, .. } => w.traffic.cars.get(*car).and_then(|car| car_route_end(w, car)),
                _ => None,
            }
            .or_else(|| destination.and_then(|b| w.building_entrance(b)));
            if c.activity.is_traveling() || matches!(c.activity, Activity::Driving { .. } | Activity::Passenger { .. } | Activity::Riding { .. } | Activity::Waiting { .. }) {
                route_ends(gizmos, w, origin, end);
            }
            if let Some(b) = destination {
                lot_outline(gizmos, w, b, 1.2, dest);
                // A thin line from where they are to where they are going,
                // so the goal reads even when the route turns away from it.
                if let Some(goal) = w.buildings.get(b).and_then(|b| w.lots.get(b.lot)).map(|l| l.center) {
                    let faint = Color::linear_rgb(1.2, 0.4, 1.3);
                    gizmos.line(at(here, 3.0), at(goal, 3.0), faint);
                }
            }
        }
    }
}

/// [clarity] Big arrows along a road, one per lane, pointing the way its
/// traffic runs: lanes forward (along the drawing) on the right of the
/// line, lanes back on the left (right-hand traffic); a one-way road's
/// lanes centred on it. Repeated every 45 m or so, at least once.
fn lane_arrows(gizmos: &mut Gizmos, line: &[SVec2], fwd: u8, bwd: u8, lift: &dyn Fn(SVec2) -> Vec3) {
    if line.len() < 2 || fwd + bwd == 0 {
        return;
    }
    let lane_w = nd_sim::roads::LANE_WIDTH;
    let total: f32 = line.windows(2).map(|w| w[0].distance(w[1])).sum();
    let n = ((total / 45.0).floor() as usize).max(1);
    let fwd_col = Color::linear_rgb(0.4, 3.2, 1.0);
    let bwd_col = Color::linear_rgb(3.2, 1.2, 0.3);
    for k in 0..n {
        let s = total * (k as f32 + 0.5) / n as f32;
        let (p, d) = nd_sim::rail::point_along(line, s);
        let d = d.normalize_or_zero();
        if d == SVec2::ZERO {
            continue;
        }
        let right = SVec2::new(-d.y, d.x);
        let mut lanes: Vec<(f32, f32, Color)> = Vec::new();
        if bwd == 0 || fwd == 0 {
            let (m, sign, col) = if bwd == 0 { (fwd, 1.0, fwd_col) } else { (bwd, -1.0, bwd_col) };
            for i in 0..m {
                lanes.push(((i as f32 - (m as f32 - 1.0) * 0.5) * lane_w, sign, col));
            }
        } else {
            for i in 0..fwd {
                lanes.push(((i as f32 + 0.5) * lane_w, 1.0, fwd_col));
            }
            for i in 0..bwd {
                lanes.push((-(i as f32 + 0.5) * lane_w, -1.0, bwd_col));
            }
        }
        for (off, sign, col) in lanes {
            let c = p + right * off;
            let dir = d * sign;
            let tail = c - dir * 5.0;
            let tip = c + dir * 5.0;
            let side = SVec2::new(-dir.y, dir.x);
            let up = Vec3::Y * 1.4;
            for y in [up, up + Vec3::Y * 0.25] {
                gizmos.line(lift(tail) + y, lift(tip) + y, col);
                gizmos.line(lift(tip) + y, lift(tip - dir * 2.6 + side * 1.4) + y, col);
                gizmos.line(lift(tip) + y, lift(tip - dir * 2.6 - side * 1.4) + y, col);
            }
        }
    }
}

/// An upright box round a person standing at `base` (ground level).
fn person_outline(gizmos: &mut Gizmos, base: Vec3, col: Color) {
    let (r, h) = (0.7, 2.2);
    let ring = |y: f32| [Vec3::new(-r, y, -r), Vec3::new(r, y, -r), Vec3::new(r, y, r), Vec3::new(-r, y, r), Vec3::new(-r, y, -r)].map(|v| v + base);
    gizmos.linestrip(ring(0.05), col);
    gizmos.linestrip(ring(h), col);
    for (x, z) in [(-r, -r), (r, -r), (r, r), (-r, r)] {
        gizmos.line(base + Vec3::new(x, 0.05, z), base + Vec3::new(x, h, z), col);
    }
}

/// A rectangle round a vehicle's body, a little above the road, drawn twice
/// so it reads as a box.
pub(crate) fn vehicle_outline(gizmos: &mut Gizmos, p: SVec2, heading: f32, length: f32, width: f32, y: f32, col: Color) {
    let fwd = SVec2::new(heading.cos(), heading.sin());
    let right = SVec2::new(-fwd.y, fwd.x);
    let (hl, hw) = (length * 0.5, width * 0.5);
    let corner = |a: f32, b: f32| {
        let q = p + fwd * (hl * a) + right * (hw * b);
        Vec3::new(q.x, y, q.y)
    };
    let pts = [corner(1.0, 1.0), corner(1.0, -1.0), corner(-1.0, -1.0), corner(-1.0, 1.0), corner(1.0, 1.0)];
    gizmos.linestrip(pts, col);
    gizmos.linestrip(pts.iter().map(|v| *v + Vec3::Y * 1.4), col);
}

pub(crate) fn lot_outline(gizmos: &mut Gizmos, w: &nd_sim::World, b: BuildingId, y: f32, col: Color) {
    // [land] the whole footprint of a building over merged plots
    let Some(lot) = w.footprint(b) else { return };
    let mut pts: Vec<Vec3> = lot.polygon.iter().map(|&p| Vec3::new(p.x, y, p.y)).collect();
    if pts.len() >= 3 {
        pts.push(pts[0]);
        gizmos.linestrip(pts, col);
    }
}

/// [clarity] The ends of a selected route: a small "o" (a ring) where the
/// trip began and an "x" where it ends, standing a little above the road
/// so they read from any height.
fn route_ends(gizmos: &mut Gizmos, w: &nd_sim::World, origin: Option<SVec2>, end: Option<SVec2>) {
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let at = |p: SVec2, y: f32| Vec3::new(p.x, y + w.surface_height(p), p.y);
    if let Some(o) = origin {
        let col = Color::linear_rgb(0.6, 3.0, 1.2);
        for y in [1.2, 1.5] {
            gizmos.circle(Isometry3d::new(at(o, y), flat), 3.0, col);
        }
        gizmos.line(at(o, 0.1), at(o, 1.2), col);
    }
    if let Some(e) = end {
        let col = Color::linear_rgb(3.2, 0.6, 2.4);
        let r = 3.2;
        for y in [1.2, 1.5] {
            gizmos.line(at(e + SVec2::new(-r, -r), y), at(e + SVec2::new(r, r), y), col);
            gizmos.line(at(e + SVec2::new(-r, r), y), at(e + SVec2::new(r, -r), y), col);
        }
        gizmos.line(at(e, 0.1), at(e, 1.2), col);
    }
}

/// Where a car's lane route ends.
fn car_route_end(w: &nd_sim::World, car: &nd_sim::Car) -> Option<SVec2> {
    let net = &w.traffic.net;
    if car.idx >= car.route.len() || car.route.iter().any(|&l| l as usize >= net.lanes.len()) {
        return None;
    }
    net.polyline(&car.route[car.idx..], car.s, car.s_end).last().copied()
}

/// The rest of a car's lane route, from where it is to where it stops.
fn car_route(gizmos: &mut Gizmos, w: &nd_sim::World, car: &nd_sim::Car, col: Color) {
    let net = &w.traffic.net;
    // Routes outlive lane rebuilds for a tick; never index past the lanes.
    if car.idx >= car.route.len() || car.route.iter().any(|&l| l as usize >= net.lanes.len()) {
        return;
    }
    let pts: Vec<Vec3> = net.polyline(&car.route[car.idx..], car.s, car.s_end).iter().map(|&q| Vec3::new(q.x, 1.0 + w.surface_height(q), q.y)).collect();
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
    // [tolls] The booth line across the road where it would stand.
    if let nd_sim::Placeable::Toll(size) = what {
        match nd_sim::tolls::site(&w.roads, c, size) {
            Ok((sid, at, dir)) => {
                let hw = w.roads.segment(sid).half_width() + if size == nd_sim::tolls::TollSize::Plaza { 8.0 } else { 0.5 };
                let right = SVec2::new(-dir.y, dir.x);
                let y = |p: SVec2| Vec3::new(p.x, 1.0 + w.surface_height(p), p.y);
                let near = w.tolls.booths.iter().any(|b| b.pos.distance(at) < nd_sim::tolls::SPACING_M);
                let col = if near || w.economy.treasury < size.cost() { red } else { green };
                gizmos.line(y(at - right * hw), y(at + right * hw), col);
                for u in [-4.0f32, 4.0] {
                    gizmos.line(y(at + dir * u - right * hw), y(at + dir * u + right * hw), col.with_alpha(0.5));
                }
                let flare = size.flare().max(10.0);
                gizmos.line(y(at - dir * flare), y(at + dir * flare), col.with_alpha(0.4));
            }
            Err(_) => {
                gizmos.circle(Isometry3d::new(lift(c), flat), 6.0, red);
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

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn road(points: Vec<SVec2>, f: u8, b: u8) -> Gesture {
        Gesture::Road { points, lanes_fwd: f, lanes_bwd: b, curved: false, kind: RoadKind::Street, elevation: Vec::new() }
    }

    /// A ramp drawn onto a highway's start shares its control point. A click
    /// there, or on the ramp beside it, picks the ramp: picking the older
    /// highway turned the whole of it into a ramp when the player then set
    /// the Ramp preset in the upgrade panel.
    #[test]
    fn a_click_at_a_joint_picks_the_road_attached_there() {
        let mut plan = Plan::default();
        let start = SVec2::new(-400.0, 0.0);
        plan.gestures.insert(GestureId(1), road(vec![start, SVec2::new(-150.0, 10.0), SVec2::new(100.0, 0.0)], 4, 0));
        plan.gestures.insert(GestureId(2), road(vec![SVec2::new(-600.0, 120.0), SVec2::new(-480.0, 40.0), start], 1, 0));
        // Exactly on the joint: the newer road.
        assert_eq!(nearest_control_point(&plan, start), Some((GestureId(2), 2)));
        assert_eq!(nearest_gesture(&plan, start).map(|g| g.0), Some(GestureId(2)));
        // A little up the ramp: the ramp.
        let on_ramp = start + (SVec2::new(-480.0, 40.0) - start).normalize() * 3.0;
        assert_eq!(nearest_control_point(&plan, on_ramp), Some((GestureId(2), 2)));
        assert_eq!(nearest_gesture(&plan, on_ramp).map(|g| g.0), Some(GestureId(2)));
        // A little along the highway: the highway.
        let on_highway = start + (SVec2::new(-150.0, 10.0) - start).normalize() * 3.0;
        assert_eq!(nearest_control_point(&plan, on_highway), Some((GestureId(1), 0)));
        assert_eq!(nearest_gesture(&plan, on_highway).map(|g| g.0), Some(GestureId(1)));
    }
}
