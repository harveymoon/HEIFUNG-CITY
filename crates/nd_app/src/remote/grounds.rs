//! [park-paths] `grounds_junctions`: the gates of parks and other public
//! grounds a path may be joined to (the Path tool's snap rings), and the
//! paths joined to them.

use super::view::round;
use nd_sim::{Vec2, World};
use serde_json::{json, Value};

/// One gate as the reply lists it; `index` is what `add_road`'s
/// `from_junction` / `to_junction` take.
pub fn junction_json(w: &World, index: usize) -> Value {
    let j = &w.grounds_junctions()[index];
    let link = w.path_links.links.iter().find(|l| l.pos.distance(j.pos) < 0.05);
    json!({
        "index": index,
        "pos": [round(j.pos.x), round(j.pos.y)],
        "kind": j.kind.key(),
        "label": j.kind.label(),
        "lot": j.lot.map(super::view::key_str),
        "attached": j.attached,
        "walk_node": j.node,
        "gesture_id": link.map(|l| l.gesture.0),
    })
}

/// The gates, optionally only those of `lot`, of a `kind`, or within
/// `radius` of `near`.
pub fn list(w: &World, lot: Option<nd_sim::LotId>, kind: Option<&str>, near: Option<Vec2>, radius: f32) -> Value {
    let rows: Vec<Value> = w
        .grounds_junctions()
        .iter()
        .enumerate()
        .filter(|(_, j)| lot.is_none_or(|l| j.lot == Some(l)))
        .filter(|(_, j)| kind.is_none_or(|k| j.kind.key() == k))
        .filter(|(_, j)| near.is_none_or(|p| j.pos.distance(p) <= radius))
        .map(|(i, _)| junction_json(w, i))
        .collect();
    json!({
        "count": rows.len(),
        "junctions": rows,
        "links": w.path_links.links.iter().map(|l| json!({ "gesture_id": l.gesture.0, "pos": [round(l.pos.x), round(l.pos.y)], "kind": l.kind.key() })).collect::<Vec<_>>(),
        "note": "a path drawn with an end on a gate (add_road kind path, an end within 1 m, or from_junction/to_junction) is joined to the grounds' walkways when implemented; when the grounds go the path is cut back to its last point, or removed",
    })
}
