//! [tools] The columns under raised roads in the world (`crate::pillars`):
//! what stands where a column would, the plan for a road being drawn (the
//! Road tool's preview), placing them when roads are rebuilt, and the
//! headroom under a deck that spans over lots.

use super::*;
use crate::geom::{distance_to_outline, point_in_polygon};
use crate::pillars::{self, Pillar, PillarPlan, Site, PILLAR_RADIUS};
use std::collections::HashSet;

/// Half width of the deck of a way of this kind and these lanes: the
/// carriageway and its pavements (a highway and its ramps have none).
pub fn deck_half_width(kind: RoadKind, fwd: u8, bwd: u8) -> f32 {
    deck_half_width_use(kind, fwd, bwd, None)
}

/// [w1-roads] [`deck_half_width`] with the road's use set.
pub fn deck_half_width_use(kind: RoadKind, fwd: u8, bwd: u8, car_only: Option<bool>) -> f32 {
    match kind {
        RoadKind::Street => {
            let (f, b) = if fwd == 0 && bwd == 0 { (1, 1) } else { (fwd, bwd) };
            let motorway = crate::roads::is_highway_use(kind, f, b, car_only) || crate::roads::is_ramp_use(kind, f, b, car_only);
            (f + b) as f32 * crate::roads::LANE_WIDTH * 0.5 + crate::roads::SHOULDER + if motorway { 0.6 } else { crate::zoning::SIDEWALK }
        }
        RoadKind::Path => crate::roads::PATH_HALF_WIDTH + 0.6,
        RoadKind::Rail => crate::roads::RAIL_HALF_WIDTH + 0.6,
    }
}

/// Distance from segment `a`-`b` to a polygon (0 when they touch).
fn segment_polygon_distance(a: Vec2, b: Vec2, poly: &[Vec2]) -> f32 {
    if point_in_polygon(a, poly) || point_in_polygon(b, poly) {
        return 0.0;
    }
    let mut d = distance_to_outline(a, poly).min(distance_to_outline(b, poly));
    for &v in poly {
        d = d.min(crate::roads::project_on_segment(v, a, b).1.distance(v));
    }
    let n = poly.len();
    for i in 0..n {
        if crate::roads::segment_intersection(a, b, poly[i], poly[(i + 1) % n]).is_some() {
            return 0.0;
        }
    }
    d
}

impl World {
    /// A building the city never removes for a column: anything the player
    /// placed (plants, depots, stations, services, docks, hubs). A park is
    /// not in the way at all: a column stands in it as on open ground.
    /// [r4-decks] (It used to block a column: Harvey's day-9 city zoned
    /// parks under its raised walkways, the only use their 6 m decks allow,
    /// on the ground of their columns, and every later rebuild found no
    /// room for those columns and showed spans of 45 m and 85 m in red.)
    fn pillar_protected(b: &Building) -> bool {
        Self::is_civic_structure(b)
    }

    /// Lots whose box comes near the polyline: the candidates for its sites.
    fn lots_near_way(&self, pts: &[Vec2], reach: f32) -> Vec<LotId> {
        let (lo, hi) = crate::geom::bounds(pts);
        let (lo, hi) = (lo - Vec2::splat(reach + 60.0), hi + Vec2::splat(reach + 60.0));
        self.lots
            .iter()
            .filter(|(_, l)| l.polygon.len() >= 3 && l.center.x > lo.x && l.center.y > lo.y && l.center.x < hi.x && l.center.y < hi.y)
            .map(|(id, _)| id)
            .collect()
    }

    /// What stands at `p` under a deck at `level`: a road at a lower level
    /// (its pavements and junction corners too) or a facility blocks a
    /// column; a lot with a protected building blocks it; a grown building
    /// is bought out; an empty lot is taken; anything else is free.
    pub fn pillar_site(&self, p: Vec2, level: f32, lots: &[LotId], footprints: &[Vec<Vec2>]) -> Site {
        self.pillar_site_replacing(p, level, lots, footprints, &[])
    }

    /// [r1-roads] (K13) `pillar_site`, leaving out the roads lying along
    /// `replaced` (the road an edit replaces: raised in the Edit tool, the
    /// road as it was, at grade under its own new deck, blocked every
    /// column and the edit was refused for want of room).
    pub fn pillar_site_replacing(&self, p: Vec2, level: f32, lots: &[LotId], footprints: &[Vec<Vec2>], replaced: &[Vec<Vec2>]) -> Site {
        let r = PILLAR_RADIUS;
        let roads = &self.roads;
        for sid in roads.segments_in_box(p - Vec2::splat(40.0), p + Vec2::splat(40.0)) {
            let seg = roads.segment(sid);
            if !replaced.is_empty() {
                let (a, b) = roads.segment_points(sid);
                if replaced.iter().any(|w| crate::geom::distance_to_polyline(a, w) < 1.0 && crate::geom::distance_to_polyline(b, w) < 1.0) {
                    continue;
                }
            }
            let (a, b) = roads.segment_points(sid);
            let (t, q) = crate::roads::project_on_segment(p, a, b);
            let lv = roads.segment_level(sid, t);
            // Deep tunnels leave room for a footing; the deck itself and
            // anything at or above it is no obstacle.
            if lv <= crate::roads::TUNNEL_LEVEL || lv > level - 2.0 {
                continue;
            }
            let side = if seg.is_street() && !seg.fronts_nothing() { crate::zoning::SIDEWALK } else { 0.6 };
            if q.distance(p) < seg.half_width() + side + r {
                return Site::Road;
            }
            // A junction's corners are road too.
            for n in [seg.a, seg.b] {
                let node = roads.node(n);
                let arms = node.segments.iter().filter(|&&o| roads.segment(o).alive).count();
                if arms >= 3 && node.level < level - 2.0 {
                    let widest = node.segments.iter().filter(|&&o| roads.segment(o).alive).map(|&o| roads.segment(o).half_width()).fold(0.0, f32::max);
                    if node.pos.distance(p) < widest + crate::zoning::SIDEWALK + 1.0 + r {
                        return Site::Road;
                    }
                }
            }
        }
        for fp in footprints {
            if point_in_polygon(p, fp) || distance_to_outline(p, fp) < r {
                return Site::Blocked;
            }
        }
        let mut out = Site::Free;
        for &lid in lots {
            let Some(l) = self.lots.get(lid) else { continue };
            if l.center.distance(p) > 120.0 || !(point_in_polygon(p, &l.polygon) || distance_to_outline(p, &l.polygon) < r) {
                continue;
            }
            match l.building.and_then(|b| self.buildings.get(b).map(|bb| (b, bb))) {
                Some((_, b)) if Self::pillar_protected(b) => return Site::Blocked,
                Some((_, b)) if b.land_use == LandUse::Park => {}
                Some((bid, _)) => out = Site::Buyout(lid, bid),
                None if l.land_use == LandUse::Park => {}
                None if matches!(out, Site::Free) => out = Site::EmptyLot(lid),
                None => {}
            }
        }
        out
    }

    /// The columns a raised way along `pts` at `levels` would stand on,
    /// placed against the city as it is (the Road tool's preview and the
    /// project's check). `held`: columns already standing that carry it.
    pub fn plan_pillars(&self, pts: &[Vec2], levels: &[f32], half_width: f32, held: &[Vec2]) -> PillarPlan {
        self.plan_pillars_replacing(pts, levels, half_width, held, &[])
    }

    /// [r1-roads] `plan_pillars` with the roads along `replaced` left out
    /// (`pillar_site_replacing`).
    pub fn plan_pillars_replacing(&self, pts: &[Vec2], levels: &[f32], half_width: f32, held: &[Vec2], replaced: &[Vec<Vec2>]) -> PillarPlan {
        if !pillars::is_raised(levels) {
            return PillarPlan::default();
        }
        let lots = self.lots_near_way(pts, half_width);
        let fps = self.structure_footprints();
        let site = |p: Vec2, level: f32| self.pillar_site_replacing(p, level, &lots, &fps, replaced);
        pillars::plan(pts, levels, half_width, pillars::way_key(pts, levels), &site, held)
    }

    /// The plan for a road as the tools draw it (lanes and kind as given),
    /// with its ends where and at the level the network will lay them (an
    /// end dropped on a road ramps down to it: no column there).
    pub fn road_pillar_plan(&self, pts: &[Vec2], levels: &[f32], fwd: u8, bwd: u8, kind: RoadKind) -> PillarPlan {
        self.road_pillar_plan_replacing(pts, levels, fwd, bwd, kind, &[])
    }

    /// [r1-roads] (K13) `road_pillar_plan` for a road replacing the roads
    /// along `replaced` (the Edit tool's road as it was): they are not in
    /// the way of its columns, and its own old columns hold nothing up.
    pub fn road_pillar_plan_replacing(&self, pts: &[Vec2], levels: &[f32], fwd: u8, bwd: u8, kind: RoadKind, replaced: &[Vec<Vec2>]) -> PillarPlan {
        let (pts, levels) = self.roads.as_built(pts, levels, kind);
        let old: Vec<u64> = replaced.iter().map(|r| r.as_slice()).filter_map(|r| self.pillars.iter().find(|p| crate::geom::distance_to_polyline(p.at, r) < 0.5).map(|p| p.way)).collect();
        let held: Vec<Vec2> = self.pillars.iter().filter(|p| !old.contains(&p.way)).map(|p| p.at).collect();
        self.plan_pillars_replacing(&pts, &levels, deck_half_width(kind, fwd, bwd), &held, replaced)
    }

    /// The columns of every raised way (`ways`: built polyline, levels and
    /// deck half width), placed while the roads are rebuilt: after the new
    /// network is in, before the lots are recut (so the buildings are still
    /// on their old lots). A way drawn as it was keeps its columns where
    /// they stand while every one of them is still clear. Returns the
    /// grown buildings bought out for a column; the gaps (no room) are
    /// kept in `pillar_gaps`.
    pub(super) fn place_pillars(&mut self, ways: &[(u64, Vec<Vec2>, Vec<f32>, f32)], buy: bool) -> HashSet<BuildingId> {
        let mut out: Vec<Pillar> = Vec::new();
        let mut taken: HashSet<BuildingId> = HashSet::new();
        let mut gaps: Vec<pillars::Gap> = Vec::new();
        let fps = self.structure_footprints();
        let this: &World = self;
        let fps_ref = &fps;
        let site_for = |pts: &[Vec2], hw: f32| {
            let lots = this.lots_near_way(pts, hw);
            move |p: Vec2, level: f32| {
                let s = this.pillar_site(p, level, &lots, fps_ref);
                // On load nothing is pulled down: a building is in the way.
                match s {
                    Site::Buyout(..) | Site::EmptyLot(_) if !buy => Site::Blocked,
                    s => s,
                }
            }
        };
        // First the ways drawn as before whose columns all still stand
        // clear: they keep them. Then the rest, each held up where one of
        // those (or one placed before it) stands under it: the order the
        // ways come in makes no difference to what any of them gets, as
        // in the Road tool's preview (`road_pillar_plan`), which counts
        // every standing column.
        let mut todo: Vec<usize> = Vec::new();
        for (i, (key, pts, levels, hw)) in ways.iter().enumerate() {
            if !pillars::is_raised(levels) {
                continue;
            }
            let site = site_for(pts, *hw);
            let old: Vec<Pillar> = self.pillars.iter().filter(|p| p.way == *key).copied().collect();
            if !old.is_empty() && old.iter().all(|p| matches!(site(p.at, p.level), Site::Free | Site::EmptyLot(_))) {
                out.extend(old);
            } else {
                todo.push(i);
            }
        }
        for i in todo {
            let (key, pts, levels, hw) = &ways[i];
            let key = *key;
            let site = site_for(pts, *hw);
            let held: Vec<Vec2> = out.iter().map(|p| p.at).collect();
            let plan = pillars::plan(pts, levels, *hw, key, &site, &held);
            for (_, b) in &plan.buyouts {
                taken.insert(*b);
            }
            // [r4-decks] A way that stood before keeps the columns it stood
            // on in a stretch that has no room now (what grew or was placed
            // round them since), unless a road now runs there: the road
            // stands as it did, and no red span is left on it.
            let old: Vec<Pillar> = self.pillars.iter().filter(|p| p.way == key).copied().collect();
            for g in &plan.gaps {
                let kept: Vec<Pillar> = if g.end { Vec::new() } else { old.iter().filter(|p| pillars::on_stretch(p.at, g) && site(p.at, p.level) != Site::Road).copied().collect() };
                if !pillars::stretch_held(g, &kept) {
                    gaps.push(*g);
                }
                out.extend(kept);
            }
            out.extend(plan.pillars);
        }
        self.pillars = out;
        self.pillar_gaps = gaps;
        taken
    }

    /// Columns standing on this outline (with their footing).
    pub fn pillars_on(&self, poly: &[Vec2]) -> bool {
        self.pillars.iter().any(|p| point_in_polygon(p.at, poly) || distance_to_outline(p.at, poly) < PILLAR_RADIUS)
    }

    /// [p1-sim] Mark the parcels a lot could never stand on for the raised
    /// roads (`Parcel::under_deck`): a column on it, or a deck too low over
    /// it for a storey, the same test the rebuild drops a lot by. Zoning
    /// them is refused, so the plan never holds a zone that gets no lot
    /// (in Harvey's day-2 city 23 parcels by a cloverleaf were zoned under
    /// its ramps, lost their lots and kept their plan hologram for good).
    pub(in crate::world) fn mark_under_decks(&self, parcels: &mut [crate::zoning::Parcel]) {
        let decks = self.roads.alive_segments().any(|(sid, _)| self.roads.spans_lots(sid));
        if self.pillars.is_empty() && !decks {
            return;
        }
        // Columns by 32 m cell, so a parcel looks only at those near it.
        const CELL: f32 = 32.0;
        let cell = |p: Vec2| ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (k, p) in self.pillars.iter().enumerate() {
            grid.entry(cell(p.at)).or_default().push(k);
        }
        for p in parcels.iter_mut() {
            let (lo, hi) = crate::geom::bounds(&p.polygon);
            let (a, b) = (cell(lo - Vec2::splat(PILLAR_RADIUS)), cell(hi + Vec2::splat(PILLAR_RADIUS)));
            let mut column = false;
            'cells: for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for &k in grid.get(&(x, y)).map_or(&[][..], |v| &v[..]) {
                        let at = self.pillars[k].at;
                        if point_in_polygon(at, &p.polygon) || distance_to_outline(at, &p.polygon) < PILLAR_RADIUS {
                            column = true;
                            break 'cells;
                        }
                    }
                }
            }
            p.under_deck = column || (decks && self.deck_cap_of(&p.polygon) == Some(0));
        }
    }

    /// Floors that fit on a lot of this outline under the decks that span
    /// over it (`RoadNetwork::spans_lots`), or None with no deck over it.
    pub fn deck_cap_of(&self, poly: &[Vec2]) -> Option<u8> {
        if poly.len() < 3 {
            return None;
        }
        let (lo, hi) = crate::geom::bounds(poly);
        let mut cap: Option<u8> = None;
        for sid in self.roads.segments_in_box(lo - Vec2::splat(20.0), hi + Vec2::splat(20.0)) {
            if !self.roads.spans_lots(sid) {
                continue;
            }
            let seg = self.roads.segment(sid);
            let (a, b) = self.roads.segment_points(sid);
            let hw = deck_half_width(seg.kind, seg.lanes_fwd, seg.lanes_bwd);
            if segment_polygon_distance(a, b, poly) < hw {
                let level = self.roads.node(seg.a).level.min(self.roads.node(seg.b).level);
                let f = pillars::floors_under(level);
                cap = Some(cap.map_or(f, |c: u8| c.min(f)));
            }
        }
        cap
    }

    /// [r5-layered] Height (world z) of the lowest deck soffit over a lot
    /// of this outline, as built, None with no deck over it: what a
    /// building there must stay under as drawn. On a hilly map a deck's
    /// height over the ground is not its planned level, so it is measured
    /// over a grid of the lot; on flat ground it is the level less the slab.
    pub fn deck_soffit_over(&self, poly: &[Vec2]) -> Option<f32> {
        if poly.len() < 3 {
            return None;
        }
        let (lo, hi) = crate::geom::bounds(poly);
        let any = self.roads.segments_in_box(lo - Vec2::splat(20.0), hi + Vec2::splat(20.0)).any(|sid| {
            let seg = self.roads.segment(sid);
            seg.alive && seg.viaduct && !seg.tunnel && {
                let (a, b) = self.roads.segment_points(sid);
                segment_polygon_distance(a, b, poly) < deck_half_width(seg.kind, seg.lanes_fwd, seg.lanes_bwd)
            }
        });
        if !any {
            return None;
        }
        let slab = pillars::DECK_DEPTH + 0.2;
        let mut soffit: Option<f32> = None;
        const N: usize = 6;
        for i in 0..=N {
            for j in 0..=N {
                let q = Vec2::new(lo.x + (hi.x - lo.x) * i as f32 / N as f32, lo.y + (hi.y - lo.y) * j as f32 / N as f32);
                if !point_in_polygon(q, poly) && distance_to_outline(q, poly) > 0.5 {
                    continue;
                }
                let z = if self.relief {
                    let g = self.ground_height(q);
                    self.ground.decks_at(q).into_iter().filter(|&z| z > g + 1.0).reduce(f32::min)
                } else {
                    self.roads.segments_in_box(q - Vec2::splat(20.0), q + Vec2::splat(20.0)).filter_map(|sid| {
                        let seg = self.roads.segment(sid);
                        if !seg.alive || !seg.viaduct || seg.tunnel {
                            return None;
                        }
                        let (a, b) = self.roads.segment_points(sid);
                        let (t, p) = crate::roads::project_on_segment(q, a, b);
                        (p.distance(q) < deck_half_width(seg.kind, seg.lanes_fwd, seg.lanes_bwd)).then(|| self.roads.segment_level(sid, t))
                    }).filter(|&z| z > 1.0).reduce(f32::min)
                };
                if let Some(z) = z {
                    soffit = Some(soffit.map_or(z - slab, |s: f32| s.min(z - slab)));
                }
            }
        }
        soffit
    }

    /// `deck_cap_of` a lot.
    pub fn deck_cap(&self, lot: LotId) -> Option<u8> {
        self.lots.get(lot).and_then(|l| self.deck_cap_of(&l.polygon))
    }

    /// [r4-decks] Floors that fit under the decks over a building on these
    /// plots (its lot and any it was merged over), None with no deck over
    /// any of them.
    pub fn deck_cap_plots(&self, plots: &[LotId]) -> Option<u8> {
        plots.iter().filter_map(|&l| self.deck_cap(l)).min()
    }

    /// [r4-decks] `floors` held to what fits under a deck over `lot`, as
    /// they are with none over it: every way a building goes up (growth,
    /// a rebuild, a car park, an annex) passes through this or
    /// `height_cap`, so nothing taller than the clearance stands under a
    /// raised road. At least one storey: ground too low for one is never a
    /// lot.
    pub fn deck_fit(&self, lot: LotId, floors: u8) -> u8 {
        match self.deck_cap(lot) {
            Some(cap) => floors.min(cap.max(1)),
            None => floors,
        }
    }

    /// [r4-decks] A lot under a raised road (a deck spans over it, or a
    /// column stands on it): no tower, headquarters or merged plot takes it.
    pub fn under_a_deck(&self, lot: LotId) -> bool {
        self.lots.get(lot).is_some_and(|l| (!self.pillars.is_empty() && self.pillars_on(&l.polygon)) || self.deck_cap_of(&l.polygon).is_some())
    }

    /// Lots the road drawn along `pts` at `levels` would take: those in its
    /// corridor where it is at grade or low, and under a deck high enough
    /// to span over them, those whose building is too tall for the
    /// headroom. Not in a tunnel. (What the Road tool outlines in red.)
    pub fn road_takes(&self, pts: &[Vec2], levels: &[f32], fwd: u8, bwd: u8, kind: RoadKind) -> Vec<LotId> {
        let half = deck_half_width(kind, fwd, bwd);
        let (lo, hi) = crate::geom::bounds(pts);
        let (lo, hi) = (lo - Vec2::splat(half + 40.0), hi + Vec2::splat(half + 40.0));
        self.lots
            .iter()
            .filter(|(_, l)| l.building.is_some() && l.center.x > lo.x && l.center.y > lo.y && l.center.x < hi.x && l.center.y < hi.y)
            .filter(|(_, l)| {
                pts.windows(2).enumerate().any(|(i, s)| {
                    let (la, lb) = (levels.get(i).copied().unwrap_or(0.0), levels.get(i + 1).copied().unwrap_or(0.0));
                    if la.max(lb) <= crate::roads::TUNNEL_LEVEL {
                        return false;
                    }
                    let (_, q) = crate::roads::project_on_segment(l.center, s[0], s[1]);
                    let under = l.contains(q) || crate::geom::distance_to_outline(q, &l.polygon) < half;
                    if !under {
                        return false;
                    }
                    if la.min(lb) >= pillars::SPAN_LEVEL {
                        let floors = l.building.and_then(|b| self.buildings.get(b)).map_or(0, |b| b.floors);
                        return floors > pillars::floors_under(la.min(lb));
                    }
                    true
                })
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// After a load: columns for every raised way from a save that has
    /// none (nothing is pulled down for them).
    pub(super) fn pillars_after_load(&mut self) {
        if !self.pillars.is_empty() {
            // Saves keyed by std's hasher: to the stable `way_key`.
            let (built, _) = self.master.built_roads_levels();
            let raised: Vec<_> = built.into_iter().filter(|(_, _, levels, ..)| pillars::is_raised(levels)).map(|(_, pts, levels, ..)| (pts, levels)).collect();
            pillars::migrate_way_keys(&mut self.pillars, raised.iter().map(|(p, l)| (p.as_slice(), l.as_slice())));
            return;
        }
        let ways = self.raised_ways();
        if !ways.is_empty() {
            self.place_pillars(&ways, false);
        }
    }

    /// Every raised way of the master plan: its key (`pillars::way_key` of
    /// the polyline and levels as drawn), polyline, levels and deck half
    /// width.
    pub(super) fn raised_ways(&self) -> Vec<(u64, Vec<Vec2>, Vec<f32>, f32)> {
        let (built, _) = self.master.built_roads_levels();
        built
            .into_iter()
            .filter(|(_, _, levels, ..)| pillars::is_raised(levels))
            .map(|(_, pts, levels, f, b, k)| {
                let key = pillars::way_key(&pts, &levels);
                // Built already: each end's node says where it landed.
                let (pts, levels) = self.roads.as_built(&pts, &levels, k);
                (key, pts, levels, deck_half_width(k, f, b))
            })
            .collect()
    }
}
