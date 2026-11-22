//! [r11-adopt-frames] Merged map cells drawn off the main thread.
//!
//! Skid row and the trash (`render/society.rs`), murals and gardens
//! (`render/social.rs`) are drawn as one merged
//! mesh per map cell. On a big city most cells change with every batch the
//! sim thread hands over (a bin crossing an overflow step, a camp growing):
//! putting the cell's pieces together and turning them into meshes took
//! 20-70 ms of the frame that took in the batch (over 200 ms when the
//! ground changed and every piece was built again). A [`CellJob`] does
//! that work on Bevy's async compute pool from a lease on the screen's
//! world (`sim_thread::Lease`, as `chunk_jobs` does for buildings); the
//! main thread only adds the finished meshes and swaps them for the old
//! ones, all cells of a job in the same frame. An edit while a job holds
//! the world recalls it, and the cells are drawn again from the new world.

use super::*;
use bevy::tasks::{block_on, AsyncComputeTaskPool, Task};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// One surface's mesh, made off the main thread, ready to add.
pub(super) type ReadyMesh = (Surface, Mesh, nd_procgen::Lod);

/// The meshes [`spawn_geometry`] would make from `geo`, in its order: one
/// per non-empty surface, then the detail layers.
pub(super) fn geometry_meshes(geo: &Geometry) -> Vec<ReadyMesh> {
    geometry_layers(geo).map(|(l, s, data)| (s, to_mesh(data, !s.is_glow()), l)).collect()
}

/// Spawn meshes from [`geometry_meshes`]: the same entities
/// [`spawn_geometry`] spawns.
pub(super) fn spawn_meshes<T: Component + Clone>(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, ready: Vec<ReadyMesh>, tag: T) -> Vec<Entity> {
    let mut out = Vec::with_capacity(ready.len());
    for (s, mesh, lod) in ready {
        let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, tag.clone()));
        if s.is_glow() {
            e.insert((NotShadowCaster, NotShadowReceiver));
        }
        // [r13-detail] Detail layers within their band, as everywhere.
        if lod != nd_procgen::Lod::Base {
            e.insert(detail_range(lod));
        }
        out.push(e.id());
    }
    out
}

/// Renderer work may run on the async pool: the sim runs on its thread and
/// nothing writes the screen's city every frame (a lease held then would
/// make each write wait). `ND_CHUNK_JOBS=0` keeps it on the main thread.
pub(super) fn off_thread(sim: &Sim) -> bool {
    super::buildings::chunk_jobs_on() && sim.threaded && sim.world.threaded() && sim.edit_streak < crate::sim::EDIT_STREAK
}

/// Work on the async pool that reads the world: `T` when done, None when
/// it was recalled (an edit) or dropped.
pub(super) struct CellJob<T: Send + 'static> {
    stop: Arc<AtomicBool>,
    task: Task<Option<T>>,
}

impl<T: Send + 'static> CellJob<T> {
    /// Run `work` on the pool with the world lent; `work` asks its `halt`
    /// between pieces and gives up (None) when it says stop.
    pub(super) fn start(lease: crate::sim_thread::Lease, work: impl FnOnce(&World, &dyn Fn() -> bool) -> Option<T> + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let halt = || flag.load(Ordering::Relaxed) || lease.recalled();
            let out = work(lease.world(), &halt);
            drop(lease);
            out
        });
        Self { stop, task }
    }

    /// Run `work` on the pool without the world (it has all it needs).
    pub(super) fn detached(work: impl FnOnce(&dyn Fn() -> bool) -> Option<T> + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { work(&|| flag.load(Ordering::Relaxed)) });
        Self { stop, task }
    }

    pub(super) fn finished(&self) -> bool {
        self.task.is_finished()
    }

    /// The result (None: recalled or stopped). Only after [`Self::finished`].
    pub(super) fn take(&mut self) -> Option<T> {
        block_on(&mut self.task)
    }
}

impl<T: Send + 'static> Drop for CellJob<T> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
