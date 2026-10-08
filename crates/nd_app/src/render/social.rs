//! [social] What crews and clubs made, drawn from the simulation's state:
//! street-art murals (a painted wall at the front of the lot) and community
//! gardens on vacant lots (`nd_procgen::social`). One merged geometry,
//! rebuilt when a mural or a garden comes or goes.

use super::*;
use nd_procgen::social::{garden, mural_wall};

#[derive(Component, Clone)]
pub(super) struct SocialTag;

#[derive(Resource, Default)]
pub(super) struct SocialRender {
    key: Option<u64>,
    ents: Vec<Entity>,
}

pub(super) fn sync_social(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, mats: Res<Materials>, mut state: ResMut<SocialRender>) {
    let w = &sim.world;
    let s = &w.social;
    let mut h = DefaultHasher::new();
    for m in &s.murals {
        (m.seed, (m.quality * 10.0) as i32).hash(&mut h);
    }
    for g in &s.gardens {
        g.seed.hash(&mut h);
    }
    w.landscape.is_flat().hash(&mut h);
    let key = h.finish();
    if state.key == Some(key) && !sim.full_rebuild {
        return;
    }
    state.key = Some(key);
    for e in state.ents.drain(..) {
        commands.entity(e).despawn();
    }
    let mut geo = Geometry::default();
    for m in &s.murals {
        geo.append(&mural_wall(m.pos, m.along, m.facing, m.width, m.height, m.quality, m.seed));
    }
    for g in &s.gardens {
        let Some(lot) = w.lots.get(g.lot) else { continue };
        let size = lot.width().min(lot.depth());
        geo.append(&garden(lot.center, lot.front_dir, size, g.seed));
    }
    if geo.triangle_count() == 0 {
        return;
    }
    if !w.landscape.is_flat() {
        for part in geo.parts.iter_mut() {
            for p in part.positions.iter_mut() {
                p[1] += w.surface_height(SVec2::new(p[0], p[2]));
            }
        }
    }
    state.ents = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, SocialTag);
}
