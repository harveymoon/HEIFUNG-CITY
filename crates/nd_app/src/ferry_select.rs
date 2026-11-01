//! [ferry-routes] The water network on the map. Select a ferry terminal
//! (or a shipping port) and every route that calls there is drawn as a
//! bright glowing band over the water in its route colour, with chevrons
//! flowing both ways along it, a post and rings at the far end and a gold
//! ring at the selected dock; every other route in the city is drawn as a
//! dim line, so the whole network reads at once. Ferries on the selected
//! routes get an outline. Selecting a ferry or freighter does the same for
//! its own route.
//!
//! The routes come from `World::water_net` (`nd_sim::world::ferry_routes`),
//! which runs the water router: `net` keeps one copy, shared by the map,
//! the inspector and the remote API, and rebuilds it only when
//! `World::water_net_signature` changes. The bands are meshes, one per
//! route, built with the network and only restyled (bright, dim, hidden)
//! when the selection changes.

use std::sync::{Arc, Mutex};

use bevy::gizmos::config::{GizmoConfig, GizmoConfigGroup, GizmoLineConfig};
use bevy::prelude::*;
use bevy_egui::egui;
use nd_procgen::MeshData;
use nd_sim::ports::{Ship, ShipId, ShipKind};
use nd_sim::world::ferry_routes::{LineEnd, LineKind, WaterLine, WaterNet};
use nd_sim::{BuildingId, PortKind, Vec2 as SVec2, World};

use crate::overlay::mesh::{MeshOverlay, MeshOverlayPlugin, OverlayCx, OverlayMat, Pieces, Wanted};
use crate::overlay::OverlaySet;
use bevy::ecs::system::SystemParamItem;
use crate::sim::Sim;
use crate::overlay::shapes::{flat_at, ribbon, vehicle_box, GOLD};
use crate::tools::Selection;

pub struct FerrySelectPlugin;

impl Plugin for FerrySelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BandStyle>()
            .init_resource::<WaterFrame>()
            .insert_gizmo_config(LitGizmos, GizmoConfig { depth_bias: -0.5, line: GizmoLineConfig { width: 4.0, ..default() }, ..default() })
            .insert_gizmo_config(DimGizmos, GizmoConfig { depth_bias: -0.5, line: GizmoLineConfig { width: 2.0, ..default() }, ..default() })
            .add_plugins(MeshOverlayPlugin::<WaterBands>::default())
            .add_systems(Update, draw.in_set(OverlaySet::Draw)).add_systems(bevy_egui::EguiPrimaryContextPass, labels);
    }
}

/// Lines of the routes in focus: wide enough to read from high up.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct LitGizmos;

/// The rest of the network, thinner.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct DimGizmos;

// ── the shared network ──────────────────────────────────────────────────────

static NET: Mutex<Option<(u64, Arc<WaterNet>)>> = Mutex::new(None);

/// The city's water network and its signature, rebuilt only when the
/// signature has changed since the last call.
pub fn net(w: &World) -> (u64, Arc<WaterNet>) {
    let sig = w.water_net_signature();
    let mut g = NET.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, n)) = g.as_ref() {
        if *s == sig {
            return (sig, n.clone());
        }
    }
    let n = Arc::new(w.water_net());
    *g = Some((sig, n.clone()));
    (sig, n)
}

/// [overlay] The water network as `sync_lines` last worked it out, with
/// the `Sim` change tick it was read at: while the tick holds (nothing has
/// touched the sim since), the draws and labels after it reuse it rather
/// than ask `net` again (review P-O3).
#[derive(Resource, Default)]
pub struct WaterFrame {
    at: Option<(bevy::ecs::change_detection::Tick, Arc<WaterNet>)>,
}

impl WaterFrame {
    /// The network, if read since the sim last changed.
    pub fn fresh(&self, sim: &Res<Sim>) -> Option<&WaterNet> {
        self.at.as_ref().filter(|(t, _)| *t == sim.last_changed()).map(|(_, n)| &**n)
    }

    /// The network, from this frame's copy when it is still good.
    fn get(&self, sim: &Res<Sim>) -> Arc<WaterNet> {
        match &self.at {
            Some((t, n)) if *t == sim.last_changed() => n.clone(),
            _ => net(&sim.world).1,
        }
    }
}

/// Is `b` a dock with routes to show: a ferry terminal or shipping port?
pub fn is_route_dock(w: &World, b: BuildingId) -> bool {
    matches!(w.ports.kind_of(b), Some(PortKind::FerryTerminal | PortKind::FerryTerminalLarge | PortKind::ShippingPort))
}

/// What the selection puts in focus: the lines to draw bright and the dock
/// at the middle of it (none for a ship). `None`: the network is hidden.
pub fn focus(w: &World, net: &WaterNet, sel: Selection) -> Option<(Vec<usize>, Option<BuildingId>)> {
    match sel {
        Selection::Building(b) if is_route_dock(w, b) => Some((net.at(b), Some(b))),
        Selection::Ship(id) => w.ports.ships.get(id).map(|s| (net.of_ship(s).into_iter().collect(), None)),
        _ => None,
    }
}

/// A line's colour (linear, about unit brightness): its route colour for a
/// ferry, amber for a freighter lane, grey while it does not run.
pub fn line_rgb(l: &WaterLine) -> [f32; 3] {
    if !l.running {
        return [0.45, 0.47, 0.52];
    }
    match l.kind {
        LineKind::Ferry => nd_procgen::metro::line_colour(l.color),
        LineKind::Freight => [1.0, 0.55, 0.12],
    }
}

/// The same as an egui colour, for the inspector's swatches.
pub fn line_color32(l: &WaterLine) -> egui::Color32 {
    let [r, g, b] = line_rgb(l);
    let hi = r.max(g).max(b).max(1e-3);
    let s = |x: f32| ((x / hi).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    egui::Color32::from_rgb(s(r), s(g), s(b))
}

fn glow(l: &WaterLine, k: f32) -> Color {
    let g = crate::train_select::glow_rgb(line_rgb(l));
    Color::linear_rgb(g[0] * k, g[1] * k, g[2] * k)
}

/// Height of the bands over the water.
const LIFT: f32 = nd_sim::landscape::WATER_LEVEL + 0.6;

// ── ships ───────────────────────────────────────────────────────────────────

/// A ship's drawn pose this frame: centre and heading (radians).
pub fn ship_pose(s: &Ship, f: f32) -> (SVec2, f32) {
    let (p, d) = s.pose_at(s.prev_s + (s.s - s.prev_s) * f);
    (p, d.y.atan2(d.x))
}

fn ship_width(s: &Ship) -> f32 {
    match s.kind {
        ShipKind::Freighter => 13.0,
        ShipKind::Ferry => 9.0,
        ShipKind::CarFerry => 16.0,
        _ => 5.0,
    }
}

/// The ferry or freighter under `p`, by its drawn hull, and how far off its
/// centreline the click is.
pub fn pick(w: &World, p: SVec2, f: f32) -> Option<(f32, ShipId)> {
    w.ports
        .ships
        .iter()
        .filter(|(_, s)| matches!(s.kind, ShipKind::Ferry | ShipKind::CarFerry | ShipKind::Freighter))
        .filter_map(|(id, s)| {
            let (c, h) = ship_pose(s, f);
            let fwd = SVec2::new(h.cos(), h.sin());
            let d = p - c;
            let (along, across) = (d.dot(fwd).abs(), d.perp_dot(fwd).abs());
            (along <= s.length() * 0.5 + 2.0 && across <= ship_width(s) * 0.5 + 2.0).then_some((across, id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// A ship's name for titles: "Ferry", "Freighter", with where it plies.
pub fn ship_title(w: &World, id: ShipId) -> String {
    let Some(s) = w.ports.ships.get(id) else { return "a ship".into() };
    let home = w.buildings.get(s.port).map_or("its terminal", |b| b.name.as_str());
    match s.kind {
        ShipKind::Freighter => format!("Freighter for {home}"),
        _ => {
            let to = match (s.to, s.remote) {
                (Some(t), _) => w.buildings.get(t).map_or_else(|| "another terminal".to_string(), |b| b.name.clone()),
                (None, Some(k)) => w.ferry_town_name(s.port, k),
                _ => "elsewhere".to_string(),
            };
            let short = |n: &str| n.strip_suffix(" Ferry").unwrap_or(n).to_string();
            format!("Ferry: {} - {}", short(home), short(&to))
        }
    }
}

/// What a ship is doing, in a few words.
pub fn ship_doing(w: &World, id: ShipId) -> String {
    let Some(s) = w.ports.ships.get(id) else { return String::new() };
    let home = w.buildings.get(s.port).map_or("its terminal".to_string(), |b| b.name.clone());
    let p = s.pose().0;
    let off_map = p.x.abs().max(p.y.abs()) > w.landscape.land_half();
    match s.kind {
        ShipKind::Freighter => {
            if s.docked && s.dwell > 0 {
                format!("unloading at {home}")
            } else if s.docked {
                "heading back out to sea".into()
            } else {
                format!("coming in to {home}")
            }
        }
        _ => {
            let far = match (s.to, s.remote) {
                (Some(t), _) => w.buildings.get(t).map_or_else(|| "the other terminal".to_string(), |b| b.name.clone()),
                (None, Some(k)) => w.ferry_town_name(s.port, k),
                _ => "elsewhere".to_string(),
            };
            if s.home {
                format!("back at {home}, letting people off")
            } else if s.s <= 0.0 && s.dwell > 0 {
                format!("boarding at {home}")
            } else if off_map {
                format!("out of sight, on the way to or from {far}")
            } else if s.docked && s.dwell > 0 {
                format!("at the pier in {far}")
            } else if s.docked {
                format!("sailing back to {home}")
            } else {
                format!("sailing to {far}")
            }
        }
    }
}

// ── the bands ───────────────────────────────────────────────────────────────

#[derive(Component, Default)]
struct LineTag;

/// The focus the bands were last styled for (signature, lines lit; None
/// when hidden).
#[derive(Resource, Default)]
struct BandStyle(Option<(u64, Option<Vec<usize>>)>);

/// A glowing band along `pts`: a bright core and a soft halo fading out to
/// either side, flat on the water. Vertex colours carry the glow; the
/// material scales it bright or dim.
fn band(pts: &[SVec2], rgb: [f32; 3]) -> MeshData {
    // (offset across, alpha): halo, core, halo.
    const PROFILE: [(f32, f32); 6] = [(-9.0, 0.0), (-3.0, 0.45), (-1.6, 1.0), (1.6, 1.0), (3.0, 0.45), (9.0, 0.0)];
    ribbon(pts, &PROFILE, LIFT, crate::train_select::glow_rgb(rgb))
}

/// [overlay] The bands as a mesh overlay: rebuilt (hidden, one per line)
/// when the network changes, restyled (bright, dim, hidden) when the
/// selection does. The network is looked at only when something could
/// show it.
struct WaterBands;

impl MeshOverlay for WaterBands {
    /// The network's signature.
    type Key = u64;
    type Param = (Res<'static, Selection>, ResMut<'static, WaterFrame>, ResMut<'static, BandStyle>, Query<'static, 'static, (&'static mut Visibility, &'static mut MeshMaterial3d<StandardMaterial>), With<LineTag>>);
    type Marker = LineTag;

    fn key((selection, frame, style, _): &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx) -> Wanted<u64> {
        let w = &cx.sim.world;
        cx.mat(OverlayMat::WaterBright);
        cx.mat(OverlayMat::WaterDim);
        frame.at = None;
        // Only look at the network when something could show it.
        let wants = match **selection {
            Selection::Building(b) => is_route_dock(w, b),
            Selection::Ship(_) => true,
            _ => false,
        };
        if !wants && style.0.as_ref().is_some_and(|s| s.1.is_none()) {
            return Wanted::Unchanged;
        }
        let (sig, net) = net(w);
        frame.at = Some((cx.sim_tick, net));
        Wanted::Show(sig)
    }

    fn build((_, frame, style, _): &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx, _: &u64, out: &mut Pieces) {
        let Some((_, net)) = &frame.at else { return };
        // Freshly spawned: styled next frame, once they exist.
        style.0 = None;
        let dim = cx.mat(OverlayMat::WaterDim);
        for l in &net.lines {
            out.add_hidden(band(&l.visible(), line_rgb(l)), dim.clone());
        }
    }

    fn restyle((selection, frame, style, q): &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx, &sig: &u64, ents: &[Entity]) {
        let Some((_, net)) = &frame.at else { return };
        let lit = focus(&cx.sim.world, net, **selection).map(|(l, _)| l);
        let key = (sig, lit.clone());
        if style.0.as_ref() == Some(&key) {
            return;
        }
        let (bright, dim) = (cx.mat(OverlayMat::WaterBright), cx.mat(OverlayMat::WaterDim));
        for (i, e) in ents.iter().enumerate() {
            let Ok((mut vis, mut mat)) = q.get_mut(*e) else { continue };
            match &lit {
                None => *vis = Visibility::Hidden,
                Some(on) => {
                    *vis = Visibility::Visible;
                    mat.0 = if on.contains(&i) { bright.clone() } else { dim.clone() };
                }
            }
        }
        style.0 = Some(key);
    }
}

/// Chevrons along `pts` pointing forward, sliding with time.
fn chevrons(gizmos: &mut Gizmos, pts: &[SVec2], side: f32, time: f32, col: Color) {
    const GAP: f32 = 70.0;
    const SIZE: f32 = 4.5;
    let line = nd_sim::geom::offset_polyline(pts, side);
    let len = nd_sim::geom::path_length(&line);
    if len < GAP {
        return;
    }
    let mut s = (time * 14.0) % GAP + 20.0;
    while s < len - 20.0 {
        let (p, d) = nd_sim::geom::point_along(&line, s);
        let n = SVec2::new(-d.y, d.x);
        let at = |q: SVec2| Vec3::new(q.x, LIFT + 0.9, q.y);
        let tip = p + d * SIZE;
        gizmos.linestrip([at(p - d * SIZE + n * SIZE), at(tip), at(p - d * SIZE - n * SIZE)], col);
        s += GAP;
    }
}

/// The overlay on top of the bands: a crisp core line on every route (so
/// the network reads from any height), chevrons both ways on the routes in
/// focus, posts at their far ends, a ring at the selected dock, and an
/// outline round each boat on them.
fn draw(mut gizmos: Gizmos, mut lit_g: Gizmos<LitGizmos>, mut dim_g: Gizmos<DimGizmos>, sim: Res<Sim>, selection: Res<Selection>, time: Res<Time>, frame: Res<WaterFrame>) {
    let w = &sim.world;
    let Some(_) = (match *selection {
        Selection::Building(b) if is_route_dock(w, b) => Some(()),
        Selection::Ship(_) => Some(()),
        _ => None,
    }) else {
        return;
    };
    let net = frame.get(&sim);
    let Some((lit, dock)) = focus(w, &net, *selection) else { return };
    let t = time.elapsed_secs();
    let at = |p: SVec2, y: f32| Vec3::new(p.x, LIFT + y, p.y);
    let f = sim.frac();
    for (i, l) in net.lines.iter().enumerate() {
        let pts = l.visible();
        if pts.len() < 2 {
            continue;
        }
        if !lit.contains(&i) {
            dim_g.linestrip(pts.iter().map(|&p| at(p, 0.5)), glow(l, 0.18));
            continue;
        }
        lit_g.linestrip(pts.iter().map(|&p| at(p, 0.8)), glow(l, 1.0));
        // Out along the right-hand side, back along the left, from the
        // selected dock's end (the line's own `from` for a ship).
        let from_here = dock.is_none_or(|d| l.from == d);
        let (out, back): (Vec<SVec2>, Vec<SVec2>) = if from_here { (pts.clone(), pts.iter().rev().copied().collect()) } else { (pts.iter().rev().copied().collect(), pts.clone()) };
        chevrons(&mut gizmos, &out, -4.0, t, glow(l, 1.1));
        chevrons(&mut gizmos, &back, -4.0, t, glow(l, 0.6));
        // The far end: a post and rings (the route's last point in sight
        // for a town off the map), the near end a smaller ring.
        let far = *out.last().unwrap();
        let near = out[0];
        gizmos.line(at(far, 0.0), at(far, 30.0), glow(l, 1.2));
        gizmos.circle(flat_at(at(far, 0.6)), 12.0, glow(l, 1.0));
        gizmos.circle(flat_at(at(far, 30.0)), 4.0, glow(l, 1.0));
        if matches!(l.to, LineEnd::Town(_) | LineEnd::Elsewhere) && from_here {
            // Off the map: a second ring, wider, where it leaves sight.
            gizmos.circle(flat_at(at(far, 0.6)), 20.0, glow(l, 0.5));
        }
        gizmos.circle(flat_at(at(near, 0.6)), 8.0, glow(l, 0.8));
        // Boats on the route (a selected one is the selection's to draw:
        // `draw_ship`).
        for (sid, s) in w.ports.ships.iter().filter(|(_, s)| l.carries(s)) {
            if *selection == Selection::Ship(sid) {
                continue;
            }
            let (p, h) = ship_pose(s, f);
            vehicle_box(&mut gizmos, p, h, s.length() + 4.0, ship_width(s) + 3.0, LIFT + 1.0, glow(l, 0.9));
        }
    }
    if let Some(b) = dock {
        if let Some(port) = w.ports.port_of(b) {
            let pulse = (t * 3.0).sin() * 0.5 + 0.5;
            gizmos.circle(flat_at(at(port.berth, 0.8)), 16.0 + pulse * 5.0, GOLD);
            gizmos.line(at(port.berth, 0.0), at(port.berth, 36.0), GOLD);
        }
    }
}

/// [overlay] A selected ferry or freighter, outlined in gold: on its line
/// where that line shows on the map (none where it does not), or on its own
/// when it is on no line (a fishing boat, or a route since gone).
pub fn draw_ship(gizmos: &mut Gizmos, w: &World, net: &WaterNet, id: ShipId, f: f32) {
    let Some(s) = w.ports.ships.get(id) else { return };
    let shown = match net.of_ship(s) {
        None => true,
        Some(i) => net.lines[i].visible().len() >= 2,
    };
    if shown {
        let (p, h) = ship_pose(s, f);
        vehicle_box(gizmos, p, h, s.length() + 4.0, ship_width(s) + 3.0, LIFT + 1.0, GOLD);
    }
}

/// Names at the far ends of the routes in focus, the same size at any zoom.
fn labels(mut contexts: bevy_egui::EguiContexts, sim: Res<Sim>, selection: Res<Selection>, mode: Res<crate::camera::CameraMode>, flags: Res<crate::ui::UiFlags>, cams: Query<(&Camera, &GlobalTransform)>, frame: Res<WaterFrame>) -> Result {
    let w = &sim.world;
    let wants = match *selection {
        Selection::Building(b) => is_route_dock(w, b),
        Selection::Ship(_) => true,
        _ => false,
    };
    if !wants || mode.street() {
        return Ok(());
    }
    let net = frame.get(&sim);
    let Some((lit, dock)) = focus(w, &net, *selection) else { return Ok(()) };
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let clip = flags.map_rect(ctx.content_rect());
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("ferry_route_labels"))).with_clip_rect(clip);
    for i in lit {
        let l = &net.lines[i];
        let pts = l.visible();
        let Some(&last) = pts.last() else { continue };
        let first = pts[0];
        // Seen from the selected dock: the other end; for a ship, both.
        let ends: Vec<(SVec2, String)> = match dock {
            Some(d) => {
                let end = l.other_end(d);
                let p = if l.from == d { last } else { first };
                vec![(p, end_label(w, d, end, l))]
            }
            None => vec![(first, w.buildings.get(l.from).map_or_else(String::new, |b| b.name.clone())), (last, end_label(w, l.from, l.to, l))],
        };
        for (p, text) in ends {
            let Ok(s) = camera.world_to_viewport(gt, Vec3::new(p.x, LIFT + 32.0, p.y)) else { continue };
            let s = crate::ui::theme::to_points(ctx, s);
            if !clip.expand(40.0).contains(s) {
                continue;
            }
            let col = line_color32(l);
            let g = painter.layout_no_wrap(text, crate::ui::theme::small(), egui::Color32::WHITE);
            let rect = egui::Rect::from_center_size(s - egui::vec2(0.0, g.size().y), g.size()).expand2(egui::vec2(6.0, 2.0));
            painter.rect_filled(rect, 3.0, egui::Color32::from_rgba_unmultiplied(12, 14, 24, 215));
            painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.5, col), egui::StrokeKind::Outside);
            painter.galley(rect.min + egui::vec2(6.0, 2.0), g, egui::Color32::WHITE);
        }
    }
    Ok(())
}

/// "Westholm (off the map, west)", a terminal's name, "Open sea".
fn end_label(w: &World, from: BuildingId, end: LineEnd, l: &WaterLine) -> String {
    match end {
        LineEnd::Town(k) => {
            let edge = w.ferry_towns(l.from).get(k as usize).map_or("", |t| t.edge.label());
            format!("{} (off the map, {edge})", w.ferry_town_name(from, k))
        }
        LineEnd::Elsewhere => "to the open sea".into(),
        LineEnd::Terminal(t) => w.buildings.get(t).map_or_else(|| "a terminal".into(), |b| b.name.clone()),
    }
}
