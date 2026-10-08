//! [driving] What driving leaves on the street: tyre smoke boiling off the
//! cars spinning donuts in a takeover, the black rings they burn into the
//! junction, smoke from the bonnet of a crashed car (written-off ones sit
//! askew), and the wreck riding on the back of a tow truck. The crowd round
//! a takeover needs nothing here: its people are citizens, drawn as such.

use super::*;
use nd_sim::traffic::StuntKind;

#[derive(Component)]
pub(super) struct StuntTag;

pub(super) struct StuntAssets {
    smoke: Handle<Mesh>,
    bonnet: Handle<Mesh>,
    skid: Handle<Mesh>,
    load: Handle<Mesh>,
    hazard: Handle<StandardMaterial>,
}

#[derive(Default)]
pub(super) struct StuntState {
    /// Tyre smoke per spinning car.
    smoke: HashMap<CarId, Entity>,
    /// Bonnet smoke and the hazard lamp per crashed car.
    crashed: HashMap<CarId, [Entity; 2]>,
    /// The wreck on a tow truck's bed.
    loads: HashMap<CarId, Entity>,
    /// Burned rings, oldest first (kept after the takeover: rubber stays).
    skids: Vec<(SVec2, Entity)>,
}

const MAX_SKIDS: usize = 24;

fn make_assets(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> StuntAssets {
    // Tyre smoke: a low, wide bank of pale puffs, thickest at the wheels.
    let smoke = {
        let mut m = MeshData::default();
        for k in 0..10 {
            let t = k as f32;
            let a = t * 2.2;
            let r = 0.7 + t * 0.18;
            m.boxed(Vec3::new(-1.2 - t * 0.35 + a.cos() * 0.8, 0.35 + t * 0.22, a.sin() * (0.9 + t * 0.15)), Vec3::new(r * 2.2, r * 1.1, r * 2.2), a, [0.75, 0.75, 0.78, (0.5 - t * 0.04).max(0.08)]);
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    // A thin plume from a crumpled bonnet.
    let bonnet = {
        let mut m = MeshData::default();
        for k in 0..6 {
            let t = k as f32;
            let r = 0.25 + t * 0.12;
            m.boxed(Vec3::new(1.4 + t * 0.15, 1.0 + t * 0.45, t * 0.05), Vec3::new(r * 2.0, r * 1.3, r * 2.0), t * 0.7, [0.18, 0.18, 0.2, (0.4 - t * 0.05).max(0.08)]);
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    // A burned ring: dark rubber in overlapping arcs.
    let skid = {
        let mut m = MeshData::default();
        for k in 0..36 {
            let a = k as f32 / 36.0 * std::f32::consts::TAU;
            for (r, w) in [(1.55f32, 0.32f32), (3.25, 0.36)] {
                m.boxed(Vec3::new(a.cos() * r, 0.03, a.sin() * r), Vec3::new(0.62 * r * 0.55, 0.01, w), -a + std::f32::consts::FRAC_PI_2, [0.02, 0.02, 0.02, 0.55]);
            }
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    // A wreck on the bed: a crumpled dark body.
    let load = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(-1.6, 1.25, 0.0), Vec3::new(3.2, 0.7, 1.7), 0.06, [0.16, 0.15, 0.15, 1.0]);
        m.boxed(Vec3::new(-1.8, 1.75, 0.05), Vec3::new(1.6, 0.4, 1.4), -0.1, [0.1, 0.1, 0.11, 1.0]);
        meshes.add(super::shared_mesh(&m, true))
    };
    let hazard = materials.add(StandardMaterial { base_color: Color::linear_rgb(3.0, 1.4, 0.1), emissive: LinearRgba::rgb(9.0, 4.0, 0.3), unlit: true, ..default() });
    StuntAssets { smoke, bonnet, skid, load, hazard }
}

fn despawn_all(commands: &mut Commands, st: &mut StuntState) {
    for (_, e) in st.smoke.drain() {
        commands.entity(e).despawn();
    }
    for (_, es) in st.crashed.drain() {
        for e in es {
            commands.entity(e).despawn();
        }
    }
    for (_, e) in st.loads.drain() {
        commands.entity(e).despawn();
    }
    for (_, e) in st.skids.drain(..) {
        commands.entity(e).despawn();
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_stunts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    state: Res<RenderState>,
    time: Res<Time>,
    mut assets: Local<Option<StuntAssets>>,
    mut st: Local<StuntState>,
    mut q: Query<(&mut Transform, &mut Visibility), With<StuntTag>>,
    mut cars: Query<&mut Transform, (With<CarTag>, Without<StuntTag>)>,
    tools: Res<crate::tools::ToolState>,
) {
    let w = &sim.world;
    if sim.full_rebuild {
        despawn_all(&mut commands, &mut st);
    }
    let traffic = &w.traffic;
    let busy = traffic.cars.values().any(|c| c.in_stunt() || c.crashed()) || !w.tows().is_empty();
    if !busy && st.smoke.is_empty() && st.crashed.is_empty() && st.loads.is_empty() {
        return;
    }
    let a = assets.get_or_insert_with(|| make_assets(&mut meshes, &mut materials));
    let t = time.elapsed_secs();
    let f = sim.frac();

    // Tyre smoke and burned rings under the donut cars.
    let spinning: Vec<(CarId, SVec2, f32, SVec2)> = traffic
        .cars
        .iter()
        .filter_map(|(id, c)| match c.drive.stunt {
            Some(s) if s.settled => match s.kind {
                StuntKind::Donut { centre, .. } => {
                    let (p, h) = c.pose_at(f);
                    Some((id, p, h, centre))
                }
                StuntKind::Ring { .. } | StuntKind::Manoeuvre { .. } => None,
            },
            _ => None,
        })
        .collect();
    let live: HashSet<CarId> = spinning.iter().map(|s| s.0).collect();
    st.smoke.retain(|id, e| {
        let keep = live.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    for &(id, p, h, centre) in &spinning {
        let e = *st.smoke.entry(id).or_insert_with(|| commands.spawn((Mesh3d(a.smoke.clone()), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, StuntTag)).id());
        let y = traffic.cars.get(id).map_or(0.0, |c| w.car_height(c));
        if let Ok((mut tf, _)) = q.get_mut(e) {
            let billow = 1.0 + 0.15 * (t * 4.0 + p.x).sin();
            tf.translation = Vec3::new(p.x, y + 0.02, p.y);
            tf.rotation = Quat::from_rotation_y(-h + 0.2 * (t * 3.0).sin());
            tf.scale = Vec3::new(0.85 * billow, 0.7 * billow, 0.85 * billow);
        }
        if !st.skids.iter().any(|s| s.0.distance(centre) < 1.0) {
            let e = commands.spawn((Mesh3d(a.skid.clone()), MeshMaterial3d(mats.overlay.clone()), Transform::from_translation(Vec3::new(centre.x, w.surface_height(centre) + 0.02, centre.y)), NotShadowCaster, NotShadowReceiver, StuntTag)).id();
            st.skids.push((centre, e));
            if st.skids.len() > MAX_SKIDS {
                let (_, old) = st.skids.remove(0);
                commands.entity(old).despawn();
            }
        }
    }

    // Crashed cars: bonnet smoke, a blinking hazard lamp, and written-off
    // ones knocked askew.
    let crashed: Vec<(CarId, bool)> = traffic.cars.iter().filter_map(|(id, c)| c.drive.crash.map(|s| (id, s.wrecked))).collect();
    let ids: HashSet<CarId> = crashed.iter().map(|c| c.0).collect();
    st.crashed.retain(|id, es| {
        let keep = ids.contains(id);
        if !keep {
            for e in es.iter() {
                commands.entity(*e).despawn();
            }
        }
        keep
    });
    // [render] In the layer its car is seen in: a crash in a tunnel only in
    // the Underground view.
    let under = tools.depth == crate::tools::Depth::Underground;
    // [driving] Wrecks on their side or roof, rolling back onto their
    // wheels while the recovery crew rights them: (roll, lift).
    let now = w.clock.tick as f32 + f;
    let flips: HashMap<CarId, f32> = w
        .driving
        .wrecks
        .iter()
        .filter(|x| x.flipped > 0)
        .map(|x| {
            let base = if x.flipped == 1 { std::f32::consts::FRAC_PI_2 } else { std::f32::consts::PI };
            let left = if x.righted_at > 0 { ((x.righted_at as f32 - now) / nd_sim::RIGHTING_MIN as f32).clamp(0.0, 1.0) } else { 1.0 };
            (x.car, base * left)
        })
        .collect();
    for &(id, wrecked) in &crashed {
        let Some(car) = traffic.cars.get(id) else { continue };
        let seen = w.car_in_tunnel(car) == under;
        let (p, h) = car.pose_at(f);
        let y = w.car_height(car);
        let es = *st.crashed.entry(id).or_insert_with(|| {
            let smoke = commands.spawn((Mesh3d(a.bonnet.clone()), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, StuntTag)).id();
            let lamp = commands.spawn((Mesh3d(meshes.add(Cuboid::new(0.35, 0.18, 0.35))), MeshMaterial3d(a.hazard.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, StuntTag)).id();
            [smoke, lamp]
        });
        let skew = if wrecked { 0.5 } else { 0.0 };
        if let Ok((mut tf, mut vis)) = q.get_mut(es[0]) {
            tf.translation = Vec3::new(p.x, y, p.y);
            tf.rotation = Quat::from_rotation_y(-(h + skew));
            *vis = if wrecked && seen { Visibility::Inherited } else { Visibility::Hidden };
        }
        if let Ok((mut tf, mut vis)) = q.get_mut(es[1]) {
            tf.translation = Vec3::new(p.x, y + 1.7, p.y);
            *vis = if seen && (t * 2.5) as i64 % 2 == 0 { Visibility::Inherited } else { Visibility::Hidden };
        }
        if wrecked {
            if let Some(ents) = state.cars.get(&id) {
                for e in ents {
                    if let Ok(mut tf) = cars.get_mut(*e) {
                        let roll = flips.get(&id).copied().unwrap_or(0.0);
                        tf.rotation = Quat::from_rotation_y(-(h + skew)) * Quat::from_rotation_x(0.12 + roll);
                        if roll > 0.0 {
                            // Rolled about its length, raised to lie on the road.
                            let half = if car.model.body.two_wheeler() { 0.35 } else { 0.9 };
                            tf.translation.y = y + 0.05 + half * roll.sin().abs() + 0.7 * (1.0 - roll.cos());
                        }
                    }
                }
            }
        }
    }

    // Tow trucks carry their wreck.
    let tows: Vec<CarId> = w.tows().iter().map(|t| t.truck).filter(|id| traffic.cars.contains_key(*id)).collect();
    let tow_set: HashSet<CarId> = tows.iter().copied().collect();
    st.loads.retain(|id, e| {
        let keep = tow_set.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    for id in tows {
        let car = &traffic.cars[id];
        let (p, h) = car.pose_at(f);
        let e = *st.loads.entry(id).or_insert_with(|| commands.spawn((Mesh3d(a.load.clone()), MeshMaterial3d(mats.surface(Surface::Concrete)), Transform::IDENTITY, StuntTag)).id());
        if let Ok((mut tf, _)) = q.get_mut(e) {
            tf.translation = Vec3::new(p.x, w.car_height(car) + 0.05, p.y);
            tf.rotation = Quat::from_rotation_y(-h);
        }
    }
}
