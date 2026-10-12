//! [w1-roads] Nothing is built over a road (flags H1, H3): a dock, a plant,
//! a service, an attraction, a station or yard, whatever the
//! Build window puts down is refused, in the preview and on the click,
//! when its footprint takes in a carriageway, built or planned in the
//! active project. Tunnels pass under and decks high enough to span lots
//! pass over; footpaths are left out (they are rerouted round grounds).

use super::*;
use crate::geom::{point_in_polygon, segment_distance_between};
use crate::pillars::SPAN_LEVEL;
use crate::roads::{half_width_spec, RoadKind, TUNNEL_LEVEL};

/// How far a carriageway may reach onto a footprint before it counts as
/// built over: the kerb and a sliver of the outer lane (a dock's front
/// edge or a lot's corner drawn close by).
const KERB_SLACK: f32 = 1.2;

/// What a footprint is checked as.
#[derive(Clone, Copy, Debug)]
pub enum Footprint<'a> {
    /// A closed outline (a lot, a dock, a station).
    Area(&'a [Vec2]),
}

impl Footprint<'_> {
    fn bounds(&self) -> (Vec2, Vec2) {
        let Footprint::Area(pts) = self;
        crate::geom::bounds(pts)
    }

    /// Does a way along `p0`-`p1`, `hw` wide each side, run onto it?
    fn takes(&self, p0: Vec2, p1: Vec2, hw: f32) -> bool {
        match *self {
            Footprint::Area(poly) => {
                if poly.len() < 3 {
                    return false;
                }
                let reach = (hw - KERB_SLACK).max(0.5);
                if point_in_polygon(p0, poly) || point_in_polygon(p1, poly) {
                    return true;
                }
                (0..poly.len()).any(|i| segment_distance_between(p0, p1, poly[i], poly[(i + 1) % poly.len()]) < reach)
            }
        }
    }
}

/// What a crossing way is, for the refusal.
fn way_word(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> &'static str {
    match kind {
        RoadKind::Rail => "railway",
        RoadKind::Path => "path",
        RoadKind::Street if crate::roads::is_highway_spec(kind, lanes_fwd, lanes_bwd) => "highway",
        RoadKind::Street => "road",
    }
}

impl World {
    /// [w1-roads] The first way whose carriageway runs onto `fp`, as the
    /// refusal for putting `what` there ("a road runs across the site at
    /// (x, y): ..."), or None. `rail`: a railway counts too (not for a
    /// station or yard, which stands on its track).
    pub fn road_under(&self, fp: Footprint, what: &str, rail: bool) -> Option<String> {
        let (lo, hi) = fp.bounds();
        let pad = Vec2::splat(20.0);
        for sid in self.roads.segments_in_box(lo - pad, hi + pad) {
            let s = self.roads.segment(sid);
            if s.kind == RoadKind::Path || (s.is_rail() && !rail) || s.tunnel || self.roads.spans_lots(sid) {
                continue;
            }
            let (p0, p1) = self.roads.segment_points(sid);
            if fp.takes(p0, p1, s.half_width()) {
                let at = crate::geom::project_on_segment((lo + hi) * 0.5, p0, p1).1;
                let way = if s.ring { "roundabout" } else { way_word(s.kind, s.lanes_fwd, s.lanes_bwd) };
                return Some(format!("a {way} runs across the site at ({:.0}, {:.0}): nothing may be built over a {way}; move the {what} off it", at.x, at.y));
            }
        }
        for (pts, kind, lanes, levels) in self.planned_ways() {
            if kind == RoadKind::Path || (kind == RoadKind::Rail && !rail) {
                continue;
            }
            let hw = half_width_spec(kind, lanes.0, lanes.1);
            for (i, w) in pts.windows(2).enumerate() {
                let (la, lb) = (levels.get(i).copied().unwrap_or(0.0), levels.get(i + 1).copied().unwrap_or(0.0));
                if (la <= TUNNEL_LEVEL && lb <= TUNNEL_LEVEL) || (la >= SPAN_LEVEL && lb >= SPAN_LEVEL) {
                    continue;
                }
                if fp.takes(w[0], w[1], hw) {
                    let way = way_word(kind, lanes.0, lanes.1);
                    let at = crate::geom::project_on_segment((lo + hi) * 0.5, w[0], w[1]).1;
                    return Some(format!("a {way} planned in this project runs across the site at ({:.0}, {:.0}): move the {what} or change the plan", at.x, at.y));
                }
            }
        }
        None
    }

    /// [w1-roads] The ways the active project adds or changes and has not
    /// built yet, as they will be built (a curved road along its sampled
    /// spline, a roundabout round its ring): points, kind, lanes (forward,
    /// back) and the level at each point.
    pub(crate) fn planned_ways(&self) -> Vec<(Vec<Vec2>, RoadKind, (u8, u8), Vec<f32>)> {
        if self.project.is_none() {
            return Vec::new();
        }
        self.effective_plan()
            .gestures
            .iter()
            .filter(|(id, g)| self.master.gestures.get(*id) != Some(*g))
            .filter_map(|(_, g)| match g {
                Gesture::Road { kind, lanes_fwd, lanes_bwd, .. } => {
                    let pts = g.road_polyline();
                    (pts.len() >= 2).then(|| (pts, *kind, (*lanes_fwd, *lanes_bwd), g.road_levels()))
                }
                Gesture::Roundabout { lanes, .. } => {
                    let mut pts = g.road_polyline();
                    if let Some(&first) = pts.first() {
                        pts.push(first);
                    }
                    let n = pts.len();
                    (n >= 2).then(|| (pts, RoadKind::Street, (*lanes, 0), vec![0.0; n]))
                }
                _ => None,
            })
            .collect()
    }
}
