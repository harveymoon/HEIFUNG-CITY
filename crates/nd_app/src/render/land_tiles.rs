//! [r4-tiles] The Land for sale layer: every tile round the city outlined
//! on the ground, the city's own land in cyan, the tiles for sale now in
//! amber (the one picked in the Land for sale window brighter, outlined
//! twice), the rest dim. The labels and the window are `ui::land`.

use bevy::prelude::*;
use nd_sim::landscape::{tile_rect, Tile};
use nd_sim::Vec2 as SVec2;

use super::layers::LoadGizmos;
use super::Overlays;
use crate::sim::Sim;

pub const OWNED: [f32; 3] = [0.25, 0.95, 1.0];
pub const FOR_SALE: [f32; 3] = [1.0, 0.72, 0.18];
pub const LATER: [f32; 3] = [0.55, 0.55, 0.62];

/// The tile picked in the Land for sale window (or by its label).
#[derive(Resource, Default)]
pub struct PickedTile(pub Option<Tile>);

/// Line pieces draped on the ground along a tile's edge.
fn draped(sim: &Sim, a: SVec2, b: SVec2, lift: f32, out: &mut Vec<(Vec3, Vec3)>) {
    let w = &sim.world;
    let n = ((a.distance(b) / 30.0).ceil() as usize).max(1);
    let at = |t: f32| {
        let p = a.lerp(b, t);
        Vec3::new(p.x, w.ground_height(p).max(0.0) + lift, p.y)
    };
    for k in 0..n {
        out.push((at(k as f32 / n as f32), at((k + 1) as f32 / n as f32)));
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn land_tile_gizmos(sim: Res<Sim>, overlays: Res<Overlays>, picked: Res<PickedTile>, lighting: Res<super::Lighting>, mut cache: Local<(u64, Option<Tile>, Vec<(Vec3, Vec3, Color)>)>, mut gizmos: Gizmos<LoadGizmos>) {
    if lighting.heat || !overlays.land_tiles || sim.world.landscape.is_flat() {
        cache.2.clear();
        return;
    }
    let w = &sim.world;
    let key = w.landscape.version() ^ (w.ground.version() << 20);
    if cache.2.is_empty() || cache.0 != key || cache.1 != picked.0 || sim.full_rebuild {
        cache.0 = key;
        cache.1 = picked.0;
        cache.2.clear();
        let buyable = w.landscape.buyable_tiles();
        let mut tiles: Vec<Tile> = nd_sim::landscape::outer_tiles();
        // The starting map's own nine, for the city's outline.
        for j in -1..=1 {
            for i in -1..=1 {
                tiles.push((i, j));
            }
        }
        // Later ones first, so the brighter edges are drawn over them.
        let rank = |t: Tile| if Some(t) == picked.0 { 3 } else if w.landscape.owns_tile(t) { 2 } else if buyable.contains(&t) { 1 } else { 0 };
        tiles.sort_by_key(|&t| rank(t));
        for t in tiles {
            let owned = w.landscape.owns_tile(t);
            let (lo, hi) = tile_rect(t);
            let corners = [lo, SVec2::new(hi.x, lo.y), hi, SVec2::new(lo.x, hi.y)];
            let picked_now = Some(t) == picked.0;
            let (rgb, a, lift) = if picked_now {
                ([1.0, 0.9, 0.5], 1.0, 6.0)
            } else if owned {
                (OWNED, 0.9, 4.0)
            } else if buyable.contains(&t) {
                (FOR_SALE, 0.95, 5.0)
            } else {
                (LATER, 0.5, 3.0)
            };
            let col = Color::linear_rgba(rgb[0] * 2.0, rgb[1] * 2.0, rgb[2] * 2.0, a);
            let mut pieces = Vec::new();
            for k in 0..4 {
                let (a, b) = (corners[k], corners[(k + 1) % 4]);
                // The city's own tiles: only where its land ends.
                if owned {
                    let mid = (a + b) * 0.5;
                    let out = mid + (mid - (lo + hi) * 0.5).normalize_or_zero() * 5.0;
                    if w.landscape.owns(out) {
                        continue;
                    }
                }
                // Inset a little, so two tiles' edges sit side by side.
                let c = (lo + hi) * 0.5;
                let inset = if owned { 0.0 } else { 6.0 };
                let ia = a + (c - a).normalize_or_zero() * inset;
                let ib = b + (c - b).normalize_or_zero() * inset;
                draped(&sim, ia, ib, lift, &mut pieces);
            }
            // The picked tile: a second outline inside the first.
            if picked_now {
                let c = (lo + hi) * 0.5;
                for k in 0..4 {
                    let (a, b) = (corners[k], corners[(k + 1) % 4]);
                    draped(&sim, a + (c - a).normalize_or_zero() * 28.0, b + (c - b).normalize_or_zero() * 28.0, lift, &mut pieces);
                }
            }
            cache.2.extend(pieces.into_iter().map(|(p, q)| (p, q, col)));
        }
    }
    for (a, b, c) in &cache.2 {
        gizmos.line(*a, *b, *c);
    }
}
