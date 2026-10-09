//! Setting things on the built ground: static meshes once, movers every frame.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

// ── landscape: ground, water, waterworks, ships, and setting things on the ground ──

/// Marker for anything that moves and should stand on the ground (hills,
/// bridge decks): add it to a moving entity and `lift_movers` raises it each
/// frame after its pose is written. Citizens, cars and trains have it implied.
#[derive(Component)]
pub struct Grounded;

/// The lift `lift_movers` last applied, and the height it left the entity
/// at, so a pose not rewritten this frame is not lifted twice.
#[derive(Component)]
pub(super) struct Lifted {
    pub(super) after: f32,
    pub(super) lift: f32,
}

/// [footbridge] The height of the raised way (elevated path, viaduct,
/// bridge) a walker or rider is going along this frame, if any
/// (`World::walker_way_height`): `lift_movers` puts them on it rather than
/// on whatever surface is nearest their last height.
#[derive(Component, Default)]
pub struct WayHeight(pub Option<f32>);

/// Set freshly spawned flat-datum meshes (streets, lot fills, previews,
/// overlays, parks, vacant dressing, stops, scatter) on the built ground,
/// and signal lamps with them. Buildings are raised to their pad when they
/// are generated.
#[allow(clippy::type_complexity)]
pub(super) fn lift_spawned(
    sim: Res<Sim>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<&Mesh3d, Or<(Added<LotsTag>, Added<PreviewTag>, Added<ParkTag>, Added<VacantTag>, Added<OverlayTag>, Added<PathsTag>, Added<TransitTag>, Added<ScatterTag>, Added<RingPreviewTag>)>>,
    tolls: Query<&Mesh3d, Added<TollTag>>,
    mut signals: Query<&mut Transform, Added<SignalTag>>,
) {
    let w = &sim.world;
    if !w.relief {
        return;
    }
    // Toll booths and signal lamps belong to a road: they go at its height
    // (a tunnel junction's lamps stay in the tunnel). Lane arrows are drawn
    // at their road's height already (`street::lane_arrows`).
    let on_road = |p: SVec2| match w.roads.nearest_where(p, |s| !s.is_rail()) {
        Some((sid, t, q)) if q.distance(p) < 25.0 => w.road_height(sid, t),
        _ => w.surface_height(p),
    };
    for m in &tolls {
        let Some(mut mesh) = meshes.get_mut(&m.0) else { continue };
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            for p in pos.iter_mut() {
                p[1] += on_road(SVec2::new(p[0], p[2]));
            }
        }
    }
    // Lots, parks, overlays and the like lie on the ground: under a viaduct
    // they stay down (over the water, a bridge deck is the nearest surface).
    let lie = |p: SVec2| {
        let g = w.ground_height(p);
        w.surface_height_near(p, g)
    };
    for m in &q {
        let Some(mut mesh) = meshes.get_mut(&m.0) else { continue };
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
            for p in pos.iter_mut() {
                p[1] += lie(SVec2::new(p[0], p[2]));
            }
        }
    }
    for mut tf in &mut signals {
        tf.translation.y += on_road(SVec2::new(tf.translation.x, tf.translation.z));
    }
}

/// Raise movers onto the ground after their poses are written.
#[allow(clippy::type_complexity)]
pub(super) fn lift_movers(
    mut commands: Commands,
    sim: Res<Sim>,
    mut surfaces: Local<Option<(u64, u64, u64)>>,
    mut q: Query<(Entity, &mut Transform, Option<&mut Lifted>, Option<Ref<WayHeight>>), (Or<(With<CitizenTag>, With<CarTag>, With<TrainTag>, With<Grounded>)>, Without<SelfLifted>)>,
) {
    let w = &sim.world;
    if !w.relief {
        *surfaces = None;
        return;
    }
    // [refactor/app] Lifting is idempotent: a pose not rewritten since the
    // last lift, on surfaces that have not changed (the built ground, the
    // landscape), would get the same lift again. Skip those, so parked
    // cars, clamps and rubble no longer dirty their transforms every frame.
    let key = (sim.generation, w.ground.version(), w.landscape.version());
    let same_surfaces = *surfaces == Some(key);
    *surfaces = Some(key);
    for (e, mut tf, lifted, way) in &mut q {
        if same_surfaces && lifted.is_some() && !tf.is_changed() && way.as_ref().is_none_or(|h| !h.is_changed()) {
            continue;
        }
        let base = match &lifted {
            // Not rewritten since the last lift: take the old lift off.
            Some(l) if tf.translation.y == l.after => tf.translation.y - l.lift,
            _ => tf.translation.y,
        };
        // Keep to the level it was on: under a viaduct or on it.
        let at = SVec2::new(tf.translation.x, tf.translation.z);
        let lift = match (way.and_then(|h| h.0), &lifted) {
            // [footbridge] On a raised way: its deck, whatever was nearest.
            (Some(h), _) => h,
            (None, Some(l)) => w.surface_height_near(at, l.lift),
            (None, None) => w.surface_height_near(at, w.ground_height(at)),
        };
        tf.translation.y = base + lift;
        let after = tf.translation.y;
        match lifted {
            Some(mut l) => {
                l.after = after;
                l.lift = lift;
            }
            None => {
                commands.entity(e).insert(Lifted { after, lift });
            }
        }
    }
}
