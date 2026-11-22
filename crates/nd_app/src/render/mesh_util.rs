//! Mesh conversion and spawning helpers shared by the renderers.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// Convert mesh data; lit meshes get tangents so normal maps work.
///
/// The CPU copy is dropped once the mesh is uploaded: nothing reads vertices
/// back, and the generated city is hundreds of megabytes of them. Vertex
/// colours stay `Float32x4` because that is the only format Bevy accepts for
/// `ATTRIBUTE_COLOR`, and the generators use values above 1 for emissive
/// surfaces anyway.
pub fn to_mesh(data: &MeshData, tangents: bool) -> Mesh {
    mesh_with_usage(data, tangents, RenderAssetUsages::RENDER_WORLD)
}

/// A mesh many entities share over the whole run (citizens, cars, signal
/// lamps). These keep their CPU copy: Bevy computes an entity's bounds from
/// the main-world mesh when the entity is spawned, and movers are spawned
/// long after the mesh was first uploaded.
pub(super) fn shared_mesh(data: &MeshData, tangents: bool) -> Mesh {
    mesh_with_usage(data, tangents, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD)
}

pub(super) fn mesh_with_usage(data: &MeshData, tangents: bool, usage: RenderAssetUsages) -> Mesh {
    // Bevy's mesh allocator skips allocating a mesh with no vertices but
    // still copies its (empty) data, which logs "Use-after-free: attempted
    // to copy element data for an unallocated key" every time such a mesh
    // is extracted. An empty mesh becomes one zero-area triangle instead.
    let placeholder;
    let data = if data.positions.is_empty() || data.indices.is_empty() {
        let mut m = MeshData::default();
        for _ in 0..3 {
            m.positions.push([0.0, 0.0, 0.0]);
            m.normals.push([0.0, 1.0, 0.0]);
            m.colors.push([0.0, 0.0, 0.0, 1.0]);
            m.uvs.push([0.0, 0.0]);
            m.tangents.push([1.0, 0.0, 0.0, 1.0]);
        }
        m.indices.extend_from_slice(&[0, 1, 2]);
        placeholder = m;
        &placeholder
    } else {
        data
    };
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone())
        .with_inserted_indices(Indices::U32(data.indices.clone()));
    // [r12-grime] The weathering channel, as the second UV set.
    if !data.aux.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, data.aux_full());
    }
    if tangents && !data.indices.is_empty() {
        if data.tangents.len() == data.positions.len() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, data.tangents.clone());
        } else {
            let _ = mesh.generate_tangents();
        }
    }
    mesh
}

pub(super) fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

pub(super) fn scale(c: [f32; 3], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, 1.0]
}

/// Every non-empty surface of a geometry with the layer it is in: the base
/// mesh, then the detail layers ([`nd_procgen::Lod::DETAIL`] order).
pub(super) fn geometry_layers(geo: &Geometry) -> impl Iterator<Item = (nd_procgen::Lod, Surface, &MeshData)> {
    geo.iter().map(|(s, m)| (nd_procgen::Lod::Base, s, m)).chain(nd_procgen::Lod::DETAIL.into_iter().flat_map(move |l| geo.detail_iter(l).map(move |(s, m)| (l, s, m))))
}

/// Spawn one entity per non-empty surface of a geometry. Glow strips are
/// thin unlit decals: they neither cast nor receive shadows, which also
/// keeps them out of the shadow passes.
///
/// [r13-detail] The detail layers come too, each within its distance band
/// ([`detail_range`]), as a building chunk's are: parks, vacant lots, the
/// undercroft and every other world-placed geometry fade their detail at
/// the same distances. (Previews and the viewer draw their own meshes.)
pub(super) fn spawn_geometry<T: Component + Clone>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, geo: &Geometry, tag: T) -> Vec<Entity> {
    let mut out = Vec::new();
    for (lod, s, data) in geometry_layers(geo) {
        let mesh = to_mesh(data, !s.is_glow());
        let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, tag.clone()));
        if s.is_glow() {
            e.insert((NotShadowCaster, NotShadowReceiver));
        }
        if lod != nd_procgen::Lod::Base {
            e.insert(detail_range(lod));
        }
        out.push(e.id());
    }
    out
}

/// [r13-detail] The `VisibilityRange` of a shared detail band
/// ([`nd_procgen::DetailBand`]): drawn, and casting shadows, only within
/// its range of the camera, fading out over its last part. `use_aabb`:
/// measured to the mesh's bounds (merged meshes: a chunk's detail), else to
/// its origin (an instanced prop).
pub(super) fn band_range(band: nd_procgen::DetailBand, use_aabb: bool) -> VisibilityRange {
    VisibilityRange { start_margin: 0.0..0.0, end_margin: band.fade(), use_aabb }
}

/// [r10-chunk-hitch] The distance range a geometry detail layer is drawn
/// within: its band's ([`band_range`]).
pub(super) fn detail_range(lod: nd_procgen::Lod) -> VisibilityRange {
    band_range(nd_procgen::DetailBand::of(lod).unwrap_or(nd_procgen::DetailBand::Mid), true)
}

/// Spawn a glow-surface mesh: unlit, no shadows either way.
pub(super) fn spawn_glow<T: Component>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, data: &MeshData, tag: T) {
    commands.spawn((Mesh3d(meshes.add(to_mesh(data, false))), MeshMaterial3d(mats.glow()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, tag));
}

/// Spawn a translucent overlay mesh (heat map, haze): no shadows either way.
pub(super) fn spawn_overlay<T: Component>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, data: &MeshData, tag: T) {
    commands.spawn((Mesh3d(meshes.add(to_mesh(data, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, tag));
}

/// [island-lots] `pts` with points added along every leg longer than
/// `step`, so a line lifted onto the ground vertex by vertex follows it.
pub(super) fn densify(pts: &[SVec2], step: f32) -> Vec<SVec2> {
    let mut out = Vec::with_capacity(pts.len());
    for (k, &p) in pts.iter().enumerate() {
        if k > 0 {
            let a = pts[k - 1];
            let n = (a.distance(p) / step).floor() as usize;
            for i in 1..=n.min(256) {
                out.push(a.lerp(p, i as f32 / (n + 1) as f32));
            }
        }
        out.push(p);
    }
    out
}

/// Hash helper for change detection on generated geometry inputs.
pub(super) fn hash_f32s<H: Hasher>(h: &mut H, pts: impl IntoIterator<Item = SVec2>) {
    for p in pts {
        p.x.to_bits().hash(h);
        p.y.to_bits().hash(h);
    }
}

/// Terrain extents follow the buildable square: the hills start a little
/// past the edge of the city and the mesh reaches twice as far out.
pub(super) fn terrain_config(w: &World) -> TerrainConfig {
    let half = w.config.half_extent;
    // On a landscape the scatter is generated flat and set on the ground
    // by `lift_spawned`, so it must not carry the old hills itself.
    let hill_height = if w.landscape.is_flat() { TerrainConfig::default().hill_height } else { 0.0 };
    // The flat map's roads lie at zero: the hills are levelled along those
    // running out of the city (the highway, the trunk, roads off the map).
    let corridors = if w.landscape.is_flat() { far::far_ways(w) } else { Vec::new() };
    TerrainConfig { half_size: half * 2.0, basin_radius: half + 100.0, hill_height, corridors, ..TerrainConfig::default() }
}

pub(super) fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = b - a;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    a + d * t
}

pub(super) fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub(super) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
