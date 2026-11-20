//! [r11-lot-memory] Which building layers the lot history redraws.
//!
//! The ghosts of buildings gone (on their neighbours' walls) and the old
//! signs painted over are drawn with the buildings, in their chunk layers
//! (`buildings::append_building`, `nd_procgen::lot_memory`). A layer is
//! redrawn only when a trace that touches it is new, gone, or turns over
//! to its next fade stage (once a week at most): the renderer keeps what
//! each trace was drawn at and compares when the history's version or the
//! day moves. Nothing a frame otherwise.

use super::*;
use nd_sim::lot_memory::{stage, TraceKind, SIGN_STAGES, WALL_STAGES};

/// What a trace was drawn at: its fade stage, the box it reaches and, for
/// a change, the building it is in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Mark {
    stage: u8,
    lo: SVec2,
    hi: SVec2,
    building: Option<BuildingId>,
}

/// Every trace the buildings draw, as of now.
pub(super) fn marks(world: &World) -> HashMap<u32, Mark> {
    let tick = world.clock.tick;
    let mut out = HashMap::new();
    for t in world.lot_memory.traces() {
        let m = match t.kind {
            TraceKind::Demolished if t.floors > 0 && t.outline.len() >= 3 => {
                let (lo, hi) = t.bounds();
                Mark { stage: stage(t, tick, WALL_STAGES), lo, hi, building: None }
            }
            TraceKind::Changed => Mark { stage: stage(t, tick, SIGN_STAGES), lo: t.at, hi: t.at, building: Some(t.building) },
            _ => continue,
        };
        out.insert(t.id, m);
    }
    out
}

/// The layers to redraw for the traces that are new, gone or at another
/// stage than in `drawn` (which becomes `now`): the building a change is
/// in, and the buildings round a building gone.
pub(super) fn changed_layers(world: &World, drawn: &mut HashMap<u32, Mark>, now: HashMap<u32, Mark>) -> Vec<chunk_jobs::LayerKey> {
    let mut moved: Vec<Mark> = Vec::new();
    for (id, m) in &now {
        if drawn.get(id) != Some(m) {
            moved.push(*m);
        }
    }
    for (id, m) in drawn.iter() {
        if !now.contains_key(id) {
            moved.push(*m);
        }
    }
    *drawn = now;
    if moved.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let reach = SVec2::splat(nd_procgen::lot_memory::ABUT + 1.0);
    let walls: Vec<(SVec2, SVec2)> = moved.iter().filter(|m| m.building.is_none()).map(|m| (m.lo - reach, m.hi + reach)).collect();
    for m in &moved {
        if let Some(k) = m.building.and_then(|b| chunk_jobs::building_layer(world, b)) {
            out.push(k);
        }
    }
    if !walls.is_empty() {
        for (id, b) in world.buildings.iter() {
            if b.is_tenant() {
                continue;
            }
            let plots = std::iter::once(b.lot).chain(b.plot.extra_lots.iter().copied());
            let touches = plots.filter_map(|l| world.lots.get(l)).any(|l| {
                let (lo, hi) = l.polygon.iter().fold((l.center, l.center), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                walls.iter().any(|(a, z)| lo.x <= z.x && lo.y <= z.y && hi.x >= a.x && hi.y >= a.y)
            });
            if touches {
                if let Some(k) = chunk_jobs::building_layer(world, id) {
                    out.push(k);
                }
            }
        }
    }
    out.sort_unstable_by_key(|k| (k.0 .0, k.0 .1, k.1));
    out.dedup();
    out
}
