//! [r4] (K33) Rail freight in containers: the standard ISO boxes on the
//! freight wagons, and the freight yards' gantry cranes lifting them off
//! and on, box by box, from what the train really carries
//! (`nd_sim::rail::BoxLoad`).
//!
//! Each box is one entity (its surfaces children of it) with a mesh from
//! [`ContainerMeshes`], built once per kind and paint from
//! `nd_procgen::container_geometry`, so the boxes on the trains are the
//! same model as the yards', the hubs' and the stacks homes'. The crane is
//! a bridge on the yard's runway beams (drawn by the yard itself), a
//! trolley and a spreader on its cables, parked at the platform's end
//! while no train stands there.

use super::*;
use nd_sim::rail::{BoxKind, BoxLoad, Lift};

/// Paints of each box size and kind kept as meshes (seeds fold into these).
const PAINTS: u32 = 30;
/// Deck height of a container flat (the wagon's) and the apron.
const DECK_Y: f32 = 0.3 + 1.25;
const APRON_Y: f32 = 0.3;
/// Underside of a box on the spreader at the top of its lift.
const LIFT_Y: f32 = 5.3;

/// Meshes of the standard box, per (forty-foot, kind, paint).
#[derive(Resource, Default)]
pub(super) struct ContainerMeshes {
    map: HashMap<(bool, u8, u32), Vec<(Surface, Handle<Mesh>)>>,
}

impl ContainerMeshes {
    pub(super) fn get(&mut self, meshes: &mut Assets<Mesh>, pal: &nd_procgen::Palette, forty: bool, kind: BoxKind, seed: u32) -> Vec<(Surface, Handle<Mesh>)> {
        let look = match kind {
            BoxKind::Dry => nd_procgen::BoxLook::Dry,
            BoxKind::Reefer => nd_procgen::BoxLook::Reefer,
            BoxKind::Tank => nd_procgen::BoxLook::Tank,
            BoxKind::OpenTop => nd_procgen::BoxLook::OpenTop,
        };
        let key = (forty && kind != BoxKind::Tank, kind as u8, seed % PAINTS);
        self.map
            .entry(key)
            .or_insert_with(|| {
                let g = nd_procgen::container_geometry(key.0, look, key.2 as u64 * 7919 + 13, 0.25 + 0.4 * ((key.2 % 5) as f32 / 4.0), pal);
                g.iter().filter(|(_, m)| !m.positions.is_empty()).map(|(s, m)| (s, meshes.add(shared_mesh(m, true)))).collect()
            })
            .clone()
    }
}

/// Spawn one box: a parent carrying the transform, its surfaces as children.
pub(super) fn spawn_box(commands: &mut Commands, mats: &Materials, parts: &[(Surface, Handle<Mesh>)], tag: impl Component + Clone) -> Entity {
    let parent = commands.spawn((Transform::IDENTITY, Visibility::default(), tag.clone())).id();
    for (s, m) in parts {
        commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mats.surface(*s)), Transform::IDENTITY, tag.clone(), ChildOf(parent)));
    }
    parent
}

#[derive(Component, Clone)]
pub(super) struct FreightBoxTag;

#[derive(Component, Clone)]
pub(super) struct YardCraneTag;

/// What the freight renderer has spawned.
#[derive(Resource, Default)]
pub(super) struct FreightRender {
    /// Each train's boxes, by the box's seed.
    boxes: HashMap<nd_sim::TrainId, HashMap<u32, Entity>>,
    /// Each yard's crane: bridge, trolley, spreader, cables.
    cranes: HashMap<nd_sim::BuildingId, [Entity; 4]>,
    crane_meshes: Option<[Handle<Mesh>; 4]>,
}

/// A yard's track-side geometry in world terms.
struct Yard {
    pa: Vec2,
    along: Vec2,
    out: Vec2,
    length: f32,
    lay: nd_procgen::YardLayout,
}

fn yard_of(w: &nd_sim::World, b: nd_sim::BuildingId) -> Option<Yard> {
    let bld = w.buildings.get(b)?;
    let link = bld.rail.as_ref().filter(|l| l.kind == nd_sim::RailFacility::FreightYard)?;
    let lot = w.lots.get(bld.lot)?;
    let [pa, pb] = link.platform;
    let along = link.along();
    let out = Vec2::new(-along.y, along.x) * link.side;
    Some(Yard { pa, along, out, length: pa.distance(pb), lay: nd_procgen::yard_layout(link, &lot.polygon) })
}

impl Yard {
    /// The point `t` metres along the platform line, `off` out from it.
    fn at(&self, t: f32, off: f32) -> Vec2 {
        self.pa + self.along * t + self.out * off
    }
    fn t_of(&self, p: Vec2) -> f32 {
        (p - self.pa).dot(self.along)
    }
    /// Across offset of a box set down on the apron: imports by the
    /// track, exports waiting at the far edge (stacked on them when the
    /// apron is narrow).
    fn apron(&self, export: bool) -> (f32, f32) {
        let l = &self.lay;
        if !export {
            (l.apron_in + 1.4, APRON_Y)
        } else if l.apron_w >= 5.8 {
            (l.apron_in + l.apron_w - 1.4, APRON_Y)
        } else {
            (l.apron_in + 1.4, APRON_Y + nd_procgen::CONTAINER_H)
        }
    }
}

/// Where a box rides on its wagon with the train's head at `s`: centre and heading.
fn on_wagon(t: &nd_sim::Train, b: &BoxLoad, s: f32) -> (Vec2, f32) {
    let (c, heading) = t.vehicle_pose_at(b.wagon as usize, s);
    let dir = Vec2::new(heading.cos(), heading.sin());
    let off = if b.forty { 0.0 } else if b.slot == 0 { 3.1 } else { -3.1 };
    (c + dir * off, heading)
}

/// One crane move: where the box and the crane are `q` (0..1) through it.
/// `from` and `to` are (across offset, floor height) of the pick and the
/// set-down; `t_box` where along the platform the box is; `t_prev` where
/// the bridge comes from. Returns (box across, box floor, bridge t,
/// trolley across, spreader height, box hanging on it).
fn crane_move(q: f32, from: (f32, f32), to: (f32, f32), t_box: f32, t_prev: f32) -> (f32, f32, f32, f32, f32, bool) {
    let ease = |x: f32| x * x * (3.0 - 2.0 * x);
    let seg = |a: f32, b: f32| ease(((q - a) / (b - a)).clamp(0.0, 1.0));
    let top = LIFT_Y + nd_procgen::CONTAINER_H;
    let bridge = t_prev + (t_box - t_prev) * seg(0.0, 0.2);
    let (fx, fy) = from;
    let (tx, ty) = to;
    let pick_top = fy + nd_procgen::CONTAINER_H;
    let drop_top = ty + nd_procgen::CONTAINER_H;
    if q < 0.2 {
        (fx, fy, bridge, fx, top + 0.6, false)
    } else if q < 0.38 {
        let k = seg(0.2, 0.38);
        (fx, fy, t_box, fx, top + 0.6 + (pick_top - top - 0.6) * k, false)
    } else if q < 0.52 {
        let k = seg(0.38, 0.52);
        let y = fy + (LIFT_Y - fy) * k;
        (fx, y, t_box, fx, y + nd_procgen::CONTAINER_H, true)
    } else if q < 0.74 {
        let k = seg(0.52, 0.74);
        let x = fx + (tx - fx) * k;
        (x, LIFT_Y, t_box, x, top, true)
    } else if q < 0.9 {
        let k = seg(0.74, 0.9);
        let y = LIFT_Y + (ty - LIFT_Y) * k;
        (tx, y, t_box, tx, y + nd_procgen::CONTAINER_H, true)
    } else {
        let k = seg(0.9, 1.0);
        (tx, ty, t_box, tx, drop_top + (top + 0.6 - drop_top) * k, false)
    }
}

fn crane_meshes(meshes: &mut Assets<Mesh>) -> [Handle<Mesh>; 4] {
    let mut unit = |size: Vec3, col: [f32; 4]| {
        let mut m = nd_procgen::MeshData::default();
        m.boxed(Vec3::ZERO, size, 0.0, col);
        meshes.add(shared_mesh(&m, true))
    };
    // A bridge girder one metre long (scaled to the span), the trolley,
    // the spreader frame and a cable one metre long (scaled to its drop).
    [
        unit(Vec3::new(1.0, 0.9, 1.2), [0.85, 0.62, 0.12, 1.0]),
        unit(Vec3::new(2.6, 1.0, 2.2), [0.32, 0.33, 0.35, 1.0]),
        unit(Vec3::new(6.2, 0.32, nd_procgen::CONTAINER_W + 0.2), [0.85, 0.62, 0.12, 1.0]),
        unit(Vec3::new(0.06, 1.0, 0.06), [0.08, 0.08, 0.09, 1.0]),
    ]
}

/// The boxes on every freight train and the cranes over every yard.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_freight(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut boxes_mesh: ResMut<ContainerMeshes>,
    mut state: ResMut<FreightRender>,
    mut q: Query<&mut Transform, Or<(With<FreightBoxTag>, With<YardCraneTag>)>>,
) {
    let w = &sim.world;
    if sim.full_rebuild {
        for (_, m) in state.boxes.drain() {
            for (_, e) in m {
                commands.entity(e).despawn();
            }
        }
        for (_, ents) in state.cranes.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let f = sim.frac();
    let secs = w.config.traffic_seconds_per_tick;
    // Trains gone take their boxes.
    let gone: Vec<nd_sim::TrainId> = state.boxes.keys().copied().filter(|id| !w.rail.trains.contains_key(*id)).collect();
    for id in gone {
        for (_, e) in state.boxes.remove(&id).unwrap_or_default() {
            commands.entity(e).despawn();
        }
    }
    // The crane work under way at each yard: (train, progress).
    let mut working: HashMap<nd_sim::BuildingId, (nd_sim::TrainId, f32)> = HashMap::new();
    for (tid, t) in w.rail.trains.iter().filter(|(_, t)| t.kind == nd_sim::TrainKind::Freight) {
        if let Some(at) = t.dwelling_at().filter(|_| t.dwell_total > 0.0) {
            let left = (t.dwell - secs * f).max(0.0);
            working.insert(at, (tid, (1.0 - left / t.dwell_total).clamp(0.0, 1.0)));
        }
    }
    // Cranes for every yard; the gone ones go.
    let yards: Vec<nd_sim::BuildingId> = w.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == nd_sim::RailFacility::FreightYard)).map(|(id, _)| id).collect();
    let stale: Vec<nd_sim::BuildingId> = state.cranes.keys().copied().filter(|b| !yards.contains(b)).collect();
    for b in stale {
        for e in state.cranes.remove(&b).unwrap() {
            commands.entity(e).despawn();
        }
    }
    let cm = state.crane_meshes.get_or_insert_with(|| crane_meshes(&mut meshes)).clone();
    for &b in &yards {
        if !state.cranes.contains_key(&b) {
            let ents = [0, 1, 2, 3].map(|k| commands.spawn((Mesh3d(cm[k].clone()), MeshMaterial3d(mats.surface(Surface::Metal)), Transform::IDENTITY, YardCraneTag)).id());
            state.cranes.insert(b, ents);
        }
    }

    // Each freight train's boxes.
    for (tid, t) in w.rail.trains.iter().filter(|(_, t)| t.kind == nd_sim::TrainKind::Freight) {
        let s = t.prev_s + (t.s - t.prev_s) * f;
        let ents = state.boxes.entry(tid).or_default();
        let live: Vec<u32> = t.boxes.iter().map(|b| b.seed).collect();
        ents.retain(|seed, e| {
            let keep = live.contains(seed);
            if !keep {
                commands.entity(*e).despawn();
            }
            keep
        });
        for b in &t.boxes {
            if !ents.contains_key(&b.seed) {
                let parts = boxes_mesh.get(&mut meshes, &assets.palette, b.forty, b.kind, b.seed);
                ents.insert(b.seed, spawn_box(&mut commands, &mats, &parts, FreightBoxTag));
            }
        }
        let yard = t.dwelling_at().and_then(|at| yard_of(w, at).map(|y| (at, y)));
        let moves = t.boxes.iter().filter(|b| b.lift != Lift::Aboard).count().max(1) as f32;
        let progress = yard.as_ref().and_then(|(at, _)| working.get(at)).filter(|(id, _)| *id == tid).map_or(0.0, |x| x.1);
        for b in &t.boxes {
            let Some(&e) = ents.get(&b.seed) else { continue };
            let Ok(mut tf) = q.get_mut(e) else { continue };
            let (wc, heading) = on_wagon(t, b, s);
            let (pos, y, yaw) = match (b.lift, &yard) {
                (Lift::Aboard, _) | (_, None) => (wc, DECK_Y, heading),
                (Lift::Off(k) | Lift::On(k), Some((_, yd))) => {
                    let export = matches!(b.lift, Lift::On(_));
                    let t_box = yd.t_of(wc).clamp(0.0, yd.length);
                    let wagon = (yd.lay.main, DECK_Y);
                    let apron = yd.apron(export);
                    let (from, to) = if export { (apron, wagon) } else { (wagon, apron) };
                    let (a, z) = (0.05 + 0.9 * k as f32 / moves, 0.05 + 0.9 * (k as f32 + 1.0) / moves);
                    let qq = ((progress - a) / (z - a)).clamp(0.0, 1.0);
                    let (x, y, ..) = crane_move(qq, from, to, t_box, t_box);
                    let yaw = yd.along.y.atan2(yd.along.x);
                    (yd.at(t_box, x), y, yaw)
                }
            };
            tf.translation = Vec3::new(pos.x, y, pos.y);
            tf.rotation = Quat::from_rotation_y(-yaw);
        }
    }

    // The cranes: at work over the box being moved, else parked.
    for &b in &yards {
        let Some(yd) = yard_of(w, b) else { continue };
        let Some(ents) = state.cranes.get(&b).copied() else { continue };
        let top = LIFT_Y + nd_procgen::CONTAINER_H;
        let mut pose = (yd.length.min(8.0), yd.lay.apron_in + 1.4, top + 0.6, None::<(f32, f32)>);
        if let Some(&(tid, p)) = working.get(&b) {
            if let Some(t) = w.rail.trains.get(tid) {
                let s = t.s;
                let moving: Vec<&BoxLoad> = t.boxes.iter().filter(|x| x.lift != Lift::Aboard).collect();
                let n = moving.len().max(1) as f32;
                let k = (((p - 0.05) / 0.9) * n).floor().clamp(0.0, n - 1.0) as usize;
                if let Some(bx) = moving.iter().find(|x| matches!(x.lift, Lift::Off(j) | Lift::On(j) if j as usize == k)).or(moving.first()) {
                    let export = matches!(bx.lift, Lift::On(_));
                    let t_box = yd.t_of(on_wagon(t, bx, s).0).clamp(0.0, yd.length);
                    // Where the bridge was for the move before.
                    let t_prev = moving
                        .iter()
                        .find(|x| matches!(x.lift, Lift::Off(j) | Lift::On(j) if k > 0 && j as usize == k - 1))
                        .map_or(yd.length.min(8.0), |x| yd.t_of(on_wagon(t, x, s).0).clamp(0.0, yd.length));
                    let wagon = (yd.lay.main, DECK_Y);
                    let apron = yd.apron(export);
                    let (from, to) = if export { (apron, wagon) } else { (wagon, apron) };
                    let (a, z) = (0.05 + 0.9 * k as f32 / n, 0.05 + 0.9 * (k as f32 + 1.0) / n);
                    let qq = ((p - a) / (z - a)).clamp(0.0, 1.0);
                    let (_, _, bridge, trolley, spreader, _) = crane_move(qq, from, to, t_box, t_prev);
                    pose = (bridge, trolley, spreader, Some((bridge, trolley)));
                }
            }
        }
        let (bt, tx, sy, _) = pose;
        let yaw = yd.along.y.atan2(yd.along.x);
        let rot = Quat::from_rotation_y(-yaw);
        let ry = nd_procgen::YARD_RUNWAY_Y + 0.65;
        let near = yd.at(bt, yd.lay.near_off);
        let far = yd.at(bt, yd.lay.far_off);
        let span = near.distance(far) + 1.0;
        let mid = (near + far) * 0.5;
        let across = Quat::from_rotation_y(-(yd.out.y.atan2(yd.out.x)));
        if let Ok(mut tf) = q.get_mut(ents[0]) {
            *tf = Transform::from_xyz(mid.x, ry, mid.y).with_rotation(across).with_scale(Vec3::new(span, 1.0, 1.0));
        }
        let tp = yd.at(bt, tx);
        if let Ok(mut tf) = q.get_mut(ents[1]) {
            *tf = Transform::from_xyz(tp.x, ry + 0.9, tp.y).with_rotation(rot);
        }
        if let Ok(mut tf) = q.get_mut(ents[2]) {
            *tf = Transform::from_xyz(tp.x, sy + 0.16, tp.y).with_rotation(rot);
        }
        let drop = (ry + 0.4 - sy).max(0.1);
        if let Ok(mut tf) = q.get_mut(ents[3]) {
            *tf = Transform::from_xyz(tp.x, sy + drop * 0.5 + 0.3, tp.y).with_scale(Vec3::new(1.0, drop, 1.0));
        }
    }
}
