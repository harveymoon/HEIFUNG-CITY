//! [r8] The depots' straddle carriers at work: each depot with lorry bays
//! has one, standing by its bays. When the sim starts a move
//! (`DepotYard::crane`), the carrier drives over the very box (in the stack,
//! or on the chassis of the lorry in its bay), lifts it on its spreader,
//! carries it across and sets it down on the chassis or in the stack. The
//! box hanging from it is the record the sim is moving: its line, number,
//! wear and length.

use super::*;
use nd_sim::containers::{BoxFace, BoxId};

#[derive(Component, Clone)]
pub(super) struct CarrierTag;

struct Carrier {
    body: Entity,
    beacon: Entity,
    spreader: Entity,
    /// The box it carries, drawn.
    load: Option<(BoxId, Entity)>,
    /// Where it stands now and which way (ground point, yaw).
    at: Vec3,
    yaw: f32,
    /// Where the move under way began, and its two ends.
    from: Option<(Vec3, f32)>,
    job: Option<(u64, BoxId)>,
    ends: Option<((Vec3, f32), (Vec3, f32))>,
}

#[derive(Resource, Default)]
pub(super) struct CarrierRender {
    carriers: HashMap<nd_sim::BuildingId, Carrier>,
    meshes: Option<[Handle<Mesh>; 3]>,
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_yaw(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    // A box is the same either way round: turn the short way.
    if d > std::f32::consts::FRAC_PI_2 {
        d -= std::f32::consts::PI;
    } else if d < -std::f32::consts::FRAC_PI_2 {
        d += std::f32::consts::PI;
    }
    a + d * t
}

/// Where a box rides on the chassis of the lorry in bay `bay` of depot
/// `d`: its floor's centre and yaw.
fn on_chassis(sim: &Sim, d: nd_sim::BuildingId, bay: u8, car: nd_sim::CarId, id: BoxId) -> Option<(Vec3, f32)> {
    let w = &sim.world;
    let &(at, yaw) = sim.depot_bays.get(&d)?.get(bay as usize)?;
    let mut forties: Vec<bool> = w.boxes_on_truck(car).iter().filter(|b| **b != id).filter_map(|b| w.containers.boxes.get(*b).map(|r| r.forty)).collect();
    let me = w.containers.boxes.get(id)?.forty;
    forties.push(me);
    let teu: u32 = forties.iter().map(|f| if *f { 2 } else { 1 }).sum();
    let code = if teu >= 2 { nd_sim::vehicle::CHASSIS_40 } else { nd_sim::vehicle::CHASSIS_20 };
    let xs = nd_procgen::vehicles::boxes_on_chassis(code, &forties);
    let x = *xs.last()?;
    let deck = nd_procgen::vehicles::chassis_deck(code)?.1;
    let along = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    Some((at + along * x + Vec3::Y * deck, yaw))
}

/// Where a box stands (or will stand) in depot `d`'s stack.
fn in_stack(sim: &Sim, d: nd_sim::BuildingId, id: BoxId, seen: &HashMap<BoxId, (Vec3, f32)>) -> Option<(Vec3, f32)> {
    if let Some(p) = seen.get(&id) {
        return Some(*p);
    }
    let w = &sim.world;
    let day = w.clock.day() as i64;
    let (_, slots) = sim.depot_slots.get(&d)?;
    let mut faces: Vec<BoxFace> = w.containers.faces_at(d, day);
    faces.push(w.containers.boxes.get(id)?.face(id, day));
    let k = faces.len() - 1;
    super::boxes::lay_out(&faces, slots).into_iter().find(|x| x.0 == k).map(|x| (x.1, x.2))
}

/// The carriers: one per depot that draws bays, parked beside them, and
/// at work through each move.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_carriers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut bm: ResMut<super::boxes::BoxMeshes>,
    stacks: Res<super::boxes::DepotBoxRender>,
    mut state: ResMut<CarrierRender>,
    mut q: Query<&mut Transform, With<CarrierTag>>,
) {
    let w = &sim.world;
    if sim.full_rebuild {
        for (_, c) in state.carriers.drain() {
            for e in [c.body, c.beacon, c.spreader].into_iter().chain(c.load.map(|l| l.1)) {
                commands.entity(e).despawn();
            }
        }
    }
    let hs = state
        .meshes
        .get_or_insert_with(|| [meshes.add(shared_mesh(&nd_procgen::vehicles::straddle_carrier_mesh(), true)), meshes.add(shared_mesh(&nd_procgen::vehicles::carrier_beacon_mesh(), false)), meshes.add(shared_mesh(&nd_procgen::vehicles::spreader_mesh(), true))])
        .clone();
    // Gone: a depot that no longer draws bays.
    let gone: Vec<nd_sim::BuildingId> = state.carriers.keys().copied().filter(|d| !sim.depot_bays.contains_key(d) || w.bay_depot(*d).is_none()).collect();
    for d in gone {
        if let Some(c) = state.carriers.remove(&d) {
            for e in [c.body, c.beacon, c.spreader].into_iter().chain(c.load.map(|l| l.1)) {
                commands.entity(e).despawn();
            }
        }
    }
    let tick = w.clock.tick;
    let f = sim.frac();
    let day = w.clock.day() as i64;
    for (&d, bays) in sim.depot_bays.iter() {
        if w.bay_depot(d).is_none() || bays.is_empty() {
            continue;
        }
        if !state.carriers.contains_key(&d) {
            // Parked just past the last bay, facing along the row.
            let (a, yaw) = bays[0];
            let (b, _) = *bays.last().unwrap();
            let row = (b - a).normalize_or_zero();
            let side = if row.length_squared() > 0.0 { row } else { Vec3::new(-yaw.sin(), 0.0, yaw.cos()) };
            let at = b + side * (nd_procgen::BAY_W + 3.5);
            let body = commands.spawn((Mesh3d(hs[0].clone()), MeshMaterial3d(mats.surface(Surface::Metal)), Transform::from_translation(at).with_rotation(Quat::from_rotation_y(-yaw)), CarrierTag)).id();
            let beacon = commands.spawn((Mesh3d(hs[1].clone()), MeshMaterial3d(mats.glow()), Transform::from_translation(at).with_rotation(Quat::from_rotation_y(-yaw)), NotShadowCaster, CarrierTag)).id();
            let spreader = commands.spawn((Mesh3d(hs[2].clone()), MeshMaterial3d(mats.surface(Surface::Metal)), Transform::from_translation(at + Vec3::Y * (nd_procgen::vehicles::CARRIER_H - 1.2)).with_rotation(Quat::from_rotation_y(-yaw)), CarrierTag)).id();
            state.carriers.insert(d, Carrier { body, beacon, spreader, load: None, at, yaw, from: None, job: None, ends: None });
            continue;
        }
        let job = w.containers.depots.get(&d).and_then(|y| y.crane);
        let c = state.carriers.get_mut(&d).unwrap();
        // Where it is, where the box is and its height, this frame.
        let mut carrying: Option<(BoxId, Vec3, f32)> = None;
        let mut hoist = nd_procgen::vehicles::CARRIER_H - 1.2;
        let lift_y = nd_procgen::CONTAINER_H * 3.0 + 0.6;
        match job {
            Some(j) => {
                if c.job != Some((j.start, j.boxid)) {
                    c.job = Some((j.start, j.boxid));
                    c.from = Some((c.at, c.yaw));
                    let truck = on_chassis(&sim, d, j.bay, j.car, j.boxid);
                    // A box the stacks had no room to draw: the stack slot
                    // nearest the bay (or beside the bays) stands in.
                    let stack = in_stack(&sim, d, j.boxid, &stacks.seen).or_else(|| {
                        let (bay, yaw) = *sim.depot_bays.get(&d)?.get(j.bay as usize)?;
                        let near = sim.depot_slots.get(&d).and_then(|(_, s)| s.iter().filter(|s| s.tier == 0).min_by(|a, b| a.at.distance(bay).total_cmp(&b.at.distance(bay))).map(|s| (s.at, s.yaw)));
                        Some(near.unwrap_or((bay + Vec3::new(-yaw.sin(), 0.0, yaw.cos()) * 9.0, yaw)))
                    });
                    c.ends = match (truck, stack) {
                        (Some(t), Some(s)) => Some(if j.onto_truck { (s, t) } else { (t, s) }),
                        _ => None,
                    };
                }
                if let (Some(((src, syaw), (dst, dyaw))), Some((from, fyaw))) = (c.ends, c.from) {
                    let p = j.progress(tick, f).clamp(0.0, 1.0);
                    let ground = |v: Vec3| Vec3::new(v.x, from.y, v.z);
                    let (pos, yaw, box_y, attached) = if p < 0.2 {
                        let t = smooth(p / 0.2);
                        (ground(from).lerp(ground(src), t), lerp_yaw(fyaw, syaw, t), None, false)
                    } else if p < 0.35 {
                        let t = (p - 0.2) / 0.15;
                        // Down onto the box, then up with it.
                        let dip = (t * std::f32::consts::PI).sin();
                        let y = if t < 0.5 { src.y + nd_procgen::CONTAINER_H + (lift_y - src.y) * (1.0 - dip) } else { src.y + (lift_y - src.y) * (1.0 - dip) };
                        (ground(src), syaw, Some(if t < 0.5 { src.y } else { y }), t >= 0.5)
                    } else if p < 0.75 {
                        let t = smooth((p - 0.35) / 0.4);
                        (ground(src).lerp(ground(dst), t), lerp_yaw(syaw, dyaw, t), Some(src.y.max(dst.y) + lift_y - src.y.min(dst.y)), true)
                    } else if p < 0.9 {
                        let t = smooth((p - 0.75) / 0.15);
                        let top = src.y.max(dst.y) + lift_y - src.y.min(dst.y);
                        (ground(dst), dyaw, Some(top + (dst.y - top) * t), true)
                    } else {
                        (ground(dst), dyaw, None, false)
                    };
                    c.at = pos;
                    c.yaw = yaw;
                    if let (Some(y), true) = (box_y, attached) {
                        carrying = Some((j.boxid, Vec3::new(pos.x, y, pos.z), yaw));
                        hoist = y - from.y + nd_procgen::CONTAINER_H;
                    }
                }
            }
            None => {
                c.job = None;
                c.ends = None;
            }
        }
        let rot = Quat::from_rotation_y(-c.yaw);
        for e in [c.body, c.beacon] {
            if let Ok(mut tf) = q.get_mut(e) {
                tf.translation = c.at;
                tf.rotation = rot;
            }
        }
        if let Ok(mut tf) = q.get_mut(c.spreader) {
            tf.translation = c.at + Vec3::Y * hoist.min(nd_procgen::vehicles::CARRIER_H - 1.2);
            tf.rotation = rot;
            let forty = carrying.and_then(|(b, _, _)| w.containers.boxes.get(b)).is_none_or(|r| r.forty);
            tf.scale = Vec3::new(if forty { 1.0 } else { 0.5 }, 1.0, 1.0);
        }
        // The box on the spreader.
        match carrying {
            Some((b, at, yaw)) => {
                if c.load.is_none_or(|l| l.0 != b) {
                    if let Some((_, e)) = c.load.take() {
                        commands.entity(e).despawn();
                    }
                    if let Some(r) = w.containers.boxes.get(b) {
                        let face = r.face(b, day);
                        let parts = bm.get(&mut meshes, &assets.palette, &face);
                        let tf = Transform::from_translation(at).with_rotation(Quat::from_rotation_y(-yaw));
                        let e = super::boxes::spawn_real_box(&mut commands, &mats, &parts, tf, super::boxes::BoxTag { id: b, face, offset: Vec3::ZERO }, (), CarrierTag);
                        c.load = Some((b, e));
                    }
                } else if let Some((_, e)) = c.load {
                    if let Ok(mut tf) = q.get_mut(e) {
                        tf.translation = at;
                        tf.rotation = Quat::from_rotation_y(-yaw);
                    }
                }
            }
            None => {
                if let Some((_, e)) = c.load.take() {
                    commands.entity(e).despawn();
                }
            }
        }
    }
}
