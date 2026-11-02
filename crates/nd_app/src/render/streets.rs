//! Streets, lane arrows, signals, bus stops and toll booths.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// [footbridge] A key for the set of crosswalks footbridges replaced.
pub(super) fn closed_hash(closed: &[SVec2]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in closed {
        (p.x.to_bits(), p.y.to_bits()).hash(&mut h);
    }
    closed.len().hash(&mut h);
    h.finish()
}

/// [fix/roads-tools] The crosswalk key with the signal switches folded in:
/// turning a junction's signals on or off changes no road and no lane, so
/// without this the new heads were never drawn (todo 2026-10-11).
pub(super) fn signals_key(closed: u64, signals_version: u64) -> u64 {
    closed ^ signals_version.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

pub(super) fn sync_roads(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut stats: ResMut<RenderStats>,
    mut lamps: ResMut<street_look::StreetLamps>,
    existing: Query<Entity, Or<(With<RoadsTag>, With<holo::TunnelTag>, With<holo::GhostRoadTag>)>>,
) {
    let _t = crate::frame_stats::span("sync_roads");
    let version = sim.world.roads.version;
    // [footbridge] A footbridge built (or its crosswalk kept) repaints the
    // streets: its zebra goes or comes back.
    let closed = sim.world.closed_crosswalks();
    // [r1-roads] (K14) Path ramps a footbridge took over.
    let joined: Vec<(nd_sim::roads::NodeId, nd_sim::roads::NodeId)> = sim.world.walk.footbridges.iter().filter_map(|f| f.path_join.map(|(_, r, n)| (r, n))).collect();
    // [fix/roads-tools] Signals switched on in the Intersection editor
    // repaint them too: their poles are part of the street mesh.
    let closed_key = signals_key(closed_hash(&closed), sim.world.traffic.net.signals_version);
    if version == state.roads_version && closed_key == state.closed_roads && !existing.is_empty() && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.roads_version = version;
    state.closed_roads = closed_key;
    despawn_all(&mut commands, &existing);
    let w = &sim.world;
    // Each run is set on its own height profile here (a viaduct over a
    // street, the street under it), so the roads are not lifted again.
    let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
    let geo = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &assets.palette, seed: w.config.seed, heights, inner: if w.relief { &w.road_inner } else { &[] }, layer: nd_procgen::street::StreetLayer::Surface, closed: &closed, joined_ramps: &joined });
    stats.street_triangles = geo.triangle_count();
    lamps.lamps.clone_from(&geo.lamps);
    lamps.version += 1;
    for (s, data) in geo.iter() {
        let mesh = meshes.add(to_mesh(data, !s.is_glow()));
        let mut e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, RoadsTag));
        if s.is_glow() {
            e.insert((NotShadowCaster, NotShadowReceiver));
        } else if ghosts_underground(s) {
            // The same mesh again, see-through, for the underground view.
            // [pt-ux] Only the street itself: trees, lamps, poles and
            // street furniture stood over the tunnels as ghosts too.
            commands.spawn((Mesh3d(mesh), MeshMaterial3d(mats.ghost_road.clone()), Transform::IDENTITY, holo::GhostRoadTag, Visibility::Hidden, NotShadowCaster, NotShadowReceiver));
        }
    }
    // The tunnels on their own, shown in the underground view.
    if w.roads.alive_segments().any(|(_, s)| s.tunnel) {
        let geo = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &assets.palette, seed: w.config.seed, heights, inner: if w.relief { &w.road_inner } else { &[] }, layer: nd_procgen::street::StreetLayer::Underground, closed: &closed, joined_ramps: &joined });
        stats.street_triangles += geo.triangle_count();
        for (s, data) in geo.iter() {
            commands.spawn((Mesh3d(meshes.add(to_mesh(data, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, holo::TunnelTag, Visibility::Hidden, NotShadowCaster));
        }
    }
}

/// Lane arrows painted before junction stop lines; redrawn when the arrows
/// change (turning demand, the player's lane use, a road rebuild).
pub(super) fn sync_lane_arrows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<LaneArrowsTag>>,
) {
    let net = &sim.world.traffic.net;
    let w = &sim.world;
    let inputs = (sim.generation, net.arrows_version, net.version, w.roads.version(), w.ground.version(), w.relief);
    if net.arrows_version == state.arrows_version && !assets.palette_changed && !sim.full_rebuild {
        // Nothing drawn means the last build came out empty: build again
        // only when something it was built from has moved.
        if !existing.is_empty() || state.arrows_empty.same(inputs) {
            return;
        }
    }
    state.arrows_empty = Gate(Some(inputs));
    state.arrows_version = net.arrows_version;
    despawn_all(&mut commands, &existing);
    // Set at their own road's height (not lifted in `lift_spawned`).
    let w = &sim.world;
    let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
    let geo = nd_procgen::street::lane_arrows(&w.roads, net, heights, if w.relief { &w.road_inner } else { &[] }, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, LaneArrowsTag);
}

/// A glowing lamp in every signal head, coloured by whether that lane
/// currently has a green connector.
pub(super) fn sync_signals(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<SignalTag>>,
    mut q: Query<(&SignalTag, Option<&PedSignalTag>, &mut MeshMaterial3d<StandardMaterial>, Option<&swap::SwapOriginal>)>,
) {
    let _t = crate::frame_stats::span("sync_signals");
    let net = &sim.world.traffic.net;
    let closed = sim.world.closed_crosswalks();
    let closed_key = signals_key(closed_hash(&closed), net.signals_version);
    if net.version != state.lanes_version || closed_key != state.closed_signals || sim.full_rebuild {
        state.lanes_version = net.version;
        state.closed_signals = closed_key;
        despawn_all(&mut commands, &existing);
        for post in signal_posts(&sim.world.roads, net) {
            commands.spawn((
                Mesh3d(mats.signal_mesh.clone()),
                MeshMaterial3d(mats.signal_red.clone()),
                Transform::from_translation(post.lamp).with_rotation(Quat::from_rotation_y(-post.heading)),
                NotShadowCaster,
                SignalTag(post.lane),
            ));
        }
        for post in nd_procgen::street::ped_signal_posts(&sim.world.roads, net).into_iter().filter(|p| !nd_procgen::street::crosswalk_closed(&closed, p.crosswalk)) {
            commands.spawn((
                Mesh3d(mats.signal_mesh.clone()),
                MeshMaterial3d(mats.ped_stop.clone()),
                Transform::from_translation(post.lamp).with_rotation(Quat::from_rotation_y(-post.heading)).with_scale(Vec3::new(1.0, 1.4, 1.0)),
                NotShadowCaster,
                SignalTag(LaneId::MAX),
                PedSignalTag(post.inter, post.approach),
            ));
        }
        return;
    }
    for (tag, ped, mut mat, heat_original) in &mut q {
        // Under heat vision the entity shows a stand-in; what it stands in
        // for is what counts.
        let shown = heat_original.map_or(mat.0.clone(), |h| h.0.clone());
        if let Some(p) = ped {
            let wanted = if sim.world.traffic.peds.walk_shown(p.0, p.1) { &mats.ped_walk } else { &mats.ped_stop };
            if shown != *wanted {
                mat.0 = wanted.clone();
            }
            continue;
        }
        let Some(lane) = net.lanes.get(tag.0 as usize) else { continue };
        let green = lane.next.iter().any(|&n| net.lanes[n as usize].green);
        let wanted = if green { &mats.signal_green } else { &mats.signal_red };
        if shown != *wanted {
            mat.0 = wanted.clone();
        }
    }
}

/// Bus stops: a pole, a sign, a kerb mark and a shelter at every stop of
/// every line, redrawn when a line is edited.
pub(super) fn sync_transit(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<TransitTag>>,
) {
    let w = &sim.world;
    let inputs = (sim.generation, w.transit.version, w.roads.version());
    if w.transit.version == state.transit_version && !assets.palette_changed && !sim.full_rebuild {
        if existing.is_empty() == w.transit.routes.is_empty() {
            return;
        }
        // Routes but nothing drawn: the last build found no stop to draw.
        // Build again only when the routes or the roads have moved.
        if existing.is_empty() && state.transit_empty.same(inputs) {
            return;
        }
    }
    state.transit_empty = Gate(Some(inputs));
    state.transit_version = w.transit.version;
    despawn_all(&mut commands, &existing);
    let mut stops: Vec<(SVec2, SVec2, SVec2)> = Vec::new();
    for route in w.transit.routes.values() {
        for stop in &route.stops {
            if !w.roads.segment(stop.segment).alive {
                continue;
            }
            let (a, b) = w.roads.segment_points(stop.segment);
            let mut d = (b - a).normalize_or_zero();
            let mut right = SVec2::new(-d.y, d.x);
            let (_, q) = nd_sim::roads::project_on_segment(stop.pos, a, b);
            if (stop.pos - q).dot(right) < 0.0 {
                d = -d;
                right = -right;
            }
            stops.push((stop.pos, d, right));
        }
    }
    if stops.is_empty() {
        return;
    }
    let geo = bus_stops(&stops, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, TransitTag);
}

/// [tolls] Toll booths and plazas across the roads, redrawn when a booth
/// is placed, removed or repriced, or the roads are rebuilt.
pub(super) fn sync_tolls(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<TollTag>>,
) {
    let w = &sim.world;
    let version = w.tolls.version + 1;
    if version == state.toll_version && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.toll_version = version;
    despawn_all(&mut commands, &existing);
    let draws: Vec<nd_procgen::TollDraw> = w
        .tolls
        .booths
        .iter()
        .filter_map(|b| {
            let sid = b.segment?;
            let seg = w.roads.try_segment(sid)?;
            // How far the plaza may spread either side before it would lie
            // on another road (a highway's other carriageway, a street
            // alongside): walk out across the road until one is underfoot.
            let right = SVec2::new(-b.dir.y, b.dir.x);
            let room = [-1.0f32, 1.0].map(|side| {
                let mut d = seg.half_width();
                while d < 30.0 {
                    let p = b.pos + right * side * d;
                    if let Some((other, _, q)) = w.roads.nearest_where(p, |s| !s.is_rail() && !s.tunnel) {
                        if other != sid && q.distance(p) < w.roads.segment(other).half_width() + 0.3 {
                            return (d - 0.3).max(seg.half_width());
                        }
                    }
                    d += 0.5;
                }
                30.0
            });
            Some(nd_procgen::TollDraw { pos: b.pos, dir: b.dir, lanes: seg.lanes(), half_width: seg.half_width(), plaza: b.size == nd_sim::tolls::TollSize::Plaza, id: b.id, room })
        })
        .collect();
    if draws.is_empty() {
        return;
    }
    let geo = nd_procgen::toll_booths(&draws, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, TollTag);
}

/// [pt-ux] Which street surfaces the underground view keeps as see-through
/// ghosts over the tunnels: the carriageway, pavements and kerbs, not
/// trees, lamps, poles or furniture.
fn ghosts_underground(s: nd_procgen::Surface) -> bool {
    use nd_procgen::Surface as S;
    matches!(s, S::Asphalt | S::WornAsphalt | S::Paving | S::WornPaving | S::Concrete | S::WornConcrete | S::Dirt | S::Tactile)
}
