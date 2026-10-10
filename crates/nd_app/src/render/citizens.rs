//! Citizens on foot: figures, poses, highlight.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_citizens(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    selection: Res<Selection>,
    lighting: Res<Lighting>,
    mut state: ResMut<RenderState>,
    mut people: ResMut<people::PeopleRender>,
    mut stats: ResMut<RenderStats>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    tools: Res<crate::tools::ToolState>,
    mut q: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>, &mut Mesh3d, Option<&swap::SwapOriginal>, Option<&mut WayHeight>, Has<PlatformTag>), With<CitizenTag>>,
) {
    let lifted = lifts(*selection);
    // [pt-transit] Metro riders wait on the platform below the street:
    // drawn there in the Underground view only.
    let under = tools.depth == crate::tools::Depth::Underground;
    let shown = |c: &nd_sim::Citizen| c.is_visible() || (under && c.on_platform());
    let w = &sim.world;
    let f = sim.frac();
    let plain = people::plain();
    // [looks] One lit material for every figure; its trim glows at night.
    let look_mat = people.material(&mut materials, &mut images);
    people.set_night(night_strength(&sim, &lighting), &mut materials);
    people.cache.begin_frame();
    for (id, _) in w.citizens.iter() {
        if !state.citizens.contains_key(&id) {
            let e = commands
                .spawn((Mesh3d(mats.citizen_mesh.clone()), MeshMaterial3d(look_mat.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, CitizenTag, WayHeight::default()))
                .id();
            state.citizens.insert(id, e);
        }
    }
    let gone: Vec<CitizenId> = state.citizens.keys().copied().filter(|id| !w.citizens.contains_key(*id)).collect();
    for id in gone {
        if let Some(e) = state.citizens.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    // Everyone outdoors and their look, first, so no look on screen this
    // frame is evicted to make room for another.
    // Nearest the camera first: they claim figures before the distant.
    let eye = camera.iter().next().map_or(Vec3::ZERO, |t| t.translation());
    let mut outdoors: Vec<(CitizenId, Entity, nd_procgen::people::PersonKey, f32, f32)> = state
        .citizens
        .iter()
        .filter_map(|(id, &e)| w.citizens.get(*id).filter(|c| shown(c)).map(|c| (*id, e, people::figure_of(c), c.pos.distance_squared(SVec2::new(eye.x, eye.z)))))
        .map(|(id, e, (k, s), d)| (id, e, k, s, d))
        .collect();
    outdoors.sort_by(|a, b| a.4.total_cmp(&b.4));
    let mut protected: HashSet<nd_procgen::people::PersonKey> = HashSet::new();
    for o in &outdoors {
        if protected.len() >= people::MESH_CAP {
            break;
        }
        if protected.insert(o.2) {
            people.cache.touch(&o.2);
        }
    }
    let mut looks: HashSet<nd_procgen::people::PersonKey> = HashSet::new();
    let down = w.down_on_street();
    for &(id, e, key, stature, _) in &outdoors {
        let Some(c) = w.citizens.get(id) else { continue };
        let Ok((mut tf, mut vis, _, mut mesh, _, way, platform)) = q.get_mut(e) else { continue };
        if platform != c.on_platform() {
            if platform {
                commands.entity(e).try_remove::<PlatformTag>();
            } else {
                commands.entity(e).try_insert(PlatformTag);
            }
        }
        *vis = Visibility::Visible;
        let pos = if c.prev_pos.distance(c.pos) < 30.0 { c.prev_pos.lerp(c.pos, f) } else { c.pos };
        let heading = lerp_angle(c.prev_heading, c.heading, f);
        // [clarity] Benches and kerbs sit; the hurt (and the homeless
        // sleeping rough) lie flat on the ground.
        let pose = w.pose_with(id, &down);
        let seated = matches!(pose, nd_sim::world::Pose::Bench | nd_sim::world::Pose::SittingKerb);
        let lying = pose == nd_sim::world::Pose::Lying;
        // Thrown clear of the wreck: beside it, not under it.
        let pos = if lying && down.contains(&id) { pos + SVec2::new(-c.heading.sin(), c.heading.cos()) * 2.6 } else { pos };
        // [tools] Up the stairs and over a footbridge (a raised walkway).
        let raised = w.walker_level(c, pos);
        // [footbridge] On an elevated path, a viaduct or a bridge: on its deck.
        if let Some(mut way) = way {
            way.0 = w.walker_way_height(c, pos);
        }
        tf.translation = Vec3::new(pos.x, if lying { 0.12 } else if seated { 0.02 } else { 0.16 } + raised, pos.y);
        if c.on_platform() {
            tf.translation.y = 0.16 - nd_sim::subway::DEPTH;
        }
        tf.rotation = Quat::from_rotation_y(-heading);
        if lying {
            tf.rotation *= Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        }
        // On the kerb: lower, hunched forward over the knees.
        let kerb = pose == nd_sim::world::Pose::SittingKerb;
        if kerb {
            tf.rotation *= Quat::from_rotation_x(0.35);
            tf.translation.y -= 0.1;
        }
        tf.scale = if kerb { Vec3::new(1.05, 0.5, 1.25) } else if seated { Vec3::new(1.0, 0.62, 1.15) } else { Vec3::ONE } * stature;
        looks.insert(key);
        let fallback = || if key.ride { mats.bike_mesh.clone() } else { mats.citizen_mesh.clone() };
        let wanted_mesh = if plain { fallback() } else { people.mesh_for(key, &mut meshes).unwrap_or_else(fallback) };
        if plain {
            tf.scale = if seated { Vec3::new(1.0, 0.62, 1.15) } else { Vec3::ONE };
        }
        if mesh.0 != wanted_mesh {
            mesh.0 = wanted_mesh;
        }
    }
    for (id, &e) in state.citizens.iter() {
        let Some(c) = w.citizens.get(*id) else { continue };
        let Ok((_, mut vis, mut mat, mut mesh, heat_original, _, _)) = q.get_mut(e) else { continue };
        if !shown(c) {
            *vis = Visibility::Hidden;
            // Indoors: let go of the figure so the cache can drop it.
            if mesh.0 != mats.citizen_mesh {
                mesh.0 = mats.citizen_mesh.clone();
            }
        }
        let wanted = if lifted.citizen == Some(*id) {
            &mats.citizen_selected
        } else if c.spotlight {
            &mats.citizen_spotlight
        } else if plain {
            &mats.citizen
        } else {
            &look_mat
        };
        // Under heat vision the entity shows a stand-in; what it stands in
        // for is what counts.
        if heat_original.map_or(&mat.0, |h| &h.0) != wanted {
            mat.0 = wanted.clone();
        }
    }
    stats.people_meshes = people.cache.len();
    stats.people_visible = outdoors.len();
    stats.people_looks_visible = looks.len();
    stats.people_built = people.cache.built;
    stats.people_borrowed = people.cache.borrowed;
}
