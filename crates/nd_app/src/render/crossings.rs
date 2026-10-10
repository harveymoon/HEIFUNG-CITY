//! [rail-crossing] Level crossing gear: the posts, crossbucks, lamp boards
//! and bells (fixed meshes), the twin red lamps on each post that flash in
//! turn while the crossing is shut, and the striped barrier arms that swing
//! down before a train and up after it, their tip lamps lit while they are
//! anything but up. Placed from `nd_procgen::crossing::rigs`, at the height
//! of the street there; redrawn when the crossings change.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::crossing::{arm_geometry, furniture, rigs, LAMP_SIZE, RAISED};
use nd_procgen::{Geometry, MeshData};
use nd_sim::level_crossing::LevelCrossing;
use nd_sim::roads::SegmentId;
use nd_sim::World;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::{shared_mesh, spawn_geometry, Materials};
use crate::assets::GameAssets;
use crate::sim::Sim;

/// Seconds each lamp of a pair stays lit before the other takes over.
const FLASH: f32 = 0.55;

#[derive(Component, Clone)]
pub(super) struct CrossingPart;

/// A lamp of crossing `crossing`: the left (0) or right (1) of a post's
/// pair, flashing in turn, or one on an arm (2), lit steady.
#[derive(Component)]
pub(super) struct CrossingLamp {
    crossing: usize,
    side: u8,
}

/// The pivot of a barrier arm of crossing `crossing`, pointing `yaw`.
#[derive(Component)]
pub(super) struct CrossingArm {
    crossing: usize,
    yaw: Quat,
}

#[derive(Default)]
pub(super) struct CrossingRender {
    key: Option<u64>,
    ents: Vec<Entity>,
    meshes: Option<(Handle<Mesh>, Handle<Mesh>)>,
    mats: Option<(Handle<StandardMaterial>, Handle<StandardMaterial>)>,
}

/// What the gear was placed for: every crossing's place and guard, and
/// whether the ground has relief.
fn key(w: &World) -> u64 {
    let mut h = DefaultHasher::new();
    w.relief.hash(&mut h);
    for c in w.level_crossings() {
        c.node.0.hash(&mut h);
        c.pos.x.to_bits().hash(&mut h);
        c.pos.y.to_bits().hash(&mut h);
        (c.guard as u8).hash(&mut h);
        for a in &c.arms {
            a.segment.hash(&mut h);
            a.lanes_in.hash(&mut h);
            a.lanes_out.hash(&mut h);
            a.stop.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

/// The height of the street at the crossing.
fn lift(w: &World, c: &LevelCrossing) -> f32 {
    if !w.relief {
        return 0.0;
    }
    match c.arms.first() {
        Some(a) => {
            let sid = SegmentId(a.segment);
            let t = if w.roads.segment(sid).a == c.node { 0.0 } else { 1.0 };
            w.road_height(sid, t)
        }
        None => w.surface_height(c.pos),
    }
}

/// An arm's pose with `down` of the way down (0 up, 1 down).
fn arm_pose(yaw: Quat, down: f32) -> Quat {
    // Ease in and out, as the motor does.
    let k = down.clamp(0.0, 1.0);
    let k = k * k * (3.0 - 2.0 * k);
    yaw * Quat::from_rotation_z((1.0 - k) * RAISED)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_crossings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    assets: Res<GameAssets>,
    time: Res<Time>,
    mut state: Local<CrossingRender>,
    mut lamps: Query<(&CrossingLamp, &mut MeshMaterial3d<StandardMaterial>, Option<&super::swap::SwapOriginal>)>,
    mut arms: Query<(&CrossingArm, &mut Transform)>,
) {
    let w = &sim.world;
    let pal = &assets.palette;
    if state.meshes.is_none() {
        let mut lens = MeshData::default();
        lens.boxed(Vec3::ZERO, Vec3::new(0.08, LAMP_SIZE, LAMP_SIZE), 0.0, [1.0, 1.0, 1.0, 1.0]);
        let mut bulb = MeshData::default();
        bulb.boxed(Vec3::ZERO, Vec3::splat(0.13), 0.0, [1.0, 1.0, 1.0, 1.0]);
        state.meshes = Some((meshes.add(shared_mesh(&lens, false)), meshes.add(shared_mesh(&bulb, false))));
        let red = pal.signal_red;
        let on = materials.add(StandardMaterial { base_color: Color::linear_rgb(red[0] * 2.2, red[1] * 0.6, red[2] * 0.4), unlit: true, ..default() });
        let off = materials.add(StandardMaterial { base_color: Color::linear_rgb(0.1, 0.012, 0.01), unlit: true, ..default() });
        state.mats = Some((on, off));
    }
    let k = key(w);
    if state.key != Some(k) || sim.full_rebuild {
        state.key = Some(k);
        for e in state.ents.drain(..) {
            commands.entity(e).despawn();
        }
        let (lens, bulb) = state.meshes.clone().unwrap();
        let (_, off) = state.mats.clone().unwrap();
        let mut ents = Vec::new();
        for (ci, c) in w.level_crossings().iter().enumerate() {
            let y = lift(w, c);
            let rig = rigs(c);
            let mut g = Geometry::default();
            furniture(&mut g, pal, &rig);
            for e in spawn_geometry(&mut commands, &mut meshes, &mats, &g, CrossingPart) {
                commands.entity(e).insert(Transform::from_xyz(0.0, y, 0.0));
                ents.push(e);
            }
            for r in &rig {
                let turn = Quat::from_rotation_y(-r.facing.y.atan2(r.facing.x));
                for (side, l) in r.lamps.iter().enumerate() {
                    let e = commands.spawn((Mesh3d(lens.clone()), MeshMaterial3d(off.clone()), Transform::from_translation(*l + Vec3::Y * y).with_rotation(turn), NotShadowCaster, NotShadowReceiver, CrossingPart, CrossingLamp { crossing: ci, side: side as u8 })).id();
                    ents.push(e);
                }
                for a in &r.arms {
                    let yaw = Quat::from_rotation_y(-a.along.y.atan2(a.along.x));
                    let pivot = commands.spawn((Transform::from_translation(a.pivot + Vec3::Y * y).with_rotation(arm_pose(yaw, c.arm)), Visibility::Inherited, CrossingPart, CrossingArm { crossing: ci, yaw })).id();
                    ents.push(pivot);
                    for e in spawn_geometry(&mut commands, &mut meshes, &mats, &arm_geometry(pal, a.len, a.small), CrossingPart) {
                        commands.entity(e).insert(ChildOf(pivot));
                    }
                    for p in a.lamps() {
                        commands.spawn((Mesh3d(bulb.clone()), MeshMaterial3d(off.clone()), Transform::from_translation(p), NotShadowCaster, NotShadowReceiver, CrossingPart, CrossingLamp { crossing: ci, side: 2 }, ChildOf(pivot)));
                    }
                }
            }
        }
        state.ents = ents;
    }
    let list = w.level_crossings();
    if list.is_empty() {
        return;
    }
    // The arms, between the last two ticks' positions.
    let f = sim.frac();
    for (a, mut tf) in &mut arms {
        let Some(c) = list.get(a.crossing) else { continue };
        let down = c.prev_arm + (c.arm - c.prev_arm) * f;
        tf.rotation = arm_pose(a.yaw, down);
    }
    // The lamps: the post's pair in turn, the arms' steady.
    let (on, off) = state.mats.clone().unwrap();
    let beat = (time.elapsed_secs() / FLASH) as u64 % 2;
    for (l, mut m, orig) in &mut lamps {
        let Some(c) = list.get(l.crossing) else { continue };
        let lit = c.flashing() && (l.side == 2 || l.side as u64 == beat);
        let want = if lit { &on } else { &off };
        // [refactor/app] Against the original under a stand-in view, so heat
        // and focus do not swap the lamp back and forth every frame.
        if orig.map_or(&m.0, |o| &o.0) != want {
            m.0 = want.clone();
        }
    }
}

// ── [pt-visual] crossing lamps that light the road ──────────────────────────

#[derive(Component)]
pub(super) struct CrossingLight;

/// Red light of a crossing's flashing lamps on the road at night: a small
/// pool of real point lights (no shadows), sized from the street lamps'
/// pool (`street_look::lamp_pool`), hung on the flashing posts nearest the
/// view and pulsing with the lamps' beat. None at Low.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_crossing_lights(
    mut commands: Commands,
    sim: Res<Sim>,
    look: Res<super::sky::SkyLook>,
    stats: Res<super::RenderStats>,
    lighting: Res<super::Lighting>,
    mode: Res<crate::camera::CameraMode>,
    time: Res<Time>,
    cams: Query<(&GlobalTransform, &crate::camera::OrbitCamera)>,
    mut pool: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<CrossingLight>>,
    mut spawned: Local<usize>,
    mut near: Local<Vec<(f32, Vec3)>>,
    mut spots: Local<Vec<Vec3>>,
) {
    let want = (super::street_look::lamp_pool(stats.graphics.tier) / 3).min(8);
    if *spawned < want {
        for _ in *spawned..want {
            commands.spawn((
                PointLight { color: Color::linear_rgb(1.0, 0.08, 0.04), intensity: 0.0, range: CROSSING_LIGHT_RANGE, radius: 0.2, shadow_maps_enabled: false, ..default() },
                super::offstage(),
                Visibility::Hidden,
                NotShadowCaster,
                CrossingLight,
            ));
        }
        *spawned = want;
        return;
    }
    let w = &sim.world;
    let night = super::street_look::lamp_night(look.daylight);
    spots.clear();
    if night > 0.01 && !lighting.heat {
        for c in w.level_crossings().iter().filter(|c| c.flashing()) {
            let y = lift(w, c);
            for r in rigs(c) {
                // Half a metre in front of the pair, towards the traffic.
                let mid = (r.lamps[0] + r.lamps[1]) * 0.5;
                spots.push(mid + Vec3::new(r.facing.x, 0.0, r.facing.y) * 0.5 + Vec3::Y * y);
            }
        }
    }
    near.clear();
    if let Some((tf, cam)) = cams.iter().next() {
        super::street_look::nearest_lamps(&spots, super::street_look::lamp_focus(*mode, tf.translation(), cam), want, &mut near);
    }
    // The pair flashes in turn: the light pulses at the beat, never off.
    let phase = (time.elapsed_secs() / FLASH).fract();
    let pulse = 0.55 + 0.45 * (phase * std::f32::consts::TAU).cos().abs();
    let mut lit = near.iter();
    for (mut light, mut ltf, mut vis) in &mut pool {
        match lit.next() {
            Some((_, p)) => {
                light.intensity = CROSSING_LUMENS * night * pulse;
                if ltf.translation != *p {
                    ltf.translation = *p;
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

/// A crossing lamp's light: dimmer and shorter than a street lamp's.
const CROSSING_LUMENS: f32 = 9.0e5;
const CROSSING_LIGHT_RANGE: f32 = 16.0;
