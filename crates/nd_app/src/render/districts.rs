//! [districts] The district layer: each district's cells washed in its
//! colour with a bright border, rebuilt only when the districts change
//! (`Districts::version`) or the layer is turned on. The names are drawn by
//! the UI over the map (`ui::districts::labels`).
//!
//! [tourism] Visitors on foot, drawn with the citizens' figure generator
//! and cache (`render/people.rs`) as a tourist figure: their look with a
//! daypack and a camera. They keep a small figure cache of their own:
//! the citizens' cache is often full of the looks on the street, and a
//! visitor borrowing a citizen's figure would lose the daypack that marks
//! them out.

/// Visitor figures kept at most; beyond it they borrow a visitor figure of
/// the same silhouette.
const TOURIST_FIGURES: usize = 192;

use super::*;
use nd_procgen::people::{person_mesh, tourist_key, FigureCache, PersonKey};
use nd_sim::districts::{DistrictGrid, CELL, COLORS, HALF};
use nd_sim::tourism::TouristId;

#[derive(Component, Clone)]
pub(super) struct DistrictLayerTag;

#[derive(Component)]
pub(super) struct TouristTag;

#[derive(Resource)]
pub(super) struct DistrictRender {
    /// Districts version and colour signature the layer was built for.
    shown: Option<(u64, u64)>,
    tourists: HashMap<TouristId, Entity>,
    figures: FigureCache<Handle<Mesh>>,
}

impl Default for DistrictRender {
    fn default() -> Self {
        Self { shown: None, tourists: HashMap::new(), figures: FigureCache::new(TOURIST_FIGURES) }
    }
}

/// The fills (one quad per run of cells in a row) and borders of every
/// district, flat at the datum; `lift` sets it on the ground.
pub(super) fn district_mesh(grid: &DistrictGrid, colour_of: impl Fn(u16) -> Option<[f32; 3]>) -> MeshData {
    let mut m = MeshData::default();
    for r in 0..grid.rows {
        let mut c = 0;
        while c < grid.cols {
            let id = grid.get(c, r);
            let start = c;
            while c < grid.cols && grid.get(c, r) == id {
                c += 1;
            }
            let Some(col) = (id != 0).then(|| colour_of(id)).flatten() else { continue };
            let (x0, x1) = (-HALF + start as f32 * CELL, -HALF + c as f32 * CELL);
            let (y0, y1) = (-HALF + r as f32 * CELL, -HALF + (r + 1) as f32 * CELL);
            let poly = [SVec2::new(x0, y0), SVec2::new(x1, y0), SVec2::new(x1, y1), SVec2::new(x0, y1)];
            m.ground_polygon(&poly, 0.55, [col[0] * 0.5, col[1] * 0.5, col[2] * 0.5, 0.16]);
        }
    }
    for (a, b, id) in grid.borders() {
        let Some(col) = colour_of(id) else { continue };
        // Inset the border a little into its own district so two
        // neighbours' borders sit side by side rather than on top.
        m.ground_strip(&[a, b], 1.6, 0.6, [col[0], col[1], col[2], 0.9]);
    }
    m
}

pub(super) fn sync_district_layer(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, mats: Res<Materials>, overlays: Res<Overlays>, mut state: ResMut<DistrictRender>, q: Query<Entity, With<DistrictLayerTag>>) {
    let w = &sim.world;
    if !overlays.districts {
        if state.shown.is_some() {
            despawn_all(&mut commands, &q);
            state.shown = None;
        }
        return;
    }
    let mut h = DefaultHasher::new();
    for d in &w.districts.list {
        (d.id, d.color).hash(&mut h);
    }
    let key = (w.districts.version, h.finish());
    if state.shown == Some(key) && !sim.full_rebuild {
        return;
    }
    despawn_all(&mut commands, &q);
    state.shown = Some(key);
    let colour_of = |id: u16| w.districts.get(id).map(|d| COLORS[d.color as usize % COLORS.len()]);
    let mut m = district_mesh(&w.districts.grid, colour_of);
    if m.is_empty() {
        return;
    }
    if !w.landscape.is_flat() {
        for p in &mut m.positions {
            p[1] += w.surface_height(SVec2::new(p[0], p[2]));
        }
    }
    commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, DistrictLayerTag));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_tourists(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut state: ResMut<DistrictRender>,
    mut people: ResMut<people::PeopleRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut q: Query<(&mut Transform, &mut Visibility, &mut Mesh3d), With<TouristTag>>,
) {
    let w = &sim.world;
    if sim.full_rebuild {
        // Ids are reused by a loaded world.
        for (_, e) in state.tourists.drain() {
            commands.entity(e).despawn();
        }
    }
    let look_mat = people.material(&mut materials, &mut images);
    let gone: Vec<TouristId> = state.tourists.keys().copied().filter(|id| !w.tourism.tourists.contains_key(*id)).collect();
    for id in gone {
        if let Some(e) = state.tourists.remove(&id) {
            commands.entity(e).despawn();
        }
    }
    let f = sim.frac();
    state.figures.begin_frame();
    for (id, t) in w.tourism.tourists.iter() {
        let Some(&e) = state.tourists.get(&id) else {
            let e = commands.spawn((Mesh3d(mats.citizen_mesh.clone()), MeshMaterial3d(look_mat.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, Grounded, TouristTag)).id();
            state.tourists.insert(id, e);
            continue;
        };
        let Ok((mut tf, mut vis, mut mesh)) = q.get_mut(e) else { continue };
        if !t.is_visible() {
            *vis = Visibility::Hidden;
            if mesh.0 != mats.citizen_mesh {
                mesh.0 = mats.citizen_mesh.clone();
            }
            continue;
        }
        *vis = Visibility::Visible;
        let look = nd_sim::looks::Appearance::of_seed(t.seed, t.age, t.class);
        let key = tourist_key(&look, t.age, t.class);
        let pos = if t.prev_pos.distance(t.pos) < 30.0 { t.prev_pos.lerp(t.pos, f) } else { t.pos };
        tf.translation = Vec3::new(pos.x, 0.16, pos.y);
        tf.rotation = Quat::from_rotation_y(-lerp_angle(t.prev_heading, t.heading, f));
        tf.scale = Vec3::splat(look.stature(t.age));
        let wanted: Handle<Mesh> = state.figures.get_or_build(key, |k: &PersonKey| meshes.add(shared_mesh(&person_mesh(k), false))).unwrap_or_else(|| mats.citizen_mesh.clone());
        if mesh.0 != wanted {
            mesh.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::districts::Districts;

    #[test]
    fn a_painted_square_is_one_quad_a_row_and_its_border() {
        let mut d = Districts::default();
        let id = d.create(None, 1, 0).unwrap();
        d.paint_polygon(&[SVec2::new(0.0, 0.0), SVec2::new(160.0, 0.0), SVec2::new(160.0, 160.0), SVec2::new(0.0, 160.0)], id);
        let m = district_mesh(&d.grid, |_| Some([1.0, 0.5, 0.2]));
        // Ten rows of fill and forty border edges, each a two-triangle quad.
        assert_eq!(m.indices.len() / 3, 2 * (10 + 40));
        assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
    }
}
