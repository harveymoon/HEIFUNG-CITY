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

/// Every station's platform: where it lies and which way (along the first
/// line calling there), its street entrance; stations no line calls at
/// yet still get their platform box.
fn station_sites(w: &nd_sim::World) -> Vec<(nd_sim::BuildingId, PlatformSite)> {
    let mut sites: Vec<(nd_sim::BuildingId, PlatformSite)> = Vec::new();
    for (_, l) in &w.subway.lines {
        for (k, &s) in l.stations.iter().enumerate() {
            if sites.iter().any(|(_, x)| w.metro_platform(s).is_some_and(|p| p.distance(x.at) < 1.0)) {
                continue;
            }
            let Some(_) = w.metro_platform(s) else { continue };
            let at = nd_sim::rail::point_along(&l.path, l.stops.get(k).copied().unwrap_or(0.0));
            let entrance = w.building_entrance(s).unwrap_or(at.0);
            sites.push((s, PlatformSite { at: at.0, dir: at.1, entrance, colour: l.color }));
        }
    }
    for s in w.metro_stations() {
        let Some(p) = w.metro_platform(s) else { continue };
        if sites.iter().any(|(_, x)| x.at.distance(p) < 60.0) {
            continue;
        }
        let entrance = w.building_entrance(s).unwrap_or(p);
        let dir = (entrance - p).normalize_or_zero().perp();
        sites.push((s, PlatformSite { at: p, dir: if dir == SVec2::ZERO { SVec2::X } else { dir }, entrance, colour: 7 }));
    }
    sites
}

/// [pt-ux] Where someone waiting on a platform stands: spread along and
/// across the island platform by a hash of who they are (the same spot
/// every frame), `half_len` and `half_width` in from its ends and edges.
pub(super) fn platform_spot(id_bits: u64, at: SVec2, dir: SVec2, half_len: f32, half_width: f32) -> SVec2 {
    let h = id_bits.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let u = ((h >> 11) & 0xFFFF) as f32 / 65535.0 * 2.0 - 1.0;
    let v = ((h >> 35) & 0xFFFF) as f32 / 65535.0 * 2.0 - 1.0;
    let d = dir.normalize_or_zero();
    at + d * u * half_len + d.perp() * v * half_width
}

/// [pt-ux] The figures waiting on the platforms in the Underground view.
#[derive(Component)]
pub(super) struct PlatformFigure;

/// [pt-ux] People waiting for a metro stand on its platform, drawn in the
/// Underground view (at street level the station is a building and they
/// are inside it): a pool of plain lit figures, one per person waiting,
/// shown only underground.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_platform_figures(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    tools: Res<crate::tools::ToolState>,
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut pool: Local<Vec<Entity>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<PlatformFigure>>,
) {
    if sim.full_rebuild {
        for e in pool.drain(..) {
            commands.entity(e).despawn();
        }
    }
    let under = tools.depth == crate::tools::Depth::Underground;
    let w = &sim.world;
    let mut spots: Vec<Vec3> = Vec::new();
    if under {
        let waiting = crate::metro_ux::waiting_by_station(w);
        if !waiting.is_empty() {
            for (s, site) in station_sites(w) {
                let Some(people) = waiting.get(&s) else { continue };
                let y = ground(w, site.at) - DEPTH + 1.0;
                for c in people.iter().take(160) {
                    let p = platform_spot(slotmap::Key::data(c).as_ffi(), site.at, site.dir, nd_procgen::metro::PLATFORM_LEN * 0.5 - 3.0, 1.2);
                    spots.push(Vec3::new(p.x, y, p.y));
                }
            }
        }
    }
    let (mesh, mat) = look
        .get_or_insert_with(|| {
            (
                meshes.add(Capsule3d::new(0.24, 1.15)),
                materials.add(StandardMaterial { base_color: Color::linear_rgb(0.9, 0.85, 0.75), emissive: LinearRgba::rgb(0.9, 0.75, 0.5), ..default() }),
            )
        })
        .clone();
    while pool.len() < spots.len() {
        pool.push(commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, PlatformFigure)).id());
    }
    for (k, e) in pool.iter().enumerate() {
        let Ok((mut tf, mut v)) = q.get_mut(*e) else { continue };
        match spots.get(k) {
            Some(p) => {
                tf.translation = *p + Vec3::Y * 0.82;
                v.set_if_neq(Visibility::Inherited);
            }
            None => {
                v.set_if_neq(Visibility::Hidden);
            }
        }
    }
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
    let sites: Vec<PlatformSite> = station_sites(w).into_iter().map(|(_, s)| s).collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_spots_stay_on_the_platform_and_still() {
        let at = SVec2::new(100.0, -40.0);
        let dir = SVec2::new(0.6, 0.8);
        for id in 0..500u64 {
            let p = platform_spot(id, at, dir, 42.0, 1.2);
            let rel = p - at;
            assert!(rel.dot(dir).abs() <= 42.0 + 1e-3);
            assert!(rel.dot(dir.perp()).abs() <= 1.2 + 1e-3);
            assert_eq!(p, platform_spot(id, at, dir, 42.0, 1.2));
        }
        // People spread out rather than stacking on one spot.
        let a = platform_spot(1, at, dir, 42.0, 1.2);
        let b = platform_spot(2, at, dir, 42.0, 1.2);
        assert!(a.distance(b) > 0.1);
    }
}
