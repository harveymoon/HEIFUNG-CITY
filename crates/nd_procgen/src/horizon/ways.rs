//! [horizon] The ways past the end of the map's: the highway, the railway
//! and every road off the map run on past `landscape::WAY_REACH` (where the
//! simulation's own ways end, out of sight) across the far land to the
//! neighbouring town they lead to, lit by night; and the shipping lanes run
//! out to sea, marked by buoy lights.

use super::land::{FarLand, HORIZON};
use super::towns::{light_dot, FarTown};
use crate::mesh::MeshData;
use glam::{Vec2, Vec3};
use nd_sim::landscape::{LAND_HALF, WATER_LEVEL};

/// Samples along a far way, metres.
const STEP: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarWayKind {
    Road,
    Rail,
}

/// Where one of the map's ways ends (out past sight) and which way it runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WayEnd {
    pub at: Vec2,
    pub dir: Vec2,
    /// Half width of what is drawn.
    pub half_width: f32,
    pub kind: FarWayKind,
}

/// A way end's line on to the town it leads to (the nearest town ahead of
/// it), a gentle curve leaving in the way's own direction; or straight on
/// to the horizon when no town lies ahead.
pub fn way_line(end: &WayEnd, towns: &[FarTown]) -> Vec<Vec2> {
    let ahead = towns
        .iter()
        .filter(|t| (t.pos - end.at).normalize_or_zero().dot(end.dir) > 0.55)
        .min_by(|a, b| a.pos.distance(end.at).total_cmp(&b.pos.distance(end.at)));
    let to = ahead.map_or(end.at + end.dir * (HORIZON - end.at.length()).max(STEP), |t| t.pos);
    let len = to.distance(end.at);
    let ctrl = end.at + end.dir * (len * 0.5);
    let n = (len / STEP).ceil().max(1.0) as usize;
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            end.at * (u * u) + ctrl * (2.0 * u * t) + to * (t * t)
        })
        .collect()
}

/// The ribbons of the far ways (draped on the far ground `ground`, a little
/// above it) and their lamps by night (along roads only).
pub fn ways(ends: &[WayEnd], towns: &[FarTown], ground: &dyn Fn(Vec2) -> f32) -> (MeshData, MeshData) {
    let mut ribbons = MeshData::default();
    let mut lamps = MeshData::default();
    for end in ends {
        let line = way_line(end, towns);
        let colour = match end.kind {
            FarWayKind::Road => [0.055, 0.055, 0.06, 1.0],
            FarWayKind::Rail => [0.085, 0.075, 0.065, 1.0],
        };
        let hw = end.half_width.max(3.0);
        let lift = |d: f32| 1.5 + d * 0.0006;
        let mut left = Vec::with_capacity(line.len());
        let mut right = Vec::with_capacity(line.len());
        for (i, p) in line.iter().enumerate() {
            let a = line[i.saturating_sub(1)];
            let b = line[(i + 1).min(line.len() - 1)];
            let dir = (b - a).normalize_or(end.dir);
            let side = Vec2::new(-dir.y, dir.x) * hw;
            let d = p.distance(end.at);
            let y = ground(*p).max(WATER_LEVEL + 1.0) + lift(d);
            left.push(Vec3::new(p.x + side.x, y, p.y + side.y));
            right.push(Vec3::new(p.x - side.x, y, p.y - side.y));
            if end.kind == FarWayKind::Road && i > 0 && i % 2 == 0 {
                // Lamps both sides, dimmer with distance (less of them carry).
                let k = (1.0 - d / 9000.0).clamp(0.25, 1.0) * 0.8;
                for s in [side * 1.6, -side * 1.6] {
                    light_dot(&mut lamps, Vec3::new(p.x + s.x, y + 9.0, p.y + s.y), 4.0, [1.0 * k, 0.5 * k, 0.15 * k]);
                }
            }
        }
        for i in 0..line.len().saturating_sub(1) {
            let base = ribbons.positions.len() as u32;
            for p in [left[i], right[i], right[i + 1], left[i + 1]] {
                ribbons.positions.push(p.to_array());
                ribbons.normals.push([0.0, 1.0, 0.0]);
                ribbons.colors.push(colour);
                ribbons.uvs.push([p.x, -p.z]);
            }
            // Facing up whichever way the line runs.
            let up = (right[i] - left[i]).cross(left[i + 1] - left[i]).y > 0.0;
            if up {
                ribbons.indices.extend_from_slice(&[base, base + 1, base + 3, base + 1, base + 2, base + 3]);
            } else {
                ribbons.indices.extend_from_slice(&[base, base + 3, base + 1, base + 1, base + 3, base + 2]);
            }
        }
    }
    (ribbons, lamps)
}

/// The shipping lanes: from each of `from` (a point and the way out to
/// sea), straight on out past the map's edge to the horizon, over water.
pub fn lanes(far: &FarLand, from: &[(Vec2, Vec2)]) -> Vec<Vec<Vec2>> {
    from.iter()
        .map(|&(p, dir)| {
            let dir = dir.normalize_or(Vec2::NEG_Y);
            // Start where the lane leaves the map.
            let mut s = 0.0;
            while (p + dir * s).abs().max_element() < LAND_HALF && s < HORIZON {
                s += 50.0;
            }
            let mut pts = Vec::new();
            while s < HORIZON {
                let q = p + dir * s;
                if q.abs().max_element() > HORIZON {
                    break;
                }
                // The lane goes round nothing: it stops at land.
                if !far.is_water(q) {
                    break;
                }
                pts.push(q);
                s += 700.0;
            }
            pts
        })
        .collect()
}

/// The lanes' buoys by night: red to port, green to starboard (seen
/// from the city), a pair every 700 m, dimmer farther out.
pub fn lane_lights(lanes: &[Vec<Vec2>]) -> MeshData {
    let mut m = MeshData::default();
    for lane in lanes {
        for w in lane.windows(2) {
            let dir = (w[1] - w[0]).normalize_or(Vec2::NEG_Y);
            let side = Vec2::new(-dir.y, dir.x) * 110.0;
            let k = (1.0 - w[0].length() / HORIZON).clamp(0.3, 1.0) * 1.4;
            for (s, rgb) in [(side, [1.0, 0.08, 0.05]), (-side, [0.08, 1.0, 0.25])] {
                let p = w[0] + s;
                light_dot(&mut m, Vec3::new(p.x, WATER_LEVEL + 4.0, p.y), 9.0, [rgb[0] * k, rgb[1] * k, rgb[2] * k]);
            }
        }
    }
    m
}

/// Where the map's ways end out past sight (`landscape::is_elsewhere`):
/// roads and railways, not footpaths.
pub fn way_ends(w: &nd_sim::World) -> Vec<WayEnd> {
    use nd_sim::roads::RoadKind;
    let mut out = Vec::new();
    for (id, s) in w.roads.alive_segments() {
        let kind = match s.kind {
            RoadKind::Rail => FarWayKind::Rail,
            RoadKind::Path => continue,
            _ => FarWayKind::Road,
        };
        let (a, b) = w.roads.segment_points(id);
        for (end, from) in [(a, b), (b, a)] {
            if nd_sim::landscape::is_elsewhere(end) && !nd_sim::landscape::is_elsewhere(from) {
                out.push(WayEnd { at: end, dir: (end - from).normalize_or(end.normalize_or(Vec2::X)), half_width: s.half_width(), kind });
            }
        }
    }
    // A fixed order whatever the network's.
    out.sort_by(|p, q| (p.at.x, p.at.y).partial_cmp(&(q.at.x, q.at.y)).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Where the shipping lanes start and the way they run: from each
/// shipping port and ferry terminal out the way its ships leave; with
/// none, from the sea's own way out (south off a coast, two bearings
/// round an island), so the lanes are there before the first port is.
pub fn lane_starts(w: &nd_sim::World) -> Vec<(Vec2, Vec2)> {
    use nd_sim::landscape::SeaExit;
    use nd_sim::ports::PortKind;
    let Some(sea) = w.landscape.sea.as_ref() else { return Vec::new() };
    let mut out: Vec<(Vec2, Vec2)> = w
        .ports
        .ports
        .values()
        .filter(|p| matches!(p.kind, PortKind::ShippingPort | PortKind::FerryTerminal))
        .map(|p| (p.berth, (sea.exit_from(p.berth) - p.berth).normalize_or(Vec2::NEG_Y)))
        .collect();
    out.sort_by(|p, q| (p.0.x, p.0.y).partial_cmp(&(q.0.x, q.0.y)).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup_by(|a, b| a.0.distance(b.0) < 300.0);
    if out.is_empty() {
        let seed = w.landscape.params.seed;
        let r = |k: u64| (crate::hash::hash2_salted(seed, k, 0x1A_7E5) >> 40) as f32 / (1u64 << 24) as f32;
        match sea.outward {
            SeaExit::South => out.push((Vec2::new(-700.0 + 1400.0 * r(1), 0.0), Vec2::NEG_Y)),
            SeaExit::Radial => {
                for k in 0..2 {
                    let a = r(10 + k) * std::f32::consts::TAU;
                    out.push((Vec2::ZERO, Vec2::new(a.cos(), a.sin())));
                }
            }
        }
    }
    out
}
