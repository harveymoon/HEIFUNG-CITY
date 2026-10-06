//! Map layers beyond land value: traffic load per lane, vehicle types,
//! where people are right now, and pollution and noise. Each is rebuilt on
//! a short real-time timer while it is on and dropped when it is turned off.
//! The colour ramps and the heat grid live in `nd_procgen::overlay`, where
//! they are tested; the legend in `ui::legend` draws the same ramps.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::overlay::{heat_color, pollution_color, traffic_color, traffic_load, HeatGrid, VehicleClass};
use nd_procgen::MeshData;
use nd_sim::traffic::LaneKind;
use nd_sim::{Activity, LandUse, Vec2 as SVec2, World};

use super::{despawn_all, to_mesh, Grounded, Materials, Overlays};
use crate::sim::Sim;
use crate::ui::UiFlags;

#[derive(Component)]
pub(super) struct TrafficLayerTag;
#[derive(Component)]
pub(super) struct PeopleLayerTag;
#[derive(Component)]
pub(super) struct PollutionLayerTag;
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

/// Traffic load per lane: a ribbon down each lane carrying cars, green to
/// red with the load (fullness and slowness), drawn faintly on empty lanes
/// so the network still reads. Rebuilt twice a second while on.
pub(super) fn traffic_ribbons(w: &World) -> MeshData {
    let mut m = MeshData::default();
    for lane in &w.traffic.net.lanes {
        if !matches!(lane.kind, LaneKind::Road { .. }) || lane.points.len() < 2 {
            continue;
        }
        let n = lane.cars.len();
        let mean_v = if n > 0 { lane.cars.iter().filter_map(|c| w.traffic.cars.get(*c)).map(|c| c.v).sum::<f32>() / n as f32 } else { 0.0 };
        let load = traffic_load(n, lane.length, mean_v, lane.speed_limit);
        // Empty lanes are a faint grey so the network reads; lanes with
        // traffic are coloured by load and widen as it builds.
        let color = if n == 0 {
            [0.45, 0.5, 0.6, 0.18]
        } else {
            let c = traffic_color(load);
            [c[0] * 1.3, c[1] * 1.3, c[2] * 1.3, 0.85]
        };
        let width = if n == 0 { 1.0 } else { 1.8 + 1.2 * load };
        m.ground_strip(&lane.points, width, 0.45, color);
    }
    m
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
    keys: Res<ButtonInput<KeyCode>>,
    flags: Res<UiFlags>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    time: Res<Time>,
    mut overlays: ResMut<Overlays>,
    mut timers: Local<[f32; 3]>,
    mut shown: Local<[bool; 3]>,
    mut marker_assets: Local<MarkerAssets>,
    traffic_q: Query<Entity, With<TrafficLayerTag>>,
    people_q: Query<Entity, With<PeopleLayerTag>>,
    pollution_q: Query<Entity, With<PollutionLayerTag>>,
    mut markers: Query<(Entity, &VehicleMarker, &mut Transform)>,
) {
    if keys.just_pressed(KeyCode::KeyL) && !flags.keyboard_captured {
        overlays.cycle();
    }
    let w = &sim.world;
    let now = time.elapsed_secs();
    let on = [overlays.traffic, overlays.people, overlays.pollution];
    let every = [0.5f32, 1.0, 2.0];
    for i in 0..3 {
        let due = on[i] && (!shown[i] || now - timers[i] >= every[i] || sim.full_rebuild);
        if (shown[i] && !on[i]) || due {
            match i {
                0 => despawn_all(&mut commands, &traffic_q),
                1 => despawn_all(&mut commands, &people_q),
                _ => despawn_all(&mut commands, &pollution_q),
            }
            shown[i] = false;
        }
        if !due {
            continue;
        }
        timers[i] = now;
        shown[i] = true;
        match i {
            0 => {
                let mut m = traffic_ribbons(w);
                lift(w, &mut m);
                spawn_layer(&mut commands, &mut meshes, &mats, &m, TrafficLayerTag);
            }
            1 => {
                if let Some(g) = people_grid(w) {
                    // Saturate a little below the peak so a few crowded
                    // towers do not wash everything else out.
                    let mut m = g.mesh(g.max() * 0.6, 0.6, heat_color, |_| 0.0);
                    lift(w, &mut m);
                    spawn_layer(&mut commands, &mut meshes, &mats, &m, PeopleLayerTag);
                }
            }
            _ => {
                if let Some(g) = pollution_grid(w) {
                    let mut m = g.mesh(1.0, 0.7, pollution_color, |_| 0.0);
                    lift(w, &mut m);
                    spawn_layer(&mut commands, &mut meshes, &mats, &m, PollutionLayerTag);
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
        match w.traffic.cars.get(mk.0) {
            Some(car) => {
                let p = car.prev_pos.lerp(car.pos, f);
                tf.translation = Vec3::new(p.x, 0.0, p.y);
                tf.rotation = Quat::from_rotation_y(-car.heading);
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
        let p = car.prev_pos.lerp(car.pos, f);
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
