//! [subway] The metro in the Underground view: the bores, platforms and
//! shafts (`nd_procgen::metro::tunnels`, redrawn when a station or line
//! changes) and the trains running in them. Both carry `TunnelTag`, so
//! `holo::apply_depth` shows them only underground; at street level the
//! entrances are ordinary buildings.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use nd_procgen::metro::{metro_car, tunnels, PlatformSite, TunnelLine};
use nd_procgen::Surface;
use nd_sim::subway::{MetroTrainId, CARS, DEPTH};
use nd_sim::Vec2 as SVec2;
use std::collections::HashMap;

use super::holo::TunnelTag;
use crate::assets::GameAssets;
use super::{shared_mesh, to_mesh, Materials};
use crate::sim::Sim;

/// The tunnels' meshes.
#[derive(Component)]
pub(super) struct MetroTunnelTag;

/// A metro car; which train it belongs to is in [`MetroState`].
#[derive(Component)]
pub(super) struct MetroCarTag;

#[derive(Default)]
pub(super) struct MetroState {
    /// Car meshes per line colour, one per surface, shared by every car.
    cars: HashMap<u8, Vec<(Surface, Handle<Mesh>)>>,
    trains: HashMap<MetroTrainId, Vec<Entity>>,
}

fn ground(w: &nd_sim::World, p: SVec2) -> f32 {
    if w.relief { w.ground_height(p) } else { 0.0 }
}

/// Redraw the tunnels when the subway changes.
pub(super) fn sync_metro_tunnels(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut state: Local<Option<u64>>, existing: Query<Entity, With<MetroTunnelTag>>) {
    let w = &sim.world;
    // Bumped on every edit; the line count guards a load (version restarts).
    let version = w.subway.version.wrapping_add(w.subway.lines.len() as u64 * 7919);
    if *state == Some(version) && !sim.full_rebuild {
        return;
    }
    *state = Some(version);
    for e in &existing {
        commands.entity(e).despawn();
    }
    let lines: Vec<TunnelLine> = w.subway.lines.values().map(|l| TunnelLine { path: &l.path, colour: l.color }).collect();
    let mut sites: Vec<PlatformSite> = Vec::new();
    for (_, l) in &w.subway.lines {
        for (k, &s) in l.stations.iter().enumerate() {
            if sites.iter().any(|x| w.metro_platform(s).is_some_and(|p| p.distance(x.at) < 1.0)) {
                continue;
            }
            let Some(_) = w.metro_platform(s) else { continue };
            let at = nd_sim::rail::point_along(&l.path, l.stops.get(k).copied().unwrap_or(0.0));
            let entrance = w.building_entrance(s).unwrap_or(at.0);
            sites.push(PlatformSite { at: at.0, dir: at.1, entrance, colour: l.color });
        }
    }
    // Stations no line calls at yet still show their platform box.
    for s in w.metro_stations() {
        let Some(p) = w.metro_platform(s) else { continue };
        if sites.iter().any(|x| x.at.distance(p) < 60.0) {
            continue;
        }
        let entrance = w.building_entrance(s).unwrap_or(p);
        let dir = (entrance - p).normalize_or_zero().perp();
        sites.push(PlatformSite { at: p, dir: if dir == SVec2::ZERO { SVec2::X } else { dir }, entrance, colour: 7 });
    }
    if lines.is_empty() && sites.is_empty() {
        return;
    }
    let geo = tunnels(&lines, &sites, &|p| ground(w, p), &assets.palette);
    for (s, data) in geo.iter() {
        let mut e = commands.spawn((Mesh3d(meshes.add(to_mesh(data, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, TunnelTag, MetroTunnelTag, Visibility::Hidden, NotShadowCaster));
        if s.is_glow() {
            e.insert(bevy::light::NotShadowReceiver);
        }
    }
}

/// Metro trains: a set of cars along the line, blended between ticks,
/// on the bore of their direction.
pub(super) fn sync_metro_trains(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut state: Local<MetroState>, mut q: Query<&mut Transform, With<MetroCarTag>>) {
    let w = &sim.world;
    let sub = &w.subway;
    if sim.full_rebuild {
        for (_, ents) in state.trains.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    for (id, t) in sub.trains.iter() {
        if state.trains.contains_key(&id) {
            continue;
        }
        let colour = t.run.color;
        if !state.cars.contains_key(&colour) {
            let g = metro_car(colour, &assets.palette);
            let parts = g.iter().map(|(s, m)| (s, meshes.add(shared_mesh(m, !s.is_glow())))).collect();
            state.cars.insert(colour, parts);
        }
        let parts = state.cars[&colour].clone();
        let mut ents = Vec::new();
        for _ in 0..CARS {
            let parent = commands.spawn((Transform::IDENTITY, Visibility::Hidden, TunnelTag, MetroCarTag)).id();
            for (s, mesh) in &parts {
                let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mats.surface(*s)), Transform::IDENTITY, NotShadowCaster)).id();
                commands.entity(parent).add_child(child);
            }
            ents.push(parent);
        }
        state.trains.insert(id, ents);
    }
    let gone: Vec<MetroTrainId> = state.trains.keys().copied().filter(|id| !sub.trains.contains_key(*id)).collect();
    for id in gone {
        if let Some(ents) = state.trains.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let f = sim.frac();
    for (id, ents) in state.trains.iter() {
        let Some(t) = sub.trains.get(*id) else { continue };
        let s = t.run.prev_s + (t.run.s - t.run.prev_s) * f;
        for (k, e) in ents.iter().enumerate() {
            if let Ok(mut tf) = q.get_mut(*e) {
                let (p, heading) = t.car_pose_at(k, s);
                tf.translation = Vec3::new(p.x, ground(w, p) - DEPTH, p.y);
                tf.rotation = Quat::from_rotation_y(-heading);
            }
        }
    }
}
