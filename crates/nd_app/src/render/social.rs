//! [social] What crews and clubs made, drawn from the simulation's state:
//! street-art murals and community gardens on vacant lots
//! (`nd_procgen::social`). One merged geometry, rebuilt when a mural or a
//! garden comes or goes, or the building a mural is on changes.
//!
//! A mural is painted on its building's street face as drawn: the
//! building's geometry is generated again (`render::building_geometry`, the
//! call its chunk uses) and `facade_spot` finds the clear wall facing the
//! street. Only a mural with no building to paint (a vacant lot, or a wall
//! of glass) stands on a free-standing wall. The floodlit night copy of
//! the paint is its own set of meshes, shown only after dark.

use super::*;
use nd_procgen::social::{crew_tag, facade_spot, garden, mural_wall, paint_mural};

#[derive(Component, Clone)]
pub(super) struct SocialTag;

/// The murals' floodlit paint: shown at night only.
#[derive(Component, Clone)]
pub(super) struct MuralNightTag;

#[derive(Resource, Default)]
pub(super) struct SocialRender {
    key: Option<u64>,
    ents: Vec<Entity>,
    night: Vec<Entity>,
    /// Buildings carrying a mural, so a change to one repaints.
    walls: HashSet<BuildingId>,
}

/// Below this daylight the floodlit paint is on.
const NIGHT_BELOW: f32 = 0.35;

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_social(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, sky: Res<sky::SkyLook>, mut state: ResMut<SocialRender>, mut nights: Query<&mut Visibility, With<MuralNightTag>>) {
    // The night paint follows the light every frame (cheap: a handful of entities).
    let show = if sky.daylight < NIGHT_BELOW { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut nights {
        if *v != show {
            *v = show;
        }
    }
    let w = &sim.world;
    let s = &w.social;
    let mut h = DefaultHasher::new();
    for m in &s.murals {
        (m.seed, m.group, (m.quality * 10.0) as i32, m.building).hash(&mut h);
        if let Some(b) = m.building.and_then(|b| w.buildings.get(b)) {
            (b.seed, b.floors, b.tier, b.land_use, b.construction.is_some(), b.demolition.is_some()).hash(&mut h);
        }
    }
    for g in &s.gardens {
        g.seed.hash(&mut h);
    }
    w.landscape.is_flat().hash(&mut h);
    let key = h.finish();
    let touched = sim.events.iter().any(|e| matches!(e, SimEvent::BuildingChanged(id) | SimEvent::BuildingBuilt(id) if state.walls.contains(id)));
    if state.key == Some(key) && !sim.full_rebuild && !touched && !assets.rules_changed && !assets.palette_changed {
        return;
    }
    state.key = Some(key);
    let mut old: Vec<Entity> = std::mem::take(&mut state.ents);
    old.append(&mut state.night);
    for e in old {
        commands.entity(e).despawn();
    }
    state.walls.clear();
    // On the ground: walls on vacant lots and gardens (raised onto the land);
    // on buildings: the facade paint (raised onto the building's pad).
    let mut ground = Geometry::default();
    let mut ground_night = Geometry::default();
    let mut raised = Geometry::default();
    let mut raised_night = Geometry::default();
    let mut walls = HashSet::new();
    for m in &s.murals {
        let crew = m.group as u64;
        let tag = s.group(m.group).map(|g| crew_tag(&g.name)).unwrap_or_default();
        let on_facade = m.building.and_then(|bid| {
            let b = w.buildings.get(bid)?;
            walls.insert(bid);
            // Building works: the wall is scaffolding for now; the mural
            // comes back with the finished face.
            if b.construction.is_some() {
                return Some((Geometry::default(), Geometry::default()));
            }
            let geo = building_geometry(w, &assets, &sim.styles, bid)?;
            // The lot's street edge: the mural's record stands just inside it.
            let front = m.pos + m.facing.normalize_or_zero() * 0.9;
            let spot = facade_spot(&geo, front, m.facing)?;
            let (mut day, mut night) = paint_mural(&spot, m.quality, m.seed, crew, &tag, &assets.palette);
            let pad = w.lot_pad(b.lot);
            nd_procgen::terrain::raise_geometry(&mut day, pad);
            nd_procgen::terrain::raise_geometry(&mut night, pad);
            Some((day, night))
        });
        match on_facade {
            Some((day, night)) => {
                raised.append(&day);
                raised_night.append(&night);
            }
            None => {
                let (day, night) = mural_wall(m.pos, m.along, m.facing, m.width, m.height, m.quality, m.seed, crew, &tag, &assets.palette);
                ground.append(&day);
                ground_night.append(&night);
            }
        }
    }
    state.walls = walls;
    for g in &s.gardens {
        let Some(lot) = w.lots.get(g.lot) else { continue };
        let size = lot.width().min(lot.depth());
        ground.append(&garden(lot.center, lot.front_dir, size, g.seed));
    }
    if !w.landscape.is_flat() {
        for geo in [&mut ground, &mut ground_night] {
            for part in geo.parts.iter_mut() {
                for p in part.positions.iter_mut() {
                    p[1] += w.surface_height(SVec2::new(p[0], p[2]));
                }
            }
        }
    }
    ground.append(&raised);
    ground_night.append(&raised_night);
    if ground.triangle_count() > 0 {
        state.ents = spawn_geometry(&mut commands, &mut meshes, &mats, &ground, SocialTag);
    }
    if ground_night.triangle_count() > 0 {
        state.night = spawn_geometry(&mut commands, &mut meshes, &mats, &ground_night, MuralNightTag);
        for e in &state.night {
            commands.entity(*e).insert(show);
        }
    }
}
