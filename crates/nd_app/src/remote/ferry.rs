//! [ferry-routes] Ferry routes and ships for the remote API: a dock's
//! routes in `inspect {building}`, and `inspect {ship}`.

use nd_sim::ports::ShipId;
use nd_sim::world::ferry_routes::{LineEnd, LineKind};
use nd_sim::{BuildingId, World};
use serde_json::{json, Value};

use super::view::key_str;

fn hex(c: bevy_egui::egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn clock(tick: u64) -> Value {
    super::view::tick_time(tick)
}

/// A dock's routes for `inspect {building}` (null for anything else).
pub fn routes_json(w: &World, b: BuildingId) -> Value {
    if !crate::ferry_select::is_route_dock(w, b) {
        return Value::Null;
    }
    let (_, net) = crate::ferry_select::net(w);
    let rows: Vec<Value> = w
        .line_infos(&net, b)
        .into_iter()
        .map(|i| {
            let l = &net.lines[i.line];
            let end = match i.end {
                LineEnd::Terminal(t) => json!({ "kind": "terminal", "building": key_str(t) }),
                LineEnd::Town(k) => json!({ "kind": "town_off_map", "index": k, "edge": w.ferry_towns(l.from).get(k as usize).map(|t| t.edge.label()) }),
                LineEnd::Elsewhere => json!({ "kind": "open_sea" }),
            };
            json!({
                "kind": match i.kind { LineKind::Ferry => "ferry", LineKind::Freight => "freight" },
                "to": i.end_name, "end": end, "end_at": [i.end_at.x.round(), i.end_at.y.round()],
                "running": i.running, "colour": hex(crate::ferry_select::line_color32(l)),
                "ships": i.ships.iter().map(|s| json!({ "id": key_str(*s), "doing": crate::ferry_select::ship_doing(w, *s), "passengers": w.ports.ships.get(*s).map_or(0, |x| x.passengers) })).collect::<Vec<_>>(),
                "headway_min": i.headway, "next_departure": i.next_departure.map(clock),
                "passengers_today": i.passengers_today, "length_m": i.length.round(),
            })
        })
        .collect();
    Value::Array(rows)
}

/// `inspect {ship}`: what it is, what it is doing, its route.
pub fn ship_json(w: &World, id: ShipId) -> Option<Value> {
    let s = w.ports.ships.get(id)?;
    let (_, net) = crate::ferry_select::net(w);
    let line = net.of_ship(s).map(|i| {
        let l = &net.lines[i];
        json!({ "from": w.buildings.get(l.from).map(|b| b.name.clone()), "to": w.line_end_name(l.from, l.to), "colour": hex(crate::ferry_select::line_color32(l)) })
    });
    let p = s.pose().0;
    Some(json!({
        "id": key_str(id), "kind": format!("{:?}", s.kind).to_lowercase(), "title": crate::ferry_select::ship_title(w, id), "doing": crate::ferry_select::ship_doing(w, id),
        "at": [p.x.round(), p.y.round()], "passengers": s.passengers, "cargo": s.cargo.len(), "home": key_str(s.port),
        "route": line,
    }))
}
