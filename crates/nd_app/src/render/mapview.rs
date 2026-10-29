//! [layers] Map view: the city as a street map. Buildings, people, traffic,
//! trees and street furniture are hidden; a flat map is drawn instead, dark
//! land with each lot tinted lightly by its zone, parks green, water left as
//! it is, and the roads as clean ribbons with a casing (highways amber,
//! avenues gold, streets pale, paths and track thin), tunnels dashed. Over
//! it, each street's name (`nd_sim::streets`) runs along the road, as many
//! times as fit at the current zoom, and the special buildings (services,
//! plants, stations, depots, civic and venues) carry a pin with their name.
//!
//! The map is drawn with an unlit opaque material, layered by small height
//! steps (land, lots, casings, fills), so it reads the same at noon and at
//! midnight. Labels are painted with egui after the 3D pass, decluttered
//! against each other so a zoomed-out map shows the main roads only.

use bevy::camera::visibility::VisibilitySystems;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_procgen::overlay::HeatGrid;
use nd_procgen::MeshData;
use nd_sim::streets::StreetClass;
use nd_sim::roads::RoadKind;
use nd_sim::{LandUse, Vec2 as SVec2, World};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::{despawn_all, to_mesh, Overlays, WaterTag};
use crate::sim::{Sim, SimSet};

pub(super) struct MapViewPlugin;

impl Plugin for MapViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_map.in_set(SimSet::Sync))
            .add_systems(PostUpdate, hide_for_map.before(VisibilitySystems::VisibilityPropagate))
            .add_systems(EguiPrimaryContextPass, map_labels);
    }
}

/// The map's own meshes.
#[derive(Component)]
pub(super) struct MapTag;
/// Hidden by the map view, to be shown again when it ends.
#[derive(Component)]
struct MapHidden;

/// Land outside the lots.
const LAND: [f32; 3] = [0.030, 0.034, 0.048];

/// A lot's tint by zone: light, so the roads stay what the eye follows.
pub fn zone_tint(lu: LandUse) -> [f32; 3] {
    match lu {
        LandUse::Residential => [0.085, 0.078, 0.058],
        LandUse::Commercial => [0.052, 0.068, 0.120],
        LandUse::Industrial => [0.100, 0.082, 0.036],
        LandUse::Entertainment => [0.110, 0.045, 0.105],
        LandUse::Civic => [0.035, 0.095, 0.100],
        LandUse::Park => [0.040, 0.135, 0.055],
    }
}

/// The road fills the map draws, for the legend.
pub const ROAD_KEY: [(&str, [f32; 3]); 5] = [("highway", HIGHWAY), ("avenue", AVENUE), ("street", STREET), ("path", PATH), ("railway", RAIL)];
const HIGHWAY: [f32; 3] = [1.05, 0.58, 0.16];
const AVENUE: [f32; 3] = [0.80, 0.68, 0.30];
const STREET: [f32; 3] = [0.46, 0.48, 0.55];
const PATH: [f32; 3] = [0.20, 0.26, 0.21];
const RAIL: [f32; 3] = [0.26, 0.26, 0.29];

/// Fill, casing and width of a road on the map.
fn road_style(s: &nd_sim::Segment) -> ([f32; 3], [f32; 3], f32) {
    match s.kind {
        RoadKind::Path => (PATH, [0.0; 3], 1.4),
        RoadKind::Rail => (RAIL, [0.0; 3], 1.8),
        RoadKind::Street => {
            let width = s.half_width() * 2.0;
            if s.is_highway() || s.ramp {
                (HIGHWAY, [0.32, 0.15, 0.04], width)
            } else if s.lanes() >= 4 || (s.lanes_fwd >= 2 && s.lanes_bwd >= 2) {
                (AVENUE, [0.25, 0.19, 0.07], width)
            } else {
                (STREET, [0.12, 0.13, 0.17], width)
            }
        }
    }
}

fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

/// Set a road piece on the road's height (on top of the ground over a
/// tunnel, so the dashed tunnel shows).
fn lift_road(w: &World, m: &mut MeshData, from: usize, sid: nd_sim::SegmentId, tunnel: bool) {
    if !w.relief {
        return;
    }
    let (a, b) = w.roads.segment_points(sid);
    for p in &mut m.positions[from..] {
        let q = SVec2::new(p[0], p[2]);
        p[1] += if tunnel { w.surface_height(q) } else { w.road_height(sid, nd_sim::geom::project_on_segment(q, a, b).0) };
    }
}

fn disc(c: SVec2, r: f32) -> Vec<SVec2> {
    (0..14).map(|i| c + SVec2::from_angle(i as f32 / 14.0 * std::f32::consts::TAU) * r).collect()
}

/// The whole map as one mesh: land, lots, road casings, road fills.
fn build_map(w: &World) -> MeshData {
    let mut m = MeshData::default();
    // Well past the buildable square, so the map does not end in the sky's
    // haze at the edge of the view.
    let half = (w.config.half_extent + 900.0).max(if w.landscape.is_grown() { w.landscape.land_half() + 300.0 } else { 0.0 });
    // Land: a grid on the ground, open over the water so it shows through.
    let g = HeatGrid::covering(SVec2::splat(-half), SVec2::splat(half), 16.0, 40_000);
    let n = g.cols + 1;
    let water = |k: usize| {
        let p = g.corner(k % n, k / n);
        !w.landscape.is_flat() && w.landscape.is_water(p)
    };
    let land = g.mesh_by(0.12, |_| rgba(LAND), water, |p| if w.relief { w.surface_height(p) } else { 0.0 });
    m.append(&land);
    // Lots, tinted by zone.
    for lot in w.lots.values() {
        if lot.polygon.len() < 3 {
            continue;
        }
        let from = m.positions.len();
        m.ground_polygon(&lot.polygon, 0.22, rgba(zone_tint(lot.land_use)));
        if w.relief {
            for p in &mut m.positions[from..] {
                p[1] += w.surface_height(SVec2::new(p[0], p[2]));
            }
        }
    }
    // Roads: every casing first, then every fill a little higher, so fills
    // run on over the junctions without the casings cutting across them.
    for pass in 0..2 {
        for (sid, s) in w.roads.alive_segments() {
            let (fill, casing, width) = road_style(s);
            let (a, b) = w.roads.segment_points(sid);
            let has_casing = casing != [0.0; 3];
            if pass == 0 && !has_casing {
                continue;
            }
            let (color, width, y) = if pass == 0 { (casing, width + 1.6, 0.34) } else { (fill, width, 0.46) };
            let from = m.positions.len();
            if s.tunnel {
                // Dashed, and dimmer: it runs under what is drawn.
                let len = a.distance(b);
                let steps = (len / 9.0).ceil().max(1.0) as usize;
                for k in (0..steps).step_by(2) {
                    let p0 = a.lerp(b, k as f32 / steps as f32);
                    let p1 = a.lerp(b, ((k + 1) as f32 / steps as f32).min(1.0));
                    m.ground_strip(&[p0, p1], width * 0.7, y, rgba([color[0] * 0.55, color[1] * 0.55, color[2] * 0.55]));
                }
            } else {
                m.ground_strip(&[a, b], width, y, rgba(color));
                // Round the joints so bends and junctions close up.
                if s.kind != RoadKind::Rail {
                    for p in [a, b] {
                        m.ground_polygon(&disc(p, width * 0.5), y, rgba(color));
                    }
                }
            }
            lift_road(w, &mut m, from, sid, s.tunnel);
        }
    }
    m
}

fn map_signature(w: &World) -> u64 {
    let mut h = DefaultHasher::new();
    w.roads.version.hash(&mut h);
    w.lots.len().hash(&mut h);
    for (id, l) in w.lots.iter() {
        id.hash(&mut h);
        (l.land_use as u8).hash(&mut h);
    }
    w.relief.hash(&mut h);
    h.finish()
}

/// Build the map while the view is on (again when roads or lots change),
/// drop it when it goes off.
#[allow(clippy::too_many_arguments)]
fn sync_map(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, sim: Res<Sim>, overlays: Res<Overlays>, existing: Query<Entity, With<MapTag>>, mut built: Local<Option<u64>>, mut mat: Local<Option<Handle<StandardMaterial>>>, mut gate: Local<super::Gate<(u64, u64, u64, bool)>>) {
    if !overlays.map {
        if built.is_some() {
            despawn_all(&mut commands, &existing);
            *built = None;
        }
        return;
    }
    // [refactor/app] Hash the lots only when a counter the signature reads has moved.
    let w = &sim.world;
    if gate.same((sim.generation, w.roads.version, w.lots.version(), w.relief)) && built.is_some() && !sim.full_rebuild {
        return;
    }
    let sig = map_signature(&sim.world);
    if *built == Some(sig) && !sim.full_rebuild {
        return;
    }
    *built = Some(sig);
    despawn_all(&mut commands, &existing);
    let material = mat.get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, fog_enabled: false, ..default() })).clone();
    let m = build_map(&sim.world);
    commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(material), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, MapTag));
}

/// Hide every other mesh while the map is up (water and the sky stay), and
/// show them again after. Runs after every system that sets visibility in
/// the frame, so owners that show their entities each frame (citizens, cars)
/// cannot bring them back.
#[allow(clippy::type_complexity)]
fn hide_for_map(mut commands: Commands, overlays: Res<Overlays>, mut was_on: Local<bool>, mut q: Query<(Entity, &mut Visibility, Has<MapHidden>), (With<Mesh3d>, Without<MapTag>, Without<WaterTag>, Without<super::sky::SkyDome>)>) {
    if overlays.map {
        *was_on = true;
        for (e, mut v, marked) in &mut q {
            if *v != Visibility::Hidden {
                *v = Visibility::Hidden;
                if !marked {
                    commands.entity(e).try_insert(MapHidden);
                }
            }
        }
    } else if *was_on {
        *was_on = false;
        for (e, mut v, marked) in &mut q {
            if marked {
                *v = Visibility::Inherited;
                commands.entity(e).try_remove::<MapHidden>();
            }
        }
    }
}

/// The pins, for the legend: glyph, colour, what it marks.
pub const PIN_KEY: [(&str, [u8; 3], &str); 7] = [
    ("P", [70, 130, 255], "police"),
    ("F", [255, 90, 60], "fire"),
    ("H", [255, 80, 120], "health"),
    ("\u{26A1}", [255, 220, 60], "power"),
    ("\u{24C2}", [90, 200, 255], "station"),
    ("\u{2605}", [255, 90, 230], "venue"),
    ("S", [255, 210, 80], "school"),
];

/// What a pin says a building is: a letter in the pin, its colour, and a
/// priority (lower first when pins crowd).
fn pin_of(b: &nd_sim::Building) -> Option<(&'static str, [u8; 3], u8)> {
    use nd_sim::services::ServiceKind as K;
    if let Some(s) = b.service {
        return Some(match s.kind {
            K::Police => ("P", [70, 130, 255], 0),
            K::Fire => ("F", [255, 90, 60], 0),
            K::Health => ("H", [255, 80, 120], 0),
            K::Dispatch => ("911", [255, 170, 60], 1),
            K::Jail => ("J", [150, 150, 170], 1),
            K::School => ("S", [255, 210, 80], 1),
            K::Monument => ("M", [200, 170, 255], 1),
            K::Stadium => ("A", [255, 80, 220], 1),
            K::TrashDepot => ("T", [140, 160, 110], 2),
            K::Vertiport => ("V", [80, 230, 255], 1),
            K::ParkingEnforcement => ("E", [170, 170, 120], 2),
            K::University => ("U", [120, 200, 255], 1),
            // [attractions] the landmarks: a star in the attractions' pink
            _ => ("\u{2605}", [255, 120, 200], 0),
        });
    }
    if b.plant.is_some() {
        return Some(("\u{26A1}", [255, 220, 60], 1));
    }
    if b.rail.is_some() || b.metro {
        return Some(("\u{24C2}", [90, 200, 255], 0));
    }
    if b.port.is_some() {
        return Some(("\u{2693}", [90, 160, 255], 1));
    }
    if b.depot {
        return Some(("B", [80, 220, 160], 2));
    }
    // Plain civic offices are left unpinned: a government quarter would be
    // a carpet of pins. Services above carry the civic buildings that matter.
    match b.land_use {
        LandUse::Entertainment if b.floors >= 3 || b.jobs >= 25 => Some(("\u{2605}", [255, 90, 230], 3)),
        _ => None,
    }
}

/// Two labels' boxes (centre, half size) overlap?
fn overlaps(a: (egui::Pos2, egui::Vec2), b: (egui::Pos2, egui::Vec2)) -> bool {
    (a.0.x - b.0.x).abs() < a.1.x + b.1.x && (a.0.y - b.0.y).abs() < a.1.y + b.1.y
}

/// Street names along the roads and pins on special buildings.
fn map_labels(mut contexts: EguiContexts, sim: Res<Sim>, overlays: Res<Overlays>, flags: Res<crate::ui::UiFlags>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    if !overlays.map {
        return Ok(());
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let w = &sim.world;
    let ctx = contexts.ctx_mut()?;
    // [city-data] The map is covered: nothing to label.
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    // Clear of the top bar and the toolbar with its sub-pane, so no name
    // runs under them.
    let full = ctx.content_rect();
    // [fonts] The bars' real rectangles (they grow with the text), and the
    // painter clipped to them, so a long name cannot run under a bar either.
    let screen = flags.map_rect(full).shrink(4.0);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("map_labels"))).with_clip_rect(screen);
    let to_screen = |p: SVec2, y: f32| camera.world_to_viewport(gt, Vec3::new(p.x, y, p.y)).ok().map(|v| crate::ui::theme::to_points(ctx, v));
    let mut taken: Vec<(egui::Pos2, egui::Vec2)> = Vec::new();

    // Pins first: they matter most and are fewest.
    let mut pins: Vec<(u8, SVec2, &str, &'static str, [u8; 3])> = Vec::new();
    for b in w.buildings.values() {
        let Some((glyph, col, prio)) = pin_of(b) else { continue };
        let Some(lot) = w.lots.get(b.lot) else { continue };
        pins.push((prio, lot.center, b.name.as_str(), glyph, col));
    }
    pins.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(b.2)));
    let name_font = crate::ui::theme::small();
    for (_, at, name, glyph, col) in pins {
        let y = if w.relief { w.surface_height(at) } else { 0.0 } + 0.5;
        let Some(tip) = to_screen(at, y) else { continue };
        if !screen.contains(tip) {
            continue;
        }
        let head = tip - egui::vec2(0.0, 19.0);
        let galley = painter.layout_no_wrap(name.to_string(), name_font.clone(), egui::Color32::from_rgb(235, 238, 250));
        let text_at = head + egui::vec2(14.0, -galley.size().y * 0.5);
        let bx = (egui::pos2(head.x + (galley.size().x + 12.0) * 0.5 - 4.0, head.y), egui::vec2((galley.size().x + 24.0) * 0.5, 11.0));
        let pin_box = (head, egui::vec2(10.5, 20.0));
        if taken.iter().any(|t| overlaps(*t, pin_box)) {
            continue;
        }
        // Room for the name beside it? (Checked before the pin itself is
        // taken, which the name's box would always overlap.)
        let name_fits = !taken.iter().any(|t| overlaps(*t, bx));
        let c = egui::Color32::from_rgb(col[0], col[1], col[2]);
        // The pin: a round head on a point, dark-rimmed.
        painter.add(egui::Shape::convex_polygon(vec![head + egui::vec2(-5.5, 3.5), tip, head + egui::vec2(5.5, 3.5)], c, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(200))));
        painter.circle(head, 9.5, c, egui::Stroke::new(1.2, egui::Color32::from_black_alpha(220)));
        painter.text(head, egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(if glyph.len() > 1 && glyph.is_ascii() { 9.0 } else { 12.0 }), egui::Color32::from_rgb(12, 14, 22));
        taken.push(pin_box);
        // The name beside it, if there is room.
        if name_fits {
            for off in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, 1.0), egui::vec2(1.0, -1.0), egui::vec2(-1.0, -1.0)] {
                painter.add(egui::epaint::TextShape::new(text_at + off, galley.clone(), egui::Color32::BLACK).with_override_text_color(egui::Color32::from_black_alpha(230)));
            }
            painter.add(egui::epaint::TextShape::new(text_at, galley, egui::Color32::WHITE));
            taken.push(bx);
        }
    }

    // Street names: main roads first, longest first, each repeated along
    // its length as often as there is room on screen.
    let mut order: Vec<usize> = (0..w.streets.streets.len()).collect();
    let rank = |c: StreetClass| match c {
        StreetClass::Highway => 0,
        StreetClass::Avenue => 1,
        StreetClass::Ring => 2,
        StreetClass::Street => 3,
    };
    let lens: Vec<f32> = w.streets.streets.iter().map(|s| s.length()).collect();
    order.sort_by(|a, b| rank(w.streets.streets[*a].class).cmp(&rank(w.streets.streets[*b].class)).then(lens[*b].total_cmp(&lens[*a])).then(a.cmp(b)));
    for i in order {
        let st = &w.streets.streets[i];
        if st.points.len() < 2 || st.name.is_empty() {
            continue;
        }
        let tunnel = st.segments.first().and_then(|s| w.roads.try_segment(*s)).is_some_and(|s| s.tunnel);
        // The polyline on screen (points behind the camera break it).
        let pts: Vec<Option<egui::Pos2>> = st
            .points
            .iter()
            .map(|p| {
                let y = if w.relief { w.surface_height(*p) } else { 0.0 } + 0.6;
                to_screen(*p, y)
            })
            .collect();
        let (size, color) = match st.class {
            StreetClass::Highway => (15.0, egui::Color32::from_rgb(255, 200, 130)),
            StreetClass::Avenue => (14.0, egui::Color32::from_rgb(250, 230, 170)),
            _ => (crate::ui::theme::SMALL, if tunnel { egui::Color32::from_gray(150) } else { egui::Color32::from_rgb(220, 225, 240) }),
        };
        let galley = painter.layout_no_wrap(st.name.clone(), crate::ui::theme::display(size), color);
        let tw = galley.size().x;
        let th = galley.size().y;
        // Runs of consecutive visible points.
        let mut run: Vec<egui::Pos2> = Vec::new();
        let mut runs: Vec<Vec<egui::Pos2>> = Vec::new();
        for p in pts {
            match p {
                Some(p) if screen.expand(200.0).contains(p) => run.push(p),
                _ => {
                    if run.len() >= 2 {
                        runs.push(std::mem::take(&mut run));
                    }
                    run.clear();
                }
            }
        }
        if run.len() >= 2 {
            runs.push(run);
        }
        let spacing = (tw * 3.0).max(320.0);
        for r in runs {
            let cum: Vec<f32> = std::iter::once(0.0).chain(r.windows(2).scan(0.0, |acc, s| {
                *acc += s[0].distance(s[1]);
                Some(*acc)
            })).collect();
            let total = *cum.last().unwrap_or(&0.0);
            if total < tw + 24.0 {
                continue;
            }
            let at = |d: f32| {
                let k = cum.partition_point(|c| *c < d).clamp(1, r.len() - 1);
                let seg = (cum[k] - cum[k - 1]).max(1e-3);
                r[k - 1] + (r[k] - r[k - 1]) * ((d - cum[k - 1]) / seg).clamp(0.0, 1.0)
            };
            let count = ((total / spacing).floor() as usize).max(1);
            for j in 0..count {
                let mid = total * (j as f32 + 0.5) / count as f32;
                let (p0, p1) = (at((mid - tw * 0.5).max(0.0)), at((mid + tw * 0.5).min(total)));
                let centre = at(mid);
                if !screen.contains(centre) {
                    continue;
                }
                let d = p1 - p0;
                // Too bent under the label to read: skip this spot.
                if d.length() < tw * 0.8 {
                    continue;
                }
                let mut angle = d.y.atan2(d.x);
                if angle > std::f32::consts::FRAC_PI_2 {
                    angle -= std::f32::consts::PI;
                } else if angle < -std::f32::consts::FRAC_PI_2 {
                    angle += std::f32::consts::PI;
                }
                let (s, c) = angle.sin_cos();
                let half = egui::vec2((tw * c.abs() + th * s.abs()) * 0.5, (tw * s.abs() + th * c.abs()) * 0.5);
                let bx = (centre, half);
                if taken.iter().any(|t| overlaps(*t, bx)) {
                    continue;
                }
                taken.push(bx);
                let pos = centre - galley.size() * 0.5;
                for off in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, 1.0), egui::vec2(1.0, -1.0), egui::vec2(-1.0, -1.0)] {
                    painter.add(egui::epaint::TextShape::new(pos + off, galley.clone(), egui::Color32::BLACK).with_override_text_color(egui::Color32::from_black_alpha(220)).with_angle_and_anchor(angle, egui::Align2::CENTER_CENTER));
                }
                painter.add(egui::epaint::TextShape::new(pos, galley.clone(), color).with_angle_and_anchor(angle, egui::Align2::CENTER_CENTER));
            }
        }
    }
    Ok(())
}
