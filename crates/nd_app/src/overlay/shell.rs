//! [feat/pt-ux] The selected building glows: a see-through gold shell over
//! its whole shape (walls and roof, `tools::building_prism`), breathing in
//! and out, so a tower picked by its walls reads as picked from anywhere
//! and the selection is the building, not only its plot.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use nd_sim::{BuildingId, Vec2 as SVec2};

use crate::selection::Selection;
use crate::sim::Sim;

#[derive(Resource, Default)]
pub struct Shell {
    /// The shell's entity, its material, and what its mesh was built for.
    ent: Option<(Entity, Handle<StandardMaterial>)>,
    key: Option<(BuildingId, u64, i32)>,
}

/// How bright the shell is at `t` seconds: a slow breath between a faint
/// and a clear glow.
pub(crate) fn breath(t: f32) -> f32 {
    0.5 + 0.5 * (t * 3.2).sin()
}

/// The prism's walls and roof, pushed out a little so it wraps the drawn
/// building, as a triangle mesh (positions and normals).
pub(crate) fn shell_mesh(poly: &[SVec2], base: f32, top: f32) -> Mesh {
    let n = poly.len().max(1) as f32;
    let c = poly.iter().copied().fold(SVec2::ZERO, |a, p| a + p) / n;
    let out: Vec<SVec2> = poly.iter().map(|&p| p + (p - c).normalize_or_zero() * 0.6).collect();
    let (base, top) = (base - 0.2, top + 0.6);
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for i in 0..out.len() {
        let (a, b) = (out[i], out[(i + 1) % out.len()]);
        let e = (b - a).normalize_or_zero();
        let normal = [e.y, 0.0, -e.x];
        let k = pos.len() as u32;
        for (p, y) in [(a, base), (b, base), (b, top), (a, top)] {
            pos.push([p.x, y, p.y]);
            nrm.push(normal);
        }
        idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    // The roof: a fan from the middle (lots are near enough convex).
    let k = pos.len() as u32;
    pos.push([c.x, top, c.y]);
    nrm.push([0.0, 1.0, 0.0]);
    for p in &out {
        pos.push([p.x, top, p.y]);
        nrm.push([0.0, 1.0, 0.0]);
    }
    for i in 0..out.len() as u32 {
        idx.extend([k, k + 1 + i, k + 1 + (i + 1) % out.len() as u32]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_indices(Indices::U32(idx));
    m
}

#[allow(clippy::too_many_arguments)]
pub fn sync_shell(
    mut commands: Commands,
    sim: Res<Sim>,
    sel: Res<Selection>,
    tools: Res<crate::tools::ToolState>,
    mode: Res<crate::camera::CameraMode>,
    time: Res<Time>,
    mut shell: ResMut<Shell>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let w = &sim.world;
    let want = match *sel {
        Selection::Building(b) if tools.depth != crate::tools::Depth::Underground && *mode != crate::camera::CameraMode::Walk => crate::tools::building_prism(w, &sim.building_tops, b).map(|p| (b, p)),
        _ => None,
    };
    let Some((b, (poly, base, top))) = want else {
        if let Some((e, _)) = shell.ent.take() {
            commands.entity(e).despawn();
        }
        shell.key = None;
        return;
    };
    let shape = poly.iter().fold(0u64, |h, p| h.rotate_left(7) ^ (p.x.to_bits() as u64) ^ ((p.y.to_bits() as u64) << 32));
    let key = (b, shape, ((top - base) * 4.0) as i32);
    if shell.key != Some(key) || shell.ent.is_none() || sim.full_rebuild {
        if let Some((e, _)) = shell.ent.take() {
            commands.entity(e).despawn();
        }
        let mat = materials.add(StandardMaterial {
            base_color: Color::linear_rgba(1.0, 0.7, 0.2, 0.2),
            emissive: LinearRgba::rgb(1.6, 1.1, 0.3),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        let mesh = meshes.add(shell_mesh(&poly, base, top));
        let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NotShadowCaster, Name::new("selection shell"))).id();
        shell.ent = Some((e, mat));
        shell.key = Some(key);
    }
    // Breathe.
    if let Some((_, mat)) = &shell.ent {
        if let Some(mut m) = materials.get_mut(mat) {
            let k = breath(time.elapsed_secs());
            let a = 0.10 + 0.22 * k;
            // Added onto what is behind (the alpha is 1: `Add` scales by it).
            m.base_color = Color::linear_rgba(1.0 * a, 0.72 * a, 0.22 * a, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_has_a_wall_per_edge_and_a_roof() {
        let sq = [SVec2::new(0.0, 0.0), SVec2::new(10.0, 0.0), SVec2::new(10.0, 10.0), SVec2::new(0.0, 10.0)];
        let m = shell_mesh(&sq, 0.0, 30.0);
        let tris = match m.indices() {
            Some(i) => i.len() / 3,
            None => 0,
        };
        assert_eq!(tris, 4 * 2 + 4);
        assert!((0..100).all(|i| (0.0..=1.0).contains(&breath(i as f32 * 0.1))));
    }
}
