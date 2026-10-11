//! [`ToolState`]: the active tool and everything it is in the middle of.

use bevy::prelude::*;
use nd_sim::roads::RoadKind;
use nd_sim::{BuildingId, CitizenId, GestureId, LandUse, Vec2 as SVec2};

use super::{Depth, PaintStroke, Tool, TransitMode, ZoneMode, ELEVATION_MAX, ELEVATION_MIN, ELEVATION_STEP, POINT_PICK_RADIUS};
use crate::sim::Sim;

#[derive(Resource)]
pub struct ToolState {
    pub tool: Tool,
    pub lanes_fwd: u8,
    pub lanes_bwd: u8,
    pub land_use: LandUse,
    pub points: Vec<SVec2>,
    pub status: String,
    pub drag: Option<(GestureId, usize)>,
    /// [fix/review-app] The current drag has moved its point (and so has
    /// its own undo step, see [`drag_frame`]).
    pub drag_moved: bool,
    pub hover_point: Option<(GestureId, usize)>,
    pub hover_gesture: Option<GestureId>,
    /// [free-placement] Bulldoze tool: the city building under the pointer
    /// (a click bulldozes it).
    pub hover_city_building: Option<nd_sim::BuildingId>,
    /// [pt-ux] Bulldoze tool: the piece of the hovered road a click takes
    /// (junction to junction), as metres along its built polyline.
    pub hover_piece: Option<(GestureId, f32, f32)>,
    /// [dirty-industry] Bulldoze tool: clean up contaminated ground
    /// instead of removing things, over a circle of `cleanup_radius` metres.
    pub cleanup: bool,
    pub cleanup_radius: f32,
    /// [free-placement] Buildings tool: what a click here would do (true:
    /// it can go down) and the site's outline.
    pub build_here: Option<(bool, String, Vec<SVec2>)>,
    /// [free-placement] The last site worked out, kept while the pointer
    /// stays on its parcel: (item, parcel, parcels version, lots, site).
    pub build_site: Option<(nd_sim::Placeable, usize, u64, usize, Result<nd_sim::PlacementSite, String>)>,
    /// [plant-scale] The verdict on that site (with a plant's figures),
    /// kept while nothing it depends on changes: (key, ok, text).
    pub build_verdict: Option<(u64, bool, String)>,
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
    /// [lines] The bus line being edited (the Lines window's "Edit line"):
    /// map clicks add, insert, share, drag and remove its stops.
    pub line_edit: Option<super::line_edit::LineEdit>,
    /// [lines] The metro line opened in the Lines window.
    pub metro_sel: Option<nd_sim::subway::MetroLineId>,
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
    /// People starred in the person pane; listed in City Data, People, Favourites.
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
    /// [waterfront] Zone tool, parks: paint public beaches (parcels that
    /// run down to the water) instead of green parks.
    pub beach: bool,
    /// Road tool: stamp roundabouts instead of drawing roads.
    pub roundabout: bool,
    /// [interchange] Road tool, Interchange preset: two clicks across the
    /// highway lay a diamond interchange (`World::plan_diamond`).
    pub interchange: bool,
    /// [interchange] Why the diamond under the pointer would be refused, and
    /// where (refreshed with its hologram).
    pub interchange_refusal: Option<(String, SVec2)>,
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
    /// [pt-ux] Edit tool: the junction (or crosswalk) under the pointer,
    /// offered with an "Edit intersection" button (`ui::junction::hover_button`).
    pub junction_hover: Option<SVec2>,
    /// [fix/roads-tools] Edit tool: the junction under the pointer (every
    /// road meeting there moves with it).
    pub hover_joint: Option<nd_sim::Joint>,
    /// [fix/roads-tools] Edit tool: a junction being dragged.
    pub joint_drag: Option<JointDrag>,
    /// [fix/roads-tools] Road tool: draw a circle (centre, then radius).
    pub circle: bool,
    /// [tools] What implementing the active project would refuse on,
    /// refreshed with the plan view (None: nothing).
    pub plan_check: Option<nd_sim::ProjectCheck>,
}

/// [fix/roads-tools] A junction being dragged in the Edit tool: the roads
/// meeting there as they were when it was grabbed (a point already put in
/// those running through), the control points that move, and whether the
/// drag has moved them yet (its own undo step).
#[derive(Clone, Debug)]
pub struct JointDrag {
    pub base: std::collections::BTreeMap<GestureId, nd_sim::Gesture>,
    pub moving: Vec<(GestureId, usize)>,
    pub moved: bool,
}

impl JointDrag {
    pub fn of(joint: &nd_sim::Joint, plan: &nd_sim::Plan) -> Self {
        let (base, moving) = joint.handles(plan);
        Self { base, moving, moved: false }
    }

    /// The roads with the junction at `p`.
    pub fn at(&self, p: SVec2) -> Vec<(GestureId, nd_sim::Gesture)> {
        let mut out = self.base.clone();
        for &(id, i) in &self.moving {
            if let Some(g) = out.get_mut(&id) {
                g.move_point(i, p);
            }
        }
        out.into_iter().collect()
    }

    /// How many roads move with it.
    pub fn roads(&self) -> usize {
        self.base.len()
    }
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
        // [park-paths] A path snaps onto a park's (or other grounds') gate
        // when one is nearer than any road; streets never do.
        if let Some(j) = Self::grounds_snap(sim, p, kind) {
            return (j, true, 0.0);
        }
        let roads = &sim.world.roads;
        // [fix/roads-tools] A path dropped by a street ends on its
        // pavement (G12), not on the street's centre or junction node.
        if kind == RoadKind::Path {
            if let Some((q, at)) = roads.pavement_snap(p, POINT_PICK_RADIUS) {
                if any || (at - level).abs() < nd_sim::roads::GRADE_SEPARATION {
                    return (q, true, at);
                }
            }
        }
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

    /// [park-paths] The grounds gate a path point at `p` snaps onto: the
    /// nearest within the pick radius, unless a road is nearer.
    pub fn grounds_snap(sim: &Sim, p: SVec2, kind: RoadKind) -> Option<SVec2> {
        if kind != RoadKind::Path {
            return None;
        }
        let j = sim.world.grounds_junction_near(p, POINT_PICK_RADIUS)?;
        let (q, on_road) = sim.world.roads.snap_point_kind(p, POINT_PICK_RADIUS, kind);
        (!on_road || q.distance(p) > j.pos.distance(p)).then_some(j.pos)
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
        if label.eq_ignore_ascii_case("interchange") {
            // [interchange] A diamond: the cross street is a two-way street
            // of the lanes set (a Street unless two-way lanes are set).
            self.roundabout = false;
            self.interchange = true;
            self.kind = RoadKind::Street;
            if self.lanes_fwd == 0 || self.lanes_bwd == 0 {
                self.lanes_fwd = 2;
                self.lanes_bwd = 2;
            }
            if self.tunnel {
                self.tunnel = false;
            }
            self.elevation = 0.0;
            self.points.clear();
            self.point_levels.clear();
            if self.depth == Depth::Underground {
                self.depth = Depth::Ground;
            }
            return true;
        }
        self.interchange = false;
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
    /// [fix/review-app] A different city starts (`city_reset`): drop
    /// everything that names the old city's ids (the drawing, drags,
    /// hovers, the route and stop being edited, favourites, the district
    /// polygon, the junction editor) and keep the player's choices: the
    /// tool, road kind and lanes, zone and brush, the Build pick, transit
    /// and rail modes, elevation, and the status line (the replacing
    /// command has just written it).
    pub fn forget_city(&mut self) {
        let fresh = ToolState {
            tool: self.tool,
            lanes_fwd: self.lanes_fwd,
            lanes_bwd: self.lanes_bwd,
            land_use: self.land_use,
            status: std::mem::take(&mut self.status),
            cleanup: self.cleanup,
            cleanup_radius: self.cleanup_radius,
            curved: self.curved,
            circle: self.circle,
            kind: self.kind,
            build_cat: self.build_cat,
            build: self.build,
            build_tier: self.build_tier,
            transit_mode: self.transit_mode,
            show_routes: self.show_routes,
            rail_kind: self.rail_kind,
            dock: self.dock,
            zone_mode: self.zone_mode,
            brush_radius: self.brush_radius,
            unzone: self.unzone,
            industry: self.industry,
            beach: self.beach,
            roundabout: self.roundabout,
            interchange: self.interchange,
            ring_radius: self.ring_radius,
            ring_lanes: self.ring_lanes,
            elevation: self.elevation,
            tunnel: self.tunnel,
            depth: self.depth,
            ..ToolState::default()
        };
        *self = fresh;
    }

    /// [fix/roads-tools] Draw straight, curved or a circle.
    pub fn set_draw(&mut self, m: super::DrawMethod) {
        if self.circle != (m == super::DrawMethod::Circle) {
            self.points.clear();
            self.point_levels.clear();
        }
        self.curved = m == super::DrawMethod::Curved;
        self.circle = m == super::DrawMethod::Circle;
    }

    pub fn draw_method(&self) -> super::DrawMethod {
        if self.circle {
            super::DrawMethod::Circle
        } else if self.curved {
            super::DrawMethod::Curved
        } else {
            super::DrawMethod::Straight
        }
    }

    /// [fix/roads-tools] The road type being drawn (one of `ROAD_TYPES`):
    /// a two-way street, a one-way road too narrow for a highway (a ramp
    /// at one lane), or a highway (one-way, three lanes or more).
    pub fn road_type(&self) -> &'static str {
        match self.kind {
            RoadKind::Path => "Path",
            RoadKind::Rail => "Rail",
            RoadKind::Street if self.lanes_fwd > 0 && self.lanes_bwd > 0 => "Street",
            RoadKind::Street if nd_sim::roads::is_highway_spec(RoadKind::Street, self.lanes_fwd, self.lanes_bwd) => "Highway",
            RoadKind::Street => "One-way",
        }
    }

    /// [fix/roads-tools] Pick a road type: its usual lanes, unless the
    /// lanes set already suit it. The level is kept (a tunnel is any road
    /// deep enough). False for a name that is no type.
    pub fn pick_type(&mut self, label: &str) -> bool {
        let Some(&ty) = super::ROAD_TYPES.iter().find(|t| t.eq_ignore_ascii_case(label)) else { return false };
        self.roundabout = false;
        self.interchange = false;
        self.tunnel = false;
        let was = self.road_type();
        match ty {
            "Path" => self.kind = RoadKind::Path,
            _ => {
                self.kind = RoadKind::Street;
                if was != ty {
                    let (f, b) = match ty {
                        "Street" => (2, 2),
                        "One-way" => (1, 0),
                        _ => (4, 0),
                    };
                    self.lanes_fwd = f;
                    self.lanes_bwd = b;
                }
            }
        }
        true
    }

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
            drag_moved: false,
            right_press: None,
            hover_point: None,
            hover_gesture: None,
            hover_city_building: None,
            hover_piece: None,
            cleanup: false,
            cleanup_radius: 80.0,
            build_here: None,
            build_site: None,
            build_verdict: None,
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
            line_edit: None,
            metro_sel: None,
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
            beach: false,
            roundabout: false,
            interchange: false,
            interchange_refusal: None,
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
            junction_hover: None,
            hover_joint: None,
            joint_drag: None,
            circle: false,
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
        // [fix/review-app] The District brush erases with a right-drag too.
        (self.tool == Tool::Zone && matches!(self.zone_mode, ZoneMode::Brush | ZoneMode::Marquee)) || (self.tool == Tool::District && self.district.mode == crate::district_tool::DistrictMode::Brush)
    }

    /// The land use a paint stroke gives: the selected one, or none when
    /// unzoning.
    pub fn paint_use(&self, erase: bool) -> Option<LandUse> {
        if erase || self.unzone { None } else { Some(self.land_use) }
    }

    /// The industry sub-zone painting industrial land records.
    pub fn paint_industry(&self) -> Option<&'static str> {
        if self.unzone {
            return None;
        }
        match self.land_use {
            LandUse::Industrial => Some(self.industry),
            // [waterfront] a park painted as a public beach
            LandUse::Park if self.beach => Some(nd_sim::shore::BEACH_SUBZONE),
            _ => None,
        }
    }

    /// [waterfront] Painting public beaches.
    pub fn painting_beach(&self) -> bool {
        self.land_use == LandUse::Park && self.beach && !self.unzone
    }
}
