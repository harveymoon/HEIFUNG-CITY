//! [train-inspect] A selected train for the remote API, as the inspector
//! shows it: what it is, what it is doing, its stops made and to come with
//! ETAs, and who or what is aboard (`inspect {train}` / `inspect {metro}`,
//! and `inspect` with a train selected).

use nd_sim::rail::{self, Train, TrainKind, TrainState};
use nd_sim::{BuildingId, MetroTrainId, TrainId, World};
use serde_json::{json, Value};

use super::view::{building_name, key_str, points, round, tick_time, v2};
use crate::train_select::{body_rgb, headcode, metro_code};
use crate::ui::train::{load_purpose, rider_trip};

fn state_json(w: &World, s: Option<TrainState>) -> Value {
    match s {
        Some(TrainState::Dwelling { at, left }) => json!({ "kind": "dwelling", "at": key_str(at), "at_name": building_name(w, Some(at)), "leaves_in_s": round(left.max(0.0)) }),
        Some(TrainState::HeldBlock { by }) => json!({ "kind": "held_signal", "text": "held at a signal: the block ahead is taken", "by": by.map(key_str), "by_headcode": by.and_then(|b| w.rail.trains.get(b).map(|t| headcode(b, t.kind))) }),
        Some(TrainState::HeldBranch) => json!({ "kind": "held_branch", "text": "waiting at the switch: another train is on the single-track branch" }),
        Some(TrainState::Running) => json!({ "kind": "running" }),
        Some(TrainState::Leaving) => json!({ "kind": "leaving", "text": "all calls made: running out to the end of the line" }),
        None => Value::Null,
    }
}

/// Stops made, the stop it stands at, the calls ahead with ETAs (sim
/// minutes and clock time), the end of the run, and the route geometry.
fn run_json(w: &World, t: &Train, dwell: f32) -> Value {
    let spt = w.config.traffic_seconds_per_tick;
    let alive = |b: BuildingId| w.buildings.contains_key(b);
    let clock = |m: f32| tick_time(w.clock.tick + m.round().max(0.0) as u64)["time"].clone();
    let standing = t.dwelling_at();
    let made: Vec<Value> = t.calls_made().iter().filter(|(b, _)| Some(*b) != standing).map(|(b, _)| json!({ "id": key_str(*b), "name": building_name(w, Some(*b)) })).collect();
    let ahead: Vec<Value> = t
        .calls_ahead(dwell, spt, alive)
        .iter()
        .map(|c| json!({ "id": key_str(c.stop), "name": building_name(w, Some(c.stop)), "eta_min": round(c.minutes), "eta_time": clock(c.minutes), "pos": v2(rail::point_along(&t.poly, c.s).0) }))
        .collect();
    let end = t.minutes_to_end(dwell, spt, alive);
    json!({
        "calls_made": made,
        "standing_at": standing.map(|b| json!({ "id": key_str(b), "name": building_name(w, Some(b)) })),
        "calls_ahead": ahead,
        "next_stop": ahead.first().cloned(),
        "origin": t.stops.first().map(|s| building_name(w, Some(s.0))), "terminus": t.stops.last().map(|s| building_name(w, Some(s.0))),
        "end_of_run": { "pos": t.poly.last().map(|p| v2(*p)), "eta_min": round(end), "eta_time": clock(end) },
        "route_m": round(t.total()), "run_m": round(t.s), "progress": round(t.s / t.total().max(1.0)),
        "route_ahead": points(&t.route_ahead(t.s)),
    })
}

fn riders_json(w: &World, riders: &[nd_sim::CitizenId], capacity: usize) -> Value {
    let list: Vec<Value> = riders
        .iter()
        .filter_map(|&p| {
            let c = w.citizens.get(p)?;
            let (alight, target, why) = rider_trip(c);
            Some(json!({ "id": key_str(p), "name": c.name, "alights": alight.map(key_str), "alights_name": building_name(w, alight), "going_to": target.map(key_str), "going_to_name": building_name(w, target), "why": why }))
        })
        .collect();
    json!({ "count": riders.len(), "capacity": capacity, "riders": list })
}

pub fn train_detail(w: &World, id: TrainId, palette: &[[f32; 3]]) -> Option<Value> {
    let t = w.rail.trains.get(id)?;
    let passenger = t.kind == TrainKind::Passenger;
    let mut v = json!({
        "id": key_str(id), "headcode": headcode(id, t.kind),
        "kind": if passenger { "passenger" } else { "freight" },
        "cars": t.cars, "car_name": if passenger { "coach" } else { "wagon" }, "length_m": round(t.length()),
        "colour_index": t.color, "colour_rgb": body_rgb(palette, t.color),
        "pos": v2(t.head()), "line_speed_kmh": round(t.speed * 3.6), "held": t.held, "on_branch": t.on_branch(),
        "state": state_json(w, w.rail.state(id)),
        "run": run_json(w, t, rail::DWELL_SECONDS),
    });
    if passenger {
        v["passengers"] = riders_json(w, &t.passengers, rail::TRAIN_CAPACITY);
    } else if let Some(m) = w.freight_manifest(id) {
        let loads: Vec<Value> = m
            .loads
            .iter()
            .map(|l| json!({
                "resource": l.resource.label(), "amount": round(l.amount), "price": round(l.price),
                "from": "out of town (by rail)", "yard": key_str(l.yard), "yard_name": building_name(w, Some(l.yard)),
                "hub": l.hub.map(key_str), "hub_name": building_name(w, l.hub),
                "buyer": key_str(l.buyer), "buyer_name": building_name(w, Some(l.buyer)), "purpose": load_purpose(w, l),
            }))
            .collect();
        let mut kinds: Vec<(&str, f32)> = Vec::new();
        for l in &m.loads {
            match kinds.iter_mut().find(|(r, _)| *r == l.resource.label()) {
                Some(k) => k.1 += l.amount,
                None => kinds.push((l.resource.label(), l.amount)),
            }
        }
        v["freight"] = json!({
            "import_units": round(m.import_units().max(0.0)), "unloaded": round(m.unloaded),
            "wagons": m.wagons, "capacity": round(m.capacity), "aboard": round(m.aboard),
            "by_kind": kinds.iter().map(|(r, a)| json!({ "resource": r, "amount": round(*a) })).collect::<Vec<_>>(),
            "loads": loads,
            "exports_aboard": round(m.exports_aboard.max(0.0)),
            "exports_to_collect": m.exports_to_collect.iter().map(|(y, a)| json!({ "yard": key_str(*y), "yard_name": building_name(w, Some(*y)), "amount": round(*a) })).collect::<Vec<_>>(),
        });
    }
    Some(v)
}

pub fn metro_detail(w: &World, id: MetroTrainId) -> Option<Value> {
    let t = w.subway.trains.get(id)?;
    let line = w.subway.lines.get(t.line);
    let state = if let Some(at) = t.run.dwelling_at() { TrainState::Dwelling { at, left: t.run.dwell } } else if t.run.next_stop >= t.run.stops.len() { TrainState::Leaving } else { TrainState::Running };
    Some(json!({
        "id": key_str(id), "headcode": metro_code(id), "kind": "metro",
        "line": key_str(t.line), "line_name": line.map(|l| l.name.clone()), "looped": line.is_some_and(|l| l.looped), "forward": t.forward,
        "cars": nd_sim::subway::CARS, "colour_index": t.run.color, "colour_rgb": nd_procgen::metro::line_colour(t.run.color),
        "pos": v2(t.head()), "line_speed_kmh": round(t.run.speed * 3.6),
        "state": state_json(w, Some(state)),
        "run": run_json(w, &t.run, nd_sim::subway::DWELL_SECONDS),
        "passengers": riders_json(w, &t.run.passengers, nd_sim::subway::CAPACITY),
    }))
}

/// A metro train as `list {kind: "metro_trains"}` gives it.
pub fn metro_row(w: &World, id: MetroTrainId) -> Value {
    let Some(t) = w.subway.trains.get(id) else { return Value::Null };
    json!({ "id": key_str(id), "headcode": metro_code(id), "line_name": w.subway.lines.get(t.line).map(|l| l.name.clone()), "pos": v2(t.head()), "passengers": t.run.passengers.len(), "dwelling": t.run.dwell > 0.0 })
}
