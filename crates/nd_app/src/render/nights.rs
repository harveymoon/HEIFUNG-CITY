//! [events] What the nights and the years leave on the streets: light beams
//! sweeping the sky over a warehouse rave and coloured light spilling from
//! it; the race's route lit up while the cars are on it and a pulsing ring
//! at the meet and the after-party; cracks, patches and potholes on worn
//! street pieces (rebuilt once a night, when the wear changes); and the
//! Road condition layer, a ribbon down every street from green to red.

use super::*;
use nd_sim::road_wear::wears;
use nd_sim::street_events::RaceStage;

#[derive(Component)]
pub(super) struct CrackTag;
#[derive(Component)]
pub(super) struct WearLayerTag;
#[derive(Component)]
pub(super) struct RaveTag;

/// Green (new) through yellow to red (crumbling), `t` the wear `0..1`.
pub fn road_wear_color(t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        let u = t / 0.5;
        [0.15 + 0.85 * u, 0.85, 0.3 - 0.15 * u]
    } else {
        let u = (t - 0.5) / 0.5;
        [1.0, 0.85 - 0.72 * u, 0.15 - 0.05 * u]
    }
}

fn mix(a: u64, b: u64) -> u64 {
    let mut x = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// A street piece's frame: point at `u` metres along and `v` across, at the
/// road surface (plus `lift`).
struct Frame<'a> {
    w: &'a nd_sim::World,
    sid: nd_sim::SegmentId,
    a: SVec2,
    dir: SVec2,
    side: SVec2,
    len: f32,
}

impl Frame<'_> {
    fn at(&self, u: f32, v: f32, lift: f32) -> Vec3 {
        let p = self.a + self.dir * u + self.side * v;
        let y = self.w.road_height(self.sid, (u / self.len).clamp(0.0, 1.0)) + lift;
        Vec3::new(p.x, y, p.y)
    }

    /// A thin strip on the surface from (u0, v0) to (u1, v1).
    fn strip(&self, m: &mut MeshData, p: (f32, f32), q: (f32, f32), width: f32, lift: f32, color: [f32; 4]) {
        let d = SVec2::new(q.0 - p.0, q.1 - p.1);
        let n = SVec2::new(-d.y, d.x).normalize_or_zero() * (width * 0.5);
        let c = [self.at(p.0 - n.x, p.1 - n.y, lift), self.at(q.0 - n.x, q.1 - n.y, lift), self.at(q.0 + n.x, q.1 + n.y, lift), self.at(p.0 + n.x, p.1 + n.y, lift)];
        m.quad_facing(c, Vec3::Y, color);
    }

    /// A rough blob (a pothole, a patch) centred at (u, v).
    fn blob(&self, m: &mut MeshData, u: f32, v: f32, r: (f32, f32), h: u64, lift: f32, color: [f32; 4]) {
        let n = 7;
        let ring: Vec<Vec3> = (0..n)
            .map(|k| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                let s = 0.7 + 0.5 * unit(mix(h, k as u64));
                self.at(u + a.cos() * r.0 * s, v + a.sin() * r.1 * s, lift)
            })
            .collect();
        let c = self.at(u, v, lift);
        for k in 0..n {
            m.tri_facing(c, ring[k], ring[(k + 1) % n], Vec3::Y, color);
        }
    }
}

/// Cracks from 40% wear, patches from 55%, potholes from 75%, placed by a
/// hash of where the piece is (so they stay put through road works).
fn crack_mesh(w: &nd_sim::World) -> MeshData {
    let mut m = MeshData::default();
    const CRACK: [f32; 4] = [0.015, 0.015, 0.02, 0.85];
    const HOLE: [f32; 4] = [0.01, 0.01, 0.012, 0.95];
    for i in 0..w.roads.segments.len() {
        if !wears(&w.roads, i) {
            continue;
        }
        let wear = w.road_wear.wear_of(i);
        if wear < 0.4 {
            continue;
        }
        let s = &w.roads.segments[i];
        let sid = nd_sim::SegmentId(i as u32);
        let (a, b) = w.roads.segment_points(sid);
        let len = a.distance(b);
        if len < 2.0 {
            continue;
        }
        let dir = (b - a) / len;
        let f = Frame { w, sid, a, dir, side: SVec2::new(-dir.y, dir.x), len };
        let half = ((s.lanes_fwd + s.lanes_bwd).max(1) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 - 0.4).max(1.0);
        let mid = (a + b) * 0.5;
        let seed = mix((mid.x * 4.0).round() as i64 as u64, (mid.y * 4.0).round() as i64 as u64);
        // Cracks: short zigzags, some along the wheel tracks, some across.
        let n = ((len / 4.0) * (wear - 0.35) * 2.2).ceil().min(28.0) as usize;
        for k in 0..n {
            let h = mix(seed, k as u64);
            let mut p = (0.5 + unit(h) * (len - 1.0), (unit(h >> 8) * 2.0 - 1.0) * half);
            let along = k % 3 != 0;
            let sign = if unit(h >> 16) < 0.5 { -1.0 } else { 1.0 };
            for j in 0..3u64 {
                let hj = mix(h, j + 1);
                let (du, dv) = if along { (1.0 + unit(hj) * 1.8, (unit(hj >> 9) - 0.5) * 0.8) } else { ((unit(hj) - 0.5) * 0.8, sign * (0.6 + unit(hj >> 9) * 1.0)) };
                let q = ((p.0 + du).clamp(0.2, len - 0.2), (p.1 + dv).clamp(-half, half));
                f.strip(&mut m, p, q, 0.1 + 0.12 * wear, 0.075, CRACK);
                p = q;
            }
        }
        // Worn wheel tracks down every lane, darker the worse it gets.
        if wear >= 0.5 {
            let lanes = (s.lanes_fwd + s.lanes_bwd).max(1);
            let total = lanes as f32 * nd_sim::roads::LANE_WIDTH;
            let a = 0.12 + 0.35 * (wear - 0.5);
            for k in 0..lanes {
                let c = -total * 0.5 + (k as f32 + 0.5) * nd_sim::roads::LANE_WIDTH;
                for off in [-0.85, 0.85] {
                    f.strip(&mut m, (0.0, c + off), (len, c + off), 0.55, 0.068, [0.01, 0.01, 0.012, a]);
                }
            }
        }
        // Patches: rectangles of newer (dark) or older (pale) tar.
        if wear >= 0.55 {
            let n = ((len / 14.0) * (wear - 0.5) * 3.0).ceil().min(10.0) as usize;
            for k in 0..n {
                let h = mix(seed ^ 0xBA7C, k as u64);
                let (u, v) = (1.0 + unit(h) * (len - 2.0), (unit(h >> 8) * 2.0 - 1.0) * (half - 0.8));
                let (pl, pw) = (0.8 + unit(h >> 16) * 1.8, 0.6 + unit(h >> 24) * 1.0);
                let col = if k % 2 == 0 { [0.035, 0.035, 0.04, 0.9] } else { [0.16, 0.16, 0.17, 0.85] };
                f.strip(&mut m, ((u - pl).max(0.2), v), ((u + pl).min(len - 0.2), v), pw * 2.0, 0.07, col);
            }
        }
        // Potholes.
        if wear >= 0.75 {
            let n = ((len / 12.0) * (wear - 0.7) * 4.0).ceil().min(8.0) as usize;
            for k in 0..n {
                let h = mix(seed ^ 0x9071, k as u64);
                let (u, v) = (0.8 + unit(h) * (len - 1.6), (unit(h >> 8) * 2.0 - 1.0) * (half - 0.5));
                let r = 0.35 + unit(h >> 16) * 0.5;
                f.blob(&mut m, u, v, (r * 1.3, r), h, 0.08, HOLE);
            }
        }
    }
    m
}

/// The Road condition layer: a ribbon down every street piece.
fn wear_ribbons(w: &nd_sim::World) -> MeshData {
    let mut m = MeshData::default();
    for i in 0..w.roads.segments.len() {
        if !wears(&w.roads, i) {
            continue;
        }
        let s = &w.roads.segments[i];
        let sid = nd_sim::SegmentId(i as u32);
        let (a, b) = w.roads.segment_points(sid);
        let len = a.distance(b);
        if len < 0.5 {
            continue;
        }
        let dir = (b - a) / len;
        let f = Frame { w, sid, a, dir, side: SVec2::new(-dir.y, dir.x), len };
        let wear = w.road_wear.wear_of(i);
        let c = road_wear_color(wear);
        let width = ((s.lanes_fwd + s.lanes_bwd).max(1) as f32 * nd_sim::roads::LANE_WIDTH * 0.75).max(2.5);
        f.strip(&mut m, (0.0, 0.0), (len, 0.0), width, 0.5, [c[0], c[1], c[2], 0.75]);
    }
    m
}

#[derive(Default)]
pub(super) struct WearState {
    cracks: Option<(u64, u64, usize)>,
    layer: Option<(u64, u64, f32)>,
    material: Option<Handle<StandardMaterial>>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_road_wear(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    time: Res<Time>,
    mut st: Local<WearState>,
    cracks: Query<Entity, With<CrackTag>>,
    layer: Query<Entity, With<WearLayerTag>>,
) {
    let w = &sim.world;
    let key = (w.road_wear.version, w.roads.version, w.road_wear.wear.len());
    if sim.full_rebuild || st.cracks != Some(key) {
        despawn_all(&mut commands, &cracks);
        st.cracks = Some(key);
        let m = crack_mesh(w);
        if !m.is_empty() {
            let mat = st
                .material
                .get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, alpha_mode: AlphaMode::Blend, depth_bias: 2.0, ..default() }))
                .clone();
            commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster, CrackTag));
        }
    }
    if !overlays.road_condition {
        if st.layer.take().is_some() {
            despawn_all(&mut commands, &layer);
        }
        return;
    }
    // Rebuilt when the wear changes, and every few seconds anyway (a new
    // road built while the layer is up).
    let now = time.elapsed_secs();
    let due = match st.layer {
        Some((v, r, at)) => v != w.road_wear.version || r != w.roads.version || now - at > 5.0 || sim.full_rebuild,
        None => true,
    };
    if due {
        despawn_all(&mut commands, &layer);
        let m = wear_ribbons(w);
        if !m.is_empty() {
            commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, WearLayerTag));
        }
        st.layer = Some((w.road_wear.version, w.roads.version, now));
    }
}

/// The colours a rave throws: hot pink, cyan, violet, acid green.
const RAVE_COLORS: [[f32; 3]; 4] = [[1.0, 0.15, 0.75], [0.1, 0.85, 1.0], [0.55, 0.2, 1.0], [0.5, 1.0, 0.2]];

#[derive(Default)]
pub(super) struct RaveState {
    /// The rave (id) or race party (id + 1<<31) the lights belong to.
    showing: Option<u32>,
    beams: Vec<Entity>,
    lights: Vec<Entity>,
    beam_mesh: Option<Handle<Mesh>>,
    beam_mats: Vec<Handle<StandardMaterial>>,
    roof: f32,
    centre: Vec3,
}

/// Beams over the warehouse and light pouring out of it while a rave is on;
/// lights over the lot where a race parties.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_raves(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    time: Res<Time>,
    mut st: Local<RaveState>,
    mut tf: Query<&mut Transform, With<RaveTag>>,
    mut lights: Query<&mut PointLight, With<RaveTag>>,
) {
    let w = &sim.world;
    let party = w.race().filter(|r| r.stage == RaceStage::Party);
    let want: Option<(u32, Vec3, f32, usize)> = if let Some(r) = w.rave() {
        let floors = w.buildings.get(r.building).map_or(2, |b| b.floors.max(1)) as f32;
        let ground = w.surface_height(r.centre);
        Some((r.id, Vec3::new(r.centre.x, ground, r.centre.y), floors * 3.6 + 2.0, 4))
    } else {
        party.map(|r| (r.id | 1 << 31, Vec3::new(r.finish.centre.x, w.surface_height(r.finish.centre), r.finish.centre.y), 1.0, 0))
    };
    if want.map(|x| x.0) != st.showing || sim.full_rebuild {
        let st = &mut *st;
        for e in st.beams.drain(..).chain(st.lights.drain(..)) {
            commands.entity(e).despawn();
        }
        st.showing = None;
        if let Some((id, centre, roof, beams)) = want {
            if st.beam_mesh.is_none() {
                // A long, faint cone of light from its base upwards.
                let mut m = MeshData::default();
                m.cylinder(SVec2::ZERO, 0.35, 5.5, 0.0, 160.0, 10, [1.0, 1.0, 1.0, 1.0]);
                st.beam_mesh = Some(meshes.add(to_mesh(&m, false)));
                for c in RAVE_COLORS {
                    st.beam_mats.push(materials.add(StandardMaterial {
                        base_color: Color::linear_rgba(c[0] * 0.6, c[1] * 0.6, c[2] * 0.6, 0.08),
                        emissive: LinearRgba::rgb(c[0] * 2.5, c[1] * 2.5, c[2] * 2.5),
                        unlit: true,
                        alpha_mode: AlphaMode::Add,
                        cull_mode: None,
                        double_sided: true,
                        ..default()
                    }));
                }
            }
            for k in 0..beams {
                let e = commands.spawn((Mesh3d(st.beam_mesh.clone().unwrap()), MeshMaterial3d(st.beam_mats[k % st.beam_mats.len()].clone()), Transform::from_translation(centre + Vec3::Y * roof), NotShadowCaster, NotShadowReceiver, RaveTag)).id();
                st.beams.push(e);
            }
            let n_lights = if beams > 0 { 3 } else { 2 };
            for k in 0..n_lights {
                let c = RAVE_COLORS[k % RAVE_COLORS.len()];
                let e = commands
                    .spawn((PointLight { color: Color::linear_rgb(c[0], c[1], c[2]), intensity: 0.0, range: 60.0, radius: 0.5, shadow_maps_enabled: false, ..default() }, Transform::from_translation(centre + Vec3::new(0.0, roof + 2.0, 0.0)), RaveTag))
                    .id();
                st.lights.push(e);
            }
            st.roof = roof;
            st.centre = centre;
            st.showing = Some(id);
        }
    }
    if st.showing.is_none() {
        return;
    }
    let t = time.elapsed_secs();
    // Beams sweep: each its own phase, tilted out and swinging round.
    for (k, &e) in st.beams.iter().enumerate() {
        if let Ok(mut x) = tf.get_mut(e) {
            let ph = k as f32 * 1.7;
            let yaw = t * (0.6 + 0.15 * k as f32) + ph;
            let tilt = 0.35 + 0.25 * (t * 0.9 + ph).sin();
            x.translation = st.centre + Vec3::new(((k as f32) - 1.5) * 3.0, st.roof, 0.0);
            x.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt);
        }
    }
    // The lights pulse to a beat (128 bpm), each colour on its own off-beat.
    let beat = t * 128.0 / 60.0;
    for (k, &e) in st.lights.iter().enumerate() {
        if let Ok(mut l) = lights.get_mut(e) {
            let ph = (beat + k as f32 * 0.33).fract();
            l.intensity = 2.0e6 * (1.0 - ph).powi(3) + 2.0e5;
        }
        if let Ok(mut x) = tf.get_mut(e) {
            let a = t * 0.7 + k as f32 * 2.1;
            x.translation = st.centre + Vec3::new(a.cos() * 6.0, st.roof + 1.5, a.sin() * 6.0);
        }
    }
}

/// The race on the map: a pulsing ring where the cars gather and where
/// they party, and the route lit up in electric blue while they are on it,
/// with the start and finish gates.
pub(super) fn race_gizmos(sim: Res<Sim>, time: Res<Time>, mut gizmos: Gizmos) {
    let w = &sim.world;
    let Some(r) = w.race() else { return };
    let t = time.elapsed_secs();
    let blue = Color::linear_rgb(0.3, 1.4, 3.0);
    let pink = Color::linear_rgb(3.0, 0.4, 2.0);
    let ring = |gizmos: &mut Gizmos, p: SVec2, col: Color, radius: f32| {
        let y = w.surface_height(p) + 0.4;
        let pulse = (t * 1.2).fract();
        let iso = Isometry3d::new(Vec3::new(p.x, y, p.y), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
        gizmos.circle(iso, radius * (0.6 + 0.6 * pulse), col.with_alpha(1.0 - pulse));
        gizmos.circle(iso, radius * 0.55, col);
    };
    let gate = |gizmos: &mut Gizmos, p: SVec2, col: Color| {
        let y = w.surface_height(p);
        for k in 0..8 {
            let y0 = y + k as f32 * 0.8;
            let c = if k % 2 == 0 { Color::WHITE } else { col };
            gizmos.line(Vec3::new(p.x - 0.4, y0, p.y), Vec3::new(p.x - 0.4, y0 + 0.8, p.y), c);
            gizmos.line(Vec3::new(p.x + 0.4, y0, p.y), Vec3::new(p.x + 0.4, y0 + 0.8, p.y), c);
        }
    };
    match r.stage {
        RaceStage::Gathering => ring(&mut gizmos, r.meet.centre, blue, (r.meet.width.max(r.meet.depth) * 0.6).clamp(10.0, 30.0)),
        RaceStage::Racing => {
            let lanes = &w.traffic.net.lanes;
            let flash = if (t * 4.0) as i64 % 2 == 0 { blue } else { Color::linear_rgb(0.8, 1.8, 3.0) };
            for &l in &r.route {
                let Some(lane) = lanes.get(l as usize) else { continue };
                let pts: Vec<Vec3> = lane.points.iter().map(|p| Vec3::new(p.x, w.surface_height(*p) + 0.6, p.y)).collect();
                gizmos.linestrip(pts, flash);
            }
            gate(&mut gizmos, r.meet.kerb, blue);
            gate(&mut gizmos, r.finish.kerb, pink);
        }
        RaceStage::Party => ring(&mut gizmos, r.finish.centre, pink, (r.finish.width.max(r.finish.depth) * 0.6).clamp(10.0, 30.0)),
    }
}
