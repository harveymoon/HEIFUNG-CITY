//! [fishing] The fishing fleets (`nd_sim::fishing`): every trawler drawn
//! where the sim has it, tied up at its own berth or out on the water, so
//! a boat at sea leaves its berth empty. Under way it trails a wake; on
//! the grounds its outriggers are lowered and its warps run into the sea;
//! gulls wheel over a boat hauling its nets or unloading; its deck lights
//! come on in the dark of dawn and dusk; boxes of fish pile up on the quay
//! beside a boat unloading.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::{MeshData, Surface};
use nd_sim::fishing::{TrawlerPhase, UNLOAD_MIN};
use nd_sim::landscape::{BANK_HEIGHT, WATER_LEVEL};
use nd_sim::BuildingId;
use std::collections::HashMap;

use super::{night_strength, shared_mesh, Lighting, Materials};
use crate::assets::GameAssets;
use crate::sim::Sim;

/// Gulls round one boat.
const GULLS: usize = 5;

#[derive(Component, Clone)]
pub(super) struct TrawlerTag;

struct BoatEnts {
    hull: Entity,
    nets: Entity,
    wake: Entity,
    lights: Entity,
    crates: Entity,
    gulls: Vec<Entity>,
}

struct Meshes {
    hull: Handle<Mesh>,
    nets: Handle<Mesh>,
    wake: Handle<Mesh>,
    lights: Handle<Mesh>,
    crates: Handle<Mesh>,
    gull: Handle<Mesh>,
}

#[derive(Default)]
pub(super) struct FishingRender {
    meshes: Option<Meshes>,
    boats: HashMap<(BuildingId, usize), BoatEnts>,
}

fn rgba(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

/// A V of churned water astern, fading out behind; bow along +x.
fn wake_mesh() -> MeshData {
    let mut m = MeshData::default();
    let y = WATER_LEVEL + 0.12;
    let n = 8;
    for side in [-1.0f32, 1.0] {
        for k in 0..n {
            let (a, b) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
            let x = |t: f32| -7.0 - t * 46.0;
            let z = |t: f32| side * (2.2 + t * 13.0);
            let w = |t: f32| 0.9 + t * 2.4;
            let (al, bl) = (0.75 * (1.0 - a), 0.75 * (1.0 - b));
            let p0 = Vec3::new(x(a), y, z(a) - side * w(a) * 0.5);
            let p1 = Vec3::new(x(b), y, z(b) - side * w(b) * 0.5);
            let p2 = Vec3::new(x(b), y, z(b) + side * w(b) * 0.5);
            let p3 = Vec3::new(x(a), y, z(a) + side * w(a) * 0.5);
            let c = [0.92, 0.96, 1.0];
            let base = m.positions.len() as u32;
            for (p, al) in [(p0, al), (p1, bl), (p2, bl), (p3, al)] {
                m.positions.push(p.to_array());
                m.normals.push([0.0, 1.0, 0.0]);
                m.colors.push(rgba(c, al));
                m.uvs.push([0.0, 0.0]);
                m.tangents.push([1.0, 0.0, 0.0, 1.0]);
            }
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    // The churn right under the stern.
    let base = m.positions.len() as u32;
    for (p, al) in [(Vec3::new(-7.5, y, -2.0), 0.7), (Vec3::new(-20.0, y, -1.2), 0.0), (Vec3::new(-20.0, y, 1.2), 0.0), (Vec3::new(-7.5, y, 2.0), 0.7)] {
        m.positions.push(p.to_array());
        m.normals.push([0.0, 1.0, 0.0]);
        m.colors.push(rgba([1.0, 1.0, 1.0], al));
        m.uvs.push([0.0, 0.0]);
        m.tangents.push([1.0, 0.0, 0.0, 1.0]);
    }
    m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
    m
}

/// Masthead, sidelights and deck floods, bow along +x.
fn lights_mesh() -> MeshData {
    let mut m = MeshData::default();
    m.boxed(Vec3::new(3.2, 8.95, 0.0), Vec3::splat(0.45), 0.0, [1.0, 1.0, 0.95, 1.0]);
    m.boxed(Vec3::new(2.4, 4.4, 1.9), Vec3::new(0.35, 0.3, 0.2), 0.0, [0.2, 1.0, 0.3, 1.0]);
    m.boxed(Vec3::new(2.4, 4.4, -1.9), Vec3::new(0.35, 0.3, 0.2), 0.0, [1.0, 0.15, 0.1, 1.0]);
    m.boxed(Vec3::new(-6.0, 6.5, 0.0), Vec3::new(0.4, 0.25, 1.2), 0.0, [1.0, 0.85, 0.55, 1.0]);
    m.boxed(Vec3::new(3.2, 6.2, 0.0), Vec3::new(0.3, 0.3, 0.6), 0.0, [1.0, 0.9, 0.7, 1.0]);
    m
}

/// A stack of fish boxes on a pallet, one unit tall (scaled up as the
/// stack grows); x along the quay.
fn crates_mesh() -> MeshData {
    let mut m = MeshData::default();
    m.boxed(Vec3::new(0.0, 0.08, 0.0), Vec3::new(3.6, 0.16, 2.4), 0.0, [0.45, 0.33, 0.2, 1.0]);
    let colors = [[0.15, 0.4, 0.8], [0.95, 0.55, 0.1], [0.85, 0.85, 0.85], [0.15, 0.4, 0.8]];
    for layer in 0..4 {
        for i in 0..3 {
            for j in 0..2 {
                let c = colors[(layer + i + j) % 4];
                m.boxed(Vec3::new(-1.15 + i as f32 * 1.15, 0.16 + 0.3 + layer as f32 * 0.6, -0.6 + j as f32 * 1.2), Vec3::new(1.05, 0.55, 1.1), 0.0, rgba(c, 1.0));
            }
        }
    }
    m
}

/// A gull: two swept wings in a shallow V, along +x.
fn gull_mesh() -> MeshData {
    let mut m = MeshData::default();
    let white = [0.95, 0.95, 0.93, 1.0];
    m.boxed(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.7, 0.18, 0.2), 0.0, white);
    for s in [-1.0f32, 1.0] {
        m.beam(Vec3::new(0.05, 0.0, s * 0.1), Vec3::new(-0.25, 0.35, s * 1.15), 0.12, white);
        m.beam(Vec3::new(-0.25, 0.35, s * 1.15), Vec3::new(-0.45, 0.2, s * 1.6), 0.08, [0.3, 0.3, 0.32, 1.0]);
    }
    m
}

fn yaw_rot(dir: Vec2) -> Quat {
    Quat::from_rotation_y(-dir.y.atan2(dir.x))
}

/// Where the boxes of a boat unloading go: on the quay beside its berth.
fn crate_spot(port: &nd_sim::ports::Port, mooring: Vec2) -> (Vec3, Quat) {
    let [root, tip] = port.pier;
    let (_, q) = nd_sim::geom::project_on_segment(mooring, root, tip);
    let to_quay = (q - mooring).normalize_or_zero();
    let p = mooring + to_quay * (2.5 + 3.2);
    let along = Vec2::new(-to_quay.y, to_quay.x);
    (Vec3::new(p.x, BANK_HEIGHT + 0.6 + 0.4, p.y), yaw_rot(along))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_trawlers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut st: Local<FishingRender>,
    mut q: Query<(&mut Transform, &mut Visibility), With<TrawlerTag>>,
) {
    let w = &sim.world;
    if w.fishing.fleets.is_empty() && st.boats.is_empty() {
        return;
    }
    let m = st
        .meshes
        .get_or_insert_with(|| Meshes {
            hull: meshes.add(shared_mesh(&nd_procgen::terrain::trawler_mesh(false, &assets.palette), true)),
            nets: meshes.add(shared_mesh(&nd_procgen::terrain::trawler_mesh(true, &assets.palette), true)),
            wake: meshes.add(shared_mesh(&wake_mesh(), false)),
            lights: meshes.add(shared_mesh(&lights_mesh(), false)),
            crates: meshes.add(shared_mesh(&crates_mesh(), true)),
            gull: meshes.add(shared_mesh(&gull_mesh(), false)),
        });
    let (hull_m, nets_m, wake_m, lights_m, crates_m, gull_m) = (m.hull.clone(), m.nets.clone(), m.wake.clone(), m.lights.clone(), m.crates.clone(), m.gull.clone());
    // Boats come and go with their ports.
    let gone: Vec<(BuildingId, usize)> = st.boats.keys().copied().filter(|(b, k)| sim.full_rebuild || w.fishing.fleets.get(b).is_none_or(|f| *k >= f.boats.len())).collect();
    for key in gone {
        if let Some(e) = st.boats.remove(&key) {
            for x in [e.hull, e.nets, e.wake, e.lights, e.crates].into_iter().chain(e.gulls) {
                commands.entity(x).despawn();
            }
        }
    }
    let hidden = (Transform::IDENTITY, Visibility::Hidden, TrawlerTag);
    for (b, f) in &w.fishing.fleets {
        for k in 0..f.boats.len() {
            st.boats.entry((*b, k)).or_insert_with(|| BoatEnts {
                hull: commands.spawn((Mesh3d(hull_m.clone()), MeshMaterial3d(mats.surface(Surface::Panel)), hidden.clone())).id(),
                nets: commands.spawn((Mesh3d(nets_m.clone()), MeshMaterial3d(mats.surface(Surface::Panel)), hidden.clone())).id(),
                wake: commands.spawn((Mesh3d(wake_m.clone()), MeshMaterial3d(mats.overlay.clone()), NotShadowCaster, NotShadowReceiver, hidden.clone())).id(),
                lights: commands.spawn((Mesh3d(lights_m.clone()), MeshMaterial3d(mats.glow()), NotShadowCaster, hidden.clone())).id(),
                crates: commands.spawn((Mesh3d(crates_m.clone()), MeshMaterial3d(mats.surface(Surface::Panel)), hidden.clone())).id(),
                gulls: (0..GULLS).map(|_| commands.spawn((Mesh3d(gull_m.clone()), MeshMaterial3d(mats.surface(Surface::Panel)), NotShadowCaster, hidden.clone())).id()).collect(),
            });
        }
    }
    let frac = sim.frac();
    let dark = night_strength(&sim, &lighting) > 0.25;
    let t = time.elapsed_secs();
    let mut set = |e: Entity, tf: Option<Transform>| {
        if let Ok((mut x, mut v)) = q.get_mut(e) {
            match tf {
                Some(tf) => {
                    *x = tf;
                    *v = Visibility::Inherited;
                }
                None => *v = Visibility::Hidden,
            }
        }
    };
    for ((b, k), e) in st.boats.iter() {
        let Some(f) = w.fishing.fleets.get(b) else { continue };
        let Some(boat) = f.boats.get(*k) else { continue };
        let (p, dir) = f.boat_pose(*k, frac);
        let rot = yaw_rot(dir);
        let at = Vec3::new(p.x, WATER_LEVEL, p.y);
        // A gentle roll and pitch at sea.
        let sway = if boat.phase.at_sea() {
            let ph = *k as f32 * 1.7;
            Quat::from_rotation_x(0.035 * (t * 0.9 + ph).sin()) * Quat::from_rotation_z(0.02 * (t * 0.7 + ph).cos())
        } else {
            Quat::IDENTITY
        };
        let boat_tf = Transform::from_translation(at).with_rotation(rot * sway);
        let nets = boat.phase.nets_out();
        set(e.hull, (!nets).then_some(boat_tf));
        set(e.nets, nets.then_some(boat_tf));
        let wake = match boat.phase {
            TrawlerPhase::Outbound | TrawlerPhase::Inbound => Some(1.0),
            TrawlerPhase::Fishing => Some(0.45),
            _ => None,
        };
        set(e.wake, wake.map(|s| Transform::from_translation(at).with_rotation(rot).with_scale(Vec3::new(s, 1.0, s))));
        set(e.lights, (dark && boat.phase != TrawlerPhase::InPort).then_some(boat_tf));
        // Boxes on the quay while unloading.
        let unloading = boat.phase == TrawlerPhase::Unloading;
        let port = w.ports.port_of(*b);
        let crates = match (unloading, port) {
            (true, Some(port)) => {
                let (spot, r) = crate_spot(port, f.mooring(boat.slot).0);
                let done = 1.0 - boat.left as f32 / UNLOAD_MIN as f32;
                Some(Transform::from_translation(spot).with_rotation(r).with_scale(Vec3::new(1.0, 0.15 + 0.85 * done, 1.0)))
            }
            _ => None,
        };
        set(e.crates, crates);
        // Gulls after a boat hauling its nets, or at the quay unloading.
        let gulls = matches!(boat.phase, TrawlerPhase::Hauling | TrawlerPhase::Unloading);
        let stern = p - dir * 7.0;
        for (i, g) in e.gulls.iter().enumerate() {
            let tf = gulls.then(|| {
                let ph = i as f32 * 1.3 + *k as f32 * 0.7;
                let r = 6.0 + i as f32 * 1.8;
                let a = t * (0.5 + 0.08 * i as f32) * if i % 2 == 0 { 1.0 } else { -1.0 } + ph;
                let off = Vec2::from_angle(a) * r;
                let y = 9.0 + 2.5 * (t * 0.6 + ph).sin();
                let heading = Vec2::from_angle(a + if i % 2 == 0 { 1.57 } else { -1.57 });
                let flap = 0.6 + 0.5 * (t * 7.0 + ph * 3.0).sin().abs();
                Transform::from_translation(Vec3::new(stern.x + off.x, WATER_LEVEL + y, stern.y + off.y)).with_rotation(yaw_rot(heading)).with_scale(Vec3::new(1.6, 1.6 * flap, 1.6))
            });
            set(*g, tf);
        }
    }
}
