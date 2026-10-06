//! [society] Skid row and the trash, drawn from the simulation's state:
//! camps (tents, tarps, carts) on cheap pavements and vacant lots, the RVs
//! and vans people live in at the kerb, and bins overflowing onto the
//! pavement in front of buildings nobody collects from. Garbage trucks need
//! nothing here: they are cars whose model is the generator's garbage
//! truck, drawn by `sync_cars`.
//!
//! Everything is one merged geometry, rebuilt when its signature changes:
//! a camp's size, a van arriving or leaving, a building's bins crossing an
//! overflow step, or the land under a camp crossing the skid row line (a
//! camp shows only where land value is low).

use super::*;
use nd_procgen::society::{camp, rv, trash_pile};
use nd_sim::homeless::SKID_ROW_LV;

#[derive(Component, Clone)]
pub(super) struct SocietyTag;

#[derive(Resource, Default)]
pub(super) struct SocietyRender {
    key: Option<u64>,
    ents: Vec<Entity>,
}

/// Overflow drawn in steps, so a pile is not rebuilt every hour.
fn overflow_step(o: f32) -> u8 {
    if o <= 0.0 {
        0
    } else {
        (1.0 + o * 3.0).min(8.0) as u8
    }
}

/// Camps fade from view where the land is no longer cheap.
fn camp_visible(w: &World, p: SVec2) -> bool {
    w.land_value.at(p) < SKID_ROW_LV + 0.06
}

pub(super) fn sync_society(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut state: ResMut<SocietyRender>) {
    let w = &sim.world;
    let s = &w.society;
    let mut h = DefaultHasher::new();
    for c in &s.homeless.camps {
        (c.id, c.tents(), c.sidewalk, camp_visible(w, c.pos)).hash(&mut h);
        hash_f32s(&mut h, [c.pos]);
    }
    for r in &s.homeless.rvs {
        (r.seed, r.camper).hash(&mut h);
    }
    for (id, bin) in &s.trash.bins {
        let step = overflow_step(bin.overflow());
        if step > 0 {
            (id, step).hash(&mut h);
        }
    }
    w.landscape.is_flat().hash(&mut h);
    let key = h.finish();
    if state.key == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.key = Some(key);
    for e in state.ents.drain(..) {
        commands.entity(e).despawn();
    }
    let pal = &assets.palette;
    let mut geo = Geometry::default();
    for c in &s.homeless.camps {
        if !camp_visible(w, c.pos) {
            continue;
        }
        let out = if c.out != SVec2::ZERO { c.out } else { c.lot.and_then(|l| w.lots.get(l)).map_or(SVec2::Y, |l| l.front_dir) };
        geo.append(&camp(c.pos, c.dir, out, c.tents(), c.sidewalk, w.config.seed ^ c.id as u64 * 0x9E37, pal));
    }
    for r in &s.homeless.rvs {
        geo.append(&rv(r.pos, r.heading, r.camper, r.seed, pal));
    }
    for (id, bin) in &s.trash.bins {
        let step = overflow_step(bin.overflow());
        if step == 0 {
            continue;
        }
        let Some(b) = w.buildings.get(*id) else { continue };
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let along = (lot.front_edge[1] - lot.front_edge[0]).normalize_or_zero();
        let len = lot.front_edge[0].distance(lot.front_edge[1]);
        // On the pavement, off to one side of the door.
        let p = lot.entrance() + along * (len * 0.28).min(5.0) + lot.front_dir.normalize_or_zero() * 1.2;
        let dumpster = b.homes == 0 || b.business.is_some();
        let seed = w.config.seed ^ (lot.center.x.to_bits() as u64).wrapping_mul(31) ^ lot.center.y.to_bits() as u64;
        geo.append(&trash_pile(p, along, lot.front_dir, step as f32 / 3.0, dumpster, seed, pal));
    }
    if geo.triangle_count() == 0 {
        return;
    }
    // Onto the ground (or a bridge deck) where the landscape is not flat.
    if !w.landscape.is_flat() {
        for part in geo.parts.iter_mut() {
            for p in part.positions.iter_mut() {
                p[1] += w.surface_height(SVec2::new(p[0], p[2]));
            }
        }
    }
    state.ents = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, SocietyTag);
}
