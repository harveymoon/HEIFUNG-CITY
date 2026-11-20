//! [r10-chunk-hitch] Building chunks in two layers, generated off the main
//! thread.
//!
//! A chunk's buildings are drawn as two merged layers: the settled ones,
//! and the live ones whose look follows the sim from hour to hour (building
//! sites and demolitions by their progress, car parks by the cars standing
//! in them: [`is_live`]). A car park filling up regenerates the few live
//! buildings of its chunk, not the street block of towers round it.
//!
//! Regeneration runs on Bevy's async compute pool: a job takes a lease on
//! the screen's world (`sim_thread::Lease`), the drawing kit and the
//! styles of the buildings it draws, generates the layers and turns them
//! into meshes; the main thread only adds the finished meshes and swaps
//! them for the old ones, all layers of a chunk in the same frame. An edit
//! to the world while a job holds it recalls the job (it stops at its next
//! building and is started again), so the edit never has to copy the city.

use super::*;
use super::buildings::{append_building, Kit, DETAIL_SPLIT};
use bevy::tasks::{block_on, AsyncComputeTaskPool, Task};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Which of a chunk's two merged meshes a building is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Layer {
    /// Buildings that look the same from one day to the next.
    Settled,
    /// Buildings whose look follows the sim: see [`is_live`].
    Live,
}

pub(super) type LayerKey = (ChunkKey, Layer);

/// Jobs out at once (one chunk each); Bevy gives the async pool a quarter
/// of the cores, up to 4.
pub(super) const MAX_JOBS: usize = 4;

/// A building whose drawing changes as the city runs, often: a building
/// site (the frame and scaffolds rise with its progress, every tenth), a
/// demolition (likewise), a car park (the cars drawn are the cars parked:
/// `parking_band`, up to ~37 steps), a truck lot or cargo hub (its parked
/// lorries), a grid battery (its status lights) and a landfill (its
/// mound, in twentieths).
pub(crate) fn is_live(b: &nd_sim::Building) -> bool {
    b.construction.is_some()
        || b.demolition.is_some()
        || b.parking_visual().is_some()
        || b.business.as_ref().is_some_and(|z| z.kind.is_truck_lot())
        || b.battery.is_some()
        || b.service.as_ref().is_some_and(|s| s.kind == nd_sim::ServiceKind::Landfill)
}

/// The chunk and layer a building is drawn in now.
pub(super) fn building_layer(world: &World, id: BuildingId) -> Option<LayerKey> {
    let k = super::buildings::building_chunk(world, id)?;
    Some((k, if is_live(&world.buildings[id]) { Layer::Live } else { Layer::Settled }))
}

/// What building generation reads besides the world and the styles, owned
/// so a job can carry it: the rules and the palette as they were when the
/// chunks were last all drawn.
pub(crate) struct DrawKit {
    pub rules: nd_procgen::BuildingRules,
    pub palette: nd_procgen::Palette,
}

impl DrawKit {
    pub(crate) fn kit(&self) -> Kit<'_> {
        Kit { rules: &self.rules, palette: &self.palette }
    }
}

/// One building's part in a layer besides its triangles.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Drawn {
    pub id: BuildingId,
    /// [fire-scene] Its drawn height (None: nothing could be drawn).
    pub top: Option<f32>,
    /// [r8] A depot's painted lorry bays; [K34] its box stacks.
    pub bays: Vec<(Vec3, f32)>,
    pub slots: Vec<nd_procgen::BoxSlot>,
    /// A solar or wind plant merging with its neighbours: its lot centre.
    pub plant: Option<SVec2>,
    /// [r11-lot-memory] Triangles of ghosts drawn on its walls.
    pub ghosts: u32,
}

/// One layer generated: the merged geometry, its detail cells (by lot
/// centre, [`DETAIL_SPLIT`]² of them) and what each building drew.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LayerGeo {
    pub key: LayerKey,
    pub geo: Geometry,
    pub cells: Vec<Geometry>,
    pub drawn: Vec<Drawn>,
}

/// Generate one layer's buildings, in the order given. `halt` is asked
/// before each building: None when it said stop.
pub(super) fn generate_layer(world: &World, kit: Kit, styles: &StyleBook, key: LayerKey, ids: &[BuildingId], halt: &dyn Fn() -> bool) -> Option<LayerGeo> {
    let mut geo = Geometry::default();
    let mut cells: Vec<Geometry> = (0..DETAIL_SPLIT * DETAIL_SPLIT).map(|_| Geometry::default()).collect();
    let mut drawn = Vec::with_capacity(ids.len());
    for &id in ids {
        if halt() {
            return None;
        }
        let rigs_before = geo.games.len();
        let slots_before = geo.box_slots.len();
        let drew = append_building(&mut geo, &mut cells, world, kit, styles, id);
        let (top, ghosts) = (drew.map(|d| d.0), drew.map_or(0, |d| d.1));
        // [K34] A depot's stacks are where its real boxes stand; [r8] its
        // painted lorry bays, where the real lorries stand.
        let (bays, slots): (Vec<nd_procgen::BoxSlot>, Vec<nd_procgen::BoxSlot>) = geo.box_slots.drain(slots_before..).partition(|s| s.tier == nd_procgen::BAY_TIER);
        if top.is_some() {
            // [games] A stadium's game-day dressing knows its building.
            for rig in geo.games[rigs_before..].iter_mut() {
                rig.building = Some(id);
            }
        }
        let plant = world.buildings.get(id).filter(|b| b.plant.is_some_and(|k| k.merges_with_neighbours())).and_then(|b| world.lots.get(b.lot)).map(|l| l.center);
        drawn.push(Drawn { id, top, bays: bays.iter().map(|s| (s.at, s.yaw)).collect(), slots, plant, ghosts });
    }
    Some(LayerGeo { key, geo, cells, drawn })
}

/// One detail layer of a detail cell, as meshes.
pub(super) struct ReadyDetail {
    pub lod: nd_procgen::Lod,
    pub at: Vec3,
    pub triangles: usize,
    pub meshes: Vec<(Surface, Mesh)>,
}

/// A layer turned into meshes, ready to add and spawn.
pub(super) struct ReadyLayer {
    pub key: LayerKey,
    pub base: Vec<(Surface, Mesh)>,
    pub detail: Vec<ReadyDetail>,
    pub cranes: Vec<nd_procgen::CraneRig>,
    pub games: Vec<nd_procgen::GameRig>,
    pub triangles: usize,
    pub drawn: Vec<Drawn>,
    /// Generation and mesh building, milliseconds.
    pub ms: f32,
}

/// Turn a generated layer into meshes (lit ones with tangents; each detail
/// cell's layer with the middle its distance range is measured from), so
/// the main thread only adds them.
pub(super) fn ready(l: LayerGeo, gen_ms: f32) -> ReadyLayer {
    let t = Instant::now();
    let base: Vec<(Surface, Mesh)> = l.geo.iter().map(|(s, data)| (s, to_mesh(data, !s.is_glow()))).collect();
    let mut detail = Vec::new();
    for cell in &l.cells {
        for lod in nd_procgen::Lod::DETAIL {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            let mut tris = 0;
            for (_, m) in cell.detail_iter(lod) {
                for p in &m.positions {
                    lo = lo.min(Vec3::from(*p));
                    hi = hi.max(Vec3::from(*p));
                }
                tris += m.indices.len() / 3;
            }
            if tris == 0 {
                continue;
            }
            let meshes = cell.detail_iter(lod).map(|(s, data)| (s, to_mesh(data, true))).collect();
            detail.push(ReadyDetail { lod, at: (lo + hi) * 0.5, triangles: tris, meshes });
        }
    }
    let triangles = l.geo.triangle_count() + detail.iter().map(|d| d.triangles).sum::<usize>();
    ReadyLayer { key: l.key, base, detail, cranes: l.geo.cranes, games: l.geo.games, triangles, drawn: l.drawn, ms: gen_ms + t.elapsed().as_secs_f32() * 1000.0 }
}

/// Generate a chunk's layers and turn them into meshes, building by
/// building while `halt` lets it.
pub(super) fn build_chunk(world: &World, kit: Kit, styles: &StyleBook, groups: &[(LayerKey, Vec<BuildingId>)], halt: &dyn Fn() -> bool) -> Option<Vec<ReadyLayer>> {
    let mut out = Vec::with_capacity(groups.len());
    for (key, ids) in groups {
        let t = Instant::now();
        let g = generate_layer(world, kit, styles, *key, ids, halt)?;
        out.push(ready(g, t.elapsed().as_secs_f32() * 1000.0));
    }
    Some(out)
}

/// A chunk being built on the async pool.
pub(super) struct Job {
    pub layers: Vec<Layer>,
    /// Set when the job is dropped (a load, a new look): it stops at its
    /// next building.
    stop: Arc<AtomicBool>,
    task: Task<Option<Vec<ReadyLayer>>>,
}

impl Job {
    /// Start building `groups` (one chunk's layers) on the async pool.
    pub(super) fn start(lease: crate::sim_thread::Lease, kit: Arc<DrawKit>, styles: StyleBook, groups: Vec<(LayerKey, Vec<BuildingId>)>) -> Job {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let layers = groups.iter().map(|(k, _)| k.1).collect();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let halt = || flag.load(Ordering::Relaxed) || lease.recalled();
            let out = build_chunk(lease.world(), kit.kit(), &styles, &groups, &halt);
            // The world goes back here, on the pool's thread: if it was the
            // last hold on an old copy, freeing it costs this thread.
            drop(lease);
            out
        });
        Job { layers, stop, task }
    }

    pub(super) fn finished(&self) -> bool {
        self.task.is_finished()
    }

    /// The finished layers (None: the job was recalled or stopped). Only
    /// after [`Job::finished`].
    pub(super) fn take(&mut self) -> Option<Vec<ReadyLayer>> {
        block_on(&mut self.task)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Spawn a ready layer: one entity per surface (glow with the window-lights
/// material, so lights follow occupancy, and no shadows), the detail cells
/// with their distance ranges (`detail_range`), and the crane heads and
/// game-day rigs.
pub(super) fn spawn_layer(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, r: &mut ReadyLayer, work_minutes: u64) -> ChunkMeshes {
    let mut entities = Vec::new();
    for (s, mesh) in r.base.drain(..) {
        let mesh = Mesh3d(meshes.add(mesh));
        let e = if s.is_glow() {
            commands.spawn((mesh, MeshMaterial3d(mats.windows.clone()), Transform::IDENTITY, BuildingTag, NotShadowCaster, NotShadowReceiver)).id()
        } else {
            commands.spawn((mesh, MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, BuildingTag)).id()
        };
        entities.push(e);
    }
    let mut detail = Vec::new();
    for d in r.detail.iter_mut() {
        let vis = detail_range(d.lod);
        for (s, mesh) in d.meshes.drain(..) {
            entities.push(commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, BuildingTag, vis.clone())).id());
        }
        detail.push(DetailCell { at: d.at, lod: d.lod, triangles: d.triangles });
    }
    for rig in &r.cranes {
        entities.extend(super::buildings::spawn_crane_head(commands, meshes, mats, rig, work_minutes));
    }
    for rig in &r.games {
        entities.extend(super::games::spawn_game_rig(commands, meshes, mats, rig));
    }
    ChunkMeshes { entities, triangles: r.triangles, detail }
}

#[cfg(test)]
#[path = "chunk_jobs_tests.rs"]
mod tests;
