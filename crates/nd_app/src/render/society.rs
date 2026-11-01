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
use nd_procgen::society::{camp, trash_pile};
use nd_sim::homeless::SKID_ROW_LV;

#[derive(Component, Clone)]
pub(super) struct SocietyTag;

#[derive(Resource, Default)]
pub(super) struct SocietyRender {
    key: Option<u64>,
    ents: Vec<Entity>,
    /// [r6-perf] Each camp, van and pile as last built and set on the
    /// ground, by everything it is built from, and the ground and
    /// landscape versions they were raised against (`pieces_on`). A
    /// rebuild makes again only what changed: raising every vertex of a
    /// 38,000-person city's piles took 1.7 s at each bin step.
    pieces: HashMap<u64, Geometry>,
    pieces_on: Option<u64>,
}

/// The key of a piece: what it is built from, hashed.
fn piece_key(parts: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
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
        (c.id, c.tents(), c.sidewalk, c.under, camp_visible(w, c.pos)).hash(&mut h);
        hash_f32s(&mut h, [c.pos]);
    }
    // [pt-growth] An RV shows once it has pulled up.
    let tick = w.clock.tick;
    for r in &s.homeless.rvs {
        (r.seed, r.camper, r.abandoned.is_some(), r.parked(tick)).hash(&mut h);
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
    let on = piece_key((sim.generation, w.ground.version(), w.landscape.version(), w.relief, w.landscape.is_flat()));
    if state.pieces_on != Some(on) || assets.palette_changed || sim.full_rebuild {
        state.pieces.clear();
        state.pieces_on = Some(on);
    }
    let mut pieces = std::mem::take(&mut state.pieces);
    let mut used: HashSet<u64> = HashSet::new();
    let flat = w.landscape.is_flat();
    // A piece from the cache, or built and raised: by `surface_height`
    // (onto a deck where there is one), or by the ground alone.
    let mut piece = |key: u64, deck: bool, make: &dyn Fn() -> Geometry| -> Geometry {
        used.insert(key);
        pieces
            .entry(key)
            .or_insert_with(|| {
                let mut g = make();
                if flat {
                } else if deck {
                    for part in g.parts.iter_mut() {
                        for p in part.positions.iter_mut() {
                            p[1] += w.surface_height(SVec2::new(p[0], p[2]));
                        }
                    }
                } else {
                    for part in g.parts.iter_mut().chain(g.detail.iter_mut().flatten()) {
                        for p in part.positions.iter_mut() {
                            p[1] += w.ground_height(SVec2::new(p[0], p[2]));
                        }
                    }
                }
                g
            })
            .clone()
    };
    let mut geo = Geometry::default();
    // [r5-layered] Camps under a deck: on the ground under it (not up on
    // the deck), tents in a file along it, whatever the land is worth.
    let mut under = Geometry::default();
    for c in &s.homeless.camps {
        if !c.under && !camp_visible(w, c.pos) {
            continue;
        }
        let out = if c.out != SVec2::ZERO { c.out } else { c.lot.and_then(|l| w.lots.get(l)).map_or(SVec2::Y, |l| l.front_dir) };
        let seed = w.config.seed ^ c.id as u64 * 0x9E37;
        let key = piece_key(("camp", c.under, [c.pos.x, c.pos.y, c.dir.x, c.dir.y, out.x, out.y].map(f32::to_bits), c.tents(), c.sidewalk || c.under, seed));
        let g = piece(key, !c.under, &|| camp(c.pos, c.dir, out, c.tents(), c.sidewalk || c.under, seed, pal));
        if c.under {
            under.append(&g);
        } else {
            geo.append(&g);
        }
    }
    for r in s.homeless.rvs.iter().filter(|r| r.parked(tick)) {
        let key = piece_key(("rv", [r.pos.x, r.pos.y, r.heading].map(f32::to_bits), r.camper, r.seed, r.abandoned.is_some()));
        geo.append(&piece(key, true, &|| nd_procgen::society::rv_kept(r.pos, r.heading, r.camper, r.seed, r.abandoned.is_some(), pal)));
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
        let key = piece_key(("pile", [p.x, p.y, along.x, along.y, lot.front_dir.x, lot.front_dir.y].map(f32::to_bits), step, dumpster, seed));
        geo.append(&piece(key, true, &|| trash_pile(p, along, lot.front_dir, step as f32 / 3.0, dumpster, seed, pal)));
    }
    // (Every piece is on the ground, or a bridge deck, already.)
    pieces.retain(|k, _| used.contains(k));
    state.pieces = pieces;
    geo.append(&under);
    if geo.triangle_count() == 0 {
        return;
    }
    state.ents = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, SocietyTag);
}
