//! The landscape's hooks into the world: road heights and bridges, lots on
//! water or steep ground, the built ground, the highway and its through
//! traffic, and the docks with their ships.

use super::*;
use crate::landscape::{BRIDGE_CLEARANCE, HIGHWAY_HALF_GAP, HIGHWAY_LANES, HIGHWAY_Y, UNDERPASS_CLEARANCE, WATER_LEVEL};
use crate::ports::{self, Port, PortId, PortKind, Ship, ShipKind};
use crate::zoning::WaterMask;

mod heights;
mod bridges;
mod highway;
mod dock_access;
mod docks;
mod ships;
#[cfg(test)]
mod tests;

/// Price of a metre of new bridge deck.
pub const BRIDGE_COST_PER_M: f32 = 60.0;
/// Steepest grade a street keeps; rail and paths have their own.
pub const STREET_GRADE: f32 = 0.07;
pub const RAIL_GRADE: f32 = 0.03;
pub const PATH_GRADE: f32 = 0.10;
/// At most this many through cars on the highway at once.
pub const MAX_THROUGH_CARS: usize = 100;
/// Chance per tick that a through car joins each carriageway.
pub const THROUGH_CHANCE: f64 = 0.45;
/// How far a bridge deck reaches onto each bank.
const ABUTMENT: f32 = 5.0;

/// A way of `mine` kind at level `mine_level` meeting one of `theirs` kind
/// at `their_level`, where one of them is the railway: refused (with the
/// reason) unless they pass one over the other or meet at grade, within a
/// metre. A level crossing has to be level: the track cannot follow a road
/// down a portal ramp or up to a deck.
fn rail_crossing_refusal(mine: RoadKind, theirs: RoadKind, highway: bool, mine_level: f32, their_highway: bool, their_level: f32) -> Option<String> {
    if (mine == RoadKind::Rail) == (theirs == RoadKind::Rail) {
        return None;
    }
    let gap = mine_level - their_level;
    if crate::roads::grade_separated(highway, mine_level, their_highway, their_level) || gap.abs() < 1.0 {
        return None;
    }
    let need = crate::roads::GRADE_SEPARATION - gap.abs();
    let (what, other) = if mine == RoadKind::Rail { ("the track", "the road") } else { ("the road", "the railway") };
    let (way, verb) = if gap < 0.0 { ("below", "lower") } else { ("above", "raise") };
    Some(format!(
        "{what} meets {other} {:.1} m {way} it: a level crossing must be at grade. {} {what} {need:.1} m more there to pass {} it, or start the ramp past the crossing",
        gap.abs(),
        if verb == "lower" { "Lower" } else { "Raise" },
        if gap < 0.0 { "under" } else { "over" }
    ))
}

/// Spacing of the inner height samples along a long street or path.
pub const INNER_STEP: f32 = 30.0;

/// Heights inside long streets and paths, between their nodes, so a street
/// drawn as one long leg over a hill follows the land instead of running
/// dead straight from end to end (and the ground under it is not cut and
/// filled by metres to that straight line).
///
/// A street's vertices decide where its parcels are cut (parcel keys are
/// their centres and frontages), so the samples cannot be new road nodes as
/// the highway's and the track's are (`sample_heights`): they are kept per
/// segment as `(t, z)` in rising `t`, every `INNER_STEP` or so, and read
/// through [`crate::landscape::way_height`] by everything that sets things
/// on a way (cars, lots, the built ground, the street mesh). Nothing about
/// the network or the parcels changes.
///
/// Each sample takes the land's height there, held within the way's grade
/// of both ends and of its neighbours (forward and back passes). Ways off
/// the ground (decks, tunnels, cuttings, any level the player set), the
/// highway and track (which have real vertices), and ways in a grade
/// crossing (whose underpass is worked out on the straight line) keep the
/// straight line, as does the flat map. An empty list is the straight line.
fn inner_heights(land: &crate::landscape::Landscape, roads: &crate::roads::RoadNetwork, z: &[f32], floor: f32, crossings: &[crate::roads::Crossing]) -> Vec<Vec<(f32, f32)>> {
    let mut out = vec![Vec::new(); roads.segments.len()];
    if land.is_flat() {
        return out;
    }
    let crossed: std::collections::BTreeSet<u32> = crossings.iter().flat_map(|c| [c.upper.0 .0, c.lower.0 .0]).collect();
    let edge = crate::landscape::LAND_HALF;
    for (sid, s) in roads.alive_segments() {
        if s.bridge || s.viaduct || s.tunnel || s.is_rail() || s.is_highway() || s.ring || crossed.contains(&sid.0) {
            continue;
        }
        if roads.node(s.a).level != 0.0 || roads.node(s.b).level != 0.0 {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let len = a.distance(b);
        if len < INNER_STEP * 1.5 {
            continue;
        }
        let (za, zb) = (z.get(s.a.0 as usize).copied().unwrap_or(0.0), z.get(s.b.0 as usize).copied().unwrap_or(0.0));
        let g = way_grade(s.kind);
        let m = (len / INNER_STEP).round().max(2.0) as usize;
        let ds = len / m as f32;
        let mut h: Vec<f32> = (1..m)
            .map(|k| {
                let (t, sk) = (k as f32 / m as f32, k as f32 * ds);
                let p = a.lerp(b, t).clamp(Vec2::splat(-edge), Vec2::splat(edge));
                let lo = (za - g * sk).max(zb - g * (len - sk));
                let hi = (za + g * sk).min(zb + g * (len - sk));
                let line = za + (zb - za) * t;
                if lo > hi { line } else { land.height(p).max(floor).clamp(lo, hi) }
            })
            .collect();
        // Neighbour to neighbour within the grade, both ways.
        let mut prev = za;
        for v in h.iter_mut() {
            *v = v.clamp(prev - g * ds, prev + g * ds);
            prev = *v;
        }
        let mut next = zb;
        for v in h.iter_mut().rev() {
            *v = v.clamp(next - g * ds, next + g * ds);
            next = *v;
        }
        let samples: Vec<(f32, f32)> = h.into_iter().enumerate().map(|(k, v)| ((k + 1) as f32 / m as f32, v)).collect();
        if samples.iter().any(|&(t, v)| (v - (za + (zb - za) * t)).abs() > 0.05) {
            out[sid.0 as usize] = samples;
        }
    }
    out
}

/// Steepest grade a way of this kind keeps.
pub fn way_grade(kind: RoadKind) -> f32 {
    match kind {
        RoadKind::Rail => RAIL_GRADE,
        RoadKind::Path => PATH_GRADE,
        RoadKind::Street => STREET_GRADE,
    }
}

/// Ease node heights `z` to the grades of `segs` (a, b, length, grade):
/// first every free node is brought down to within its grade of a fixed
/// node below it (the approach to a cutting), then the high end of anything
/// too steep is lowered (a cutting), never below its `least` (a bridge end
/// keeps its deck), so a way climbing to the hills at the map edge cuts
/// into them rather than carrying the basin up on an embankment as high as
/// the hills; last the low end of anything still too steep is raised (fill
/// and embankment, the approaches to a deck). Fixed nodes never move.
fn ease_heights(z: &mut [f32], fixed: &[bool], least: &[f32], segs: &[(usize, usize, f32, f32)]) {
    let n = z.len();
    if fixed.iter().any(|f| *f) {
        let mut cap: Vec<f32> = (0..n).map(|k| if fixed[k] { z[k] } else { f32::INFINITY }).collect();
        for _ in 0..200 {
            let mut changed = false;
            for &(a, b, len, grade) in segs {
                let max = len * grade;
                if !fixed[b] && cap[a] + max < cap[b] - 1e-3 {
                    cap[b] = cap[a] + max;
                    changed = true;
                }
                if !fixed[a] && cap[b] + max < cap[a] - 1e-3 {
                    cap[a] = cap[b] + max;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for k in 0..n {
            if !fixed[k] {
                z[k] = z[k].min(cap[k]);
            }
        }
    }
    for _ in 0..200 {
        let mut changed = false;
        for &(a, b, len, grade) in segs {
            let max = len * grade;
            if z[a] > z[b] + max + 1e-3 && !fixed[a] && z[a] > least[a] + 1e-3 {
                z[a] = (z[b] + max).max(least[a]);
                changed = true;
            } else if z[b] > z[a] + max + 1e-3 && !fixed[b] && z[b] > least[b] + 1e-3 {
                z[b] = (z[a] + max).max(least[b]);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for _ in 0..200 {
        let mut changed = false;
        for &(a, b, len, grade) in segs {
            let max = len * grade;
            if z[a] > z[b] + max + 1e-3 && !fixed[b] {
                z[b] = z[a] - max;
                changed = true;
            } else if z[b] > z[a] + max + 1e-3 && !fixed[a] {
                z[a] = z[b] - max;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

