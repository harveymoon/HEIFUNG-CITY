//! [review] Retaining walls where a corner lot, levelled to its front
//! street, stands over a lower side street (`nd_sim::landscape::Retaining`,
//! `nd_procgen::terrain::retaining_walls`). Built at their own heights (not
//! lifted) and rebuilt only when the walls change. [r15-hillside] With them
//! the floors and sides of the lots cut out of the ground mesh
//! (`nd_procgen::lot_ground`).

use bevy::prelude::*;
use nd_procgen::Surface;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::{despawn_all, to_mesh, Materials};
use crate::assets::GameAssets;
use crate::sim::Sim;

#[derive(Component)]
pub(super) struct RetainingTag;

pub(super) fn sync_retaining(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    existing: Query<Entity, With<RetainingTag>>,
    mut last: Local<Option<u64>>,
) {
    let w = &sim.world;
    let mut h = DefaultHasher::new();
    for r in &w.ground.retaining {
        (r.a.x.to_bits(), r.a.y.to_bits(), r.b.x.to_bits(), r.b.y.to_bits(), r.top.to_bits(), r.foot_a.to_bits(), r.foot_b.to_bits()).hash(&mut h);
    }
    // [r15-hillside] The cut lots' floors and sides follow every pad and
    // the ground drawn round them.
    w.ground.version().hash(&mut h);
    nd_procgen::holes::holes_key(w).hash(&mut h);
    let key = h.finish();
    if *last == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    *last = Some(key);
    despawn_all(&mut commands, &existing);
    if !w.relief {
        return;
    }
    let (mut walls, mut caps) = nd_procgen::terrain::retaining_walls(w, &assets.palette);
    // [r15-hillside] The lots cut out of the ground mesh: their floors and
    // banks with the ground, their tall sides with the walls.
    let (earth, sides) = nd_procgen::lot_ground::lot_ground(w, &assets.palette);
    caps.append(&earth);
    walls.append(&sides);
    if !walls.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&walls, true))), MeshMaterial3d(mats.surface(Surface::Concrete)), Transform::IDENTITY, RetainingTag));
    }
    if !caps.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&caps, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, RetainingTag));
    }
}
