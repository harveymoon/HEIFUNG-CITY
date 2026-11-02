//! Citizens on foot: figures, poses, highlight.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

#[allow(clippy::too_many_arguments)]
/// [K36] A walker whose way runs this far under the ground there (an
/// open cut's floor is its ground) is in a tunnel: drawn only in the Underground view.
const UNDERGROUND_WALK: f32 = -1.5;

/// The ground at `p`, or the water's surface over it (an open cut's
/// floor is its ground; a bridge deck above does not count).
fn under_floor(w: &nd_sim::World, p: SVec2) -> f32 {
    let g = w.ground_height(p);
    if w.landscape.is_water(p) { g.max(nd_sim::landscape::WATER_LEVEL) } else { g }
}

/// [r6-perf] What [`sync_citizens`] worked out for a figure: the heights
/// and the tunnel test (world queries over the roads near it, the bulk of
/// the system's time), kept while their inputs ([`height_key`]) stay the
/// same, and the pose it last wrote, so a figure that has not moved (a
/// paused game, someone standing or sitting) is not written again and
/// `lift_movers` leaves it be. The same pose either way.
#[derive(Component, Default)]
pub(super) struct FigureCache {
    key: Option<HeightKey>,
    below: bool,
    way: Option<f32>,
    raised: f32,
    written: Option<Transform>,
    /// The leg last asked about (its ends and the world's stamp) and
    /// whether it is plain (`World::leg_is_plain`).
    leg: Option<(([u32; 4], u64), bool)>,
}

/// [r6-perf] The radius round a figure's spot that must be out of view
/// for it to be skipped: its own size, a frame's walk or ride, and any
/// rise or drop of the way since it was last drawn.
const FIGURE_VIEW_RADIUS: f32 = 25.0;

/// [r6-perf] How far round a leg `leg_is_plain` looks: anyone within
/// this less the height queries' own 12 m of the leg is covered.
const LEG_REACH: f32 = 40.0;

type HeightKey = ([u32; 8], u8, u64);

/// Everything the heights of a figure at `pos` are read from: where it is
/// (drawn and in the sim), the leg of its path, how it travels, and the
/// versions of the ways, the ground and the city.
fn height_key(c: &nd_sim::Citizen, pos: SVec2, stamp: u64) -> HeightKey {
    let (mut from, mut to, mut flags) = (SVec2::ZERO, SVec2::ZERO, 0u8);
    if let nd_sim::Activity::Traveling { path, idx, .. } = &c.activity {
        flags |= 1;
        if *idx > 0 {
            flags |= 2;
            from = path.get(*idx - 1).copied().unwrap_or_default();
        }
        if let Some(&t) = path.get(*idx) {
            flags |= 4;
            to = t;
        }
    }
    if c.inside.is_some() {
        flags |= 8;
    }
    if c.on_platform() {
        flags |= 16;
    }
    if c.mode == nd_sim::citizens::TravelMode::Bike {
        flags |= 32;
    }
    let b = |v: f32| v.to_bits();
    ([b(pos.x), b(pos.y), b(c.pos.x), b(c.pos.y), b(from.x), b(from.y), b(to.x), b(to.y)], flags, stamp)
}

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
    view: Query<Option<&bevy::camera::primitives::Frustum>, With<crate::camera::OrbitCamera>>,
    tools: Res<crate::tools::ToolState>,
    mut q: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>, &mut Mesh3d, Option<&swap::SwapOriginal>, Option<&mut WayHeight>, Has<PlatformTag>, Option<&mut FigureCache>), With<CitizenTag>>,
) {
    let _t = crate::frame_stats::span("sync_citizens");
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
                .spawn((Mesh3d(mats.citizen_mesh.clone()), MeshMaterial3d(look_mat.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, CitizenTag, WayHeight::default(), FigureCache::default()))
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
    let frustum = view.iter().next().flatten();
    let down = w.down_on_street();
    // [K18] A seated figure of their own (on a bench, or on the kerb with
    // their knees up), not a standing one squashed into the ground.
    let sit_of = |id: CitizenId, ride: bool| match w.pose_with(id, &down) {
        _ if ride || plain => 0,
        nd_sim::world::Pose::Bench => nd_procgen::people::SIT_BENCH,
        nd_sim::world::Pose::SittingKerb => nd_procgen::people::SIT_GROUND,
        _ => 0,
    };
    // [r4-crossings] A cyclist on a footbridge's stairs or deck walks the
    // bike over: drawn on foot there, not riding up the steps into the air.
    let wheeled = |c: &nd_sim::Citizen, (k, s): (nd_procgen::people::PersonKey, f32)| (nd_procgen::people::PersonKey { ride: k.ride && !w.wheeling_bike(c), ..k }, s);
    let mut outdoors: Vec<(CitizenId, Entity, nd_procgen::people::PersonKey, f32, f32)> = state
        .citizens
        .iter()
        .filter_map(|(id, &e)| w.citizens.get(*id).filter(|c| shown(c)).map(|c| (*id, e, wheeled(c, people::figure_of(c)), c.pos.distance_squared(SVec2::new(eye.x, eye.z)))))
        .map(|(id, e, (k, s), d)| (id, e, nd_procgen::people::PersonKey { sit: sit_of(id, k.ride), ..k }, s, d))
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
    let stamp = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (sim.generation, w.walk.version, w.ground.version(), w.landscape.version(), w.roads.segments.len(), w.roads.nodes.len(), w.relief).hash(&mut h);
        h.finish()
    };
    for &(id, e, key, stature, _) in &outdoors {
        let Some(c) = w.citizens.get(id) else { continue };
        let Ok((mut tf, mut vis, _, mut mesh, _, way, platform, mut cache)) = q.get_mut(e) else { continue };
        // [r6-perf] Out of the camera's view (figures cast no shadow): kept
        // hidden, and nothing worked out for it, until it comes into view.
        // The test is round where it stands now at the height it was last
        // drawn, 25 m either way: from the street most of a big city's
        // crowd is behind the camera or the buildings' side of the screen.
        if let (Some(f), Some(_)) = (frustum, cache.as_ref().and_then(|k| k.written)) {
            // Anywhere between where it was and where it is this tick.
            let mid = (c.prev_pos + c.pos) * 0.5;
            let at = bevy::math::Vec3A::new(mid.x, tf.translation.y, mid.y);
            let radius = FIGURE_VIEW_RADIUS + c.prev_pos.distance(c.pos) * 0.5;
            if !f.intersects_sphere(&bevy::camera::primitives::Sphere { center: at, radius }, true) {
                vis.set_if_neq(Visibility::Hidden);
                continue;
            }
        }
        let pos = if c.prev_pos.distance(c.pos) < 30.0 { c.prev_pos.lerp(c.pos, f) } else { c.pos };
        let heading = lerp_angle(c.prev_heading, c.heading, f);
        // [clarity] Benches and kerbs sit; the hurt (and the homeless
        // sleeping rough) lie flat on the ground.
        let pose = w.pose_with(id, &down);
        let seated = matches!(pose, nd_sim::world::Pose::Bench | nd_sim::world::Pose::SittingKerb);
        let lying = pose == nd_sim::world::Pose::Lying;
        // Thrown clear of the wreck: beside it, not under it.
        let pos = if lying && down.contains(&id) { pos + SVec2::new(-c.heading.sin(), c.heading.cos()) * 2.6 } else { pos };
        // [r6-perf] The heights, worked out again only when what they are
        // read from changed (`FigureCache`).
        let hkey = height_key(c, pos, stamp);
        let cached = cache.as_ref().filter(|k| k.key == Some(hkey)).map(|k| (k.below, k.way, k.raised));
        // A walker on a plain leg (at grade, nothing raised or sunk near)
        // stands on the ground: no search of the roads round them.
        let plain_leg = match &c.activity {
            nd_sim::Activity::Traveling { path, idx, .. } if *idx > 0 && *idx < path.len() && cached.is_none() => {
                let (a, b) = (path[*idx - 1], path[*idx]);
                let lk = ([a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()], stamp);
                let known = cache.as_ref().and_then(|k| k.leg).filter(|(k, _)| *k == lk).map(|(_, p)| p);
                let is_plain = known.unwrap_or_else(|| w.leg_is_plain(a, b, LEG_REACH));
                if let Some(k) = cache.as_mut().filter(|_| known.is_none()) {
                    k.leg = Some((lk, is_plain));
                }
                let m = SVec2::splat(LEG_REACH - 12.0);
                let (lo, hi) = (a.min(b) - m, a.max(b) + m);
                let within = |p: SVec2| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y;
                is_plain && within(pos) && within(c.pos)
            }
            _ => false,
        };
        let (below, way_h, raised) = cached.or(plain_leg.then(|| (c.on_platform(), None, 0.0))).unwrap_or_else(|| {
            // [walk-networks] Down in a pedestrian tunnel too: shown in the
            // underground view with the platforms' people.
            // [K36] Or anyone whose way runs below ground (a street tunnel
            // under a river), however their leg was found.
            let below = c.on_platform() || w.citizen_in_tunnel(c) || w.walker_way_height(c, c.pos).is_some_and(|z| z < under_floor(w, c.pos) + UNDERGROUND_WALK);
            // [tools] Up the stairs and over a footbridge (a raised
            // walkway); [footbridge] on an elevated path, a viaduct or a
            // bridge: on its deck.
            (below, w.walker_way_height(c, pos), w.walker_level(c, pos))
        });
        if let Some(k) = cache.as_mut().filter(|_| cached.is_none()) {
            (k.key, k.below, k.way, k.raised) = (Some(hkey), below, way_h, raised);
        }
        if platform != below {
            if platform {
                commands.entity(e).try_remove::<PlatformTag>();
            } else {
                commands.entity(e).try_insert(PlatformTag);
            }
        }
        vis.set_if_neq(Visibility::Visible);
        if let Some(mut way) = way {
            if way.0 != way_h {
                way.0 = way_h;
            }
        }
        // [K18] A bench's seat is on the park's ground; a kerb sitter sits
        // on the pavement (its top, where walkers stand).
        let kerb = pose == nd_sim::world::Pose::SittingKerb;
        let mut t = Transform::from_translation(Vec3::new(pos.x, if lying { 0.12 } else if seated && !kerb { 0.02 } else { 0.16 } + raised, pos.y));
        if c.on_platform() {
            t.translation.y = 0.16 - nd_sim::subway::DEPTH;
        }
        t.rotation = Quat::from_rotation_y(-heading);
        if lying {
            t.rotation *= Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        }
        // The seated figures are modelled sitting (`PersonKey::sit`); only
        // the plain stand-in is squashed.
        t.scale = Vec3::splat(stature);
        looks.insert(key);
        let fallback = || if key.ride { mats.bike_mesh.clone() } else { mats.citizen_mesh.clone() };
        let figure = if plain { None } else { people.mesh_for(key, &mut meshes) };
        let stand_in = figure.is_none();
        let wanted_mesh = figure.unwrap_or_else(fallback);
        if stand_in && seated && !plain {
            t.scale = Vec3::new(1.0, 0.62, 1.15) * stature;
        }
        if plain {
            t.scale = if seated { Vec3::new(1.0, 0.62, 1.15) } else { Vec3::ONE };
        }
        // [r6-perf] The same pose as last written: left as it stands (with
        // the lift `lift_movers` gave it), so neither this nor the lift
        // dirties it again.
        match cache.as_mut() {
            Some(k) if k.written == Some(t) => {}
            Some(k) => {
                k.written = Some(t);
                *tf = t;
            }
            None => *tf = t,
        }
        if mesh.0 != wanted_mesh {
            mesh.0 = wanted_mesh;
        }
    }
    for (id, &e) in state.citizens.iter() {
        let Some(c) = w.citizens.get(*id) else { continue };
        let Ok((_, mut vis, mut mat, mut mesh, heat_original, _, _, _)) = q.get_mut(e) else { continue };
        if !shown(c) {
            vis.set_if_neq(Visibility::Hidden);
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
