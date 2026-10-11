//! Joining parcels into the wider sites some land uses want.

use super::*;

/// [plant-scale] How thick a piece of ground is: twice its area over its
/// perimeter (a square's half side, a long strip's width).
pub fn site_thickness(poly: &[Vec2]) -> f32 {
    let per = perimeter(poly);
    if per <= 0.0 { 0.0 } else { 2.0 * polygon_area(poly).abs() / per }
}

/// [attractions] How much of its convex hull a concave site must fill.
pub const CONCAVE_MIN_FILL: f32 = 0.8;

/// [attractions] A polygon's area over its convex hull's (1 when convex).
pub fn hull_fill(poly: &[Vec2]) -> f32 {
    let mut pts: Vec<Vec2> = poly.to_vec();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    if pts.len() < 3 {
        return 1.0;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<Vec2> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in iter {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    let h = polygon_area(&hull).abs();
    if h <= 0.0 { 1.0 } else { polygon_area(poly).abs() / h }
}

/// [plant-scale] A site for a building the player puts on unzoned ground:
/// parcel `start` joined with as few of its neighbours in `polys` as it
/// takes to reach `need` square metres with a frontage (a street or a
/// path), never past `cap`. Each step takes the neighbour that reaches the
/// need with the smallest site, else the one sharing the longest boundary
/// (compact sites, not strings of parcels); the joined outline stays one
/// convex-ish piece. Returns the members (indices into `polys`, sorted) and
/// the outline; short of `need` when the neighbours run out.
pub fn grow_site(polys: &[Vec<Vec2>], start: usize, need: f32, min_thick: f32, cap: f32, roads: &RoadNetwork) -> (Vec<usize>, Vec<Vec2>) {
    grow_site_steps(polys, start, need, min_thick, cap, roads, 12, false)
}

/// [`grow_site`] taking in up to `steps` neighbours; `concave` lets the
/// site take an L or a notch on the way ([attractions] a grand park or a
/// zoo spans a score of small plots, and rows of plots of different widths
/// never add up to a rectangle one plot at a time).
#[allow(clippy::too_many_arguments)]
pub fn grow_site_steps(polys: &[Vec<Vec2>], start: usize, need: f32, min_thick: f32, cap: f32, roads: &RoadNetwork, steps: usize, concave: bool) -> (Vec<usize>, Vec<Vec2>) {
    // [plant-scale] "Fronted" also means thick enough to build on.
    let fronted = |p: &[Vec2]| frontage_width(p, PARCEL_USE, roads).is_some() && site_thickness(p) >= min_thick;
    let mut members = vec![start];
    let mut outline = polys[start].clone();
    for _ in 0..steps {
        let area = polygon_area(&outline).abs();
        let has_front = fronted(&outline);
        if area >= need && has_front {
            break;
        }
        let bo = bounds_of(&outline);
        let go = to_geo(&outline);
        let per = perimeter(&outline);
        // (rank, score, index, outline): a site that reaches the need with a
        // frontage ranks 2 and scores smaller-is-better; otherwise one that
        // gains a frontage ranks 1, the rest 0, scored by shared boundary.
        let mut best: Option<(u8, f32, usize, Vec<Vec2>)> = None;
        for (j, pj) in polys.iter().enumerate() {
            if members.contains(&j) || pj.len() < 3 || !boxes_overlap(bo, bounds_of(pj), 0.5) {
                continue;
            }
            let u: MultiPolygon<f64> = go.union(&to_geo(pj));
            if u.0.len() != 1 || !u.0[0].interiors().is_empty() {
                continue;
            }
            let up = simplify_collinear(&from_geo(&u.0[0]));
            if up.len() < 3 || (!concave && reflex_vertex(&up, 0.6).is_some()) {
                continue;
            }
            // [attractions] Notches, yes; a C wrapped round a plot it could
            // not take, no: the site fills most of its convex hull.
            if concave && hull_fill(&up) < CONCAVE_MIN_FILL {
                continue;
            }
            let shared = (per + perimeter(pj) - perimeter(&up)) * 0.5;
            let au = polygon_area(&up).abs();
            if shared < 4.0 || au > cap {
                continue;
            }
            let front = fronted(&up);
            let (rank, score) = if au >= need && front { (2u8, -au) } else { (u8::from(front && !has_front), shared) };
            if best.as_ref().is_none_or(|b| (rank, score) > (b.0, b.1)) {
                best = Some((rank, score, j, up));
            }
        }
        let Some((_, _, j, up)) = best else { break };
        members.push(j);
        outline = up;
    }
    members.sort();
    (members, outline)
}

/// Join neighbouring parcels given the same land use into lots about the
/// size that use wants (works yards and civic sites span two or three
/// parcels, a shop or a club takes a small neighbour). A piece under three
/// quarters of the use's lot area takes the neighbour that keeps the joined
/// lot smallest, as long as the result is one convex-ish piece with
/// frontage, at most 1.35 lot areas and 1.6 lot widths. Then backland
/// (pieces with no frontage this use can take) is folded into the
/// neighbouring lot sharing the longest boundary, while the result stays
/// convex-ish, under 2.8 lot areas and three lots deep; what cannot be
/// folded stays a landlocked lot of its own. Returns the members of each
/// lot (indices into `polys`) and its outline. Uses that do not join get
/// one lot per parcel.
pub fn join_parcels(polys: &[Vec<Vec2>], lu: LandUse, roads: &RoadNetwork) -> Vec<(Vec<usize>, Vec<Vec2>)> {
    let mut groups: Vec<Option<(Vec<usize>, Vec<Vec2>)>> = polys.iter().enumerate().map(|(i, p)| Some((vec![i], p.clone()))).collect();
    if lu.joins_parcels() {
        let lw = lu.lot_width();
        let target = lu.lot_area();
        for _ in 0..4 {
            let mut merged = false;
            for i in 0..groups.len() {
                let Some((_, pi)) = groups[i].clone() else { continue };
                if frontage_width(&pi, lu, roads).is_none() || polygon_area(&pi).abs() >= target * 0.75 {
                    continue;
                }
                let bi = bounds_of(&pi);
                let gi = to_geo(&pi);
                let per_i = perimeter(&pi);
                let mut best: Option<(f32, usize, Vec<Vec2>)> = None;
                for (j, g) in groups.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let Some((_, pj)) = g else { continue };
                    if !boxes_overlap(bi, bounds_of(pj), 0.5) {
                        continue;
                    }
                    let u: MultiPolygon<f64> = gi.union(&to_geo(pj));
                    if u.0.len() != 1 || !u.0[0].interiors().is_empty() {
                        continue;
                    }
                    let up = simplify_collinear(&from_geo(&u.0[0]));
                    if up.len() < 3 {
                        continue;
                    }
                    let shared = (per_i + perimeter(pj) - perimeter(&up)) * 0.5;
                    if shared < 4.0 || reflex_vertex(&up, 0.6).is_some() {
                        continue;
                    }
                    let Some(wu) = frontage_width(&up, lu, roads) else { continue };
                    let au = polygon_area(&up).abs();
                    if wu > lw * 1.6 || au > target * 1.35 {
                        continue;
                    }
                    if best.as_ref().map_or(true, |b| au < b.0) {
                        best = Some((au, j, up));
                    }
                }
                if let Some((_, j, up)) = best {
                    let (mj, _) = groups[j].take().unwrap();
                    let g = groups[i].as_mut().unwrap();
                    g.0.extend(mj);
                    g.0.sort();
                    g.1 = up;
                    merged = true;
                }
            }
            if !merged {
                break;
            }
        }
    }
    if lu.takes_backland() {
        let (ld, max_area) = (lu.lot_depth(), backland_max_area(lu));
        for _ in 0..4 {
            let mut merged = false;
            for i in 0..groups.len() {
                let Some((_, pi)) = groups[i].clone() else { continue };
                if frontage_width(&pi, lu, roads).is_some() {
                    continue;
                }
                let bi = bounds_of(&pi);
                let gi = to_geo(&pi);
                let per_i = perimeter(&pi);
                let mut best: Option<(f32, usize, Vec<Vec2>)> = None;
                for (j, g) in groups.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let Some((_, pj)) = g else { continue };
                    if !boxes_overlap(bi, bounds_of(pj), 0.5) || frontage_width(pj, lu, roads).is_none() {
                        continue;
                    }
                    let Some((shared, up)) = fold_union(&gi, per_i, pj, lu, roads, max_area, ld) else { continue };
                    if best.as_ref().map_or(true, |b| shared > b.0) {
                        best = Some((shared, j, up));
                    }
                }
                if let Some((_, j, up)) = best {
                    let (mi, _) = groups[i].take().unwrap();
                    let g = groups[j].as_mut().unwrap();
                    g.0.extend(mi);
                    g.0.sort();
                    g.1 = up;
                    merged = true;
                }
            }
            if !merged {
                break;
            }
        }
    }
    groups.into_iter().flatten().collect()
}

/// [fix/roads-tools] The largest lot backland is folded into, for `lu`.
pub fn backland_max_area(lu: LandUse) -> f32 {
    lu.lot_area() * 2.8
}

/// Backland `gi` (perimeter `per_i`) folded into the fronted piece `pj`:
/// the shared boundary and the joined outline, if the result is one
/// convex-ish piece under `max_area` and at most three lots (`ld` each)
/// deep from its frontage.
fn fold_union(gi: &Polygon<f64>, per_i: f32, pj: &[Vec2], lu: LandUse, roads: &RoadNetwork, max_area: f32, ld: f32) -> Option<(f32, Vec<Vec2>)> {
    let u: MultiPolygon<f64> = gi.union(&to_geo(pj));
    if u.0.len() != 1 || !u.0[0].interiors().is_empty() {
        return None;
    }
    let up = simplify_collinear(&from_geo(&u.0[0]));
    if up.len() < 3 || polygon_area(&up).abs() > max_area || reflex_vertex(&up, 0.6).is_some() {
        return None;
    }
    let shared = (per_i + perimeter(pj) - perimeter(&up)) * 0.5;
    if shared < 3.0 {
        return None;
    }
    let (fi, _) = frontage_edge(&up, roads, lu.fronts_paths());
    let m = up.len();
    let along = (up[(fi + 1) % m] - up[fi]).normalize_or_zero();
    let (v0, v1) = extents(&up, Vec2::new(-along.y, along.x));
    if v1 - v0 > ld * 3.0 {
        return None;
    }
    Some((shared, up))
}

/// [fix/roads-tools] Could backland `back` be folded into a lot of `lu`
/// in front of it: is one of `fronted` (pieces with a frontage `lu` can
/// use) a neighbour it joins as `join_parcels` would join it? (Flag G5:
/// civic paint on backland nothing could take in came out as lots of
/// unusable ground.)
pub fn backland_joins_any<'a>(back: &[Vec2], fronted: impl IntoIterator<Item = &'a [Vec2]>, lu: LandUse, roads: &RoadNetwork) -> bool {
    let gi = to_geo(back);
    let per = perimeter(back);
    let bi = bounds_of(back);
    let (max_area, ld) = (backland_max_area(lu), lu.lot_depth());
    let touching: Vec<&[Vec2]> = fronted.into_iter().filter(|pj| boxes_overlap(bi, bounds_of(pj), 0.5) && frontage_width(pj, lu, roads).is_some()).collect();
    if touching.iter().any(|pj| fold_union(&gi, per, pj, lu, roads, max_area, ld).is_some()) {
        return true;
    }
    // The fronts are joined into wider lots first (`join_parcels`): the
    // backland may fit behind two or three of them together.
    let mut merged: Option<MultiPolygon<f64>> = None;
    for pj in &touching {
        let u: MultiPolygon<f64> = gi.union(&to_geo(pj));
        let shared = u.0.len() == 1 && (per + perimeter(pj) - perimeter(&from_geo(&u.0[0]))) * 0.5 >= 3.0;
        if shared {
            merged = Some(match merged {
                Some(m) => m.union(&MultiPolygon(vec![to_geo(pj)])),
                None => MultiPolygon(vec![to_geo(pj)]),
            });
        }
    }
    match merged {
        Some(m) if m.0.len() == 1 => {
            let front = from_geo(&m.0[0]);
            frontage_width(&front, lu, roads).is_some() && fold_union(&gi, per, &front, lu, roads, max_area, ld).is_some()
        }
        _ => false,
    }
}
