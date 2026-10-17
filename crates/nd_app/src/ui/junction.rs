//! [tools] The Intersection editor. [pt-ux] Opened from the "Edit
//! intersection" button that pops up beside a junction (or crosswalk)
//! under the pointer with the Edit tool ([`hover_button`]), so a click on
//! the junction itself still edits the roads there. [r1-roads] (K20) The
//! editing is done on the road itself: a box round the junction's area is
//! drawn on the map ([`on_map`]), time stands still until the editor is
//! closed ([`hold_time`]), the other tools leave the map alone meanwhile,
//! and on the roads in the box (it used to be a modal over a top-down
//! render of them):
//!
//! - an arrow at the stop line of every arriving lane, showing where the
//!   lane may go (left, straight, right). Hovering it draws the lane's
//!   moves through the junction (bright: allowed, faint: offered but not
//!   allowed); clicking it opens a small menu of the lane's options, and
//!   the street's automatic arrows and banned left;
//! - a dot on each crosswalk: click it for its upgrade (push-button signal
//!   or footbridge).
//!
//! Beside the box, a compact panel: signals on or off, the protected left
//! turn as a toggle, left turns allowed, the signal timing folded away, the
//! picked crosswalk, Reset and Done.
//!
//! Everything goes through `nd_sim` (`World::set_junction_rule`,
//! `set_lane_permissions`, `set_crossing_upgrade`), saved with the city;
//! the remote API's `intersection` command does the same.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_sim::junctions::{CrossingUpgrade, JunctionRule, LeftArrow, SignalMode, ARROW_MAX_SECONDS, ARROW_MIN_SECONDS, GREEN_MAX, GREEN_MIN};
use nd_sim::traffic::{arrows_label, LEFT, RIGHT, STRAIGHT};
use nd_sim::{IntersectionView, Vec2 as SVec2, World};

use super::{UiCtx, WARN};

/// How large the box round the junction may be (metres across).
pub const EXTENT_MIN: f32 = 30.0;
pub const EXTENT_MAX: f32 = 300.0;
/// [r1-roads] (K2) Points round a crosswalk's dot that pick the dot
/// before any lane arrow under it.
const DOT_PICK: f32 = 11.0;

const CYAN: egui::Color32 = egui::Color32::from_rgb(120, 230, 255);
const FAINT: egui::Color32 = egui::Color32::from_rgb(170, 180, 200);

/// The open editor.
#[derive(Clone, Debug)]
pub struct EditorState {
    /// The junction (or lone crosswalk) edited.
    pub at: SVec2,
    /// Metres across the box round it on the map.
    pub extent: f32,
    /// [r1-roads] The camera has been brought over the junction.
    pub framed: bool,
    /// The approach (index into the view's approaches) last picked.
    pub approach: Option<usize>,
    /// The crosswalk (walk graph index) shown in detail.
    pub crossing: Option<u32>,
    /// The last edit's outcome.
    pub message: Option<(String, bool)>,
    /// [pt-ux] The lane whose options menu is open: (approach, lane).
    pub lane_menu: Option<(usize, usize)>,
    /// [pt-ux] A lane shown as hovered (the remote API's pin; the real
    /// pointer wins when it is over a lane).
    pub hover_lane: Option<(usize, usize)>,
}

impl EditorState {
    /// Open on whatever is at `p` (a junction, else a crosswalk).
    pub fn open(w: &World, p: SVec2) -> Option<Self> {
        let v = w.intersection_view(p)?;
        let extent = (v.radius * 2.0 + 36.0).clamp(48.0, 120.0);
        Some(Self { at: v.at, extent, framed: false, approach: None, crossing: v.crossings.first().filter(|_| v.node.is_none()).map(|c| c.index), message: None, lane_menu: None, hover_lane: None })
    }

    /// [r1-roads] The corners of the box round the junction on the ground.
    pub fn box_corners(&self) -> [SVec2; 4] {
        let h = self.extent * 0.5;
        [(-h, -h), (h, -h), (h, h), (-h, h)].map(|(x, y)| self.at + SVec2::new(x, y))
    }
}

/// [r1-roads] (K20) Where points of the ground by the junction land on the
/// screen (egui points), through the map's camera, at the road's height.
pub struct Screen<'a> {
    camera: &'a Camera,
    gt: &'a GlobalTransform,
    z: f32,
    zoom: f32,
}

impl Screen<'_> {
    /// The screen point of ground point `p` (far off screen when behind
    /// the camera).
    pub fn at(&self, p: SVec2) -> egui::Pos2 {
        match self.camera.world_to_viewport(self.gt, Vec3::new(p.x, self.z, p.y)) {
            Ok(v) => egui::pos2(v.x / self.zoom, v.y / self.zoom),
            Err(_) => egui::pos2(-1.0e5, -1.0e5),
        }
    }

    /// Screen points per metre on the ground round `p`.
    fn scale(&self, p: SVec2) -> f32 {
        let o = self.at(p);
        ((self.at(p + SVec2::X) - o).length() + (self.at(p + SVec2::Y) - o).length()) * 0.5
    }
}

/// What the Edit tool offers to open under the pointer: a junction or a
/// crosswalk.
pub fn target_at(w: &World, p: SVec2) -> Option<SVec2> {
    if w.crosswalk_at(p, 2.5).is_some() || w.junction_at(p, 0.0).is_some() {
        w.intersection_view(p).map(|v| v.at)
    } else {
        None
    }
}

/// Where a street arrives from, by the way it leaves the junction (north
/// is up on the view: -y).
pub fn compass(dir: SVec2) -> &'static str {
    let a = dir.y.atan2(dir.x).to_degrees();
    match a {
        a if (-22.5..22.5).contains(&a) => "east",
        a if (22.5..67.5).contains(&a) => "south-east",
        a if (67.5..112.5).contains(&a) => "south",
        a if (112.5..157.5).contains(&a) => "south-west",
        a if (-67.5..-22.5).contains(&a) => "north-east",
        a if (-112.5..-67.5).contains(&a) => "north",
        a if (-157.5..-112.5).contains(&a) => "north-west",
        _ => "west",
    }
}

// ---- Lane options (pure) ----

/// [pt-ux] The arrows a lane may be given where the junction offers
/// `exits`, in menu order: single moves, then pairs, then all. Left and
/// right without straight on only where there is no straight on.
pub fn lane_choices(exits: u8) -> Vec<u8> {
    [LEFT, STRAIGHT, RIGHT, LEFT | STRAIGHT, STRAIGHT | RIGHT, LEFT | RIGHT, LEFT | STRAIGHT | RIGHT]
        .into_iter()
        .filter(|&c| c & !exits == 0)
        .filter(|&c| c != LEFT | RIGHT || exits & STRAIGHT == 0)
        .collect()
}

/// [pt-ux] The choice after `cur` in [`lane_choices`] (round to the first).
pub fn next_choice(cur: u8, exits: u8) -> u8 {
    let c = lane_choices(exits);
    match c.iter().position(|&x| x == cur) {
        Some(i) => c[(i + 1) % c.len()],
        None => c.first().copied().unwrap_or(exits),
    }
}

/// [pt-ux] An approach's arrows with lane `lane` set to `want`, the others
/// fitted so that every move the junction offers is still reachable from
/// some lane (the sim refuses arrows that strand an exit): a move no lane
/// has any more goes back on the lane nearest its side (left on the
/// leftmost, right on the rightmost, straight on the middle), not the lane
/// just set unless it is the only one.
pub fn fit_lanes(arrows: &[u8], lane: usize, want: u8, exits: u8) -> Vec<u8> {
    let n = arrows.len();
    let mut a: Vec<u8> = arrows.iter().map(|x| if x & exits == 0 { exits } else { x & exits }).collect();
    if lane >= n {
        return a;
    }
    a[lane] = if want & exits == 0 { exits } else { want & exits };
    for bit in [LEFT, STRAIGHT, RIGHT] {
        if exits & bit == 0 || a.iter().any(|x| x & bit != 0) {
            continue;
        }
        let order: Vec<usize> = match bit {
            LEFT => (0..n).collect(),
            RIGHT => (0..n).rev().collect(),
            _ => {
                let mid = (n as f32 - 1.0) * 0.5;
                let mut o: Vec<usize> = (0..n).collect();
                o.sort_by(|x, y| (*x as f32 - mid).abs().total_cmp(&(*y as f32 - mid).abs()));
                o
            }
        };
        let pick = order.iter().copied().find(|&i| i != lane).unwrap_or(lane);
        a[pick] |= bit;
    }
    a
}

/// "Left only", "Straight or right", ...
pub fn choice_label(a: u8) -> &'static str {
    match a {
        x if x == LEFT => "Left only",
        x if x == STRAIGHT => "Straight only",
        x if x == RIGHT => "Right only",
        x if x == LEFT | STRAIGHT => "Left or straight",
        x if x == STRAIGHT | RIGHT => "Straight or right",
        x if x == LEFT | RIGHT => "Left or right",
        x if x == LEFT | STRAIGHT | RIGHT => "Any way",
        _ => "No way",
    }
}

// ---- The hover button on the map ----

/// [pt-ux] The "Edit intersection" button beside the junction under the
/// pointer (Edit tool). It stays a moment after the pointer leaves the
/// junction, and while the pointer is on it, so it can be reached.
#[allow(clippy::too_many_arguments)]
pub fn hover_button(
    mut contexts: EguiContexts,
    sim: Res<crate::sim::Sim>,
    mut tools: ResMut<crate::tools::ToolState>,
    flags: Res<super::UiFlags>,
    cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>,
    time: Res<Time>,
    mut shown: Local<Option<(SVec2, f64)>>,
    mut over: Local<bool>,
) -> Result {
    let now = time.elapsed_secs_f64();
    if tools.tool != crate::tools::Tool::Edit || tools.junction_editor.is_some() || tools.drag.is_some() || flags.data_view {
        *shown = None;
        *over = false;
        return Ok(());
    }
    match (tools.junction_hover, *shown) {
        (Some(at), _) => *shown = Some((at, now)),
        (None, Some((at, _))) if *over => *shown = Some((at, now)),
        (None, Some((_, seen))) if now - seen > 0.9 => *shown = None,
        _ => {}
    }
    let Some((at, _)) = *shown else { return Ok(()) };
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let w = &sim.world;
    let Ok(v) = camera.world_to_viewport(gt, Vec3::new(at.x, w.surface_height(at) + 2.0, at.y)) else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let screen = crate::ui::theme::to_points(ctx, v);
    let mut open = false;
    let area = egui::Area::new(egui::Id::new("junction_offer")).order(egui::Order::Foreground).fixed_pos(screen + egui::vec2(18.0, -58.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            let b = ui.add(egui::Button::new(egui::RichText::new("\u{2b9e}  Edit intersection").color(CYAN).strong()));
            if b.clicked() {
                open = true;
            }
            ui.label(egui::RichText::new("lane arrows, signals, crosswalks").small().weak());
        });
    });
    *over = ctx.pointer_latest_pos().is_some_and(|p| area.response.rect.expand(10.0).contains(p));
    // A line from the button down to the junction.
    ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("junction_offer_line"))).line_segment([screen, area.response.rect.left_bottom()], egui::Stroke::new(1.5, CYAN.gamma_multiply(0.7)));
    if open {
        tools.junction_editor = EditorState::open(w, at);
        tools.status = "Intersection editor: click a lane's arrow to change where it may go.".into();
        *shown = None;
    }
    Ok(())
}

// ---- The panel ----

/// The editor's panel, when open: the controls beside the box on the map
/// (the arrows and dots are on the roads, [`on_map`]). Esc or Done closes.
pub fn window(ctx: &egui::Context, p: &mut UiCtx) {
    let Some(mut st) = p.tools.junction_editor.clone() else { return };
    let Some(view) = p.sim.world.intersection_view(st.at) else {
        p.tools.junction_editor = None;
        return;
    };
    // [r1-roads] (K20) The camera comes over the junction once, looking
    // well down on it so the arrows lie along their lanes.
    if !st.framed {
        st.framed = true;
        p.follow.stop();
        *p.mode = crate::camera::CameraMode::Orbit;
        p.fly.to(crate::camera::FlyGoal::Point(Vec2::new(st.at.x, st.at.y)), Some((st.extent * 2.3).clamp(90.0, 420.0)), Some(1.2));
    }
    let mut close = false;
    let mut open = true;
    egui::Window::new(egui::RichText::new("Intersection").color(CYAN))
        .id(egui::Id::new("junction_editor"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_width(310.0)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 64.0))
        .show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(egui::RichText::new(title(&view)).weak());
            ui.label(egui::RichText::new("Editing on the road, in the box: hover a lane's arrow to see its moves, click it for its options (right click: next option); click a crosswalk's dot to upgrade it. Time stands still until Done.").small().weak());
            ui.separator();
            if controls(ui, &mut p.sim.world, &mut st, &view) {
                close = true;
            }
            if let Some((msg, ok)) = &st.message {
                ui.label(egui::RichText::new(msg).color(if *ok { egui::Color32::from_rgb(140, 230, 160) } else { WARN }));
            }
        });
    if !open || (st.lane_menu.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
        close = true;
    }
    if st.lane_menu.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        st.lane_menu = None;
    }
    p.tools.junction_editor = if close { None } else { Some(st) };
}

/// [r1-roads] (K20) Time stands still while the Intersection editor is
/// open (the player edits a junction with nothing moving through it), and
/// goes on as it was when it closes, however it was closed (Done, Esc, the
/// remote API).
pub fn hold_time(tools: Res<crate::tools::ToolState>, mut sim: ResMut<crate::sim::Sim>, mut held: Local<Option<bool>>) {
    match (tools.junction_editor.is_some(), *held) {
        (true, None) => {
            *held = Some(sim.paused);
            sim.paused = true;
        }
        (true, Some(_)) => {
            if !sim.paused {
                sim.paused = true;
            }
        }
        (false, Some(was)) => {
            sim.paused = was;
            *held = None;
        }
        (false, None) => {}
    }
}

/// [r1-roads] (K20) The editor on the map: the box round the junction's
/// area, and the lane arrows and crosswalk dots on the roads themselves,
/// projected through the map's camera. The box takes the pointer (the map
/// tools leave it alone while the editor is open).
pub fn on_map(mut contexts: EguiContexts, mut sim: ResMut<crate::sim::Sim>, mut tools: ResMut<crate::tools::ToolState>, flags: Res<super::UiFlags>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    let Some(mut st) = tools.junction_editor.clone() else { return Ok(()) };
    if flags.data_view {
        return Ok(());
    }
    let Some(view) = sim.world.intersection_view(st.at) else { return Ok(()) };
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?.clone();
    let sc = Screen { camera, gt, z: sim.world.surface_height(st.at) + 0.15, zoom: ctx.zoom_factor() };
    let corners: Vec<egui::Pos2> = st.box_corners().iter().map(|&c| sc.at(c)).collect();
    if corners.iter().any(|c| c.x < -1.0e4) {
        return Ok(());
    }
    // Between the top and bottom bars: nothing is drawn over them.
    let clip = flags.map_rect(ctx.content_rect());
    let rect = egui::Rect::from_points(&corners).intersect(clip);
    if !rect.is_positive() {
        return Ok(());
    }
    road_view(&ctx, &mut sim.world, &mut st, &view, &sc, rect, &corners, clip);
    tools.junction_editor = Some(st);
    Ok(())
}

fn title(v: &IntersectionView) -> String {
    if v.node.is_none() {
        return "a crosswalk".into();
    }
    let mut names: Vec<&str> = v.approaches.iter().map(|a| a.name.as_str()).collect();
    names.sort();
    names.dedup();
    let kind = if v.roundabout {
        "roundabout"
    } else if v.signalized {
        "signals"
    } else {
        "no signals"
    };
    format!("{} \u{00b7} {kind} \u{00b7} {} streets", names.join(" \u{00d7} "), v.approaches.len())
}

/// Screen direction of a ground direction `d` at `p`.
fn screen_dir(sc: &Screen, p: SVec2, d: SVec2) -> egui::Vec2 {
    (sc.at(p + d) - sc.at(p)).normalized()
}

/// A lane arrow: `bits` drawn as a shaft up the lane (`fwd`, screen;
/// `len` from the middle to either end) with a head straight on and a
/// hooked head to each side it may turn to (`left` is the lane's left on
/// screen; `wid` from the middle to the side).
fn paint_glyph(painter: &egui::Painter, c: egui::Pos2, fwd: egui::Vec2, left: egui::Vec2, len: f32, wid: f32, bits: u8, col: egui::Color32) {
    let h = len.min(wid);
    let stroke = egui::Stroke::new((h * 0.17).max(1.8), col);
    let base = c - fwd * len * 0.8;
    let knee = c + fwd * len * 0.05;
    painter.line_segment([base, knee], stroke);
    let head = |tip: egui::Pos2, dir: egui::Vec2| {
        let side = egui::vec2(-dir.y, dir.x);
        let a = tip - dir * h * 0.45 + side * h * 0.34;
        let b = tip - dir * h * 0.45 - side * h * 0.34;
        painter.add(egui::Shape::convex_polygon(vec![tip, a, b], col, egui::Stroke::NONE));
    };
    if bits & STRAIGHT != 0 {
        let tip = c + fwd * len * 0.85;
        painter.line_segment([knee, tip - fwd * h * 0.35], stroke);
        head(tip, fwd);
    }
    for (bit, side) in [(LEFT, left), (RIGHT, -left)] {
        if bits & bit != 0 {
            let elbow = knee + fwd * len * 0.25;
            let tip = elbow + side * wid * 0.85;
            painter.line_segment([knee, elbow], stroke);
            painter.line_segment([elbow, tip - side * h * 0.35], stroke);
            head(tip, side);
        }
    }
}

/// A move through the junction on the view: a line with a head at its end.
fn paint_move(painter: &egui::Painter, sc: &Screen, path: &[SVec2], allowed: bool) {
    if path.len() < 2 {
        return;
    }
    let pts: Vec<egui::Pos2> = path.iter().map(|&q| sc.at(q)).collect();
    if allowed {
        painter.add(egui::Shape::line(pts.clone(), egui::Stroke::new(6.0, egui::Color32::from_black_alpha(160))));
        painter.add(egui::Shape::line(pts.clone(), egui::Stroke::new(3.0, CYAN)));
    } else {
        painter.extend(egui::Shape::dashed_line(&pts, egui::Stroke::new(4.5, egui::Color32::from_black_alpha(140)), 6.0, 5.0));
        painter.extend(egui::Shape::dashed_line(&pts, egui::Stroke::new(2.2, FAINT), 6.0, 5.0));
    }
    let n = pts.len();
    let dir = (pts[n - 1] - pts[n - 2]).normalized();
    let side = egui::vec2(-dir.y, dir.x);
    let tip = pts[n - 1];
    let col = if allowed { CYAN } else { FAINT };
    painter.add(egui::Shape::convex_polygon(vec![tip + dir * 4.0, tip - dir * 7.0 + side * 6.0, tip - dir * 7.0 - side * 6.0], col, egui::Stroke::NONE));
}

/// One lane's arrow on the view: where it is drawn and its frame.
struct LaneSpot {
    approach: usize,
    lane: usize,
    centre: egui::Pos2,
    fwd: egui::Vec2,
    left: egui::Vec2,
}

fn lane_spots(sc: &Screen, v: &IntersectionView) -> Vec<LaneSpot> {
    let mut out = Vec::new();
    for (i, a) in v.approaches.iter().enumerate() {
        for (j, l) in a.lanes.iter().enumerate() {
            if l.dir == SVec2::ZERO {
                continue;
            }
            // A little back from the stop line, on the lane.
            let at = l.end - l.dir * 7.5;
            let left = SVec2::new(l.dir.y, -l.dir.x);
            out.push(LaneSpot { approach: i, lane: j, centre: sc.at(at), fwd: screen_dir(sc, at, l.dir), left: screen_dir(sc, at, left) });
        }
    }
    out
}

/// The lane arrows and crosswalks on the roads in the box (`corners`, on
/// screen; `rect` their bounds), and the box itself. The pointer is read
/// straight from the input (the box is no egui area, so the camera still
/// orbits and zooms over it; the map tools stand aside while the editor
/// is open), never through a window over it.
#[allow(clippy::too_many_arguments)]
fn road_view(ctx: &egui::Context, w: &mut World, st: &mut EditorState, v: &IntersectionView, sc: &Screen, rect: egui::Rect, corners: &[egui::Pos2], clip: egui::Rect) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("junction_on_map"))).with_clip_rect(clip);
    // The box: a dim veil outside it would hide the city; a bright frame
    // with corner ticks on the ground says where the editing is.
    let mut ring = corners.to_vec();
    ring.push(corners[0]);
    painter.add(egui::Shape::line(ring.clone(), egui::Stroke::new(5.0, egui::Color32::from_black_alpha(140))));
    painter.extend(egui::Shape::dashed_line(&ring, egui::Stroke::new(2.0, CYAN), 10.0, 6.0));
    // The label over the box's top corner, kept on the map.
    let top = corners.iter().copied().fold(egui::pos2(f32::MAX, f32::MAX), |a, c| if c.y < a.y { c } else { a });
    let label = top.clamp(clip.min + egui::vec2(6.0, 18.0), clip.max - egui::vec2(220.0, 6.0)) + egui::vec2(4.0, -4.0);
    painter.text(label, egui::Align2::LEFT_BOTTOM, "INTERSECTION EDIT \u{00b7} time stopped", crate::ui::theme::small(), CYAN);
    let free = !ctx.is_pointer_over_egui();
    let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| free && rect.contains(*p));
    let click = pointer.filter(|_| ctx.input(|i| i.pointer.primary_clicked()));
    let right_click = pointer.is_some() && ctx.input(|i| i.pointer.secondary_clicked());
    // Each lane's badge: as wide as the lane (side by side, never
    // overlapping), longer along it, a clickable size at any zoom.
    let px = sc.scale(st.at);
    let wid = (nd_sim::roads::LANE_WIDTH * px * 0.44).clamp(7.0, 18.0);
    let len = (wid * 1.7).clamp(13.0, 30.0);
    let spots = lane_spots(sc, v);
    let inside = |sp: &LaneSpot, p: egui::Pos2| {
        let r = p - sp.centre;
        (r.dot(sp.fwd)).abs() < len && (r.dot(sp.left)).abs() < wid
    };
    // [r1-roads] (K2) A crosswalk's dot sits by the stop line, under the
    // lane badges there: the dot wins under the pointer (it is the small
    // target), so it can always be clicked; a badge still answers on the
    // rest of its face.
    let on_dot = |p: egui::Pos2| v.crossings.iter().any(|c| p.distance(sc.at(c.mid)) < DOT_PICK);
    let hovered = pointer.filter(|&p| !on_dot(p)).and_then(|p| spots.iter().filter(|sp| inside(sp, p)).min_by(|a, b| a.centre.distance(p).total_cmp(&b.centre.distance(p)))).map(|sp| (sp.approach, sp.lane));
    let shown = hovered.or(st.lane_menu).or(st.hover_lane);
    // The shown lane's moves under the arrows.
    if let Some((ai, li)) = shown {
        if let Some(l) = v.approaches.get(ai).and_then(|a| a.lanes.get(li)) {
            for m in l.moves.iter().filter(|m| !m.permitted) {
                paint_move(&painter, sc, &m.path, false);
            }
            for m in l.moves.iter().filter(|m| m.permitted) {
                paint_move(&painter, sc, &m.path, true);
            }
        }
    }
    for sp in &spots {
        let a = &v.approaches[sp.approach];
        let bits = a.arrows.get(sp.lane).copied().unwrap_or(0);
        let on = shown == Some((sp.approach, sp.lane));
        let fill = if on { egui::Color32::from_rgb(16, 70, 92) } else { egui::Color32::from_black_alpha(185) };
        let ring = if a.fixed { egui::Color32::from_rgb(255, 196, 90) } else if on { CYAN } else { egui::Color32::from_gray(110) };
        let (f, l) = (sp.fwd * len, sp.left * (wid - 1.0));
        let corners = vec![sp.centre - f - l, sp.centre + f - l, sp.centre + f + l, sp.centre - f + l];
        painter.add(egui::Shape::convex_polygon(corners, fill, egui::Stroke::new(if on { 2.0 } else { 1.2 }, ring)));
        paint_glyph(&painter, sp.centre, sp.fwd, sp.left, len * 0.82, wid * 0.82, bits, egui::Color32::WHITE);
    }
    if let Some((ai, li)) = hovered {
        if let Some(a) = v.approaches.get(ai) {
            let bits = a.arrows.get(li).copied().unwrap_or(0);
            let text = format!("{} from the {}, lane {} of {}: {}{}. Click to change.", a.name, compass(a.dir), li + 1, a.arrows.len(), choice_label(bits).to_lowercase(), if a.fixed { " (set by you)" } else { " (automatic)" });
            if let Some(p) = pointer {
                egui::Area::new(egui::Id::new("junction_lane_tip")).order(egui::Order::Tooltip).fixed_pos(p + egui::vec2(16.0, 16.0)).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(text));
                });
            }
        }
    }
    // The crosswalks: a dot in the middle of each.
    let mut on_crossing = false;
    for c in &v.crossings {
        let m = sc.at(c.mid);
        let (label, col) = match c.upgrade {
            CrossingUpgrade::None => ("", egui::Color32::from_gray(200)),
            CrossingUpgrade::Button => ("B", egui::Color32::from_rgb(255, 200, 60)),
            CrossingUpgrade::Footbridge => ("F", CYAN),
            CrossingUpgrade::Removed => ("X", egui::Color32::from_rgb(255, 90, 90)),
        };
        // A footbridge: its walkway and stairs, outlined.
        if let Some((top, foot, width)) = c.footbridge {
            let stroke = egui::Stroke::new((width * px).max(3.0), egui::Color32::from_rgba_unmultiplied(90, 230, 255, 150));
            painter.line_segment([sc.at(top[0]), sc.at(top[1])], stroke);
            for i in 0..2 {
                painter.line_segment([sc.at(top[i]), sc.at(foot[i])], egui::Stroke::new((width * px * 0.8).max(2.0), egui::Color32::from_rgba_unmultiplied(90, 230, 255, 110)));
            }
        }
        let sel = st.crossing == Some(c.index);
        let (a, b) = (sc.at(c.ends[0]), sc.at(c.ends[1]));
        painter.line_segment([a, b], egui::Stroke::new(if sel { 4.0 } else { 2.0 }, col.gamma_multiply(if c.closed { 0.25 } else { 0.8 })));
        painter.circle_filled(m, 8.0, egui::Color32::from_black_alpha(200));
        painter.circle_stroke(m, 8.0, egui::Stroke::new(if sel { 2.5 } else { 1.5 }, if sel { CYAN } else { col }));
        if !label.is_empty() {
            painter.text(m, egui::Align2::CENTER_CENTER, label, crate::ui::theme::small(), col);
        }
        if click.is_some_and(|p| p.distance(m) < DOT_PICK) && hovered.is_none() {
            st.crossing = Some(c.index);
            st.lane_menu = None;
            on_crossing = true;
        }
    }
    // A right click on a lane's arrow steps it to its next option.
    if let (true, Some((ai, li)), Some(k)) = (right_click, hovered, v.intersection) {
        let a = &v.approaches[ai];
        let next = next_choice(a.arrows.get(li).copied().unwrap_or(0), a.exits);
        let arrows = fit_lanes(&a.arrows, li, next, a.exits);
        st.message = Some(match w.set_lane_permissions(k, a.segment, Some(arrows)) {
            Ok(()) => (format!("{}, lane {}: {}.", a.name, li + 1, choice_label(next).to_lowercase()), true),
            Err(e) => (format!("Not done: {e}."), false),
        });
    }
    if let (Some(_), Some(h)) = (click, hovered) {
        st.lane_menu = if st.lane_menu == Some(h) { None } else { Some(h) };
        st.approach = Some(h.0);
    } else if click.is_some() && !on_crossing {
        st.lane_menu = None;
    }
    // The lane menu, beside its arrow.
    if let Some((ai, li)) = st.lane_menu {
        match spots.iter().find(|sp| sp.approach == ai && sp.lane == li) {
            Some(sp) => lane_menu(ctx, w, st, v, sp, rect),
            None => st.lane_menu = None,
        }
    }
}

/// The options of one lane, in a small panel beside its arrow.
fn lane_menu(ctx: &egui::Context, w: &mut World, st: &mut EditorState, v: &IntersectionView, sp: &LaneSpot, rect: egui::Rect) {
    let a = &v.approaches[sp.approach];
    let Some(k) = v.intersection else { return };
    let cur = a.arrows.get(sp.lane).copied().unwrap_or(0);
    // In the view's corner across from the lane, off the junction, with a
    // line to the lane's arrow.
    let w_menu = 236.0;
    let x = if sp.centre.x < rect.center().x { rect.right() - w_menu - 6.0 } else { rect.left() + 6.0 };
    let y = if sp.centre.y < rect.center().y { rect.bottom() - 250.0 } else { rect.top() + 30.0 };
    let mut pick: Option<u8> = None;
    let mut auto = false;
    let mut close = false;
    let mut ban: Option<bool> = None;
    let menu = egui::Area::new(egui::Id::new(("lane_menu", sp.approach, sp.lane))).order(egui::Order::Tooltip).fixed_pos(egui::pos2(x, y)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(w_menu - 16.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Lane {} of {}", sp.lane + 1, a.arrows.len())).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("\u{2715}").clicked() {
                        close = true;
                    }
                });
            });
            ui.label(egui::RichText::new(format!("{} from the {}", a.name, compass(a.dir))).small().weak());
            ui.add_space(2.0);
            let choices = lane_choices(a.exits);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                for c in choices {
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::click());
                    let on = c == cur;
                    let fill = if on { egui::Color32::from_rgb(16, 80, 104) } else if resp.hovered() { egui::Color32::from_gray(48) } else { egui::Color32::from_gray(28) };
                    ui.painter().rect_filled(r, 6.0, fill);
                    ui.painter().rect_stroke(r, 6.0, egui::Stroke::new(if on { 2.0 } else { 1.0 }, if on { CYAN } else { egui::Color32::from_gray(80) }), egui::StrokeKind::Inside);
                    // Drawn heading up the screen, the lane's left on the left.
                    paint_glyph(ui.painter(), r.center() + egui::vec2(0.0, 4.0), egui::vec2(0.0, -1.0), egui::vec2(-1.0, 0.0), 15.0, 15.0, c, egui::Color32::WHITE);
                    if resp.on_hover_text(choice_label(c)).clicked() && !on {
                        pick = Some(c);
                    }
                }
            });
            ui.label(egui::RichText::new(choice_label(cur)).small());
            ui.separator();
            if a.fixed {
                if ui.button("Automatic arrows for this street").on_hover_text("Let the junction set this street's lanes from how people turn.").clicked() {
                    auto = true;
                }
            } else {
                ui.label(egui::RichText::new("Automatic: follows how people turn").small().weak());
            }
            if a.left_possible {
                let mut allowed = !a.no_left;
                if toggle_row(ui, &mut allowed, "Left turns from here") {
                    ban = Some(!allowed);
                }
            }
            if a.queued > 0 {
                ui.label(egui::RichText::new(format!("{} queued", a.queued)).small().weak());
            }
        });
    });
    let r = menu.response.rect;
    let near = egui::pos2(sp.centre.x.clamp(r.left(), r.right()), sp.centre.y.clamp(r.top(), r.bottom()));
    ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("junction_on_map"))).line_segment([sp.centre, near], egui::Stroke::new(1.5, CYAN.gamma_multiply(0.8)));
    if close {
        st.lane_menu = None;
    }
    if let Some(c) = pick {
        let arrows = fit_lanes(&a.arrows, sp.lane, c, a.exits);
        st.message = Some(match w.set_lane_permissions(k, a.segment, Some(arrows.clone())) {
            Ok(()) => (format!("{}: lanes now {}.", a.name, arrows.iter().map(|&x| arrows_label(x)).collect::<Vec<_>>().join(" | ")), true),
            Err(e) => (format!("Not done: {e}."), false),
        });
    }
    if auto {
        st.message = Some(match w.set_lane_permissions(k, a.segment, None) {
            Ok(()) => (format!("{}: automatic arrows.", a.name), true),
            Err(e) => (format!("Not done: {e}."), false),
        });
    }
    if let Some(no) = ban {
        let mut rule = v.rule.clone();
        rule.set_left_forbidden(a.dir, no);
        st.message = Some(match w.set_junction_rule(rule) {
            Ok(()) => (format!("Left turns from {} {}.", a.name, if no { "banned" } else { "allowed" }), true),
            Err(e) => (format!("Not done: {e}."), false),
        });
    }
}

/// An on/off switch.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = ui.spacing().interact_size.y * egui::vec2(1.9, 1.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(resp.id, *on);
        let r = rect.height() * 0.5;
        let fill = crate::ui::theme::lerp(egui::Color32::from_gray(55), egui::Color32::from_rgb(30, 130, 160), t);
        ui.painter().rect(rect, r, fill, egui::Stroke::new(1.0, if *on { CYAN } else { egui::Color32::from_gray(90) }), egui::StrokeKind::Inside);
        let cx = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
        ui.painter().circle(egui::pos2(cx, rect.center().y), r * 0.78, egui::Color32::from_gray(235), egui::Stroke::NONE);
    }
    resp
}

/// A toggle with its label, the switch on the right. True when flipped.
fn toggle_row(ui: &mut egui::Ui, on: &mut bool, label: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| toggle(ui, on).changed()).inner
    })
    .inner
}

/// The panel beside the view. True when Done was pressed.
fn controls(ui: &mut egui::Ui, w: &mut World, st: &mut EditorState, v: &IntersectionView) -> bool {
    let mut done = false;
    if v.node.is_some() {
        if v.roundabout {
            ui.label("A roundabout's junctions give way by the ring's own rules; only its crosswalks can be upgraded.");
        } else {
            junction_controls(ui, w, st, v);
        }
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Crosswalk").strong());
    match v.crossings.iter().find(|c| Some(c.index) == st.crossing) {
        None if v.crossings.is_empty() => {
            ui.label(egui::RichText::new("None here.").weak());
        }
        None => {
            ui.label(egui::RichText::new(format!("Click a crosswalk's dot on the view to upgrade it ({} here).", v.crossings.len())).small().weak());
        }
        Some(c) => crossing_controls(ui, w, st, c),
    }
    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        if v.node.is_some() && !v.roundabout && ui.button("Reset junction").on_hover_text("Signals as the junction decides, default timing, left turns allowed, every street's lanes automatic.").clicked() {
            let mut ok = w.set_junction_rule(JunctionRule::new(v.at));
            if let Some(k) = v.intersection {
                for a in v.approaches.iter().filter(|a| a.fixed) {
                    ok = ok.and(w.set_lane_permissions(k, a.segment, None));
                }
            }
            st.message = Some(match ok {
                Ok(()) => ("Junction back to its defaults.".into(), true),
                Err(e) => (format!("Not done: {e}."), false),
            });
            st.lane_menu = None;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(egui::RichText::new("Done").strong()).min_size(egui::vec2(80.0, 0.0))).clicked() {
                done = true;
            }
        });
    });
    done
}

fn crossing_controls(ui: &mut egui::Ui, w: &mut World, st: &mut EditorState, c: &nd_sim::CrossingView) {
    let street = w.roads.nearest_where(c.mid, |s| s.is_street()).and_then(|(sid, ..)| w.streets.of(sid).map(|s| s.name.clone())).unwrap_or_else(|| "the street".into());
    ui.label(egui::RichText::new(format!("Across {street}")).small());
    let mut pick = c.upgrade;
    // [walk-crossings] The upgrades on one row, "Remove crosswalk" on its own
    // below (it squeezed into a column of letters beside the others).
    for row in [&[CrossingUpgrade::None, CrossingUpgrade::Button, CrossingUpgrade::Footbridge][..], &[CrossingUpgrade::Removed][..]] {
        ui.horizontal_wrapped(|ui| {
            for &u in row {
                let enabled = !(u == CrossingUpgrade::Button && c.signalised);
                let text = format!("{}{}", upgrade_label(u), if u.cost() > 0.0 && u != c.upgrade { format!(" ${:.0}", u.cost()) } else { String::new() });
                ui.add_enabled_ui(enabled, |ui| {
                    let b = ui.add(egui::Button::selectable(pick == u, text));
                    let b = if u == CrossingUpgrade::Removed { b.on_hover_text("No crossing here: nothing painted, no walk light; people cross at another crossing (and nobody jaywalks right beside it).") } else { b };
                    if b.clicked() {
                        pick = u;
                    }
                });
            }
        });
    }
    if pick != c.upgrade {
        st.message = Some(match w.set_crossing_upgrade(c.index, pick) {
            Ok(()) => (format!("Crosswalk across {street}: {}.", upgrade_label(pick).to_lowercase()), true),
            Err(e) => (format!("Not done: {e}."), false),
        });
    }
    if let Some(light) = c.light {
        ui.label(egui::RichText::new(format!("push button: {}", light.label())).small().weak());
    }
    if c.signalised && c.upgrade == CrossingUpgrade::None {
        ui.label(egui::RichText::new("walk lights run with the junction's signals").small().weak());
    }
    // [walk-crossings] Why a crossing is shut, or gone.
    if c.upgrade == CrossingUpgrade::Removed {
        ui.label(egui::RichText::new("no crossing here: people cross somewhere else").small().weak());
    } else if c.ramp && c.upgrade == CrossingUpgrade::None && !c.signalised {
        ui.label(egui::RichText::new("over a highway ramp: nobody crosses here without signals, a push button or a footbridge").small().weak());
    } else if c.closed && c.upgrade == CrossingUpgrade::None {
        ui.label(egui::RichText::new("replaced by the footbridge next to it").small().weak());
    }
    // [footbridge] The crosswalk under a footbridge goes, unless kept.
    if c.upgrade == CrossingUpgrade::Footbridge {
        let mut keep = c.kept;
        if toggle_row(ui, &mut keep, "Keep the crosswalk under it") {
            st.message = Some(match w.set_crosswalk_kept(c.index, keep) {
                Ok(()) => (format!("Crosswalk across {street}: {}.", if keep { "kept under the footbridge" } else { "replaced by the footbridge" }), true),
                Err(e) => (format!("Not done: {e}."), false),
            });
        }
    }
}

fn upgrade_label(u: CrossingUpgrade) -> &'static str {
    match u {
        CrossingUpgrade::None => "Plain",
        CrossingUpgrade::Button => "Push button",
        CrossingUpgrade::Footbridge => "Footbridge",
        CrossingUpgrade::Removed => "Remove crosswalk",
    }
}

/// "auto" beside a setting the junction decides, else a button putting it
/// back. True when that button was pressed.
fn auto_tag(ui: &mut egui::Ui, is_auto: bool) -> bool {
    if is_auto {
        ui.label(egui::RichText::new("auto").small().weak());
        false
    } else {
        ui.small_button("auto").on_hover_text("Let the junction decide").clicked()
    }
}

fn junction_controls(ui: &mut egui::Ui, w: &mut World, st: &mut EditorState, v: &IntersectionView) {
    let mut rule: JunctionRule = v.rule.clone();
    let before = rule.clone();
    // Signals: on or off; the junction decides until set.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Traffic signals").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut on = v.signalized;
            if toggle(ui, &mut on).on_hover_text("On: signals. Off: no signals, drivers give way.").changed() {
                rule.signal = if on { SignalMode::Signals } else { SignalMode::Unsignalised };
            }
            if auto_tag(ui, rule.signal == SignalMode::Auto) {
                rule.signal = SignalMode::Auto;
            }
        });
    });
    if v.signalized {
        ui.horizontal(|ui| {
            ui.label("Protected left turn").on_hover_text("A green arrow for left turners at the start of each green, before oncoming traffic goes.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // As needed (auto) shows on: the junction gives one when left turners wait.
                let mut on = rule.left_arrow != LeftArrow::Never;
                if toggle(ui, &mut on).changed() {
                    rule.left_arrow = if on { LeftArrow::Always } else { LeftArrow::Never };
                }
                if auto_tag(ui, rule.left_arrow == LeftArrow::Auto) {
                    rule.left_arrow = LeftArrow::Auto;
                }
            });
        });
        if rule.left_arrow == LeftArrow::Auto {
            ui.label(egui::RichText::new("auto: an arrow when left turners are waiting").small().weak());
        }
    }
    let possible: Vec<&nd_sim::ApproachView> = v.approaches.iter().filter(|a| a.left_possible).collect();
    if !possible.is_empty() {
        let banned = possible.iter().filter(|a| rule.left_forbidden(a.dir)).count();
        let mut all = banned == 0;
        ui.horizontal(|ui| {
            ui.label("Left turns");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if toggle(ui, &mut all).on_hover_text("Off: no left turns from any street here. Per street: click a lane's arrow.").changed() {
                    for a in &possible {
                        rule.set_left_forbidden(a.dir, !all);
                    }
                }
                if banned > 0 && banned < possible.len() {
                    ui.label(egui::RichText::new(format!("banned from {banned}")).small().weak());
                }
            });
        });
    }
    if v.signalized {
        egui::CollapsingHeader::new("Signal timing").id_salt(("junction_timing", v.node.map(|n| n.0))).show(ui, |ui| {
            for ph in &v.phases {
                let mut names: Vec<String> = ph.approaches.iter().filter_map(|&ap| v.approaches.get(ap)).map(|a| short(&a.name)).collect();
                names.dedup();
                let mut g = ph.green;
                let label = format!("{}{}", if ph.running { "\u{25b6} " } else { "" }, names.join(" + "));
                if ui.add(egui::Slider::new(&mut g, GREEN_MIN..=GREEN_MAX).step_by(1.0).suffix(" s").text(label)).changed() {
                    for &ap in &ph.approaches {
                        if let Some(a) = v.approaches.get(ap) {
                            rule.set_green(a.dir, Some(g));
                        }
                    }
                }
            }
            if rule.left_arrow != LeftArrow::Never {
                let mut s = if rule.arrow_seconds > 0.0 { rule.arrow_seconds } else { nd_sim::traffic::Intersection::DEFAULT_ARROW };
                if ui.add(egui::Slider::new(&mut s, ARROW_MIN_SECONDS..=ARROW_MAX_SECONDS).step_by(1.0).suffix(" s").text("left arrow")).changed() {
                    rule.arrow_seconds = s;
                }
            }
            if ui.small_button("Default timing").clicked() {
                rule.greens.clear();
                rule.arrow_seconds = 0.0;
            }
        });
    }
    if rule != before {
        st.message = Some(match w.set_junction_rule(rule) {
            Ok(()) => ("Junction settings applied.".into(), true),
            Err(e) => (format!("Not done: {e}."), false),
        });
    }
}

fn short(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_follow_the_exits() {
        assert_eq!(lane_choices(LEFT | STRAIGHT | RIGHT), vec![LEFT, STRAIGHT, RIGHT, LEFT | STRAIGHT, STRAIGHT | RIGHT, LEFT | STRAIGHT | RIGHT]);
        // A T from the stem: left or right, and both.
        assert_eq!(lane_choices(LEFT | RIGHT), vec![LEFT, RIGHT, LEFT | RIGHT]);
        assert_eq!(lane_choices(STRAIGHT | RIGHT), vec![STRAIGHT, RIGHT, STRAIGHT | RIGHT]);
        // Cycling goes round.
        let all = LEFT | STRAIGHT | RIGHT;
        assert_eq!(next_choice(LEFT, all), STRAIGHT);
        assert_eq!(next_choice(LEFT | STRAIGHT | RIGHT, all), LEFT);
        assert_eq!(next_choice(0, all), LEFT);
    }

    #[test]
    fn fitting_keeps_every_exit_reachable() {
        let all = LEFT | STRAIGHT | RIGHT;
        // Two lanes, L | S+R: the right lane made straight only, so the
        // right turn moves to the other lane.
        assert_eq!(fit_lanes(&[LEFT, STRAIGHT | RIGHT], 1, STRAIGHT, all), vec![LEFT | RIGHT, STRAIGHT]);
        // The left lane made straight: the left turn moves to the other lane.
        assert_eq!(fit_lanes(&[LEFT, STRAIGHT | RIGHT], 0, STRAIGHT, all), vec![STRAIGHT, LEFT | STRAIGHT | RIGHT]);
        // A change that strands nothing changes only that lane.
        assert_eq!(fit_lanes(&[LEFT, STRAIGHT, STRAIGHT | RIGHT], 1, LEFT | STRAIGHT, all), vec![LEFT, LEFT | STRAIGHT, STRAIGHT | RIGHT]);
        // Three lanes, the middle one loses straight: straight on goes to
        // another lane.
        let a = fit_lanes(&[LEFT, STRAIGHT, RIGHT], 1, LEFT, all);
        assert!(a.iter().any(|x| x & STRAIGHT != 0) && a[1] == LEFT);
        // One lane must carry every move.
        assert_eq!(fit_lanes(&[all], 0, LEFT, all), vec![all]);
        // Bits the junction does not offer are dropped.
        assert_eq!(fit_lanes(&[LEFT | STRAIGHT, STRAIGHT | RIGHT], 0, LEFT | STRAIGHT, STRAIGHT | RIGHT), vec![STRAIGHT, STRAIGHT | RIGHT]);
        // Every result is one the sim accepts, with the lane as chosen.
        for arrows in [vec![LEFT, STRAIGHT | RIGHT], vec![LEFT, STRAIGHT, STRAIGHT | RIGHT], vec![LEFT | STRAIGHT, RIGHT]] {
            for lane in 0..arrows.len() {
                for c in lane_choices(all) {
                    let f = fit_lanes(&arrows, lane, c, all);
                    assert!(f.iter().all(|x| *x != 0 && x & !all == 0));
                    assert_eq!(f.iter().fold(0, |acc, x| acc | x), all, "{arrows:?} lane {lane} -> {c}: {f:?}");
                    assert_eq!(f[lane], c);
                }
            }
        }
    }
}
