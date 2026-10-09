//! Map layers beyond land value: traffic load per lane, vehicle types,
//! where people are right now, pollution and noise, and crime. Each is
//! rebuilt on a short real-time timer while it is on and dropped when it is
//! turned off. The Traffic load and Crime layers also dim the city and light
//! up the vehicles they are about (`focus`); [`layer_gizmos`] draws what
//! must show through the ground: cars in tunnels, police routes.
//! The colour ramps and the heat grid live in `nd_procgen::overlay`, where
//! they are tested; the legend in `ui::legend` draws the same ramps.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::overlay::{crime_color, heat_color, pollution_color, traffic_color, traffic_load, HeatGrid, VehicleClass};
use nd_procgen::MeshData;
use nd_sim::traffic::LaneKind;
use nd_sim::{Activity, LandUse, Vec2 as SVec2, World};

use super::{despawn_all, to_mesh, Grounded, Materials, Overlays};
use crate::sim::Sim;

#[derive(Component)]
pub(super) struct TrafficLayerTag;
#[derive(Component)]
pub(super) struct PeopleLayerTag;
#[derive(Component)]
pub(super) struct PollutionLayerTag;
#[derive(Component)]
pub(super) struct CrimeLayerTag;

/// Lines drawn over everything (depth bias -1): cars in tunnels in the
/// Traffic load layer, police routes and calls in the Crime layer.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct XrayGizmos;
/// A coloured disc under one vehicle, for the traffic types layer.
#[derive(Component)]
pub(super) struct VehicleMarker(nd_sim::CarId);

/// The vehicle-type discs: one shared mesh per class (sized to the
/// vehicle) and one unlit material per class.
#[derive(Default)]
pub(super) struct MarkerAssets {
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
}

/// What a vehicle is. Emergency units on a call run with `siren` set.
pub(super) fn vehicle_class(car: &nd_sim::traffic::Car) -> VehicleClass {
    if car.siren {
        VehicleClass::Emergency
    } else if car.bus.is_some() {
        VehicleClass::Bus
    } else if car.cargo.is_some() {
        VehicleClass::Truck
    } else {
        VehicleClass::Car
    }
}

/// The occupied part of the map (every lot, plus a margin).
fn occupied(w: &World) -> Option<(SVec2, SVec2)> {
    let mut lo = SVec2::splat(f32::MAX);
    let mut hi = SVec2::splat(f32::MIN);
    for l in w.lots.values() {
        lo = lo.min(l.center);
        hi = hi.max(l.center);
    }
    for c in w.citizens.values() {
        if !matches!(c.activity, Activity::Arriving) {
            lo = lo.min(c.pos);
            hi = hi.max(c.pos);
        }
    }
    (lo.x <= hi.x).then(|| (lo - SVec2::splat(80.0), hi + SVec2::splat(80.0)))
}

/// Set a flat-datum mesh on the ground.
fn lift(w: &World, m: &mut MeshData) {
    if w.landscape.is_flat() {
        return;
    }
    for p in &mut m.positions {
        p[1] += w.surface_height(SVec2::new(p[0], p[2]));
    }
}

fn spawn_layer<T: Component>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, m: &MeshData, tag: T) {
    if m.is_empty() {
        return;
    }
    commands.spawn((Mesh3d(meshes.add(to_mesh(m, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, tag));
}

/// [clarity] Traffic load lines and crash markers, drawn over everything
/// (depth bias -1) so buildings, trees and viaducts never hide them.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct LoadGizmos;

/// [clarity] Traffic load per lane: a line down each lane carrying cars,
/// green to red with the load (fullness and slowness), half clear, and a
/// faint grey on empty lanes so the network still reads. Drawn on top of
/// everything (`LoadGizmos`), at the lane's own height.
pub(super) fn load_lines(w: &World) -> Vec<(Vec<Vec3>, Color)> {
    let mut out = Vec::new();
    for lane in &w.traffic.net.lanes {
        if !matches!(lane.kind, LaneKind::Road { .. }) || lane.points.len() < 2 {
            continue;
        }
        let n = lane.cars.len();
        let mean_v = if n > 0 { lane.cars.iter().filter_map(|c| w.traffic.cars.get(*c)).map(|c| c.v).sum::<f32>() / n as f32 } else { 0.0 };
        let load = traffic_load(n, lane.length, mean_v, lane.speed_limit);
        let color = if n == 0 {
            Color::linear_rgba(0.45, 0.5, 0.6, 0.16)
        } else {
            let c = traffic_color(load);
            Color::linear_rgba(c[0] * 1.6, c[1] * 1.6, c[2] * 1.6, 0.42 + 0.3 * load)
        };
        let pts: Vec<Vec3> = lane.points.iter().map(|&q| Vec3::new(q.x, lane_height(w, lane, q) + 0.6, q.y)).collect();
        out.push((pts, color));
    }
    out
}

/// [clarity] The danger layer's colour for a street with `n` crashes on
/// the record: amber for one, deep red from twice the complaint level.
pub fn danger_color(n: u32) -> [f32; 3] {
    let t = ((n.max(1) - 1) as f32 / (2 * nd_sim::world::journeys::DANGEROUS_STREET - 1) as f32).clamp(0.0, 1.0);
    [1.0 + 1.6 * t, 0.85 * (1.0 - t) + 0.08, 0.1]
}

/// [clarity] The Traffic danger layer: every street segment with crashes
/// on the record, thick and coloured by how many (amber to red), each
/// crash a small ring where it happened; over everything. Refreshed every
/// two seconds.
pub(super) fn danger_gizmos(sim: Res<Sim>, overlays: Res<Overlays>, lighting: Res<super::Lighting>, time: Res<Time>, mut cache: Local<(f32, Vec<(Vec3, Vec3, Color)>, Vec<Vec3>)>, mut gizmos: Gizmos<LoadGizmos>) {
    if lighting.heat || overlays.map || !overlays.danger {
        cache.1.clear();
        cache.2.clear();
        return;
    }
    let w = &sim.world;
    let now = time.elapsed_secs();
    if (cache.1.is_empty() && cache.2.is_empty()) || now - cache.0 >= 2.0 || sim.full_rebuild {
        cache.0 = now;
        cache.1.clear();
        for (sid, n) in w.crashes_per_segment() {
            let Some(seg) = w.roads.try_segment(sid) else { continue };
            if !seg.alive {
                continue;
            }
            let (a, b) = w.roads.segment_points(sid);
            let (za, zb) = (w.road_height(sid, 0.0), w.road_height(sid, 1.0));
            let c = danger_color(n);
            let col = Color::linear_rgba(c[0], c[1], c[2], 0.85);
            // Thicker the worse: parallel strokes either side.
            let d = (b - a).normalize_or_zero();
            let side = SVec2::new(-d.y, d.x);
            let strokes = 1 + (n.min(8) / 2) as i32;
            for k in -strokes..=strokes {
                let o = side * (k as f32 * 0.9);
                cache.1.push((Vec3::new(a.x + o.x, za + 1.0, a.y + o.y), Vec3::new(b.x + o.x, zb + 1.0, b.y + o.y), col));
            }
        }
        cache.2 = w.journeys.crash_sites.iter().map(|s| Vec3::new(s.pos.x, w.surface_height(s.pos) + 1.2, s.pos.y)).collect();
    }
    for (a, b, c) in &cache.1 {
        gizmos.line(*a, *b, *c);
    }
    let ring = Color::linear_rgba(3.0, 0.5, 0.2, 0.7);
    for p in &cache.2 {
        gizmos.circle(ground_ring(*p), 2.5, ring);
    }
}

/// [clarity] The Traffic load layer's lines (refreshed twice a second) and
/// a big pulsing marker at every crash still in the road: a ring, a cross
/// and a beacon up into the sky, all over everything.
pub(super) fn load_gizmos(sim: Res<Sim>, overlays: Res<Overlays>, lighting: Res<super::Lighting>, time: Res<Time>, mut cache: Local<(f32, Vec<(Vec<Vec3>, Color)>)>, mut gizmos: Gizmos<LoadGizmos>) {
    if lighting.heat || overlays.map || !overlays.traffic {
        cache.1.clear();
        return;
    }
    let w = &sim.world;
    let now = time.elapsed_secs();
    if cache.1.is_empty() || now - cache.0 >= 0.5 || sim.full_rebuild {
        cache.0 = now;
        cache.1 = load_lines(w);
    }
    for (pts, col) in &cache.1 {
        gizmos.linestrip(pts.iter().copied(), *col);
    }
    let f = sim.frac();
    for (i, car) in w.traffic.cars.values().filter(|c| c.crashed()).enumerate() {
        let (p, _) = car.pose_at(f);
        let y = w.car_height_at(car, p) + 0.8;
        let at = Vec3::new(p.x, y, p.y);
        let pulse = (now * 0.8 + i as f32 * 0.31).fract();
        let red = Color::linear_rgb(3.5, 0.3, 0.15);
        gizmos.circle(ground_ring(at), 12.0, red);
        gizmos.circle(ground_ring(at), 12.0 + 22.0 * pulse, Color::linear_rgba(3.0, 0.4, 0.1, 1.0 - pulse));
        let r = 8.0;
        gizmos.line(at + Vec3::new(-r, 0.0, -r), at + Vec3::new(r, 0.0, r), red);
        gizmos.line(at + Vec3::new(-r, 0.0, r), at + Vec3::new(r, 0.0, -r), red);
        gizmos.line(at, at + Vec3::Y * 40.0, Color::linear_rgba(3.0, 0.5, 0.2, 0.8));
    }
}

/// Where everyone is now: at home, at work, out shopping or on the
/// pavement, as a smooth heat map. People inside count at their building.
pub(super) fn people_grid(w: &World) -> Option<HeatGrid> {
    let (lo, hi) = occupied(w)?;
    let mut g = HeatGrid::covering(lo, hi, 14.0, 24_000);
    for c in w.citizens.values() {
        if matches!(c.activity, Activity::Arriving) {
            continue;
        }
        let p = c.inside.and_then(|b| w.buildings.get(b)).and_then(|b| w.lots.get(b.lot)).map_or(c.pos, |l| l.center);
        g.splat(p, 1.0);
    }
    g.blur();
    g.blur();
    Some(g)
}

/// Crime: the sim's hot spots (offences, decaying by a quarter a day) as a
/// smooth heat map, with the crimes still open counted in where they are.
pub(super) fn crime_grid(w: &World) -> Option<HeatGrid> {
    let (lo, hi) = occupied(w)?;
    let mut g = HeatGrid::covering(lo, hi, 14.0, 24_000);
    for h in &w.society.crime.hotspots {
        g.splat_radius(h.pos, nd_sim::crime::HOTSPOT_MERGE, h.heat);
    }
    for i in w.services.incidents.iter().filter(|i| i.kind == nd_sim::services::IncidentKind::Crime && i.resolved.is_none()) {
        g.splat_radius(i.pos, 35.0, nd_sim::crime::HOTSPOT_HEAT);
    }
    g.blur();
    Some(g)
}

/// Pollution and noise: plants by their pollution radius, industry, and
/// traffic noise around every moving vehicle. Absolute scale: 1 is as bad
/// as next to a coal plant.
pub(super) fn pollution_grid(w: &World) -> Option<HeatGrid> {
    let (lo, hi) = occupied(w)?;
    let mut g = HeatGrid::covering(lo, hi, 20.0, 16_000);
    for b in w.buildings.values() {
        let Some(lot) = w.lots.get(b.lot) else { continue };
        if let Some(k) = b.plant {
            let (radius, weight) = k.pollution();
            if radius > 0.0 {
                g.splat_radius(lot.center, radius, weight.abs() * 2.8);
            }
        } else if b.land_use == LandUse::Industrial && b.construction.is_none() {
            g.splat_radius(lot.center, 70.0, 0.25 + 0.1 * b.floors.min(6) as f32);
        } else if b.land_use == LandUse::Entertainment {
            // Clubs and arenas: noise, mostly.
            g.splat_radius(lot.center, 50.0, 0.18);
        }
    }
    for car in w.traffic.cars.values() {
        let w8 = if car.cargo.is_some() || car.bus.is_some() { 0.07 } else { 0.035 };
        g.splat_radius(car.pos, 35.0, w8);
    }
    g.blur();
    Some(g)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_layers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    time: Res<Time>,
    overlays: Res<Overlays>,
    mut layer_state: Local<([f32; 4], [bool; 4])>,
    mut marker_assets: Local<MarkerAssets>,
    traffic_q: Query<Entity, With<TrafficLayerTag>>,
    people_q: Query<Entity, With<PeopleLayerTag>>,
    pollution_q: Query<Entity, With<PollutionLayerTag>>,
    crime_q: Query<Entity, With<CrimeLayerTag>>,
    mut markers: Query<(Entity, &VehicleMarker, &mut Transform)>,
) {
    let w = &sim.world;
    let now = time.elapsed_secs();
    let (timers, shown) = &mut *layer_state;
    let on = [overlays.traffic, overlays.people, overlays.pollution, overlays.crime];
    let every = [0.5f32, 1.0, 2.0, 1.0];
    for i in 0..4 {
        let due = on[i] && (!shown[i] || now - timers[i] >= every[i] || sim.full_rebuild);
        if (shown[i] && !on[i]) || due {
            match i {
                0 => despawn_all(&mut commands, &traffic_q),
                1 => despawn_all(&mut commands, &people_q),
                2 => despawn_all(&mut commands, &pollution_q),
                _ => despawn_all(&mut commands, &crime_q),
            }
            shown[i] = false;
        }
        if !due {
            continue;
        }
        timers[i] = now;
        shown[i] = true;
        match i {
            // [clarity] Drawn as lines over everything by `load_gizmos`.
            0 => {}
            1 => {
                if let Some(g) = people_grid(w) {
                    // Saturate a little below the peak so a few crowded
                    // towers do not wash everything else out.
                    let mut m = g.mesh(g.max() * 0.6, 0.6, heat_color, |_| 0.0);
                    lift(w, &mut m);
                    spawn_layer(&mut commands, &mut meshes, &mats, &m, PeopleLayerTag);
                }
            }
            2 => {
                if let Some(g) = pollution_grid(w) {
                    let mut m = g.mesh(1.0, 0.7, pollution_color, |_| 0.0);
                    lift(w, &mut m);
                    spawn_layer(&mut commands, &mut meshes, &mats, &m, PollutionLayerTag);
                }
            }
            _ => {
                if let Some(g) = crime_grid(w) {
                    // Scaled to the worst spot, but never below a fresh
                    // offence's heat, so a quiet city does not glow red
                    // over a single theft.
                    let mut m = g.mesh((g.max() * 0.85).max(nd_sim::crime::HOTSPOT_HEAT * 0.35), 0.8, crime_color, |_| 0.0);
                    lift(w, &mut m);
                    spawn_layer(&mut commands, &mut meshes, &mats, &m, CrimeLayerTag);
                }
            }
        }
    }

    // Vehicle type discs follow the vehicles every frame.
    if !overlays.traffic_types {
        for (e, _, _) in &markers {
            commands.entity(e).despawn();
        }
        return;
    }
    if marker_assets.meshes.is_empty() {
        for class in VehicleClass::ALL {
            let r = match class {
                VehicleClass::Car => 3.0,
                VehicleClass::Truck => 4.6,
                VehicleClass::Bus => 6.6,
                VehicleClass::Emergency => 4.0,
            };
            let mut m = MeshData::default();
            let ring: Vec<SVec2> = (0..20).map(|i| {
                let a = i as f32 / 20.0 * std::f32::consts::TAU;
                SVec2::new(a.cos() * r, a.sin() * r)
            }).collect();
            m.ground_polygon(&ring, 0.2, [1.0, 1.0, 1.0, 1.0]);
            marker_assets.meshes.push(meshes.add(super::shared_mesh(&m, false)));
            let c = class.color();
            marker_assets.materials.push(materials.add(StandardMaterial { base_color: Color::linear_rgba(c[0], c[1], c[2], 0.75), unlit: true, alpha_mode: AlphaMode::Blend, ..default() }));
        }
    }
    let f = sim.frac();
    let mut have = std::collections::HashSet::new();
    for (e, mk, mut tf) in &mut markers {
        // [fix/review-app] A loaded city reuses car ids: its markers start
        // over rather than stick to a different car (of another class).
        match w.traffic.cars.get(mk.0).filter(|_| !sim.full_rebuild) {
            Some(car) => {
                // The interpolated pose, heading included, as the car's own.
                let (p, heading) = car.pose_at(f);
                tf.translation = Vec3::new(p.x, 0.0, p.y);
                tf.rotation = Quat::from_rotation_y(-heading);
                have.insert(mk.0);
            }
            None => commands.entity(e).despawn(),
        }
    }
    for (id, car) in w.traffic.cars.iter() {
        if have.contains(&id) {
            continue;
        }
        let k = vehicle_class(car).index();
        let p = car.pose_at(f).0;
        commands.spawn((
            Mesh3d(marker_assets.meshes[k].clone()),
            MeshMaterial3d(marker_assets.materials[k].clone()),
            Transform::from_xyz(p.x, 0.0, p.y),
            NotShadowCaster,
            NotShadowReceiver,
            Grounded,
            VehicleMarker(id),
        ));
    }
}

/// Height of the road under a point of a lane: the segment's height there
/// (tunnels and viaducts included), or the junction's for a connector.
fn lane_height(w: &World, lane: &nd_sim::traffic::Lane, p: SVec2) -> f32 {
    if !w.relief {
        return 0.0;
    }
    match lane.kind {
        LaneKind::Road { segment, .. } => {
            if w.roads.try_segment(segment).is_none() {
                return 0.0;
            }
            let (a, b) = w.roads.segment_points(segment);
            let (t, _) = nd_sim::geom::project_on_segment(p, a, b);
            w.road_height(segment, t)
        }
        LaneKind::Connector { node, .. } => w.road_z.get(node.0 as usize).copied().unwrap_or(0.0),
    }
}

/// Is the lane in a bored tunnel?
fn in_tunnel(w: &World, lane: &nd_sim::traffic::Lane) -> bool {
    match lane.kind {
        LaneKind::Road { segment, .. } => w.roads.try_segment(segment).is_some_and(|s| s.tunnel),
        LaneKind::Connector { node, .. } => w.road_z.get(node.0 as usize).copied().unwrap_or(0.0) <= nd_sim::roads::TUNNEL_LEVEL,
    }
}

/// A flat ring on the ground.
fn ground_ring(at: Vec3) -> Isometry3d {
    Isometry3d::new(at, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
}

/// What the focus layers show through everything else:
///
/// - **Traffic load**: every vehicle in a tunnel as a light-blue outline at
///   its place under the ground (the same blue the cars above turn), and
///   the tunnels themselves as faint lines, so the traffic below the city
///   reads with the traffic on it.
/// - **Crime**: each police car's route ahead (flashing red and blue when it
///   runs with lights to a call, a calm cyan on patrol), a ring where it is
///   going, and a pulsing ring on every crime not yet dealt with.
pub(super) fn layer_gizmos(sim: Res<Sim>, overlays: Res<Overlays>, lighting: Res<super::Lighting>, time: Res<Time>, mut gizmos: Gizmos<XrayGizmos>) {
    if lighting.heat || overlays.map || !(overlays.traffic || overlays.crime) {
        return;
    }
    let w = &sim.world;
    let f = sim.frac();
    let lanes = &w.traffic.net.lanes;
    if overlays.traffic && !overlays.crime {
        let blue = super::focus::TRAFFIC_BLUE;
        let car_col = Color::linear_rgb(blue[0] * 1.4, blue[1] * 1.4, blue[2] * 1.4);
        let tunnel_col = Color::linear_rgba(0.25, 0.5, 1.0, 0.35);
        for (sid, s) in w.roads.alive_segments() {
            if !s.tunnel {
                continue;
            }
            let (a, b) = w.roads.segment_points(sid);
            let (za, zb) = (w.road_height(sid, 0.0), w.road_height(sid, 1.0));
            gizmos.line(Vec3::new(a.x, za + 0.3, a.y), Vec3::new(b.x, zb + 0.3, b.y), tunnel_col);
        }
        for car in w.traffic.cars.values() {
            let Some(lane) = car.route.get(car.idx).and_then(|l| lanes.get(*l as usize)) else { continue };
            if !in_tunnel(w, lane) {
                continue;
            }
            let (p, heading) = car.pose_at(f);
            let y = w.car_height_at(car, p) + 1.0;
            let (half_l, half_w) = if car.bus.is_some() { (6.0, 1.4) } else if car.cargo.is_some() { (4.2, 1.3) } else { (2.3, 1.0) };
            let c = crate::overlay::shapes::box_corners(p, heading, half_l, half_w, y);
            gizmos.linestrip([c[0], c[1], c[2], c[3], c[0]], car_col);
            gizmos.line(c[0], c[2], car_col);
            gizmos.line(c[1], c[3], car_col);
        }
    }
    if overlays.crime {
        let t = time.elapsed_secs();
        for i in w.services.incidents.iter().filter(|i| i.kind == nd_sim::services::IncidentKind::Crime && i.resolved.is_none()) {
            let pulse = (t * 0.9 + i.id as f32 * 0.37).fract();
            let at = Vec3::new(i.pos.x, w.surface_height(i.pos) + 0.6, i.pos.y);
            gizmos.circle(ground_ring(at), 4.0 + 14.0 * pulse, Color::linear_rgba(1.8, 0.25, 0.1, 1.0 - pulse));
            gizmos.circle(ground_ring(at), 3.0, Color::linear_rgb(1.6, 0.4, 0.1));
        }
        for u in w.services.units.values() {
            if u.kind != nd_sim::services::UnitKind::PoliceCar {
                continue;
            }
            let col = if u.lights() {
                // Red and blue, alternating as the bar does.
                if (t * 2.5) as i64 % 2 == 0 { Color::linear_rgb(0.3, 0.6, 2.4) } else { Color::linear_rgb(2.2, 0.25, 0.3) }
            } else {
                Color::linear_rgba(0.3, 0.9, 1.3, 0.8)
            };
            let Some(car) = u.car.and_then(|c| w.traffic.cars.get(c)) else {
                if u.out() {
                    let at = Vec3::new(u.pos.x, w.surface_height(u.pos) + 0.6, u.pos.y);
                    gizmos.circle(ground_ring(at), 5.0, col);
                }
                continue;
            };
            let (p, _) = car.pose_at(f);
            let mut pts: Vec<Vec3> = vec![Vec3::new(p.x, w.car_height_at(car, p) + 1.2, p.y)];
            let mut left = 2500.0f32;
            for (k, lid) in car.route.iter().enumerate().skip(car.idx) {
                let Some(lane) = lanes.get(*lid as usize) else { break };
                // On the lane it is on, only the points still ahead of it.
                let ahead = |q: &SVec2| k != car.idx || lane.points.last().is_some_and(|end| (*q - car.pos).dot(*end - car.pos) > 0.0);
                for q in lane.points.iter().filter(|q| ahead(q)) {
                    let last = *pts.last().unwrap_or(&Vec3::ZERO);
                    let v = Vec3::new(q.x, lane_height(w, lane, *q) + 1.2, q.y);
                    left -= last.distance(v);
                    pts.push(v);
                }
                if left <= 0.0 {
                    break;
                }
            }
            if pts.len() >= 2 {
                let end = *pts.last().unwrap_or(&Vec3::ZERO);
                gizmos.linestrip(pts, col);
                gizmos.circle(ground_ring(end), 4.0, col);
            }
        }
    }
}
