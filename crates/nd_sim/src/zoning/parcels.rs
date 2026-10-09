//! Parcels: cut along every road as it is built, in world-stable chunks,
//! keyed so zoning finds them again.

use super::*;

/// Every parcel is cut to a home's lot; uses that want wider lots join
/// neighbouring parcels ([`join_parcels`]).
pub const PARCEL_USE: LandUse = LandUse::Residential;

/// Parcels reach this far behind the edge of a street's or path's corridor.
pub const PARCEL_REACH: f32 = 60.0;

/// Landlocked pieces bigger than this many parcels are chopped up, so a
/// park can be painted onto backland a piece at a time.
pub(super) const BACKLAND_PARCELS: f32 = 4.0;

/// Land enclosed by the reach of the roads around it is parcelled too, up
/// to this area (m²); bigger enclosures keep their open middle.
pub(super) const ENCLOSED_FILL: f64 = 40_000.0;

/// Resolution of parcel keys, in metres.
pub(super) const KEY_UNIT: f32 = 0.25;

/// Long straight frontages are first cut across where the coordinate along
/// the road is a multiple of this, so a block's parcels do not depend on
/// where its far end is.
pub(super) const CHUNK: f32 = 96.0;

/// A parcel's identity across rebuilds: its centre and frontage edge on a
/// quarter-metre grid. Lot ids change every time the lots are rebuilt; a key
/// stays the same as long as the parcel does. A parcel reshaped by road
/// works is found again by the old centre ([`ParcelMap::find`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParcelKey {
    pub center: (i32, i32),
    pub front: [(i32, i32); 2],
}

impl ParcelKey {
    pub fn new(center: Vec2, front: [Vec2; 2]) -> Self {
        let q = |p: Vec2| ((p.x / KEY_UNIT).round() as i32, (p.y / KEY_UNIT).round() as i32);
        Self { center: q(center), front: [q(front[0]), q(front[1])] }
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.center.0 as f32, self.center.1 as f32) * KEY_UNIT
    }
}

/// A piece of land along a road, cut whether or not it is zoned.
#[derive(Clone, Debug)]
pub struct Parcel {
    pub key: ParcelKey,
    /// Counter-clockwise outline.
    pub polygon: Vec<Vec2>,
    /// A point inside the outline (the centroid when that is inside).
    pub center: Vec2,
    pub front_edge: [Vec2; 2],
    pub area: f32,
    /// [waterfront] Metres of the outline on the waterline (0 inland).
    pub water_frontage: f32,
    /// Has a frontage a home may use (a street or a footpath).
    pub path_frontage: bool,
    /// Has a street frontage, which shops, industry and the rest need.
    pub street_frontage: bool,
    /// Too steep to level, or standing in the water.
    pub rough: bool,
    /// [island-lots] Why the ground is rough, in plain words (a cliff,
    /// water), for the Zone tool's refusal.
    pub rough_why: Option<&'static str>,
    /// Land use as built (from the master plan); None is unzoned.
    pub land_use: Option<LandUse>,
    /// The lot this parcel is part of, once zoned and built into one.
    pub lot: Option<LotId>,
}

impl Parcel {
    /// No frontage anyone may use, or rough ground: backland.
    pub fn landlocked(&self) -> bool {
        self.rough || !self.path_frontage
    }

    /// Why this parcel cannot take `lu`, or None if it can. The ruleset:
    /// parks go anywhere, backland and rough ground included (a park is
    /// reached through the park); homes need a street or path to front;
    /// everything else needs a street; industry and civic sites are not put
    /// on scraps, but their deep yards may take the backland behind them
    /// (it is folded into the lot in front, see [`join_parcels`]).
    pub fn refusal(&self, lu: LandUse) -> Option<&'static str> {
        if lu == LandUse::Park {
            return None;
        }
        if self.rough {
            return Some(self.rough_why.unwrap_or("too steep or wet to build on"));
        }
        if !self.path_frontage {
            if lu.takes_backland() {
                return None;
            }
            return Some("landlocked: no street or path to front");
        }
        if !lu.fronts_paths() && !self.street_frontage {
            return Some("fronts only a footpath; this use needs a street");
        }
        if self.area < lu.min_parcel_area() {
            return Some("too small for this use");
        }
        None
    }

    pub fn accepts(&self, lu: LandUse) -> bool {
        self.refusal(lu).is_none()
    }

    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.polygon)
    }
}

/// The parcels of the city with a key index and a spatial grid.
#[derive(Clone, Debug, Default)]
pub struct ParcelMap {
    pub parcels: Vec<Parcel>,
    pub(super) by_key: HashMap<ParcelKey, usize>,
    pub(super) grid: Grid,
    /// Bumped every time the parcels are cut again.
    pub version: u64,
}

impl ParcelMap {
    pub fn new(parcels: Vec<Parcel>, version: u64) -> Self {
        let by_key = parcels.iter().enumerate().map(|(i, p)| (p.key, i)).collect();
        let grid = Grid::new(48.0, parcels.iter().map(|p| bounds_of(&p.polygon)));
        ParcelMap { parcels, by_key, grid, version }
    }

    pub fn len(&self) -> usize {
        self.parcels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parcels.is_empty()
    }

    /// The parcel under a point.
    pub fn at(&self, p: Vec2) -> Option<usize> {
        self.grid.in_box(p, p).into_iter().map(|i| i as usize).find(|&i| self.parcels[i].contains(p))
    }

    /// Parcels a brush of radius `r` at `c` touches.
    pub fn in_circle(&self, c: Vec2, r: f32) -> Vec<usize> {
        self.grid
            .in_box(c - Vec2::splat(r), c + Vec2::splat(r))
            .into_iter()
            .map(|i| i as usize)
            .filter(|&i| {
                let p = &self.parcels[i];
                p.contains(c) || distance_to_outline(c, &p.polygon) <= r
            })
            .collect()
    }

    /// Parcels whose centre lies inside `poly`.
    pub fn in_polygon(&self, poly: &[Vec2]) -> Vec<usize> {
        if poly.len() < 3 {
            return Vec::new();
        }
        let (lo, hi) = bounds_of(poly);
        self.grid.in_box(lo, hi).into_iter().map(|i| i as usize).filter(|&i| point_in_polygon(self.parcels[i].center, poly)).collect()
    }

    /// The parcel a key names: the same parcel if it is still there, else
    /// the one that now holds its old centre.
    pub fn find(&self, key: &ParcelKey) -> Option<usize> {
        self.by_key.get(key).copied().or_else(|| self.at(key.center()))
    }
}

/// [waterfront] How far behind a road's corridor its parcels may reach to
/// meet the water: where a street or path runs within this of a river,
/// lake or the sea, the land between it and the water is all parcelled,
/// lots running the whole way down to the waterline.
pub const SHORE_REACH: f32 = 80.0;

/// Cut parcels along every street and path: the land within
/// [`PARCEL_REACH`] of the corridors, minus every corridor (track and
/// highway included) and the water, clipped to `clip`, each block sliced as
/// [`subdivide`] does for a home's lot. Long straight frontages are first
/// chunked at fixed world positions along the road ([`CHUNK`]), and every
/// chunk draws its own random stream from its own shape, so road works
/// elsewhere never move a parcel here: the same road gives the same
/// parcels.
///
/// With `shore_reach` ([waterfront]) the land
/// between a road and water within that reach of its corridor is
/// parcelled too, and blocks that touch the water are cut into lots that
/// run from the road the whole way to the shore (frontage-wide slices, not
/// rows a lot deep). Every parcel learns its frontage on the water.
pub fn generate_parcels_with(roads: &RoadNetwork, water: &WaterMask, clip: Option<(Vec2, Vec2)>, shore_reach: Option<f32>) -> Vec<Parcel> {
    let mut out = Vec::new();
    if roads.is_empty() {
        return out;
    }
    let shore_reach = shore_reach.filter(|r| *r > PARCEL_REACH && !water.is_empty());
    let mut corridors: Vec<Polygon<f64>> = Vec::new();
    let mut reach: Vec<Polygon<f64>> = Vec::new();
    // [waterfront] The wider reach, kept only where it meets the water.
    let mut wide: Vec<Polygon<f64>> = Vec::new();
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for (pts, hw, ext_a, ext_b, sid) in road_chains(roads) {
        if let Some(c) = chain_corridor(&pts, hw, ext_a, ext_b) {
            corridors.push(c);
        }
        let seg = roads.segment(sid);
        // Highways, their slip roads, tunnels and track front nothing, so
        // no parcels hang off them.
        if seg.is_rail() || seg.fronts_nothing() {
            continue;
        }
        let r = hw + shore_reach.unwrap_or(PARCEL_REACH);
        for p in &pts {
            lo = lo.min(*p - Vec2::splat(r + hw));
            hi = hi.max(*p + Vec2::splat(r + hw));
        }
        reach_pieces(&pts, hw, hw + PARCEL_REACH, ext_a, ext_b, &mut reach);
        if shore_reach.is_some() {
            reach_pieces(&pts, hw, r, ext_a, ext_b, &mut wide);
        }
    }
    if reach.is_empty() {
        return out;
    }
    let mut region: MultiPolygon<f64> = geo::unary_union(&reach);
    // [waterfront] Land past the usual reach that runs down to the water
    // (nothing else in the way) joins the region: between a road and the
    // shore, every metre is somebody's lot.
    if !wide.is_empty() {
        let wide: MultiPolygon<f64> = geo::unary_union(&wide);
        let mut extra = wide.difference(&region);
        if let Some(w) = water.near(lo, hi) {
            extra = extra.difference(&w);
        }
        // Only land that also borders the road's own reach: not the far
        // bank across the water.
        use geo::Intersects;
        let keep: Vec<Polygon<f64>> = extra.0.into_iter().filter(|p| area_f(p) >= MIN_AREA && water.touches(&from_geo(p), 1.2) && region.intersects(p)).collect();
        if !keep.is_empty() {
            region = region.union(&MultiPolygon(keep));
        }
    }
    // Ground the reach does not quite get to but that roads close round
    // (the middle of a big block) is still that block's: fill such holes,
    // or the block would be split around a speck of nobody's land.
    region = MultiPolygon(
        region
            .0
            .into_iter()
            .map(|p| {
                let keep: Vec<LineString<f64>> = p.interiors().iter().filter(|r| Polygon::new((*r).clone(), vec![]).unsigned_area() > ENCLOSED_FILL).cloned().collect();
                Polygon::new(p.exterior().clone(), keep)
            })
            .collect(),
    );
    let corridors: MultiPolygon<f64> = geo::unary_union(&corridors);
    region = region.difference(&corridors);
    if let Some((clo, chi)) = clip {
        region = region.intersection(&MultiPolygon(vec![to_geo(&[clo, Vec2::new(chi.x, clo.y), chi, Vec2::new(clo.x, chi.y)])]));
    }
    if let Some(w) = water.near(lo, hi) {
        region = region.difference(&w);
    }
    let mut polys: Vec<Vec<Vec2>> = Vec::new();
    for block in region.0 {
        if area_f(&block) < MIN_AREA {
            continue;
        }
        for chunk in chunk_block(block, roads) {
            let chunk = canonical(chunk);
            let mut rng = crate::rng_for(block_seed(&chunk), 5);
            let mut pieces: Vec<Vec<Vec2>> = Vec::new();
            // [waterfront] A chunk on the water that took in shore land is
            // sliced into lots running from the road to the waterline.
            match shore_reach.and_then(|_| shore_lot_size(&chunk, water, roads)) {
                Some(size) => subdivide_sized(chunk, PARCEL_USE, size, roads, &mut rng, 0, &mut pieces),
                None => subdivide(chunk, PARCEL_USE, roads, &mut rng, 0, &mut pieces),
            }
            for piece in merge_backland(pieces, PARCEL_USE, roads) {
                chop_backland(piece, roads, 0, &mut polys);
            }
        }
    }
    for poly in polys {
        if let Some(mut p) = parcel_of(poly, roads) {
            if shore_reach.is_some() || !water.is_empty() {
                p.water_frontage = water.shoreline(&p.polygon).0;
            }
            out.push(p);
        }
    }
    out
}

/// [waterfront] The lot size for a chunk of land on the water: as deep
/// as the chunk runs back from its road (so one row reaches the water),
/// and wide in proportion. None for a chunk off the water, one shallow
/// enough for ordinary rows to reach it, or one with no road to measure
/// from.
pub(super) fn shore_lot_size(chunk: &Polygon<f64>, water: &WaterMask, roads: &RoadNetwork) -> Option<(f32, f32)> {
    let pts = from_geo(chunk);
    if pts.len() < 3 || !water.touches(&pts, 1.2) {
        return None;
    }
    let runs = frontage_runs(&pts, roads, true);
    let (chain, _) = runs.first()?;
    let n = pts.len();
    let &edge = chain.iter().max_by(|&&a, &&b| pts[a].distance(pts[(a + 1) % n]).total_cmp(&pts[b].distance(pts[(b + 1) % n])))?;
    let (a, b) = (pts[edge], pts[(edge + 1) % n]);
    let d = (b - a).normalize_or_zero();
    if d == Vec2::ZERO {
        return None;
    }
    let inward = Vec2::new(-d.y, d.x);
    let deep = pts.iter().map(|p| (*p - a).dot(inward)).fold(0.0f32, f32::max);
    if deep <= PARCEL_USE.lot_depth() * 1.8 {
        return None;
    }
    // One band (1.15 of the depth) takes the whole chunk.
    let ld = deep / 1.1;
    let lw = (deep * 0.38).clamp(PARCEL_USE.lot_width(), 36.0);
    Some((lw, ld))
}

/// Cut a block across every long straight frontage edge where the
/// coordinate along the edge's direction is a multiple of [`CHUNK`]. The
/// lines are fixed in the world, so the chunks away from a change keep
/// their exact shape.
pub(super) fn chunk_block(block: Polygon<f64>, roads: &RoadNetwork) -> Vec<Polygon<f64>> {
    let pts = from_geo(&block);
    let n = pts.len();
    let mut lines: Vec<(Vec2, Vec2)> = Vec::new();
    let margin = PARCEL_USE.lot_width() * 1.5;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a.distance(b) < CHUNK * 2.0 || edge_on_road(roads, a, b, true).is_none() {
            continue;
        }
        let d = (b - a).normalize_or_zero();
        // Canonical direction, so both sides of a road agree on the lines.
        let d = if d.x < -1e-6 || (d.x.abs() <= 1e-6 && d.y < 0.0) { -d } else { d };
        let (s0, s1) = (a.dot(d).min(b.dot(d)), a.dot(d).max(b.dot(d)));
        let mut k = ((s0 + margin) / CHUNK).ceil() as i64;
        while (k as f32) * CHUNK <= s1 - margin {
            let s = k as f32 * CHUNK;
            let t = (s - a.dot(d)) / (b - a).dot(d);
            lines.push((a.lerp(b, t), d));
            k += 1;
        }
    }
    let mut pieces = vec![block];
    for (origin, normal) in lines {
        let mut next = Vec::new();
        for piece in pieces {
            let (lo, hi) = extents(&from_geo(&piece), normal);
            let at = origin.dot(normal);
            if at - lo > 1.0 && hi - at > 1.0 {
                let cut_pieces = cut(&piece, origin, normal);
                if cut_pieces.len() >= 2 {
                    next.extend(cut_pieces);
                    continue;
                }
            }
            next.push(piece);
        }
        pieces = next;
    }
    pieces
}

/// A parcel from an outline, with its frontage judged for homes and for
/// everything else. None when there is no road anywhere to hang it on.
pub(super) fn parcel_of(poly: Vec<Vec2>, roads: &RoadNetwork) -> Option<Parcel> {
    let poly = ccw(&poly);
    if poly.len() < 3 {
        return None;
    }
    let center = inner_center(&poly);
    let home = make_lot(ZoneId::default(), PARCEL_USE, poly.clone(), center, roads)?;
    let street = make_lot(ZoneId::default(), LandUse::Commercial, poly.clone(), center, roads).is_some_and(|l| !l.landlocked);
    Some(Parcel {
        key: ParcelKey::new(center, home.front_edge),
        area: polygon_area(&poly).abs(),
        center,
        front_edge: home.front_edge,
        path_frontage: !home.landlocked,
        street_frontage: street,
        rough: false,
        rough_why: None,
        water_frontage: 0.0,
        land_use: None,
        lot: None,
        polygon: poly,
    })
}
