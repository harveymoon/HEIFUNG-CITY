//! [tools] Footbridges in the world. [r1-roads] (K20) The Intersection
//! editor used to film the roads on their own layer from a top-down camera
//! here; it now edits on the map itself (`ui::junction::on_map`). The
//! roads keep the layer.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use std::hash::{Hash, Hasher};

use super::*;

/// The editor's render layer.
pub const JUNCTION_LAYER: usize = 6;

/// Footbridge meshes.
#[derive(Component, Clone)]
pub struct FootbridgeTag;

/// Footbridges, rebuilt when the walk graph or the roads change.
pub(super) fn sync_footbridges(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut last: Local<Option<u64>>, existing: Query<Entity, With<FootbridgeTag>>) {
    let w = &sim.world;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (w.walk.version, w.roads.version, w.walk.footbridges.len(), w.relief).hash(&mut h);
    let key = h.finish();
    if *last == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    *last = Some(key);
    despawn_all(&mut commands, &existing);
    if w.walk.footbridges.is_empty() {
        return;
    }
    let geo = nd_procgen::footbridge::footbridges(w, &assets.palette);
    for e in spawn_geometry(&mut commands, &mut meshes, &mats, &geo, FootbridgeTag) {
        commands.entity(e).insert(RenderLayers::from_layers(&[0, JUNCTION_LAYER]));
    }
}

/// Put the roads (and what the editor shows with them) on the editor's
/// layer too.
#[allow(clippy::type_complexity)]
pub(super) fn tag_road_layers(mut commands: Commands, q: Query<Entity, (Or<(With<RoadsTag>, With<LaneArrowsTag>, With<PathsTag>, With<FootbridgeTag>)>, Without<RenderLayers>)>) {
    for e in &q {
        commands.entity(e).insert(RenderLayers::from_layers(&[0, JUNCTION_LAYER]));
    }
}
