//! [toll-inspect] Toll booths as things to inspect: which booth a click
//! picks and the gold outline round the selected one (the inspector card
//! is `ui::toll`, the remote API's read-out `remote::tolls`).

use bevy::prelude::*;
use nd_sim::tolls::{TollBooth, TollSize};
use nd_sim::{Vec2 as SVec2, World};

/// The booth's footprint: its centre, its direction along the road, and
/// its depth along and width across the road (a plaza's flare included).
pub fn footprint(w: &World, b: &TollBooth) -> (SVec2, SVec2, f32, f32) {
    let road = b.segment.filter(|s| (s.0 as usize) < w.roads.segments.len()).map_or(6.0, |s| w.roads.segment(s).half_width());
    let (depth, wide) = match b.size {
        TollSize::Booth => (9.0, road * 2.0 + 1.0),
        TollSize::Plaza => (b.size.flare() * 2.0, road * 2.0 + 16.0),
    };
    (b.pos, b.dir, depth, wide)
}

/// The booth a click at `p` lands on, if any.
pub fn pick(w: &World, p: SVec2) -> Option<u32> {
    w.tolls
        .booths
        .iter()
        .filter_map(|b| {
            let (c, dir, depth, wide) = footprint(w, b);
            let d = p - c;
            let along = d.dot(dir).abs();
            let across = d.dot(SVec2::new(-dir.y, dir.x)).abs();
            (along <= depth * 0.5 + 1.5 && across <= wide * 0.5 + 1.5).then(|| (d.length(), b.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// The selected booth outlined in gold like any selection, with the queue
/// waiting for it boxed in red.
pub fn draw(gizmos: &mut Gizmos, w: &World, id: u32) {
    let Some(b) = w.tolls.get(id) else { return };
    let gold = Color::linear_rgb(3.2, 2.4, 0.7);
    let (c, dir, depth, wide) = footprint(w, b);
    let y = w.surface_height(c);
    crate::tools::vehicle_outline(gizmos, c, dir.y.atan2(dir.x), depth + 1.0, wide + 1.0, y + 0.35, gold);
    // Ticks along the road where drivers start braking for it, both ways.
    let right = SVec2::new(-dir.y, dir.x);
    let lift = |p: SVec2| Vec3::new(p.x, w.surface_height(p) + 0.6, p.y);
    for side in [-1.0f32, 1.0] {
        let at = c + dir * side * nd_sim::tolls::APPROACH_M;
        gizmos.line(lift(at - right * wide * 0.5), lift(at + right * wide * 0.5), gold.with_alpha(0.45));
    }
}

/// What road the booth stands on, in words ("highway carriageway",
/// "two-way street, 2 + 2 lanes"), and the ways it charges: each
/// direction's compass heading with its lane count.
pub fn road(w: &World, b: &TollBooth) -> (String, Vec<(&'static str, u8)>) {
    let Some(s) = b.segment.and_then(|s| w.roads.try_segment(s)) else { return ("no road".into(), Vec::new()) };
    let kind = if s.is_highway() {
        "highway carriageway".to_string()
    } else if s.ramp {
        "slip road".to_string()
    } else if s.lanes_fwd == 0 || s.lanes_bwd == 0 {
        format!("one-way street, {} lane{}", s.lanes(), if s.lanes() == 1 { "" } else { "s" })
    } else {
        format!("two-way street, {} + {} lanes", s.lanes_fwd, s.lanes_bwd)
    };
    let kind = if s.viaduct { format!("{kind} on a viaduct") } else if s.bridge { format!("{kind} on a bridge") } else { kind };
    let mut ways = Vec::new();
    if s.lanes_fwd > 0 {
        ways.push((crate::ui::junction::compass(b.dir), s.lanes_fwd));
    }
    if s.lanes_bwd > 0 {
        ways.push((crate::ui::junction::compass(-b.dir), s.lanes_bwd));
    }
    (kind, ways)
}
