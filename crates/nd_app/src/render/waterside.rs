//! [pt-visual] The waterside's moving parts: the boats moored at the
//! marinas, redrawn as owners take berths and billionaires bring their
//! superyachts (`nd_procgen::terrain::moored_boats`), and the boardwalk
//! promenades along long public beaches (`nd_procgen::buildings::shore::
//! boardwalks`). Each is rebuilt only when what it is drawn from changes.

use bevy::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::{despawn_all, spawn_geometry, Materials};
use crate::assets::GameAssets;
use crate::sim::Sim;

#[derive(Component, Clone)]
pub(super) struct MooredTag;

#[derive(Component, Clone)]
pub(super) struct BoardwalkTag;

pub(super) fn sync_moored_boats(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    existing: Query<Entity, With<MooredTag>>,
    mut last: Local<Option<u64>>,
) {
    let w = &sim.world;
    let mut h = DefaultHasher::new();
    for p in w.ports.ports.values().filter(|p| p.kind.is_marina()) {
        (p.center.x.to_bits(), p.center.y.to_bits(), p.pier[1].x.to_bits(), p.pier[1].y.to_bits(), p.moored).hash(&mut h);
    }
    w.landscape.version().hash(&mut h);
    let key = h.finish();
    if *last == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    *last = Some(key);
    despawn_all(&mut commands, &existing);
    let g = nd_procgen::terrain::moored_boats(w, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &g, MooredTag);
}

pub(super) fn sync_boardwalks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    existing: Query<Entity, With<BoardwalkTag>>,
    mut last: Local<Option<u64>>,
) {
    let w = &sim.world;
    let mut h = DefaultHasher::new();
    for l in w.lots.values().filter(|l| l.shore.beach) {
        (l.center.x.to_bits(), l.center.y.to_bits(), l.polygon.len()).hash(&mut h);
    }
    (w.roads.version, w.relief, w.lots.len()).hash(&mut h);
    let key = h.finish();
    if *last == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    *last = Some(key);
    despawn_all(&mut commands, &existing);
    let g = nd_procgen::buildings::shore::boardwalks(w, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &g, BoardwalkTag);
}
