//! [K34] Real shipping containers in the depots, and their numbers.
//!
//! Every box stored at a freight yard, the port, a cargo hub, a container
//! yard or a scrap yard (`nd_sim::containers`) stands in its depot's
//! stacks: the slots the depot's style hands over instead of drawing
//! boxes of its own (`Geometry::box_slots`, kept in `Sim::depot_slots`).
//! Each box is one entity whose surfaces use meshes shared by every box
//! that looks the same (size, build, line, wear step, marks, door end), so
//! a yard of hundreds of boxes draws as instances. Close up, a box's
//! number is laid over it: one small mesh per box, for the boxes near the
//! camera only (`box_numbers`); the trains' boxes carry their numbers the
//! same way.

use super::*;
use nd_sim::containers::{BoxFace, BoxId, Marks};

/// A real box drawn somewhere: its record and look, for its number and
/// for picking.
#[derive(Component, Clone)]
pub(crate) struct BoxTag {
    pub id: BoxId,
    pub face: BoxFace,
    /// Where the box's floor centre is on the tagged entity (a lorry's
    /// trailer); zero for a box entity of its own.
    pub offset: Vec3,
}

#[derive(Component, Clone)]
pub(super) struct DepotBoxTag;

/// [r8] A real box riding on a container lorry's chassis.
#[derive(Component, Clone)]
pub(super) struct TruckBoxTag;

#[derive(Component)]
pub(super) struct NumberTag;

/// (forty, kind, line, wear step, marks variant, door at +x).
type Key = (bool, u8, u8, u8, u8, bool);

fn key_of(face: &BoxFace) -> Key {
    let wq = ((face.wear * 4.0).round() as u8).min(4);
    let v = face.marks.patches.min(1) | (face.marks.dents.min(1) << 1);
    (face.forty, face.kind as u8, face.line % 10, wq, v, face.seed & 1 == 0)
}

/// Meshes of the real box looks, built once each.
#[derive(Resource, Default)]
pub(super) struct BoxMeshes {
    map: HashMap<Key, Vec<(Surface, Handle<Mesh>)>>,
}

impl BoxMeshes {
    pub(super) fn get(&mut self, meshes: &mut Assets<Mesh>, pal: &nd_procgen::Palette, face: &BoxFace) -> Vec<(Surface, Handle<Mesh>)> {
        let key = key_of(face);
        self.map
            .entry(key)
            .or_insert_with(|| {
                // A stand-in box with the bucket's look: the door end the
                // same as every box of it (`box_life`), its marks the step's.
                let (forty, _, line, wq, v, door) = key;
                let h = nd_sim::hash::mix((forty as u64) << 40 | (face.kind as u64) << 32 | (line as u64) << 24 | (wq as u64) << 16 | (v as u64) << 8, 0x0B0C);
                let seed = (h & !1) | if door { 0 } else { 1 };
                let wear = wq as f32 / 4.0;
                let marks = Marks { patches: if v & 1 != 0 { 1 + (wq / 2) } else { 0 }, dents: if v & 2 != 0 { 1 + (wq / 2) } else { 0 }, scuffed: false, repainted: false };
                let f = BoxFace { seed, wear, marks, ..*face };
                let g = nd_procgen::real_box_geometry(&f, false, pal);
                g.iter().filter(|(_, m)| !m.positions.is_empty()).map(|(s, m)| (s, meshes.add(shared_mesh(m, true)))).collect()
            })
            .clone()
    }
}

/// One depot's boxes on screen: which, and their entities.
struct DepotDrawn {
    ids: Vec<BoxId>,
    slots: u64,
    entities: Vec<Entity>,
}

#[derive(Resource, Default)]
pub(super) struct DepotBoxRender {
    depots: HashMap<nd_sim::BuildingId, DepotDrawn>,
    /// [r8] Where each box was last stood in a stack (floor centre, yaw):
    /// the carrier drives to it there when it lifts it out.
    pub(super) seen: HashMap<BoxId, (Vec3, f32)>,
    version: Option<(u64, u64)>,
    frame: u32,
}

/// Spawn one real box: a parent with its transform, the shared surfaces as children.
pub(super) fn spawn_real_box(commands: &mut Commands, mats: &Materials, parts: &[(Surface, Handle<Mesh>)], tf: Transform, tag: BoxTag, parent_extra: impl Bundle, extra: impl Component + Clone) -> Entity {
    let parent = commands.spawn((tf, Visibility::default(), tag, extra.clone(), parent_extra)).id();
    for (s, m) in parts {
        commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mats.surface(*s)), Transform::IDENTITY, extra.clone(), ChildOf(parent)));
    }
    parent
}

/// Which slot each box takes: ground tiers first; a twenty-footer in a
/// forty-foot slot takes one half (the next one the other), a forty-footer
/// a forty-foot slot. Boxes past the last slot are not drawn.
pub(super) fn lay_out(faces: &[BoxFace], slots: &[nd_procgen::BoxSlot]) -> Vec<(usize, Vec3, f32)> {
    let mut order: Vec<usize> = (0..slots.len()).collect();
    order.sort_by_key(|&i| (slots[i].tier, i));
    let mut used = vec![false; slots.len()];
    let mut half: Option<usize> = None;
    let mut out = Vec::new();
    let shift = |s: &nd_procgen::BoxSlot, d: f32| s.at + Vec3::new(s.yaw.cos(), 0.0, s.yaw.sin()) * d;
    // A slot above another is usable only once the one below is taken.
    let below: Vec<Option<usize>> = slots.iter().map(|a| if a.tier == 0 { None } else { slots.iter().position(|b| b.tier + 1 == a.tier && b.at.xz().distance(a.at.xz()) < 0.5) }).collect();
    let below_ok = |i: usize, used: &[bool]| below[i].is_none_or(|j| used[j]);
    for (k, f) in faces.iter().enumerate() {
        if f.forty {
            let Some(&i) = order.iter().find(|&&i| !used[i] && slots[i].forty && below_ok(i, &used)) else { continue };
            used[i] = true;
            out.push((k, slots[i].at, slots[i].yaw));
        } else if let Some(i) = half.take() {
            out.push((k, shift(&slots[i], nd_procgen::CONTAINER_20 * 0.5 + 0.02), slots[i].yaw));
        } else {
            let Some(&i) = order.iter().find(|&&i| !used[i] && below_ok(i, &used)) else { continue };
            used[i] = true;
            if slots[i].forty {
                half = Some(i);
                out.push((k, shift(&slots[i], -(nd_procgen::CONTAINER_20 * 0.5 + 0.02)), slots[i].yaw));
            } else {
                out.push((k, slots[i].at, slots[i].yaw));
            }
        }
    }
    out
}

/// Stand every depot's stored boxes in its slots; redrawn per depot when
/// what it holds changes.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_depot_boxes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut bm: ResMut<BoxMeshes>, mut state: ResMut<DepotBoxRender>) {
    let w = &sim.world;
    if sim.full_rebuild {
        for (_, d) in state.depots.drain() {
            for e in d.entities {
                commands.entity(e).despawn();
            }
        }
        state.version = None;
    }
    state.frame = state.frame.wrapping_add(1);
    let ver = (w.containers.version, sim.depot_slots_version);
    if state.version == Some(ver) || (state.version.is_some() && state.frame % 8 != 0) {
        return;
    }
    state.version = Some(ver);
    let day = w.clock.day() as i64;
    // Boxes the yard crane is still moving are drawn on the train.
    let lifting: std::collections::HashSet<BoxId> = w.rail.trains.values().flat_map(|t| t.boxes.iter().filter(|b| b.lift != nd_sim::rail::Lift::Aboard).filter_map(|b| b.id)).collect();
    let gone: Vec<nd_sim::BuildingId> = state.depots.keys().copied().filter(|b| !sim.depot_slots.contains_key(b)).collect();
    for b in gone {
        for e in state.depots.remove(&b).map(|d| d.entities).unwrap_or_default() {
            commands.entity(e).despawn();
        }
    }
    for (&b, (slot_ver, slots)) in sim.depot_slots.iter() {
        if w.box_depot(b).is_none() {
            if let Some(d) = state.depots.remove(&b) {
                for e in d.entities {
                    commands.entity(e).despawn();
                }
            }
            continue;
        }
        let faces: Vec<BoxFace> = w.containers.faces_at(b, day).into_iter().filter(|f| !lifting.contains(&f.id)).collect();
        let ids: Vec<BoxId> = faces.iter().map(|f| f.id).collect();
        if state.depots.get(&b).is_some_and(|d| d.ids == ids && d.slots == *slot_ver) {
            continue;
        }
        if let Some(d) = state.depots.remove(&b) {
            for e in d.entities {
                commands.entity(e).despawn();
            }
        }
        let mut entities = Vec::new();
        for (k, at, yaw) in lay_out(&faces, slots) {
            let f = faces[k];
            let parts = bm.get(&mut meshes, &assets.palette, &f);
            let tf = Transform::from_translation(at).with_rotation(Quat::from_rotation_y(-yaw));
            if state.seen.len() > 50_000 {
                state.seen.clear();
            }
            state.seen.insert(f.id, (at, yaw));
            entities.push(spawn_real_box(&mut commands, &mats, &parts, tf, BoxTag { id: f.id, face: f, offset: Vec3::ZERO }, (), DepotBoxTag));
        }
        state.depots.insert(b, DepotDrawn { ids, slots: *slot_ver, entities });
    }
}

/// Numbers shown and their meshes.
#[derive(Resource, Default)]
pub(super) struct NumberRender {
    shown: HashMap<Entity, Entity>,
    meshes: HashMap<BoxId, Handle<Mesh>>,
    frame: u32,
}

/// Boxes nearer the camera than this show their number.
const NUMBER_RANGE: f32 = 48.0;
/// At most this many numbers at once.
const MAX_NUMBERS: usize = 160;

/// Lay each nearby box's number over it; take it off again as the camera
/// pulls away.
pub(super) fn box_numbers(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mats: Res<Materials>, cams: Query<&GlobalTransform, With<Camera3d>>, boxes: Query<(Entity, &GlobalTransform, &BoxTag)>, mut state: ResMut<NumberRender>) {
    state.frame = state.frame.wrapping_add(1);
    if state.frame % 6 != 0 {
        return;
    }
    let Some(eye) = cams.iter().next().map(|t| t.translation()) else { return };
    let mut near: Vec<(f32, Entity, BoxFace, Vec3)> = boxes.iter().map(|(e, t, b)| (t.transform_point(b.offset).distance(eye), e, b.face, b.offset)).filter(|x| x.0 < NUMBER_RANGE).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(MAX_NUMBERS);
    let want: std::collections::HashSet<Entity> = near.iter().map(|x| x.1).collect();
    let alive: std::collections::HashSet<Entity> = boxes.iter().map(|x| x.0).collect();
    let drop: Vec<Entity> = state.shown.keys().copied().filter(|e| !want.contains(e)).collect();
    for e in drop {
        let n = state.shown.remove(&e).unwrap();
        if alive.contains(&e) {
            commands.entity(n).despawn();
        }
    }
    if state.meshes.len() > 4000 {
        state.meshes.clear();
    }
    for (_, e, face, offset) in near {
        if state.shown.contains_key(&e) {
            continue;
        }
        let mesh = state.meshes.entry(face.id).or_insert_with(|| meshes.add(shared_mesh(&nd_procgen::box_number_mesh(&face), false))).clone();
        let n = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mats.surface(Surface::Matte)), Transform::from_translation(offset), NumberTag, ChildOf(e))).id();
        state.shown.insert(e, n);
    }
}

/// Where the boxes on screen are, for picking one with a click.
pub(super) fn box_spots(mut sim: ResMut<Sim>, boxes: Query<(&GlobalTransform, &BoxTag)>, mut frame: Local<u32>) {
    *frame = frame.wrapping_add(1);
    if *frame % 10 != 0 {
        return;
    }
    sim.box_spots = boxes.iter().map(|(t, b)| (b.id, t.transform_point(b.offset), t.rotation(), b.face.forty)).collect();
}
