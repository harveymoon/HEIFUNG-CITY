//! [land] Billionaires on the move: a flashing red and blue light bar on
//! the police car leading the richest one's convoy (the black escort SUVs
//! and the supercar are ordinary car models), and black-suited bodyguards
//! on foot round a billionaire walking about, or standing at the kerb where
//! the convoy dropped them off.

use super::*;

#[derive(Component)]
pub(super) struct ConvoyTag;

#[derive(Resource, Default)]
pub(super) struct ConvoyRender {
    /// Light bar meshes: red side lit, blue side lit.
    bars: Option<[Handle<Mesh>; 2]>,
    guard: Option<Handle<Mesh>>,
    lit: HashMap<CarId, Entity>,
    guards: Vec<Entity>,
}

fn bar_mesh(red_on: bool) -> MeshData {
    let mut m = MeshData::default();
    let (r, b) = if red_on { ([4.0, 0.15, 0.1, 1.0], [0.05, 0.08, 0.4, 1.0]) } else { ([0.4, 0.03, 0.02, 1.0], [0.15, 0.35, 4.0, 1.0]) };
    m.boxed(Vec3::new(-0.3, 0.0, -0.45), Vec3::new(0.5, 0.18, 0.8), 0.0, r);
    m.boxed(Vec3::new(-0.3, 0.0, 0.45), Vec3::new(0.5, 0.18, 0.8), 0.0, b);
    m
}

/// A bodyguard: black suit, white shirt at the collar, dark glasses.
fn guard_mesh() -> MeshData {
    let mut m = MeshData::default();
    let suit = [0.02, 0.02, 0.025, 1.0];
    m.boxed(Vec3::new(-0.11, 0.45, 0.0), Vec3::new(0.16, 0.9, 0.18), 0.0, suit);
    m.boxed(Vec3::new(0.11, 0.45, 0.0), Vec3::new(0.16, 0.9, 0.18), 0.0, suit);
    m.boxed(Vec3::new(0.0, 1.25, 0.0), Vec3::new(0.5, 0.7, 0.28), 0.0, suit);
    m.boxed(Vec3::new(0.0, 1.55, 0.13), Vec3::new(0.14, 0.12, 0.04), 0.0, [0.9, 0.9, 0.9, 1.0]);
    m.boxed(Vec3::new(-0.3, 1.2, 0.0), Vec3::new(0.11, 0.62, 0.14), 0.0, suit);
    m.boxed(Vec3::new(0.3, 1.2, 0.0), Vec3::new(0.11, 0.62, 0.14), 0.0, suit);
    m.boxed(Vec3::new(0.0, 1.78, 0.0), Vec3::new(0.22, 0.26, 0.24), 0.0, [0.42, 0.30, 0.22, 1.0]);
    m.boxed(Vec3::new(0.0, 1.8, 0.12), Vec3::new(0.2, 0.05, 0.03), 0.0, [0.01, 0.01, 0.01, 1.0]);
    m
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_convoys(mut commands: Commands, sim: Res<Sim>, mats: Res<Materials>, mut cr: ResMut<ConvoyRender>, mut meshes: ResMut<Assets<Mesh>>, time: Res<Time>, mut q: Query<(&mut Transform, &mut Mesh3d), With<ConvoyTag>>) {
    let w = &sim.world;
    if w.estate.convoys.is_empty() && w.estate.elites.is_empty() && cr.lit.is_empty() && cr.guards.is_empty() {
        return;
    }
    if cr.bars.is_none() {
        cr.bars = Some([meshes.add(to_mesh(&bar_mesh(true), false)), meshes.add(to_mesh(&bar_mesh(false), false))]);
        cr.guard = Some(meshes.add(to_mesh(&guard_mesh(), false)));
    }
    let [red, blue] = cr.bars.clone().unwrap();
    let guard_mesh = cr.guard.clone().unwrap();
    let f = sim.frac();
    let flash = (time.elapsed_secs() * 3.0) as i64 % 2 == 0;
    // Light bars on the police cars leading a convoy.
    let police: Vec<CarId> = w.estate.convoys.iter().filter_map(|c| c.police).filter(|p| w.traffic.cars.contains_key(*p)).collect();
    let stale: Vec<CarId> = cr.lit.keys().copied().filter(|c| !police.contains(c) || sim.full_rebuild).collect();
    for c in stale {
        if let Some(e) = cr.lit.remove(&c) {
            commands.entity(e).despawn();
        }
    }
    for id in police {
        let car = &w.traffic.cars[id];
        let mut pose = car_pose(car, f, w);
        pose.translation.y += 1.55;
        let mesh = if flash { red.clone() } else { blue.clone() };
        match cr.lit.get(&id).copied() {
            Some(e) => {
                if let Ok((mut tf, mut m)) = q.get_mut(e) {
                    *tf = pose;
                    m.0 = mesh;
                }
            }
            None => {
                let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mats.glow()), pose, NotShadowCaster, NotShadowReceiver, ConvoyTag)).id();
                cr.lit.insert(id, e);
            }
        }
    }
    // Bodyguards: round a billionaire on foot, or at the kerb where the
    // convoy left them, for the hour after.
    let tick = w.clock.tick;
    let mut spots: Vec<(SVec2, f32)> = Vec::new();
    for e in &w.estate.elites {
        let Some(c) = w.citizens.get(e.citizen) else { continue };
        let walking = matches!(c.activity, nd_sim::Activity::Traveling { .. });
        let stop = w.estate.convoys.iter().find(|v| v.citizen == e.citizen).and_then(|v| v.stop).filter(|(_, until)| *until > tick).map(|(p, _)| p);
        let (here, heading) = if walking {
            (if c.prev_pos.distance(c.pos) < 30.0 { c.prev_pos.lerp(c.pos, f) } else { c.pos }, c.heading)
        } else if let Some(p) = stop {
            (p, c.heading)
        } else {
            continue;
        };
        let n = e.guards().max(2) as usize;
        for k in 0..n {
            let a = heading + std::f32::consts::FRAC_PI_4 + k as f32 / n as f32 * std::f32::consts::TAU;
            spots.push((here + SVec2::new(a.cos(), a.sin()) * 1.8, heading));
        }
    }
    if sim.full_rebuild {
        for e in cr.guards.drain(..) {
            commands.entity(e).despawn();
        }
    }
    while cr.guards.len() < spots.len() {
        let e = commands.spawn((Mesh3d(guard_mesh.clone()), MeshMaterial3d(mats.glow()), Transform::from_xyz(0.0, -100.0, 0.0), Visibility::Hidden, ConvoyTag, Grounded)).id();
        cr.guards.push(e);
    }
    for (k, e) in cr.guards.iter().enumerate() {
        match spots.get(k) {
            Some((p, heading)) => {
                let tf = Transform::from_translation(Vec3::new(p.x, 0.0, p.y)).with_rotation(Quat::from_rotation_y(-heading + std::f32::consts::FRAC_PI_2));
                commands.entity(*e).insert((tf, Visibility::Inherited));
            }
            None => {
                commands.entity(*e).insert(Visibility::Hidden);
            }
        }
    }
}
