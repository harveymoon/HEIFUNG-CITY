//! Subdivision: blocks sliced along their frontage into lot-sized pieces,
//! backland merged or chopped, and the seeds that keep the cuts stable.

use super::*;

/// Cut a polygon by the road direction at arc length `s` along a run.
pub(super) fn cut_along_run(poly: &Polygon<f64>, pts: &[Vec2], chain: &[usize], s: f32, rng: &mut impl Rng) -> Vec<Polygon<f64>> {
    let n = pts.len();
    let mut acc = 0.0;
    for &i in chain {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let len = a.distance(b);
        if acc + len >= s {
            let q = a.lerp(b, ((s - acc) / len).clamp(0.05, 0.95));
            let d = (b - a) / len;
            let angle: f32 = if rng.random_bool(0.12) { rng.random_range(-0.18..0.18) } else { 0.0 };
            let (sn, co) = angle.sin_cos();
            let normal = Vec2::new(d.x * co - d.y * sn, d.x * sn + d.y * co);
            return cut(poly, q, normal);
        }
        acc += len;
    }
    Vec::new()
}

/// Union of the edge strips of a run.
pub(super) fn run_band(pts: &[Vec2], chain: &[usize], d: f32) -> MultiPolygon<f64> {
    let n = pts.len();
    let mut acc: Option<MultiPolygon<f64>> = None;
    for &i in chain {
        let s = edge_strip(pts[i], pts[(i + 1) % n], d);
        acc = Some(match acc {
            None => MultiPolygon(vec![s]),
            Some(m) => m.union(&s),
        });
    }
    acc.unwrap_or(MultiPolygon(vec![]))
}

/// Slice a block into parcels. Rows one lot deep are peeled off every road
/// frontage at once (a union of strips along the road edges, so curved roads
/// get curved rows and nothing can fold over); the first frontage claims the
/// corners. The row depth grows while the leftover core would be a thin
/// strip, so two facing frontages meet in the middle. Rows are then cut
/// across at lot width, perpendicular to the local road. Whatever has no
/// frontage stays as unusable ground.
pub(super) fn subdivide(poly: Polygon<f64>, lu: LandUse, roads: &RoadNetwork, rng: &mut impl Rng, depth: u32, out: &mut Vec<Vec<Vec2>>) {
    subdivide_sized(poly, lu, (lu.lot_width(), lu.lot_depth()), roads, rng, depth, out);
}

/// [`subdivide`] with the lot size given: `(width, depth)` of a typical
/// lot ([waterfront] lots run the whole way from the road to the water).
#[allow(clippy::too_many_arguments)]
pub(super) fn subdivide_sized(poly: Polygon<f64>, lu: LandUse, (lw, ld): (f32, f32), roads: &RoadNetwork, rng: &mut impl Rng, depth: u32, out: &mut Vec<Vec<Vec2>>) {
    let area = area_f(&poly);
    if area < MIN_AREA {
        return;
    }
    let pts = from_geo(&poly);
    if pts.len() < 3 {
        return;
    }
    // `pts` is the outer ring only: a piece with a hole (a lake, an
    // excluded area) is never pushed whole, or the parcel would cover it.
    if depth > 60 {
        if poly.interiors().is_empty() {
            out.push(pts);
        }
        return;
    }
    if let Some(hole) = poly.interiors().first() {
        if let Some(c) = hole.0.first() {
            let origin = Vec2::new(c.x as f32, c.y as f32);
            let pieces = cut(&poly, origin, Vec2::X);
            if pieces.is_empty() {
                return;
            }
            for piece in pieces {
                subdivide_sized(piece, lu, (lw, ld), roads, rng, depth + 1, out);
            }
            return;
        }
    }
    if let Some((i, _)) = reflex_vertex(&pts, 0.6) {
        let n = pts.len();
        let a = pts[(i + n - 1) % n];
        let b = pts[i];
        let d = (b - a).normalize_or_zero();
        let on_road = edge_on_road(roads, a, b, lu.fronts_paths()).is_some();
        let normal = if on_road { d } else { Vec2::new(-d.y, d.x) };
        let pieces = cut(&poly, b, normal);
        if pieces.len() >= 2 {
            for piece in pieces {
                subdivide_sized(piece, lu, (lw, ld), roads, rng, depth + 1, out);
            }
            return;
        }
    }

    let allow = lu.fronts_paths();
    let runs = frontage_runs(&pts, roads, allow);
    let Some((chain, chain_len)) = runs.first().cloned() else {
        out.push(pts);
        return;
    };

    // Peel rows off every frontage, deepening them while the core would be thin.
    let mut chosen: Option<(Vec<MultiPolygon<f64>>, MultiPolygon<f64>)> = None;
    for k in [1.15f32, 1.45, 1.8] {
        let d = ld * k;
        let mut bands: Vec<MultiPolygon<f64>> = Vec::new();
        let mut claimed: Option<MultiPolygon<f64>> = None;
        for (ch, _) in &runs {
            let strip = run_band(&pts, ch, d);
            let mut band = poly.intersection(&strip);
            if let Some(c) = &claimed {
                band = band.difference(c);
            }
            claimed = Some(match claimed {
                None => strip,
                Some(c) => c.union(&strip),
            });
            bands.push(band);
        }
        let core = match &claimed {
            Some(c) => poly.difference(c),
            None => MultiPolygon(vec![poly.clone()]),
        };
        let thin = core.0.iter().any(|p| area_f(&p) >= MIN_AREA && thickness(p) < ld * 0.6);
        chosen = Some((bands, core));
        if !thin {
            break;
        }
    }
    let (bands, core) = chosen.unwrap();
    let core_area: f32 = core.0.iter().map(|p| area_f(&p)).sum();
    let band_area: f32 = bands.iter().flat_map(|m| m.0.iter()).map(|p| area_f(&p)).sum();

    if core_area >= MIN_AREA && band_area >= MIN_AREA && band_area < area * 0.995 {
        for band in bands {
            for piece in band.0 {
                subdivide_sized(piece, lu, (lw, ld), roads, rng, depth + 1, out);
            }
        }
        for piece in core.0 {
            // A core ringing a hole is cut through it first (above).
            if !piece.interiors().is_empty() {
                subdivide_sized(piece, lu, (lw, ld), roads, rng, depth + 1, out);
                continue;
            }
            // No frontage left here: unusable ground, kept whole.
            let p = from_geo(&piece);
            if area_f(&piece) >= MIN_AREA && p.len() >= 3 {
                out.push(p);
            }
        }
        return;
    }

    // A long thin piece whose frontages are only at its ends (a strip
    // squeezed between two roads' rows): halve it across its long axis.
    {
        let n = pts.len();
        let (mut li, mut ll) = (0, 0.0);
        for i in 0..n {
            let l = pts[i].distance(pts[(i + 1) % n]);
            if l > ll {
                li = i;
                ll = l;
            }
        }
        let axis = (pts[(li + 1) % n] - pts[li]).normalize_or_zero();
        if axis != Vec2::ZERO {
            let (u0, u1) = extents(&pts, axis);
            let perp = Vec2::new(-axis.y, axis.x);
            let (v0, v1) = extents(&pts, perp);
            let long = u1 - u0;
            let short = v1 - v0;
            // [waterfront] A lot that runs down to the water is meant to be
            // deep: never halve it across.
            let shore_deep = ld > lu.lot_depth() * 1.6;
            if !shore_deep && chain_len < lw * 1.7 && long > lw * 2.2 && long > short * 2.5 {
                let origin = axis * ((u0 + u1) * 0.5) + perp * ((v0 + v1) * 0.5);
                let pieces = cut(&poly, origin, axis);
                if pieces.len() >= 2 {
                    for piece in pieces {
                        subdivide_sized(piece, lu, (lw, ld), roads, rng, depth + 1, out);
                    }
                    return;
                }
            }
        }
    }

    // Already a strip along its frontage: cut across at lot width. A cut
    // that would leave a sliver is moved along the road and retried.
    if chain_len > lw * 1.7 && area > lw * ld * 0.9 {
        // A long run is halved first, so the recursion stays shallow and
        // the remainder of every cut is itself a well-formed strip.
        if chain_len > lw * 8.0 {
            let pieces = cut_along_run(&poly, &pts, &chain, chain_len * rng.random_range(0.45..0.55), rng);
            if pieces.len() >= 2 && pieces.iter().all(|p| area_f(p) >= MIN_AREA) {
                for piece in pieces {
                    subdivide_sized(piece, lu, (lw, ld), roads, rng, depth + 1, out);
                }
                return;
            }
        }
        // Walk along the frontage until the piece cut off is about a lot's
        // area; at a block corner this turns the narrow tip into a corner lot
        // rather than a sliver. Stop at the middle so the rest stays sliceable.
        let target = lw * ld * rng.random_range(0.8..1.2);
        let mut s = lw * rng.random_range(0.7..1.1);
        while s < chain_len * 0.55 {
            let pieces = cut_along_run(&poly, &pts, &chain, s, rng);
            if pieces.len() >= 2 {
                let small = pieces.iter().map(|p| area_f(&p)).fold(f32::MAX, f32::min);
                // Compactness is asked only of pieces that are final: a
                // remainder still several lots long is cut again and may
                // be as thin as a row of lots is.
                let compact_ok = pieces.iter().all(|p| {
                    let q = from_geo(p);
                    let a = area_f(&p);
                    let more_to_cut = a >= lw * ld * 2.5 && frontage_runs(&q, roads, allow).first().is_some_and(|(_, l)| *l >= lw * 2.0);
                    a < MIN_AREA * 0.5 || more_to_cut || a / perimeter(&q).powi(2) >= MIN_COMPACTNESS * 0.9
                });
                let frontage_ok = pieces.iter().all(|p| {
                    let q = from_geo(p);
                    area_f(&p) < MIN_AREA * 0.5 || frontage_runs(&q, roads, allow).first().map_or(false, |(_, l)| *l >= lw * 0.55)
                });
                let wide_enough = pieces.iter().all(|p| {
                    let q = from_geo(p);
                    area_f(&p) < MIN_AREA * 0.5 || frontage_runs(&q, roads, allow).first().map_or(false, |(_, l)| *l >= lw * 0.8)
                });
                if (small >= target * 0.75 || (small >= MIN_AREA * 1.6 && wide_enough)) && compact_ok && frontage_ok {
                    for piece in pieces {
                        subdivide_sized(piece, lu, (lw, ld), roads, rng, depth + 1, out);
                    }
                    return;
                }
            }
            s += lw * 0.35;
        }
    }
    out.push(pts);
}

/// Backland (pieces with no road edge, or slivers) is folded into the
/// neighbouring usable parcel that shares the longest boundary, as long as
/// the result stays convex and no more than three lots deep. Real
/// subdivisions do the same: lots run back until they meet.
pub(super) fn merge_backland(mut parcels: Vec<Vec<Vec2>>, lu: LandUse, roads: &RoadNetwork) -> Vec<Vec<Vec2>> {
    let lw = lu.lot_width();
    let ld = lu.lot_depth();
    let max_area = lw * ld * 3.2;
    for _ in 0..4 {
        let n = parcels.len();
        let good: Vec<bool> = parcels
            .iter()
            .map(|p| fronted(p, lu, roads) && reflex_vertex(p, 0.6).is_none() && polygon_area(p).abs() / perimeter(p).powi(2) >= MIN_COMPACTNESS)
            .collect();
        let boxes: Vec<(Vec2, Vec2)> = parcels.iter().map(|p| bounds_of(p)).collect();
        let mut removed = vec![false; n];
        let mut merged_any = false;
        for i in 0..n {
            if good[i] || removed[i] || reflex_vertex(&parcels[i], 0.6).is_some() {
                continue;
            }
            let per_i = perimeter(&parcels[i]);
            let gi = to_geo(&parcels[i]);
            let mut best: Option<(f32, usize, Vec<Vec2>)> = None;
            for j in 0..n {
                if i == j || !good[j] || removed[j] {
                    continue;
                }
                if !boxes_overlap(boxes[i], boxes[j], 0.5) {
                    continue;
                }
                let u: MultiPolygon<f64> = gi.union(&to_geo(&parcels[j]));
                if u.0.len() != 1 {
                    continue;
                }
                let up = from_geo(&u.0[0]);
                if up.len() < 3 || polygon_area(&up) > max_area {
                    continue;
                }
                let shared = (per_i + perimeter(&parcels[j]) - perimeter(&up)) * 0.5;
                if shared < 3.0 || reflex_vertex(&up, 0.6).is_some() {
                    continue;
                }
                let (fi, _) = frontage_edge(&up, roads, lu.fronts_paths());
                let m = up.len();
                let along = (up[(fi + 1) % m] - up[fi]).normalize_or_zero();
                let inward = Vec2::new(-along.y, along.x);
                let (v0, v1) = extents(&up, inward);
                if v1 - v0 > ld * 3.0 {
                    continue;
                }
                if best.as_ref().map_or(true, |(b, _, _)| shared > *b) {
                    best = Some((shared, j, up));
                }
            }
            if let Some((_, j, up)) = best {
                parcels[j] = up;
                removed[i] = true;
                merged_any = true;
            }
        }
        parcels = parcels.into_iter().zip(removed).filter(|(_, r)| !r).map(|(p, _)| p).collect();
        if !merged_any {
            break;
        }
    }
    parcels
}

pub(super) fn polygon_seed(poly: &[Vec2]) -> u64 {
    crate::hash::fnv_words(poly.iter().flat_map(|p| [p.x.to_bits() as u64, p.y.to_bits() as u64]))
}

/// Seed from a block's area and bounding box, rounded to the metre: the
/// same land gives the same stream.
pub(super) fn block_seed(p: &Polygon<f64>) -> u64 {
    let ring = from_geo(p);
    let (lo, hi) = bounds_of(&ring);
    crate::hash::fnv_words([area_f(p), lo.x, lo.y, hi.x, hi.y].map(|v| v.round() as i64 as u64))
}

/// Halve big landlocked pieces across their long axis until each is a few
/// parcels' worth.
pub(super) fn chop_backland(poly: Vec<Vec2>, roads: &RoadNetwork, depth: u32, out: &mut Vec<Vec<Vec2>>) {
    let limit = PARCEL_USE.lot_width() * PARCEL_USE.lot_depth() * BACKLAND_PARCELS;
    if depth > 10 || poly.len() < 3 || polygon_area(&poly).abs() <= limit || fronted(&poly, PARCEL_USE, roads) {
        out.push(poly);
        return;
    }
    let pts = ccw(&poly);
    let n = pts.len();
    let (mut li, mut ll) = (0, 0.0);
    for i in 0..n {
        let l = pts[i].distance(pts[(i + 1) % n]);
        if l > ll {
            li = i;
            ll = l;
        }
    }
    let axis = (pts[(li + 1) % n] - pts[li]).normalize_or_zero();
    let (u0, u1) = extents(&pts, axis);
    let perp = Vec2::new(-axis.y, axis.x);
    let (v0, v1) = extents(&pts, perp);
    let origin = axis * ((u0 + u1) * 0.5) + perp * ((v0 + v1) * 0.5);
    let pieces = cut(&to_geo(&pts), origin, axis);
    if pieces.len() < 2 {
        out.push(pts);
        return;
    }
    for piece in pieces {
        chop_backland(from_geo(&piece), roads, depth + 1, out);
    }
}
