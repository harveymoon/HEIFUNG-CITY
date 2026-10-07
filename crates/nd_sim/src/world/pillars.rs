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
    match kind {
        RoadKind::Street => {
            let (f, b) = if fwd == 0 && bwd == 0 { (1, 1) } else { (fwd, bwd) };
            let motorway = crate::roads::is_highway_spec(kind, f, b) || crate::roads::is_ramp_spec(kind, f, b);
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
    /// placed (plants, depots, stations, services, docks, hubs) and parks.
    fn pillar_protected(b: &Building) -> bool {
        Self::is_civic_structure(b) || b.land_use == LandUse::Park
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
        let r = PILLAR_RADIUS;
        let roads = &self.roads;
        for sid in roads.segments_in_box(p - Vec2::splat(40.0), p + Vec2::splat(40.0)) {
            let seg = roads.segment(sid);
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
                return Site::Blocked;
            }
            // A junction's corners are road too.
            for n in [seg.a, seg.b] {
                let node = roads.node(n);
                let arms = node.segments.iter().filter(|&&o| roads.segment(o).alive).count();
                if arms >= 3 && node.level < level - 2.0 {
                    let widest = node.segments.iter().filter(|&&o| roads.segment(o).alive).map(|&o| roads.segment(o).half_width()).fold(0.0, f32::max);
                    if node.pos.distance(p) < widest + crate::zoning::SIDEWALK + 1.0 + r {
                        return Site::Blocked;
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
                Some((bid, _)) => out = Site::Buyout(lid, bid),
                None if l.land_use == LandUse::Park => {}
                None if matches!(out, Site::Free) => out = Site::EmptyLot(lid),
                None => {}
            }
        }
        out
    }

    fn structure_footprints(&self) -> Vec<Vec<Vec2>> {
        self.facilities.values().map(|f| f.footprint()).chain(self.ports.ports.values().map(|p| p.footprint())).collect()
    }

    /// The columns a raised way along `pts` at `levels` would stand on,
    /// placed against the city as it is (the Road tool's preview and the
    /// project's check). `held`: columns already standing that carry it.
    pub fn plan_pillars(&self, pts: &[Vec2], levels: &[f32], half_width: f32, held: &[Vec2]) -> PillarPlan {
        if !pillars::is_raised(levels) {
            return PillarPlan::default();
        }
        let lots = self.lots_near_way(pts, half_width);
        let fps = self.structure_footprints();
        let site = |p: Vec2, level: f32| self.pillar_site(p, level, &lots, &fps);
        pillars::plan(pts, levels, half_width, pillars::way_key(pts, levels), &site, held)
    }

    /// The plan for a road as the tools draw it (lanes and kind as given).
    pub fn road_pillar_plan(&self, pts: &[Vec2], levels: &[f32], fwd: u8, bwd: u8, kind: RoadKind) -> PillarPlan {
        let held: Vec<Vec2> = self.pillars.iter().map(|p| p.at).collect();
        self.plan_pillars(pts, levels, deck_half_width(kind, fwd, bwd), &held)
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
        let mut gaps: Vec<(Vec2, Vec2)> = Vec::new();
        let fps = self.structure_footprints();
        for (key, pts, levels, hw) in ways {
            let key = *key;
            if !pillars::is_raised(levels) {
                continue;
            }
            let lots = self.lots_near_way(pts, *hw);
            let site = |p: Vec2, level: f32| {
                let s = self.pillar_site(p, level, &lots, &fps);
                // On load nothing is pulled down: a building is in the way.
                match s {
                    Site::Buyout(..) | Site::EmptyLot(_) if !buy => Site::Blocked,
                    s => s,
                }
            };
            let old: Vec<Pillar> = self.pillars.iter().filter(|p| p.way == key).copied().collect();
            if !old.is_empty() && old.iter().all(|p| matches!(site(p.at, p.level), Site::Free | Site::EmptyLot(_))) {
                out.extend(old);
                continue;
            }
            let held: Vec<Vec2> = out.iter().map(|p| p.at).collect();
            let plan = pillars::plan(pts, levels, *hw, key, &site, &held);
            for (_, b) in &plan.buyouts {
                taken.insert(*b);
            }
            gaps.extend(plan.gaps.iter().copied());
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

    /// `deck_cap_of` a lot.
    pub fn deck_cap(&self, lot: LotId) -> Option<u8> {
        self.lots.get(lot).and_then(|l| self.deck_cap_of(&l.polygon))
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
            .map(|(_, pts, levels, f, b, k)| (pillars::way_key(&pts, &levels), pts, levels, deck_half_width(k, f, b)))
            .collect()
    }
}
