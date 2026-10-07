//! The sky: a dome of vertex colours round the camera, painted from the
//! colours `update_daylight` picks for the hour. A flat clear colour gave a
//! night sky as black at the horizon as overhead; a real city at night lights
//! its own haze from below, so the dome fades from the fog's colour at the
//! horizon (where the distant ground already dissolves into it, so the two
//! meet without a line) through a band of light pollution, warm sodium and
//! neon magenta over a big city, to a dark zenith. By day the same dome
//! carries a pale horizon and the glare round the sun, a cheap stand-in for
//! atmospheric scattering that costs one small mesh and no extra pass.
//!
//! The dome is far out (the projection has no far plane, so it is always
//! behind the world), unlit, out of the fog and the shadows, and follows the
//! camera's position, never its rotation.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::MeshData;

use super::Lighting;

/// Radius of the dome: past anything drawn (the fog is solid at
/// `landscape::SIGHT`, a few kilometres).
const RADIUS: f32 = 30_000.0;
const RINGS: usize = 18;
const SEGMENTS: usize = 48;

/// The sky as `update_daylight` last chose it, linear RGB.
#[derive(Resource, Clone, PartialEq)]
pub(super) struct SkyLook {
    /// Straight up.
    pub zenith: [f32; 3],
    /// At the horizon: the fog's colour.
    pub horizon: [f32; 3],
    /// A band just above the horizon: the city's light pollution at night,
    /// the bright haze by day.
    pub glow: [f32; 3],
    /// How far up the glow band reaches, `0..1` of the way to the zenith.
    pub glow_height: f32,
    /// Toward the sun (or moon): direction and the glare colour round it.
    pub sun_dir: Vec3,
    pub sun_glare: [f32; 3],
    /// Bumped on every change so the dome is repainted.
    pub version: u64,
}

impl Default for SkyLook {
    fn default() -> Self {
        Self { zenith: [0.01, 0.01, 0.03], horizon: [0.05, 0.045, 0.06], glow: [0.05, 0.045, 0.06], glow_height: 0.2, sun_dir: Vec3::Y, sun_glare: [0.0; 3], version: 0 }
    }
}

#[derive(Component)]
pub(super) struct SkyDome;

/// Colour of the dome in direction `d` (unit vector, y up).
fn sky_color(l: &SkyLook, d: Vec3) -> [f32; 3] {
    if d.y < 0.0 {
        // Below the horizon: the fog, so a glimpse under the ground's edge
        // is the haze the ground fades into.
        return l.horizon;
    }
    let up = d.y;
    // Horizon to glow band to zenith, the band fading out quickly as the
    // haze thins with height.
    let band = (1.0 - up / l.glow_height.max(0.02)).clamp(0.0, 1.0);
    let band = band * band;
    let k = up.powf(0.55);
    let mut c = [0.0; 3];
    for i in 0..3 {
        let base = l.horizon[i] + (l.zenith[i] - l.horizon[i]) * k;
        c[i] = base + (l.glow[i] - base) * band * (1.0 - smoothstep(0.0, 0.04, 0.04 - up) * 0.5);
    }
    // Glare round the sun: a broad halo and a tighter core (Mie-like),
    // stronger low in the sky where the light crosses more air.
    let cosang = d.dot(l.sun_dir).max(0.0);
    let halo = cosang.powf(6.0) * 0.45 + cosang.powf(48.0) * 1.2;
    let low = 1.0 - up * 0.6;
    for i in 0..3 {
        c[i] += l.sun_glare[i] * halo * low;
    }
    c
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The dome: a sphere seen from inside, denser near the horizon where the
/// colours change fastest.
fn dome_mesh(l: &SkyLook) -> MeshData {
    let mut m = MeshData::default();
    // Elevation of each ring, from a little under the horizon to the top.
    let elev: Vec<f32> = (0..=RINGS)
        .map(|r| {
            let t = r as f32 / RINGS as f32;
            -0.15 + (std::f32::consts::FRAC_PI_2 + 0.15) * t * t
        })
        .collect();
    for e in &elev {
        for s in 0..=SEGMENTS {
            let a = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let d = Vec3::new(e.cos() * a.cos(), e.sin(), e.cos() * a.sin());
            let p = d * RADIUS;
            m.positions.push([p.x, p.y, p.z]);
            m.normals.push([-d.x, -d.y, -d.z]);
            let c = sky_color(l, d);
            m.colors.push([c[0], c[1], c[2], 1.0]);
            m.uvs.push([0.0, 0.0]);
        }
    }
    let row = SEGMENTS as u32 + 1;
    for r in 0..RINGS as u32 {
        for s in 0..SEGMENTS as u32 {
            let i = r * row + s;
            // Wound to face inwards.
            m.indices.extend_from_slice(&[i, i + 1, i + row, i + 1, i + row + 1, i + row]);
        }
    }
    m
}

fn repaint(mesh: &mut Mesh, l: &SkyLook) {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let colors: Vec<[f32; 4]> = pos
        .iter()
        .map(|p| {
            let c = sky_color(l, Vec3::from_array(*p).normalize_or_zero());
            [c[0], c[1], c[2], 1.0]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
}

/// Spawn the dome on first use, repaint it when the sky changes, keep it
/// on the camera, and hide it under heat vision (whose sky is black).
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    look: Res<SkyLook>,
    lighting: Res<Lighting>,
    cams: Query<&GlobalTransform, (With<Camera3d>, Without<SkyDome>)>,
    mut dome: Query<(&Mesh3d, &mut Transform, &mut Visibility), With<SkyDome>>,
    mut painted: Local<Option<u64>>,
) {
    let Ok((mesh, mut tf, mut vis)) = dome.single_mut() else {
        let data = dome_mesh(&look);
        let mat = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, fog_enabled: false, cull_mode: None, ..default() });
        commands.spawn((Mesh3d(meshes.add(super::shared_mesh(&data, false))), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, SkyDome, bevy::camera::visibility::NoFrustumCulling));
        *painted = Some(look.version);
        return;
    };
    if let Some(cam) = cams.iter().next() {
        let p = cam.translation();
        tf.translation = Vec3::new(p.x, 0.0, p.z);
    }
    let want = if lighting.heat { Visibility::Hidden } else { Visibility::Inherited };
    if *vis != want {
        *vis = want;
    }
    if *painted != Some(look.version) {
        *painted = Some(look.version);
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            repaint(&mut m, &look);
        }
    }
}
