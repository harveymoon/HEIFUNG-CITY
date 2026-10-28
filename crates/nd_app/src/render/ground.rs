//! The terrain of the old flat map and the open-ground scatter.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

pub(super) fn sync_terrain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    existing: Query<Entity, With<TerrainTag>>,
    mut ways: Local<Vec<(SVec2, SVec2, f32)>>,
    mut pits: Local<u64>,
) {
    // A landscape's ground is drawn by `sync_landscape`.
    if !sim.world.landscape.is_flat() {
        return;
    }
    // The hills are levelled along the ways running out of the city.
    let now = far::far_ways(&sim.world);
    // [p2-look] The strip mines' pits are cut out of it.
    let holes_key = nd_procgen::holes::holes_key(&sim.world);
    if !existing.is_empty() && !assets.palette_changed && !sim.full_rebuild && *ways == now && holes_key == *pits {
        return;
    }
    *pits = holes_key;
    *ways = now;
    despawn_all(&mut commands, &existing);
    let mut data = build_terrain(&terrain_config(&sim.world), &assets.palette);
    nd_procgen::holes::cut_holes(&mut data, &nd_procgen::holes::ground_holes(&sim.world));
    commands.spawn((Mesh3d(meshes.add(to_mesh(&data, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, TerrainTag));
}

/// Open ground within `reach` metres of a way, answered from a coarse grid.
/// The scatter pass asks about ~40k sample points, and the network can hold
/// thousands of segments, so a linear nearest-segment scan per sample was a
/// visible hitch on every implement. Each segment is binned into every cell
/// its reach-expanded bounding box touches, so a query only looks in its own
/// cell.
pub(super) struct NearWays {
    pub(super) cell: f32,
    pub(super) reach: f32,
    pub(super) origin: SVec2,
    pub(super) cols: i32,
    pub(super) rows: i32,
    pub(super) bins: Vec<Vec<(SVec2, SVec2)>>,
}

impl NearWays {
    pub(super) fn new(roads: &nd_sim::RoadNetwork, reach: f32, half_size: f32, keep: impl Fn(&nd_sim::Segment) -> bool) -> Self {
        let cell = reach;
        let origin = SVec2::splat(-half_size - reach);
        let cols = ((half_size + reach) * 2.0 / cell).ceil() as i32 + 1;
        let mut me = Self { cell, reach, origin, cols, rows: cols, bins: vec![Vec::new(); (cols * cols) as usize] };
        for (sid, seg) in roads.alive_segments() {
            if !keep(seg) {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let lo = a.min(b) - SVec2::splat(reach);
            let hi = a.max(b) + SVec2::splat(reach);
            let (c0, r0) = me.cell_of(lo);
            let (c1, r1) = me.cell_of(hi);
            for r in r0.max(0)..=r1.min(me.rows - 1) {
                for c in c0.max(0)..=c1.min(me.cols - 1) {
                    me.bins[(r * me.cols + c) as usize].push((a, b));
                }
            }
        }
        me
    }

    pub(super) fn cell_of(&self, p: SVec2) -> (i32, i32) {
        (((p.x - self.origin.x) / self.cell).floor() as i32, ((p.y - self.origin.y) / self.cell).floor() as i32)
    }

    pub(super) fn near(&self, p: SVec2) -> bool {
        let (c, r) = self.cell_of(p);
        if c < 0 || r < 0 || c >= self.cols || r >= self.rows {
            return false;
        }
        self.bins[(r * self.cols + c) as usize].iter().any(|&(a, b)| nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < self.reach)
    }
}

/// Rocks, scrub and dead trees on the open ground: everything at least 40 m
/// from any road or track and outside every zone. Rebuilt when roads or
/// zones change, since both push the scrub back.
pub(super) fn sync_scatter(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    existing: Query<Entity, With<ScatterTag>>,
) {
    let w = &sim.world;
    let signature = (w.roads.version, w.zones.len());
    if state.scatter_signature == Some(signature) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.scatter_signature = Some(signature);
    despawn_all(&mut commands, &existing);
    let cfg = terrain_config(w);
    const REACH: f32 = 40.0;
    // Streets and paths keep the scrub back, and so does the railway; the
    // network's own nearest queries treat them separately, so bin them all.
    let ways = NearWays::new(&w.roads, REACH, cfg.half_size, |_| true);
    // Zone polygons with bounding boxes so most samples skip the point test.
    let zones: Vec<(SVec2, SVec2, &[SVec2])> = w
        .zones
        .values()
        .filter(|z| !z.polygon.is_empty())
        .map(|z| {
            let lo = z.polygon.iter().copied().fold(SVec2::splat(f32::MAX), SVec2::min);
            let hi = z.polygon.iter().copied().fold(SVec2::splat(f32::MIN), SVec2::max);
            (lo, hi, z.polygon.as_slice())
        })
        .collect();
    let clear = |p: SVec2| {
        // Nothing grows in the water or on the wet sand.
        if !w.landscape.is_flat() && w.landscape.water_distance(p) < 6.0 {
            return false;
        }
        if zones.iter().any(|(lo, hi, poly)| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y && nd_sim::zoning::point_in_polygon(p, poly)) {
            return false;
        }
        !ways.near(p)
    };
    let mut geo = scatter_nature(&cfg, &assets.palette, w.config.seed, &clear);
    nd_procgen::climate::snow_cover(&mut geo, assets.palette.snow);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, ScatterTag);
}
