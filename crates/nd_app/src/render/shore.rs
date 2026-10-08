//! [waterfront] The private jetties businesses on the water build off
//! their lots (`nd_sim::shore`): a fishing company's jetty with its
//! trawler, a works' quay with a crane and a coaster, a restaurant's deck
//! of tables over the water, a boat rental's pontoon. Beaches and the
//! waterfront trades are buildings (`nd_procgen::buildings::shore`) and
//! go with the buildings.
//!
//! One merged geometry at absolute heights (the decking at the lot's pad,
//! the boats on the water), rebuilt when the jetties change or night
//! falls.

use super::*;

#[derive(Component, Clone)]
pub(super) struct JettyTag;

#[derive(Resource, Default)]
pub(super) struct JettyRender {
    key: Option<u64>,
    ents: Vec<Entity>,
}

pub(super) fn sync_jetties(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, mats: Res<Materials>, mut state: ResMut<JettyRender>) {
    let w = &sim.world;
    let night = !(7..20).contains(&w.clock.hour());
    let mut h = DefaultHasher::new();
    night.hash(&mut h);
    for (b, j) in &w.shore.jetties {
        b.hash(&mut h);
        j.kind.hash(&mut h);
        hash_f32s(&mut h, [j.root, j.tip, j.berth]);
        if let Some(x) = w.buildings.get(*b) {
            w.lot_pad(x.lot).to_bits().hash(&mut h);
        }
    }
    let key = h.finish();
    if state.key == Some(key) && !sim.full_rebuild {
        return;
    }
    state.key = Some(key);
    for e in state.ents.drain(..) {
        commands.entity(e).despawn();
    }
    if w.shore.jetties.is_empty() {
        return;
    }
    let mut geo = Geometry::default();
    for (b, j) in &w.shore.jetties {
        let Some(x) = w.buildings.get(*b) else { continue };
        let deck = w.lot_pad(x.lot) + 0.2;
        geo.append(&nd_procgen::buildings::shore::jetty(j, deck, nd_sim::landscape::WATER_LEVEL, night, x.seed));
    }
    state.ents = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, JettyTag);
}
