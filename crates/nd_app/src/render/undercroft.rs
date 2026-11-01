//! [r5-layered] Life under the raised roads, drawn from the simulation's
//! bays (`nd_sim::undercroft`, `World::undercroft`): each bay's floor,
//! soffit lamps, column posters and its stalls, hawkers, lock-ups or court
//! (`nd_procgen::undercroft`), set on the ground under the deck. One merged
//! geometry, rebuilt when a bay changes use or opens and shuts. After dark
//! the lamps glow (`NIGHT_MARK`, on the window material that switches by
//! daylight) and the ones nearest the view hang real sodium lights, so the
//! ground under a flyover reads as lit pools between dark columns.

use super::*;
use nd_procgen::undercroft::{bay, bay_lamps, BayLook};
use nd_sim::undercroft::Bay;

#[derive(Component, Clone)]
pub(super) struct UndercroftTag;

#[derive(Component)]
pub(super) struct UnderLight;

#[derive(Resource, Default)]
pub(super) struct UndercroftRender {
    key: Option<u64>,
    ents: Vec<Entity>,
    /// Every lamp under the decks, world positions, for the real lights.
    lamps: Vec<Vec3>,
}

/// Light of one soffit lamp: a sodium fitting a few metres over the ground.
const UNDER_LUMENS: f32 = 2.2e5;
const UNDER_RANGE: f32 = 11.0;
const UNDER_RGB: [f32; 3] = [1.0, 0.62, 0.3];

/// What a bay is drawn from: the deck's columns at its ends and whether it
/// is open now.
fn look(w: &World, b: &Bay, hour: u8) -> BayLook {
    let reach = b.len * 0.5 + nd_sim::undercroft::COLUMN_CLEAR + 1.5;
    let columns = w
        .pillars
        .iter()
        .filter(|p| {
            let d = p.at - b.pos;
            d.dot(b.dir).abs() < reach && d.dot(b.dir.perp()).abs() < b.width * 0.5 + 1.0
        })
        .map(|p| (p.at, p.dir, (p.half_width * 0.35).clamp(1.2, 3.0)))
        .collect();
    BayLook {
        pos: b.pos,
        dir: b.dir,
        len: b.len,
        width: b.width,
        headroom: b.clearance - 0.25,
        use_: b.use_,
        units: b.use_.units(b.len),
        open: b.use_.open_at(hour) && (!b.use_.is_trade() || !b.holders.is_empty()),
        park: b.park,
        columns,
        seed: w.config.seed ^ b.id,
    }
}

pub(super) fn sync_undercroft(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut state: ResMut<UndercroftRender>) {
    let w = &sim.world;
    let hour = w.clock.hour() as u8;
    let mut h = DefaultHasher::new();
    for b in &w.undercroft.bays {
        (b.id, b.use_, b.park, b.use_.open_at(hour), b.holders.is_empty()).hash(&mut h);
        hash_f32s(&mut h, [b.pos, b.dir, SVec2::new(b.len, b.width), SVec2::new(b.clearance, 0.0)]);
    }
    w.pillars.len().hash(&mut h);
    w.landscape.is_flat().hash(&mut h);
    let key = h.finish();
    if state.key == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.key = Some(key);
    for e in state.ents.drain(..) {
        commands.entity(e).despawn();
    }
    state.lamps.clear();
    let pal = &assets.palette;
    let flat = w.landscape.is_flat();
    let mut geo = Geometry::default();
    for b in &w.undercroft.bays {
        let l = look(w, b, hour);
        let mut g = bay(&l, pal);
        // On the ground under the deck (the deck's own height is what
        // `surface_height` would give).
        let ground = if flat { 0.0 } else { w.ground_height(b.pos) };
        nd_procgen::terrain::raise_geometry(&mut g, ground);
        state.lamps.extend(bay_lamps(&l).into_iter().map(|p| p + Vec3::Y * ground));
        geo.append(&g);
    }
    if geo.triangle_count() == 0 {
        return;
    }
    lights::tag_windows(&mut geo, None);
    for (s, data) in geo.iter().chain(nd_procgen::Lod::DETAIL.into_iter().flat_map(|l| geo.detail_iter(l))) {
        let mesh = Mesh3d(meshes.add(to_mesh(data, !s.is_glow())));
        let e = if s.is_glow() {
            commands.spawn((mesh, MeshMaterial3d(mats.windows.clone()), Transform::IDENTITY, UndercroftTag, NotShadowCaster, NotShadowReceiver)).id()
        } else {
            commands.spawn((mesh, MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, UndercroftTag)).id()
        };
        state.ents.push(e);
    }
}

/// Real sodium light under the decks after dark: a small pool of point
/// lights (no shadows) hung on the soffit lamps nearest the view. None at
/// Low.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_under_lights(
    mut commands: Commands,
    state: Res<UndercroftRender>,
    look: Res<super::sky::SkyLook>,
    stats: Res<super::RenderStats>,
    lighting: Res<super::Lighting>,
    mode: Res<crate::camera::CameraMode>,
    cams: Query<(&GlobalTransform, &crate::camera::OrbitCamera)>,
    mut pool: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<UnderLight>>,
    mut spawned: Local<usize>,
    mut near: Local<Vec<(f32, Vec3)>>,
) {
    let want = (super::street_look::lamp_pool(stats.graphics.tier) / 3).min(12);
    if *spawned < want {
        for _ in *spawned..want {
            commands.spawn((
                PointLight { color: Color::linear_rgb(UNDER_RGB[0], UNDER_RGB[1], UNDER_RGB[2]), intensity: 0.0, range: UNDER_RANGE, radius: 0.3, shadow_maps_enabled: false, ..default() },
                super::offstage(),
                Visibility::Hidden,
                NotShadowCaster,
                UnderLight,
            ));
        }
        *spawned = want;
        return;
    }
    let night = super::street_look::lamp_night(look.daylight);
    near.clear();
    if night > 0.01 && !lighting.heat {
        if let Some((tf, cam)) = cams.iter().next() {
            super::street_look::nearest_lamps(&state.lamps, super::street_look::lamp_focus(*mode, tf.translation(), cam), want, &mut near);
        }
    }
    let mut lit = near.iter();
    for (mut light, mut ltf, mut vis) in &mut pool {
        match lit.next() {
            Some((_, p)) => {
                light.intensity = UNDER_LUMENS * night;
                let at = *p - Vec3::Y * 0.4;
                if ltf.translation != at {
                    ltf.translation = at;
                }
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}
