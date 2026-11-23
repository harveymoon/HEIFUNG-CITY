//! [r14-foodtrucks] Food trucks standing at the kerb (`world.food_trucks`):
//! the van or cart itself (the vehicle mesh in its type's livery), and when
//! it is open for business the stall it puts out: the hatch open and lit,
//! the awning raised over the pavement with bulbs along its edge, an
//! A-board menu, stools and a high table (a cart's umbrella). The lights
//! take the shared glow material, so a truck trading after dark glows.
//! Trucks on the road are cars of the traffic model and drawn with them;
//! at home they are out of sight in the yard.

use super::*;

#[derive(Component)]
pub(super) struct FoodTruckTag;

#[derive(Resource, Default)]
pub(super) struct FoodTruckRender {
    /// Tick and truck count drawn for.
    key: Option<(u64, usize)>,
    /// Each drawn truck: where (decimetres), open, and its entities.
    trucks: HashMap<nd_sim::food_trucks::TruckId, ((i32, i32), bool, Vec<Entity>)>,
    /// The open stall's meshes per type: body, lights.
    stalls: HashMap<u8, (Handle<Mesh>, Handle<Mesh>)>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_food_trucks(mut commands: Commands, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut fr: ResMut<FoodTruckRender>, mut vr: ResMut<VehicleRender>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let w = &sim.world;
    let key = (w.clock.tick, w.food_trucks.trucks.len());
    if fr.key == Some(key) && !sim.full_rebuild {
        return;
    }
    fr.key = Some(key);
    vr.ensure_materials(&mut materials);
    let mut seen: std::collections::HashSet<nd_sim::food_trucks::TruckId> = std::collections::HashSet::new();
    for (id, p, heading, model, open, kind) in w.parked_food_trucks() {
        seen.insert(id);
        let at = ((p.x * 10.0) as i32, (p.y * 10.0) as i32);
        if fr.trucks.get(&id).is_some_and(|(k, o, _)| *k == at && *o == open) && !sim.full_rebuild {
            continue;
        }
        if let Some((_, _, es)) = fr.trucks.remove(&id) {
            for e in es {
                commands.entity(e).despawn();
            }
        }
        let v = vr.variant(&mut meshes, &model, &assets.palette);
        let finish = vr.finish_of(model.class);
        let pose = Transform::from_translation(Vec3::new(p.x, 0.05, p.y)).with_rotation(Quat::from_rotation_y(-heading));
        let mut es = vec![
            commands.spawn((Mesh3d(v.body), MeshMaterial3d(finish.clone()), pose, NotShadowCaster, FoodTruckTag, Grounded)).id(),
            commands.spawn((Mesh3d(v.lights), MeshMaterial3d(mats.glow()), pose, NotShadowCaster, NotShadowReceiver, FoodTruckTag, Grounded)).id(),
        ];
        if open {
            let k = kind as u8;
            let (body, lights) = fr
                .stalls
                .entry(k)
                .or_insert_with(|| {
                    let (b, l) = nd_procgen::vehicles::food_stall(k);
                    (meshes.add(to_mesh(&b, true)), meshes.add(to_mesh(&l, false)))
                })
                .clone();
            es.push(commands.spawn((Mesh3d(body), MeshMaterial3d(finish), pose, NotShadowCaster, FoodTruckTag, Grounded)).id());
            es.push(commands.spawn((Mesh3d(lights), MeshMaterial3d(mats.glow()), pose, NotShadowCaster, NotShadowReceiver, FoodTruckTag, Grounded)).id());
        }
        fr.trucks.insert(id, (at, open, es));
    }
    let gone: Vec<nd_sim::food_trucks::TruckId> = fr.trucks.keys().copied().filter(|id| !seen.contains(id)).collect();
    for id in gone {
        if let Some((_, _, es)) = fr.trucks.remove(&id) {
            for e in es {
                commands.entity(e).despawn();
            }
        }
    }
}
